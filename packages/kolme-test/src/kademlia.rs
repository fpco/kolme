use std::net::TcpListener;

use anyhow::Result;
use kademlia_discovery::client;
use kolme::{testtasks::TestTasks, KolmeError, SecretKey};

#[tokio::test]
async fn ensure_kademlia_discovery_works() {
    TestTasks::start(kademlia_discovery_inner, ()).await;
}

async fn kademlia_discovery_inner(testtasks: TestTasks, (): ()) {
    let port = TcpListener::bind("0.0.0.0:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    testtasks.spawn_persistent(async move {
        kademlia_discovery::validators(port, false, false, false)
            .await
            .unwrap();
    });

    let signing_secret = SecretKey::random();
    testtasks.try_spawn(kademlia_discovery_client(port, signing_secret));
}

async fn kademlia_discovery_client(port: u16, signing_secret: SecretKey) -> Result<(), KolmeError> {
    client(&format!("ws://localhost:{port}"), signing_secret, false)
        .await
        .unwrap();
    Ok(())
}
