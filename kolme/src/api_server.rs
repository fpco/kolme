use axum::{
    extract::{Query, State},
    http::header::CONTENT_TYPE,
    response::IntoResponse,
    routing::{get, put},
    Json,
};
use reqwest::{Method, StatusCode};
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
            .allow_methods([Method::GET, Method::POST])
            .allow_origin(Any)
            .allow_headers([CONTENT_TYPE]);

        let app = axum::Router::new()
            .route("/", get(basics))
            .route("/broadcast", put(broadcast))
            .route("/get-next-nonce", get(get_next_nonce))
            .layer(cors)
            .with_state(self.kolme);

        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        axum::serve(listener, app)
            .await
            .map_err(anyhow::Error::from)
    }
}

async fn basics<App: KolmeApp>(State(kolme): State<Kolme<App>>) -> impl IntoResponse {
    #[derive(serde::Serialize)]
    struct Basics<'a> {
        next_height: BlockHeight,
        next_genesis_action: Option<GenesisAction>,
        bridges: &'a BTreeMap<ExternalChain, ChainConfig>,
        balances: &'a Balances,
        app_state: serde_json::Value,
    }

    let kolme = kolme.read().await;
    let basics = Basics {
        next_height: kolme.get_next_height(),
        next_genesis_action: kolme.get_next_genesis_action(),
        bridges: kolme.get_bridge_contracts(),
        balances: kolme.get_balances(),
        app_state: serde_json::from_str(&App::save_state(kolme.get_app_state()).unwrap()).unwrap(),
    };

    Json(basics).into_response()
}

async fn broadcast<App: KolmeApp>(
    State(kolme): State<Kolme<App>>,
    Json(tx): Json<SignedTransaction<App::Message>>,
) -> impl IntoResponse {
    let txhash = tx.0.message_hash();
    if let Err(e) = kolme.read().await.execute_transaction(&tx, None).await {
        let mut res = e.to_string().into_response();
        *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        return res;
    }
    match kolme.propose_transaction(tx) {
        Ok(()) => Json(serde_json::json!({"txhash":txhash})).into_response(),
        Err(e) => {
            let mut res = e.to_string().into_response();
            *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            res
        }
    }
}

#[derive(serde::Deserialize)]
struct NextNonce {
    pubkey: PublicKey,
}

async fn get_next_nonce<App: KolmeApp>(
    State(kolme): State<Kolme<App>>,
    Query(NextNonce { pubkey }): Query<NextNonce>,
) -> impl IntoResponse {
    match kolme.read().await.get_account_and_next_nonce(pubkey).await {
        Ok(nonce) => Json(serde_json::json!({"next_nonce":nonce.next_nonce})).into_response(),
        Err(e) => {
            let mut res = e.to_string().into_response();
            *res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            res
        }
    }
}
