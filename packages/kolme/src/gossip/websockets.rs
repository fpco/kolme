use std::sync::atomic::AtomicU64;

use axum::{
    extract::{ws::WebSocket, State, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use gossip::{BlockRequest, BlockResponse};
use tokio::{
    net::TcpStream,
    sync::broadcast::{Receiver, Sender},
    task::JoinSet,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::GossipMessage;
use crate::*;

pub(super) struct WebsocketsManager<App: KolmeApp> {
    tx_gossip: Sender<GossipMessage<App>>,
    rx_message: tokio::sync::mpsc::Receiver<WebsocketsMessage<App>>,
}

pub(super) struct WebsocketsMessage<App: KolmeApp> {
    pub(super) payload: WebsocketsPayload<App>,
    pub(super) tx: WebsocketsPrivateSender<App>,
}

pub(super) struct WebsocketsPrivateSender<App: KolmeApp> {
    pub(super) tx: tokio::sync::mpsc::Sender<WebsocketsPayload<App>>,
    id: u64,
}

impl<App: KolmeApp> std::fmt::Debug for WebsocketsPrivateSender<App> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "WebsocketsPrivateSender({})", self.id)
    }
}

impl<App: KolmeApp> Clone for WebsocketsPrivateSender<App> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            id: self.id,
        }
    }
}

impl<App: KolmeApp> PartialEq for WebsocketsPrivateSender<App> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl<App: KolmeApp> Eq for WebsocketsPrivateSender<App> {}

impl<App: KolmeApp> WebsocketsManager<App> {
    pub(super) fn publish(&self, msg: &GossipMessage<App>) {
        self.tx_gossip.send(msg.clone()).ok();
    }

    // TODO: pass in a JoinSet and spawn all tasks into that instead
    pub(super) fn new(
        set: &mut JoinSet<()>,
        websockets_binds: Vec<std::net::SocketAddr>,
        websockets_servers: Vec<String>,
        local_display_name: &str,
    ) -> Result<Self> {
        let tx_gossip = Sender::new(100);
        let (tx_message, rx_message) = tokio::sync::mpsc::channel(100);
        let local_display_name: Arc<str> = local_display_name.into();
        for bind in websockets_binds {
            set.spawn(launch_server(
                ServerState {
                    rx_gossip: tx_gossip.subscribe(),
                    tx_message: tx_message.clone(),
                    local_display_name: local_display_name.clone(),
                },
                bind,
            ));
        }
        for server in websockets_servers {
            set.spawn(launch_client(
                local_display_name.clone(),
                tx_gossip.subscribe(),
                tx_message.clone(),
                server,
            ));
        }
        Ok(WebsocketsManager {
            tx_gossip,
            rx_message,
        })
    }

    pub(super) async fn get_incoming(&mut self) -> WebsocketsMessage<App> {
        match self.rx_message.recv().await {
            Some(msg) => msg,
            None => std::future::pending().await,
        }
    }
}

async fn launch_client<App: KolmeApp>(
    local_display_name: Arc<str>,
    mut rx_gossip: Receiver<GossipMessage<App>>,
    mut tx_message: tokio::sync::mpsc::Sender<WebsocketsMessage<App>>,
    server: String,
) {
    loop {
        match launch_client_inner(
            local_display_name.clone(),
            &mut rx_gossip,
            &mut tx_message,
            &server,
        )
        .await
        {
            Ok(()) => tracing::warn!(
                %local_display_name,
                "Unexpected exit from gossip::websockets::launch_client_inner for {server}"
            ),
            Err(e) => tracing::warn!(
                %local_display_name,
                "Error from gossip::websockets::launch_client_inner for {server}: {e}"
            ),
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}

async fn launch_client_inner<App: KolmeApp>(
    local_display_name: Arc<str>,
    rx_gossip: &mut Receiver<GossipMessage<App>>,
    tx_message: &mut tokio::sync::mpsc::Sender<WebsocketsMessage<App>>,
    server: &str,
) -> Result<()> {
    let (stream, res) = tokio_tungstenite::connect_async(server).await?;
    tracing::debug!(%local_display_name,"launch_client_inner on {server}: got res {res:?}");
    ws_helper(rx_gossip, tx_message, stream, &local_display_name).await;
    Ok(())
}

struct ServerState<App: KolmeApp> {
    rx_gossip: Receiver<GossipMessage<App>>,
    tx_message: tokio::sync::mpsc::Sender<WebsocketsMessage<App>>,
    local_display_name: Arc<str>,
}

impl<App: KolmeApp> Clone for ServerState<App> {
    fn clone(&self) -> Self {
        Self {
            rx_gossip: self.rx_gossip.resubscribe(),
            tx_message: self.tx_message.clone(),
            local_display_name: self.local_display_name.clone(),
        }
    }
}

async fn launch_server<App: KolmeApp>(server_state: ServerState<App>, bind: std::net::SocketAddr) {
    loop {
        match launch_server_inner(server_state.clone(), bind).await {
            Ok(()) => tracing::warn!(
                %server_state.local_display_name,
                "Unexpected exit from gossip::websockets::launch_server_inner for {bind}"
            ),
            Err(e) => {
                tracing::warn!(%server_state.local_display_name, "Error from gossip::websockets::launch_server_inner for {bind}: {e}")
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}

async fn launch_server_inner<App: KolmeApp>(
    server_state: ServerState<App>,
    bind: std::net::SocketAddr,
) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let router = axum::Router::new()
        .route("/", get(ws_handler_wrapper))
        .with_state(server_state);
    axum::serve(listener, router).await?;
    Ok(())
}

async fn ws_handler_wrapper<App: KolmeApp>(
    ws: WebSocketUpgrade,
    State(server_state): State<ServerState<App>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_handler(server_state, socket))
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(bound(serialize = "", deserialize = ""))]
#[allow(clippy::large_enum_variant)]
pub(super) enum WebsocketsPayload<App: KolmeApp> {
    Gossip(GossipMessage<App>),
    Request(BlockRequest),
    Response(BlockResponse<App::Message>),
}

impl<App: KolmeApp> std::fmt::Debug for WebsocketsPayload<App> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            WebsocketsPayload::Gossip(gossip) => f.debug_tuple("Gossip").field(gossip).finish(),
            WebsocketsPayload::Request(req) => f.debug_tuple("Request").field(req).finish(),
            WebsocketsPayload::Response(res) => f.debug_tuple("Response").field(res).finish(),
        }
    }
}

async fn ws_handler<App: KolmeApp>(
    ServerState {
        mut rx_gossip,
        mut tx_message,
        local_display_name,
    }: ServerState<App>,
    socket: WebSocket,
) {
    ws_helper(&mut rx_gossip, &mut tx_message, socket, &local_display_name).await;
}

enum WebsocketsRecv<App: KolmeApp> {
    Close,
    Skip,
    Payload(Box<WebsocketsPayload<App>>),
    Err(anyhow::Error),
}

trait WebSocketWrapper {
    async fn recv<App: KolmeApp>(&mut self, local_display_name: &str) -> WebsocketsRecv<App>;
    async fn send_payload<App: KolmeApp>(
        &mut self,
        payload: WebsocketsPayload<App>,
        local_display_name: &str,
    ) -> Result<()>;
}

impl WebSocketWrapper for WebSocket {
    async fn recv<App: KolmeApp>(&mut self, local_display_name: &str) -> WebsocketsRecv<App> {
        match self.recv().await {
            None => {
                tracing::info!(%local_display_name, "Gossip WebSockets server connection closed");
                WebsocketsRecv::Close
            }
            Some(Err(e)) => {
                tracing::error!(%local_display_name, "Gossip WebSockets server error: {e}");
                WebsocketsRecv::Close
            }
            Some(Ok(msg)) => match msg {
                axum::extract::ws::Message::Text(bytes) => {
                    match serde_json::from_slice(bytes.as_bytes()) {
                        Ok(payload) => WebsocketsRecv::Payload(payload),
                        Err(e) => WebsocketsRecv::Err(e.into()),
                    }
                }
                axum::extract::ws::Message::Close(_) => {
                    tracing::info!(%local_display_name, "Gossip WebSockets server connection received a Close");
                    WebsocketsRecv::Close
                }
                axum::extract::ws::Message::Ping(_) => WebsocketsRecv::Skip,
                axum::extract::ws::Message::Pong(_) => WebsocketsRecv::Skip,
                msg => {
                    tracing::warn!(%local_display_name, "Unhandled Gossip WebSockets server message: {msg:?}");
                    WebsocketsRecv::Close
                }
            },
        }
    }

    async fn send_payload<App: KolmeApp>(
        &mut self,
        payload: WebsocketsPayload<App>,
        _: &str,
    ) -> Result<()> {
        let payload = serde_json::to_string(&payload)?;
        self.send(axum::extract::ws::Message::text(payload)).await?;
        Ok(())
    }
}

impl WebSocketWrapper for WebSocketStream<MaybeTlsStream<TcpStream>> {
    async fn recv<App: KolmeApp>(&mut self, local_display_name: &str) -> WebsocketsRecv<App> {
        match self.next().await {
            None => {
                tracing::info!(%local_display_name, "Gossip WebSockets client connection closed");
                WebsocketsRecv::Close
            }
            Some(Err(e)) => {
                tracing::error!(%local_display_name, "Gossip WebSockets client error: {e}");
                WebsocketsRecv::Close
            }
            Some(Ok(msg)) => match msg {
                tokio_tungstenite::tungstenite::Message::Text(bytes) => {
                    match serde_json::from_slice(bytes.as_bytes()) {
                        Ok(payload) => WebsocketsRecv::Payload(payload),
                        Err(e) => WebsocketsRecv::Err(e.into()),
                    }
                }
                tokio_tungstenite::tungstenite::Message::Close(_) => {
                    tracing::info!(%local_display_name, "Gossip WebSockets client connection received a Close");
                    WebsocketsRecv::Close
                }
                tokio_tungstenite::tungstenite::Message::Ping(_) => WebsocketsRecv::Skip,
                tokio_tungstenite::tungstenite::Message::Pong(_) => WebsocketsRecv::Skip,
                msg => {
                    tracing::warn!(%local_display_name, "Unhandled Gossip WebSockets client message: {msg:?}");
                    WebsocketsRecv::Close
                }
            },
        }
    }

    async fn send_payload<App: KolmeApp>(
        &mut self,
        payload: WebsocketsPayload<App>,
        _: &str,
    ) -> Result<()> {
        let payload = serde_json::to_string(&payload)?;
        self.send(tokio_tungstenite::tungstenite::Message::text(payload))
            .await?;
        Ok(())
    }
}

async fn ws_helper<App: KolmeApp, S: WebSocketWrapper>(
    rx_gossip: &mut Receiver<GossipMessage<App>>,
    tx_message: &mut tokio::sync::mpsc::Sender<WebsocketsMessage<App>>,
    mut socket: S,
    local_display_name: &str,
) {
    let (tx_private, mut rx_private) = tokio::sync::mpsc::channel(16);

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let tx_private = WebsocketsPrivateSender {
        tx: tx_private,
        id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    };
    #[allow(clippy::large_enum_variant)]
    enum Triple<App: KolmeApp> {
        Peer(WebsocketsRecv<App>),
        LocalGossip(Result<GossipMessage<App>, tokio::sync::broadcast::error::RecvError>),
        LocalPrivate(Option<WebsocketsPayload<App>>),
    }
    loop {
        let next = tokio::select! {
            msg = socket.recv::<App>(local_display_name) => Triple::Peer(msg),
            gossip = rx_gossip.recv() => Triple::LocalGossip(gossip),
            payload = rx_private.recv() => Triple::LocalPrivate(payload),
        };

        let payload = match next {
            Triple::Peer(recv) => match recv {
                WebsocketsRecv::Close => break,
                WebsocketsRecv::Skip => continue,
                WebsocketsRecv::Payload(payload) => {
                    if let Err(e) = tx_message
                        .send(WebsocketsMessage {
                            payload: *payload,
                            tx: tx_private.clone(),
                        })
                        .await
                    {
                        tracing::error!(%local_display_name, "Gossip WebSockets: could not send payload from peer: {e}");
                        break;
                    }
                    continue;
                }
                WebsocketsRecv::Err(e) => {
                    tracing::error!(%local_display_name, "Gossip WebSockets error: {e}");
                    break;
                }
            },
            Triple::LocalGossip(Ok(gossip)) => WebsocketsPayload::Gossip(gossip),
            Triple::LocalGossip(Err(e)) => {
                tracing::error!(%local_display_name, "Gossip WebSockets: received error from rx_gossip: {e}");
                break;
            }
            Triple::LocalPrivate(None) => {
                tracing::error!(%local_display_name, "Logic error in Gossip WebSockets: rx_private returned None");
                break;
            }
            Triple::LocalPrivate(Some(payload)) => payload,
        };
        if let Err(e) = socket.send_payload(payload, local_display_name).await {
            tracing::error!(%local_display_name, "Gossip WebSockets error on delivering payload: {e}");
            break;
        }
    }
}
