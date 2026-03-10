use crate::*;
use alloy::{
    contract::{ContractInstance, Interface},
    json_abi::JsonAbi,
    primitives::{Address, B256, U256},
    providers::{DynProvider, Provider, ProviderBuilder, WsConnect},
    rpc::types::eth::{BlockNumberOrTag, Filter, Log},
    sol,
    sol_types::SolEvent,
};
use futures_util::StreamExt;

use super::get_next_bridge_event_id;

const ETH_NATIVE_DENOM: &str = "eth";
const POLL_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_secs(1);

sol! {
    event FundsReceived(uint64 indexed eventId, address indexed sender, uint256 amount);

    #[sol(rpc)]
    contract Bridge {
        function get_config() external view returns (
            bytes processor,
            bytes[] listeners,
            uint16 neededListeners,
            bytes[] approvers,
            uint16 neededApprovers,
            uint64 configNextEventId,
            uint64 configNextActionId
        );
    }
}

enum EthereumBridgeEvent {
    FundsReceived(FundsReceived),
}

impl EthereumBridgeEvent {
    fn event_id(&self) -> BridgeEventId {
        match self {
            Self::FundsReceived(FundsReceived {
                eventId: event_id, ..
            }) => BridgeEventId(*event_id),
        }
    }

    fn from_log(log: &Log) -> Result<Option<Self>> {
        let Some(topic) = log.topic0().copied() else {
            // having `topic0` as None would be unusual given our filters
            tracing::debug!(
                "Ignoring Ethereum log without topic0 at address {:#x}",
                log.address()
            );
            return Ok(None);
        };

        Ok(match topic {
            FundsReceived::SIGNATURE_HASH => Some(Self::FundsReceived(
                log.log_decode()
                    .map(|decoded| decoded.inner.data)
                    .map_err(|e| anyhow::anyhow!("Failed to decode FundsReceived: {e}"))?,
            )),
            _ => {
                tracing::debug!(
                    "Ignoring Ethereum log with unsupported topic {topic:#x} at address {:#x}",
                    log.address()
                );
                None
            }
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

    match connect_ws(ethereum_chain, *contract.address()).await {
        Ok(ws_contract) => {
            tracing::info!(
                "Ethereum listener subscribing to logs on chain {chain:?}, contract {:#x}",
                ws_contract.address()
            );

            listen_with_subscription(
                &kolme,
                &secret,
                chain,
                &ws_contract,
                &mut next_bridge_event_id,
                &mut next_block,
                &mut first_log_index,
            )
            .await?;

            tracing::warn!(
                "Ethereum listener subscription ended on chain {chain:?}, contract {:#x}; falling back to polling",
                ws_contract.address()
            );
        }
        Err(e) => {
            tracing::warn!(
                "Ethereum listener could not establish WebSocket subscription on chain {chain:?}, contract {:#x}: {e}; falling back to polling",
                contract.address()
            );
        }
    }

    listen_with_polling(
        &kolme,
        &secret,
        chain,
        &contract,
        &mut next_bridge_event_id,
        &mut next_block,
        &mut first_log_index,
    )
    .await
}

async fn connect_ws(
    chain: EthereumChain,
    contract: Address,
) -> Result<ContractInstance<DynProvider>> {
    let ws_url = chain.parse_default_ws_url()?;
    let provider = ProviderBuilder::new()
        .connect_ws(WsConnect::new(ws_url.as_str()))
        .await?;

    Ok(ContractInstance::new(
        contract,
        DynProvider::new(provider),
        Interface::new(JsonAbi::default()),
    ))
}

async fn listen_with_polling<App: KolmeApp, P: Provider>(
    kolme: &Kolme<App>,
    secret: &SecretKey,
    chain: ExternalChain,
    contract: &ContractInstance<P>,
    next_bridge_event_id: &mut BridgeEventId,
    next_block: &mut u64,
    first_log_index: &mut Option<u64>,
) -> Result<()> {
    loop {
        listen_polling_once(
            kolme,
            secret,
            chain,
            contract,
            next_bridge_event_id,
            next_block,
            first_log_index,
        )
        .await?;
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

pub async fn sanity_check_contract(
    provider: &DynProvider,
    contract: &str,
    info: &GenesisInfo,
) -> Result<()> {
    let address: Address = contract
        .parse()
        .with_context(|| format!("Invalid Ethereum contract address: {contract}"))?;
    let code = provider.get_code_at(address).await?;
    anyhow::ensure!(
        !code.is_empty(),
        "Ethereum contract {contract} has no bytecode deployed"
    );

    let bridge = Bridge::new(address, provider.clone());
    let Bridge::get_configReturn {
        processor,
        listeners,
        neededListeners: needed_listeners,
        approvers,
        neededApprovers: needed_approvers,
        configNextEventId: _,
        configNextActionId: _,
    } = bridge.get_config().call().await?;

    anyhow::ensure!(
        PublicKey::from_bytes(&processor)? == info.validator_set.processor,
        "Ethereum processor key mismatch"
    );
    anyhow::ensure!(
        decode_validator_keys(&listeners)? == info.validator_set.listeners,
        "Ethereum listener set mismatch"
    );
    anyhow::ensure!(
        needed_listeners == info.validator_set.needed_listeners,
        "Ethereum needed listener quorum mismatch"
    );
    anyhow::ensure!(
        decode_validator_keys(&approvers)? == info.validator_set.approvers,
        "Ethereum approver set mismatch"
    );
    anyhow::ensure!(
        needed_approvers == info.validator_set.needed_approvers,
        "Ethereum needed approver quorum mismatch"
    );

    Ok(())
}

async fn listen_with_subscription<App: KolmeApp, P: Provider>(
    kolme: &Kolme<App>,
    secret: &SecretKey,
    chain: ExternalChain,
    contract: &ContractInstance<P>,
    next_bridge_event_id: &mut BridgeEventId,
    next_block: &mut u64,
    first_log_index: &mut Option<u64>,
) -> Result<()> {
    let filter = Filter::new()
        .from_block(*next_block)
        .address(*contract.address())
        .event_signature(FundsReceived::SIGNATURE_HASH);
    let sub = contract.provider().subscribe_logs(&filter).await?;
    let mut stream = sub.into_stream();

    while let Some(log) = stream.next().await {
        if should_skip_log(&log, *next_block, *first_log_index) {
            tracing::debug!("Skipping Ethereum subscription log: {:?}", log);
            continue;
        }

        process_event(kolme, secret, chain, &log, next_bridge_event_id).await?;

        if let Some(block_number) = log.block_number {
            *next_block = block_number;
        }
        *first_log_index = log.log_index.map(|i| i.saturating_add(1));
    }

    Ok(())
}

async fn listen_polling_once<App: KolmeApp, P: Provider>(
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
        if should_skip_log(&log, *next_block, *first_log_index) {
            continue;
        }

        process_event(kolme, secret, chain, &log, next_bridge_event_id).await?;
    }

    *next_block = latest.saturating_add(1);
    *first_log_index = None;
    Ok(())
}

fn should_skip_log(log: &Log, next_block: u64, first_log_index: Option<u64>) -> bool {
    if log.removed {
        return true;
    }

    if let Some(min_log_index) = first_log_index {
        if log.block_number == Some(next_block)
            && log
                .log_index
                .is_some_and(|log_index| log_index < min_log_index)
        {
            return true;
        }
    }

    false
}

async fn get_resume_cursor<P: Provider>(
    contract: &ContractInstance<P>,
    next_bridge_event_id: BridgeEventId,
) -> Result<(u64, Option<u64>)> {
    // On listener startup, we find sync point by scanning from genesis while
    // filtering by contract address and indexed event ID.
    // TODO: Switch from "since genesis" to "since contract deployment block".
    let latest = contract.provider().get_block_number().await?;

    if next_bridge_event_id == BridgeEventId::start() {
        return Ok((0, None));
    }

    let filter = Filter::new()
        .from_block(BlockNumberOrTag::Earliest)
        .to_block(latest)
        .address(*contract.address())
        .event_signature(FundsReceived::SIGNATURE_HASH)
        .topic1(event_id_topic(next_bridge_event_id));

    for log in contract.provider().get_logs(&filter).await? {
        if log.removed {
            continue;
        }

        let Some(event) = EthereumBridgeEvent::from_log(&log)? else {
            continue;
        };

        anyhow::ensure!(
            event.event_id() == next_bridge_event_id,
            "Ethereum filtered resume query returned mismatched event ID. Expected {}, got {}",
            next_bridge_event_id,
            event.event_id()
        );
        return Ok((log.block_number.unwrap_or(0), log.log_index));
    }

    tracing::warn!(
        "Ethereum resume scan did not find expected event ID {} on contract {:#x}; scanned blocks 0..={} and will resume from latest+1",
        next_bridge_event_id,
        contract.address(),
        latest
    );
    Ok((latest.saturating_add(1), None))
}

/// Converts kolme's event id (u64) into 32-byte Ethereum topic value for log filtering
fn event_id_topic(event_id: BridgeEventId) -> B256 {
    let BridgeEventId(event_id) = event_id;
    let mut bytes = [0u8; 32];
    bytes[24..].copy_from_slice(&event_id.to_be_bytes());
    B256::from(bytes)
}

/// Converts `bytes[]` returned by contract's get_config() into something we can compare
fn decode_validator_keys(keys: &[alloy::primitives::Bytes]) -> Result<BTreeSet<PublicKey>> {
    keys.iter()
        .map(|key| PublicKey::from_bytes(key.as_ref()).map_err(anyhow::Error::from))
        .collect()
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
    fn decode_funds_received_log_fields() {
        use alloy::primitives::{Bytes, Log as PrimitiveLog, LogData};

        let sender = "0x1111111111111111111111111111111111111111"
            .parse::<Address>()
            .unwrap();

        let event_id = 7u64;
        let mut event_id_topic = [0u8; 32];
        event_id_topic[24..].copy_from_slice(&event_id.to_be_bytes());

        let mut sender_topic = [0u8; 32];
        sender_topic[12..].copy_from_slice(sender.as_slice());

        let amount = U256::from(42u64);
        let mut data = [0u8; 32];
        data[0..32].copy_from_slice(&amount.to_be_bytes::<32>());

        let log_data = LogData::new(
            vec![
                FundsReceived::SIGNATURE_HASH,
                B256::from(event_id_topic),
                B256::from(sender_topic),
            ],
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
