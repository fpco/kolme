use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket},
        Path, Query, State, WebSocketUpgrade,
    },
    http::header::CONTENT_TYPE,
    response::IntoResponse,
    routing::{get, put},
    Json,
};
use reqwest::{Method, StatusCode};
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};

use crate::*;

pub struct ApiServer<App: KolmeApp> {
    kolme: Kolme<App>,
}

impl<App: KolmeApp> ApiServer<App> {
    pub fn new(kolme: Kolme<App>) -> Self {
        ApiServer { kolme }
    }

    pub async fn run<A: tokio::net::ToSocketAddrs>(self, addr: A) -> Result<()> {
        let cors = CorsLayer::new()
            .allow_methods([Method::GET, Method::POST, Method::PUT])
            .allow_origin(Any)
            .allow_headers([CONTENT_TYPE]);

        let app = base_api_router().layer(cors).with_state(self.kolme);

        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        tracing::info!("Starting API server on {:?}", listener.local_addr()?);
        axum::serve(listener, app)
            .await
            .map_err(anyhow::Error::from)
    }
}

pub fn base_api_router<App: KolmeApp>() -> axum::Router<Kolme<App>> {
    axum::Router::new()
        .route("/", get(basics))
        .route("/broadcast", put(broadcast))
        .route("/get-next-nonce", get(get_next_nonce))
        .route("/block/{height}", get(get_block))
        .route("/only/block/{height}", get(get_only_block))
        .route("/notifications", get(ws_handler::<App>))
}

async fn basics<App: KolmeApp>(State(kolme): State<Kolme<App>>) -> impl IntoResponse {
    #[derive(serde::Serialize)]
    struct Basics<'a> {
        code_version: &'a String,
        chain_version: &'a String,
        next_height: BlockHeight,
        next_genesis_action: Option<GenesisAction>,
        bridges: BTreeMap<ExternalChain, &'a ChainConfig>,
        balances: BTreeMap<&'a AccountId, &'a BTreeMap<AssetId, Decimal>>,
    }

    let kolme = kolme.read();
    let basics = Basics {
        code_version: kolme.get_code_version(),
        chain_version: kolme.get_framework_state().get_chain_version(),
        next_height: kolme.get_next_height(),
        next_genesis_action: kolme.get_next_genesis_action(),
        bridges: kolme
            .get_bridge_contracts()
            .iter()
            .map(|(k, v)| (k, &v.config))
            .collect(),
        balances: kolme.get_balances().iter().collect(),
    };

    Json(basics).into_response()
}

async fn broadcast<App: KolmeApp>(
    State(kolme): State<Kolme<App>>,
    Json(tx): Json<SignedTransaction<App::Message>>,
) -> impl IntoResponse {
    let txhash = tx.0.message_hash();
    if let Err(e) = kolme
        .read()
        .execute_transaction(&tx, Timestamp::now(), BlockDataHandling::NoPriorData)
        .await
    {
        let mut res = e.to_string().into_response();
        *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        return res;
    }
    kolme.propose_transaction(Arc::new(tx));
    Json(serde_json::json!({"txhash":txhash})).into_response()
}

#[derive(serde::Deserialize)]
struct NextNonce {
    pubkey: PublicKey,
}

async fn get_next_nonce<App: KolmeApp>(
    State(kolme): State<Kolme<App>>,
    Query(NextNonce { pubkey }): Query<NextNonce>,
) -> impl IntoResponse {
    let nonce = kolme.read().get_next_nonce(pubkey);
    Json(serde_json::json!({"next_nonce":nonce})).into_response()
}

async fn get_only_block<App: KolmeApp>(
    State(kolme): State<Kolme<App>>,
    Path(height): Path<BlockHeight>,
) -> impl IntoResponse {
    #[derive(serde::Serialize)]
    struct Response<'a, App: KolmeApp> {
        blockhash: Sha256Hash,
        txhash: Sha256Hash,
        block: &'a SignedBlock<App::Message>,
        logs: &'a [Vec<String>],
    }

    match kolme.get_only_block(height).await {
        Ok(Some(block)) => {
            let resp: Response<'_, App> = Response {
                blockhash: block.blockhash,
                txhash: block.txhash,
                block: &block.block,
                logs: &block.logs
            };
            Json(resp).into_response()
        }
        Ok(None) => Json(serde_json::json!(null)).into_response(),
        Err(e) => {
            let mut res = e.to_string().into_response();
            *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            res
        }
    }
}

async fn get_block<App: KolmeApp>(
    State(kolme): State<Kolme<App>>,
    Path(height): Path<BlockHeight>,
) -> impl IntoResponse {
    #[derive(serde::Serialize)]
    struct Response<'a, App: KolmeApp> {
        blockhash: Sha256Hash,
        txhash: Sha256Hash,
        block: &'a SignedBlock<App::Message>,
        logs: &'a [Vec<String>],
    }

    match kolme.get_block(height).await {
        Ok(Some(block)) => {
            let resp: Response<'_, App> = Response {
                blockhash: block.blockhash,
                txhash: block.txhash,
                block: &block.block,
                logs: &block.logs,
            };

            Json(resp).into_response()
        }
        Ok(None) => Json(serde_json::json!(null)).into_response(),
        Err(e) => {
            let mut res = e.to_string().into_response();
            *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;

            res
        }
    }
}

async fn ws_handler<App: KolmeApp>(
    ws: WebSocketUpgrade,
    State(kolme): State<Kolme<App>>,
) -> impl IntoResponse {
    tracing::info!("New WebSocket connection established");
    let rx = kolme.subscribe();
    ws.on_upgrade(move |socket| handle_websocket::<App>(kolme, socket, rx))
}

async fn handle_websocket<App: KolmeApp>(
    kolme: Kolme<App>,
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<Notification<App::Message>>,
) {
    tracing::debug!("WebSocket subscribed to Kolme notifications");

    while let Ok(notification) = rx.recv().await {
        let notification = match notification {
            Notification::NewBlock(block) => {
                let height = block.height();
                let logs = match kolme.get_block(height).await {
                    Ok(Some(block)) => block.logs.clone(),
                    Ok(None) => {
                        tracing::error!(
                            "No information about logs for the awaited block at {height}"
                        );
                        break;
                    }
                    Err(e) => {
                        tracing::error!("Failed to get logs for block: {}", e);
                        break;
                    }
                };
                ApiNotification::NewBlock { block, logs }
            }
            Notification::GenesisInstantiation { chain, contract } => {
                ApiNotification::GenesisInstantiation { chain, contract }
            }
            Notification::FailedTransaction(failed) => ApiNotification::FailedTransaction(failed),
            Notification::LatestBlock(latest_block) => ApiNotification::LatestBlock(latest_block),
            Notification::EvictMempoolTransaction(txhash) => {
                ApiNotification::EvictMempoolTransaction(txhash)
            }
        };
        let msg = match serde_json::to_string(&notification) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::error!("Failed to serialize notification to JSON: {}", e);
                break;
            }
        };

        if let Err(error) = socket.send(WsMessage::Text(msg.into())).await {
            tracing::debug!("Client disconnected with error: {}", error);
            break;
        }
        tracing::debug!("Notification sent to WebSocket client.");
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(
    serialize = "",
    deserialize = "AppMessage: serde::de::DeserializeOwned"
))]
pub enum ApiNotification<AppMessage> {
    NewBlock {
        block: Arc<SignedBlock<AppMessage>>,
        logs: Arc<[Vec<String>]>,
    },
    /// A claim by a submitter that it has instantiated a bridge contract.
    GenesisInstantiation {
        chain: ExternalChain,
        contract: String,
    },
    /// A transaction failed in the processor.
    FailedTransaction(Arc<SignedTaggedJson<FailedTransaction>>),
    LatestBlock(Arc<SignedTaggedJson<LatestBlock>>),
    EvictMempoolTransaction(Arc<SignedTaggedJson<TxHash>>),
}
