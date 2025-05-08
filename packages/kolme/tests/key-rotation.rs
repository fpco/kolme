use std::{collections::BTreeSet, sync::OnceLock};

use kolme::*;
use testtasks::TestTasks;

/// In the future, move to an example and convert the binary to a library.
#[derive(Clone)]
pub struct SampleKolmeApp {
    pub genesis: GenesisInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
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
            processor: validator,
            listeners: set.clone(),
            needed_listeners: 1,
            approvers: set,
            needed_approvers: 1,
            chains: ConfiguredChains::default(),
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
            vec![Message::KeyRotation(KeyRotationMessage::SelfReplace {
                validator_type: ValidatorType::Listener,
                replacement: fake_validator.public_key(),
            })],
        )
        .await
        .unwrap();

    // Check that the new genesis info is updated correctly
    {
        let kolme = kolme.read();
        let config = kolme.get_framework_state().get_config();
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
            vec![Message::KeyRotation(KeyRotationMessage::SelfReplace {
                validator_type: ValidatorType::Processor,
                replacement: secret2.public_key(),
            })],
        )
        .await
        .unwrap();
    {
        let kolme = kolme.read();
        let config = kolme.get_framework_state().get_config();
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
    let tx = kolme
        .read()
        .create_signed_transaction(&client, vec![Message::App(SampleMessage::SayHi)])
        .unwrap();
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
