use std::sync::atomic::AtomicU64;

use axum::{
    extract::{ws::WebSocket, State, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
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
    pub(super) payload: GossipMessage<App>,
    pub(super) tx: WebsocketsPrivateSender<App>,
}

pub(super) struct WebsocketsPrivateSender<App: KolmeApp> {
    pub(super) tx: tokio::sync::mpsc::Sender<GossipMessage<App>>,
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
    const CHANNEL_SIZE: usize = 100;

    pub(super) fn publish(&self, msg: &GossipMessage<App>) {
        let percentage = self.tx_gossip.len() as f32 / Self::CHANNEL_SIZE as f32;

        if percentage >= 0.8 {
            tracing::warn!(
                "Gossip broadcast sender channel buffer is at {}% capacity - {}/{}.",
                (percentage * 100.) as u32,
                self.tx_gossip.len(),
                Self::CHANNEL_SIZE
            );
        }

        self.tx_gossip.send(msg.clone()).ok();
    }

    // TODO: pass in a JoinSet and spawn all tasks into that instead
    pub(super) fn new(
        set: &mut JoinSet<()>,
        websockets_binds: Vec<std::net::SocketAddr>,
        websockets_servers: Vec<String>,
        local_display_name: &str,
        kolme: Kolme<App>,
    ) -> Result<Self> {
        let tx_gossip = Sender::new(Self::CHANNEL_SIZE);
        let (tx_message, rx_message) = tokio::sync::mpsc::channel(Self::CHANNEL_SIZE);
        let local_display_name: Arc<str> = local_display_name.into();
        for bind in websockets_binds {
            set.spawn(launch_server(
                ServerState {
                    rx_gossip: tx_gossip.subscribe(),
                    tx_message: tx_message.clone(),
                    local_display_name: local_display_name.clone(),
                    kolme: kolme.clone(),
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
                kolme.clone(),
            ));
        }
        Ok(WebsocketsManager {
            tx_gossip,
            rx_message,
        })
    }

    pub(super) async fn get_incoming(&mut self) -> WebsocketsMessage<App> {
        match self.rx_message.recv().await {
            Some(msg) => {
                let percentage = self.rx_message.len() as f32 / Self::CHANNEL_SIZE as f32;

                if percentage >= 0.8 {
                    tracing::warn!(
                        "Gossip websocket message receiver channel buffer is at {}% capacity - {}/{}.",
                        (percentage * 100.) as u32,
                        self.rx_message.len(),
                        Self::CHANNEL_SIZE
                    );
                }

                msg
            }
            None => {
                tracing::error!("Peer sender channel closed. Moving into endless future!");
                std::future::pending().await
            }
        }
    }
}

async fn launch_client<App: KolmeApp>(
    local_display_name: Arc<str>,
    mut rx_gossip: Receiver<GossipMessage<App>>,
    mut tx_message: tokio::sync::mpsc::Sender<WebsocketsMessage<App>>,
    server: String,
    kolme: Kolme<App>,
) {
    loop {
        match launch_client_inner(
            local_display_name.clone(),
            &mut rx_gossip,
            &mut tx_message,
            &server,
            kolme.get_latest_block(),
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
    latest: Option<Arc<SignedTaggedJson<LatestBlock>>>,
) -> Result<()> {
    let (stream, res) = tokio_tungstenite::connect_async(server).await?;
    tracing::debug!(%local_display_name,"launch_client_inner on {server}: got res {res:?}");
    ws_helper(rx_gossip, tx_message, stream, &local_display_name, latest).await;
    Ok(())
}

struct ServerState<App: KolmeApp> {
    rx_gossip: Receiver<GossipMessage<App>>,
    tx_message: tokio::sync::mpsc::Sender<WebsocketsMessage<App>>,
    local_display_name: Arc<str>,
    kolme: Kolme<App>,
}

impl<App: KolmeApp> Clone for ServerState<App> {
    fn clone(&self) -> Self {
        Self {
            rx_gossip: self.rx_gossip.resubscribe(),
            tx_message: self.tx_message.clone(),
            local_display_name: self.local_display_name.clone(),
            kolme: self.kolme.clone(),
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
        .with_state(server_state)
        .route("/healthz", get(healthz));
    axum::serve(listener, router).await?;
    Ok(())
}

async fn healthz() -> &'static str {
    "Healthy!"
}

async fn ws_handler_wrapper<App: KolmeApp>(
    ws: WebSocketUpgrade,
    State(server_state): State<ServerState<App>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_handler(server_state, socket))
}

async fn ws_handler<App: KolmeApp>(
    ServerState {
        mut rx_gossip,
        mut tx_message,
        local_display_name,
        kolme,
    }: ServerState<App>,
    socket: WebSocket,
) {
    ws_helper(
        &mut rx_gossip,
        &mut tx_message,
        socket,
        &local_display_name,
        kolme.get_latest_block(),
    )
    .await;
}

enum WebsocketsRecv<App: KolmeApp> {
    Close,
    Skip,
    Payload(Box<GossipMessage<App>>),
    Err(anyhow::Error),
}

trait WebSocketWrapper {
    async fn recv<App: KolmeApp>(&mut self, local_display_name: &str) -> WebsocketsRecv<App>;
    async fn send_payload<App: KolmeApp>(
        &mut self,
        payload: GossipMessage<App>,
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
        payload: GossipMessage<App>,
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
        payload: GossipMessage<App>,
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
    latest: Option<Arc<SignedTaggedJson<LatestBlock>>>,
) {
    const CHANNEL_SIZE: usize = 16;

    let (tx_private, mut rx_private) = tokio::sync::mpsc::channel(CHANNEL_SIZE);

    if let Some(latest) = latest {
        let msg = GossipMessage::ProvideLatestBlock { latest };
        if let Err(e) = tx_private.send(msg).await {
            tracing::warn!(%local_display_name, "Error sending initial latest block information: {e}");
        }
    }

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let tx_private = WebsocketsPrivateSender {
        tx: tx_private,
        id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    };
    #[allow(clippy::large_enum_variant)]
    enum Triple<App: KolmeApp> {
        Peer(WebsocketsRecv<App>),
        LocalGossip(Result<GossipMessage<App>, tokio::sync::broadcast::error::RecvError>),
        LocalPrivate(Option<GossipMessage<App>>),
    }
    loop {
        let next = tokio::select! {
            msg = socket.recv::<App>(local_display_name) => Triple::Peer(msg),
            gossip = rx_gossip.recv() => Triple::LocalGossip(gossip),
            payload = rx_private.recv() => Triple::LocalPrivate(payload),
        };

        let percentage = rx_private.len() as f32 / CHANNEL_SIZE as f32;

        if percentage >= 0.8 {
            tracing::warn!(
                %local_display_name,
                "Local gossip receiver channel buffer is at {}% capacity - {}/{}.",
                (percentage * 100.) as u32,
                rx_private.len(),
                CHANNEL_SIZE
            );
        }

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
            Triple::LocalGossip(Ok(gossip)) => gossip,
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
