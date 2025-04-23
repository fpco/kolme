mod setup;

use std::{str::FromStr, net::SocketAddr};

use cosmos::SeedPhrase;
use rust_decimal::{dec, Decimal};
use shared::cosmos::ExecuteMsg;
use tokio::task;

use kolme::*;
use example_solana_cosmos_bridge::{SolanaCosmosBridgeApp, ASSET_ID};
use setup::{
    deploy_solana_bridge, make_osmo_token, make_solana_client, make_cosmos_client,
    airdrop, mint_to, deposit_and_register, bridge_pubkey_from_hex, kolme_state,
    wait_until_init, AUTHORITY_PUBKEY, AUTHORITY_SEED_BYTES
};
use kolme_solana_bridge_client::{keypair::Keypair, signer::Signer};

const DB_PATH: &str = "example-solana-cosmos-bridge.sqlite3";
const DUMMY_CODE_VERSION: &str = "dummy code version";
const COSMOS_SUBMITTER_SEED: &str = "notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius";

const USER_PUBKEY: &str = "032caf3bb79f995e0a26d8e08aa54c794660d8398cfcb39855ded310492be8815b";
const USER_SECRET: &str = "2bb0119bcf9ac0d9a8883b2832f3309217d350033ba944193352f034f197b96a";

#[test_log::test(tokio::test)]
#[ignore = "depends on local Solana validator and localosmosis thus hidden from default tests"]
async fn bridge_transfer() {
    deploy_solana_bridge().await.unwrap();

    let http_client = reqwest::Client::new();
    let solana_client = make_solana_client();
    let cosmos_client = make_cosmos_client();

    let osmo = make_osmo_token(solana_client.clone()).await.unwrap();
    let solana_submitter = Keypair::new();
    let cosmos_submitter = SeedPhrase::from_str(COSMOS_SUBMITTER_SEED).unwrap();

    airdrop(&solana_client, &solana_submitter.pubkey(), 10000000).await.unwrap();

    let kolme = Kolme::new(SolanaCosmosBridgeApp, DUMMY_CODE_VERSION, DB_PATH).await.unwrap();

    let kolme_clone = kolme.clone();
    let socket_addr = SocketAddr::from_str("[::]:3000").unwrap();
    let handle = task::spawn(async move {
        example_solana_cosmos_bridge::serve(kolme_clone, solana_submitter, cosmos_submitter, socket_addr).await
    });

    let user = Keypair::new();
    let mint_amount = 10_000000;

    futures::join!(
        async {
            wait_until_init(&http_client).await.unwrap();
        },
        async {
            airdrop(&solana_client, &user.pubkey(), 2000000).await.unwrap();
            mint_to(&osmo, &user.pubkey(), mint_amount).await.unwrap();
        }
    );

    let send_amount = mint_amount / 2;
    let pubkey = bridge_pubkey_from_hex(USER_PUBKEY);
    deposit_and_register(&solana_client, &user, &osmo, send_amount, vec![pubkey]).await.unwrap();

    tracing::info!("Aborting Kolme...");
    handle.abort();
    tracing::info!("Sucessfully exited!");
}
