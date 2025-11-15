use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    str::FromStr,
};

use anyhow::Result;
use cosmos::{HasAddressHrp, SeedPhrase};

use kolme::*;
use tokio::task::JoinSet;

#[derive(Clone, Debug)]
pub struct CosmosBridgeApp {
    pub genesis: GenesisInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct State {
    #[serde(default)]
    hi_count: u32,
}

impl MerkleSerialize for State {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store(&self.hi_count)?;
        Ok(())
    }
}

impl MerkleDeserialize for State {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            hi_count: deserializer.load()?,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum BridgeMessage {
    SayHi {},
    /// Generate a random number, for no particular reason at all
    Random {},
    /// The address itself tells us whether to send to Osmosis or Neutron
    /// FIXME: maybe this should be part of the bank module instead. Then our bridge is just a default Kolme app!
    SendTo {
        address: cosmos::Address,
        amount: Decimal,
    },
}

// Another keypair for client testing:
// Public key: 02c2b386e42945d4c11712a5bc1d20d085a7da63e57c214e2742a684a97d436599
// Secret key: 127831b9459b538eab9a338b1e96fc34249a5154c96180106dd87d39117e8e02

const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";
const SUBMITTER_SEED_PHRASE: &str = "blind frown harbor wet inform wing note frequent illegal garden shy across burger clay asthma kitten left august pottery napkin label already purpose best";

const OSMOSIS_TESTNET_CODE_ID: u64 = 12390;
const NEUTRON_TESTNET_CODE_ID: u64 = 11650;

fn my_secret_key() -> SecretKey {
    SecretKey::from_hex(SECRET_KEY_HEX).unwrap()
}

impl Default for CosmosBridgeApp {
    fn default() -> Self {
        let my_public_key = my_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        let mut bridges = ConfiguredChains::default();
        let mut assets = BTreeMap::new();
        assets.insert(
            AssetName(
                "factory/osmo1mgcky4e24969532hee55ly4rrl30z4tkzgfvq7/kolmeoutgoing".to_owned(),
            ),
            AssetConfig {
                decimals: 6,
                asset_id: AssetId(1),
            },
        );
        bridges
            .insert_cosmos(
                CosmosChain::OsmosisTestnet,
                ChainConfig {
                    assets,
                    bridge: BridgeContract::NeededCosmosBridge {
                        code_id: OSMOSIS_TESTNET_CODE_ID,
                    },
                },
            )
            .unwrap();
        let mut assets = BTreeMap::new();
        assets.insert(
            AssetName(
                "factory/neutron1mgcky4e24969532hee55ly4rrl30z4tkwvn7vt/kolmeincoming".to_owned(),
            ),
            AssetConfig {
                decimals: 6,
                asset_id: AssetId(1),
            },
        );
        bridges
            .insert_cosmos(
                CosmosChain::NeutronTestnet,
                ChainConfig {
                    assets,
                    bridge: BridgeContract::NeededCosmosBridge {
                        code_id: NEUTRON_TESTNET_CODE_ID,
                    },
                },
            )
            .unwrap();

        let genesis = GenesisInfo {
            kolme_ident: "Cosmos bridge example".to_owned(),
            validator_set: ValidatorSet {
                processor: my_public_key,
                listeners: set.clone(),
                needed_listeners: 1,
                approvers: set,
                needed_approvers: 1,
            },
            chains: bridges,
            version: CODE_VERSION.to_owned(),
        };

        Self { genesis }
    }
}

pub const CODE_VERSION: &str = "v1";

impl KolmeApp for CosmosBridgeApp {
    type State = State;
    type Message = BridgeMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
    }

    fn new_state(&self) -> Result<Self::State> {
        Ok(State { hi_count: 0 })
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> Result<()> {
        match msg {
            BridgeMessage::SayHi {} => ctx.state_mut().hi_count += 1,
            BridgeMessage::SendTo { address, amount } => {
                let chain = match address.get_address_hrp().as_str() {
                    "osmo" => ExternalChain::OsmosisTestnet,
                    "neutron" => ExternalChain::NeutronTestnet,
                    _ => anyhow::bail!("Unsupported wallet address: {address}"),
                };
                ctx.withdraw_asset(
                    AssetId(1),
                    chain,
                    ctx.get_sender_id(),
                    &Wallet(address.to_string()),
                    *amount,
                )?;
            }
            BridgeMessage::Random {} => {
                let value = ctx.load_data(RandomU32).await?;
                ctx.log(format!("Calculated a random number: {value}"));
            }
        }
        Ok(())
    }
}

#[derive(PartialEq, serde::Serialize, serde::Deserialize)]
struct RandomU32;

impl<App> KolmeDataRequest<App> for RandomU32 {
    type Response = u32;

    async fn load(self, _: &App) -> Result<Self::Response> {
        Ok(rand::random())
    }

    async fn validate(self, _: &App, _: &Self::Response) -> Result<()> {
        // No validation possible
        Ok(())
    }
}

pub async fn serve(kolme: Kolme<CosmosBridgeApp>, bind: SocketAddr) -> Result<()> {
    let mut set = JoinSet::new();

    let processor = Processor::new(kolme.clone(), my_secret_key().clone());
    set.spawn(async {
        processor.run().await;
        #[allow(unreachable_code)]
        Err(anyhow::anyhow!("Unexpected exit from processor"))
    });
    let listener = Listener::new(kolme.clone(), my_secret_key().clone());
    set.spawn(listener.run(ChainName::Cosmos));
    let approver = Approver::new(kolme.clone(), my_secret_key().clone());
    set.spawn(approver.run());
    let submitter = Submitter::new_cosmos(
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

pub async fn broadcast(message: String, secret: String, host: String) -> Result<()> {
    let message = serde_json::from_str::<BridgeMessage>(&message)?;
    let secret = SecretKey::from_hex(&secret)?;
    let public = secret.public_key();
    let client = reqwest::Client::new();

    println!("Public sec1: {public}");
    println!("Public serialized: {}", serde_json::to_string(&public)?);

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
    Ok(())
}
