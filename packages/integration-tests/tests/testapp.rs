use anyhow::{Context, Result};
use futures_util::StreamExt;
use futures_util::future::join_all;
use kolme::ApiNotification;
use kolme::{
    AccountNonce, ApiServer, AssetId, BankMessage, BlockHeight, ExecutionContext, GenesisInfo,
    Kolme, KolmeApp, KolmeStore, MerkleDeserialize, MerkleDeserializer, MerkleSerialError,
    MerkleSerialize, MerkleSerializer, Message, Processor, Transaction, ValidatorSet,
    testtasks::TestTasks,
};
use rust_decimal::dec;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use shared::cryptography::SecretKey;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use std::{collections::BTreeSet, sync::Arc};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Error;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite};

const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";

#[derive(Clone, Debug)]
struct TestApp {
    genesis: GenesisInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct TestState {
    #[serde(default)]
    counter: u32,
}
impl MerkleSerialize for TestState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store(&self.counter)
    }
}

impl MerkleDeserialize for TestState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        let counter = deserializer.load()?;
        Ok(TestState { counter })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
enum TestMessage {
    Increment,
}

impl Default for TestApp {
    fn default() -> Self {
        let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();
        let my_public_key = secret.public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);

        let genesis = GenesisInfo {
            kolme_ident: "Test framework".to_owned(),
            validator_set: ValidatorSet {
                processor: my_public_key,
                listeners: set.clone(),
                needed_listeners: 1,
                approvers: set,
                needed_approvers: 1,
            },
            chains: Default::default(),
            version: "v1".to_owned(),
        };

        Self { genesis }
    }
}

impl KolmeApp for TestApp {
    type State = TestState;
    type Message = TestMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
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

async fn setup_fjall(db_path: &Path) -> Result<(Kolme<TestApp>, SocketAddr)> {
    let app = TestApp::default();
    let store = KolmeStore::new_fjall(db_path)?;
    let code_version = app.genesis.version.clone();
    let kolme = Kolme::new(app, code_version, store).await?;
    let read = kolme.read();
    assert_eq!(read.get_next_height(), BlockHeight(0),);

    let addr = SocketAddr::new("127.0.0.1".parse()?, find_free_port().await?);

    Ok((kolme, addr))
}

async fn clear_postgres() {
    let url = std::env::var("PROCESSOR_BLOCK_DB").expect("PROCESSOR_BLOCK_DB variable missing");
    let pool = sqlx::PgPool::connect(&url).await.unwrap();

    sqlx::query("TRUNCATE TABLE blocks")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("TRUNCATE TABLE merkle_contents")
        .execute(&pool)
        .await
        .unwrap();
}

async fn setup_postgres() -> Result<(Kolme<TestApp>, SocketAddr)> {
    let app = TestApp::default();
    let url = std::env::var("PROCESSOR_BLOCK_DB").expect("PROCESSOR_BLOCK_DB variable missing");
    let store = KolmeStore::new_postgres(&url).await?;
    let code_version = app.genesis.version.clone();
    let kolme = Kolme::new(app, code_version, store).await?;

    let read = kolme.read();
    assert_eq!(read.get_next_height(), BlockHeight(0));

    let addr = SocketAddr::new("127.0.0.1".parse()?, find_free_port().await?);

    Ok((kolme, addr))
}

async fn next_message_as_json<S>(ws_stream: &mut S) -> Result<Value>
where
    S: StreamExt<Item = Result<tungstenite::Message, Error>> + Unpin,
{
    // Tests were written assuming some notifications don't yet exist.
    // Loop here is to strip that out.
    loop {
        let message = timeout(Duration::from_secs(5), ws_stream.next())
            .await?
            .context("WebSocket stream terminated")??;

        let notification: ApiNotification<TestMessage> =
            serde_json::from_slice(&message.into_data()).unwrap();
        if let ApiNotification::LatestBlock(_) = &notification {
            continue;
        }
        let notification = serde_json::to_value(notification)?;
        tracing::info!("Received genesis notification: {}", notification);

        break Ok(notification);
    }
}

#[tokio::test]
async fn test_websocket_notifications_fjall() {
    let db_path = tempfile::tempdir().unwrap();
    let (kolme, addr) = setup_fjall(db_path.path()).await.unwrap();
    TestTasks::start(test_websocket_notifications_inner, (kolme, addr)).await;
}

#[tokio::test]
async fn test_websocket_notifications_postgres() {
    let (kolme, addr) = setup_postgres().await.unwrap();
    TestTasks::start(test_websocket_notifications_inner, (kolme, addr)).await;
    clear_postgres().await;
}

async fn test_websocket_notifications_inner(
    testtasks: TestTasks,
    (kolme, addr): (Kolme<TestApp>, SocketAddr),
) {
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();

    let kolme_cloned = kolme.clone();
    testtasks.spawn_persistent(async move {
        let server = ApiServer::new(kolme_cloned.clone());
        server.run(addr).await.unwrap();
    });

    let kolme_cloned = kolme.clone();
    testtasks.try_spawn_persistent(Processor::new(kolme_cloned.clone(), secret.clone()).run());

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();
    tracing::info!("Connected to WebSocket");

    let tx = Arc::new(
        kolme
            .read()
            .create_signed_transaction(&secret, vec![Message::App(TestMessage::Increment)])
            .unwrap(),
    );

    kolme.propose_transaction(tx.clone());

    // Note we previously tested for a Broadcast notification, but those are no
    // longer emited via websockets.

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["NewBlock"].is_object(),
        "Expected NewBlock notification for transaction, got: {}",
        notification
    );

    ws.close(None).await.unwrap();
    tracing::info!("WebSocket closed successfully");
}

#[tokio::test]
#[serial_test::serial]
async fn test_validate_tx_valid_signature_fjall() {
    let db_path = tempfile::tempdir().unwrap();
    let (kolme, addr) = setup_fjall(db_path.path()).await.unwrap();
    TestTasks::start(test_validate_tx_valid_signature_inner, (kolme, addr)).await;
}

#[tokio::test]
#[serial_test::serial]
async fn test_validate_tx_valid_signature_postgres() {
    let (kolme, addr) = setup_postgres().await.unwrap();
    TestTasks::start(test_validate_tx_valid_signature_inner, (kolme, addr)).await;
    clear_postgres().await;
}

async fn test_validate_tx_valid_signature_inner(
    testtasks: TestTasks,
    (kolme, addr): (Kolme<TestApp>, SocketAddr),
) {
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();

    let kolme_cloned = kolme.clone();
    testtasks.spawn_persistent(async move {
        let server = ApiServer::new(kolme_cloned.clone());
        server.run(addr).await.unwrap();
    });

    let kolme_cloned = kolme.clone();
    testtasks.try_spawn_persistent(Processor::new(kolme_cloned.clone(), secret.clone()).run());

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();

    tracing::info!("Connected to WebSocket");

    let tx = Arc::new(
        kolme
            .read()
            .create_signed_transaction(&secret, vec![Message::App(TestMessage::Increment)])
            .unwrap(),
    );

    kolme.propose_transaction(tx.clone());

    // Note we previously tested for a Broadcast notification, but those are no
    // longer emited via websockets.

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["NewBlock"].is_object(),
        "Expected NewBlock notification for transaction, got: {}",
        notification
    );

    ws.close(None).await.unwrap();
    tracing::info!("WebSocket closed successfully");
}

#[tokio::test]
#[serial_test::serial]
async fn test_execute_transaction_genesis_fjall() {
    let db_path = tempfile::tempdir().unwrap();
    let (kolme, addr) = setup_fjall(db_path.path()).await.unwrap();
    TestTasks::start(
        test_execute_transaction_genesis_inner,
        (kolme, addr, |url| {
            Box::pin(async move {
                let (ws, _) = connect_async(url).await.unwrap();

                ws
            })
        }),
    )
    .await;
}

#[tokio::test]
#[serial_test::serial]
async fn test_execute_transaction_genesis_postgres() {
    let (kolme, addr) = setup_postgres().await.unwrap();
    TestTasks::start(
        test_execute_transaction_genesis_inner,
        (kolme, addr, |url| {
            Box::pin(async move {
                // TODO: Retries connection. Needed because postgres merkle setup takes a bit longer
                let (ws, _) = loop {
                    let result = connect_async(url).await;

                    if let Err(Error::ConnectionClosed) = &result {
                        continue;
                    }

                    break result.unwrap();
                };
                ws
            })
        }),
    )
    .await;
    clear_postgres().await;
}

type Connection =
    for<'a> fn(
        &'a str,
    ) -> futures::future::BoxFuture<'a, WebSocketStream<MaybeTlsStream<TcpStream>>>;

async fn test_execute_transaction_genesis_inner(
    testtasks: TestTasks,
    (kolme, addr, connect): (Kolme<TestApp>, SocketAddr, Connection),
) {
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();

    let kolme_cloned = kolme.clone();
    testtasks.spawn_persistent(async move {
        let server = ApiServer::new(kolme_cloned.clone());
        server.run(addr).await.unwrap();
    });

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let mut ws = connect(&ws_url).await;
    tracing::info!("Connected to WebSocket");

    let kolme_cloned = kolme.clone();
    testtasks.try_spawn_persistent(Processor::new(kolme_cloned.clone(), secret.clone()).run());

    let notification = next_message_as_json(&mut ws).await.unwrap();

    assert!(
        notification["NewBlock"].is_object(),
        "Expected NewBlock notification, got: {}",
        notification
    );

    ws.close(None).await.unwrap();
    tracing::info!("WebSocket closed successfully");
}

#[tokio::test]
#[serial_test::serial]
async fn test_validate_tx_invalid_nonce_fjall() {
    let db_path = tempfile::tempdir().unwrap();
    let (kolme, addr) = setup_fjall(db_path.path()).await.unwrap();
    TestTasks::start(test_validate_tx_invalid_nonce_inner, (kolme, addr)).await;
}

#[tokio::test]
#[serial_test::serial]
async fn test_validate_tx_invalid_nonce_postgres() {
    let (kolme, addr) = setup_postgres().await.unwrap();
    TestTasks::start(test_validate_tx_invalid_nonce_inner, (kolme, addr)).await;
    clear_postgres().await;
}

async fn test_validate_tx_invalid_nonce_inner(
    testtasks: TestTasks,
    (kolme, addr): (Kolme<TestApp>, SocketAddr),
) {
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();

    testtasks.try_spawn_persistent(ApiServer::new(kolme.clone()).run(addr));
    testtasks.try_spawn_persistent(Processor::new(kolme.clone(), secret.clone()).run());

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();
    tracing::info!("Connected to WebSocket");

    let tx = Transaction {
        pubkey: secret.public_key(),
        nonce: AccountNonce(2),
        created: jiff::Timestamp::now(),
        messages: vec![Message::App(TestMessage::Increment)],
        max_height: None,
    };
    let signed_tx = Arc::new(tx.sign(&secret).unwrap());

    kolme.propose_transaction(signed_tx.clone());

    let read = kolme.read();
    let state = read.get_app_state();
    assert_eq!(
        state.counter, 0,
        "Counter should remain 0 with invalid nonce, got: {}",
        state.counter
    );

    ws.close(None).await.unwrap();
    tracing::info!("WebSocket closed successfully");
}

#[tokio::test]
#[serial_test::serial]
async fn test_rejected_transaction_insufficient_balance_fjall() {
    let db_path = tempfile::tempdir().unwrap();
    let (kolme, addr) = setup_fjall(db_path.path()).await.unwrap();
    TestTasks::start(
        test_rejected_transaction_insufficient_balance_inner,
        (kolme, addr),
    )
    .await;
}

#[tokio::test]
#[serial_test::serial]
async fn test_rejected_transaction_insufficient_balance_postgres() {
    let (kolme, addr) = setup_postgres().await.unwrap();
    TestTasks::start(
        test_rejected_transaction_insufficient_balance_inner,
        (kolme, addr),
    )
    .await;
    clear_postgres().await;
}

async fn test_rejected_transaction_insufficient_balance_inner(
    testtasks: TestTasks,
    (kolme, addr): (Kolme<TestApp>, SocketAddr),
) {
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();

    testtasks.try_spawn_persistent(ApiServer::new(kolme.clone()).run(addr));
    testtasks.try_spawn_persistent(Processor::new(kolme.clone(), secret.clone()).run());

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();
    tracing::info!("Connected to WebSocket");

    let tx_withdraw = Arc::new(
        kolme
            .read()
            .create_signed_transaction(&secret, vec![Message::Bank(BankMessage::Transfer {
                asset: AssetId(1),
                dest: kolme::AccountId(0),
                amount: dec!(500),
            })])
            .unwrap(),
    );

    kolme.propose_transaction(tx_withdraw.clone());

    let read = kolme.read();
    let state = read.get_app_state();
    assert_eq!(
        state.counter, 0,
        "Counter should remain 0 with insufficient balance, got: {}",
        state.counter
    );

    ws.close(None).await.unwrap();
    tracing::info!("WebSocket closed successfully");
}

#[tokio::test]
#[serial_test::serial]
async fn test_many_transactions_fjall() {
    let db_path = tempfile::tempdir().unwrap();
    let (kolme, addr) = setup_fjall(db_path.path()).await.unwrap();
    TestTasks::start(test_many_transactions_inner, (kolme, addr, no_poll)).await;
}

#[tokio::test]
#[serial_test::serial]
async fn test_many_transactions_postgres() {
    let (kolme, addr) = setup_postgres().await.unwrap();
    TestTasks::start(test_many_transactions_inner, (kolme, addr, wait_for_nonce)).await;
    clear_postgres().await;
}

type NoncePoller = for<'a> fn(
    AccountNonce,
    &'a Kolme<TestApp>,
    &'a SecretKey,
) -> futures::future::BoxFuture<'a, ()>;

fn no_poll<'a>(
    _: AccountNonce,
    _: &'a Kolme<TestApp>,
    _: &'a SecretKey,
) -> futures::future::BoxFuture<'a, ()> {
    Box::pin(async move {})
}

fn wait_for_nonce<'a>(
    nonce: AccountNonce,
    kolme: &'a Kolme<TestApp>,
    secret: &'a SecretKey,
) -> futures::future::BoxFuture<'a, ()> {
    Box::pin(async move {
        loop {
            let account_nonce = kolme.read().get_next_nonce(secret.public_key());
            if account_nonce > nonce {
                tracing::debug!("Got {} new account nonce", account_nonce);
                return;
            }

            tokio::task::yield_now().await;
        }
    })
}

async fn test_many_transactions_inner(
    testtasks: TestTasks,
    (kolme, addr, poller): (Kolme<TestApp>, SocketAddr, NoncePoller),
) {
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();

    testtasks.try_spawn_persistent(ApiServer::new(kolme.clone()).run(addr));
    testtasks.try_spawn_persistent(Processor::new(kolme.clone(), secret.clone()).run());

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();
    tracing::info!("Connected to WebSocket");

    // FIXME I think that having the need for this wait_for_nonce on this kind of tests
    // is a sympton that we're doing something wrong, possibly the nonce is not getting updated before sending the websocket
    poller(AccountNonce::start(), &kolme, &secret).await;

    for i in 0..100 {
        let tx = Arc::new(
            kolme
                .read()
                .create_signed_transaction(&secret, vec![Message::App(TestMessage::Increment)])
                .unwrap(),
        );

        kolme.propose_transaction(tx.clone());

        // Note we previously tested for a Broadcast notification, but those are no
        // longer emited via websockets.

        let notification = next_message_as_json(&mut ws).await.unwrap();

        assert!(
            notification["NewBlock"].is_object(),
            "Expected NewBlock notification for tx {}, got: {}",
            i,
            notification
        );

        // FIXME same as above
        poller(tx.0.message.as_inner().nonce, &kolme, &secret).await;
    }

    let read = kolme.read();
    let state = read.get_app_state();
    assert_eq!(
        state.counter, 100,
        "Counter should be 100 after 100 increments, got: {}",
        state.counter
    );

    ws.close(None).await.unwrap();
    tracing::info!("WebSocket closed successfully");
}

#[tokio::test]
#[serial_test::serial]
async fn test_concurrent_transactions_fjall() {
    let db_path = tempfile::tempdir().unwrap();
    let (kolme, addr) = setup_fjall(db_path.path()).await.unwrap();
    TestTasks::start(test_concurrent_transactions_inner, (kolme, addr)).await;
}

#[tokio::test]
#[serial_test::serial]
async fn test_concurrent_transactions_postgres() {
    let (kolme, addr) = setup_postgres().await.unwrap();
    TestTasks::start(test_concurrent_transactions_inner, (kolme, addr)).await;
    clear_postgres().await;
}

async fn test_concurrent_transactions_inner(
    testtasks: TestTasks,
    (kolme, addr): (Kolme<TestApp>, SocketAddr),
) {
    let secret = SecretKey::from_hex(SECRET_KEY_HEX).unwrap();

    testtasks.try_spawn_persistent(ApiServer::new(kolme.clone()).run(addr));
    testtasks.try_spawn_persistent(Processor::new(kolme.clone(), secret.clone()).run());

    let ws_url = format!("ws://localhost:{}/notifications", addr.port());
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();
    tracing::info!("Connected to WebSocket");

    let mut secrets = Vec::with_capacity(50);
    for _ in 0..50 {
        let secret = SecretKey::random(&mut rand::rngs::ThreadRng::default());
        secrets.push(secret);
    }

    let mut tasks = Vec::with_capacity(50);
    for secret in secrets {
        let kolme_clone = kolme.clone();

        let task = tokio::spawn(async move {
            let next_nonce = kolme_clone.read().get_next_nonce(secret.public_key());

            let tx = Transaction {
                pubkey: secret.public_key(),
                nonce: next_nonce,
                created: jiff::Timestamp::now(),
                messages: vec![Message::App(TestMessage::Increment)],
                max_height: None,
            };

            let signed_tx = Arc::new(tx.sign(&secret).unwrap());
            kolme_clone
                .propose_and_await_transaction(signed_tx)
                .await
                .unwrap();
        });
        tasks.push(task);
    }

    join_all(tasks).await;

    let read = kolme.read();
    let state = read.get_app_state();
    assert_eq!(
        state.counter, 50,
        "Counter should be 50 after 50 concurrent increments, got: {}",
        state.counter
    );

    ws.close(None).await.unwrap();
    tracing::info!("WebSocket closed successfully");
}
