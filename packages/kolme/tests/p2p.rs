use std::collections::BTreeSet;

use anyhow::Result;

use kolme::*;
use tokio::task::JoinSet;

/// In the future, move to an example and convert the binary to a library.
#[derive(Clone, Debug)]
pub struct SampleKolmeApp;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SampleState {
    #[serde(default)]
    hi_count: u32,
}

impl MerkleSerialize for SampleState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store(&self.hi_count)?;
        Ok(())
    }
}

impl MerkleDeserialize for SampleState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            hi_count: deserializer.load()?,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SampleMessage {
    SayHi {},
}

// Another keypair for client testing:
// Public key: 02c2b386e42945d4c11712a5bc1d20d085a7da63e57c214e2742a684a97d436599
// Secret key: 127831b9459b538eab9a338b1e96fc34249a5154c96180106dd87d39117e8e02

const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";

const DUMMY_CODE_VERSION: &str = "dummy code version";

fn my_secret_key() -> SecretKey {
    SecretKey::from_hex(SECRET_KEY_HEX).unwrap()
}

impl KolmeApp for SampleKolmeApp {
    type State = SampleState;
    type Message = SampleMessage;

    fn genesis_info() -> GenesisInfo {
        let my_public_key = my_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        GenesisInfo {
            kolme_ident: "p2p example".to_owned(),
            processor: my_public_key,
            listeners: set.clone(),
            needed_listeners: 1,
            approvers: set,
            needed_approvers: 1,
            chains: ConfiguredChains::default(),
        }
    }

    fn new_state() -> Result<Self::State> {
        Ok(SampleState { hi_count: 0 })
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> Result<()> {
        match msg {
            SampleMessage::SayHi {} => ctx.state_mut().hi_count += 1,
        }
        Ok(())
    }
}

#[tokio::test]
async fn sanity() {
    kolme::init_logger(false, None);
    let mut set = JoinSet::new();

    // We're going to run two logically separate Kolmes, using different databases.
    // The first will be the processor, the second will be a client.
    // Our goal is that when we propose transactions via the client,
    // the processor picks them up, creates a new block, and that new block
    // is picked up by the client.
    let tempfile_processor = tempfile::NamedTempFile::new().unwrap();
    let kolme_processor = Kolme::new(
        SampleKolmeApp,
        DUMMY_CODE_VERSION,
        KolmeStore::new_sqlite(tempfile_processor.path())
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    set.spawn(Processor::new(kolme_processor.clone(), my_secret_key().clone()).run());
    set.spawn(Gossip::new(kolme_processor).await.unwrap().run());

    let tempfile_client = tempfile::NamedTempFile::new().unwrap();
    let kolme_client = Kolme::new(
        SampleKolmeApp,
        DUMMY_CODE_VERSION,
        KolmeStore::new_sqlite(tempfile_client.path())
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    set.spawn(Gossip::new(kolme_client.clone()).await.unwrap().run());

    {
        let secret = SecretKey::random(&mut rand::thread_rng());
        kolme_client
            .wait_for_block(BlockHeight::start())
            .await
            .unwrap();
        let tx = kolme_client
            .read()
            .create_signed_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
            .await
            .unwrap();
        let txhash = tx.hash();
        kolme_client.propose_transaction(tx).unwrap();
        let block = tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            kolme_client.wait_for_block(BlockHeight::start().next()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(txhash, block.0.message.as_inner().tx.hash());
    }

    // And nothing else should have completed, since all other tasks should run forever.
    assert!(set.try_join_next().is_none());
}
