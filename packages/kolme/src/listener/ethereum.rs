use crate::*;
use alloy::{
    contract::{ContractInstance, Interface},
    json_abi::JsonAbi,
    primitives::{Address, U256},
    providers::Provider,
    rpc::types::eth::{BlockNumberOrTag, Filter, Log},
    sol,
    sol_types::SolEvent,
};

use super::get_next_bridge_event_id;

const ETH_NATIVE_DENOM: &str = "eth";

sol! { event FundsReceived(uint64 eventId, address indexed sender, uint256 amount); }

enum EthereumBridgeEvent {
    FundsReceived(FundsReceived),
}

impl EthereumBridgeEvent {
    fn event_id(&self) -> BridgeEventId {
        match self {
            Self::FundsReceived(FundsReceived { eventId, .. }) => BridgeEventId(*eventId),
        }
    }

    fn from_log(log: &Log) -> Result<Option<Self>> {
        let Some(topic) = log.topic0().copied() else {
            return Ok(None);
        };

        Ok(match topic {
            FundsReceived::SIGNATURE_HASH => Some(Self::FundsReceived(
                log.log_decode()
                    .map(|decoded| decoded.inner.data)
                    .map_err(|e| anyhow::anyhow!("Failed to decode FundsReceived: {e}"))?,
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
            Self::FundsReceived(FundsReceived { sender, amount, .. }) => BridgeEvent::Regular {
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

    let (mut next_block, mut first_log_index) =
        get_resume_cursor(&contract, next_bridge_event_id).await?;

    tracing::info!(
        "Beginning Ethereum listener loop on chain {chain:?}, contract {:#x}, next event ID: {next_bridge_event_id}, next block: {next_block}, first log index: {first_log_index:?}",
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
            &mut first_log_index,
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
    first_log_index: &mut Option<u64>,
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

        if let Some(min_log_index) = *first_log_index {
            if log.block_number == Some(*next_block)
                && log
                    .log_index
                    .is_some_and(|log_index| log_index < min_log_index)
            {
                continue;
            }
        }

        process_event(kolme, secret, chain, &log, next_bridge_event_id).await?;
    }

    *next_block = latest.saturating_add(1);
    *first_log_index = None;
    Ok(())
}

async fn get_resume_cursor<P: Provider>(
    contract: &ContractInstance<P>,
    next_bridge_event_id: BridgeEventId,
) -> Result<(u64, Option<u64>)> {
    // On listener connecting, we need to find a point of synchronization between
    // the sidechain and main blockchain (Ethereum).
    // The simplest way is to scan all the blocks starting from genesis - thats the way
    //   how it works now.
    // CAUTION: being this way it is not ready for mainnet!
    // A bit more optimal would be to scan since contract deployment.
    // But best option would be to store the last block on the sidechain.
    let latest = contract.provider().get_block_number().await?;

    if next_bridge_event_id == BridgeEventId::start() {
        return Ok((0, None));
    }

    let filter = Filter::new()
        .from_block(BlockNumberOrTag::Earliest)
        .to_block(latest)
        .address(*contract.address());

    for log in contract.provider().get_logs(&filter).await? {
        if log.removed {
            continue;
        }

        let Some(event) = EthereumBridgeEvent::from_log(&log)? else {
            continue;
        };

        if event.event_id() == next_bridge_event_id {
            return Ok((log.block_number.unwrap_or(0), log.log_index));
        }
    }

    Ok((latest.saturating_add(1), None))
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
    let actual_event_id = event.event_id();
    anyhow::ensure!(
        actual_event_id == *next_bridge_event_id,
        "Unexpected Ethereum bridge event ID. Expected {}, got {}",
        *next_bridge_event_id,
        actual_event_id
    );

    let message = event.to_kolme_message::<App::Message>(chain, actual_event_id)?;
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
            "0x4ade6a296f99f38f840a2a880cc9a27cec9ee56ce8d56cd80d3350e3419ed44c";
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

        let event_id = 7u64;
        let amount = U256::from(42u64);
        let mut data = [0u8; 64];
        data[24..32].copy_from_slice(&event_id.to_be_bytes());
        data[32..64].copy_from_slice(&amount.to_be_bytes::<32>());

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

        assert_eq!(decoded.eventId, event_id);
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
