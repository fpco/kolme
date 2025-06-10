use std::{str::FromStr, sync::LazyLock};

use cosmos::{CosmosNetwork, HasAddress, HasAddressHrp, SeedPhrase};
use integration_tests::{
    prepare_local_contract,
    setup::{
        airdrop, cosmos_deposit_and_register, cosmos_send_osmo, deploy_solana_bridge,
        make_osmo_token, make_solana_client, solana_deposit_and_register, solana_mint_to,
        BRIDGE_PUBKEY,
    },
};
use kolme::*;
use kolme_solana_bridge_client::{keypair::Keypair, signer::Signer};
use shared::types::KeyRegistration;
use testtasks::TestTasks;
use tokio::task::JoinSet;

/// In the future, move to an example and convert the binary to a library.
#[derive(Clone)]
pub struct SampleKolmeApp {
    pub genesis: GenesisInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct SampleState {}

impl MerkleSerialize for SampleState {
    fn merkle_serialize(
        &self,
        _serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        Ok(())
    }
}

impl MerkleDeserialize for SampleState {
    fn merkle_deserialize(
        _deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(SampleState {})
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum SampleMessage {
    SayHi,
}

const DUMMY_CODE_VERSION: &str = "dummy code version";

impl SampleKolmeApp {
    fn new_cosmos(
        bridge_code_id: u64,
        processor: PublicKey,
        listener: PublicKey,
        approver: PublicKey,
    ) -> Self {
        let mut chains = ConfiguredChains::default();
        chains
            .insert_cosmos(
                CosmosChain::OsmosisLocal,
                ChainConfig {
                    assets: std::iter::once((
                        AssetName("uosmo".to_owned()),
                        AssetConfig {
                            decimals: 6,
                            asset_id: AssetId(0),
                        },
                    ))
                    .collect(),
                    bridge: BridgeContract::NeededCosmosBridge {
                        code_id: bridge_code_id,
                    },
                },
            )
            .unwrap();
        let genesis = GenesisInfo {
            kolme_ident: "Dev code".to_owned(),
            validator_set: ValidatorSet {
                processor,
                listeners: std::iter::once(listener).collect(),
                needed_listeners: 1,
                approvers: std::iter::once(approver).collect(),
                needed_approvers: 1,
            },
            chains,
        };

        Self { genesis }
    }

    fn new_solana(processor: PublicKey, listener: PublicKey, approver: PublicKey) -> Self {
        let mut chains = ConfiguredChains::default();
        chains
            .insert_solana(
                SolanaChain::Local,
                ChainConfig {
                    assets: std::iter::once((
                        AssetName("osmof7hTFAuNjwMCcxVNThBDDftMNjiLR2cidDQzvwQ".to_owned()),
                        AssetConfig {
                            decimals: 6,
                            asset_id: AssetId(0),
                        },
                    ))
                    .collect(),
                    bridge: BridgeContract::NeededSolanaBridge {
                        program_id: BRIDGE_PUBKEY.to_string(),
                    },
                },
            )
            .unwrap();

        let genesis = GenesisInfo {
            kolme_ident: "Dev code".to_owned(),
            validator_set: ValidatorSet {
                processor,
                listeners: std::iter::once(listener).collect(),
                needed_listeners: 1,
                approvers: std::iter::once(approver).collect(),
                needed_approvers: 1,
            },
            chains,
        };

        Self { genesis }
    }
}

impl KolmeApp for SampleKolmeApp {
    type State = SampleState;
    type Message = SampleMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
    }

    fn new_state() -> anyhow::Result<Self::State> {
        Ok(SampleState {})
    }

    async fn execute(
        &self,
        _ctx: &mut ExecutionContext<'_, Self>,
        _msg: &Self::Message,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

#[test_log::test(tokio::test)]
async fn test_cosmos_contract_update_self() {
    TestTasks::start(test_cosmos_contract_update_inner, true).await;
}

#[test_log::test(tokio::test)]
async fn test_cosmos_contract_update_set() {
    TestTasks::start(test_cosmos_contract_update_inner, false).await;
}

async fn test_cosmos_contract_update_inner(testtasks: TestTasks, self_replace: bool) {
    static WALLET_LOCK: LazyLock<tokio::sync::Mutex<()>> =
        LazyLock::new(|| tokio::sync::Mutex::const_new(()));

    // Ensure we have exclusive access to the master wallet
    // and fund our local wallet.
    let local_wallet = SeedPhrase::random()
        .with_hrp(CosmosNetwork::OsmosisLocal.get_address_hrp())
        .unwrap();
    let submitter_seed = SeedPhrase::random();
    let submitter_wallet = submitter_seed
        .with_hrp(CosmosNetwork::OsmosisLocal.get_address_hrp())
        .unwrap();
    let master_wallet = SeedPhrase::from_str("osmosis-local")
        .unwrap()
        .with_hrp(CosmosNetwork::OsmosisLocal.get_address_hrp())
        .unwrap();

    let cosmos = CosmosNetwork::OsmosisLocal.connect().await.unwrap();

    {
        let _guard = WALLET_LOCK.lock().await;
        cosmos_send_osmo(
            &cosmos,
            &master_wallet,
            &local_wallet.get_address(),
            20000000,
        )
        .await
        .unwrap();
    }

    cosmos_send_osmo(
        &cosmos,
        &local_wallet,
        &submitter_wallet.get_address(),
        1000000,
    )
    .await
    .unwrap();

    let orig_processor = SecretKey::random(&mut rand::thread_rng());
    let new_processor = SecretKey::random(&mut rand::thread_rng());
    let listener = SecretKey::random(&mut rand::thread_rng());
    let approver = SecretKey::random(&mut rand::thread_rng());
    let client = SecretKey::random(&mut rand::thread_rng());
    let kolme = Kolme::new(
        SampleKolmeApp::new_cosmos(
            prepare_local_contract(&local_wallet)
                .await
                .unwrap()
                .get_code_id(),
            orig_processor.public_key(),
            listener.public_key(),
            approver.public_key(),
        ),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();

    let mut processor = Processor::new(kolme.clone(), orig_processor.clone());
    processor.add_secret(new_processor.clone());
    testtasks.try_spawn_persistent(processor.run());
    testtasks.try_spawn_persistent(Submitter::new_cosmos(kolme.clone(), submitter_seed).run());
    testtasks.try_spawn_persistent(
        Listener::new(kolme.clone(), listener.clone()).run(ChainName::Cosmos),
    );
    testtasks.try_spawn_persistent(Approver::new(kolme.clone(), approver.clone()).run());

    kolme
        .wait_for_block(BlockHeight::start().next())
        .await
        .unwrap();

    let cosmos = kolme.get_cosmos(CosmosChain::OsmosisLocal).await.unwrap();
    let client_wallet = SeedPhrase::random()
        .with_hrp(CosmosNetwork::OsmosisLocal.get_address_hrp())
        .unwrap();
    let withdrawal_dest = SeedPhrase::random()
        .with_hrp(CosmosNetwork::OsmosisLocal.get_address_hrp())
        .unwrap()
        .get_address();

    cosmos_send_osmo(
        &cosmos,
        &local_wallet,
        &client_wallet.get_address(),
        10000000,
    )
    .await
    .unwrap();

    // OK, setup is done. Send some funds into the protocol
    let contract = match &kolme
        .read()
        .get_bridge_contracts()
        .get(ExternalChain::OsmosisLocal)
        .unwrap()
        .config
        .bridge
    {
        BridgeContract::NeededCosmosBridge { .. } => unreachable!(),
        BridgeContract::NeededSolanaBridge { .. } => unreachable!(),
        BridgeContract::Deployed(addr) => addr.clone(),
    };

    cosmos_deposit_and_register(
        &cosmos,
        &contract,
        &client_wallet,
        3000000,
        vec![KeyRegistration::cosmos(&client_wallet.get_address_string(), &client).unwrap()],
    )
    .await
    .unwrap();

    kolme
        .wait_account_for_key(client.public_key())
        .await
        .unwrap();

    // Get the balance of the client wallet at this state, then withdraw 1 OSMO (1e6 uosmo),
    // and confirm that we get that back
    let get_balance = || async {
        match cosmos
            .all_balances(withdrawal_dest)
            .await
            .unwrap()
            .into_iter()
            .find(|x| x.denom == "uosmo")
        {
            None => 0u64,
            Some(coin) => coin.amount.parse().unwrap(),
        }
    };
    assert_eq!(get_balance().await, 0);
    let block = kolme
        .sign_propose_await_transaction(
            &client,
            vec![Message::Bank(BankMessage::Withdraw {
                asset: AssetId(0),
                chain: ExternalChain::OsmosisLocal,
                dest: Wallet(withdrawal_dest.get_address_string()),
                amount: Decimal::ONE,
            })],
        )
        .await
        .unwrap();

    let block = kolme.get_block(block.height()).await.unwrap().unwrap();
    let chain_state = block
        .framework_state
        .get_chain_states()
        .get(ExternalChain::OsmosisLocal)
        .unwrap();
    let action_id = BridgeActionId(chain_state.next_action_id.0 - 1);
    kolme
        .wait_for_action_finished(ExternalChain::OsmosisLocal, action_id)
        .await
        .unwrap();
    assert_eq!(get_balance().await, 1_000_000);

    // OK, fund transfers are working fine. Now let's try changing the processor and do it again.
    if self_replace {
        kolme
            .sign_propose_await_transaction(
                &orig_processor,
                vec![Message::Admin(
                    AdminMessage::self_replace(
                        ValidatorType::Processor,
                        new_processor.public_key(),
                        &orig_processor,
                    )
                    .unwrap(),
                )],
            )
            .await
            .unwrap();
    } else {
        kolme
            .sign_propose_await_transaction(
                &listener,
                vec![Message::Admin(
                    AdminMessage::new_set(
                        ValidatorSet {
                            processor: new_processor.public_key(),
                            listeners: std::iter::once(listener.public_key()).collect(),
                            needed_listeners: 1,
                            approvers: std::iter::once(approver.public_key()).collect(),
                            needed_approvers: 1,
                        },
                        &listener,
                    )
                    .unwrap(),
                )],
            )
            .await
            .unwrap();
        let kolme = kolme.read();
        let (admin_proposal_id, pending_admin_proposal) = kolme
            .get_framework_state()
            .get_admin_proposal_state()
            .proposals
            .first_key_value()
            .unwrap();
        let msg = AdminMessage::approve(
            *admin_proposal_id,
            &pending_admin_proposal.payload,
            &approver,
        )
        .unwrap();
        kolme
            .sign_propose_await_transaction(&approver, vec![Message::Admin(msg)])
            .await
            .unwrap();
    }

    // And now do another withdrawal
    let block = kolme
        .sign_propose_await_transaction(
            &client,
            vec![Message::Bank(BankMessage::Withdraw {
                asset: AssetId(0),
                chain: ExternalChain::OsmosisLocal,
                dest: Wallet(withdrawal_dest.get_address_string()),
                amount: Decimal::TWO,
            })],
        )
        .await
        .unwrap();

    let block = kolme.get_block(block.height()).await.unwrap().unwrap();
    let chain_state = block
        .framework_state
        .get_chain_states()
        .get(ExternalChain::OsmosisLocal)
        .unwrap();
    let action_id = BridgeActionId(chain_state.next_action_id.0 - 1);
    kolme
        .wait_for_action_finished(ExternalChain::OsmosisLocal, action_id)
        .await
        .unwrap();
    assert_eq!(get_balance().await, 3_000_000);
}

#[test_log::test(tokio::test)]
#[ignore = "depends on local Solana validator which needs to be restarted after each test thus hidden from default tests"]
async fn solana_contract_update_self() {
    test_solana_contract_update(true).await;
}

#[test_log::test(tokio::test)]
#[ignore = "depends on local Solana validator which needs to be restarted after each test thus hidden from default tests"]
async fn solana_contract_update_set() {
    test_solana_contract_update(false).await;
}

async fn test_solana_contract_update(self_replace: bool) {
    let orig_processor = SecretKey::random(&mut rand::thread_rng());
    let new_processor = SecretKey::random(&mut rand::thread_rng());
    let listener = SecretKey::random(&mut rand::thread_rng());
    let approver = SecretKey::random(&mut rand::thread_rng());
    let client = SecretKey::random(&mut rand::thread_rng());

    let submitter = Keypair::new();
    let client_wallet = Keypair::new();
    let withdrawal_dest = Keypair::new();

    tracing::info!("withdrawal_dest: {}", withdrawal_dest.pubkey());

    let solana = make_solana_client();
    let result = futures::join!(
        async { make_osmo_token(solana.clone()).await.unwrap() },
        async { deploy_solana_bridge().await.unwrap() },
        async {
            airdrop(&solana, &submitter.pubkey(), 10000000)
                .await
                .unwrap()
        },
        async {
            airdrop(&solana, &client_wallet.pubkey(), 10000000)
                .await
                .unwrap()
        },
    );

    let osmo = result.0;
    let kolme = Kolme::new(
        SampleKolmeApp::new_solana(
            orig_processor.public_key(),
            listener.public_key(),
            approver.public_key(),
        ),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();

    let mut set = JoinSet::new();

    let mut processor = Processor::new(kolme.clone(), orig_processor.clone());
    processor.add_secret(new_processor.clone());
    set.spawn(processor.run());
    set.spawn(Submitter::new_solana(kolme.clone(), submitter).run());
    set.spawn(Listener::new(kolme.clone(), listener.clone()).run(ChainName::Solana));
    set.spawn(Approver::new(kolme.clone(), approver.clone()).run());

    futures::join!(
        async {
            kolme
                .wait_for_block(BlockHeight::start().next())
                .await
                .unwrap()
        },
        async {
            solana_mint_to(&osmo, &client_wallet.pubkey(), 10000000)
                .await
                .unwrap()
        }
    );

    // OK, setup is done. Send some funds into the protocol
    solana_deposit_and_register(
        &solana,
        &client_wallet,
        &osmo,
        3000000,
        vec![KeyRegistration::solana(client_wallet.pubkey().to_bytes(), &client).unwrap()],
    )
    .await
    .unwrap();

    tracing::info!("Waiting for account to be created.");
    kolme
        .wait_account_for_key(client.public_key())
        .await
        .unwrap();

    // Get the balance of the client wallet at this state, then withdraw 1 OSMO (1e6 uosmo),
    // and confirm that we get that back
    let get_balance = || async {
        osmo.get_or_create_associated_account_info(&withdrawal_dest.pubkey())
            .await
            .unwrap()
            .base
            .amount
    };

    assert_eq!(get_balance().await, 0);

    tracing::info!("Withdrawing 1 OSMO from Kolme.");
    let block = kolme
        .sign_propose_await_transaction(
            &client,
            vec![Message::Bank(BankMessage::Withdraw {
                asset: AssetId(0),
                chain: ExternalChain::SolanaLocal,
                dest: Wallet(withdrawal_dest.pubkey().to_string()),
                amount: Decimal::ONE,
            })],
        )
        .await
        .unwrap();

    let block = kolme.get_block(block.height()).await.unwrap().unwrap();
    let chain_state = block
        .framework_state
        .get_chain_states()
        .get(ExternalChain::SolanaLocal)
        .unwrap();
    let action_id = BridgeActionId(chain_state.next_action_id.0 - 1);
    kolme
        .wait_for_action_finished(ExternalChain::SolanaLocal, action_id)
        .await
        .unwrap();
    assert_eq!(get_balance().await, 1_000_000);

    // OK, fund transfers are working fine. Now let's try changing the processor and do it again.
    if self_replace {
        kolme
            .sign_propose_await_transaction(
                &orig_processor,
                vec![Message::Admin(
                    AdminMessage::self_replace(
                        ValidatorType::Processor,
                        new_processor.public_key(),
                        &orig_processor,
                    )
                    .unwrap(),
                )],
            )
            .await
            .unwrap();
    } else {
        kolme
            .sign_propose_await_transaction(
                &listener,
                vec![Message::Admin(
                    AdminMessage::new_set(
                        ValidatorSet {
                            processor: new_processor.public_key(),
                            listeners: std::iter::once(listener.public_key()).collect(),
                            needed_listeners: 1,
                            approvers: std::iter::once(approver.public_key()).collect(),
                            needed_approvers: 1,
                        },
                        &listener,
                    )
                    .unwrap(),
                )],
            )
            .await
            .unwrap();
        let kolme = kolme.read();
        let (admin_proposal_id, pending_admin_proposal) = kolme
            .get_framework_state()
            .get_admin_proposal_state()
            .proposals
            .first_key_value()
            .unwrap();
        let msg = AdminMessage::approve(
            *admin_proposal_id,
            &pending_admin_proposal.payload,
            &approver,
        )
        .unwrap();
        kolme
            .sign_propose_await_transaction(&approver, vec![Message::Admin(msg)])
            .await
            .unwrap();
    }

    // And now do another withdrawal
    tracing::info!("Withdrawing 2 OSMO from Kolme.");
    let block = kolme
        .sign_propose_await_transaction(
            &client,
            vec![Message::Bank(BankMessage::Withdraw {
                asset: AssetId(0),
                chain: ExternalChain::SolanaLocal,
                dest: Wallet(withdrawal_dest.pubkey().to_string()),
                amount: Decimal::TWO,
            })],
        )
        .await
        .unwrap();

    let block = kolme.get_block(block.height()).await.unwrap().unwrap();
    let chain_state = block
        .framework_state
        .get_chain_states()
        .get(ExternalChain::SolanaLocal)
        .unwrap();
    let action_id = BridgeActionId(chain_state.next_action_id.0 - 1);

    tracing::info!("Waiting for action: {action_id}");
    kolme
        .wait_for_action_finished(ExternalChain::SolanaLocal, action_id)
        .await
        .unwrap();
    assert_eq!(get_balance().await, 3_000_000);

    assert!(
        set.try_join_next().is_none(),
        "A task has exited unexpectedly."
    );
    set.shutdown().await;
}
