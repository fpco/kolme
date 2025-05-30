use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    io::{self, BufRead},
    path::PathBuf,
    str::FromStr,
};

use anyhow::{anyhow, ensure, Result};
use backon::{ExponentialBuilder, Retryable};
use cosmos::{AddressHrp, Coin, Contract, Cosmos, HasAddress, SeedPhrase};
use pretty_assertions::assert_eq;
use rust_decimal::{dec, Decimal};
use shared::{cosmos::ExecuteMsg, types::KeyRegistration};
use tempfile::NamedTempFile;
use tokio::process::{Child, Command};

use example_six_sigma::*;
use kolme::*;

#[test_log::test(tokio::test)]
#[ignore = "depends on localosmosis thus hidden from default tests, also doesn't work with pass-through enable"]
async fn basic_scenario() {
    tracing::debug!("starting");
    prebuild().await.unwrap();
    let log_file = NamedTempFile::new().unwrap();
    let mut app = start_the_app(log_file.path().to_path_buf()).await.unwrap();
    let client = reqwest::Client::new();
    let contract_addr = wait_contract_deployed(&client).await.unwrap();
    tracing::debug!("contract: {contract_addr}");

    let cosmos = cosmos::CosmosNetwork::OsmosisLocal
        .builder_local()
        .build()
        .unwrap();
    let contract = cosmos.make_contract(contract_addr.parse().unwrap());

    register_funder_account(&client, &contract).await.unwrap();
    tracing::debug!("Sending initial funds for the application");
    broadcast_check_state(
        FUNDER_SECRET_KEY,
        AppMessage::SendFunds {
            asset_id: AssetId(1),
            amount: dec!(100_000),
        },
        |state| {
            ensure!(state
                .strategic_reserve
                .get(&AssetId(1))
                .is_some_and(|balance| balance == &dec!(100_000)));
            Ok(())
        },
    )
    .await
    .unwrap();

    tracing::debug!("Initializing the contract");
    broadcast_check_state(ADMIN_SECRET_KEY, AppMessage::Init, |state| {
        ensure!(matches!(state.state, AppState::Operational));
        Ok(())
    })
    .await
    .unwrap();

    tracing::debug!("Creating a market");
    broadcast_check_state(
        ADMIN_SECRET_KEY,
        AppMessage::AddMarket {
            id: 1,
            asset_id: AssetId(1),
            name: "sample market".to_string(),
        },
        |state| {
            ensure!(state.markets.len() == 1);
            Ok(())
        },
    )
    .await
    .unwrap();

    let log_lines = io::BufReader::new(log_file.as_file())
        .lines()
        .map(|line| -> Result<_> {
            let line = line?;
            serde_json::from_str::<LogOutput>(&line).map_err(Into::into)
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    fn block_at(height: u64, msg: LoggedMessage) -> LogOutput {
        LogOutput::NewBlock {
            height: BlockHeight(height),
            messages: vec![msg],
        }
    }

    // we should see the following notifications:
    // genesis msg, genesis notification, contract instantiation,
    // regular for SR, regular for bettor, set_config, add_market
    assert_eq!(
        log_lines,
        vec![
            block_at(0, LoggedMessage::Genesis),
            LogOutput::GenesisInstantiation,
            block_at(1, LoggedMessage::Listener(LoggedBridgeEvent::Instantiated)),
            // registering account and transferring funds into it for funder
            block_at(2, LoggedMessage::Listener(LoggedBridgeEvent::Regular)),
            block_at(
                3,
                LoggedMessage::App(AppMessage::SendFunds {
                    asset_id: AssetId(1),
                    amount: dec!(100_000)
                })
            ),
            block_at(4, LoggedMessage::App(AppMessage::Init)),
            block_at(
                5,
                LoggedMessage::App(AppMessage::AddMarket {
                    id: 1,
                    asset_id: AssetId(1),
                    name: "sample market".to_string()
                })
            )
        ]
    );

    let (bettor_wallet, _bettor_account) =
        create_and_regsiter_bettor_account(&cosmos, &client, &contract)
            .await
            .unwrap();
    tracing::debug!("Placing a bet");
    broadcast_check_state(
        BETTOR_SECRET_KEY,
        AppMessage::PlaceBet {
            wallet: bettor_wallet.get_address_string(),
            amount: dec!(0.1),
            market_id: 1,
            outcome: 0,
        },
        |state| {
            ensure!(state
                .markets
                .iter()
                .next()
                .is_some_and(|(_, market)| !market.bets.is_empty()));
            Ok(())
        },
    )
    .await
    .unwrap();
    let balance_after_bet = u128::from_str(
        &cosmos
            .all_balances(bettor_wallet.get_address())
            .await
            .unwrap()[0]
            .amount,
    )
    .unwrap();

    tracing::debug!("Settling the market");
    broadcast_check_state(
        ADMIN_SECRET_KEY,
        AppMessage::SettleMarket {
            market_id: 1,
            outcome: 0,
        },
        |state| {
            ensure!(state
                .markets
                .iter()
                .next()
                .is_some_and(|(_, market)| market.state == MarketState::Settled));
            Ok(())
        },
    )
    .await
    .unwrap();

    tracing::debug!("Checking if award was received");
    (|| async {
        let balances = cosmos
            .all_balances(bettor_wallet.get_address())
            .await
            .unwrap();
        ensure!(
            balances
                == vec![Coin {
                    denom: "uosmo".to_string(),
                    // with odds 1.8 the bettor is supposed to get 0.18 as award
                    amount: (balance_after_bet + 180000).to_string()
                }]
        );
        Ok(())
    })
    .retry(
        ExponentialBuilder::default()
            .with_min_delay(std::time::Duration::from_millis(50))
            .with_max_times(10),
    )
    .await
    .unwrap();

    // extra check that the server is still alive
    assert_eq!(app.try_wait().unwrap(), None);
}

async fn broadcast_check_state(
    secret: &str,
    msg: AppMessage,
    check: impl Fn(SixSigmaState) -> Result<()>,
) -> Result<()> {
    let cmd = six_sigma_cmd();
    basic_broadcast_check_state(secret, msg, "http://localhost:3000", cmd, state, check).await
}

async fn basic_broadcast_check_state<F, Fut>(
    secret: &str,
    msg: AppMessage,
    host: &str,
    mut cmd: Command,
    get_state: F,
    check: impl Fn(SixSigmaState) -> Result<()>,
) -> Result<()>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<SixSigmaState>>,
{
    cmd.args([
        "broadcast",
        "--secret",
        secret,
        "--message",
        &serde_json::to_string(&msg)?,
        "--host",
        host,
    ]);
    if let Ok(rust_log) = std::env::var("RUST_LOG") {
        cmd.env("RUST_LOG", rust_log);
    }
    let status = cmd.status().await?;
    ensure!(status.success());
    (|| async {
        let state = get_state().await?;
        check(state)
    })
    .retry(
        ExponentialBuilder::default()
            .with_min_delay(std::time::Duration::from_millis(50))
            .with_max_times(10),
    )
    .await
}

async fn state() -> Result<SixSigmaState> {
    let out = six_sigma_cmd().args(["state"]).output().await?;
    ensure!(
        out.status.success(),
        "err was: {}",
        std::str::from_utf8(&out.stderr).unwrap()
    );
    serde_json::from_slice(&out.stdout).map_err(Into::into)
}

async fn register_funder_account(client: &reqwest::Client, contract: &Contract) -> Result<()> {
    const SR_BALANCE: Decimal = dec!(123_456);
    let wallet = sr_wallet()?;
    let public_key = SecretKey::from_str(FUNDER_SECRET_KEY)?;
    send_funds_with_key_and_find_account(client, contract, &wallet, &public_key, SR_BALANCE)
        .await?;
    Ok(())
}

const ADMIN_SECRET_KEY: &str = "127831b9459b538eab9a338b1e96fc34249a5154c96180106dd87d39117e8e02";

//const FUNDER_PUBLIC_KEY: &str = "032caf3bb79f995e0a26d8e08aa54c794660d8398cfcb39855ded310492be8815b";
const FUNDER_SECRET_KEY: &str = "2bb0119bcf9ac0d9a8883b2832f3309217d350033ba944193352f034f197b96a";

// const BETTOR_PUBLIC_KEY: &str = "03e92af83772943d5a83c40dd35dcf813644655deb6fddb300b1cd6f146a53a4d3";
const BETTOR_SECRET_KEY: &str = "6075334f2d4f254147fe37cb87c962112f9dad565720dd128b37b4ed07431690";

async fn create_and_regsiter_bettor_account(
    cosmos: &Cosmos,
    client: &reqwest::Client,
    contract: &Contract,
) -> Result<(cosmos::Wallet, AccountId)> {
    create_wallet_and_regsiter_account(cosmos, client, contract, BETTOR_SECRET_KEY, dec!(345_678))
        .await
}

async fn create_wallet_and_regsiter_account(
    cosmos: &Cosmos,
    client: &reqwest::Client,
    contract: &Contract,
    secret_key: &str,
    marker_amount: Decimal,
) -> Result<(cosmos::Wallet, AccountId)> {
    let cosmos_balance = marker_amount + dec!(1.0); // extra coins for feees
    let new_wallet = wallet_from_seed(SeedPhrase::random())?;
    let wallet = sr_wallet()?;
    wallet
        .send_coins(cosmos, new_wallet.get_address(), vec![coin(cosmos_balance)])
        .await?;
    let secret_key = SecretKey::from_str(secret_key)?;
    let account_id = send_funds_with_key_and_find_account(
        client,
        contract,
        &new_wallet,
        &secret_key,
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
    secret_key: &SecretKey,
    to_send: Decimal,
) -> Result<AccountId> {
    let resp = contract
        .execute(
            wallet,
            vec![coin(to_send)],
            ExecuteMsg::Regular {
                keys: vec![KeyRegistration::cosmos(
                    &wallet.get_address_string(),
                    secret_key,
                )?],
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
        .args(["build", "-p", "example-six-sigma"])
        .spawn()?
        .wait()
        .await?;
    Ok(())
}

fn six_sigma_cmd() -> Command {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["run", "-p", "example-six-sigma"]);
    cmd
}

async fn start_the_app(log_path: PathBuf) -> Result<Child> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(env!("CARGO_MANIFEST_DIR")).args([
        "run",
        "-p",
        "example-six-sigma",
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
}

#[derive(serde::Deserialize, Debug)]
struct SixSigmaState {
    state: AppState,
    strategic_reserve: HashMap<AssetId, Decimal>,
    markets: HashMap<u64, Market>,
}

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
