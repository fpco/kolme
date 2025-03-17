use axum::{
    extract::State, http::header::CONTENT_TYPE, response::IntoResponse, routing::get, Json,
};
use reqwest::Method;
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
        next_event_height: EventHeight,
        next_exec_height: EventHeight,
        next_genesis_action: Option<GenesisAction>,
        bridges: &'a BTreeMap<ExternalChain, ChainConfig>,
    }

    let kolme = kolme.read().await;
    let basics = Basics {
        next_event_height: kolme.get_next_event_height(),
        next_exec_height: kolme.get_next_exec_height(),
        next_genesis_action: kolme.get_next_genesis_action(),
        bridges: kolme.get_bridge_contracts(),
    };

    Json(basics).into_response()
}
