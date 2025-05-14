use std::{str::FromStr, sync::LazyLock};

use cosmos::{
    proto::cosmos::bank::v1beta1::MsgSend, Coin, CosmosNetwork, HasAddress, HasAddressHrp,
    SeedPhrase, TxBuilder,
};
use kolme::*;
use shared::cosmos::{ExecuteMsg, KeyRegistration};
use testtasks::TestTasks;

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
    fn new(processor: PublicKey, listener: PublicKey, approver: PublicKey) -> Self {
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
                    bridge: BridgeContract::NeededCosmosBridge { code_id: 1 },
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

#[tokio::test]
async fn test_cosmos_contract_update_self() {
    TestTasks::start(test_cosmos_contract_update_inner, true).await;
}

#[tokio::test]
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
        let mut builder = TxBuilder::default();
        builder.add_message(MsgSend {
            from_address: master_wallet.get_address_string(),
            to_address: local_wallet.get_address_string(),
            amount: vec![Coin {
                denom: "uosmo".to_owned(),
                amount: "20000000".to_owned(),
            }],
        });
        builder
            .sign_and_broadcast(&cosmos, &master_wallet)
            .await
            .unwrap();
    }
    let mut builder = TxBuilder::default();
    builder.add_message(MsgSend {
        from_address: local_wallet.get_address_string(),
        to_address: submitter_wallet.get_address_string(),
        amount: vec![Coin {
            denom: "uosmo".to_owned(),
            amount: "1000000".to_owned(),
        }],
    });
    builder
        .sign_and_broadcast(&cosmos, &local_wallet)
        .await
        .unwrap();

    let orig_processor = SecretKey::random(&mut rand::thread_rng());
    let new_processor = SecretKey::random(&mut rand::thread_rng());
    let listener = SecretKey::random(&mut rand::thread_rng());
    let approver = SecretKey::random(&mut rand::thread_rng());
    let client = SecretKey::random(&mut rand::thread_rng());
    let kolme = Kolme::new(
        SampleKolmeApp::new(
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
    let mut builder = TxBuilder::default();
    builder.add_message(MsgSend {
        from_address: local_wallet.get_address_string(),
        to_address: client_wallet.get_address_string(),
        amount: vec![Coin {
            denom: "uosmo".to_owned(),
            amount: "10000000".to_owned(),
        }],
    });
    builder
        .sign_and_broadcast(&cosmos, &local_wallet)
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
        BridgeContract::Deployed(addr) => addr.parse().unwrap(),
    };
    let contract = cosmos.make_contract(contract);
    contract
        .execute(
            &client_wallet,
            vec![Coin {
                denom: "uosmo".to_owned(),
                amount: "3000000".to_owned(),
            }],
            ExecuteMsg::Regular {
                keys: vec![
                    KeyRegistration::new(&client_wallet.get_address_string(), &client).unwrap(),
                ],
            },
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
                vec![Message::KeyRotation(
                    KeyRotationMessage::self_replace(
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
                vec![Message::KeyRotation(
                    KeyRotationMessage::new_set(
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
        let (change_set_id, pending_change_set) = kolme
            .get_framework_state()
            .get_key_rotation_state()
            .change_sets
            .first_key_value()
            .unwrap();
        let msg = KeyRotationMessage::approve(
            *change_set_id,
            &pending_change_set.validator_set,
            &approver,
        )
        .unwrap();
        kolme
            .sign_propose_await_transaction(&approver, vec![Message::KeyRotation(msg)])
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
