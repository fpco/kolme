use anyhow::Result;
use futures_util::future::join_all;
use futures_util::StreamExt;
use kolme::{
    AccountNonce, ApiServer, AssetId, BankMessage, BlockHeight, ExecutionContext, ExternalChain,
    GenesisInfo, Kolme, KolmeApp, Message, Processor, Transaction, Wallet,
};
use rust_decimal::Decimal;
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

#[derive(Clone, Debug)]
struct TestApp;

#[derive(Serialize, Deserialize, Clone, Debug)]
struct TestState {
    #[serde(default)]
    counter: u32,
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

    fn save_state(state: &Self::State) -> Result<String> {
        serde_json::to_string(state).map_err(anyhow::Error::from)
    }

    fn load_state(v: &str) -> Result<Self::State> {
        serde_json::from_str(v).map_err(anyhow::Error::from)
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
) -> Result<(
    Kolme<TestApp>,
    tokio::task::JoinHandle<Result<()>>,
    SocketAddr,
)> {
    let app = TestApp;
    let kolme = Kolme::new(app, "test_version", db_path).await?;
    let read = kolme.read().await;
    assert_eq!(read.get_next_height(), BlockHeight(0),);

    let addr = SocketAddr::new("127.0.0.1".parse()?, find_free_port().await?);
    let server_handle = tokio::spawn({
        let kolme = kolme.clone();
        async move {
            let server = ApiServer::new(kolme);
            server.run(addr).await?;
            Ok(())
        }
    });

    Ok((kolme, server_handle, addr))
}

async fn next_message<S>(
    ws_stream: &mut S,
) -> Result<tungstenite::Message, Box<dyn std::error::Error>>
where
    S: StreamExt<Item = Result<tungstenite::Message, tungstenite::Error>> + Unpin,
{
    let message = timeout(Duration::from_secs(5), ws_stream.next())
        .await?
        .ok_or("WebSocket stream terminated")??;

    Ok(message)
}

#[test_log::test(tokio::test)]
async fn test_websocket_notifications() {
    let db_path = NamedTempFile::new().unwrap();
    let (kolme, server_handle, addr) = setup(db_path.path()).await.unwrap();
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

    let _server_handle = AbortOnDropHandle::new(server_handle);

    let message = next_message(&mut ws).await.unwrap();

    let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();

    tracing::info!("Received genesis notification: {}", notification);
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

    let message = next_message(&mut ws).await.unwrap();

    let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
    tracing::info!("Received notification: {}", notification);
    assert!(
        notification["Broadcast"].is_object(),
        "Expected Broadcast notification, got: {}",
        notification
    );

    let message = next_message(&mut ws).await.unwrap();

    let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
    tracing::info!("Received notification: {}", notification);
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
    let (kolme, server_handle, addr) = setup(db_path.path()).await.unwrap();
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

    let _server_handle = AbortOnDropHandle::new(server_handle);

    let message = next_message(&mut ws).await.unwrap();

    let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
    tracing::info!("\nReceived notification: {}\n", notification);
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

    let message = next_message(&mut ws).await.unwrap();

    let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
    tracing::info!("\nReceived notification: {}\n", notification);
    assert!(
        notification["Broadcast"].is_object(),
        "Expected Broadcast notification, got: {}",
        notification
    );

    let message = next_message(&mut ws).await.unwrap();

    let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
    tracing::info!("\nReceived notification: {}\n", notification);
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
    let (kolme, server_handle, addr) = setup(db_path.path()).await.unwrap();
    let _server_handle = AbortOnDropHandle::new(server_handle);
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();
    let processor = Processor::new(kolme.clone(), secret.clone());

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();
    tracing::info!("Connected to WebSocket");

    processor.create_genesis_event().await.unwrap();

    let message = next_message(&mut ws).await.unwrap();

    let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
    tracing::info!("Received notification: {}", notification);
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
    let (kolme, server_handle, addr) = setup(db_path.path()).await.unwrap();
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

    let _server_handle = AbortOnDropHandle::new(server_handle);

    let message = next_message(&mut ws).await.unwrap();

    let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
    tracing::info!("Received notification: {}", notification);
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

    let message = next_message(&mut ws).await.unwrap();

    let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
    tracing::info!("Received notification: {}", notification);
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
    let (kolme, server_handle, _) = setup(db_path.path()).await.unwrap();
    let _server_handle = AbortOnDropHandle::new(server_handle);
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
    let (kolme, server_handle, addr) = setup(db_path.path()).await.unwrap();
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

    let _server_handle = AbortOnDropHandle::new(server_handle);
    let message = next_message(&mut ws).await.unwrap();

    let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
    tracing::info!("Received notification: {}", notification);
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
            vec![Message::Bank(BankMessage::Withdraw {
                asset: AssetId(1),
                chain: ExternalChain::OsmosisTestnet,
                dest: Wallet(secret.public_key().to_string()),
                amount: Decimal::new(1000, 0),
            })],
        )
        .await
        .unwrap();

    kolme.propose_transaction(tx_withdraw.clone()).unwrap();

    let message = next_message(&mut ws).await.unwrap();

    let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
    tracing::info!("Received notification: {}", notification);
    assert!(
        notification["Broadcast"].is_object(),
        "Expected Broadcast notification for withdraw, got: {}",
        notification
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
    let (kolme, server_handle, addr) = setup(db_path.path()).await.unwrap();
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

    let _server_handle = AbortOnDropHandle::new(server_handle);

    let message = next_message(&mut ws).await.unwrap();

    let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
    tracing::info!("Received genesis notification: {}", notification);
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

        let message = next_message(&mut ws).await.unwrap();

        let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
        tracing::info!("Received notification for tx {}: {}", i, notification);
        assert!(
            notification["Broadcast"].is_object(),
            "Expected Broadcast notification for tx {}, got: {}",
            i,
            notification
        );

        let message = next_message(&mut ws).await.unwrap();

        let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
        tracing::info!("Received notification for tx {}: {}", i, notification);
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
    let (kolme, server_handle, addr) = setup(db_path.path()).await.unwrap();
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

    let _server_handle = AbortOnDropHandle::new(server_handle);

    let message = next_message(&mut ws).await.unwrap();

    let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
    tracing::info!("Received genesis notification: {}", notification);
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
            let read = kolme_clone.read().await;

            let next_nonce = read
                .get_account_and_next_nonce(secret.public_key())
                .await
                .unwrap()
                .next_nonce;

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

    for i in 0..200 {
        let message = next_message(&mut ws).await.unwrap();
        let notification: Value = serde_json::from_slice(&message.into_data()).unwrap();
        tracing::info!("Received notification {}: {}", i, notification);
        assert!(
            notification["Broadcast"].is_object() || notification["NewBlock"].is_object(),
            "Expected Broadcast or NewBlock notification for tx {}, got: {}",
            i,
            notification
        );
    }

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
