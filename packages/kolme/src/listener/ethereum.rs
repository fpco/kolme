use crate::utils::ethereum::{token_address_to_denom, DEFAULT_ETHEREUM_CONFIRMATION_DEPTH};
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
use std::collections::VecDeque;

use super::get_next_bridge_event_id;

// Ethereum block times are typically >= 4s on public networks (e.g. ~12s on mainnet),
// so polling every 4s significantly reduces RPC load with negligible freshness impact.
// Keep local polling at 1s for fast dev/test feedback loops.
const POLL_INTERVAL_LOCAL: tokio::time::Duration = tokio::time::Duration::from_secs(1);
const POLL_INTERVAL_DEFAULT: tokio::time::Duration = tokio::time::Duration::from_secs(4);
const WS_RETRY_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_secs(30);
const SUBSCRIBER_HINT_CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug)]
struct SubscriberHint {
    event_id: BridgeEventId,
    block_number: u64,
}

type SubscriberHintState = Arc<tokio::sync::RwLock<VecDeque<SubscriberHint>>>;

sol! {
    event FundsReceived(uint64 indexed eventId, address indexed sender, address[] tokens, uint256[] amounts, bytes[] keys);

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
            Self::FundsReceived(FundsReceived {
                sender,
                tokens,
                amounts,
                keys,
                ..
            }) => {
                anyhow::ensure!(
                    tokens.len() == amounts.len(),
                    "Ethereum FundsReceived malformed payload: tokens length {} != amounts length {}",
                    tokens.len(),
                    amounts.len()
                );
                let funds = tokens
                    .iter()
                    .zip(amounts.iter())
                    .map(|(token, amount)| {
                        Ok(BridgedAssetAmount {
                            denom: token_address_to_denom(*token),
                            amount: u256_to_u128(*amount)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;

                BridgeEvent::Regular {
                    wallet: Wallet::from_ethereum(*sender),
                    funds,
                    keys: keys
                        .iter()
                        .map(|key| PublicKey::from_bytes(key.as_ref()))
                        .collect::<Result<Vec<_>, _>>()?,
                }
            }
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
    ethereum_chain: EthereumChain,
    contract: String,
) -> Result<()> {
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
    let subscriber_hint: SubscriberHintState = Arc::new(tokio::sync::RwLock::new(VecDeque::new()));

    tracing::info!(
        "Beginning Ethereum listener loop on chain {chain:?}, contract {:#x}, next event ID: {next_bridge_event_id}, next block: {next_block}, first log index: {first_log_index:?}",
        contract.address()
    );

    listen_with_ws_retry(
        &kolme,
        &secret,
        ethereum_chain,
        &contract,
        &subscriber_hint,
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

#[allow(clippy::too_many_arguments)]
async fn listen_with_ws_retry<App: KolmeApp, P: Provider>(
    kolme: &Kolme<App>,
    secret: &SecretKey,
    ethereum_chain: EthereumChain,
    contract: &ContractInstance<P>,
    subscriber_hint: &SubscriberHintState,
    next_bridge_event_id: &mut BridgeEventId,
    next_block: &mut u64,
    first_log_index: &mut Option<u64>,
) -> Result<()> {
    let chain: ExternalChain = ethereum_chain.into();
    let contract_address = *contract.address();
    let poll_interval = match ethereum_chain {
        EthereumChain::Local => POLL_INTERVAL_LOCAL,
        EthereumChain::Mainnet | EthereumChain::Sepolia => POLL_INTERVAL_DEFAULT,
    };
    let mut next_ws_retry = tokio::time::Instant::now();
    let mut subscriber_task: Option<tokio::task::JoinHandle<()>> = None;

    loop {
        // Every WS_RETRY_INTERVAL seconds, check whether subscriber task is running.
        // If there is none, spawn one.
        if subscriber_task
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
        {
            if let Some(handle) = subscriber_task.take() {
                if let Err(e) = handle.await {
                    tracing::warn!(
                        "Ethereum listener subscriber task join failed on chain {chain:?}: {e}"
                    );
                }
            }
        }

        if tokio::time::Instant::now() >= next_ws_retry {
            next_ws_retry = tokio::time::Instant::now() + WS_RETRY_INTERVAL;
            if subscriber_task.is_none() {
                let kolme = kolme.clone();
                let secret = secret.clone();
                let subscriber_hint = subscriber_hint.clone();
                subscriber_task = Some(tokio::spawn(async move {
                    if let Err(e) = try_ws_session_once(
                        &kolme,
                        &secret,
                        ethereum_chain,
                        contract_address,
                        &subscriber_hint,
                    )
                    .await
                    {
                        tracing::debug!(
                            "Ethereum listener subscriber session ended on chain {chain:?}: {e}"
                        );
                    }
                }));
            }
        }

        listen_polling_once(
            kolme,
            secret,
            chain,
            contract,
            subscriber_hint,
            next_bridge_event_id,
            next_block,
            first_log_index,
        )
        .await?;
        tokio::time::sleep(poll_interval).await;
    }
}

async fn try_ws_session_once<App: KolmeApp>(
    kolme: &Kolme<App>,
    secret: &SecretKey,
    ethereum_chain: EthereumChain,
    contract_address: Address,
    subscriber_hint: &SubscriberHintState,
) -> Result<()> {
    let chain: ExternalChain = ethereum_chain.into();
    let ws_contract = match connect_ws(ethereum_chain, contract_address).await {
        Ok(ws_contract) => {
            tracing::info!(
                "Ethereum listener subscribing to logs on chain {chain:?}, contract {:#x}",
                ws_contract.address()
            );
            ws_contract
        }
        Err(e) => {
            tracing::warn!(
                "Ethereum listener could not establish WebSocket subscription on chain {chain:?}, contract {:#x}: {e}; continuing with polling and retrying WS in {}s",
                contract_address,
                WS_RETRY_INTERVAL.as_secs()
            );
            return Err(e);
        }
    };

    if let Err(e) =
        listen_with_subscription(kolme, secret, chain, &ws_contract, subscriber_hint).await
    {
        tracing::warn!(
            "Ethereum listener subscription failed on chain {chain:?}, contract {:#x}: {e}; continuing with polling and retrying WS in {}s",
            ws_contract.address(),
            WS_RETRY_INTERVAL.as_secs()
        );
        return Err(e);
    }

    tracing::warn!(
        "Ethereum listener subscription ended on chain {chain:?}, contract {:#x}; continuing with polling and retrying WS in {}s",
        ws_contract.address(),
        WS_RETRY_INTERVAL.as_secs()
    );
    anyhow::bail!(
        "Ethereum listener subscription ended on chain {chain:?}, contract {:#x}",
        ws_contract.address()
    );
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
    _kolme: &Kolme<App>,
    _secret: &SecretKey,
    chain: ExternalChain,
    contract: &ContractInstance<P>,
    subscriber_hint: &SubscriberHintState,
) -> Result<()> {
    let filter = Filter::new()
        .from_block(BlockNumberOrTag::Latest)
        .address(*contract.address())
        .event_signature(FundsReceived::SIGNATURE_HASH);
    let sub = contract.provider().subscribe_logs(&filter).await?;
    let mut stream = sub.into_stream();

    while let Some(log) = stream.next().await {
        if log.removed {
            continue;
        }
        if let Some(event) = EthereumBridgeEvent::from_log(&log)? {
            if let Some(block_number) = log.block_number {
                let mut hints = subscriber_hint.write().await;
                hints.push_back(SubscriberHint {
                    event_id: event.event_id(),
                    block_number,
                });
                while hints.len() > SUBSCRIBER_HINT_CAPACITY {
                    if let Some(evicted) = hints.pop_front() {
                        tracing::debug!(
                            "Ethereum subscriber hint queue full (capacity {}), evicting oldest hint: event_id={}, block_number={}",
                            SUBSCRIBER_HINT_CAPACITY,
                            evicted.event_id,
                            evicted.block_number
                        );
                    }
                }
            }
            tracing::debug!(
                "Ethereum subscription observed event {} on chain {chain:?} at block {:?}, log index {:?}",
                event.event_id(),
                log.block_number,
                log.log_index
            );
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn listen_polling_once<App: KolmeApp, P: Provider>(
    kolme: &Kolme<App>,
    secret: &SecretKey,
    chain: ExternalChain,
    contract: &ContractInstance<P>,
    subscriber_hint: &SubscriberHintState,
    next_bridge_event_id: &mut BridgeEventId,
    next_block: &mut u64,
    first_log_index: &mut Option<u64>,
) -> Result<()> {
    let confirmation_depth = kolme
        .read()
        .get_framework_state()
        .get_chain_states()
        .get(chain)
        .ok()
        .map(|state| match state.config.confirmation_depth {
            ConfirmationDepth::UseDefault => Some(DEFAULT_ETHEREUM_CONFIRMATION_DEPTH),
            ConfirmationDepth::Disabled => None,
            ConfirmationDepth::Value(depth) => Some(depth),
        })
        .unwrap_or(Some(DEFAULT_ETHEREUM_CONFIRMATION_DEPTH));

    let latest = contract.provider().get_block_number().await?;

    let Some(confirmed_head) =
        confirmation_depth.map_or(Some(latest), |depth| latest.checked_sub(depth))
    else {
        // edge case: total chain length is less than confirmation_depth
        return Ok(());
    };

    if *next_block > confirmed_head {
        return Ok(());
    }

    // Look if any hints were left by subscriber
    let hinted_upper_bound = {
        let mut hints = subscriber_hint.write().await;
        take_next_usable_hint_upper_bound(&mut hints, *next_bridge_event_id, confirmed_head)
    };
    let usable_hint = hinted_upper_bound.filter(|hint| *hint >= *next_block);
    let expected_event_id = *next_bridge_event_id;

    if let Some(hint_block) = usable_hint {
        // Try a direct probe on the hinted block first.
        let fast_track_first_log_index = if hint_block == *next_block {
            *first_log_index
        } else {
            None
        };
        let hinted_event_id = process_range(
            contract,
            kolme,
            secret,
            chain,
            hint_block,
            fast_track_first_log_index,
            expected_event_id,
            hint_block,
        )
        .await?;

        if hinted_event_id != expected_event_id {
            // Hint helped, the event was processed - lets do an early exit to get
            // back in a next iteration
            *next_bridge_event_id = hinted_event_id;
            *next_block = hint_block.saturating_add(1);
            *first_log_index = None;
            return Ok(());
        }
    }

    // Hint missing or probe did not help; poll normally up to confirmed head.
    *next_bridge_event_id = process_range(
        contract,
        kolme,
        secret,
        chain,
        *next_block,
        *first_log_index,
        expected_event_id,
        confirmed_head,
    )
    .await?;
    *next_block = confirmed_head.saturating_add(1);
    *first_log_index = None;
    Ok(())
}

fn take_next_usable_hint_upper_bound(
    hints: &mut VecDeque<SubscriberHint>,
    next_bridge_event_id: BridgeEventId,
    confirmed_head: u64,
) -> Option<u64> {
    while hints
        .front()
        .is_some_and(|hint| hint.event_id < next_bridge_event_id)
    {
        hints.pop_front();
    }

    match hints.front().copied() {
        Some(hint)
            if hint.event_id == next_bridge_event_id && hint.block_number <= confirmed_head =>
        {
            hints.pop_front().map(|hint| hint.block_number)
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
async fn process_range<App: KolmeApp, P: Provider>(
    contract: &ContractInstance<P>,
    kolme: &Kolme<App>,
    secret: &SecretKey,
    chain: ExternalChain,
    next_block: u64,
    first_log_index: Option<u64>,
    next_bridge_event_id: BridgeEventId,
    to_block: u64,
) -> Result<BridgeEventId> {
    let filter = Filter::new()
        .from_block(next_block)
        .to_block(to_block)
        .address(*contract.address());

    let mut next_bridge_event_id = next_bridge_event_id;
    for log in contract.provider().get_logs(&filter).await? {
        if should_skip_log(&log, next_block, first_log_index) {
            continue;
        }

        process_event(kolme, secret, chain, &log, &mut next_bridge_event_id).await?;
    }

    Ok(next_bridge_event_id)
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
    use std::collections::VecDeque;

    #[test]
    fn funds_received_topic_hash_matches_constant() {
        // To recalculate signature hash:
        // cast keccak "FundsReceived(uint64,address,address[],uint256[],bytes[])"
        // (in contracts/ethereum)
        const FUNDS_RECEIVED_EVENT_TOPIC0_HEX: &str =
            "0x02bb338ef0fcb89993f4087b28532775e53951c8025a81666d399e7263389a6c";
        let expected = FUNDS_RECEIVED_EVENT_TOPIC0_HEX.parse::<B256>().unwrap();
        assert_eq!(FundsReceived::SIGNATURE_HASH, expected);
    }

    #[test]
    fn decode_funds_received_log_fields() {
        use alloy::primitives::{Bytes, Log as PrimitiveLog, LogData};

        let sender = "0x1111111111111111111111111111111111111111"
            .parse::<Address>()
            .unwrap();
        let token_a = "0x2222222222222222222222222222222222222222"
            .parse::<Address>()
            .unwrap();
        let token_b = Address::ZERO;

        let event_id = 7u64;
        let mut event_id_topic = [0u8; 32];
        event_id_topic[24..].copy_from_slice(&event_id.to_be_bytes());

        let mut sender_topic = [0u8; 32];
        sender_topic[12..].copy_from_slice(sender.as_slice());
        let amounts = vec![U256::from(42u64), U256::from(7u64)];
        let tokens = vec![token_a, token_b];
        let keys = vec![Bytes::from(vec![0x02; 33])];
        let event = FundsReceived {
            eventId: event_id,
            sender,
            tokens: tokens.clone(),
            amounts: amounts.clone(),
            keys: keys.clone(),
        };

        let log_data = LogData::new(
            vec![
                FundsReceived::SIGNATURE_HASH,
                B256::from(event_id_topic),
                B256::from(sender_topic),
            ],
            Bytes::from(event.encode_data()),
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
        assert_eq!(decoded.tokens, tokens);
        assert_eq!(decoded.amounts, amounts);
        assert_eq!(decoded.keys, keys);
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
    fn to_kolme_message_maps_eth_and_erc20_denoms() {
        let event = EthereumBridgeEvent::FundsReceived(FundsReceived {
            eventId: 9,
            sender: "0x1111111111111111111111111111111111111111"
                .parse::<Address>()
                .unwrap(),
            tokens: vec![
                "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                    .parse::<Address>()
                    .unwrap(),
                Address::ZERO,
            ],
            amounts: vec![U256::from(5u64), U256::from(7u64)],
            keys: vec![],
        });

        let message = event
            .to_kolme_message::<()>(ExternalChain::EthereumLocal, BridgeEventId(9))
            .unwrap();
        let Message::Listener {
            event: BridgeEvent::Regular { wallet, funds, .. },
            ..
        } = message
        else {
            panic!("unexpected message kind");
        };

        assert_eq!(
            wallet,
            Wallet("0x1111111111111111111111111111111111111111".to_owned())
        );
        assert_eq!(funds.len(), 2);
        assert_eq!(funds[0].denom, "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(funds[0].amount, 5);
        assert_eq!(funds[1].denom, "eth");
        assert_eq!(funds[1].amount, 7);
    }

    #[test]
    fn to_kolme_message_rejects_mismatched_token_and_amount_lengths() {
        let event = EthereumBridgeEvent::FundsReceived(FundsReceived {
            eventId: 1,
            sender: "0x1111111111111111111111111111111111111111"
                .parse::<Address>()
                .unwrap(),
            tokens: vec![Address::ZERO],
            amounts: vec![],
            keys: vec![],
        });

        assert!(event
            .to_kolme_message::<()>(ExternalChain::EthereumLocal, BridgeEventId(1))
            .is_err());
    }

    #[test]
    fn u256_to_u128_rejects_overflow() {
        assert!(u256_to_u128(U256::from(u128::MAX) + U256::from(1)).is_err());
    }

    #[test]
    fn hint_queue_drops_stale_then_uses_matching_confirmed_hint() {
        let mut hints = VecDeque::from([
            SubscriberHint {
                event_id: BridgeEventId(1),
                block_number: 10,
            },
            SubscriberHint {
                event_id: BridgeEventId(2),
                block_number: 12,
            },
        ]);

        let upper_bound = take_next_usable_hint_upper_bound(&mut hints, BridgeEventId(2), 20);

        assert_eq!(upper_bound, Some(12));
        assert!(hints.is_empty());
    }

    #[test]
    fn hint_queue_keeps_front_when_event_id_is_ahead_of_next() {
        let mut hints = VecDeque::from([SubscriberHint {
            event_id: BridgeEventId(3),
            block_number: 8,
        }]);

        let upper_bound = take_next_usable_hint_upper_bound(&mut hints, BridgeEventId(2), 20);

        assert_eq!(upper_bound, None);
        assert_eq!(hints.len(), 1);
        assert_eq!(
            hints.front().map(|hint| hint.event_id),
            Some(BridgeEventId(3))
        );
    }

    #[test]
    fn hint_queue_keeps_front_when_block_is_above_confirmed_head() {
        let mut hints = VecDeque::from([SubscriberHint {
            event_id: BridgeEventId(2),
            block_number: 50,
        }]);

        let upper_bound = take_next_usable_hint_upper_bound(&mut hints, BridgeEventId(2), 20);

        assert_eq!(upper_bound, None);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints.front().map(|hint| hint.block_number), Some(50));
    }
}
