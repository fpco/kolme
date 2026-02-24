use crate::*;
use alloy::{
    contract::{ContractInstance, Interface},
    json_abi::JsonAbi,
    primitives::{Address, U256},
    providers::Provider,
    rpc::types::eth::{Filter, Log},
    sol,
    sol_types::SolEvent,
};

use super::get_next_bridge_event_id;

const ETH_NATIVE_DENOM: &str = "eth";

sol! { event FundsReceived(address indexed sender, uint256 amount); }

enum EthereumBridgeEvent {
    FundsReceived(FundsReceived),
}

impl EthereumBridgeEvent {
    fn from_log(log: &Log) -> Result<Option<Self>> {
        let Some(topic) = log.topic0().copied() else {
            return Ok(None);
        };

        Ok(match topic {
            FundsReceived::SIGNATURE_HASH => Some(Self::FundsReceived(
                log.log_decode()
                    .map(|decoded| decoded.inner.data)
                    .map_err(|e| {
                        anyhow::anyhow!("Failed to decode FundsReceived(address,uint256): {e}")
                    })?,
            )),
            _ => None,
        })
    }

    fn to_kolme_message<AppMessage>(
        &self,
        chain: ExternalChain,
        event_id: BridgeEventId,
    ) -> Result<Message<AppMessage>> {
        let event = match self {
            Self::FundsReceived(FundsReceived { sender, amount }) => BridgeEvent::Regular {
                wallet: Wallet(format!("{:#x}", sender)),
                funds: vec![BridgedAssetAmount {
                    denom: ETH_NATIVE_DENOM.to_owned(),
                    amount: u256_to_u128(*amount)?,
                }],
                keys: vec![],
            },
        };

        Ok(Message::Listener {
            chain,
            event_id,
            event,
        })
    }
}

pub async fn listen<App: KolmeApp>(
    kolme: Kolme<App>,
    secret: SecretKey,
    chain: EthereumChain,
    contract: String,
) -> Result<()> {
    let ethereum_chain = chain;
    let chain: ExternalChain = ethereum_chain.into();
    let contract: Address = contract
        .parse()
        .with_context(|| format!("Invalid Ethereum contract address: {contract}"))?;
    let provider = kolme.get_ethereum_client(ethereum_chain).await?;
    let contract: ContractInstance<_> =
        ContractInstance::new(contract, provider, Interface::new(JsonAbi::default()));

    let mut next_bridge_event_id = {
        let kolme_r = kolme.read();
        get_next_bridge_event_id(&kolme_r, secret.public_key(), chain)
    };

    // Start from the next block to avoid replaying already-mined tip logs on listener restarts.
    let mut next_block = contract
        .provider()
        .get_block_number()
        .await?
        .saturating_add(1);

    tracing::info!(
        "Beginning Ethereum listener loop on chain {chain:?}, contract {:#x}, next event ID: {next_bridge_event_id}, next block: {next_block}",
        contract.address()
    );

    loop {
        listen_once(
            &kolme,
            &secret,
            chain,
            &contract,
            &mut next_bridge_event_id,
            &mut next_block,
        )
        .await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}

async fn listen_once<App: KolmeApp, P: Provider>(
    kolme: &Kolme<App>,
    secret: &SecretKey,
    chain: ExternalChain,
    contract: &ContractInstance<P>,
    next_bridge_event_id: &mut BridgeEventId,
    next_block: &mut u64,
) -> Result<()> {
    let latest = contract.provider().get_block_number().await?;
    if *next_block > latest {
        return Ok(());
    }

    let filter = Filter::new()
        .from_block(*next_block)
        .to_block(latest)
        .address(*contract.address());

    for log in contract.provider().get_logs(&filter).await? {
        if log.removed {
            continue;
        }

        process_event(kolme, secret, chain, &log, next_bridge_event_id).await?;
    }

    *next_block = latest.saturating_add(1);
    Ok(())
}

async fn process_event<App: KolmeApp>(
    kolme: &Kolme<App>,
    secret: &SecretKey,
    chain: ExternalChain,
    log: &Log,
    next_bridge_event_id: &mut BridgeEventId,
) -> Result<()> {
    let Some(event) = EthereumBridgeEvent::from_log(log)? else {
        return Ok(());
    };

    let message = event.to_kolme_message::<App::Message>(chain, *next_bridge_event_id)?;
    kolme
        .sign_propose_await_transaction(secret, vec![message])
        .await?;
    *next_bridge_event_id = next_bridge_event_id.next();
    Ok(())
}

// Ethereum uses uint256 as the standard amount type, in kolme we have u128,
// hence the explicit conversion function with a distinct error logging
fn u256_to_u128(value: U256) -> Result<u128> {
    if value > U256::from(u128::MAX) {
        anyhow::bail!("Ethereum value {value} does not fit into u128");
    }
    Ok(value.to())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::{primitives::B256, rpc::types::eth::Log};

    #[test]
    fn funds_received_topic_hash_matches_constant() {
        const FUNDS_RECEIVED_EVENT_TOPIC0_HEX: &str =
            "0x8e47b87b0ef542cdfa1659c551d88bad38aa7f452d2bbb349ab7530dfec8be8f";
        let expected = FUNDS_RECEIVED_EVENT_TOPIC0_HEX.parse::<B256>().unwrap();
        assert_eq!(FundsReceived::SIGNATURE_HASH, expected);
    }

    #[test]
    fn decode_funds_received_log_extracts_sender_and_amount() {
        use alloy::primitives::{Bytes, Log as PrimitiveLog, LogData};

        let sender = "0x1111111111111111111111111111111111111111"
            .parse::<Address>()
            .unwrap();

        let mut sender_topic = [0u8; 32];
        sender_topic[12..].copy_from_slice(sender.as_slice());

        let amount = U256::from(42u64);
        let mut data = [0u8; 32];
        amount.to_be_bytes::<32>().clone_into(&mut data);

        let log_data = LogData::new(
            vec![FundsReceived::SIGNATURE_HASH, B256::from(sender_topic)],
            Bytes::copy_from_slice(&data),
        )
        .unwrap();
        let log = Log {
            inner: PrimitiveLog {
                address: Address::ZERO,
                data: log_data,
            },
            ..Default::default()
        };

        let decoded = log.log_decode::<FundsReceived>().unwrap().inner.data;

        assert_eq!(decoded.sender, sender);
        assert_eq!(decoded.amount, amount);
    }

    #[test]
    fn decode_bridge_v1_event_ignores_unknown_topic() {
        use alloy::primitives::{Bytes, Log as PrimitiveLog, LogData};

        let log_data = LogData::new(
            vec![B256::from([7u8; 32])],
            Bytes::copy_from_slice(&[0u8; 32]),
        )
        .unwrap();
        let log = Log {
            inner: PrimitiveLog {
                address: Address::ZERO,
                data: log_data,
            },
            ..Default::default()
        };

        assert!(EthereumBridgeEvent::from_log(&log).unwrap().is_none());
    }

    #[test]
    fn u256_to_u128_rejects_overflow() {
        assert!(u256_to_u128(U256::from(u128::MAX) + U256::from(1)).is_err());
    }
}
