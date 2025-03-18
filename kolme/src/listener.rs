use base64::{prelude::BASE64_STANDARD, Engine};
use cosmos::Contract;
use tokio::task::JoinSet;

use crate::*;

pub struct Listener<App: KolmeApp> {
    kolme: Kolme<App>,
    secret: k256::SecretKey,
}

impl<App: KolmeApp> Listener<App> {
    pub fn new(kolme: Kolme<App>, secret: k256::SecretKey) -> Self {
        Listener { kolme, secret }
    }

    pub async fn run(self) -> Result<()> {
        let contracts = self.wait_for_contracts().await?;
        let mut set = JoinSet::new();
        set.spawn(subscribe(self.kolme.clone(), self.secret.clone()));
        for (chain, contract) in contracts {
            set.spawn(listen(
                self.kolme.clone(),
                self.secret.clone(),
                chain,
                contract,
            ));
        }

        while let Some(res) = set.join_next().await {
            match res {
                Err(e) => {
                    set.abort_all();
                    return Err(anyhow::anyhow!("Listener panicked: {e}"));
                }
                Ok(Err(e)) => return Err(e),
                Ok(Ok(())) => (),
            }
        }

        Ok(())
    }

    async fn wait_for_contracts(&self) -> Result<BTreeMap<ExternalChain, String>> {
        let mut receiver = self.kolme.subscribe();
        loop {
            if let Some(contracts) = self.get_contracts().await {
                return Ok(contracts);
            }
            receiver.recv().await?;
        }
    }

    async fn get_contracts(&self) -> Option<BTreeMap<ExternalChain, String>> {
        let mut res = BTreeMap::new();
        for (chain, config) in self.kolme.read().await.get_bridge_contracts() {
            match &config.bridge {
                BridgeContract::NeededCosmosBridge { code_id: _ } => return None,
                BridgeContract::Deployed(contract) => {
                    res.insert(*chain, contract.clone());
                }
            }
        }
        Some(res)
    }
}

async fn subscribe<App: KolmeApp>(kolme: Kolme<App>, secret: k256::SecretKey) -> Result<()> {
    let mut receiver = kolme.subscribe();
    loop {
        let notification = receiver.recv().await?;
        match notification {
            Notification::NewBlock(_) => (),
            Notification::GenesisInstantiation { chain, contract } => {
                // FIXME sanity check the supplied contract and confirm it meets the genesis requirements
                let signed = kolme
                    .read()
                    .await
                    .create_signed_transaction(
                        &secret,
                        vec![Message::Listener {
                            chain,
                            event: BridgeEvent::Instantiated { contract },
                        }],
                    )
                    .await?;
                kolme.propose_transaction(signed)?;
            }
            Notification::Broadcast { tx: _ } => (),
        }
    }
}

async fn listen<App: KolmeApp>(
    kolme: Kolme<App>,
    secret: k256::SecretKey,
    chain: ExternalChain,
    contract: String,
) -> Result<()> {
    let cosmos = kolme.read().await.get_cosmos(chain).await?;
    let contract = cosmos.make_contract(contract.parse()?);
    tracing::info!("Beginning listener loop on contract {contract}");
    let mut next_bridge_event_id = kolme
        .read()
        .await
        .get_next_bridge_event_id(chain, secret.public_key())
        .await?;
    // We _should_ be subscribing to events. I tried doing that and failed miserably.
    // So we're trying this polling approach instead.
    loop {
        listen_once(&kolme, &secret, chain, &contract, &mut next_bridge_event_id).await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
    }
}

async fn listen_once<App: KolmeApp>(
    kolme: &Kolme<App>,
    secret: &k256::SecretKey,
    chain: ExternalChain,
    contract: &Contract,
    next_bridge_event_id: &mut BridgeEventId,
) -> Result<()> {
    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
    #[serde(rename_all = "snake_case")]
    enum QueryMsg {
        GetToKolmeMessage { id: BridgeEventId },
    }
    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum GetToKolmeMessageResp {
        Found { message: String },
        NotFound {},
    }
    match contract
        .query(&QueryMsg::GetToKolmeMessage {
            id: *next_bridge_event_id,
        })
        .await?
    {
        GetToKolmeMessageResp::Found { message } => {
            let message = BASE64_STANDARD.decode(&message)?;
            let message = serde_json::from_slice::<BridgeEventContents>(&message)?;
            broadcast_listener_event(kolme, secret, chain, *next_bridge_event_id, &message).await?;
            *next_bridge_event_id = next_bridge_event_id.next();
            Ok(())
        }
        GetToKolmeMessageResp::NotFound {} => Ok(()),
    }
}

async fn broadcast_listener_event<App: KolmeApp>(
    kolme: &Kolme<App>,
    secret: &k256::SecretKey,
    chain: ExternalChain,
    bridge_event_id: BridgeEventId,
    message: &BridgeEventContents,
) -> Result<()> {
    let mut messages = vec![];
    match message {
        BridgeEventContents::Regular {
            wallet,
            funds,
            keys,
        } => {
            for Coin { denom, amount } in funds {
                messages.push(Message::Listener {
                    chain,
                    event: BridgeEvent::Deposit {
                        asset: denom.clone(),
                        wallet: wallet.clone(),
                        amount: amount.parse()?,
                    },
                });
            }
            for key in keys {
                let key = hex::decode(key)?;
                let key = PublicKey::from_sec1_bytes(&key);
                panic!("{key:#?}");
                // messages.push(EventMessage::Listener(ListenerMessage::AddPublicKey { wallet: wallet.clone(), key: () }))
                todo!()
            }
        }
        BridgeEventContents::Signed {
            wallet,
            outgoing_id,
        } => todo!(),
    }
    let pubkey = secret.public_key();
    let nonce = kolme.read().await.get_next_account_nonce(pubkey).await?;
    let payload = Transaction {
        pubkey,
        nonce,
        created: Timestamp::now(),
        messages,
    };
    let proposed = SignedTransaction(TaggedJson::new(payload)?.sign(secret)?);
    kolme.propose_transaction(proposed)?;
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
enum BridgeEventContents {
    Regular {
        wallet: String,
        funds: Vec<Coin>,
        keys: Vec<String>,
    },
    Signed {
        wallet: String,
        outgoing_id: u32,
    },
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
struct Coin {
    denom: String,
    amount: String,
}
