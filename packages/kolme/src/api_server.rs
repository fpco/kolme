use std::time::Duration;

use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket},
        Path, Query, State, WebSocketUpgrade,
    },
    http::header::CONTENT_TYPE,
    response::{IntoResponse, Response},
    routing::{get, put},
    Json, Router,
};
use reqwest::{Method, StatusCode};
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};

use crate::*;

pub use axum;

pub struct ApiServer<App: KolmeApp> {
    kolme: Kolme<App>,
    extra_routes: Option<Router>,
}

impl<App: KolmeApp> ApiServer<App> {
    pub fn new(kolme: Kolme<App>) -> Self {
        ApiServer {
            kolme,
            extra_routes: None,
        }
    }

    /// Add in extra routes to be served.
    pub fn with_extra_routes(mut self, extra_routes: Router) -> Self {
        self.extra_routes = Some(extra_routes);
        self
    }

    pub async fn run<A: tokio::net::ToSocketAddrs>(self, addr: A) -> Result<()> {
        let cors = CorsLayer::new()
            .allow_methods([Method::GET, Method::POST, Method::PUT])
            .allow_origin(Any)
            .allow_headers([CONTENT_TYPE]);

        let mut app = base_api_router().with_state(self.kolme);
        if let Some(extra_routes) = self.extra_routes {
            app = app.merge(extra_routes);
        }
        app = app.layer(cors);

        let listener = tokio::net::TcpListener::bind(addr).await?;
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
        .route("/notifications", get(ws_handler::<App>))
        .route("/account-id/wallet/{wallet}", get(account_id_for_wallet))
        .route("/account-id/pubkey/{wallet}", get(account_id_for_pubkey))
        .route("/account-id/{account_id}", get(account_id))
        .route("/healthz", get(healthz))
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

async fn healthz() -> &'static str {
    "Healthy!"
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
    let (account_id, nonce) = kolme.read().get_next_nonce(pubkey);
    Json(serde_json::json!({"next_nonce":nonce, "account_id":account_id})).into_response()
}

async fn get_block<App: KolmeApp>(
    State(kolme): State<Kolme<App>>,
    Path(height): Path<BlockHeight>,
) -> impl IntoResponse {
    get_block_inner(&kolme, height).await.map_err(|e| {
        let mut res = e.to_string().into_response();
        *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        res
    })
}

async fn get_block_inner<App: KolmeApp>(
    kolme: &Kolme<App>,
    height: BlockHeight,
) -> Result<Response, anyhow::Error> {
    #[derive(serde::Serialize)]
    struct Response<'a, App: KolmeApp> {
        blockhash: Sha256Hash,
        txhash: Sha256Hash,
        block: &'a SignedBlock<App::Message>,
        logs: &'a [Vec<String>],
    }

    let Some(block) = kolme.get_block(height).await? else {
        return Ok(Json(serde_json::json!(null)).into_response());
    };
    let logs = kolme
        .get_merkle_by_hash::<Vec<_>>(block.block.0.message.as_inner().logs)
        .await?;
    let resp: Response<'_, App> = Response {
        blockhash: block.blockhash,
        txhash: block.txhash,
        block: &block.block,
        logs: &logs,
    };

    Ok(Json(resp).into_response())
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
    let mut interval = tokio::time::interval(Duration::from_secs(30));

    enum Action<AppMessage> {
        Ping,
        Notification(Notification<AppMessage>),
    }

    loop {
        let action = tokio::select! {
            _ = interval.tick() => Ok(Action::Ping),
            res = rx.recv() => res.map(Action::Notification),
        };

        let action = match action {
            Ok(action) => action,
            Err(e) => {
                tracing::error!("API server websockets: error on receiving notification: {e}");
                break;
            }
        };

        let msg = match action {
            Action::Ping => {
                tracing::debug!("Sending ping");
                axum::extract::ws::Message::Ping(Vec::new().into())
            }
            Action::Notification(notification) => {
                let api_notification = match to_api_notification(&kolme, notification).await {
                    Ok(api_notification) => api_notification,
                    Err(e) => {
                        tracing::error!("API server websockets: Error converting Notification to ApiNotification: {e}");
                        break;
                    }
                };
                let msg = match serde_json::to_string(&api_notification) {
                    Ok(msg) => msg,
                    Err(e) => {
                        tracing::error!("API server websockets: error serializing API notification to string: {e}");
                        break;
                    }
                };
                WsMessage::Text(msg.into())
            }
        };
        let res = socket.send(msg).await;

        if let Err(e) = res {
            tracing::warn!("Client connection closed, stopping pings: {e}");
            break;
        }
        tracing::debug!("Notification sent to WebSocket client.");
    }
}

async fn to_api_notification<App: KolmeApp>(
    kolme: &Kolme<App>,
    notification: Notification<App::Message>,
) -> Result<ApiNotification<App::Message>> {
    match notification {
        Notification::NewBlock(block) => {
            let height = block.height();
            let logs = match kolme.get_block(height).await {
                Ok(Some(block)) => {
                    match kolme
                        .get_merkle_by_hash(block.block.0.message.as_inner().logs)
                        .await
                    {
                        Ok(logs) => logs,
                        Err(e) => {
                            anyhow::bail!("No logs found in Merkle store for block {height}: {e}");
                        }
                    }
                }
                Ok(None) => {
                    anyhow::bail!("No information about logs for the awaited block at {height}")
                }
                Err(e) => {
                    anyhow::bail!("Failed to get logs for block: {}", e);
                }
            };
            Ok(ApiNotification::NewBlock { block, logs })
        }
        Notification::GenesisInstantiation { chain, contract } => {
            Ok(ApiNotification::GenesisInstantiation { chain, contract })
        }
        Notification::FailedTransaction(failed) => Ok(ApiNotification::FailedTransaction(failed)),
        Notification::LatestBlock(latest_block) => Ok(ApiNotification::LatestBlock(latest_block)),
        Notification::EvictMempoolTransaction(txhash) => {
            Ok(ApiNotification::EvictMempoolTransaction(txhash))
        }
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

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountIdResp {
    NotFound {},
    Found { account_id: AccountId },
}

#[derive(serde::Deserialize)]
struct AccountIdQuery {
    timeout: Option<u64>,
}

impl AccountIdQuery {
    fn duration(&self) -> tokio::time::Duration {
        tokio::time::Duration::from_millis(self.timeout.unwrap_or(5000))
    }
}

async fn account_id_for_wallet<App: KolmeApp>(
    State(kolme): State<Kolme<App>>,
    Path(wallet): Path<Wallet>,
    Query(timeout): Query<AccountIdQuery>,
) -> Result<Json<AccountIdResp>, axum::response::Response> {
    async {
        tokio::time::timeout(timeout.duration(), kolme.wait_account_for_wallet(&wallet))
            .await
            .ok()
            .transpose()
            .map(|res| match res {
                Some(account_id) => AccountIdResp::Found { account_id },
                None => AccountIdResp::NotFound {},
            })
    }
    .await
    .map(Json)
    .map_err(|e| {
        let mut res = e.to_string().into_response();
        *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        res
    })
}

async fn account_id_for_pubkey<App: KolmeApp>(
    State(kolme): State<Kolme<App>>,
    Path(pubkey): Path<PublicKey>,
    Query(timeout): Query<AccountIdQuery>,
) -> Result<Json<AccountIdResp>, axum::response::Response> {
    async {
        tokio::time::timeout(timeout.duration(), kolme.wait_account_for_key(pubkey))
            .await
            .ok()
            .transpose()
            .map(|res| match res {
                Some(account_id) => AccountIdResp::Found { account_id },
                None => AccountIdResp::NotFound {},
            })
    }
    .await
    .map(Json)
    .map_err(|e| {
        let mut res = e.to_string().into_response();
        *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        res
    })
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountResp {
    NotFound {},
    Found {
        wallets: BTreeSet<Wallet>,
        pubkeys: BTreeSet<PublicKey>,
    },
}

async fn account_id<App: KolmeApp>(
    State(kolme): State<Kolme<App>>,
    Path(account_id): Path<AccountId>,
) -> Json<AccountResp> {
    Json(
        kolme
            .read()
            .get_framework_state()
            .get_accounts()
            .get_account(account_id)
            .map_or(AccountResp::NotFound {}, |account| AccountResp::Found {
                wallets: account.get_wallets().clone(),
                pubkeys: account.get_pubkeys().clone(),
            }),
    )
}
