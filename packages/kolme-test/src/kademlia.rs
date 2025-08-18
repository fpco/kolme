use anyhow::Result;
use kademlia_discovery::client;
use kolme::{testtasks::TestTasks, SecretKey};

const KADEMLIA_PORT: u16 = 5400;

#[tokio::test]
async fn ensure_kademlia_discovery_works() {
    TestTasks::start(kademlia_discovery_inner, ()).await;
}

async fn kademlia_discovery_inner(testtasks: TestTasks, (): ()) {
    testtasks.spawn_persistent(async move {
        kademlia_discovery::validators(KADEMLIA_PORT, false, false, false)
            .await
            .unwrap();
    });

    let signing_secret = SecretKey::random();
    testtasks.try_spawn(kademlia_discovery_client(KADEMLIA_PORT, signing_secret));
}

async fn kademlia_discovery_client(port: u16, signing_secret: SecretKey) -> Result<()> {
    client(
        &format!("/dns4/localhost/tcp/{port}"),
        signing_secret,
        false,
    )
    .await
    .unwrap();
    Ok(())
}
