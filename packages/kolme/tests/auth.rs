use std::{collections::BTreeSet, sync::OnceLock};

use jiff::Timestamp;
use tokio::task::JoinSet;

use kolme::*;

/// In the future, move to an example and convert the binary to a library.
#[derive(Clone, Debug)]
pub struct SampleKolmeApp;

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

pub fn get_sample_secret_key() -> &'static SecretKey {
    static KEY: OnceLock<SecretKey> = OnceLock::new();
    let mut rng = rand::thread_rng();
    KEY.get_or_init(|| SecretKey::random(&mut rng))
}

const DUMMY_CODE_VERSION: &str = "dummy code version";

impl KolmeApp for SampleKolmeApp {
    type State = SampleState;
    type Message = SampleMessage;

    fn genesis_info() -> GenesisInfo {
        let my_public_key = get_sample_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        GenesisInfo {
            kolme_ident: "Dev code".to_owned(),
            processor: my_public_key,
            listeners: set.clone(),
            needed_listeners: 1,
            approvers: set,
            needed_approvers: 1,
            chains: ConfiguredChains::default(),
        }
    }

    fn new_state() -> anyhow::Result<Self::State> {
        Ok(SampleState {})
    }

    async fn execute(
        &self,
        _ctx: &mut ExecutionContext<'_, Self>,
        _msg: &Self::Message,
    ) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("execute not implemented"))
    }
}

#[tokio::test]
async fn test_sample_sanity() {
    let tempfile = tempfile::NamedTempFile::new().unwrap();

    let kolme = Kolme::new(SampleKolmeApp, DUMMY_CODE_VERSION, tempfile.path())
        .await
        .unwrap();

    let mut subscription = kolme.subscribe();

    let mut set = JoinSet::new();
    set.spawn(Processor::new(kolme.clone(), get_sample_secret_key().clone(), None).run());
    subscription.recv().await.unwrap();

    let mut rng = rand::thread_rng();
    let secret1 = SecretKey::random(&mut rng);
    let secret2 = SecretKey::random(&mut rng);
    let secret3 = SecretKey::random(&mut rng);

    let perform_many = |signer: &SecretKey, msgs: Vec<AuthMessage>| {
        let signer = signer.clone();
        let kolme = kolme.clone();
        async move {
            let tx = kolme
                .read()
                .await
                .create_signed_transaction(&signer, msgs.into_iter().map(Message::Auth).collect())
                .await?;
            kolme
                .read()
                .await
                .execute_transaction(&tx, Timestamp::now(), None)
                .await?;
            let mut subscribe = kolme.subscribe();
            let next_height = kolme.read().await.get_next_height();
            kolme.propose_transaction(tx)?;
            loop {
                match subscribe.recv().await? {
                    Notification::NewBlock(block) => {
                        if block.0.message.as_inner().height == next_height {
                            break anyhow::Ok(());
                        }
                    }
                    Notification::GenesisInstantiation { .. } => (),
                    Notification::Broadcast { .. } => (),
                    Notification::FailedTransaction { .. } => (),
                }
            }
        }
    };

    let perform = |signer: &SecretKey, msg: AuthMessage| perform_many(signer, vec![msg]);

    // Add secret2 to secret1's account, this should work
    perform(
        &secret1,
        AuthMessage::AddPublicKey {
            key: secret2.public_key(),
        },
    )
    .await
    .unwrap();

    // But trying to do it a second time will fail because the key is already present.
    for signer in [&secret1, &secret2, &secret3] {
        perform(
            signer,
            AuthMessage::AddPublicKey {
                key: secret2.public_key(),
            },
        )
        .await
        .unwrap_err();
    }

    // Add a wallet to a new account
    perform(
        &secret3,
        AuthMessage::AddWallet {
            wallet: "deadbeef".to_owned(),
        },
    )
    .await
    .unwrap();

    // And now we can't add this wallet to any other account.
    for signer in [&secret1, &secret2, &secret3] {
        perform(
            signer,
            AuthMessage::AddWallet {
                wallet: "deadbeef".to_owned(),
            },
        )
        .await
        .unwrap_err();
    }

    // Now that we have a new account for secret3, we can't add it to secret1
    perform(
        &secret1,
        AuthMessage::AddPublicKey {
            key: secret3.public_key(),
        },
    )
    .await
    .unwrap_err();

    // Removing the wallet from secret3 works the first time, fails the second.
    perform(
        &secret3,
        AuthMessage::RemoveWallet {
            wallet: "deadbeef".to_owned(),
        },
    )
    .await
    .unwrap();
    perform(
        &secret3,
        AuthMessage::RemoveWallet {
            wallet: "deadbeef".to_owned(),
        },
    )
    .await
    .unwrap_err();

    // And then we can add this wallet to the secret1 account.
    perform(
        &secret1,
        AuthMessage::AddWallet {
            wallet: "deadbeef".to_owned(),
        },
    )
    .await
    .unwrap();
    perform(
        &secret1,
        AuthMessage::AddWallet {
            wallet: "deadbeef".to_owned(),
        },
    )
    .await
    .unwrap_err();
    perform(
        &secret3,
        AuthMessage::AddWallet {
            wallet: "deadbeef".to_owned(),
        },
    )
    .await
    .unwrap_err();

    // Cannot remove a key while using that key
    perform(
        &secret1,
        AuthMessage::RemovePublicKey {
            key: secret1.public_key(),
        },
    )
    .await
    .unwrap_err();
    perform(
        &secret2,
        AuthMessage::RemovePublicKey {
            key: secret1.public_key(),
        },
    )
    .await
    .unwrap();

    // And now we can reuse secret1 on the secret3 account
    perform(
        &secret3,
        AuthMessage::AddPublicKey {
            key: secret1.public_key(),
        },
    )
    .await
    .unwrap();
    perform(
        &secret3,
        AuthMessage::AddPublicKey {
            key: secret1.public_key(),
        },
    )
    .await
    .unwrap_err();

    // And now confirm that intermediate state transitions are respected
    perform_many(
        &secret3,
        vec![
            AuthMessage::RemovePublicKey {
                key: secret1.public_key(),
            },
            AuthMessage::AddPublicKey {
                key: secret1.public_key(),
            },
            AuthMessage::RemovePublicKey {
                key: secret1.public_key(),
            },
            AuthMessage::AddPublicKey {
                key: secret1.public_key(),
            },
        ],
    )
    .await
    .unwrap();
    perform_many(
        &secret3,
        vec![
            AuthMessage::RemovePublicKey {
                key: secret1.public_key(),
            },
            AuthMessage::AddPublicKey {
                key: secret1.public_key(),
            },
            AuthMessage::AddPublicKey {
                key: secret1.public_key(),
            },
        ],
    )
    .await
    .unwrap_err();
    perform_many(
        &secret3,
        vec![
            AuthMessage::AddWallet {
                wallet: "foobar".to_owned(),
            },
            AuthMessage::RemoveWallet {
                wallet: "foobar".to_owned(),
            },
            AuthMessage::AddWallet {
                wallet: "foobar".to_owned(),
            },
            AuthMessage::RemoveWallet {
                wallet: "foobar".to_owned(),
            },
        ],
    )
    .await
    .unwrap();
    perform_many(
        &secret3,
        vec![
            AuthMessage::AddWallet {
                wallet: "foobar".to_owned(),
            },
            AuthMessage::RemoveWallet {
                wallet: "foobar".to_owned(),
            },
            AuthMessage::RemoveWallet {
                wallet: "foobar".to_owned(),
            },
        ],
    )
    .await
    .unwrap_err();
}
