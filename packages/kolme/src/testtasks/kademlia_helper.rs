use super::TestTasks;
use crate::*;

impl TestTasks {
    pub fn launch_websockets_discovery<App: KolmeApp>(
        &self,
        kolme: Kolme<App>,
        display_name: &str,
    ) -> WebsocketsDiscovery {
        self.launch_websockets_discovery_with(kolme, display_name, |f| f)
    }

    pub fn launch_websockets_discovery_with<App: KolmeApp, F>(
        &self,
        kolme: Kolme<App>,
        display_name: &str,
        f: F,
    ) -> WebsocketsDiscovery
    where
        F: FnOnce(GossipBuilder) -> GossipBuilder,
    {
        let port = std::net::TcpListener::bind("0.0.0.0:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert_ne!(port, 0);

        let gossip = GossipBuilder::new()
            .set_local_display_name(display_name)
            .add_websockets_bind(format!("127.0.0.1:{port}").parse().unwrap());
        let gossip = f(gossip);
        let gossip = gossip.build(kolme).unwrap();

        // And now launch a gossip node for this Kolme
        self.try_spawn_persistent(gossip.run());

        WebsocketsDiscovery { port }
    }

    pub async fn launch_websockets_client<App: KolmeApp>(
        &self,
        kolme: Kolme<App>,
        display_name: &str,
        discovery: &WebsocketsDiscovery,
    ) {
        self.launch_websockets_client_with(kolme, display_name, discovery, |builder| {
            builder.set_sync_mode(
                SyncMode::BlockTransfer,
                DataLoadValidation::ValidateDataLoads,
            )
        })
        .await;
    }

    pub async fn launch_websockets_client_with<App: KolmeApp, F>(
        &self,
        kolme: Kolme<App>,
        display_name: &str,
        discovery: &WebsocketsDiscovery,
        f: F,
    ) where
        F: FnOnce(GossipBuilder) -> GossipBuilder,
    {
        let builder = GossipBuilder::new()
            .set_local_display_name(display_name)
            .add_websockets_server(format!("ws://127.0.0.1:{}", discovery.port));
        let builder = f(builder);
        let gossip = builder.build(kolme.clone()).unwrap();
        let mut ready = gossip.subscribe_network_ready();
        self.try_spawn_persistent(gossip.run());
        tokio::time::timeout(tokio::time::Duration::from_secs(30), ready.changed())
            .await
            .expect("Timed out waiting for network to be ready")
            .unwrap();
    }
}

#[derive(Debug, Clone)]
pub struct WebsocketsDiscovery {
    port: u16,
}
