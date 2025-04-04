use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    str::FromStr,
};

use anyhow::{anyhow, ensure, Result};
use backon::{ExponentialBuilder, Retryable};
use cosmos::{AddressHrp, Coin, Contract, Cosmos, HasAddress, SeedPhrase};
use futures_util::stream::StreamExt;
use pretty_assertions::assert_eq;
use rust_decimal::{dec, Decimal};
use serde_json::Value;
use shared::cosmos::ExecuteMsg;
use tempfile::NamedTempFile;
use tokio::{
    process::{Child, Command},
    time::{sleep, timeout, Duration},
};

mod six_sigma;
use six_sigma::types::*;

use kolme::*;
use tokio_tungstenite::connect_async;

const INVALID_SECRET_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[test_log::test(tokio::test)]
async fn test_websocket_notifications() {
    tracing::debug!("Starting test_websocket_notifications");
    prebuild().await.unwrap();
    let log_file = NamedTempFile::new().unwrap();
    let mut app = start_the_app(log_file.path().to_path_buf()).await.unwrap();
    let client = reqwest::Client::new();
    let contract_addr = wait_contract_deployed(&client).await.unwrap();
    tracing::debug!("Contract deployed at: {contract_addr}");

    let cosmos = cosmos::CosmosNetwork::OsmosisLocal
        .builder_local()
        .build()
        .unwrap();
    let contract = cosmos.make_contract(contract_addr.parse().unwrap());

    register_funder_account(&client, &contract).await.unwrap();
    broadcast(
        FUNDER_SECRET_KEY,
        AppMessage::SendFunds {
            asset_id: AssetId(1),
            amount: dec!(100_000),
        },
        &client,
        |state| {
            ensure!(state
                .app_state
                .strategic_reserve
                .get(&AssetId(1))
                .is_some_and(|balance| balance == &dec!(100_000)));
            Ok(())
        },
    )
    .await
    .unwrap();

    broadcast(ADMIN_SECRET_KEY, AppMessage::Init, &client, |state| {
        ensure!(matches!(state.app_state.state, AppState::Operational));
        Ok(())
    })
    .await
    .unwrap();

    let (mut ws, _) = connect_async("ws://localhost:3000/notifications")
        .await
        .unwrap();
    println!("Connected to WebSocket");

    broadcast(
        ADMIN_SECRET_KEY,
        AppMessage::AddMarket {
            id: 55,
            asset_id: AssetId(1),
            name: "WS market".to_string(),
        },
        &client,
        |state| {
            ensure!(state.app_state.markets.len() == 1);
            Ok(())
        },
    )
    .await
    .unwrap();

    let message = ws.next().await.unwrap().unwrap();
    let notification: Value = serde_json::from_str(message.into_text().unwrap().as_str()).unwrap();
    println!("Received notification: {}", notification);

    ws.close(None).await.unwrap();
    println!("WebSocket closed successfully");
    assert!(
        notification["NewBlock"].is_object() || notification["Broadcast"].is_object(),
        "Expected NewBlock or Broadcast notification, got: {}",
        notification
    );

    assert_eq!(app.try_wait().unwrap(), None);
}

#[test_log::test(tokio::test)]
async fn test_osmosis_unavailable() -> Result<()> {
    tracing::debug!("Stopping docker compose");
    Command::new("docker")
        .args(["compose", "down"])
        .status()
        .await?;

    tracing::debug!("Starting test_osmosis_unavailable");
    prebuild().await.unwrap();
    let log_file = NamedTempFile::new().unwrap();
    let mut app = start_the_app(log_file.path().to_path_buf()).await.unwrap();
    let client = reqwest::Client::new();

    let ws_result = connect_async("ws://localhost:3000/notifications").await;
    println!("WebSocket connection result: {:?}", ws_result);

    assert_eq!(app.try_wait()?, None);

    let broadcast_result = broadcast(
        ADMIN_SECRET_KEY,
        AppMessage::AddMarket {
            id: 57,
            asset_id: AssetId(1),
            name: "No Osmosis market".to_string(),
        },
        &client,
        |state| {
            ensure!(state.app_state.markets.len() == 0);
            Ok(())
        },
    )
    .await;

    assert!(
        broadcast_result.is_err(),
        "Expected transaction to fail due to Osmosis being unavailable"
    );

    if let Ok((mut ws, _)) = ws_result {
        ws.close(None).await.unwrap();
        println!("WebSocket closed successfully");
    }

    Command::new("docker")
        .args(["compose", "up", "-d"])
        .status()
        .await?;

    Ok(())
}

#[test_log::test(tokio::test)]
async fn test_rejected_transaction_invalid_signature() {
    tracing::debug!("Starting test_rejected_transaction_invalid_signature");
    prebuild().await.unwrap();
    let log_file = NamedTempFile::new().unwrap();
    let mut app = start_the_app(log_file.path().to_path_buf()).await.unwrap();
    let client = reqwest::Client::new();
    let contract_addr = wait_contract_deployed(&client).await.unwrap();
    tracing::debug!("Contract deployed at: {contract_addr}");

    let cosmos = cosmos::CosmosNetwork::OsmosisLocal
        .builder_local()
        .build()
        .unwrap();
    let contract = cosmos.make_contract(contract_addr.parse().unwrap());

    register_funder_account(&client, &contract).await.unwrap();
    broadcast(
        FUNDER_SECRET_KEY,
        AppMessage::SendFunds {
            asset_id: AssetId(1),
            amount: dec!(100_000),
        },
        &client,
        |state| {
            ensure!(state
                .app_state
                .strategic_reserve
                .get(&AssetId(1))
                .is_some_and(|balance| balance == &dec!(100_000)));
            Ok(())
        },
    )
    .await
    .unwrap();

    broadcast(ADMIN_SECRET_KEY, AppMessage::Init, &client, |state| {
        ensure!(matches!(state.app_state.state, AppState::Operational));
        Ok(())
    })
    .await
    .unwrap();

    let (mut ws, _) = connect_async("ws://localhost:3000/notifications")
        .await
        .unwrap();
    println!("Connected to WebSocket");

    let result = broadcast(
        INVALID_SECRET_KEY,
        AppMessage::AddMarket {
            id: 1,
            asset_id: AssetId(1),
            name: "Invalid market".to_string(),
        },
        &client,
        |state| {
            ensure!(state.app_state.markets.is_empty());
            Ok(())
        },
    )
    .await;

    assert!(
        result.is_err(),
        "Transaction should be rejected due to invalid signature"
    );

    let message = timeout(Duration::from_secs(5), ws.next()).await;
    match message {
        Ok(Some(Ok(message))) => {
            let notification: Value =
                serde_json::from_str(message.into_text().unwrap().as_str()).unwrap();
            println!("Unexpected notification received: {}", notification);
            panic!("Should not receive a notification for a rejected transaction");
        }
        Ok(Some(Err(e))) => panic!("WebSocket error: {}", e),
        Ok(None) => panic!("WebSocket closed unexpectedly"),
        Err(_) => println!("No notification received, as expected for a rejected transaction"),
    }

    ws.close(None).await.unwrap();
    println!("WebSocket closed successfully");

    assert_eq!(app.try_wait().unwrap(), None);
}

#[test_log::test(tokio::test)]
async fn test_rejected_transaction_insufficient_balance() {
    tracing::debug!("Starting test_rejected_transaction_insufficient_balance");
    prebuild().await.unwrap();
    let log_file = NamedTempFile::new().unwrap();
    let mut app = start_the_app(log_file.path().to_path_buf()).await.unwrap();
    let client = reqwest::Client::new();
    let contract_addr = wait_contract_deployed(&client).await.unwrap();
    tracing::debug!("Contract deployed at: {contract_addr}");

    let cosmos = cosmos::CosmosNetwork::OsmosisLocal
        .builder_local()
        .build()
        .unwrap();
    let contract = cosmos.make_contract(contract_addr.parse().unwrap());

    register_funder_account(&client, &contract).await.unwrap();
    broadcast(
        FUNDER_SECRET_KEY,
        AppMessage::SendFunds {
            asset_id: AssetId(1),
            amount: dec!(100_000),
        },
        &client,
        |state| {
            ensure!(state
                .app_state
                .strategic_reserve
                .get(&AssetId(1))
                .is_some_and(|balance| balance == &dec!(100_000)));
            Ok(())
        },
    )
    .await
    .unwrap();

    broadcast(ADMIN_SECRET_KEY, AppMessage::Init, &client, |state| {
        ensure!(matches!(state.app_state.state, AppState::Operational));
        Ok(())
    })
    .await
    .unwrap();

    broadcast(
        ADMIN_SECRET_KEY,
        AppMessage::AddMarket {
            id: 1,
            asset_id: AssetId(1),
            name: "Test market".to_string(),
        },
        &client,
        |state| {
            ensure!(state.app_state.markets.len() == 1);
            Ok(())
        },
    )
    .await
    .unwrap();

    let result = broadcast(
        ADMIN_SECRET_KEY,
        AppMessage::PlaceBet {
            wallet: sr_wallet().unwrap().get_address_string(),
            amount: dec!(1_000_000),
            market_id: 1,
            outcome: 0,
        },
        &client,
        |state| {
            ensure!(state
                .app_state
                .markets
                .get(&1)
                .is_some_and(|market| market.bets.is_empty()));
            Ok(())
        },
    )
    .await;

    assert!(
        result.is_err(),
        "Transaction should be rejected due to insufficient balance"
    );
    assert_eq!(app.try_wait().unwrap(), None);
}

#[test_log::test(tokio::test)]
async fn test_no_notification_subscribers() {
    tracing::debug!("Starting test_no_notification_subscribers");
    prebuild().await.unwrap();
    let log_file = NamedTempFile::new().unwrap();
    let mut app = start_the_app(log_file.path().to_path_buf()).await.unwrap();
    let client = reqwest::Client::new();
    let contract_addr = wait_contract_deployed(&client).await.unwrap();
    tracing::debug!("Contract deployed at: {contract_addr}");

    let cosmos = cosmos::CosmosNetwork::OsmosisLocal
        .builder_local()
        .build()
        .unwrap();
    let contract = cosmos.make_contract(contract_addr.parse().unwrap());

    register_funder_account(&client, &contract).await.unwrap();
    broadcast(
        FUNDER_SECRET_KEY,
        AppMessage::SendFunds {
            asset_id: AssetId(1),
            amount: dec!(100_000),
        },
        &client,
        |state| {
            ensure!(state
                .app_state
                .strategic_reserve
                .get(&AssetId(1))
                .is_some_and(|balance| balance == &dec!(100_000)));
            Ok(())
        },
    )
    .await
    .unwrap();

    broadcast(ADMIN_SECRET_KEY, AppMessage::Init, &client, |state| {
        ensure!(matches!(state.app_state.state, AppState::Operational));
        Ok(())
    })
    .await
    .unwrap();

    app.kill().await.unwrap();
    println!("Application killed to simulate no subscribers");

    let result = broadcast(
        ADMIN_SECRET_KEY,
        AppMessage::AddMarket {
            id: 1,
            asset_id: AssetId(1),
            name: "No subscribers market".to_string(),
        },
        &client,
        |state| {
            ensure!(state.app_state.markets.is_empty());
            Ok(())
        },
    )
    .await;

    assert!(
        result.is_err(),
        "Transaction should fail if no subscribers are listening"
    );
}

#[test_log::test(tokio::test)]
async fn test_malformed_cosmos_event() {
    tracing::debug!("Starting test_malformed_cosmos_event");
    prebuild().await.unwrap();
    let log_file = NamedTempFile::new().unwrap();
    let mut app = start_the_app(log_file.path().to_path_buf()).await.unwrap();
    let client = reqwest::Client::new();
    let contract_addr = wait_contract_deployed(&client).await.unwrap();
    tracing::debug!("Contract deployed at: {contract_addr}");

    let cosmos = cosmos::CosmosNetwork::OsmosisLocal
        .builder_local()
        .build()
        .unwrap();
    let contract = cosmos.make_contract(contract_addr.parse().unwrap());

    let wallet = sr_wallet().unwrap();
    let result = contract
        .execute(&wallet, vec![], ExecuteMsg::Regular { keys: vec![] })
        .await;

    sleep(Duration::from_secs(5)).await;

    let state = server_state(&client).await.unwrap();
    assert!(
        state.balances.is_empty(),
        "No accounts should be created from malformed event"
    );

    assert!(
        result.is_ok(),
        "Contract execution should succeed but event should not affect state"
    );
    assert_eq!(app.try_wait().unwrap(), None);
}

#[test_log::test(tokio::test)]
async fn test_many_transactions() {
    tracing::debug!("Starting test_many_transactions");
    prebuild().await.unwrap();
    let log_file = NamedTempFile::new().unwrap();
    let mut app = start_the_app(log_file.path().to_path_buf()).await.unwrap();
    let client = reqwest::Client::new();
    let contract_addr = wait_contract_deployed(&client).await.unwrap();
    tracing::debug!("Contract deployed at: {contract_addr}");

    let cosmos = cosmos::CosmosNetwork::OsmosisLocal
        .builder_local()
        .build()
        .unwrap();
    let contract = cosmos.make_contract(contract_addr.parse().unwrap());

    register_funder_account(&client, &contract).await.unwrap();
    broadcast(
        FUNDER_SECRET_KEY,
        AppMessage::SendFunds {
            asset_id: AssetId(1),
            amount: dec!(100_000),
        },
        &client,
        |state| {
            ensure!(state
                .app_state
                .strategic_reserve
                .get(&AssetId(1))
                .is_some_and(|balance| balance == &dec!(100_000)));
            Ok(())
        },
    )
    .await
    .unwrap();

    broadcast(ADMIN_SECRET_KEY, AppMessage::Init, &client, |state| {
        ensure!(matches!(state.app_state.state, AppState::Operational));
        Ok(())
    })
    .await
    .unwrap();

    for i in 1..=100 {
        let client = client.clone();
        let market_id = i * 2;
        broadcast(
            ADMIN_SECRET_KEY,
            AppMessage::AddMarket {
                id: market_id,
                asset_id: AssetId(1),
                name: format!("Market {}", i),
            },
            &client,
            |state| {
                ensure!(state.app_state.markets.contains_key(&market_id));
                Ok(())
            },
        )
        .await
        .unwrap();
    }

    let state = server_state(&client).await.unwrap();
    assert_eq!(
        state.app_state.markets.len(),
        100,
        "All markets should be created"
    );
    assert_eq!(app.try_wait().unwrap(), None);
}

#[test_log::test(tokio::test)]
async fn test_concurrent_transactions() {
    tracing::debug!("Starting test_concurrent_transactions");
    prebuild().await.unwrap();
    let log_file = NamedTempFile::new().unwrap();
    let mut app = start_the_app(log_file.path().to_path_buf()).await.unwrap();
    let client = reqwest::Client::new();
    let contract_addr = wait_contract_deployed(&client).await.unwrap();
    tracing::debug!("Contract deployed at: {contract_addr}");

    let cosmos = cosmos::CosmosNetwork::OsmosisLocal
        .builder_local()
        .build()
        .unwrap();
    let contract = cosmos.make_contract(contract_addr.parse().unwrap());

    register_funder_account(&client, &contract).await.unwrap();
    broadcast(
        FUNDER_SECRET_KEY,
        AppMessage::SendFunds {
            asset_id: AssetId(1),
            amount: dec!(100_000),
        },
        &client,
        |state| {
            ensure!(state
                .app_state
                .strategic_reserve
                .get(&AssetId(1))
                .is_some_and(|balance| balance == &dec!(100_000)));
            Ok(())
        },
    )
    .await
    .unwrap();

    broadcast(ADMIN_SECRET_KEY, AppMessage::Init, &client, |state| {
        ensure!(matches!(state.app_state.state, AppState::Operational));
        Ok(())
    })
    .await
    .unwrap();

    let mut tasks = vec![];
    for i in 1..=10 {
        let client = client.clone();
        let task = tokio::spawn(async move {
            let market_id = i * 2;
            broadcast(
                ADMIN_SECRET_KEY,
                AppMessage::AddMarket {
                    id: market_id,
                    asset_id: AssetId(1),
                    name: format!("Market {}", i),
                },
                &client,
                |state| {
                    ensure!(state.app_state.markets.contains_key(&market_id));
                    Ok(())
                },
            )
            .await
        });
        tasks.push(task);
    }

    for task in tasks {
        task.await.unwrap().unwrap();
    }

    let state = server_state(&client).await.unwrap();
    assert_eq!(
        state.app_state.markets.len(),
        10,
        "All markets should be created"
    );
    assert_eq!(app.try_wait().unwrap(), None);
}

async fn broadcast(
    secret: &str,
    msg: AppMessage,
    client: &reqwest::Client,
    check: impl Fn(ServerState) -> Result<()>,
) -> Result<()> {
    let status = six_sigma_cmd()
        .args([
            "broadcast",
            "--secret",
            secret,
            "--message",
            &serde_json::to_string(&msg)?,
        ])
        .status()
        .await?;
    ensure!(status.success());
    (|| async {
        let state = server_state(client).await?;
        check(state)
    })
    .retry(
        ExponentialBuilder::default()
            .with_min_delay(std::time::Duration::from_millis(50))
            .with_max_times(10),
    )
    .await
}

async fn register_funder_account(client: &reqwest::Client, contract: &Contract) -> Result<()> {
    const SR_BALANCE: Decimal = dec!(123_456);
    let wallet = sr_wallet()?;
    let public_key = PublicKey::from_str(FUNDER_PUBLIC_KEY)?;
    send_funds_with_key_and_find_account(client, contract, &wallet, &public_key, SR_BALANCE)
        .await?;
    Ok(())
}

const ADMIN_SECRET_KEY: &str = "127831b9459b538eab9a338b1e96fc34249a5154c96180106dd87d39117e8e02";

const FUNDER_PUBLIC_KEY: &str =
    "032caf3bb79f995e0a26d8e08aa54c794660d8398cfcb39855ded310492be8815b";
const FUNDER_SECRET_KEY: &str = "2bb0119bcf9ac0d9a8883b2832f3309217d350033ba944193352f034f197b96a";

const _BETTOR_PUBLIC_KEY: &str =
    "03e92af83772943d5a83c40dd35dcf813644655deb6fddb300b1cd6f146a53a4d3";
const _BETTOR_SECRET_KEY: &str = "6075334f2d4f254147fe37cb87c962112f9dad565720dd128b37b4ed07431690";

async fn _create_and_register_bettor_account(
    cosmos: &Cosmos,
    client: &reqwest::Client,
    contract: &Contract,
) -> Result<(cosmos::Wallet, AccountId)> {
    _create_wallet_and_register_account(cosmos, client, contract, _BETTOR_PUBLIC_KEY, dec!(345_678))
        .await
}

async fn _create_wallet_and_register_account(
    cosmos: &Cosmos,
    client: &reqwest::Client,
    contract: &Contract,
    public_key: &str,
    marker_amount: Decimal,
) -> Result<(cosmos::Wallet, AccountId)> {
    let cosmos_balance = marker_amount + dec!(1.0); // extra coins for feees
    let new_wallet = wallet_from_seed(SeedPhrase::random())?;
    let wallet = sr_wallet()?;
    wallet
        .send_coins(cosmos, new_wallet.get_address(), vec![coin(cosmos_balance)])
        .await?;
    let public_key = PublicKey::from_str(public_key)?;
    let account_id = send_funds_with_key_and_find_account(
        client,
        contract,
        &new_wallet,
        &public_key,
        marker_amount,
    )
    .await?;
    Ok((new_wallet, account_id))
}

fn coin(amount: Decimal) -> Coin {
    Coin {
        amount: u128::try_from(amount * dec!(1_000_000))
            .unwrap()
            .to_string(),
        denom: "uosmo".to_string(),
    }
}

// currently we use a very hacky way to find added accounts using funds transferred,
// TODO we should have a way to get notified when bridge message lands on Kolme
// TODO we should have a way to match Kolme accounts to their public keys
async fn send_funds_with_key_and_find_account(
    client: &reqwest::Client,
    contract: &Contract,
    wallet: &cosmos::Wallet,
    public_key: &PublicKey,
    to_send: Decimal,
) -> Result<AccountId> {
    let resp = contract
        .execute(
            wallet,
            vec![coin(to_send)],
            ExecuteMsg::Regular {
                keys: vec![*public_key],
            },
        )
        .await?;
    assert!(resp.code == 0);
    (|| async {
        tracing::debug!("checking if account was created");
        let state = server_state(client).await?;
        let acc = state
            .balances
            .iter()
            .find_map(|(acc_id, assets)| {
                // we don't create any other assets for this app
                assets
                    .get(&AssetId(1))
                    .is_some_and(|balance| *balance == to_send)
                    .then_some(*acc_id)
            })
            .ok_or(anyhow!(
                "account with the corresponding balance {to_send} should exist, last seen server state: {state:?}"
            ))?;
        Ok(acc)
    })
    .retry(
        ExponentialBuilder::default()
            .with_min_delay(std::time::Duration::from_millis(50))
            .with_max_times(8),
    )
    .await
}

fn sr_wallet() -> Result<cosmos::Wallet> {
    wallet_from_seed(SeedPhrase::from_str(SR_SEED_PHRASE)?)
}

fn wallet_from_seed(seed_phrase: SeedPhrase) -> Result<cosmos::Wallet> {
    seed_phrase
        .with_hrp(AddressHrp::from_str("osmo")?)
        .map_err(Into::into)
}

// we use lo-test1 for this as we have a lot of funds in that wallet
const SR_SEED_PHRASE: &str = "notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius";

async fn prebuild() -> Result<()> {
    // prebuild first
    let mut cmd = Command::new("cargo");
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["build", "-p", "kolme", "--example", "six-sigma"])
        .spawn()?
        .wait()
        .await?;
    Ok(())
}

fn six_sigma_cmd() -> Command {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(env!("CARGO_MANIFEST_DIR")).args([
        "run",
        "-p",
        "kolme",
        "--example",
        "six-sigma",
    ]);
    cmd
}

async fn start_the_app(log_path: PathBuf) -> Result<Child> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(env!("CARGO_MANIFEST_DIR")).args([
        "run",
        "-p",
        "kolme",
        "--example",
        "six-sigma",
        "serve",
        "--tx-log-path",
        log_path.to_str().unwrap(),
    ]);
    if let Ok(rust_log) = std::env::var("RUST_LOG") {
        tracing::debug!("Setting RUST_LOG to {rust_log}");
        cmd.env("RUST_LOG", rust_log);
    }

    cmd.kill_on_drop(true).spawn().map_err(Into::into)
}

async fn wait_contract_deployed(client: &reqwest::Client) -> Result<String> {
    (|| async {
        let state = server_state(client).await?;
        let BridgeContract::Deployed(ref contract_addr) = state
            .bridges
            .get(&ExternalChain::OsmosisLocal)
            .expect("localosmosis config should exist")
            .bridge
        else {
            // TODO: shouldn't we wait for initial deployment?
            return Err(anyhow!("bridge contract should be already deployed"));
        };

        Ok(contract_addr.clone())
    })
    .retry(
        ExponentialBuilder::default()
            .with_min_delay(std::time::Duration::from_millis(50))
            .with_max_times(8),
    )
    .await
}

async fn server_state(client: &reqwest::Client) -> Result<ServerState> {
    let resp = client.get("http://localhost:3000").send().await?;
    ensure!(resp.status().is_success());
    resp.json::<ServerState>().await.map_err(Into::into)
}

// copied over as we don't have notifications yet
#[derive(serde::Deserialize, Debug)]
struct ServerState {
    bridges: BTreeMap<ExternalChain, ChainConfig>,
    balances: BTreeMap<AccountId, BTreeMap<AssetId, Decimal>>,
    app_state: SixSigmaState,
}

#[derive(serde::Deserialize, Debug)]
struct SixSigmaState {
    state: AppState,
    strategic_reserve: HashMap<AssetId, Decimal>,
    markets: HashMap<u64, Market>,
}

#[allow(dead_code)]
#[derive(serde::Deserialize, Debug)]
struct Market {
    state: MarketState,
    bets: Vec<Bet>,
}

#[derive(PartialEq, serde::Deserialize, Debug)]
enum MarketState {
    Operational,
    Settled,
}

#[derive(serde::Deserialize, Debug)]
struct Bet {}
