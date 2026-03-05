use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket},
        Path, State, WebSocketUpgrade,
    },
    response::IntoResponse,
    routing::{get, post},
    Json,
};
use futures_util::StreamExt;
use listener::get_next_bridge_event_id;
use reqwest::{header::CONTENT_TYPE, Method, StatusCode};
use serde::{Deserialize, Serialize};
use shared::types::KeyRegistration;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio_tungstenite::connect_async;
use tower_http::cors::{Any, CorsLayer};

use crate::*;

type Broadcast = tokio::sync::broadcast::Sender<BridgeEventMessage>;

#[derive(Clone)]
pub struct PassThrough {
    next_event_id: Arc<Mutex<BridgeEventId>>,
    notify: Broadcast,
    actions: Arc<RwLock<BTreeMap<BridgeActionId, Action>>>,
    latest_action: tokio::sync::watch::Sender<Option<BridgeActionId>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Msg {
    pub wallet: String,
    pub coins: Vec<BridgedAssetAmount>,
    pub msg: ExecuteMsg,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Transfer {
    pub bridge_action_id: BridgeActionId,
    pub recipient: Wallet,
    pub funds: Vec<AssetAmount>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Action {
    pub processor: SignatureWithRecovery,
    pub approvers: Vec<SignatureWithRecovery>,
    pub payload: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Regular {
        keys: Vec<KeyRegistration>,
    },
    Signed {
        /// Signature from the processor
        processor: SignatureWithRecovery,
        /// Signatures from the approvers
        approvers: Vec<SignatureWithRecovery>,
        /// The raw payload to execute
        ///
        /// This is a JSON encoding of [PayloadWithId]. We use a rendered
        /// String here to ensure identical binary representation
        /// so that the signatures will match.
        payload: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum BridgeEventMessage {
    Regular {
        wallet: String,
        funds: Vec<BridgedAssetAmount>,
        keys: Vec<PublicKey>,
    },
    Signed {
        wallet: String,
        action_id: BridgeActionId,
    },
}

pub async fn execute(
    client: reqwest::Client,
    port: u16,
    processor: SignatureWithRecovery,
    approvals: &BTreeMap<PublicKey, SignatureWithRecovery>,
    payload: &str,
) -> Result<String> {
    let url = format!("http://localhost:{port}/actions");
    tracing::debug!("Sending bridge action to {url}");
    let resp = client
        .post(url)
        .json(&Action {
            processor,
            approvers: approvals.values().copied().collect(),
            payload: payload.to_owned(),
        })
        .send()
        .await?;
    resp.error_for_status()?;
    Ok("no-tx-hash-for-pass-through".to_string())
}

impl PassThrough {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let (latest_action, _) = tokio::sync::watch::channel(None);
        Self {
            next_event_id: Arc::new(Mutex::new(BridgeEventId::start())),
            notify: tokio::sync::broadcast::channel(100).0,
            actions: Arc::new(RwLock::new(BTreeMap::new())),
            latest_action,
        }
    }

    pub async fn run<A: tokio::net::ToSocketAddrs>(self, addr: A) -> Result<()> {
        let cors = CorsLayer::new()
            .allow_methods([Method::GET, Method::POST, Method::PUT])
            .allow_origin(Any)
            .allow_headers([CONTENT_TYPE]);

        let app = axum::Router::new()
            .route("/msg", post(msg))
            .route("/notifications", get(ws_handler))
            .route("/actions", get(actions).post(new_action))
            .route("/actions/{bridge_action_id}", get(action))
            .route("/actions/{bridge_action_id}/wait", get(action_wait))
            .layer(cors)
            .with_state(self);

        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        tracing::info!(
            "Starting PassThrough server on {:?}",
            listener.local_addr()?
        );
        axum::serve(listener, app)
            .await
            .map_err(anyhow::Error::from)
    }
}

pub async fn listen<App: KolmeApp>(
    kolme: Kolme<App>,
    secret: SecretKey,
    port: String,
) -> Result<()> {
    tracing::debug!("pass through listen");
    let mut next_bridge_event_id = get_next_bridge_event_id(
        &kolme.read(),
        secret.public_key(),
        ExternalChain::PassThrough,
    );

    let ws_url = format!("ws://localhost:{}/notifications", port);
    tracing::debug!("Connecting to {ws_url}");
    let (mut ws, _) = connect_async(&ws_url).await.unwrap();

    loop {
        let message = ws.next().await.context("WebSocket stream terminated")??; //receiver.recv().await?;
        let message = serde_json::from_slice::<BridgeEventMessage>(&message.into_data())?;
        tracing::debug!("Received {}", serde_json::to_string(&message).unwrap());
        let message = to_kolme_message::<App::Message>(
            message,
            ExternalChain::PassThrough,
            next_bridge_event_id,
        );

        kolme
            .sign_propose_await_transaction(&secret, vec![message])
            .await?;

        next_bridge_event_id = next_bridge_event_id.next();
    }
}

fn to_kolme_message<T>(
    msg: BridgeEventMessage,
    chain: ExternalChain,
    event_id: BridgeEventId,
) -> Message<T> {
    match msg {
        BridgeEventMessage::Regular {
            wallet,
            funds,
            keys,
        } => Message::Listener {
            chain,
            event_id,
            event: BridgeEvent::Regular {
                wallet: Wallet(wallet),
                funds,
                keys,
            },
        },
        BridgeEventMessage::Signed { wallet, action_id } => Message::Listener {
            chain,
            event_id,
            event: BridgeEvent::Signed {
                wallet: Wallet(wallet),
                action_id,
            },
        },
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MsgResponse {
    pub bridge_event_id: BridgeEventId,
}

async fn msg(State(state): State<PassThrough>, Json(msg): Json<Msg>) -> impl IntoResponse {
    tracing::debug!("sending to kolme {msg:?}");

    let mut guard = state.next_event_id.lock().await;
    let message = match msg.msg {
        ExecuteMsg::Regular { keys } => BridgeEventMessage::Regular {
            wallet: msg.wallet,
            funds: msg.coins,
            keys: keys.into_iter().map(|x| x.key).collect(),
        },
        ExecuteMsg::Signed {
            processor: _,
            approvers: _,
            payload: _,
        } => todo!(),
    };

    state.notify.send(message).unwrap();
    let bridge_event_id = *guard;
    *guard = guard.next();

    Json(MsgResponse { bridge_event_id })
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<PassThrough>) -> impl IntoResponse {
    tracing::debug!("New WebSocket connection established");
    let rx = state.notify.subscribe();
    ws.on_upgrade(move |socket| handle_websocket(socket, rx))
}

async fn handle_websocket(mut socket: WebSocket, mut rx: broadcast::Receiver<BridgeEventMessage>) {
    tracing::debug!("WebSocket subscribed to message notifications");

    while let Ok(notification) = rx.recv().await {
        let msg = match serde_json::to_string(&notification) {
            Ok(json) => json,
            Err(e) => {
                format!("Failed to serialize notification to JSON: {}", e)
            }
        };

        if let Err(error) = socket.send(WsMessage::Text(msg.into())).await {
            tracing::warn!("Client disconnected with error: {}", error);
            break;
        }
        tracing::debug!("Notification sent to WebSocket client.");
    }
}

async fn new_action(
    State(state): State<PassThrough>,
    Json(action): Json<Action>,
) -> impl IntoResponse {
    tracing::debug!("new action to pass-through bridge: {action:?}");
    let Transfer {
        bridge_action_id, ..
    } = serde_json::from_str(&action.payload)
        .expect("payload is expected to contain Transfer serialized as JSON");
    let mut guard = state.actions.write().await;
    guard.insert(bridge_action_id, action);
    state.latest_action.send(Some(bridge_action_id)).ok();
}

async fn actions(State(state): State<PassThrough>) -> impl IntoResponse {
    let actions = state.actions.read().await.clone();
    Json(actions)
}

async fn action(
    State(state): State<PassThrough>,
    Path(bridge_action_id): Path<BridgeActionId>,
) -> impl IntoResponse {
    tracing::debug!("got request for action {bridge_action_id}");
    let actions = state.actions.read().await.clone();
    let action = actions.get(&bridge_action_id).cloned();
    Json(action)
}

async fn action_wait(
    State(state): State<PassThrough>,
    Path(bridge_action_id): Path<BridgeActionId>,
) -> axum::response::Response {
    let mut recv = state.latest_action.subscribe();
    let res = tokio::time::timeout(tokio::time::Duration::from_secs(30), async move {
        loop {
            tracing::debug!("got request for action {bridge_action_id}");
            let actions = state.actions.read().await.clone();
            match actions.get(&bridge_action_id) {
                Some(action) => break Json(action).into_response(),
                None => {
                    recv.changed().await.ok();
                }
            }
        }
    })
    .await;
    match res {
        Ok(res) => res,
        Err(_) => {
            let mut res = Json(serde_json::Value::Null).into_response();
            *res.status_mut() = StatusCode::NOT_FOUND;
            res
        }
    }
}
