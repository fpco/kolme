use std::collections::HashMap;

use crate::*;

/// Account information used at the event metadata layer.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Account {
    created: EventHeight,
    pubkeys: BTreeSet<PublicKey>,
    wallets: BTreeSet<Wallet>,
    next_nonce: AccountNonce,
}

/// Raw event state that can be serialized to the database.
///
/// We build an [EventState] on top of this which has more helpful
/// bidirectional lookup capabilities.
#[derive(serde::Serialize, serde::Deserialize)]
struct RawEventState {
    /// Unique identifier for this deployment, never changes.
    ///
    /// We store this in the raw event state to ensure we don't
    /// accidentally load up the database for a different application.
    kolme_ident: String,

    /// Current state of accounts.
    ///
    /// Used only for validating events, not for actual execution.
    accounts: BTreeMap<AccountId, Account>,

    /// Listener reports pending sufficient signatures.
    pending_listeners: BTreeMap<ExternalChain, BTreeMap<BridgeEventId, PendingListenerMessage>>,

    /// Last approved bridge event ID per chain.
    last_bridge_event_id: BTreeMap<ExternalChain, BridgeEventId>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PendingListenerMessage {
    signed: BTreeSet<PublicKey>,
    message: ListenerMessage,
}

pub struct EventState {
    raw: RawEventState,
    next_height: EventHeight,

    /// Reverse lookup information for public keys
    pubkeys: BTreeMap<k256::PublicKey, AccountId>,

    /// Reverse lookup information for wallets
    wallets: HashMap<Wallet, AccountId>,
}

impl EventState {
    pub fn get_account_id(&self, key: &k256::PublicKey) -> Option<AccountId> {
        self.pubkeys.get(key).copied()
    }

    pub(crate) fn get_or_insert_account_id(
        &mut self,
        key: &k256::PublicKey,
        height: EventHeight,
    ) -> AccountId {
        match self.get_account_id(key) {
            Some(id) => id,
            None => {
                let id = AccountId(self.raw.accounts.len().try_into().unwrap());
                let mut pubkeys = BTreeSet::new();
                pubkeys.insert(*key);
                self.raw.accounts.insert(
                    id,
                    Account {
                        created: height,
                        pubkeys,
                        wallets: BTreeSet::new(),
                        next_nonce: AccountNonce::start(),
                    },
                );
                id
            }
        }
    }

    pub fn get_next_nonce(&self, account_id: AccountId) -> Result<AccountNonce> {
        self.raw
            .accounts
            .get(&account_id)
            .context("get_next_nonce: account not found")
            .map(|x| x.next_nonce)
    }

    /// Errors if the account doesn't exist.
    pub(crate) fn bump_nonce_for(&mut self, account_id: AccountId) -> Result<()> {
        self.raw
            .accounts
            .get_mut(&account_id)
            .context("bump_nonce_for found a missing account")?
            .next_nonce
            .increment();
        Ok(())
    }

    pub fn get_next_height(&self) -> EventHeight {
        self.next_height
    }

    pub fn increment_height(&mut self) {
        self.next_height = self.next_height.next();
    }

    pub(super) fn new(kolme_ident: impl Into<String>) -> Result<Self> {
        Ok(EventState {
            raw: RawEventState {
                kolme_ident: kolme_ident.into(),
                accounts: BTreeMap::new(),
                pending_listeners: BTreeMap::new(),
                last_bridge_event_id: BTreeMap::new(),
            },
            next_height: EventHeight::start(),
            pubkeys: BTreeMap::new(),
            wallets: HashMap::new(),
        })
    }

    pub(super) fn load(
        expected_kolme_ident: &str,
        EventStreamState { height, state }: EventStreamState,
    ) -> Result<Self> {
        let raw = serde_json::from_slice::<RawEventState>(&state)?;
        anyhow::ensure!(
            raw.kolme_ident == expected_kolme_ident,
            "Loaded an event state with ident {:?}, but expected ident {:?}",
            raw.kolme_ident,
            expected_kolme_ident
        );

        let mut pubkeys = BTreeMap::new();
        let mut wallets = HashMap::new();

        for (account_id, account) in &raw.accounts {
            for pubkey in &account.pubkeys {
                pubkeys.insert(*pubkey, *account_id);
            }
            for wallet in &account.wallets {
                wallets.insert(wallet.clone(), *account_id);
            }
        }

        Ok(Self {
            raw,
            next_height: height.next(),
            pubkeys,
            wallets,
        })
    }

    pub(crate) fn serialize_raw_state(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&self.raw).map_err(anyhow::Error::from)
    }
}
