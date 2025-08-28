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
    match broadcast_inner(kolme, tx).await {
        Ok(txhash) => Json(serde_json::json!({"txhash": txhash})).into_response(),
        Err(e) => {
            let mut res = e.to_string().into_response();
            *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            res
        }
    }
}

async fn broadcast_inner<App: KolmeApp>(
    kolme: Kolme<App>,
    tx: SignedTransaction<App::Message>,
) -> Result<TxHash> {
    let txhash = tx.hash();
    kolme
        .read()
        .execute_transaction(&tx, Timestamp::now(), BlockDataHandling::NoPriorData)
        .await?;
    kolme.propose_transaction(Arc::new(tx))?;
    Ok(txhash)
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
    ws.on_upgrade(move |socket| handle_websocket::<App>(kolme, socket))
}

enum RawMessage<AppMessage> {
    Block(Arc<SignedBlock<AppMessage>>),
    Failed(Arc<SignedTaggedJson<FailedTransaction>>),
    Latest(Arc<SignedTaggedJson<LatestBlock>>),
}

async fn handle_websocket<App: KolmeApp>(kolme: Kolme<App>, mut socket: WebSocket) {
    tracing::debug!("WebSocket subscribed to Kolme notifications");
    let mut interval = tokio::time::interval(Duration::from_secs(30));

    let mut next_height = kolme.read().get_next_height();
    let mut failed_txs = kolme.subscribe_failed_txs();
    let mut latest = kolme.subscribe_latest_block();

    async fn get_next_latest(
        latest: &mut tokio::sync::watch::Receiver<Option<Arc<SignedTaggedJson<LatestBlock>>>>,
    ) -> Result<Arc<SignedTaggedJson<LatestBlock>>> {
        loop {
            latest.changed().await?;
            if let Some(latest) = latest.borrow().clone().as_ref() {
                break Ok(latest.clone());
            }
        }
    }

    enum Action<AppMessage> {
        Ping,
        Raw(RawMessage<AppMessage>),
    }

    loop {
        let action = tokio::select! {
            _ = interval.tick() => Ok(Action::Ping),
            block = kolme.wait_for_block(next_height) => {
                next_height = next_height.next();
                block.map(|block| Action::Raw(RawMessage::Block(block)))
            }
            failed = failed_txs.recv() => failed.map(|failed| Action::Raw(RawMessage::Failed(failed))).map_err(anyhow::Error::from),
            latest = get_next_latest(&mut latest) => latest.map(|latest| Action::Raw(RawMessage::Latest(latest))),
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
            Action::Raw(raw) => {
                let api_notification = match to_api_notification(&kolme, raw).await {
                    Ok(api_notification) => api_notification,
                    Err(e) => {
                        tracing::error!("API server websockets: Error converting RawMessage to ApiNotification: {e}");
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
    raw: RawMessage<App::Message>,
) -> Result<ApiNotification<App::Message>> {
    match raw {
        RawMessage::Block(block) => {
            let height = block.height();
            // Ensure we have the block in local storage.
            let block = tokio::time::timeout(
                tokio::time::Duration::from_secs(20),
                kolme.wait_for_block(height),
            )
            .await
            .with_context(|| format!("Loading logs: took too long to load block {height}"))?
            .with_context(|| format!("Failed to get block {height} in order to load logs"))?;
            let logs = kolme.load_logs(block.as_inner().logs).await?.into();
            Ok(ApiNotification::NewBlock { block, logs })
        }
        RawMessage::Failed(failed) => Ok(ApiNotification::FailedTransaction(failed)),
        RawMessage::Latest(latest_block) => Ok(ApiNotification::LatestBlock(latest_block)),
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
    /// A transaction failed in the processor.
    FailedTransaction(Arc<SignedTaggedJson<FailedTransaction>>),
    LatestBlock(Arc<SignedTaggedJson<LatestBlock>>),
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
