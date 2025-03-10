use k256::{
    ecdsa::{signature::SignerMut, Signature},
    PublicKey,
};

use crate::prelude::*;

/// A proposed event from a client, not yet added to the stream
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProposedEvent<AppMessage> {
    pub payload: EventPayload<AppMessage>,
    pub signature: Signature,
}

/// The content of an event, sent by a client to be included in the event series.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct EventPayload<AppMessage> {
    pub pubkey: PublicKey,
    pub nonce: AccountNonce,
    pub created: Timestamp,
    pub messages: Vec<EventMessage<AppMessage>>,
}

impl<AppMessage: serde::Serialize> EventPayload<AppMessage> {
    pub fn sign(self, key: &k256::SecretKey) -> Result<ProposedEvent<AppMessage>> {
        let mut signing = k256::ecdsa::SigningKey::from(key);
        let msg = serde_json::to_vec(&self)?;
        let signature = signing.sign(&msg);
        Ok(ProposedEvent {
            payload: self,
            signature,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum EventMessage<AppMessage> {
    Genesis(RawFrameworkState),
    App(AppMessage),
    Listener(ListenerMessage),
    Auth(AuthMessage),
    // TODO Bank, with things like
    // Transfer {
    //     asset: AssetId,
    //     amount: Decimal,
    //     dest: AccountId,
    // }

    // TODO: outgoing actions
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum ListenerMessage {
    Deposit {
        asset: String,
        wallet: String,
        amount: u128,
    },
    /// Only include the bare-bones necessary to bootstrap into the auth system
    AddPublicKey { wallet: String, key: String },
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum AuthMessage {
    AddPublicKey { key: PublicKey },
    RemovePublicKey { key: PublicKey },
    AddWallet { wallet: String },
    RemoveWallet { wallet: String },
}
