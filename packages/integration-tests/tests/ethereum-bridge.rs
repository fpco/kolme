use std::{collections::BTreeMap, fs, path::PathBuf, time::Duration};

use alloy::{
    network::TransactionBuilder,
    primitives::{Address, Bytes, U256},
    providers::{DynProvider, Provider, ProviderBuilder},
    rpc::types::eth::TransactionRequest,
    sol,
    sol_types::{SolCall, SolConstructor},
};
use anyhow::{Context, Result};
use kolme::*;
use rand::Rng;
use serde::Deserialize;
use testtasks::TestTasks;

// Important note: if same account is used for multiple tests simultaneously in
// multiple threads, "nonce too low" error may appear. Either --test-threads=1 must be
// used, or different account for different concurrent tests
const TEST_ANVIL_ACCOUNT_0_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const TEST_ANVIL_ACCOUNT_0_PRIVATE_KEY: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const TEST_ANVIL_ACCOUNT_2_ADDRESS: &str = "0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC";

const TEST_ANVIL_RPC_URL: &str = "http://localhost:8545";
const TEST_ERC20_MINTABLE_ARTIFACT_PATH: &str =
    "../../contracts/ethereum/out/TestERC20Mintable.sol/TestERC20Mintable.json";

sol! {
    #[sol(rpc)]
    interface IBridgeIntegration {
        function regular(address[] tokens, uint256[] amounts, bytes[] keys) external payable;
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

    contract TestERC20Mintable {
        constructor(string name_, string symbol_);
    }

    #[sol(rpc)]
    interface ITestERC20Mintable {
        function mint(address to, uint256 amount) external;
        function approve(address spender, uint256 amount) external returns (bool);
        function balanceOf(address account) external view returns (uint256);
    }
}

struct RegularCall {
    tokens: Vec<Address>,
    amounts_wei: Vec<u128>,
    keys: Vec<Bytes>,
    msg_value_wei: u128,
}

#[derive(Deserialize)]
struct FoundryArtifact {
    bytecode: ArtifactBytecode,
}

#[derive(Deserialize)]
struct ArtifactBytecode {
    object: String,
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
enum EmptyMessage {
    Mint { amount: Decimal },
    Withdraw { recipient: String, amount: Decimal },
}

const DUMMY_CODE_VERSION: &str = "ethereum-listener-test-v1";
const ETH_ASSET_ID: AssetId = AssetId(1);
const ERC20_ASSET_ID: AssetId = AssetId(2);
impl EthereumBridgeTestApp {
    fn new(validator: PublicKey, bridge: BridgeContract) -> Self {
        Self::new_with_optional_erc20(validator, bridge, None)
    }

    fn new_with_optional_erc20(
        validator: PublicKey,
        bridge: BridgeContract,
        erc20_denom: Option<String>,
    ) -> Self {
        let mut chains = ConfiguredChains::default();
        let mut assets = BTreeMap::new();
        assets.insert(
            AssetName("eth".to_owned()),
            AssetConfig {
                decimals: 18,
                asset_id: ETH_ASSET_ID,
            },
        );
        if let Some(erc20_denom) = erc20_denom {
            assets.insert(
                AssetName(erc20_denom),
                AssetConfig {
                    decimals: 18,
                    asset_id: ERC20_ASSET_ID,
                },
            );
        }
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
    fn with_needed_bridge(validator: PublicKey) -> Self {
        Self::new(validator, BridgeContract::NeededEthereumBridge)
    }

    #[allow(dead_code)]
    fn with_needed_bridge_and_erc20_asset(validator: PublicKey, erc20_address: Address) -> Self {
        Self::new_with_optional_erc20(
            validator,
            BridgeContract::NeededEthereumBridge,
            Some(format!("{:#x}", erc20_address)),
        )
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
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> anyhow::Result<()> {
        match msg {
            EmptyMessage::Mint { amount } => {
                ctx.mint_asset(ETH_ASSET_ID, ctx.get_sender_id(), *amount)?;
            }
            EmptyMessage::Withdraw { recipient, amount } => {
                ctx.withdraw_asset(
                    ETH_ASSET_ID,
                    ExternalChain::EthereumLocal,
                    ctx.get_sender_id(),
                    &Wallet(recipient.clone()),
                    *amount,
                )?;
            }
        }
        Ok(())
    }
}

#[tokio::test]
async fn ethereum_listener_ingests_local_deposit() {
    init_logger(true, None);
    TestTasks::start(ethereum_listener_ingests_local_deposit_inner, ()).await;
}

#[tokio::test]
async fn ethereum_listener_ingests_local_erc20_regular_deposit() {
    init_logger(true, None);
    TestTasks::start(
        ethereum_listener_ingests_local_erc20_regular_deposit_inner,
        (),
    )
    .await;
}

#[tokio::test]
async fn subscription_only_listener_works() {
    init_logger(true, None);
    assert_eq!(
        std::env::var("KOLME_ETH_LISTENER_MODE").ok().as_deref(),
        Some("subscription-only"),
        "KOLME_ETH_LISTENER_MODE must be set to \"subscription-only\" for this test",
    );
    TestTasks::start(ethereum_listener_ingests_local_deposit_inner, ()).await;
}

#[tokio::test]
async fn ethereum_bridge_get_config_is_readable() {
    init_logger(true, None);
    TestTasks::start(ethereum_bridge_get_config_is_readable_inner, ()).await;
}

async fn ethereum_bridge_get_config_is_readable_inner(testtasks: TestTasks, (): ()) {
    let provider = anvil_provider().expect("failed to build anvil provider");
    assert_anvil_identifiers_match(&provider)
        .await
        .expect("local anvil setup does not match expected deterministic identifiers");
    let deployed = deploy_bridge_with_kolme(&testtasks)
        .await
        .expect("failed to deploy Ethereum bridge with kolme");
    let contract = IBridgeIntegration::new(deployed.bridge_address, provider);
    let cfg = contract
        .get_config()
        .call()
        .await
        .expect("bridge get_config call failed");

    assert_eq!(cfg.processor.len(), 33, "invalid processor key length");
    assert_eq!(
        cfg.processor.as_ref(),
        deployed.validator.public_key().as_bytes().as_ref(),
        "unexpected processor key in bridge config"
    );
    assert_eq!(cfg.listeners.len(), 1, "unexpected listener count");
    assert_eq!(
        cfg.listeners[0].as_ref(),
        deployed.validator.public_key().as_bytes().as_ref(),
        "unexpected listener key in bridge config"
    );
    assert_eq!(cfg.approvers.len(), 1, "unexpected approver count");
    assert_eq!(
        cfg.approvers[0].as_ref(),
        deployed.validator.public_key().as_bytes().as_ref(),
        "unexpected approver key in bridge config"
    );
    assert_eq!(cfg.neededListeners, 1, "unexpected listener quorum");
    assert_eq!(cfg.neededApprovers, 1, "unexpected approver quorum");
    assert_eq!(
        cfg.configNextEventId, 1,
        "unexpected initial configNextEventId"
    );
    assert_eq!(
        cfg.configNextActionId, 0,
        "unexpected initial configNextActionId"
    );
}

async fn ethereum_listener_ingests_local_deposit_inner(testtasks: TestTasks, (): ()) {
    let deployed = deploy_bridge_with_kolme(&testtasks)
        .await
        .expect("failed to deploy Ethereum bridge with kolme");
    let test_tx_amount: u128 = rand::thread_rng().gen_range(20u128..100u128);
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
    let tx_hash = send_eth_and_wait(
        &deployed.kolme,
        &provider,
        &expected_wallet,
        TEST_ANVIL_ACCOUNT_0_ADDRESS,
        deployed.bridge_address,
        test_tx_amount,
    )
    .await
    .expect("failed to send ETH to bridge contract");

    tracing::info!(
        "Ethereum deposit ingested by Kolme listener. sender={TEST_ANVIL_ACCOUNT_0_ADDRESS}, tx={tx_hash}, contract={:#x}, amount_wei={test_tx_amount}",
        deployed.bridge_address
    );
}

async fn ethereum_listener_ingests_local_erc20_regular_deposit_inner(testtasks: TestTasks, (): ()) {
    let provider = anvil_provider().expect("failed to build anvil provider");
    assert_anvil_identifiers_match(&provider)
        .await
        .expect("local anvil setup does not match expected deterministic identifiers");

    let token = deploy_test_erc20_mintable(&provider, TEST_ANVIL_ACCOUNT_0_ADDRESS, "Test", "TST")
        .await
        .expect("failed to deploy test ERC20");
    let deployed = deploy_bridge_with_kolme_with_erc20_asset(&testtasks, token)
        .await
        .expect("failed to deploy Ethereum bridge with kolme");

    let deposit_amount: u128 = rand::thread_rng().gen_range(20u128..100u128);
    mint_test_erc20_mintable(
        &provider,
        token,
        TEST_ANVIL_ACCOUNT_2_ADDRESS,
        TEST_ANVIL_ACCOUNT_2_ADDRESS
            .parse()
            .expect("hardcoded Anvil account 2 address is invalid"),
        deposit_amount,
    )
    .await
    .expect("failed to mint test ERC20 to depositor");
    approve_test_erc20_mintable(
        &provider,
        token,
        TEST_ANVIL_ACCOUNT_2_ADDRESS,
        deployed.bridge_address,
        deposit_amount,
    )
    .await
    .expect("failed to approve bridge for test ERC20");

    let expected_wallet = Wallet(format!(
        "{:#x}",
        TEST_ANVIL_ACCOUNT_2_ADDRESS
            .parse::<Address>()
            .expect("hardcoded Anvil account 2 address is invalid")
    ));
    let expected_denom = format!("{:#x}", token);
    send_regular_erc20_and_wait(
        &deployed.kolme,
        &provider,
        &expected_wallet,
        TEST_ANVIL_ACCOUNT_2_ADDRESS,
        deployed.bridge_address,
        token,
        deposit_amount,
    )
    .await
    .expect("failed to submit regular ERC20 deposit");

    tracing::info!(
        "Ethereum ERC20 regular() deposit ingested by Kolme listener. sender={TEST_ANVIL_ACCOUNT_2_ADDRESS}, contract={:#x}, token={expected_denom}, amount={deposit_amount}",
        deployed.bridge_address
    );
}

#[tokio::test]
async fn ethereum_submitter_deploys_local_bridge() {
    init_logger(true, None);
    TestTasks::start(ethereum_submitter_deploys_local_bridge_inner, ()).await;
}

#[tokio::test]
async fn ethereum_submitter_executes_signed_transfer_action() {
    init_logger(true, None);
    TestTasks::start(ethereum_submitter_executes_signed_transfer_action_inner, ()).await;
}

async fn ethereum_submitter_deploys_local_bridge_inner(testtasks: TestTasks, (): ()) {
    let provider = anvil_provider().expect("failed to build anvil provider");
    assert_anvil_identifiers_match(&provider)
        .await
        .expect("local anvil setup does not match expected deterministic identifiers");
    let deployed = deploy_bridge_with_kolme(&testtasks)
        .await
        .expect("failed to deploy Ethereum bridge with kolme");
    let contract = IBridgeIntegration::new(deployed.bridge_address, provider);
    let cfg = contract
        .get_config()
        .call()
        .await
        .expect("deployed bridge get_config call failed");

    assert_eq!(
        cfg.processor.as_ref(),
        deployed.validator.public_key().as_bytes().as_ref(),
        "unexpected processor key in deployed bridge config"
    );
    assert_eq!(cfg.listeners.len(), 1, "unexpected listener count");
    assert_eq!(
        cfg.listeners[0].as_ref(),
        deployed.validator.public_key().as_bytes().as_ref(),
        "unexpected listener key in deployed bridge config"
    );
    assert_eq!(cfg.approvers.len(), 1, "unexpected approver count");
    assert_eq!(
        cfg.approvers[0].as_ref(),
        deployed.validator.public_key().as_bytes().as_ref(),
        "unexpected approver key in deployed bridge config"
    );
    assert_eq!(cfg.neededListeners, 1, "unexpected listener quorum");
    assert_eq!(cfg.neededApprovers, 1, "unexpected approver quorum");
    assert_eq!(
        cfg.configNextEventId, 1,
        "unexpected initial configNextEventId"
    );
    assert_eq!(
        cfg.configNextActionId, 0,
        "unexpected initial configNextActionId"
    );
}

async fn ethereum_submitter_executes_signed_transfer_action_inner(testtasks: TestTasks, (): ()) {
    let provider = anvil_provider().expect("failed to build anvil provider");
    assert_anvil_identifiers_match(&provider)
        .await
        .expect("local anvil setup does not match expected deterministic identifiers");
    let deployed = deploy_bridge_with_kolme(&testtasks)
        .await
        .expect("failed to deploy Ethereum bridge with kolme");

    // Ensure bridge has enough ETH to execute transfer action.
    let bridge_fund_wei = 300_000_000_000_000_000u128; // 0.3 ETH
    send_eth(
        &provider,
        TEST_ANVIL_ACCOUNT_0_ADDRESS,
        deployed.bridge_address,
        bridge_fund_wei,
    )
    .await
    .expect("failed to fund deployed bridge");

    let recipient: Address = TEST_ANVIL_ACCOUNT_2_ADDRESS
        .parse()
        .expect("hardcoded Anvil account 2 address is invalid");
    let recipient_before = provider
        .get_balance(recipient)
        .await
        .expect("failed to fetch recipient balance before transfer");

    let withdraw_wei = 100_000_000_000_000_000u128; // 0.1 ETH
    let withdraw_amount = Decimal::try_from_i128_with_scale(withdraw_wei as i128, 18)
        .expect("valid decimal conversion for withdraw amount");
    let recipient_wallet = format!("{recipient:#x}");

    deployed
        .kolme
        .sign_propose_await_transaction(
            &deployed.validator,
            vec![
                Message::App(EmptyMessage::Mint {
                    amount: withdraw_amount,
                }),
                Message::App(EmptyMessage::Withdraw {
                    recipient: recipient_wallet,
                    amount: withdraw_amount,
                }),
            ],
        )
        .await
        .expect("failed to propose mint+withdraw transaction");

    let expected_balance = recipient_before + U256::from(withdraw_wei);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let current = provider
                .get_balance(recipient)
                .await
                .expect("failed to fetch recipient balance");
            if current >= expected_balance {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("timed out waiting for Ethereum submitter transfer execution");
}

#[derive(Clone)]
struct DeployedBridge {
    kolme: Kolme<EthereumBridgeTestApp>,
    validator: SecretKey,
    bridge_address: Address,
}

async fn deploy_bridge_with_kolme(testtasks: &TestTasks) -> Result<DeployedBridge> {
    let validator = SecretKey::random();
    let signer = TEST_ANVIL_ACCOUNT_0_PRIVATE_KEY.parse()?;
    let kolme = Kolme::new(
        EthereumBridgeTestApp::with_needed_bridge(validator.public_key()),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await?;

    testtasks.spawn_persistent(Processor::new(kolme.clone(), validator.clone()).run());
    testtasks.try_spawn_persistent(Approver::new(kolme.clone(), validator.clone()).run());
    testtasks.try_spawn_persistent(
        Listener::new(kolme.clone(), validator.clone()).run(ChainName::Ethereum),
    );
    testtasks.try_spawn_persistent(Submitter::new_ethereum(kolme.clone(), signer).run());

    let bridge_address = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(contract) = deployed_bridge_address(&kolme) {
                break Ok::<Address, anyhow::Error>(contract.parse()?);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .context("timed out waiting for Ethereum bridge deployment")??;

    Ok(DeployedBridge {
        kolme,
        validator,
        bridge_address,
    })
}

async fn deploy_bridge_with_kolme_with_erc20_asset(
    testtasks: &TestTasks,
    erc20_address: Address,
) -> Result<DeployedBridge> {
    let validator = SecretKey::random();
    let signer = TEST_ANVIL_ACCOUNT_0_PRIVATE_KEY.parse()?;
    let kolme = Kolme::new(
        EthereumBridgeTestApp::with_needed_bridge_and_erc20_asset(
            validator.public_key(),
            erc20_address,
        ),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await?;

    testtasks.spawn_persistent(Processor::new(kolme.clone(), validator.clone()).run());
    testtasks.try_spawn_persistent(Approver::new(kolme.clone(), validator.clone()).run());
    testtasks.try_spawn_persistent(
        Listener::new(kolme.clone(), validator.clone()).run(ChainName::Ethereum),
    );
    testtasks.try_spawn_persistent(Submitter::new_ethereum(kolme.clone(), signer).run());

    let bridge_address = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(contract) = deployed_bridge_address(&kolme) {
                break Ok::<Address, anyhow::Error>(contract.parse()?);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .context("timed out waiting for Ethereum bridge deployment")??;

    Ok(DeployedBridge {
        kolme,
        validator,
        bridge_address,
    })
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
    to: Address,
    amount_wei: u128,
) -> Result<String> {
    send_regular(
        provider,
        from,
        to,
        RegularCall {
            tokens: vec![],
            amounts_wei: vec![],
            keys: vec![],
            msg_value_wei: amount_wei,
        },
    )
    .await
}

async fn send_eth_and_wait(
    kolme: &Kolme<EthereumBridgeTestApp>,
    provider: &DynProvider,
    expected_wallet: &Wallet,
    from: &str,
    to: Address,
    amount_wei: u128,
) -> Result<String> {
    let kolme_for_waiter = kolme.clone();
    let wallet_for_waiter = expected_wallet.clone();
    let waiter = tokio::spawn(async move {
        wait_for_expected_listener_message_in_new_blocks(
            &kolme_for_waiter,
            &wallet_for_waiter,
            amount_wei,
        )
        .await
    });
    tokio::task::yield_now().await;

    let tx_hash = send_eth(provider, from, to, amount_wei).await?;

    tokio::time::timeout(Duration::from_secs(10), waiter)
        .await
        .context("timed out waiting for specific Ethereum listener message")?
        .context("listener message waiter task failed")??;

    Ok(tx_hash)
}

async fn send_regular_erc20_and_wait(
    kolme: &Kolme<EthereumBridgeTestApp>,
    provider: &DynProvider,
    expected_wallet: &Wallet,
    from: &str,
    bridge: Address,
    token: Address,
    amount_wei: u128,
) -> Result<String> {
    send_regular_and_wait(
        kolme,
        provider,
        expected_wallet,
        from,
        bridge,
        RegularCall {
            tokens: vec![token],
            amounts_wei: vec![amount_wei],
            keys: vec![],
            msg_value_wei: 0,
        },
    )
    .await
}

async fn send_regular_and_wait(
    kolme: &Kolme<EthereumBridgeTestApp>,
    provider: &DynProvider,
    expected_wallet: &Wallet,
    from: &str,
    bridge: Address,
    regular: RegularCall,
) -> Result<String> {
    let RegularCall {
        tokens,
        amounts_wei,
        keys,
        msg_value_wei,
    } = regular;
    anyhow::ensure!(
        tokens.len() == amounts_wei.len(),
        "send_regular_and_wait tokens/amounts length mismatch: {} vs {}",
        tokens.len(),
        amounts_wei.len()
    );

    // Waiter matches exactly one expected fund entry.
    // Keep this helper single-asset; batch matching is intentionally out of
    // scope for these tests.
    anyhow::ensure!(
        tokens.len() == 1,
        "send_regular_and_wait currently expects exactly one token/amount entry"
    );

    let expected_denom = if tokens[0] == Address::ZERO {
        "eth".to_owned()
    } else {
        format!("{:#x}", tokens[0])
    };
    let expected_amount_wei = amounts_wei[0];
    let kolme_for_waiter = kolme.clone();
    let wallet_for_waiter = expected_wallet.clone();
    let denom_for_waiter = expected_denom.clone();

    // Start waiting for the event before the tx is sent to avoid race condition
    let waiter = tokio::spawn(async move {
        wait_for_expected_listener_message_in_new_blocks_for_fund(
            &kolme_for_waiter,
            &wallet_for_waiter,
            &denom_for_waiter,
            expected_amount_wei,
        )
        .await
    });
    tokio::task::yield_now().await;

    let tx_hash = send_regular(
        provider,
        from,
        bridge,
        RegularCall {
            tokens,
            amounts_wei,
            keys,
            msg_value_wei,
        },
    )
    .await?;

    tokio::time::timeout(Duration::from_secs(10), waiter)
        .await
        .context("timed out waiting for specific Ethereum listener message")?
        .context("listener message waiter task failed")??;

    Ok(tx_hash)
}

async fn send_regular(
    provider: &DynProvider,
    from: &str,
    bridge: Address,
    regular: RegularCall,
) -> Result<String> {
    let RegularCall {
        tokens,
        amounts_wei,
        keys,
        msg_value_wei,
    } = regular;
    anyhow::ensure!(
        tokens.len() == amounts_wei.len(),
        "send_regular tokens/amounts length mismatch: {} vs {}",
        tokens.len(),
        amounts_wei.len()
    );

    let from: Address = from.parse()?;
    let call = IBridgeIntegration::regularCall {
        tokens,
        amounts: amounts_wei.into_iter().map(U256::from).collect(),
        keys,
    };
    let request = TransactionRequest::default()
        .with_from(from)
        .with_to(bridge)
        .with_value(U256::from(msg_value_wei))
        .with_input(call.abi_encode());
    let pending = provider.send_transaction(request).await?;
    let tx_hash = *pending.tx_hash();
    let _receipt = pending.get_receipt().await?;
    Ok(format!("{tx_hash:#x}"))
}

async fn wait_for_expected_listener_message_in_new_blocks_for_fund(
    kolme: &Kolme<EthereumBridgeTestApp>,
    expected_wallet: &Wallet,
    expected_denom: &str,
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
                        && funds[0].denom == expected_denom
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

fn test_erc20_mintable_artifact_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(TEST_ERC20_MINTABLE_ARTIFACT_PATH)
}

fn load_test_erc20_mintable_create_bytecode() -> Result<Vec<u8>> {
    let path = test_erc20_mintable_artifact_path();
    let artifact = fs::read_to_string(&path).with_context(|| {
        format!(
            "failed to read TestERC20Mintable artifact at {}. Run `just build-ethereum-contract` first",
            path.display()
        )
    })?;

    let parsed: FoundryArtifact =
        serde_json::from_str(&artifact).context("failed to parse TestERC20Mintable artifact")?;

    let object = parsed.bytecode.object.trim();
    anyhow::ensure!(
        !object.is_empty(),
        "TestERC20Mintable artifact contains empty bytecode.object"
    );
    hex::decode(object.trim_start_matches("0x"))
        .context("TestERC20Mintable artifact contains invalid hex in bytecode.object")
}

#[allow(dead_code)]
async fn deploy_test_erc20_mintable(
    provider: &DynProvider,
    from: &str,
    name: &str,
    symbol: &str,
) -> Result<Address> {
    let from: Address = from.parse()?;
    let mut initcode = load_test_erc20_mintable_create_bytecode()?;
    let ctor = TestERC20Mintable::constructorCall::new((name.to_owned(), symbol.to_owned()));
    initcode.extend_from_slice(&ctor.abi_encode());

    let request = TransactionRequest::default()
        .with_from(from)
        .with_deploy_code(initcode);
    let receipt = provider
        .send_transaction(request)
        .await?
        .get_receipt()
        .await?;
    receipt
        .contract_address
        .context("ERC20 deployment transaction did not return a contract address")
}

#[allow(dead_code)]
async fn mint_test_erc20_mintable(
    provider: &DynProvider,
    token: Address,
    from: &str,
    to: Address,
    amount: u128,
) -> Result<()> {
    let from: Address = from.parse()?;
    let call = ITestERC20Mintable::mintCall {
        to,
        amount: U256::from(amount),
    };
    let request = TransactionRequest::default()
        .with_from(from)
        .with_to(token)
        .with_input(call.abi_encode());
    let _receipt = provider
        .send_transaction(request)
        .await?
        .get_receipt()
        .await?;
    Ok(())
}

#[allow(dead_code)]
async fn approve_test_erc20_mintable(
    provider: &DynProvider,
    token: Address,
    from: &str,
    spender: Address,
    amount: u128,
) -> Result<()> {
    let from: Address = from.parse()?;
    let call = ITestERC20Mintable::approveCall {
        spender,
        amount: U256::from(amount),
    };
    let request = TransactionRequest::default()
        .with_from(from)
        .with_to(token)
        .with_input(call.abi_encode());
    let _receipt = provider
        .send_transaction(request)
        .await?
        .get_receipt()
        .await?;
    Ok(())
}

#[allow(dead_code)]
async fn balance_of_test_erc20_mintable(
    provider: DynProvider,
    token: Address,
    account: Address,
) -> Result<U256> {
    let contract = ITestERC20Mintable::new(token, provider);
    Ok(contract.balanceOf(account).call().await?)
}
