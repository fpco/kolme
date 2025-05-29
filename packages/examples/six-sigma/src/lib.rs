mod state;
mod types;

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::File,
    io::Write,
    net::SocketAddr,
    ops::Deref,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
};

use anyhow::Result;
use cosmos::SeedPhrase;

use serde::{Deserialize, Serialize};
#[allow(unused_imports)] // It's not unused...
use shared::cosmos::KeyRegistration;

use kolme::*;
use rust_decimal::{dec, Decimal};
use solana_keypair::Keypair as SolanaKeypair;
use tokio::task::{AbortHandle, JoinSet};

pub use state::*;
pub use types::*;

#[derive(Clone)]
pub struct SixSigmaApp {
    pub genesis: GenesisInfo,
    pub chain: ExternalChain,
    make_submitter: Arc<MakeSubmitterFn>,
}

const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";
// lo-test1
const SUBMITTER_SEED_PHRASE: &str = "notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius";

const ADMIN_SECRET_KEY_HEX: &str =
    "127831b9459b538eab9a338b1e96fc34249a5154c96180106dd87d39117e8e02";

const LOCALOSMOSIS_CODE_ID: u64 = 1;

const DUMMY_CODE_VERSION: &str = "dummy code version";

type MakeSubmitterFn = dyn Fn(Kolme<SixSigmaApp>) -> Submitter<SixSigmaApp> + Send + Sync;

fn my_secret_key() -> SecretKey {
    SecretKey::from_hex(SECRET_KEY_HEX).unwrap()
}

fn admin_secret_key() -> SecretKey {
    SecretKey::from_hex(ADMIN_SECRET_KEY_HEX).unwrap()
}

pub enum BalanceChange {
    Mint {
        asset_id: AssetId,
        amount: Decimal,
        account: AccountId,
        withdraw_wallet: Wallet,
    },
    Burn {
        asset_id: AssetId,
        amount: Decimal,
        account: AccountId,
    },
}

impl SixSigmaApp {
    pub fn new_cosmos() -> Self {
        let mut chains = ConfiguredChains::default();

        let mut assets = BTreeMap::new();
        assets.insert(
            AssetName("uosmo".to_owned()),
            AssetConfig {
                decimals: 6,
                asset_id: AssetId(1),
            },
        );

        chains
            .insert_cosmos(
                CosmosChain::OsmosisLocal,
                ChainConfig {
                    assets,
                    bridge: BridgeContract::NeededCosmosBridge {
                        code_id: LOCALOSMOSIS_CODE_ID,
                    },
                },
            )
            .unwrap();

        let make_submitter = Arc::new(|kolme: Kolme<SixSigmaApp>| {
            Submitter::new_cosmos(kolme, SeedPhrase::from_str(SUBMITTER_SEED_PHRASE).unwrap())
        });

        Self::new(chains, ExternalChain::OsmosisLocal, make_submitter)
    }

    pub fn new_solana(submitter: SolanaKeypair) -> Self {
        let mut chains = ConfiguredChains::default();

        let mut assets = BTreeMap::new();
        assets.insert(
            AssetName("osmof7hTFAuNjwMCcxVNThBDDftMNjiLR2cidDQzvwQ".to_owned()),
            AssetConfig {
                decimals: 6,
                asset_id: AssetId(1),
            },
        );

        chains
            .insert_solana(
                SolanaChain::Local,
                ChainConfig {
                    assets,
                    bridge: BridgeContract::NeededSolanaBridge {
                        program_id: "7Y2ftN9nSf4ubzRDiUvcENMeV4S695JEFpYtqdt836pW".into(),
                    },
                },
            )
            .unwrap();

        let submitter = Arc::new(submitter.to_bytes());
        let make_submitter = Arc::new(move |kolme: Kolme<SixSigmaApp>| {
            let submitter = SolanaKeypair::from_bytes(submitter.deref()).unwrap();
            Submitter::new_solana(kolme, submitter)
        });

        Self::new(chains, ExternalChain::SolanaLocal, make_submitter)
    }

    pub fn new_passthrough() -> Self {
        let mut chains = ConfiguredChains::default();

        let mut assets = BTreeMap::new();
        assets.insert(
            AssetName("uosmo".to_owned()),
            AssetConfig {
                decimals: 6,
                asset_id: AssetId(1),
            },
        );

        chains
            .insert_pass_through(ChainConfig {
                assets,
                bridge: BridgeContract::Deployed("12345".to_string()),
            })
            .unwrap();

        let make_submitter =
            Arc::new(|kolme: Kolme<SixSigmaApp>| Submitter::new_pass_through(kolme.clone(), 12345));

        Self::new(chains, ExternalChain::PassThrough, make_submitter)
    }

    fn new(
        chains: ConfiguredChains,
        chain: ExternalChain,
        make_submitter: Arc<MakeSubmitterFn>,
    ) -> Self {
        let my_public_key = my_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);

        let genesis = GenesisInfo {
            kolme_ident: "Six sigma example".to_owned(),
            validator_set: ValidatorSet {
                processor: my_public_key,
                listeners: set.clone(),
                needed_listeners: 1,
                approvers: set,
                needed_approvers: 1,
            },
            chains,
        };

        Self {
            genesis,
            chain,
            make_submitter,
        }
    }
}

impl KolmeApp for SixSigmaApp {
    type State = State;
    type Message = AppMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
    }

    fn new_state() -> Result<Self::State> {
        Ok(State::new([admin_secret_key().public_key()]))
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> Result<()> {
        match msg {
            AppMessage::SendFunds { asset_id, amount } => {
                ctx.state_mut().send_funds(*asset_id, *amount)
            }
            AppMessage::Init => {
                let signing_key = ctx.get_signing_key();
                ctx.state_mut().initialize(signing_key)
            }
            AppMessage::AddMarket { id, asset_id, name } => {
                let signing_key = ctx.get_signing_key();
                ctx.state_mut()
                    .add_market(signing_key, *id, *asset_id, name)
            }
            AppMessage::PlaceBet {
                wallet,
                amount,
                market_id,
                outcome,
            } => {
                let sender = ctx.get_sender_id();
                let balances = ctx
                    .get_account_balances(&sender)
                    .map_or(Default::default(), Clone::clone);
                let odds = ctx.load_data(OddsSource).await?;
                let change = ctx.state_mut().place_bet(
                    sender,
                    balances,
                    *market_id,
                    Wallet(wallet.clone()),
                    *amount,
                    *outcome,
                    odds,
                )?;
                change_balance(ctx, &change)
            }
            AppMessage::SettleMarket { market_id, outcome } => {
                let signing_key = ctx.get_signing_key();
                let balance_changes =
                    ctx.state_mut()
                        .settle_market(signing_key, *market_id, *outcome)?;
                for change in balance_changes {
                    change_balance(ctx, &change)?
                }
                Ok(())
            }
        }
    }
}

impl fmt::Debug for SixSigmaApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SixSigmaApp")
            .field("genesis", &self.genesis)
            .field("chain", &self.chain)
            .finish()
    }
}

fn change_balance(
    ctx: &mut ExecutionContext<'_, SixSigmaApp>,
    change: &BalanceChange,
) -> Result<()> {
    match change {
        BalanceChange::Mint {
            asset_id,
            amount,
            account,
            withdraw_wallet,
        } => {
            tracing::debug!("withdraw");
            let chain = ctx.app().chain;
            ctx.mint_asset(*asset_id, *account, *amount)?;
            let bridge_action_id =
                ctx.withdraw_asset(*asset_id, chain, *account, withdraw_wallet, *amount)?;
            ctx.log(serde_json::to_string(&Event::Withdrawal {
                bridge_action_id,
            })?);
            Ok(())
        }
        BalanceChange::Burn {
            asset_id,
            amount,
            account,
        } => ctx.burn_asset(*asset_id, *account, *amount),
    }
}

#[derive(Serialize, Deserialize, Debug)]
enum Event {
    Withdrawal { bridge_action_id: BridgeActionId },
}

pub const OUTCOME_COUNT: u8 = 3;

pub type Odds = [Decimal; OUTCOME_COUNT as usize];

#[derive(PartialEq, serde::Serialize, serde::Deserialize)]
struct OddsSource;

impl<App> KolmeDataRequest<App> for OddsSource {
    type Response = Odds;

    async fn load(self, _: &App) -> Result<Self::Response> {
        Ok([dec!(1.8), dec!(2.5), dec!(6.5)])
    }

    async fn validate(self, _: &App, _: &Self::Response) -> Result<()> {
        // No validation possible
        Ok(())
    }
}

pub struct Tasks {
    pub set: JoinSet<Result<()>>,
    pub processor: Option<AbortHandle>,
    pub listener: Option<AbortHandle>,
    pub approver: Option<AbortHandle>,
    pub submitter: Option<AbortHandle>,
    pub api_server: Option<AbortHandle>,
    kolme: Kolme<SixSigmaApp>,
    bind: SocketAddr,
}

impl Tasks {
    pub fn spawn_processor(&mut self) {
        let processor = Processor::new(self.kolme.clone(), my_secret_key().clone());
        self.processor = Some(self.set.spawn(processor.run()));
    }

    pub fn spawn_listener(&mut self) {
        let chain = self.kolme.get_app().chain;
        let listener = Listener::new(self.kolme.clone(), my_secret_key().clone());

        self.listener = Some(self.set.spawn(listener.run(chain.name())));
    }

    pub fn spawn_approver(&mut self) {
        let approver = Approver::new(self.kolme.clone(), my_secret_key().clone());
        self.approver = Some(self.set.spawn(approver.run()));
    }

    pub fn spawn_submitter(&mut self) {
        let make_submitter = self.kolme.get_app().make_submitter.clone();
        let submitter = (make_submitter)(self.kolme.clone());

        self.submitter = Some(self.set.spawn(submitter.run()));
    }

    pub fn spawn_api_server(&mut self) {
        let api_server = ApiServer::new(self.kolme.clone());
        self.api_server = Some(self.set.spawn(api_server.run(self.bind)));
    }
}

pub async fn serve(
    app: SixSigmaApp,
    bind: SocketAddr,
    db_path: PathBuf,
    tx_log_path: Option<PathBuf>,
    component: Option<AppComponent>,
) -> Result<()> {
    tracing::info!("starting");

    let mut tasks = run_tasks(app, bind, db_path, tx_log_path, component).await?;
    while let Some(res) = tasks.set.join_next().await {
        match res {
            Err(e) => {
                tasks.set.abort_all();
                return Err(anyhow::anyhow!("Task panicked: {e}"));
            }
            Ok(Err(e)) => {
                tasks.set.abort_all();
                return Err(e);
            }
            Ok(Ok(())) => (),
        }
    }

    Ok(())
}

pub async fn run_tasks(
    app: SixSigmaApp,
    bind: SocketAddr,
    db_path: PathBuf,
    tx_log_path: Option<PathBuf>,
    component: Option<AppComponent>,
) -> Result<Tasks> {
    let kolme = Kolme::new(app, DUMMY_CODE_VERSION, KolmeStore::new_fjall(db_path)?).await?;

    let mut tasks = Tasks {
        set: JoinSet::new(),
        processor: None,
        listener: None,
        approver: None,
        submitter: None,
        api_server: None,
        kolme: kolme.clone(),
        bind,
    };

    if let Some(tx_log_path) = tx_log_path {
        let logger = TxLogger::new(kolme.clone(), tx_log_path);
        tasks.set.spawn(logger.run());
    }

    match component {
        Some(AppComponent::Processor) => {
            tracing::info!("Running processor ...");
            tasks.spawn_processor();
        }
        Some(AppComponent::Listener) => {
            tracing::info!("Running listener ...");
            tasks.spawn_listener();
        }
        Some(AppComponent::Approver) => {
            tracing::info!("Running approver ...");
            tasks.spawn_approver();
        }
        Some(AppComponent::Submitter) => {
            tracing::info!("Running submitter ...");
            tasks.spawn_submitter();
        }
        Some(AppComponent::ApiServer) => {
            tracing::info!("Running api-server ...");
            tasks.spawn_api_server();
        }
        None => {
            tracing::info!("Running in monolith mode ...");
            tasks.spawn_processor();
            tasks.spawn_listener();
            tasks.spawn_approver();
            tasks.spawn_submitter();
            tasks.spawn_api_server();
        }
    }

    Ok(tasks)
}

pub async fn state(app: SixSigmaApp, db_path: PathBuf) -> Result<State> {
    let kolme = Kolme::new(app, DUMMY_CODE_VERSION, KolmeStore::new_fjall(db_path)?)
        .await?
        .read();
    Ok(kolme.get_app_state().clone())
}

struct TxLogger {
    kolme: Kolme<SixSigmaApp>,
    path: PathBuf,
}

impl TxLogger {
    fn new(kolme: Kolme<SixSigmaApp>, path: PathBuf) -> Self {
        Self { kolme, path }
    }

    async fn run(self) -> Result<()> {
        let mut file = File::create(self.path)?;
        let mut receiver = self.kolme.subscribe();
        loop {
            let notification = receiver.recv().await?;
            let output = match notification.clone() {
                Notification::FailedTransaction(_) => continue,
                Notification::NewBlock(msg) => {
                    let block = msg.0.message.as_inner();
                    let height = block.height;
                    let messages = block
                        .tx
                        .0
                        .message
                        .as_inner()
                        .messages
                        .iter()
                        .cloned()
                        .map(LoggedMessage::from)
                        .collect();
                    LogOutput::NewBlock { height, messages }
                }
                Notification::GenesisInstantiation { .. } => LogOutput::GenesisInstantiation,
                Notification::LatestBlock(_) => continue,
            };
            serde_json::to_writer(&file, &output)?;
            writeln!(file)?;
            file.flush()?;
            tracing::info!("Log output: {notification:?}");
        }
    }
}

pub async fn broadcast(message: String, secret: String, host: String) -> Result<Sha256Hash> {
    let message = serde_json::from_str::<AppMessage>(&message)?;
    let secret = SecretKey::from_hex(&secret)?;
    let public = secret.public_key();
    let client = reqwest::Client::new();

    println!("Public key: {public}");

    #[derive(serde::Deserialize)]
    struct NonceResp {
        next_nonce: AccountNonce,
    }
    let NonceResp { next_nonce: nonce } = client
        .get(format!("{host}/get-next-nonce"))
        .query(&[("pubkey", public)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let signed = Transaction {
        pubkey: public,
        nonce,
        created: jiff::Timestamp::now(),
        messages: vec![Message::App(message)],
        max_height: None,
    }
    .sign(&secret)?;
    #[derive(serde::Deserialize)]
    struct Res {
        txhash: Sha256Hash,
    }
    let res = client
        .put(format!("{host}/broadcast"))
        .json(&signed)
        .send()
        .await?;

    if let Err(e) = res.error_for_status_ref() {
        let t = res.text().await?;
        anyhow::bail!("Error broadcasting:\n{e}\n{t}");
    }

    let Res { txhash } = res.json().await?;
    println!("txhash: {txhash}");
    Ok(txhash)
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, BufRead},
        path::Path,
        sync::Arc,
    };

    use anyhow::{ensure, Context};
    use backon::{ExponentialBuilder, Retryable};
    use cosmos::{AddressHrp, HasAddress};
    use futures_util::StreamExt;
    use pass_through::{MsgResponse, PassThrough};
    use shared::cosmos::ExecuteMsg;
    use tempfile::NamedTempFile;
    use tokio::time::{Duration, Instant};
    use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

    use super::*;

    #[test_log::test(tokio::test)]
    async fn basic_pass_through_scenario() {
        println!("In");
        tracing::debug!("starting basic pass through scenario");
        let passthrough = PassThrough::new();
        let passthrough =
            tokio::task::spawn(passthrough.run(SocketAddr::from_str("[::]:12345").unwrap()));
        assert!(!passthrough.is_finished());
        let db_file = tempfile::tempdir().unwrap();
        let log_file = NamedTempFile::new().unwrap();
        let db_path = db_file.path().to_path_buf();
        let app = tokio::task::spawn(serve(
            SixSigmaApp::new_passthrough(),
            SocketAddr::from_str("[::]:3001").unwrap(),
            db_path.clone(),
            Some(log_file.path().to_path_buf()),
            None,
        ));
        let client = reqwest::Client::new();

        tracing::debug!("wait for the app to be up");
        (|| async { server_state(&client).await })
            .retry(
                ExponentialBuilder::default()
                    .with_min_delay(std::time::Duration::from_millis(50))
                    .with_max_times(10),
            )
            .await
            .unwrap();

        assert!(!passthrough.is_finished());
        register_funder_account(&client).await.unwrap();
        tracing::debug!("Sending initial funds for the application");
        broadcast_check_state(
            SixSigmaApp::new_passthrough(),
            FUNDER_SECRET_KEY,
            &db_path,
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
        broadcast_check_state(
            SixSigmaApp::new_passthrough(),
            ADMIN_SECRET_KEY,
            &db_path,
            AppMessage::Init,
            |state| {
                ensure!(matches!(state.state, AppState::Operational));
                Ok(())
            },
        )
        .await
        .unwrap();

        tracing::debug!("Creating a market");
        broadcast_check_state(
            SixSigmaApp::new_passthrough(),
            ADMIN_SECRET_KEY,
            &db_path,
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
        // genesis msg, regular for SR, regular for bettor, set_config, add_market
        assert_eq!(
            log_lines,
            vec![
                block_at(0, LoggedMessage::Genesis),
                // no genesis for pass-through
                // registering account and transferring funds into it for funder
                block_at(1, LoggedMessage::Listener(LoggedBridgeEvent::Regular)),
                block_at(
                    2,
                    LoggedMessage::App(AppMessage::SendFunds {
                        asset_id: AssetId(1),
                        amount: dec!(100_000)
                    })
                ),
                block_at(3, LoggedMessage::App(AppMessage::Init)),
                block_at(
                    4,
                    LoggedMessage::App(AppMessage::AddMarket {
                        id: 1,
                        asset_id: AssetId(1),
                        name: "sample market".to_string()
                    })
                )
            ]
        );

        let (bettor_wallet, _bettor_account) =
            create_and_regsiter_bettor_account(&client).await.unwrap();
        tracing::debug!("Placing a bet");
        broadcast_check_state(
            SixSigmaApp::new_passthrough(),
            BETTOR_SECRET_KEY,
            &db_path,
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

        tracing::debug!("Settling the market");
        let (block, logs) = broadcast_wait_tx(
            ADMIN_SECRET_KEY,
            AppMessage::SettleMarket {
                market_id: 1,
                outcome: 0,
            },
        )
        .await
        .unwrap();
        tracing::info!("Landed as tx: {}", block.tx.hash());
        tracing::info!("Logs: {logs:#?}");
        let [_new_bridge_action, log] = logs.as_slice() else {
            panic!("Expected exactly two log lines with withdrawal")
        };
        let Event::Withdrawal { bridge_action_id } =
            serde_json::from_str(log).expect("log line should contain event JSON");
        tracing::info!("Bridge action id: {}", bridge_action_id);

        tracing::debug!("Checking executed transfer action with the award");
        (|| async {
            let resp = client
                .get(format!(
                    "http://localhost:12345/actions/{bridge_action_id}/wait"
                ))
                .send()
                .await
                .unwrap();
            let resp = resp.error_for_status().unwrap();
            let action = resp.json::<pass_through::Action>().await.unwrap();
            let transfer = serde_json::from_str::<pass_through::Transfer>(&action.payload).unwrap();
            ensure!(transfer.recipient.0 == bettor_wallet.get_address_string());
            ensure!(transfer.funds[0].amount == 180000);
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
        assert!(!app.is_finished());
        app.abort();
        assert!(!passthrough.is_finished());
        passthrough.abort();
    }

    const ADMIN_SECRET_KEY: &str =
        "127831b9459b538eab9a338b1e96fc34249a5154c96180106dd87d39117e8e02";

    // const FUNDER_PUBLIC_KEY: &str = "032caf3bb79f995e0a26d8e08aa54c794660d8398cfcb39855ded310492be8815b";
    const FUNDER_SECRET_KEY: &str =
        "2bb0119bcf9ac0d9a8883b2832f3309217d350033ba944193352f034f197b96a";

    // const BETTOR_PUBLIC_KEY: &str = "03e92af83772943d5a83c40dd35dcf813644655deb6fddb300b1cd6f146a53a4d3";
    const BETTOR_SECRET_KEY: &str =
        "6075334f2d4f254147fe37cb87c962112f9dad565720dd128b37b4ed07431690";

    async fn register_funder_account(client: &reqwest::Client) -> Result<()> {
        const SR_BALANCE: Decimal = dec!(123_456);
        let wallet = sr_wallet()?;
        let secret_key = SecretKey::from_str(FUNDER_SECRET_KEY)?;
        let registration = KeyRegistration::new(&wallet.get_address_string(), &secret_key)?;

        send_funds_with_key_and_find_account_pass_through(
            client,
            wallet.to_string(),
            registration,
            SR_BALANCE,
        )
        .await?;
        Ok(())
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

    async fn create_and_regsiter_bettor_account(
        client: &reqwest::Client,
    ) -> Result<(cosmos::Wallet, AccountId)> {
        create_wallet_and_regsiter_account(client, BETTOR_SECRET_KEY, dec!(345_678)).await
    }

    async fn create_wallet_and_regsiter_account(
        client: &reqwest::Client,
        secret_key: &str,
        marker_amount: Decimal,
    ) -> Result<(cosmos::Wallet, AccountId)> {
        let new_wallet = wallet_from_seed(SeedPhrase::random())?;
        let secret_key = SecretKey::from_str(secret_key)?;
        let registration = KeyRegistration::new(&new_wallet.get_address_string(), &secret_key)?;

        let account_id = send_funds_with_key_and_find_account_pass_through(
            client,
            new_wallet.to_string(),
            registration,
            marker_amount,
        )
        .await?;

        Ok((new_wallet, account_id))
    }

    async fn send_funds_with_key_and_find_account_pass_through(
        client: &reqwest::Client,
        wallet: String,
        registration: KeyRegistration,
        to_send: Decimal,
    ) -> Result<AccountId> {
        let ws = connect_ws_notifications().await;
        let resp = client
            .post("http://localhost:12345/msg")
            .json(&kolme::pass_through::Msg {
                wallet,
                coins: vec![std_coin(to_send)],
                msg: ExecuteMsg::Regular {
                    keys: vec![registration],
                },
            })
            .send()
            .await?;
        ensure!(resp.status().is_success());
        let MsgResponse { bridge_event_id } = resp.json().await?;
        wait_tx(ws, |_block, logs| {
            logs.iter().flatten().find_map(|log| {
                serde_json::from_str::<LogEvent>(log)
                    .ok()
                    .and_then(|log_event| match log_event {
                        LogEvent::ProcessedBridgeEvent(LogBridgeEvent::Regular {
                            bridge_event_id: event_id,
                            account_id,
                        }) => (event_id == bridge_event_id).then_some(account_id),
                        _ => unreachable!(),
                    })
            })
        })
        .await
    }

    async fn server_state(client: &reqwest::Client) -> Result<()> {
        let resp = client.get("http://localhost:3001").send().await?;
        ensure!(resp.status().is_success());
        ensure!(resp.json::<serde_json::Value>().await.is_ok());
        Ok(())
    }

    fn std_coin(amount: Decimal) -> cosmwasm_std::Coin {
        cosmwasm_std::Coin {
            amount: cosmwasm_std::Uint128::new(u128::try_from(amount * dec!(1_000_000)).unwrap()),
            denom: "uosmo".to_string(),
        }
    }

    async fn broadcast_check_state(
        app: SixSigmaApp,
        secret: &str,
        db_path: &Path,
        msg: AppMessage,
        check: impl Fn(State) -> Result<()>,
    ) -> Result<()> {
        broadcast(
            serde_json::to_string(&msg).unwrap(),
            secret.to_string(),
            "http://localhost:3001".to_string(),
        )
        .await?;
        (|| async {
            let state = state(app.clone(), db_path.to_path_buf()).await?;
            check(state)
        })
        .retry(
            ExponentialBuilder::default()
                .with_min_delay(std::time::Duration::from_millis(50))
                .with_max_times(10),
        )
        .await
    }

    type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

    async fn connect_ws_notifications() -> WsStream {
        let ws_url = format!("ws://localhost:{}/notifications", 3001);
        connect_async(&ws_url).await.unwrap().0
    }

    async fn wait_tx<F, R>(mut ws: WsStream, check_result: F) -> Result<R>
    where
        F: Fn(Arc<SignedBlock<AppMessage>>, Arc<[Vec<String>]>) -> Option<R>,
    {
        let timeout_at = Instant::now() + Duration::from_secs(2);
        loop {
            let message = tokio::time::timeout_at(timeout_at, ws.next())
                .await?
                .context("WebSocket stream terminated")??;
            let notification =
                serde_json::from_slice::<ApiNotification<AppMessage>>(&message.into_data())?;
            if let ApiNotification::NewBlock { block, logs } = notification {
                if let Some(result) = check_result(block, logs) {
                    return Ok(result);
                }
            }
        }
    }

    async fn broadcast_wait_tx(
        secret: &str,
        msg: AppMessage,
    ) -> Result<(Block<AppMessage>, Vec<String>)> {
        let ws = connect_ws_notifications().await;
        let txhash = broadcast(
            serde_json::to_string(&msg).unwrap(),
            secret.to_string(),
            "http://localhost:3001".to_string(),
        )
        .await?;
        wait_tx(ws, |block, logs| {
            if block.tx().hash().0 == txhash {
                let block = block.0.message.as_inner().clone();
                let logs = logs.iter().flatten().cloned().collect();
                Some((block, logs))
            } else {
                None
            }
        })
        .await
    }
}
