use std::{ops::Deref, str::FromStr};

use base64::Engine;
use borsh::BorshDeserialize;
use kolme_solana_bridge_client::{
    pubkey::Pubkey, BridgeMessage, Message as ContractMessage, State as BridgeState,
};
use libp2p::futures::StreamExt;
use solana_client::rpc_config::{RpcTransactionLogsConfig, RpcTransactionLogsFilter};

use super::*;

pub async fn listen<App: KolmeApp>(
    kolme: Kolme<App>,
    secret: SecretKey,
    chain: SolanaChain,
    contract: String,
) -> Result<()> {
    const PROGRAM_DATA_LOG: &str = "Program data: ";

    let client = chain.make_pubsub_client().await?;
    let mut next_bridge_event_id = kolme
        .read()
        .await
        .get_next_bridge_event_id(chain.into(), secret.public_key())
        .await?;

    let filter = RpcTransactionLogsFilter::Mentions(vec![contract.clone()]);
    let config = RpcTransactionLogsConfig {
        commitment: None, // Defaults to finalized
    };

    let (mut subscription, unsub) = client.logs_subscribe(filter, config).await?;

    tracing::info!(
        "Beginning listener loop on contract {contract}, next event ID: {next_bridge_event_id}"
    );

    // TODO: How do we handle disconnects and missed updates?
    'subscription_loop: while let Some(resp) = subscription.next().await {
        // We don't care about unsuccessful transactions.
        if resp.value.err.is_some() {
            continue;
        }

        let mut msg: Option<BridgeMessage> = None;

        // Our program data should always be the last "Program data:" entry even if CPI was invoked.
        for log in resp.value.logs.into_iter().rev() {
            if !log.starts_with(PROGRAM_DATA_LOG) {
                continue;
            }

            let data = &log.as_str()[PROGRAM_DATA_LOG.len()..];
            let bytes = base64::engine::general_purpose::STANDARD.decode(data)?;

            let result: BridgeMessage = BorshDeserialize::try_from_slice(&bytes).map_err(|x| {
                anyhow::anyhow!(
                    "Error deserializing Solana bridge message from logs: {:?}",
                    x
                )
            })?;

            if next_bridge_event_id.0 != result.id {
                tracing::warn!(
                    "Received bridge message with ID {} but expected ID {}. Ignoring...",
                    next_bridge_event_id.0,
                    result.id
                );

                continue 'subscription_loop;
            }

            msg = Some(result);

            break;
        }

        if let Some(msg) = msg {
            let msg = to_kolme_message::<App::Message>(msg, chain);

            let signed = kolme
                .read()
                .await
                .create_signed_transaction(&secret, vec![msg])
                .await?;

            kolme.propose_transaction(signed)?;

            next_bridge_event_id = next_bridge_event_id.next();
        } else {
            tracing::error!("No bridge message data log was found in {contract} logs.");
        }
    }

    (unsub)().await;

    Ok(())
}

pub async fn sanity_check_contract(
    client: &SolanaClient,
    program: &str,
    info: &GenesisInfo,
) -> Result<()> {
    let program_id = Pubkey::from_str(program)?;
    let state_acc = kolme_solana_bridge_client::derive_state_pda(&program_id);

    let acc = client.get_account(&state_acc).await?;

    if acc.owner != program_id || acc.data.is_empty() {
        return Err(anyhow::anyhow!(
            "Bridge program {program} hasn't been initialized yet."
        ));
    }

    let state: BridgeState = BorshDeserialize::try_from_slice(&acc.data)
        .map_err(|x| anyhow::anyhow!("Error deserializing Solana bridge state: {:?}", x))?;

    anyhow::ensure!(info.processor.as_bytes().deref() == state.processor.0.as_slice());
    anyhow::ensure!(info.approvers.len() == state.executors.len());

    for a in &state.executors {
        anyhow::ensure!(info
            .approvers
            .contains(&PublicKey::try_from_bytes(a.0.as_slice())?));
    }

    anyhow::ensure!(info.needed_approvers == usize::from(state.needed_executors));

    Ok(())
}

fn to_kolme_message<T>(msg: BridgeMessage, chain: SolanaChain) -> Message<T> {
    let event_id = BridgeEventId(msg.id);
    let wallet = Pubkey::new_from_array(msg.wallet).to_string();
    let event = match msg.ty {
        ContractMessage::Regular { funds, keys } => {
            let mut new_funds = Vec::with_capacity(funds.len());
            let mut new_keys = Vec::with_capacity(keys.len());

            for coin in funds {
                new_funds.push(BridgedAssetAmount {
                    denom: Pubkey::new_from_array(coin.mint).to_string(),
                    amount: coin.amount.into(),
                });
            }

            for key in keys {
                if let Ok(key) = PublicKey::try_from_bytes(key.0.as_slice()) {
                    new_keys.push(key);
                }
            }

            // TODO: Do we still need to emit if both funds and keys are empty?
            BridgeEvent::Regular {
                wallet,
                funds: new_funds,
                keys: new_keys,
            }
        }
        ContractMessage::Signed { action_id } => BridgeEvent::Signed {
            wallet,
            action_id: BridgeActionId(action_id.into()),
        },
    };

    Message::Listener {
        chain: chain.into(),
        event_id,
        event,
    }
}
