use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    str::FromStr,
};

use anyhow::Result;
use clap::Parser;
use cosmos::SeedPhrase;

use kolme::*;
use rust_decimal::{dec, Decimal};
use tokio::task::JoinSet;

mod six_sigma;

use six_sigma::state::*;

/// In the future, move to an example and convert the binary to a library.
#[derive(Clone, Debug)]
pub struct SixSigmaApp;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AppMessage {
    SetConfig {
        sr_account: AccountId,
        market_funds_account: AccountId,
    },
    AddMarket {
        id: u64,
        asset_id: AssetId,
        name: String,
    },
    PlaceBet {
        wallet: String,
        amount: Decimal,
        market_id: u64,
        outcome: u8,
    },
    SettleMarket {
        market_id: u64,
        outcome: u8,
    },
}

// Another keypair for client testing:
// Public key: 02c2b386e42945d4c11712a5bc1d20d085a7da63e57c214e2742a684a97d436599
// Secret key: 127831b9459b538eab9a338b1e96fc34249a5154c96180106dd87d39117e8e02

const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";
//const SUBMITTER_SEED_PHRASE: &str = "blind frown harbor wet inform wing note frequent illegal garden shy across burger clay asthma kitten left august pottery napkin label already purpose best";
// lo-test1
const SUBMITTER_SEED_PHRASE: &str = "notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius";

const LOCALOSMOSIS_CODE_ID: u64 = 1;

const DUMMY_CODE_VERSION: &str = "dummy code version";

fn my_secret_key() -> SecretKey {
    SecretKey::from_hex(SECRET_KEY_HEX).unwrap()
}

pub(crate) struct Transfer {
    asset_id: AssetId,
    amount: Decimal,
    from: AccountId,
    to: AccountId,
    withdraw_wallet: Option<Wallet>,
}

impl KolmeApp for SixSigmaApp {
    type State = State;
    type Message = AppMessage;

    fn genesis_info() -> GenesisInfo {
        let my_public_key = my_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        let mut bridges = BTreeMap::new();
        let mut assets = BTreeMap::new();
        assets.insert(
            AssetName("uosmo".to_owned()),
            AssetConfig {
                decimals: 6,
                asset_id: AssetId(1),
            },
        );
        bridges.insert(
            ExternalChain::OsmosisLocal,
            ChainConfig {
                assets,
                bridge: BridgeContract::NeededCosmosBridge {
                    code_id: LOCALOSMOSIS_CODE_ID,
                },
            },
        );
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
        Ok(State::default())
    }

    fn save_state(state: &Self::State) -> Result<String> {
        serde_json::to_string(state).map_err(anyhow::Error::from)
    }

    fn load_state(v: &str) -> Result<Self::State> {
        serde_json::from_str(v).map_err(anyhow::Error::from)
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> Result<()> {
        match msg {
            AppMessage::SetConfig {
                sr_account,
                market_funds_account,
            } => ctx
                .state_mut()
                .set_config(*sr_account, *market_funds_account),
            AppMessage::AddMarket { id, asset_id, name } => {
                let transfer = ctx.state_mut().add_market(*id, *asset_id, name)?;
                ctx.transfer_asset(
                    transfer.asset_id,
                    transfer.from,
                    transfer.to,
                    transfer.amount,
                )?;
                Ok(())
            }
            AppMessage::PlaceBet {
                wallet,
                amount,
                market_id,
                outcome,
            } => {
                let sender = ctx.get_sender_id();
                // TODO: check if sender balance has enough funds
                let odds = ctx.load_data(OddsSource).await?;
                let transfer = ctx.state_mut().place_bet(
                    sender,
                    *market_id,
                    Wallet(wallet.clone()),
                    *amount,
                    *outcome,
                    odds,
                )?;
                ctx.transfer_asset(
                    transfer.asset_id,
                    transfer.from,
                    transfer.to,
                    transfer.amount,
                )?;
                Ok(())
            }
            AppMessage::SettleMarket { market_id, outcome } => {
                let transfers = ctx.state_mut().settle_market(*market_id, *outcome)?;
                for transfer in transfers {
                    ctx.transfer_asset(
                        transfer.asset_id,
                        transfer.from,
                        transfer.to,
                        transfer.amount,
                    )?;
                    if let Some(wallet) = transfer.withdraw_wallet {
                        ctx.withdraw_asset(
                            transfer.asset_id,
                            ExternalChain::OsmosisLocal,
                            transfer.to,
                            &wallet,
                            transfer.amount,
                        )?;
                    }
                }
                // TODO do the transfer from above using banking subsystem
                Ok(())
            }
        }
    }
}

const OUTCOME_COUNT: u8 = 3;

type Odds = [Decimal; OUTCOME_COUNT as usize];

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

#[derive(clap::Parser)]
struct Opt {
    #[clap(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Parser)]
enum Cmd {
    Serve {
        #[clap(long, default_value = "[::]:3000")]
        bind: SocketAddr,
    },
    GenPair {},
    Broadcast {
        #[clap(long)]
        message: String,
        #[clap(long)]
        secret: String,
        #[clap(long, default_value = "http://localhost:3000")]
        host: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    main_inner().await
}

async fn main_inner() -> Result<()> {
    match Opt::parse().cmd {
        Cmd::Serve { bind } => serve(bind).await,
        Cmd::GenPair {} => {
            let mut rng = rand::thread_rng();
            let secret = SecretKey::random(&mut rng);
            let public = secret.public_key();
            println!("Public key: {public}");
            println!("Secret key: {}", secret.reveal_as_hex());
            Ok(())
        }
        Cmd::Broadcast {
            message,
            secret,
            host,
        } => broadcast(message, secret, host).await,
    }
}

async fn serve(bind: SocketAddr) -> Result<()> {
    const DB_PATH: &str = "six-sigma-app.sqlite3";
    kolme::init_logger(true, None);
    let kolme = Kolme::new(SixSigmaApp, DUMMY_CODE_VERSION, DB_PATH).await?;

    let mut set = JoinSet::new();

    let processor = Processor::new(kolme.clone(), my_secret_key().clone());
    set.spawn(processor.run());
    let listener = Listener::new(kolme.clone(), my_secret_key().clone());
    set.spawn(listener.run());
    let approver = Approver::new(kolme.clone(), my_secret_key().clone());
    set.spawn(approver.run());
    let submitter = Submitter::new(
        kolme.clone(),
        SeedPhrase::from_str(SUBMITTER_SEED_PHRASE).unwrap(),
    );
    set.spawn(submitter.run());
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

async fn broadcast(message: String, secret: String, host: String) -> Result<()> {
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
