mod state;
mod types;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Write,
    marker::PhantomData,
    net::SocketAddr,
    path::PathBuf,
    str::FromStr,
};

use anyhow::Result;
use cosmos::SeedPhrase;

use kolme::*;
use rust_decimal::{dec, Decimal};
use tokio::task::JoinSet;

pub use state::*;
pub use types::*;

#[derive(Clone, Debug)]
pub struct SixSigmaApp<C: Config> {
    _phantom: PhantomData<C>,
}

impl<C: Config> Default for SixSigmaApp<C> {
    fn default() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";
// lo-test1
const SUBMITTER_SEED_PHRASE: &str = "notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius";

const ADMIN_SECRET_KEY_HEX: &str =
    "127831b9459b538eab9a338b1e96fc34249a5154c96180106dd87d39117e8e02";

const LOCALOSMOSIS_CODE_ID: u64 = 1;

const DUMMY_CODE_VERSION: &str = "dummy code version";

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

pub trait Config: Clone + Send + Sync + 'static {
    fn insert_bridge(
        bridges: &mut ConfiguredChains,
        assets: BTreeMap<AssetName, AssetConfig>,
    ) -> Result<()>;
    fn chain_name() -> ChainName;
    fn external_chain() -> ExternalChain;
    fn new_submitter(kolme: Kolme<SixSigmaApp<Self>>) -> Submitter<SixSigmaApp<Self>>;
}

#[derive(Clone)]
pub struct SixSigmaCosmos;

impl Config for SixSigmaCosmos {
    fn insert_bridge(
        bridges: &mut ConfiguredChains,
        assets: BTreeMap<AssetName, AssetConfig>,
    ) -> Result<()> {
        bridges.insert_cosmos(
            CosmosChain::OsmosisLocal,
            ChainConfig {
                assets,
                bridge: BridgeContract::NeededCosmosBridge {
                    code_id: LOCALOSMOSIS_CODE_ID,
                },
            },
        )
    }

    fn chain_name() -> ChainName {
        ChainName::Cosmos
    }

    fn external_chain() -> ExternalChain {
        ExternalChain::OsmosisLocal
    }

    fn new_submitter(kolme: Kolme<SixSigmaApp<Self>>) -> Submitter<SixSigmaApp<Self>> {
        Submitter::new_cosmos(kolme, SeedPhrase::from_str(SUBMITTER_SEED_PHRASE).unwrap())
    }
}

#[derive(Clone)]
pub struct SixSigmaPassThrough;

impl Config for SixSigmaPassThrough {
    fn insert_bridge(
        bridges: &mut ConfiguredChains,
        assets: BTreeMap<AssetName, AssetConfig>,
    ) -> Result<()> {
        bridges.insert_pass_through(ChainConfig {
            assets,
            bridge: BridgeContract::Deployed("12345".to_string()),
        })
    }

    fn chain_name() -> ChainName {
        ChainName::PassThrough
    }

    fn external_chain() -> ExternalChain {
        ExternalChain::PassThrough
    }

    fn new_submitter(kolme: Kolme<SixSigmaApp<Self>>) -> Submitter<SixSigmaApp<Self>> {
        Submitter::new_pass_through(kolme.clone(), 12345)
    }
}

impl<C: Config> KolmeApp for SixSigmaApp<C> {
    type State = State;
    type Message = AppMessage;

    fn genesis_info() -> GenesisInfo {
        let my_public_key = my_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        let mut bridges = ConfiguredChains::default();
        let mut assets = BTreeMap::new();
        assets.insert(
            AssetName("uosmo".to_owned()),
            AssetConfig {
                decimals: 6,
                asset_id: AssetId(1),
            },
        );
        C::insert_bridge(&mut bridges, assets).unwrap();
        GenesisInfo {
            kolme_ident: "Six sigma example".to_owned(),
            processor: my_public_key,
            listeners: set.clone(),
            needed_listeners: 1,
            approvers: set,
            needed_approvers: 1,
            chains: bridges,
        }
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

fn change_balance<C: Config>(
    ctx: &mut ExecutionContext<'_, SixSigmaApp<C>>,
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
            ctx.mint_asset(*asset_id, *account, *amount)?;
            ctx.withdraw_asset(
                *asset_id,
                C::external_chain(),
                *account,
                withdraw_wallet,
                *amount,
            )
        }
        BalanceChange::Burn {
            asset_id,
            amount,
            account,
        } => ctx.burn_asset(*asset_id, *account, *amount),
    }
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

pub async fn serve<C: Config>(
    bind: SocketAddr,
    db_path: PathBuf,
    tx_log_path: Option<PathBuf>,
    component: Option<AppComponent>,
) -> Result<()> {
    tracing::info!("starting");
    let kolme = Kolme::new(
        SixSigmaApp::<C>::default(),
        DUMMY_CODE_VERSION,
        KolmeStore::new_sqlite(db_path).await?,
    )
    .await?;

    let mut set = JoinSet::new();

    if let Some(tx_log_path) = tx_log_path {
        let logger = TxLogger::new(kolme.clone(), tx_log_path);
        set.spawn(logger.run());
    }

    match component {
        Some(AppComponent::Processor) => {
            tracing::info!("Running processor ...");
            let processor = Processor::new(kolme.clone(), my_secret_key().clone());
            set.spawn(processor.run());
        }
        Some(AppComponent::Listener) => {
            tracing::info!("Running listener ...");
            let listener = Listener::new(kolme.clone(), my_secret_key().clone());
            set.spawn(listener.run(C::chain_name()));
        }
        Some(AppComponent::Approver) => {
            tracing::info!("Running approver ...");
            let approver = Approver::new(kolme.clone(), my_secret_key().clone());
            set.spawn(approver.run());
        }
        Some(AppComponent::Submitter) => {
            tracing::info!("Running submitter ...");
            let submitter = C::new_submitter(kolme.clone());
            set.spawn(submitter.run());
        }
        Some(AppComponent::ApiServer) => {
            tracing::info!("Running api-server ...");
            let api_server = ApiServer::new(kolme);
            set.spawn(api_server.run(bind));
        }
        None => {
            tracing::info!("Running in monolith mode ...");
            let processor = Processor::new(kolme.clone(), my_secret_key().clone());
            set.spawn(processor.run());
            let listener = Listener::new(kolme.clone(), my_secret_key().clone());
            set.spawn(listener.run(C::chain_name()));
            let approver = Approver::new(kolme.clone(), my_secret_key().clone());
            set.spawn(approver.run());
            let submitter = C::new_submitter(kolme.clone());
            set.spawn(submitter.run());
            let api_server = ApiServer::new(kolme);
            set.spawn(api_server.run(bind));
        }
    }

    while let Some(res) = set.join_next().await {
        match res {
            Err(e) => {
                set.abort_all();
                return Err(anyhow::anyhow!("Task panicked: {e}"));
            }
            Ok(Err(e)) => {
                set.abort_all();
                return Err(e);
            }
            Ok(Ok(())) => (),
        }
    }

    Ok(())
}

pub async fn state<C: Config>(db_path: PathBuf) -> Result<State> {
    let kolme = Kolme::new(
        SixSigmaApp::<C>::default(),
        DUMMY_CODE_VERSION,
        KolmeStore::new_sqlite(db_path).await?,
    )
    .await?
    .read()
    .await;
    Ok(kolme.get_app_state().clone())
}

struct TxLogger<C: Config> {
    kolme: Kolme<SixSigmaApp<C>>,
    path: PathBuf,
}

impl<C: Config> TxLogger<C> {
    fn new(kolme: Kolme<SixSigmaApp<C>>, path: PathBuf) -> Self {
        Self { kolme, path }
    }

    async fn run(self) -> Result<()> {
        let mut file = File::create(self.path)?;
        let mut receiver = self.kolme.subscribe();
        loop {
            let output = match receiver.recv().await? {
                Notification::Broadcast { .. } => {
                    // we skip initial tx broadcast, only tx as a part of a block
                    continue;
                }
                Notification::FailedTransaction { .. } => continue,
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
                        .map(LogMessage::from)
                        .collect();
                    LogOutput::NewBlock { height, messages }
                }
                Notification::GenesisInstantiation { .. } => LogOutput::GenesisInstantiation,
            };
            serde_json::to_writer(&file, &output)?;
            writeln!(file)?;
            file.flush()?;
            tracing::info!("Log output: {output:?}");
        }
    }
}

pub async fn broadcast(message: String, secret: String, host: String) -> Result<()> {
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, BufRead},
        path::Path,
    };

    use anyhow::ensure;
    use backon::{ExponentialBuilder, Retryable};
    use cosmos::{AddressHrp, HasAddress};
    use pass_through::PassThrough;
    use shared::cosmos::ExecuteMsg;
    use tempfile::NamedTempFile;

    use super::*;

    #[test_log::test(tokio::test)]
    async fn basic_pass_through_scenario() {
        println!("In");
        tracing::debug!("starting basic pass through scenario");
        let passthrough = PassThrough::new();
        let passthrough =
            tokio::task::spawn(passthrough.run(SocketAddr::from_str("[::]:12345").unwrap()));
        assert!(!passthrough.is_finished());
        let db_file = NamedTempFile::new().unwrap();
        let log_file = NamedTempFile::new().unwrap();
        let db_path = db_file.path().to_path_buf();
        let app = tokio::task::spawn(serve::<SixSigmaPassThrough>(
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
        broadcast_check_state::<SixSigmaPassThrough>(
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
        broadcast_check_state::<SixSigmaPassThrough>(
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
        broadcast_check_state::<SixSigmaPassThrough>(
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

        fn block_at(height: u64, msg: LogMessage) -> LogOutput {
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
                block_at(0, LogMessage::Genesis),
                // no genesis for pass-through
                // registering account and transferring funds into it for funder
                block_at(1, LogMessage::Listener(LogBridgeEvent::Regular)),
                block_at(
                    2,
                    LogMessage::App(AppMessage::SendFunds {
                        asset_id: AssetId(1),
                        amount: dec!(100_000)
                    })
                ),
                block_at(3, LogMessage::App(AppMessage::Init)),
                block_at(
                    4,
                    LogMessage::App(AppMessage::AddMarket {
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
        broadcast_check_state::<SixSigmaPassThrough>(
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
        broadcast_check_state::<SixSigmaPassThrough>(
            ADMIN_SECRET_KEY,
            &db_path,
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
            let resp = client
                .get("http://localhost:12345/actions")
                .send()
                .await
                .unwrap();
            let resp = resp.error_for_status().unwrap();
            let actions = resp.json::<Vec<pass_through::Action>>().await.unwrap();
            ensure!(actions.len() == 1);
            let transfer =
                serde_json::from_str::<pass_through::Transfer>(&actions[0].payload).unwrap();
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

    const FUNDER_PUBLIC_KEY: &str =
        "032caf3bb79f995e0a26d8e08aa54c794660d8398cfcb39855ded310492be8815b";
    const FUNDER_SECRET_KEY: &str =
        "2bb0119bcf9ac0d9a8883b2832f3309217d350033ba944193352f034f197b96a";

    const BETTOR_PUBLIC_KEY: &str =
        "03e92af83772943d5a83c40dd35dcf813644655deb6fddb300b1cd6f146a53a4d3";
    const BETTOR_SECRET_KEY: &str =
        "6075334f2d4f254147fe37cb87c962112f9dad565720dd128b37b4ed07431690";

    async fn register_funder_account(client: &reqwest::Client) -> Result<()> {
        const SR_BALANCE: Decimal = dec!(123_456);
        let wallet = sr_wallet()?;
        let public_key = PublicKey::from_str(FUNDER_PUBLIC_KEY)?;
        send_funds_with_key_and_find_account_pass_through(
            client,
            wallet.to_string(),
            &public_key,
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
        create_wallet_and_regsiter_account(client, BETTOR_PUBLIC_KEY, dec!(345_678)).await
    }

    async fn create_wallet_and_regsiter_account(
        client: &reqwest::Client,
        public_key: &str,
        marker_amount: Decimal,
    ) -> Result<(cosmos::Wallet, AccountId)> {
        let new_wallet = wallet_from_seed(SeedPhrase::random())?;
        let public_key = PublicKey::from_str(public_key)?;
        let account_id = send_funds_with_key_and_find_account_pass_through(
            client,
            new_wallet.to_string(),
            &public_key,
            marker_amount,
        )
        .await?;
        Ok((new_wallet, account_id))
    }

    async fn send_funds_with_key_and_find_account_pass_through(
        client: &reqwest::Client,
        wallet: String,
        public_key: &PublicKey,
        to_send: Decimal,
    ) -> Result<AccountId> {
        let resp = client
            .post("http://localhost:12345/msg")
            .json(&kolme::pass_through::Msg {
                wallet,
                coins: vec![std_coin(to_send)],
                msg: ExecuteMsg::Regular {
                    keys: vec![*public_key],
                },
            })
            .send()
            .await?;
        resp.error_for_status()?;
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
            .ok_or(anyhow::anyhow!(
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

    async fn server_state(client: &reqwest::Client) -> Result<ServerState> {
        let resp = client.get("http://localhost:3001").send().await?;
        ensure!(resp.status().is_success());
        resp.json::<ServerState>().await.map_err(Into::into)
    }

    #[derive(serde::Deserialize, Debug)]
    struct ServerState {
        balances: BTreeMap<AccountId, BTreeMap<AssetId, Decimal>>,
    }

    fn std_coin(amount: Decimal) -> cosmwasm_std::Coin {
        cosmwasm_std::Coin {
            amount: cosmwasm_std::Uint128::new(u128::try_from(amount * dec!(1_000_000)).unwrap()),
            denom: "uosmo".to_string(),
        }
    }

    async fn broadcast_check_state<C: Config>(
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
            let state = state::<C>(db_path.to_path_buf()).await?;
            check(state)
        })
        .retry(
            ExponentialBuilder::default()
                .with_min_delay(std::time::Duration::from_millis(50))
                .with_max_times(10),
        )
        .await
    }
}
