use std::{ops::Deref, str::FromStr};

use base64::Engine;
use borsh::BorshDeserialize;
use kolme_solana_bridge_client::{
    pubkey::Pubkey, BridgeMessage, Message as ContractMessage, State as BridgeState,
};
use libp2p::futures::StreamExt;
use solana_client::rpc_config::{RpcTransactionLogsConfig, RpcTransactionLogsFilter};
use solana_commitment_config::{CommitmentConfig, CommitmentLevel};
use solana_signature::Signature;
use solana_transaction_status_client_types::{
    option_serializer::OptionSerializer, UiTransactionEncoding,
};

use super::*;

pub async fn listen<App: KolmeApp>(
    kolme: Kolme<App>,
    secret: SecretKey,
    chain: SolanaChain,
    contract: String,
) -> Result<()> {
    let contract_pubkey = Pubkey::from_str_const(&contract);
    let pubsub_client = chain.make_pubsub_client().await?;
    let client = chain.make_client();

    let mut next_bridge_event_id =
        get_next_bridge_event_id(&kolme.read(), secret.public_key(), chain.into());

    loop {
        let filter = RpcTransactionLogsFilter::Mentions(vec![contract.clone()]);
        let config = RpcTransactionLogsConfig {
            commitment: Some(CommitmentConfig {
                commitment: CommitmentLevel::Finalized,
            }),
        };

        // Subscribe now in order to ensure we don't miss any transactions while catching up.
        let (mut subscription, unsub) = pubsub_client.logs_subscribe(filter, config).await?;

        let last_seen = next_bridge_event_id
            .prev()
            .unwrap_or(BridgeEventId::start());

        if let Some(latest_id) =
            catch_up(&kolme, &client, &secret, last_seen, chain, &contract_pubkey).await?
        {
            next_bridge_event_id = latest_id.next();
        }

        tracing::info!(
            "Beginning listener loop on contract {contract}, next event ID: {next_bridge_event_id}"
        );

        while let Some(resp) = subscription.next().await {
            // We don't care about unsuccessful transactions.
            if resp.value.err.is_some() {
                continue;
            }

            let Some(msg) = extract_bridge_message_from_logs(&resp.value.logs)? else {
                tracing::warn!(
                    "No bridge message data log was found in TX {} logs.",
                    resp.value.signature
                );

                continue;
            };

            if next_bridge_event_id.0 > msg.id {
                tracing::warn!(
                    "Received bridge message in TX {} that was already processed. Message ID: {}, next expected ID {}. Ignoring...",
                    resp.value.signature,
                    msg.id,
                    next_bridge_event_id.0,
                );
            } else if next_bridge_event_id.0 != msg.id {
                let last_seen = next_bridge_event_id
                    .prev()
                    .unwrap_or(BridgeEventId::start());
                let latest_id =
                    catch_up(&kolme, &client, &secret, last_seen, chain, &contract_pubkey).await?;

                next_bridge_event_id = latest_id
                    .expect("should have at least one TX processed.")
                    .next();
            } else {
                let msg = to_kolme_message::<App::Message>(msg, chain);

                kolme
                    .sign_propose_await_transaction(&secret, vec![msg])
                    .await?;

                next_bridge_event_id = next_bridge_event_id.next();
            }
        }

        (unsub)().await;
    }
}

pub async fn sanity_check_contract(
    client: &SolanaClient,
    program: &str,
    info: &GenesisInfo,
) -> Result<()> {
    let program_id = Pubkey::from_str(program)?;
    let state_acc = kolme_solana_bridge_client::derive_state_pda(&program_id);

    let acc = client.get_account(&state_acc).await?;

    if acc.owner != program_id || acc.data.len() < 2 {
        return Err(anyhow::anyhow!(
            "Bridge program {program} hasn't been initialized yet."
        ));
    }

    // Skip the first two bytes which are the discriminator byte and the bump seed respectively.
    let state: BridgeState = BorshDeserialize::try_from_slice(&acc.data[2..])
        .map_err(|x| anyhow::anyhow!("Error deserializing Solana bridge state: {:?}", x))?;

    anyhow::ensure!(
        info.validator_set.processor.as_bytes().deref() == state.processor.0.as_slice()
    );
    anyhow::ensure!(info.validator_set.approvers.len() == state.executors.len());

    for a in &state.executors {
        anyhow::ensure!(info
            .validator_set
            .approvers
            .contains(&PublicKey::try_from_bytes(a.0.as_slice())?));
    }

    anyhow::ensure!(info.validator_set.needed_approvers == u16::from(state.needed_executors));

    Ok(())
}

async fn catch_up<App: KolmeApp>(
    kolme: &Kolme<App>,
    client: &SolanaClient,
    secret: &SecretKey,
    last_seen: BridgeEventId,
    chain: SolanaChain,
    contract: &Pubkey,
) -> Result<Option<BridgeEventId>> {
    tracing::info!("Catching up on missing bridge events until {}.", last_seen);

    let mut messages = vec![];
    let txs = client.get_signatures_for_address(contract).await?;

    // First entry is the latest transaction, we want to work up until to the target ID provided.
    for tx in txs {
        // We don't care about unsuccessful transactions.
        if tx.err.is_some() {
            continue;
        }

        let sig = Signature::from_str(&tx.signature)?;
        let tx = client
            .get_transaction(&sig, UiTransactionEncoding::Binary)
            .await?
            .transaction;

        let Some(meta) = tx.meta else {
            tracing::warn!("No transaction metadata was found for {}.", sig);

            continue;
        };

        let OptionSerializer::Some(logs) = meta.log_messages else {
            tracing::warn!("No transaction logs were found for {}.", sig);

            continue;
        };

        let Some(msg) = extract_bridge_message_from_logs(&logs)? else {
            tracing::warn!("No bridge message data log was found in TX {} logs.", sig);

            continue;
        };

        if msg.id <= last_seen.0 {
            break;
        }

        messages.push(msg);
    }

    assert!(messages.is_sorted_by(|a, b| a.id > b.id));
    let Some(latest_id) = messages.first().map(|x| x.id) else {
        return Ok(None);
    };

    tracing::info!(
        "Found {} missed bridge events. Proposing Kolme transaction...",
        messages.len()
    );

    // Now process in reverse insertion order - from oldest to newest.
    let kolme_messages: Vec<Message<App::Message>> = messages
        .into_iter()
        .rev()
        .map(|x| to_kolme_message(x, chain))
        .collect();

    kolme
        .sign_propose_await_transaction(secret, kolme_messages)
        .await?;

    Ok(Some(BridgeEventId(latest_id)))
}

fn extract_bridge_message_from_logs(logs: &[String]) -> Result<Option<BridgeMessage>> {
    const PROGRAM_DATA_LOG: &str = "Program data: ";

    // Our program data should always be the last "Program data:" entry even if CPI was invoked.
    for log in logs.iter().rev() {
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

        return Ok(Some(result));
    }

    Ok(None)
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
                wallet: Wallet(wallet),
                funds: new_funds,
                keys: new_keys,
            }
        }
        ContractMessage::Signed { action_id } => BridgeEvent::Signed {
            wallet: Wallet(wallet),
            action_id: BridgeActionId(action_id),
        },
    };

    Message::Listener {
        chain: chain.into(),
        event_id,
        event,
    }
}
