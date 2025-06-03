use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
};

use anyhow::Result;
use cosmos::SeedPhrase as CosmosSeedPhrase;
use solana_keypair::Keypair as SolanaKeypair;
use solana_pubkey::Pubkey as SolanaPubkey;

use kolme::*;
use tokio::task::JoinSet;

#[derive(Clone, Debug)]
pub struct SolanaCosmosBridgeApp {
    pub genesis: GenesisInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Default, Debug)]
pub struct State;

impl MerkleSerialize for State {
    fn merkle_serialize(&self, _: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        Ok(())
    }
}

impl MerkleDeserialize for State {
    fn merkle_deserialize(_: &mut MerkleDeserializer) -> Result<Self, MerkleSerialError> {
        Ok(State)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum BridgeMessage {
    ToSolana {
        to: SolanaPubkey,
        amount: Decimal,
    },
    ToCosmos {
        to: cosmos::Address,
        amount: Decimal,
    },
}

pub const ASSET_ID: AssetId = AssetId(1);
const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";

const OSMOSIS_CODE_ID: u64 = 1;

fn my_secret_key() -> SecretKey {
    SecretKey::from_hex(SECRET_KEY_HEX).unwrap()
}

impl Default for SolanaCosmosBridgeApp {
    fn default() -> Self {
        let my_public_key = my_secret_key().public_key();

        let mut set = BTreeSet::new();
        set.insert(my_public_key);

        let mut bridges = ConfiguredChains::default();
        let mut assets = BTreeMap::new();

        assets.insert(
            AssetName("uosmo".into()),
            AssetConfig {
                decimals: 6,
                asset_id: ASSET_ID,
            },
        );

        bridges
            .insert_cosmos(
                CosmosChain::OsmosisLocal,
                ChainConfig {
                    assets,
                    bridge: BridgeContract::NeededCosmosBridge {
                        code_id: OSMOSIS_CODE_ID,
                    },
                },
            )
            .unwrap();

        let mut assets = BTreeMap::new();
        assets.insert(
            AssetName("osmof7hTFAuNjwMCcxVNThBDDftMNjiLR2cidDQzvwQ".into()),
            AssetConfig {
                decimals: 6,
                asset_id: ASSET_ID,
            },
        );

        bridges
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

        let genesis = GenesisInfo {
            kolme_ident: "Solana<->Cosmos bridge example".to_owned(),
            validator_set: ValidatorSet {
                processor: my_public_key,
                listeners: set.clone(),
                needed_listeners: 1,
                approvers: set,
                needed_approvers: 1,
            },
            chains: bridges,
        };

        Self { genesis }
    }
}

impl KolmeApp for SolanaCosmosBridgeApp {
    type State = State;
    type Message = BridgeMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
    }

    fn new_state() -> Result<Self::State> {
        Ok(State)
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> Result<()> {
        match msg {
            BridgeMessage::ToSolana { to, amount } => {
                ctx.withdraw_asset(
                    ASSET_ID,
                    ExternalChain::SolanaLocal,
                    ctx.get_sender_id(),
                    &Wallet(to.to_string()),
                    *amount,
                )?;
            }
            BridgeMessage::ToCosmos { to, amount } => {
                ctx.withdraw_asset(
                    ASSET_ID,
                    ExternalChain::OsmosisLocal,
                    ctx.get_sender_id(),
                    &Wallet(to.to_string()),
                    *amount,
                )?;
            }
        }

        Ok(())
    }
}

pub async fn serve(
    kolme: Kolme<SolanaCosmosBridgeApp>,
    solana_submitter: SolanaKeypair,
    cosmos_submitter: CosmosSeedPhrase,
    bind: SocketAddr,
) -> Result<()> {
    let mut set = JoinSet::new();

    let processor = Processor::new(kolme.clone(), my_secret_key().clone());
    set.spawn(processor.run());

    let listener = Listener::new(kolme.clone(), my_secret_key().clone());
    set.spawn(listener.run(ChainName::Cosmos));

    let listener = Listener::new(kolme.clone(), my_secret_key().clone());
    set.spawn(listener.run(ChainName::Solana));

    let approver = Approver::new(kolme.clone(), my_secret_key().clone());
    set.spawn(approver.run());

    let submitter = Submitter::new_cosmos(kolme.clone(), cosmos_submitter);
    set.spawn(submitter.run());

    let submitter = Submitter::new_solana(kolme.clone(), solana_submitter);
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

pub async fn broadcast(
    message: BridgeMessage,
    secret: SecretKey,
    host: &str,
) -> Result<Sha256Hash> {
    let public = secret.public_key();
    let client = reqwest::Client::new();

    println!("Public sec1: {public}");

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

    Ok(txhash)
}
