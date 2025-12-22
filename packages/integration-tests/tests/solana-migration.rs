use std::collections::BTreeMap;

use integration_tests::setup::{
    airdrop, deploy_solana_bridge, make_buffer, make_solana_client, BRIDGE_PUBKEY,
};
use kolme::*;
use kolme_solana_bridge_client::{derive_upgrade_authority_pda, keypair::Keypair, signer::Signer};
use testtasks::TestTasks;

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
    fn new(listener: PublicKey, processor: PublicKey, approver: PublicKey) -> Self {
        let mut chains = ConfiguredChains::default();
        chains
            .insert_solana(
                SolanaChain::Local,
                ChainConfig {
                    assets: BTreeMap::new(),
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

    fn new_state(&self) -> anyhow::Result<Self::State> {
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
#[ignore = "depends on local Solana validator thus hidden from default tests"]
async fn test_solana_migrate() {
    init_logger(true, None);
    TestTasks::start(test_solana_migrate_inner, ()).await;
}

async fn test_solana_migrate_inner(testtasks: TestTasks, (): ()) {
    deploy_solana_bridge().await.unwrap();

    let client = make_solana_client();

    let submitter = Keypair::new();
    // let deployer = Keypair::new();

    // futures::join!(
    //     async { airdrop(&client, &submitter.pubkey(), 10000000).await.unwrap(); },
    //     async { airdrop(&client, &deployer.pubkey(), 20000000000).await.unwrap(); },
    // );

    airdrop(&client, &submitter.pubkey(), 10000000)
        .await
        .unwrap();

    let spill_address = Keypair::new();

    let processor = SecretKey::random();
    let listener = SecretKey::random();
    let approver = SecretKey::random();

    let kolme = Kolme::new(
        SampleKolmeApp::new(
            listener.public_key(),
            processor.public_key(),
            approver.public_key(),
        ),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();

    testtasks.spawn_persistent(Processor::new(kolme.clone(), processor.clone()).run());
    testtasks.try_spawn_persistent(Submitter::new_solana(kolme.clone(), submitter, None).run());
    testtasks.try_spawn_persistent(
        Listener::new(kolme.clone(), listener.clone()).run(ChainName::Solana),
    );
    testtasks.try_spawn_persistent(Approver::new(kolme.clone(), approver.clone()).run());

    let (_, buffer) = futures::join!(
        async {
            kolme
                .wait_for_block(BlockHeight::start().next())
                .await
                .unwrap();
        },
        async {
            let authority = derive_upgrade_authority_pda(&BRIDGE_PUBKEY);

            let workdir = env!("CARGO_MANIFEST_DIR");
            let path = format!("{workdir}/solana/sbf-out/kolme_solana_bridge.so");

            make_buffer(&path, &authority).await.unwrap()
        }
    );

    assert_eq!(
        client.get_balance(&spill_address.pubkey()).await.unwrap(),
        0
    );

    let block = kolme
        .sign_propose_await_transaction(
            &approver,
            vec![Message::Admin(AdminMessage::MigrateContract(Box::new(
                TaggedJson::new(MigrateContract::Solana {
                    chain: SolanaChain::Local,
                    buffer_address: buffer,
                    spill_address: spill_address.pubkey(),
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

    assert!(client.get_balance(&spill_address.pubkey()).await.unwrap() > 0);
}
