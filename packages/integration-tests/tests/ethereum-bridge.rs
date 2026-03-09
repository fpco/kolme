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
const TEST_ANVIL_ACCOUNT_0_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const TEST_ANVIL_ACCOUNT_0_PRIVATE_KEY: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

const TEST_ANVIL_RPC_URL: &str = "http://localhost:8545";
const TEST_PROCESSOR_KEY_HEX: &str =
    "038318535b54105d4a7aae60c08fc45f9687181b4fdfc625bd1a753fa7397fed75";
const TEST_APPROVER_KEY_HEX: &str =
    "02ba5734d8f7091719471e7f7ed6b9df170dc70cc661ca05e688601ad984f068b0";

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
    fn new(validator: PublicKey, bridge: BridgeContract) -> Self {
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
            .insert_ethereum(EthereumChain::Local, ChainConfig { assets, bridge })
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

    fn with_deployed_bridge(validator: PublicKey, bridge_contract: &str) -> Self {
        Self::new(
            validator,
            BridgeContract::Deployed(bridge_contract.to_string()),
        )
    }

    fn with_needed_bridge(validator: PublicKey) -> Self {
        Self::new(validator, BridgeContract::NeededEthereumBridge)
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
    assert_eq!(
        hex::encode(cfg.processor.as_ref()),
        TEST_PROCESSOR_KEY_HEX,
        "unexpected processor key in bridge config"
    );
    assert_eq!(
        cfg.listeners.len(),
        1,
        "unexpected number of listeners in bridge config"
    );
    assert_eq!(
        hex::encode(cfg.listeners[0].as_ref()),
        TEST_PROCESSOR_KEY_HEX,
        "unexpected listener key in bridge config"
    );
    assert_eq!(
        cfg.approvers.len(),
        1,
        "unexpected number of approvers in bridge config"
    );
    assert_eq!(
        hex::encode(cfg.approvers[0].as_ref()),
        TEST_APPROVER_KEY_HEX,
        "unexpected approver key in bridge config"
    );
    assert_eq!(
        cfg.neededListeners, 1,
        "unexpected listener quorum in bridge config"
    );
    assert_eq!(
        cfg.neededApprovers, 1,
        "unexpected approver quorum in bridge config"
    );
    assert_eq!(
        cfg.configNextEventId, 0,
        "unexpected initial value for configNextEventId"
    );
    assert_eq!(
        cfg.configNextActionId, 0,
        "unexpected initial value for configNextActionId"
    );
}

async fn ethereum_listener_ingests_local_deposit_inner(testtasks: TestTasks, (): ()) {
    let validator = SecretKey::random();
    let kolme = Kolme::new(
        EthereumBridgeTestApp::with_deployed_bridge(validator.public_key(), TEST_BRIDGE_ADDRESS),
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
        TEST_ANVIL_ACCOUNT_0_ADDRESS
            .parse::<Address>()
            .expect("hardcoded Anvil account 0 address is invalid")
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
        TEST_ANVIL_ACCOUNT_0_ADDRESS,
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
        "Ethereum deposit ingested by Kolme listener. sender={TEST_ANVIL_ACCOUNT_0_ADDRESS}, tx={tx_hash}, contract={TEST_BRIDGE_ADDRESS}, amount_wei={test_tx_amount}"
    );
}

#[tokio::test]
async fn ethereum_submitter_deploys_local_bridge() {
    init_logger(true, None);
    TestTasks::start(ethereum_submitter_deploys_local_bridge_inner, ()).await;
}

async fn ethereum_submitter_deploys_local_bridge_inner(testtasks: TestTasks, (): ()) {
    let validator = SecretKey::random();
    let signer = TEST_ANVIL_ACCOUNT_0_PRIVATE_KEY
        .parse()
        .expect("hardcoded Anvil account 0 private key is invalid");
    let kolme = Kolme::new(
        EthereumBridgeTestApp::with_needed_bridge(validator.public_key()),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();

    testtasks.spawn_persistent(Processor::new(kolme.clone(), validator.clone()).run());
    testtasks.try_spawn_persistent(
        Listener::new(kolme.clone(), validator.clone()).run(ChainName::Ethereum),
    );
    testtasks.try_spawn_persistent(Submitter::new_ethereum(kolme.clone(), signer).run());

    let deployed_contract = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(contract) = deployed_bridge_address(&kolme) {
                break contract;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("timed out waiting for Ethereum bridge deployment");

    let provider = anvil_provider().expect("failed to build anvil provider");
    let bridge: Address = deployed_contract
        .parse()
        .expect("deployed bridge address in kolme state is invalid");
    let contract = IBridgeIntegration::new(bridge, provider);
    let cfg = contract
        .get_config()
        .call()
        .await
        .expect("deployed bridge get_config call failed");

    assert_eq!(
        cfg.processor.as_ref(),
        validator.public_key().as_bytes().as_ref(),
        "unexpected processor key in deployed bridge config"
    );
    assert_eq!(cfg.listeners.len(), 1, "unexpected listener count");
    assert_eq!(
        cfg.listeners[0].as_ref(),
        validator.public_key().as_bytes().as_ref(),
        "unexpected listener key in deployed bridge config"
    );
    assert_eq!(cfg.approvers.len(), 1, "unexpected approver count");
    assert_eq!(
        cfg.approvers[0].as_ref(),
        validator.public_key().as_bytes().as_ref(),
        "unexpected approver key in deployed bridge config"
    );
    assert_eq!(cfg.neededListeners, 1, "unexpected listener quorum");
    assert_eq!(cfg.neededApprovers, 1, "unexpected approver quorum");
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

fn deployed_bridge_address(kolme: &Kolme<EthereumBridgeTestApp>) -> Option<String> {
    let kolme_r = kolme.read();
    let bridge = &kolme_r
        .get_bridge_contracts()
        .get(ExternalChain::EthereumLocal)
        .ok()?
        .config
        .bridge;
    match bridge {
        BridgeContract::Deployed(address) => Some(address.clone()),
        BridgeContract::NeededEthereumBridge => None,
        BridgeContract::NeededCosmosBridge { .. } | BridgeContract::NeededSolanaBridge { .. } => {
            None
        }
    }
}

async fn assert_anvil_identifiers_match(provider: &DynProvider) -> Result<()> {
    anyhow::ensure!(
        TEST_ANVIL_ACCOUNT_0_PRIVATE_KEY.starts_with("0x")
            && TEST_ANVIL_ACCOUNT_0_PRIVATE_KEY.len() == 66,
        "Invalid hardcoded Anvil account 0 private key format"
    );

    let expected_account: Address = TEST_ANVIL_ACCOUNT_0_ADDRESS.parse()?;
    let accounts = provider.get_accounts().await?;
    let has_expected_account = accounts
        .into_iter()
        .any(|account| account == expected_account);
    anyhow::ensure!(
        has_expected_account,
        "Expected Anvil account 0 {TEST_ANVIL_ACCOUNT_0_ADDRESS} is not available"
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
