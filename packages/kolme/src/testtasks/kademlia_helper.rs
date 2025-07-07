use gossip::GossipListener;
use libp2p::{Multiaddr, PeerId};

use super::TestTasks;
use crate::*;

impl TestTasks {
    pub fn launch_kademlia_discovery<App: KolmeApp>(
        &self,
        kolme: Kolme<App>,
        display_name: &str,
    ) -> KademliaDiscovery {
        let listener = GossipListener::random().unwrap();
        let port = listener.port;
        assert_ne!(port, 0);

        let gossip = GossipBuilder::new()
            .set_local_display_name(display_name)
            .add_listener(listener)
            .build(kolme)
            .unwrap();

        let peer = gossip.peer_id();
        let addr = format!("/ip4/127.0.0.1/tcp/{port}").parse().unwrap();

        // And now launch a gossip node for this Kolme
        self.try_spawn_persistent(gossip.run());

        KademliaDiscovery { peer, addr }
    }

    pub fn launch_kademlia_client<App: KolmeApp>(
        &self,
        kolme: Kolme<App>,
        display_name: &str,
        discovery: &KademliaDiscovery,
    ) {
        self.launch_kademlia_client_with(kolme, display_name, discovery, |builder| {
            builder.set_sync_mode(
                SyncMode::BlockTransfer,
                DataLoadValidation::ValidateDataLoads,
            )
        });
    }

    pub fn launch_kademlia_client_with<App: KolmeApp, F>(
        &self,
        kolme: Kolme<App>,
        display_name: &str,
        discovery: &KademliaDiscovery,
        f: F,
    ) where
        F: FnOnce(GossipBuilder) -> GossipBuilder,
    {
        let builder = GossipBuilder::new()
            .set_local_display_name(display_name)
            .add_bootstrap(discovery.peer, discovery.addr.clone());
        let builder = f(builder);
        self.try_spawn_persistent(builder.build(kolme.clone()).unwrap().run());
    }
}

#[derive(Debug, Clone)]
pub struct KademliaDiscovery {
    peer: PeerId,
    addr: Multiaddr,
}
