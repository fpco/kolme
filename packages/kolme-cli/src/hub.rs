mod sanity;

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD_NO_PAD, Engine};
use kolme::{gossip::KademliaBootstrap, init_logger};
use libp2p::{
    autonat, dcutr,
    futures::StreamExt,
    gossipsub::{self},
    identity::Keypair,
    kad::{self, store::MemoryStore},
    noise, relay, rendezvous,
    swarm::NetworkBehaviour,
    tcp, upnp, yamux, Multiaddr, SwarmBuilder,
};

#[derive(clap::Parser)]
pub(super) enum Cmd {
    /// Generate a new P2P keypair
    GenKeypair,
    /// Start a Kolme Hub instance
    Start(#[clap(flatten)] StartOpt),
    /// Sanity test a hub by running multiple services talking to it.
    Sanity(#[clap(flatten)] sanity::SanityOpt),
}

#[derive(clap::Parser)]
pub(super) struct StartOpt {
    /// Turn on verbose debugging?
    #[clap(long, env = "KOLME_VERBOSE", global = true)]
    verbose: bool,
    /// P2P keypair's secret for this node
    #[clap(long, env = "KOLME_P2P_SECRET")]
    p2p_secret: String,
    /// P2P multiaddrs to listen on
    #[clap(long, env = "KOLME_LISTEN", value_delimiter = ',', required = true)]
    listen: Vec<Multiaddr>,
    /// Other Kolme Hub nodes to bootstrap from
    #[clap(long, env = "KOLME_BOOTSTRAP", value_delimiter = ',')]
    bootstrap: Vec<KademliaBootstrap>,
    /// What external addresses should we advertise?
    #[clap(long, env = "KOLME_EXTERNAL", value_delimiter = ',')]
    external: Vec<Multiaddr>,
}

pub(super) async fn run(cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::GenKeypair => gen_keypair()?,
        Cmd::Start(opt) => start(opt).await?,
        Cmd::Sanity(opt) => sanity::sanity(opt).await?,
    }

    Ok(())
}

fn gen_keypair() -> Result<()> {
    let keypair = Keypair::generate_ed25519();

    let secret = keypair.to_protobuf_encoding()?;
    let secret = STANDARD_NO_PAD.encode(&secret);
    println!("PeerID: {}", keypair.public().to_peer_id());
    println!("Secret: {secret}");
    Ok(())
}

#[derive(NetworkBehaviour)]
struct MyBehaviour {
    gossipsub: gossipsub::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    relay: relay::Behaviour,
    autonat: autonat::Behaviour,
    dcutr: dcutr::Behaviour,
    upnp: upnp::tokio::Behaviour,
    rendezvous: rendezvous::server::Behaviour,
}

async fn start(opt: StartOpt) -> Result<()> {
    let StartOpt {
        p2p_secret,
        verbose,
        listen,
        bootstrap,
        external,
    } = opt;

    init_logger(verbose, Some(env!("CARGO_CRATE_NAME")));
    let p2p_secret = STANDARD_NO_PAD
        .decode(&p2p_secret)
        .context("Unable to base64-decode the P2P secret provided")?;
    let keypair = Keypair::from_protobuf_encoding(&p2p_secret)?;
    tracing::info!(
        "Launching Kolme Hub with Peer ID: {}",
        keypair.public().to_peer_id()
    );

    let mut swarm = SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default().nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_behaviour(|key| {
            let gossipsub_config = gossipsub::ConfigBuilder::default().build()?;
            let gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub_config,
            )?;

            let kademlia_config = kad::Config::default();
            let store = MemoryStore::new(key.public().to_peer_id());
            let mut kademlia =
                kad::Behaviour::with_config(key.public().to_peer_id(), store, kademlia_config);
            kademlia.set_mode(Some(kad::Mode::Server));

            let relay = relay::Behaviour::new(key.public().to_peer_id(), relay::Config::default());

            let mut autonat =
                autonat::Behaviour::new(key.public().to_peer_id(), autonat::Config::default());

            let dcutr = dcutr::Behaviour::new(key.public().to_peer_id());
            let upnp = upnp::tokio::Behaviour::default();

            let rendezvous =
                rendezvous::server::Behaviour::new(rendezvous::server::Config::default());

            for KademliaBootstrap { peer, address } in bootstrap {
                kademlia.add_address(&peer, address);
                autonat.add_server(peer, None);
            }

            Ok(MyBehaviour {
                gossipsub,
                kademlia,
                relay,
                autonat,
                dcutr,
                upnp,
                rendezvous,
            })
        })?
        .build();

    for addr in listen {
        swarm.listen_on(addr)?;
    }

    for addr in external {
        swarm.add_external_address(addr);
    }

    loop {
        let event = swarm.select_next_some().await;
        tracing::info!("{event:?}");
    }
}
