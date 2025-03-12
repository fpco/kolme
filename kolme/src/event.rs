use k256::{
    ecdsa::{
        signature::{SignerMut, Verifier},
        Signature,
    },
    PublicKey,
};

use crate::*;

/// An event that is signed by the processor.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SignedEvent {
    /// An [ApprovedEvent], serialized to ensure the signature matches.
    ///
    /// FIXME: overall decide where to use Vec<u8> vs concrete types below, I'm being very inconsistent. Using a helper data type that wraps up the binary and parsed representations together may be advantageous.
    pub event: Vec<u8>,
    pub signature: Signature,
}

/// An event that has been accepted by the processor.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ApprovedEvent<AppMessage> {
    pub event: ProposedEvent<AppMessage>,
    pub timestamp: Timestamp,
    pub processor: PublicKey,
    // FIXME include a hash of the previous event in the stream
    // FIXME include the event height too
}

/// A proposed event from a client, not yet added to the stream
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProposedEvent<AppMessage> {
    pub payload: EventPayload<AppMessage>,
    pub signature: Signature,
}

impl<AppMessage: serde::Serialize> ProposedEvent<AppMessage> {
    pub fn validate_signature(&self) -> Result<()> {
        let key = k256::ecdsa::VerifyingKey::from(self.payload.pubkey);
        let msg = serde_json::to_vec(&self.payload)?;
        key.verify(&msg, &self.signature)?;
        Ok(())
    }

    pub fn ensure_is_genesis(&self) -> Result<()> {
        anyhow::ensure!(self.payload.messages.len() == 1);
        anyhow::ensure!(matches!(self.payload.messages[0], EventMessage::Genesis(_)));
        Ok(())
    }

    pub fn ensure_no_genesis(&self) -> Result<()> {
        for msg in &self.payload.messages {
            anyhow::ensure!(!matches!(msg, EventMessage::Genesis(_)));
        }
        Ok(())
    }
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
    Genesis(GenesisInfo),
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

    // TODO: admin actions: update code version, change processor/listeners/executors (need to update contracts too), modification to chain values (like asset definitions)
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
