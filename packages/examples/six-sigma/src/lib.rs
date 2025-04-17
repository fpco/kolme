mod state;
mod types;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Write,
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
pub struct SixSigmaApp;

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

impl KolmeApp for SixSigmaApp {
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
        bridges
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
            ctx.mint_asset(*asset_id, *account, *amount)?;
            ctx.withdraw_asset(
                *asset_id,
                ExternalChain::OsmosisLocal,
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

pub async fn serve(bind: SocketAddr, tx_log_path: Option<PathBuf>) -> Result<()> {
    const DB_PATH: &str = "six-sigma-app.sqlite3";
    kolme::init_logger(true, None);
    let kolme = Kolme::new(SixSigmaApp, DUMMY_CODE_VERSION, DB_PATH).await?;

    let mut set = JoinSet::new();

    let processor = Processor::new(kolme.clone(), my_secret_key().clone(), None);
    set.spawn(processor.run());
    let listener = Listener::new(kolme.clone(), my_secret_key().clone());
    set.spawn(listener.run(ChainName::Cosmos));
    let approver = Approver::new(kolme.clone(), my_secret_key().clone());
    set.spawn(approver.run());
    let submitter = Submitter::new_cosmos(
        kolme.clone(),
        SeedPhrase::from_str(SUBMITTER_SEED_PHRASE).unwrap(),
    );
    set.spawn(submitter.run());
    if let Some(tx_log_path) = tx_log_path {
        let logger = TxLogger::new(kolme.clone(), tx_log_path);
        set.spawn(logger.run());
    }
    let api_server = ApiServer::new(kolme);
    set.spawn(api_server.run(bind));

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
