use std::{
    collections::BTreeSet,
    sync::{Arc, OnceLock},
};

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

pub fn get_sample_secret_key() -> &'static SecretKey {
    static KEY: OnceLock<SecretKey> = OnceLock::new();
    let mut rng = rand::thread_rng();
    KEY.get_or_init(|| SecretKey::random(&mut rng))
}

const DUMMY_CODE_VERSION: &str = "dummy code version";

impl SampleKolmeApp {
    fn new(validator: PublicKey) -> Self {
        let mut set = BTreeSet::new();
        set.insert(validator);

        let genesis = GenesisInfo {
            kolme_ident: "Dev code".to_owned(),
            validator_set: ValidatorSet {
                processor: validator,
                listeners: set.clone(),
                needed_listeners: 1,
                approvers: set,
                needed_approvers: 1,
            },
            chains: ConfiguredChains::default(),
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
async fn test_self_replace() {
    TestTasks::start(test_self_replace_inner, ()).await;
}

async fn test_self_replace_inner(testtasks: TestTasks, (): ()) {
    let secret1 = SecretKey::random(&mut rand::thread_rng());
    let secret2 = SecretKey::random(&mut rand::thread_rng());
    let client = SecretKey::random(&mut rand::thread_rng());
    let fake_validator = SecretKey::random(&mut rand::thread_rng());
    let store = KolmeStore::new_in_memory();
    let kolme = Kolme::new(
        SampleKolmeApp::new(secret1.public_key()),
        DUMMY_CODE_VERSION,
        store.clone(),
    )
    .await
    .unwrap();

    let processor1 = Processor::new(kolme.clone(), secret1.clone());
    testtasks.try_spawn_persistent(processor1.run());

    // Prove that we can produce a block
    kolme
        .sign_propose_await_transaction(&client, vec![Message::App(SampleMessage::SayHi)])
        .await
        .unwrap();

    // Now try replacing ourselves as a listener, that should work and
    // block production should be unaffected.
    kolme
        .sign_propose_await_transaction(
            &secret1,
            vec![Message::Admin(
                AdminMessage::self_replace(
                    ValidatorType::Listener,
                    fake_validator.public_key(),
                    &secret1,
                )
                .unwrap(),
            )],
        )
        .await
        .unwrap();

    // Check that the new genesis info is updated correctly
    {
        let kolme = kolme.read();
        let config = kolme.get_framework_state().get_validator_set();
        assert_eq!(config.processor, secret1.public_key());
        assert_eq!(
            vec![secret1.public_key()],
            config.approvers.iter().copied().collect::<Vec<_>>()
        );
        assert_eq!(
            vec![fake_validator.public_key()],
            config.listeners.iter().copied().collect::<Vec<_>>()
        );
    }

    // Replace the processor and confirm the new config lands
    kolme
        .sign_propose_await_transaction(
            &secret1,
            vec![Message::Admin(
                AdminMessage::self_replace(
                    ValidatorType::Processor,
                    secret2.public_key(),
                    &secret1,
                )
                .unwrap(),
            )],
        )
        .await
        .unwrap();
    {
        let kolme = kolme.read();
        let config = kolme.get_framework_state().get_validator_set();
        assert_eq!(config.processor, secret2.public_key());
        assert_eq!(
            vec![secret1.public_key()],
            config.approvers.iter().copied().collect::<Vec<_>>()
        );
        assert_eq!(
            vec![fake_validator.public_key()],
            config.listeners.iter().copied().collect::<Vec<_>>()
        );
    }

    // Generate a new transaction then try to broadcast it. It should fail to land
    // because we don't have a valid processor running.
    let tx = Arc::new(
        kolme
            .read()
            .create_signed_transaction(&client, vec![Message::App(SampleMessage::SayHi)])
            .unwrap(),
    );
    let txhash = tx.hash();
    kolme.propose_transaction(tx.clone());

    // Wait for the mempool to clear out, meaning the processor tried to process
    kolme.wait_on_empty_mempool().await;

    // Confirm that the transaction did not land.
    assert_eq!(kolme.get_tx_height(txhash).await.unwrap(), None);

    // Now try again, but starting up the new processor.
    // Give it its own Kolme so the two processors aren't fighting
    // over the mempool.
    let kolme2 = Kolme::new(
        SampleKolmeApp::new(secret1.public_key()),
        DUMMY_CODE_VERSION,
        store.clone(),
    )
    .await
    .unwrap();
    assert_eq!(kolme.get_tx_height(txhash).await.unwrap(), None);
    testtasks.try_spawn_persistent(Processor::new(kolme2.clone(), secret2.clone()).run());
    assert_eq!(kolme.get_tx_height(txhash).await.unwrap(), None);

    // And try broadcasting the transaction again, it should work this time.
    kolme2.propose_transaction(tx);
    kolme2.wait_on_empty_mempool().await;
    assert_ne!(kolme2.get_tx_height(txhash).await.unwrap(), None);
}

#[tokio::test]
async fn test_total_replace() {
    init_logger(false, None);
    TestTasks::start(test_total_replace_inner, ()).await;
}

async fn test_total_replace_inner(testtasks: TestTasks, (): ()) {
    let orig_processor = SecretKey::random(&mut rand::thread_rng());
    let listener = SecretKey::random(&mut rand::thread_rng());
    let temp_approver = SecretKey::random(&mut rand::thread_rng());
    let new_approvers = std::iter::repeat_with(|| SecretKey::random(&mut rand::thread_rng()))
        .take(3)
        .collect::<Vec<_>>();
    let new_processor = SecretKey::random(&mut rand::thread_rng());
    let client = SecretKey::random(&mut rand::thread_rng());
    let store = KolmeStore::new_in_memory();
    let kolme = Kolme::new(
        SampleKolmeApp::new(orig_processor.public_key()),
        DUMMY_CODE_VERSION,
        store.clone(),
    )
    .await
    .unwrap();

    let mut processor = Processor::new(kolme.clone(), orig_processor.clone());
    processor.add_secret(new_processor.clone());
    testtasks.try_spawn_persistent(processor.run());

    // Swap out the approver and listener right away. Since there's only one
    // key being used, we don't need to do any approving.
    let expected_new_set = ValidatorSet {
        processor: orig_processor.public_key(),
        listeners: std::iter::once(listener.public_key()).collect(),
        needed_listeners: 1,
        approvers: std::iter::once(temp_approver.public_key()).collect(),
        needed_approvers: 1,
    };
    kolme
        .sign_propose_await_transaction(
            &orig_processor,
            vec![Message::Admin(
                AdminMessage::new_set(expected_new_set.clone(), &orig_processor).unwrap(),
            )],
        )
        .await
        .unwrap();
    assert_eq!(
        &expected_new_set,
        kolme.read().get_framework_state().get_validator_set()
    );
    // And no pending proposals should be left behind
    assert_eq!(
        kolme
            .read()
            .get_framework_state()
            .get_admin_proposal_state()
            .proposals
            .len(),
        0
    );

    // Use the processor to initiate an approver replacement
    let proposed_set1 = ValidatorSet {
        processor: new_processor.public_key(),
        listeners: [orig_processor.public_key()].into_iter().collect(),
        needed_listeners: 1,
        approvers: new_approvers.iter().map(SecretKey::public_key).collect(),
        needed_approvers: 2,
    };
    kolme
        .sign_propose_await_transaction(
            &orig_processor,
            vec![Message::Admin(
                AdminMessage::new_set(proposed_set1.clone(), &orig_processor).unwrap(),
            )],
        )
        .await
        .unwrap();

    // Old approver can still make a proposal too
    let proposed_set2 = ValidatorSet {
        processor: temp_approver.public_key(),
        listeners: [temp_approver.public_key()].into_iter().collect(),
        needed_listeners: 1,
        approvers: [temp_approver.public_key()].into_iter().collect(),
        needed_approvers: 1,
    };
    kolme
        .sign_propose_await_transaction(
            &temp_approver,
            vec![Message::Admin(
                AdminMessage::new_set(proposed_set2.clone(), &temp_approver).unwrap(),
            )],
        )
        .await
        .unwrap();

    // But a random client can't make a proposal
    let rejected_set = ValidatorSet {
        processor: client.public_key(),
        listeners: [client.public_key()].into_iter().collect(),
        needed_listeners: 1,
        approvers: [client.public_key()].into_iter().collect(),
        needed_approvers: 1,
    };
    kolme
        .sign_propose_await_transaction(
            &client,
            vec![Message::Admin(
                AdminMessage::new_set(rejected_set, &client).unwrap(),
            )],
        )
        .await
        .unwrap_err();

    // These proposals shouldn't have changed anything yet
    assert_eq!(
        &expected_new_set,
        kolme.read().get_framework_state().get_validator_set()
    );

    // And make sure the new proposals are waiting
    let (change_id_1, change_id_2) = {
        let kolme = kolme.read();
        let proposals = kolme.get_framework_state().get_admin_proposal_state();
        assert_eq!(proposals.proposals.len(), 2);
        let first_id = AdminProposalId(proposals.next_admin_proposal_id.0 - 2);
        let second_id = first_id.next();
        assert_eq!(
            &proposed_set1,
            match &proposals.proposals.get(&first_id).unwrap().payload {
                ProposalPayload::NewSet(set) => set.as_inner(),
                _ => unreachable!(),
            }
        );
        assert_eq!(
            &proposed_set2,
            match &proposals.proposals.get(&second_id).unwrap().payload {
                ProposalPayload::NewSet(set) => set.as_inner(),
                _ => unreachable!(),
            }
        );
        (first_id, second_id)
    };

    // Random clients, newly added validators, and validators that
    // already voted cannot approve
    kolme
        .sign_propose_await_transaction(
            &client,
            vec![Message::Admin(
                AdminMessage::approve(
                    change_id_1,
                    &ProposalPayload::NewSet(TaggedJson::new(proposed_set1.clone()).unwrap()),
                    &client,
                )
                .unwrap(),
            )],
        )
        .await
        .unwrap_err();
    kolme
        .sign_propose_await_transaction(
            &new_processor,
            vec![Message::Admin(
                AdminMessage::approve(
                    change_id_1,
                    &ProposalPayload::NewSet(TaggedJson::new(proposed_set1.clone()).unwrap()),
                    &new_processor,
                )
                .unwrap(),
            )],
        )
        .await
        .unwrap_err();
    kolme
        .sign_propose_await_transaction(
            &orig_processor,
            vec![Message::Admin(
                AdminMessage::approve(
                    change_id_1,
                    &ProposalPayload::NewSet(TaggedJson::new(proposed_set1.clone()).unwrap()),
                    &orig_processor,
                )
                .unwrap(),
            )],
        )
        .await
        .unwrap_err();

    // One vote from a current approver is sufficient
    kolme
        .sign_propose_await_transaction(
            &temp_approver,
            vec![Message::Admin(
                AdminMessage::approve(
                    change_id_1,
                    &ProposalPayload::NewSet(TaggedJson::new(proposed_set1.clone()).unwrap()),
                    &temp_approver,
                )
                .unwrap(),
            )],
        )
        .await
        .unwrap();

    // This should have worked, so signing with a listener will fail because
    // the change is no longer pending.
    kolme
        .sign_propose_await_transaction(
            &listener,
            vec![Message::Admin(
                AdminMessage::approve(
                    change_id_1,
                    &ProposalPayload::NewSet(TaggedJson::new(proposed_set1.clone()).unwrap()),
                    &listener,
                )
                .unwrap(),
            )],
        )
        .await
        .unwrap_err();
    kolme
        .sign_propose_await_transaction(
            &listener,
            vec![Message::Admin(
                AdminMessage::approve(
                    change_id_2,
                    &ProposalPayload::NewSet(TaggedJson::new(proposed_set2.clone()).unwrap()),
                    &listener,
                )
                .unwrap(),
            )],
        )
        .await
        .unwrap_err();

    // Confirm the new change set is correct
    assert_eq!(
        &proposed_set1,
        kolme.read().get_framework_state().get_validator_set()
    );

    // Now that we have more than 1 approver, do a final check that we need
    // 2 members of the approver group before a change is approved.
    kolme
        .sign_propose_await_transaction(
            &new_approvers[0],
            vec![Message::Admin(
                AdminMessage::new_set(expected_new_set.clone(), &new_approvers[0]).unwrap(),
            )],
        )
        .await
        .unwrap();
    let change_set_id = *kolme
        .read()
        .get_framework_state()
        .get_admin_proposal_state()
        .proposals
        .first_key_value()
        .unwrap()
        .0;
    kolme
        .sign_propose_await_transaction(
            &new_processor,
            vec![Message::Admin(
                AdminMessage::approve(
                    change_set_id,
                    &ProposalPayload::NewSet(TaggedJson::new(expected_new_set.clone()).unwrap()),
                    &new_processor,
                )
                .unwrap(),
            )],
        )
        .await
        .unwrap();
    kolme
        .sign_propose_await_transaction(
            &new_approvers[1],
            vec![Message::Admin(
                AdminMessage::approve(
                    change_set_id,
                    &ProposalPayload::NewSet(TaggedJson::new(expected_new_set.clone()).unwrap()),
                    &new_approvers[1],
                )
                .unwrap(),
            )],
        )
        .await
        .unwrap();
    kolme
        .sign_propose_await_transaction(
            &new_approvers[2],
            vec![Message::Admin(
                AdminMessage::approve(
                    change_set_id,
                    &ProposalPayload::NewSet(TaggedJson::new(expected_new_set.clone()).unwrap()),
                    &new_approvers[2],
                )
                .unwrap(),
            )],
        )
        .await
        .unwrap_err();

    // Confirm the new change set is correct
    assert_eq!(
        &expected_new_set,
        kolme.read().get_framework_state().get_validator_set()
    );
    assert_eq!(
        kolme
            .read()
            .get_framework_state()
            .get_admin_proposal_state()
            .proposals
            .len(),
        0
    );
}
