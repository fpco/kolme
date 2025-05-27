const KADEMLIA_PORT: u16 = 5400;

#[tokio::test]
async fn ensure_kademlia_discovery_works() {
    let validator = tokio::spawn(kademlia_discovery::validators(KADEMLIA_PORT));
    _ = kademlia_discovery::client(&format!("/ip4/127.0.0.1/tcp/{KADEMLIA_PORT}")).await;
    validator.abort();
}
