use std::{collections::BTreeMap, time::Duration};

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, U256},
    providers::{DynProvider, Provider, ProviderBuilder},
    rpc::types::eth::TransactionRequest,
    sol,
};
use anyhow::Result;
use kolme::*;
use rand::Rng;
use testtasks::TestTasks;

// for actual addresses/keys and ways to renew them, check contracts/ethereum/e2e/README.md
const TEST_BRIDGE_ADDRESS: &str = "0x5FbDB2315678afecb367f032d93F642f64180aa3";
const TEST_ADMIN_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const TEST_ADMIN_PRIVATE_KEY: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

const TEST_ANVIL_RPC_URL: &str = "http://localhost:8545";

sol! {
    #[sol(rpc)]
    interface IBridgeIntegration {
        function get_config()
            external
            view
            returns (
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

#[derive(Clone)]
struct EthereumBridgeTestApp {
    genesis: GenesisInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct EmptyState {}

impl MerkleSerialize for EmptyState {
    fn merkle_serialize(
        &self,
        _serializer: &mut MerkleSerializer,
    ) -> std::result::Result<(), MerkleSerialError> {
        Ok(())
    }
}

impl MerkleDeserialize for EmptyState {
    fn merkle_deserialize(
        _deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> std::result::Result<Self, MerkleSerialError> {
        Ok(Self {})
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
enum EmptyMessage {}

const DUMMY_CODE_VERSION: &str = "ethereum-listener-test-v1";
const ETH_ASSET_ID: AssetId = AssetId(1);
impl EthereumBridgeTestApp {
    fn new(validator: PublicKey, bridge_contract: &str) -> Self {
        let mut chains = ConfiguredChains::default();
        let mut assets = BTreeMap::new();
        assets.insert(
            AssetName("eth".to_owned()),
            AssetConfig {
                decimals: 18,
                asset_id: ETH_ASSET_ID,
            },
        );
        chains
            .insert_ethereum(
                EthereumChain::Local,
                ChainConfig {
                    assets,
                    bridge: BridgeContract::Deployed(bridge_contract.to_string()),
                },
            )
            .unwrap();

        Self {
            genesis: GenesisInfo {
                kolme_ident: "Ethereum local bridge test".to_owned(),
                validator_set: ValidatorSet {
                    processor: validator,
                    listeners: std::iter::once(validator).collect(),
                    needed_listeners: 1,
                    approvers: std::iter::once(validator).collect(),
                    needed_approvers: 1,
                },
                chains,
                version: DUMMY_CODE_VERSION.to_owned(),
            },
        }
    }
}

impl KolmeApp for EthereumBridgeTestApp {
    type State = EmptyState;
    type Message = EmptyMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
    }

    fn new_state(&self) -> anyhow::Result<Self::State> {
        Ok(EmptyState {})
    }

    async fn execute(
        &self,
        _ctx: &mut ExecutionContext<'_, Self>,
        _msg: &Self::Message,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn ethereum_listener_ingests_local_deposit() {
    init_logger(true, None);
    TestTasks::start(ethereum_listener_ingests_local_deposit_inner, ()).await;
}

#[tokio::test]
async fn ethereum_bridge_get_config_is_readable() {
    let provider = anvil_provider().expect("failed to build anvil provider");

    assert_anvil_identifiers_match(&provider)
        .await
        .expect("local anvil setup does not match expected deterministic identifiers");

    let bridge: Address = TEST_BRIDGE_ADDRESS
        .parse()
        .expect("hardcoded bridge address is invalid");
    let contract = IBridgeIntegration::new(bridge, provider.clone());
    let cfg = contract
        .get_config()
        .call()
        .await
        .expect("bridge get_config call failed");

    assert_eq!(cfg.processor.len(), 33, "invalid processor key length");
    assert!(
        !cfg.listeners.is_empty(),
        "bridge returned empty listeners in config"
    );
    assert!(
        !cfg.approvers.is_empty(),
        "bridge returned empty approvers in config"
    );
    assert!(
        cfg.neededListeners > 0,
        "listener quorum should be non-zero"
    );
    assert!(
        cfg.neededApprovers > 0,
        "approver quorum should be non-zero"
    );
    assert!(
        usize::from(cfg.neededListeners) <= cfg.listeners.len(),
        "listener quorum exceeds listeners length"
    );
    assert!(
        usize::from(cfg.neededApprovers) <= cfg.approvers.len(),
        "approver quorum exceeds approvers length"
    );
}

async fn ethereum_listener_ingests_local_deposit_inner(testtasks: TestTasks, (): ()) {
    let validator = SecretKey::random();
    let kolme = Kolme::new(
        EthereumBridgeTestApp::new(validator.public_key(), TEST_BRIDGE_ADDRESS),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    let test_tx_amount: u128 = {
        let mut rng = rand::thread_rng();
        rng.gen_range(20u128..100u128)
    };

    testtasks.spawn_persistent(Processor::new(kolme.clone(), validator.clone()).run());
    testtasks.try_spawn_persistent(
        Listener::new(kolme.clone(), validator.clone()).run(ChainName::Ethereum),
    );

    // Give the listener loop a short head start before submitting the deposit tx.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let provider = anvil_provider().expect("failed to build anvil provider");

    assert_anvil_identifiers_match(&provider)
        .await
        .expect("local anvil setup does not match expected deterministic identifiers");

    let expected_wallet = Wallet(format!(
        "{:#x}",
        TEST_ADMIN_ADDRESS
            .parse::<Address>()
            .expect("hardcoded admin address is invalid")
    ));
    let kolme_for_waiter = kolme.clone();
    let wallet_for_waiter = expected_wallet.clone();
    let waiter = tokio::spawn(async move {
        wait_for_expected_listener_message_in_new_blocks(
            &kolme_for_waiter,
            &wallet_for_waiter,
            test_tx_amount,
        )
        .await
    });
    tokio::task::yield_now().await;

    let tx_hash = send_eth(
        &provider,
        TEST_ADMIN_ADDRESS,
        TEST_BRIDGE_ADDRESS,
        test_tx_amount,
    )
    .await
    .expect("failed to send ETH to bridge contract");

    tokio::time::timeout(Duration::from_secs(10), waiter)
        .await
        .expect("timed out waiting for specific Ethereum listener message")
        .expect("listener message waiter task failed")
        .expect("failed while waiting for specific Ethereum listener message");

    tracing::info!(
        "Ethereum deposit ingested by Kolme listener. sender={TEST_ADMIN_ADDRESS}, tx={tx_hash}, contract={TEST_BRIDGE_ADDRESS}, amount_wei={test_tx_amount}"
    );
}

fn iter_block_messages(
    block: &SignedBlock<EmptyMessage>,
) -> std::slice::Iter<'_, Message<EmptyMessage>> {
    // dirty way to access block's messages. Is there a cleaner one?
    block.tx().0.message.as_inner().messages.iter()
}

async fn wait_for_expected_listener_message_in_new_blocks(
    kolme: &Kolme<EthereumBridgeTestApp>,
    expected_wallet: &Wallet,
    expected_amount_wei: u128,
) -> Result<()> {
    let target_chain = ExternalChain::EthereumLocal;
    let mut next_height = kolme.read().get_next_height();

    loop {
        let block = kolme.wait_for_block(next_height).await?;
        let found = iter_block_messages(&block)
            .filter(|message| {
                if let Message::Listener {
                    chain,
                    event_id: _,
                    event:
                        BridgeEvent::Regular {
                            wallet,
                            funds,
                            keys,
                        },
                } = message
                {
                    *chain == target_chain
                        && wallet == expected_wallet
                        && keys.is_empty()
                        && funds.len() == 1
                        && funds[0].denom == "eth"
                        && funds[0].amount == expected_amount_wei
                } else {
                    false
                }
            })
            .count();

        if found == 1 {
            return Ok(());
        }

        next_height = next_height.next();
    }
}

fn anvil_provider() -> Result<DynProvider> {
    let url = reqwest::Url::parse(TEST_ANVIL_RPC_URL)?;
    Ok(DynProvider::new(ProviderBuilder::new().connect_http(url)))
}

async fn assert_anvil_identifiers_match(provider: &DynProvider) -> Result<()> {
    anyhow::ensure!(
        TEST_ADMIN_PRIVATE_KEY.starts_with("0x") && TEST_ADMIN_PRIVATE_KEY.len() == 66,
        "Invalid hardcoded Anvil admin private key format"
    );

    let expected_admin: Address = TEST_ADMIN_ADDRESS.parse()?;
    let accounts = provider.get_accounts().await?;
    let has_expected_admin = accounts
        .into_iter()
        .any(|account| account == expected_admin);
    anyhow::ensure!(
        has_expected_admin,
        "Expected Anvil admin account {TEST_ADMIN_ADDRESS} is not available"
    );

    Ok(())
}

async fn send_eth(
    provider: &DynProvider,
    from: &str,
    to: &str,
    amount_wei: u128,
) -> Result<String> {
    let from: Address = from.parse()?;
    let to: Address = to.parse()?;
    let request = TransactionRequest::default()
        .with_from(from)
        .with_to(to)
        .with_value(U256::from(amount_wei));
    let pending = provider.send_transaction(request).await?;
    let tx_hash = *pending.tx_hash();
    let _receipt = pending.get_receipt().await?;
    Ok(format!("{tx_hash:#x}"))
}
