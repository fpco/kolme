use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket},
        Path, Query, State, WebSocketUpgrade,
    },
    http::header::{self, CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::{get, put},
    Json, Router,
};
use reqwest::{Method, StatusCode};
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use version_compare::Version;

use crate::*;

pub use axum;

#[derive(thiserror::Error, Debug)]
pub enum KolmeApiError {
    #[error("Block {0} not found")]
    BlockNotFound(BlockHeight),

    #[error("Chain version {requested} not found, earliest is {earliest}")]
    ChainVersionNotFound { requested: String, earliest: String },

    #[error("Unable to compare chain versions")]
    VersionComparisonFailed,

    #[error("Could not find any block with chain version {0}")]
    BlockNotFoundOnChainVersion(String),

    #[error("Invalid chain version: {0}")]
    InvalidChainVersion(String),

    #[error("No blocks in chain")]
    NoBlocksInChain,

    #[error("Timeout waiting for block {0}")]
    BlockTimeout(BlockHeight),

    #[error("Failed to load block {0}")]
    BlockLoadFailed(BlockHeight),

    #[error("Could not find first block for chain version {0}")]
    FirstBlockNotFound(String),

    #[error("Could not find last block for chain version {0}")]
    LastBlockNotFound(String),

    #[error("Underflow in prev() operation")]
    UnderflowInPrev,

    #[error("Merkle serialization error")]
    MerkleSerial(#[from] MerkleSerialError),
}

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

    pub async fn run<A: tokio::net::ToSocketAddrs>(self, addr: A) -> Result<(), KolmeError> {
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
        axum::serve(listener, app).await.map_err(KolmeError::from)
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
        .route("/fork-info", get(fork_info))
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
) -> Result<TxHash, KolmeError> {
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
) -> Result<Response, KolmeError> {
    #[derive(serde::Serialize)]
    struct Response<'a, App: KolmeApp> {
        code_version: &'a String,
        chain_version: &'a String,
        blockhash: Sha256Hash,
        txhash: Sha256Hash,
        block: &'a SignedBlock<App::Message>,
        logs: &'a [Vec<String>],
    }

    let Some(block) = kolme.get_block(height).await? else {
        return Ok(Json(serde_json::json!(null)).into_response());
    };

    let framework_hash = block.block.as_inner().framework_state;
    let state = kolme.get_framework(framework_hash).await?;
    let code_version = kolme.get_code_version();
    let chain_version = state.get_chain_version();

    let logs = kolme
        .get_merkle_by_hash::<Vec<_>>(block.block.0.message.as_inner().logs)
        .await?;
    let resp: Response<'_, App> = Response {
        code_version,
        chain_version,
        blockhash: block.blockhash,
        txhash: block.txhash,
        block: &block.block,
        logs: &logs,
    };

    Ok(Json(resp).into_response())
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct ForkInfo {
    pub first_block: BlockHeight,
    pub last_block: BlockHeight,
}

struct BlockResponse {
    /// Chain version at that specific block
    pub chain_version: String,
    pub block_height: BlockHeight,
}

async fn block_response<App: KolmeApp>(
    kolme: &Kolme<App>,
    block: BlockHeight,
) -> Result<BlockResponse, KolmeError> {
    let Some(signed_block) = kolme.get_block(block).await? else {
        return Err(KolmeError::from(KolmeApiError::BlockNotFound(block)));
    };

    let framework_hash = signed_block.block.as_inner().framework_state;
    let state = kolme
        .get_framework(framework_hash)
        .await
        .map_err(KolmeError::from)?;
    let chain_version = state.get_chain_version().clone();
    Ok(BlockResponse {
        chain_version,
        block_height: block,
    })
}

/// Find an arbitrary block height with a particular chain version
async fn find_block_height<App: KolmeApp>(
    kolme: &Kolme<App>,
    chain_version: &Version<'_>,
) -> Result<BlockResponse, KolmeError> {
    let next_height = kolme.read().get_next_height();
    let mut start_block = BlockHeight::start();
    let mut end_block = next_height.prev().ok_or(KolmeApiError::NoBlocksInChain)?;

    while start_block.0 <= end_block.0 {
        let middle_block = start_block.increasing_middle(end_block)?;
        let response = block_response(kolme, middle_block).await?;
        let existing_version = Version::from(&response.chain_version)
            .ok_or_else(|| KolmeApiError::InvalidChainVersion(response.chain_version.clone()))?;
        let result = chain_version.compare(existing_version);
        match result {
            version_compare::Cmp::Eq => return Ok(response),
            version_compare::Cmp::Lt | version_compare::Cmp::Le => {
                // The version we want is older than the one at `middle_block`.
                // Search in the lower half.
                if middle_block.is_start() {
                    // We are at the beginning and the version is still too high.
                    return Err(KolmeError::from(KolmeApiError::ChainVersionNotFound {
                        requested: chain_version.to_string(),
                        earliest: response.chain_version,
                    }));
                }
                end_block = middle_block
                    .prev()
                    .ok_or(KolmeError::from(KolmeApiError::UnderflowInPrev))?;
            }
            version_compare::Cmp::Gt | version_compare::Cmp::Ge => {
                // The version we want is newer than the one at `middle_block`.
                // Search in the upper half.
                start_block = middle_block.next();
            }
            version_compare::Cmp::Ne => {
                return Err(KolmeError::from(KolmeApiError::VersionComparisonFailed));
            }
        }
    }

    Err(KolmeError::from(
        KolmeApiError::BlockNotFoundOnChainVersion(chain_version.to_string()),
    ))
}

/// Find the first block height with a particular chain version
async fn find_first_block<App: KolmeApp>(
    kolme: &Kolme<App>,
    chain_version: &Version<'_>,
    mut end_block: BlockHeight,
) -> Result<BlockHeight, KolmeError> {
    let mut start_block = BlockHeight::start();
    let mut first_block = None;

    while start_block.0 <= end_block.0 {
        let middle_block = start_block.increasing_middle(end_block)?;
        let response = block_response(kolme, middle_block).await?;

        let response_chain_version =
            Version::from(&response.chain_version).ok_or(KolmeError::from(
                KolmeApiError::InvalidChainVersion(response.chain_version.to_owned()),
            ))?;

        if response_chain_version == *chain_version {
            first_block = Some(middle_block);
            if middle_block.is_start() {
                break;
            }
            end_block = middle_block
                .prev()
                .ok_or(KolmeError::from(KolmeApiError::UnderflowInPrev))?;
        } else if response_chain_version.compare(chain_version) == version_compare::Cmp::Lt {
            start_block = middle_block.next();
        } else {
            if middle_block.is_start() {
                break;
            }
            end_block = middle_block
                .prev()
                .ok_or(KolmeError::from(KolmeApiError::UnderflowInPrev))?;
        }
    }

    match first_block {
        Some(block_height) => Ok(block_height),
        None => Err(KolmeError::from(KolmeApiError::FirstBlockNotFound(
            chain_version.to_string(),
        ))),
    }
}

/// Find the last block height with a particular chain version
async fn find_last_block<App: KolmeApp>(
    kolme: &Kolme<App>,
    chain_version: &Version<'_>,
    mut start_block: BlockHeight,
) -> Result<BlockHeight, KolmeError> {
    let next_height = kolme.read().get_next_height();
    let latest_block = next_height
        .prev()
        .ok_or(KolmeError::from(KolmeApiError::NoBlocksInChain))?;
    let mut end_block = latest_block;
    let mut last_block = None;

    while start_block.0 <= end_block.0 {
        let middle_block = start_block.increasing_middle(end_block)?;
        let response = block_response(kolme, middle_block).await?;

        let response_chain_version =
            Version::from(&response.chain_version).ok_or(KolmeError::from(
                KolmeApiError::InvalidChainVersion(response.chain_version.to_owned()),
            ))?;

        if response_chain_version == *chain_version {
            last_block = Some(middle_block);
            start_block = middle_block.next();
        } else if response_chain_version.compare(chain_version) == version_compare::Cmp::Gt {
            if middle_block.is_start() {
                break;
            }
            end_block = middle_block
                .prev()
                .ok_or(KolmeError::from(KolmeApiError::UnderflowInPrev))?;
        } else {
            start_block = middle_block.next();
        }
    }

    match last_block {
        Some(block_height) => Ok(block_height),
        None => Err(KolmeError::from(KolmeApiError::LastBlockNotFound(
            chain_version.to_string(),
        ))),
    }
}

#[derive(serde::Deserialize)]
struct ForkInfoQuery {
    chain_version: String,
}

async fn fork_info<App: KolmeApp>(
    State(kolme): State<Kolme<App>>,
    Query(query): Query<ForkInfoQuery>,
) -> impl IntoResponse {
    let chain_version = query.chain_version;
    let chain_version = Version::from(&chain_version);

    let chain_version = match chain_version {
        Some(version) => version,
        None => {
            let mut res = "Invalid chain version".into_response();
            *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            return res;
        }
    };

    let result: Result<ForkInfo> = async {
        let found_block = find_block_height(&kolme, &chain_version).await?;
        let first_block =
            find_first_block(&kolme, &chain_version, found_block.block_height).await?;
        let last_block = find_last_block(&kolme, &chain_version, found_block.block_height).await?;
        Ok(ForkInfo {
            first_block,
            last_block,
        })
    }
    .await;

    match result {
        Ok(fork_info) => {
            let mut response = Json(fork_info).into_response();
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("public, max-age=300"),
            );
            response
        }
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
            failed = failed_txs.recv() => failed.map(|failed| Action::Raw(RawMessage::Failed(failed))).map_err(KolmeError::from),
            latest = get_next_latest(&mut latest) => latest.map(|latest| Action::Raw(RawMessage::Latest(latest))).map_err(KolmeError::from),
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
) -> Result<ApiNotification<App::Message>, KolmeApiError> {
    match raw {
        RawMessage::Block(block) => {
            let height = block.height();
            // Ensure we have the block in local storage.
            let block = tokio::time::timeout(
                tokio::time::Duration::from_secs(20),
                kolme.wait_for_block(height),
            )
            .await
            .map_err(|_| KolmeApiError::BlockTimeout(height))?
            .map_err(|_| KolmeApiError::BlockLoadFailed(height))?;

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
