use std::{collections::BTreeMap, path::PathBuf, str::FromStr};

use cosmos::{
    proto::cosmos::bank::v1beta1::MsgSend, Coin, CosmosNetwork, HasAddress, HasAddressHrp,
    SeedPhrase, TxBuilder,
};
use integration_tests::{get_cosmos_connection, prepare_local_contract};
use kolme::*;
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
    fn new(code_id: u64, listener: PublicKey, processor: PublicKey, approver: PublicKey) -> Self {
        let mut chains = ConfiguredChains::default();
        chains
            .insert_cosmos(
                CosmosChain::OsmosisLocal,
                ChainConfig {
                    assets: BTreeMap::new(),
                    bridge: BridgeContract::NeededCosmosBridge { code_id },
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
            version: DUMMY_CODE_VERSION.to_owned(),
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
async fn test_cosmos_migrate() {
    init_logger(true, None);
    TestTasks::start(test_cosmos_migrate_inner, ()).await;
}

async fn test_cosmos_migrate_inner(testtasks: TestTasks, (): ()) {
    // Ensure we have exclusive access to the master wallet
    // and fund our local wallet.
    let cosmos = get_cosmos_connection().await.unwrap();
    let submitter_seed = SeedPhrase::random();
    let submitter_wallet = submitter_seed
        .with_hrp(CosmosNetwork::OsmosisLocal.get_address_hrp())
        .unwrap();
    let master_wallet = SeedPhrase::from_str("osmosis-local")
        .unwrap()
        .with_hrp(CosmosNetwork::OsmosisLocal.get_address_hrp())
        .unwrap();

    let mut builder = TxBuilder::default();
    builder.add_message(MsgSend {
        from_address: master_wallet.get_address_string(),
        to_address: submitter_wallet.get_address_string(),
        amount: vec![Coin {
            denom: "uosmo".to_owned(),
            amount: "20000000".to_owned(),
        }],
    });
    builder
        .sign_and_broadcast(&cosmos, &master_wallet)
        .await
        .unwrap();

    let processor = SecretKey::random(&mut rand::thread_rng());
    let listener = SecretKey::random(&mut rand::thread_rng());
    let approver = SecretKey::random(&mut rand::thread_rng());
    let orig_code_id = prepare_local_contract(&submitter_wallet)
        .await
        .unwrap()
        .get_code_id();
    let kolme = Kolme::new(
        SampleKolmeApp::new(
            orig_code_id,
            listener.public_key(),
            processor.public_key(),
            approver.public_key(),
        ),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();

    testtasks.try_spawn_persistent(Processor::new(kolme.clone(), processor.clone()).run());
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

    // OK, setup is done.
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

    // Make sure we started with code ID 1
    assert_eq!(contract.info().await.unwrap().code_id, orig_code_id);

    // Upload the contract again
    let mut wasm_file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    wasm_file.push("../../wasm/artifacts/kolme_cosmos_bridge.wasm");
    let new_code_id = cosmos
        .store_code_path(&submitter_wallet, &wasm_file)
        .await
        .unwrap();

    // Initiate a contract migration
    let block = kolme
        .sign_propose_await_transaction(
            &approver,
            vec![Message::Admin(AdminMessage::MigrateContract(Box::new(
                TaggedJson::new(MigrateContract {
                    chain: ExternalChain::OsmosisLocal,
                    new_code_id: new_code_id.get_code_id(),
                    message: serde_json::json!({}),
                })
                .unwrap()
                .sign(&approver)
                .unwrap(),
            )))],
        )
        .await
        .unwrap();

    let mut log_events = kolme.get_log_events_for(block.height()).await.unwrap();
    assert_eq!(log_events.len(), 1);
    let proposal_id = match log_events.pop().unwrap() {
        LogEvent::NewAdminProposal(id) => id,
        _ => unreachable!(),
    };
    let kolme_r = kolme.read();
    let payload = kolme_r.get_admin_proposal_payload(proposal_id).unwrap();
    let block = kolme
        .sign_propose_await_transaction(
            &listener,
            vec![Message::Admin(
                AdminMessage::approve(proposal_id, payload, &listener).unwrap(),
            )],
        )
        .await
        .unwrap();
    let log_events = kolme.get_log_events_for(block.height()).await.unwrap();
    assert_eq!(log_events.len(), 2);
    assert_eq!(log_events[0], LogEvent::AdminProposalApproved(proposal_id));
    match log_events[1] {
        LogEvent::NewBridgeAction { chain, id } => {
            kolme.wait_for_action_finished(chain, id).await.unwrap();
        }
        _ => unreachable!(),
    }

    assert_eq!(
        contract.info().await.unwrap().code_id,
        new_code_id.get_code_id()
    );
}
