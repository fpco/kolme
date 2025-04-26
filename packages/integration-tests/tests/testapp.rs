use anyhow::Result;
use futures_util::future::join_all;
use futures_util::StreamExt;
use kolme::{
    AccountId, AccountNonce, ApiServer, AssetId, BankMessage, BlockHeight, ExecutionContext,
    GenesisInfo, Kolme, KolmeApp, KolmeStore, MerkleDeserialize, MerkleDeserializer,
    MerkleSerialError, MerkleSerialize, MerkleSerializer, Message, Processor, Transaction,
};

use rust_decimal::dec;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use shared::cryptography::SecretKey;
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite};
use tokio_util::task::AbortOnDropHandle;

const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";

#[derive(Clone, Debug, Copy)]
struct TestApp;

#[derive(Serialize, Deserialize, Clone, Debug)]
struct TestState {
    #[serde(default)]
    counter: u32,
}
impl MerkleSerialize for TestState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        self.counter.merkle_serialize(serializer)
    }
}

impl MerkleDeserialize for TestState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let counter = u32::merkle_deserialize(deserializer)?;
        Ok(TestState { counter })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
enum TestMessage {
    Increment,
}

impl KolmeApp for TestApp {
    type State = TestState;
    type Message = TestMessage;

    fn genesis_info() -> GenesisInfo {
        let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();
        let my_public_key = secret.public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);

        GenesisInfo {
            kolme_ident: "Test framework".to_owned(),
            processor: my_public_key,
            listeners: set.clone(),
            needed_listeners: 1,
            approvers: set,
            needed_approvers: 1,
            chains: Default::default(),
        }
    }

    fn new_state() -> Result<Self::State> {
        Ok(TestState { counter: 0 })
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> Result<()> {
        match msg {
            TestMessage::Increment => {
                ctx.state_mut().counter += 1;
                Ok(())
            }
        }
    }
}

async fn find_free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    Ok(port)
}

async fn setup(
    db_path: &Path,
) -> Result<(Kolme<TestApp>, AbortOnDropHandle<Result<()>>, SocketAddr)> {
    let app = TestApp;
    let store = KolmeStore::new_sqlite(db_path).await?;
    let kolme = Kolme::new(app, "test_version", store).await?;
    let read = kolme.read().await;
    assert_eq!(read.get_next_height(), BlockHeight(0),);

    let addr = SocketAddr::new("127.0.0.1".parse()?, find_free_port().await?);
    let server_handle = AbortOnDropHandle::new(tokio::spawn({
        let kolme = kolme.clone();
        async move {
            let server = ApiServer::new(kolme);
            server.run(addr).await?;
            Ok(())
        }
    }));

    Ok((kolme, server_handle, addr))
}

async fn next_message_as_json<S>(ws_stream: &mut S) -> Result<Value, Box<dyn std::error::Error>>
where
    S: StreamExt<Item = Result<tungstenite::Message, tungstenite::Error>> + Unpin,
{
    let message = timeout(Duration::from_secs(5), ws_stream.next())
        .await?
        .ok_or("WebSocket stream terminated")??;

    let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
    tracing::info!("Received genesis notification: {}", notification);

    Ok(notification)
}

#[test_log::test(tokio::test)]
async fn test_websocket_notifications() {
    let db_path = NamedTempFile::new().unwrap();
    let (kolme, _server_handle, addr) = setup(db_path.path()).await.unwrap();
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();
    let processor = Processor::new(kolme.clone(), secret.clone());

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();
    tracing::info!("Connected to WebSocket");

    processor.create_genesis_event().await.unwrap();

    let _processor_handle = AbortOnDropHandle::new(tokio::spawn({
        let processor = Processor::new(kolme.clone(), secret.clone());
        async move { processor.run().await }
    }));

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["NewBlock"].is_object(),
        "Expected NewBlock notification for genesis, got: {}",
        notification
    );

    let tx = kolme
        .read()
        .await
        .create_signed_transaction(&secret, vec![Message::App(TestMessage::Increment)])
        .await
        .unwrap();

    kolme.propose_transaction(tx.clone()).unwrap();

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["Broadcast"].is_object(),
        "Expected Broadcast notification, got: {}",
        notification
    );

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["NewBlock"].is_object(),
        "Expected NewBlock notification for transaction, got: {}",
        notification
    );

    ws.close(None).await.unwrap();
    tracing::info!("WebSocket closed successfully");
}

#[test_log::test(tokio::test)]
async fn test_validate_tx_valid_signature() {
    let db_path = NamedTempFile::new().unwrap();
    let (kolme, _server_handle, addr) = setup(db_path.path()).await.unwrap();
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();
    let processor = Processor::new(kolme.clone(), secret.clone());

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();

    tracing::info!("Connected to WebSocket");

    processor.create_genesis_event().await.unwrap();
    let _processor_handle = AbortOnDropHandle::new(tokio::spawn({
        let processor = Processor::new(kolme.clone(), secret.clone());
        async move { processor.run().await }
    }));

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["NewBlock"].is_object(),
        "Expected NewBlock notification for genesis, got: {}",
        notification
    );

    let tx = kolme
        .read()
        .await
        .create_signed_transaction(&secret, vec![Message::App(TestMessage::Increment)])
        .await
        .unwrap();

    kolme.propose_transaction(tx.clone()).unwrap();

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["Broadcast"].is_object(),
        "Expected Broadcast notification, got: {}",
        notification
    );

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["NewBlock"].is_object(),
        "Expected NewBlock notification for transaction, got: {}",
        notification
    );

    ws.close(None).await.unwrap();
    tracing::info!("WebSocket closed successfully");
}

#[test_log::test(tokio::test)]
async fn test_execute_transaction_genesis() {
    let db_path = NamedTempFile::new().unwrap();
    let (kolme, _server_handle, addr) = setup(db_path.path()).await.unwrap();
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();
    let processor = Processor::new(kolme.clone(), secret.clone());

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();
    tracing::info!("Connected to WebSocket");

    processor.create_genesis_event().await.unwrap();

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["NewBlock"].is_object(),
        "Expected NewBlock notification, got: {}",
        notification
    );

    ws.close(None).await.unwrap();
    tracing::info!("WebSocket closed successfully");
}

#[test_log::test(tokio::test)]
async fn test_validate_tx_invalid_nonce() {
    let db_path = NamedTempFile::new().unwrap();
    let (kolme, _server_handle, addr) = setup(db_path.path()).await.unwrap();
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();
    let processor = Processor::new(kolme.clone(), secret.clone());

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();
    tracing::info!("Connected to WebSocket");

    processor.create_genesis_event().await.unwrap();

    let _processor_handle = AbortOnDropHandle::new(tokio::spawn({
        let processor = Processor::new(kolme.clone(), secret.clone());
        async move { processor.run().await }
    }));

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["NewBlock"].is_object(),
        "Expected NewBlock notification for genesis, got: {}",
        notification
    );

    let tx = Transaction {
        pubkey: secret.public_key(),
        nonce: AccountNonce(2),
        created: jiff::Timestamp::now(),
        messages: vec![Message::App(TestMessage::Increment)],
    };
    let signed_tx = tx.sign(&secret).unwrap();

    kolme.propose_transaction(signed_tx.clone()).unwrap();

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["Broadcast"].is_object(),
        "Expected Broadcast notification, got: {}",
        notification
    );

    let read = kolme.read().await;
    let state = read.get_app_state();
    assert_eq!(
        state.counter, 0,
        "Counter should remain 0 with invalid nonce, got: {}",
        state.counter
    );

    ws.close(None).await.unwrap();
    tracing::info!("WebSocket closed successfully");
}

#[test_log::test(tokio::test)]
async fn test_no_subscribers() {
    let db_path = NamedTempFile::new().unwrap();
    let (kolme, _server_handle, _) = setup(db_path.path()).await.unwrap();
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();

    let tx = kolme
        .read()
        .await
        .create_signed_transaction(&secret, vec![Message::App(TestMessage::Increment)])
        .await
        .unwrap();

    tracing::info!("Proposing transaction with no subscribers");
    let result = kolme.propose_transaction(tx.clone());

    assert!(
        result.is_err(),
        "Transaction should fail with no subscribers listening"
    );
    assert_eq!(
        result.unwrap_err().to_string(),
        "Tried to propose an event, but no one is listening to our notifications"
    );
}

#[test_log::test(tokio::test)]
async fn test_rejected_transaction_insufficient_balance() {
    let db_path = NamedTempFile::new().unwrap();
    let (kolme, _server_handle, addr) = setup(db_path.path()).await.unwrap();
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();
    let processor = Processor::new(kolme.clone(), secret.clone());

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();
    tracing::info!("Connected to WebSocket");

    processor.create_genesis_event().await.unwrap();
    let _processor_handle = AbortOnDropHandle::new(tokio::spawn({
        let processor = Processor::new(kolme.clone(), secret.clone());
        async move { processor.run().await }
    }));

    let notification = next_message_as_json(&mut ws).await.unwrap();
    assert!(
        notification["NewBlock"].is_object(),
        "Expected NewBlock notification for genesis, got: {}",
        notification
    );

    let tx_withdraw = kolme
        .read()
        .await
        .create_signed_transaction(
            &secret,
            vec![Message::Bank(BankMessage::Transfer {
                asset: AssetId(1),
                dest: AccountId::from(kolme::AccountId(0)),
                amount: dec!(500),
            })],
        )
        .await
        .unwrap();

    kolme.propose_transaction(tx_withdraw.clone()).unwrap();

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["Broadcast"].is_object(),
        "Expected Broadcast notification"
    );

    // Return an Error due Insufficient balance for account
    let result = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        result["FailedTransaction"].is_object(),
        "Expected FailedTransaction notification, but got: {:?}",
        result
    );

    let error = result["FailedTransaction"]["error"]
        .as_str()
        .expect("Expected error message in FailedTransaction");
    assert!(
        error.contains("Insufficient balance"),
        "Expected insufficient balance error, but got: {}",
        error
    );

    let read = kolme.read().await;
    let state = read.get_app_state();
    assert_eq!(
        state.counter, 0,
        "Counter should remain 0 with insufficient balance, got: {}",
        state.counter
    );

    ws.close(None).await.unwrap();
    tracing::info!("WebSocket closed successfully");
}

#[test_log::test(tokio::test)]
async fn test_many_transactions() {
    let db_path = NamedTempFile::new().unwrap();
    let (kolme, _server_handle, addr) = setup(db_path.path()).await.unwrap();
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();
    let processor = Processor::new(kolme.clone(), secret.clone());

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();
    tracing::info!("Connected to WebSocket");

    processor.create_genesis_event().await.unwrap();
    let _processor_handle = AbortOnDropHandle::new(tokio::spawn({
        let processor = Processor::new(kolme.clone(), secret.clone());
        async move { processor.run().await }
    }));

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["NewBlock"].is_object(),
        "Expected NewBlock notification for genesis, got: {}",
        notification
    );

    for i in 0..100 {
        let tx = kolme
            .read()
            .await
            .create_signed_transaction(&secret, vec![Message::App(TestMessage::Increment)])
            .await
            .unwrap();

        kolme.propose_transaction(tx.clone()).unwrap();

        let notification = next_message_as_json(&mut ws).await.unwrap();

        assert!(
            notification["Broadcast"].is_object(),
            "Expected Broadcast notification for tx {}, got: {}",
            i,
            notification
        );

        let notification = next_message_as_json(&mut ws).await.unwrap();

        assert!(
            notification["NewBlock"].is_object(),
            "Expected NewBlock notification for tx {}, got: {}",
            i,
            notification
        );
    }

    let read = kolme.read().await;
    let state = read.get_app_state();
    assert_eq!(
        state.counter, 100,
        "Counter should be 100 after 100 increments, got: {}",
        state.counter
    );

    ws.close(None).await.unwrap();
    tracing::info!("WebSocket closed successfully");
}

#[test_log::test(tokio::test)]
async fn test_concurrent_transactions() {
    let db_path = NamedTempFile::new().unwrap();
    let (kolme, _server_handle, addr) = setup(db_path.path()).await.unwrap();
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();
    let processor = Processor::new(kolme.clone(), secret.clone());

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();
    tracing::info!("Connected to WebSocket");

    processor.create_genesis_event().await.unwrap();
    let _processor_handle = AbortOnDropHandle::new(tokio::spawn({
        let processor = Processor::new(kolme.clone(), secret.clone());
        async move { processor.run().await }
    }));

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["NewBlock"].is_object(),
        "Expected NewBlock notification for genesis, got: {}",
        notification
    );

    // Generate 100 secret keys and each one will receive a nonce from Kolme.
    let mut rng = rand::rngs::ThreadRng::default();
    let mut secrets = Vec::with_capacity(100);
    for _ in 0..100 {
        let secret = SecretKey::random(&mut rng);
        secrets.push(secret);
    }

    let mut tasks = Vec::with_capacity(100);
    for secret in secrets {
        let kolme_clone = kolme.clone();

        let task = tokio::spawn(async move {
            let next_nonce = kolme_clone.read().await.get_next_nonce(secret.public_key());

            let tx = Transaction {
                pubkey: secret.public_key(),
                nonce: next_nonce,
                created: jiff::Timestamp::now(),
                messages: vec![Message::App(TestMessage::Increment)],
            };

            let signed_tx = tx.sign(&secret).unwrap();

            kolme_clone.propose_transaction(signed_tx).unwrap();
        });
        tasks.push(task);
    }

    join_all(tasks).await;

    let mut notifications = Vec::with_capacity(200);
    for _ in 0..200 {
        let notification = next_message_as_json(&mut ws).await.unwrap();
        notifications.push(notification);
    }

    let (broadcasts, new_blocks): (Vec<_>, Vec<_>) = notifications
        .into_iter()
        .partition(|n| n["Broadcast"].is_object());

    assert_eq!(
        broadcasts.len(),
        100,
        "Expected 100 Broadcast notifications, got: {}",
        broadcasts.len()
    );
    assert_eq!(
        new_blocks.len(),
        100,
        "Expected 100 NewBlock notifications, got: {}",
        new_blocks.len()
    );

    let read = kolme.read().await;
    let state = read.get_app_state();
    assert_eq!(
        state.counter, 100,
        "Counter should be 100 after 100 concurrent increments, got: {}",
        state.counter
    );

    ws.close(None).await.unwrap();
    tracing::info!("WebSocket closed successfully");
}
