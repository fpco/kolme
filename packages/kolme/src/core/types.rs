mod balances;

use std::{fmt::Display, sync::OnceLock};

use cosmwasm_std::Uint128;

use crate::*;

pub use balances::{Balances, BalancesError};

#[derive(
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Clone,
    Copy,
    Debug,
    Hash,
    strum::AsRefStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum ExternalChain {
    OsmosisTestnet,
    NeutronTestnet,
    OsmosisLocal,
}

impl ExternalChain {
    pub async fn make_cosmos(self) -> Result<cosmos::Cosmos> {
        let network = match self {
            ExternalChain::OsmosisTestnet => cosmos::CosmosNetwork::OsmosisTestnet,
            ExternalChain::NeutronTestnet => cosmos::CosmosNetwork::NeutronTestnet,
            ExternalChain::OsmosisLocal => cosmos::CosmosNetwork::OsmosisLocal,
        };
        Ok(network.builder_with_config().await?.build()?)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ChainConfig {
    pub assets: BTreeMap<AssetName, AssetConfig>,
    pub bridge: BridgeContract,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct AssetConfig {
    pub decimals: u8,
    pub asset_id: AssetId,
}

#[derive(snafu::Snafu, Debug)]
pub enum AssetError {
    #[snafu(display(
        "Could not convert {amount} to decimal using {decimals} decimal points: {source}"
    ))]
    CouldNotConvertToDecimal {
        source: rust_decimal::Error,
        amount: u128,
        decimals: u8,
    },
    #[snafu(display("Amount {amount} too large to convert to decimal: {source}"))]
    U128TooLarge {
        source: std::num::TryFromIntError,
        amount: u128,
    },
    #[snafu(display("Cannot convert {amount} to integer because it's negative: {source}"))]
    ToU128WithNegative {
        source: std::num::TryFromIntError,
        amount: Decimal,
    },
}

impl AssetConfig {
    pub(crate) fn to_decimal(&self, amount: u128) -> Result<Decimal, AssetError> {
        Decimal::try_from_i128_with_scale(
            amount
                .try_into()
                .map_err(|source| AssetError::U128TooLarge { source, amount })?,
            self.decimals.into(),
        )
        .map_err(|source| AssetError::CouldNotConvertToDecimal {
            source,
            amount,
            decimals: self.decimals,
        })
    }

    /// Convert a decimal amount to u128
    ///
    /// Note that this returns a potentially modified Decimal, to
    /// account for potential rounding due to on-chain representation.
    /// The idea is to allow dust to remain in the user account.
    pub(crate) fn to_u128(&self, amount: Decimal) -> Result<(Decimal, u128), AssetError> {
        let mut int = u128::try_from(amount.mantissa())
            .map_err(|source| AssetError::ToU128WithNegative { source, amount })?;
        let decimals = u32::from(self.decimals);
        let scale = amount.scale();
        match decimals.cmp(&scale) {
            std::cmp::Ordering::Less => {
                for _ in decimals..scale {
                    int /= 10;
                }
            }
            std::cmp::Ordering::Equal => (),
            std::cmp::Ordering::Greater => {
                for _ in scale..decimals {
                    int *= 10;
                }
            }
        }
        Ok((self.to_decimal(int)?, int))
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum BridgeContract {
    NeededCosmosBridge { code_id: u64 },
    Deployed(String),
}

#[derive(serde::Serialize)]
pub enum GenesisAction {
    InstantiateCosmos {
        chain: ExternalChain,
        code_id: u64,
        processor: PublicKey,
        listeners: BTreeSet<PublicKey>,
        needed_listeners: usize,
        approvers: BTreeSet<PublicKey>,
        needed_approvers: usize,
    },
}

pub struct PendingBridgeAction {
    pub chain: ExternalChain,
    pub payload: String,
    pub height: BlockHeight,
    /// Index of the message within the block
    pub message: usize,
    pub action_id: BridgeActionId,
}

#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct AssetId(pub u64);

impl Display for AssetId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug,
)]
pub struct AssetName(pub String);

#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct AccountId(pub u64);
impl AccountId {
    pub fn next(self) -> AccountId {
        AccountId(self.0 + 1)
    }
}

impl Display for AccountId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct AccountNonce(pub u64);

impl AccountNonce {
    pub fn start() -> Self {
        AccountNonce(0)
    }

    pub fn next(self) -> Self {
        AccountNonce(self.0 + 1)
    }
}

impl Display for AccountNonce {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl TryFrom<i64> for AccountNonce {
    type Error = anyhow::Error;

    fn try_from(value: i64) -> Result<Self> {
        Ok(AccountNonce(value.try_into()?))
    }
}

/// Height of a block
#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct BlockHeight(pub u64);
impl BlockHeight {
    pub fn next(self) -> BlockHeight {
        BlockHeight(self.0 + 1)
    }

    pub fn start() -> BlockHeight {
        BlockHeight(0)
    }

    pub(crate) fn is_start(&self) -> bool {
        self.0 == 0
    }

    pub(crate) fn try_into_i64(self) -> Result<i64> {
        self.0.try_into().map_err(anyhow::Error::from)
    }
}

impl Display for BlockHeight {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl TryFrom<i64> for BlockHeight {
    type Error = anyhow::Error;

    fn try_from(value: i64) -> Result<Self> {
        value.try_into().map_err(anyhow::Error::from).map(Self)
    }
}

/// Blockchain wallet address.
///
/// To allow support for arbitrary chains, we represent this as a simple [String].
///
/// TODO: Do we need to be worried about differences in case representation, e.g. for EVM hex addresses?
#[derive(
    PartialEq, PartialOrd, Ord, Eq, Clone, Debug, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct Wallet(pub String);

/// A block that is signed by the processor.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(bound(
    serialize = "",
    deserialize = "AppMessage: serde::de::DeserializeOwned"
))]
pub struct SignedBlock<AppMessage>(pub SignedTaggedJson<Block<AppMessage>>);

impl<AppMessage> SignedBlock<AppMessage> {
    pub fn validate_signature(&self) -> Result<()> {
        let pubkey = self.0.verify_signature()?;
        anyhow::ensure!(pubkey == self.0.message.as_inner().processor);
        Ok(())
    }
}

/// The hash of a [Block].
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug)]
pub struct BlockHash(pub Sha256Hash);
impl BlockHash {
    pub(crate) fn genesis_parent() -> BlockHash {
        static LOCK: OnceLock<BlockHash> = OnceLock::new();
        *LOCK.get_or_init(|| BlockHash(Sha256Hash::hash("genesis parent")))
    }
}

/// A block containing a single transaction.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(bound = "AppMessage: serde::de::DeserializeOwned")]
pub struct Block<AppMessage> {
    pub tx: SignedTransaction<AppMessage>,
    pub timestamp: Timestamp,
    pub processor: PublicKey,
    pub height: BlockHeight,
    pub parent: BlockHash,
    /// New framework state at the end of execution
    pub framework_state: Sha256Hash,
    /// New app state at the end of execution
    pub app_state: Sha256Hash,
    /// Any data loads that were used for execution.
    pub loads: Vec<BlockDataLoad>,
}

/// A proposed event from a client, not yet added to the stream
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(bound(
    serialize = "",
    deserialize = "AppMessage: serde::de::DeserializeOwned"
))]
pub struct SignedTransaction<AppMessage>(pub SignedTaggedJson<Transaction<AppMessage>>);

impl<AppMessage: serde::Serialize> SignedTransaction<AppMessage> {
    pub fn validate_signature(&self) -> Result<()> {
        let pubkey = self.0.verify_signature()?;
        anyhow::ensure!(pubkey == self.0.message.as_inner().pubkey);
        Ok(())
    }
}

impl<AppMessage: serde::Serialize> Transaction<AppMessage> {
    pub fn ensure_is_genesis(&self) -> Result<()> {
        anyhow::ensure!(self.messages.len() == 1);
        anyhow::ensure!(matches!(self.messages[0], Message::Genesis(_)));
        Ok(())
    }

    pub fn ensure_no_genesis(&self) -> Result<()> {
        for msg in &self.messages {
            anyhow::ensure!(!matches!(msg, Message::Genesis(_)));
        }
        Ok(())
    }
}

/// A transaction, proposed by clients to be included in a block.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Transaction<AppMessage> {
    pub pubkey: PublicKey,
    pub nonce: AccountNonce,
    pub created: Timestamp,
    pub messages: Vec<Message<AppMessage>>,
}

impl<AppMessage: serde::Serialize> Transaction<AppMessage> {
    pub fn sign(self, key: &SecretKey) -> Result<SignedTransaction<AppMessage>> {
        Ok(SignedTransaction(TaggedJson::new(self)?.sign(key)?))
    }
}

/// An individual message included in a transaction.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub enum Message<AppMessage> {
    Genesis(GenesisInfo),
    App(AppMessage),
    Listener {
        chain: ExternalChain,
        event_id: BridgeEventId,
        event: BridgeEvent,
    },
    /// Approval from a single approver for a bridge action
    Approve {
        chain: ExternalChain,
        action_id: BridgeActionId,
        signature: Signature,
        recovery: RecoveryId,
    },
    /// Final approval from the processor to confirm approvals from approvers.
    ProcessorApprove {
        chain: ExternalChain,
        action_id: BridgeActionId,
        processor: SignatureWithRecovery,
        approvers: Vec<SignatureWithRecovery>,
    },
    Auth(AuthMessage),
    Bank(BankMessage),
    // TODO: admin actions: update code version, change processor/listeners/approvers (need to update contracts too), modification to chain values (like asset definitions)
}

/// An event emitted by a bridge contract and reported by a listener.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub enum BridgeEvent {
    /// A bridge was instantiated
    Instantiated { contract: String },
    /// Regular action performed by the user
    Regular {
        wallet: String,
        funds: Vec<BridgedAssetAmount>,
        keys: Vec<PublicKey>,
    },
    Signed {
        wallet: String,
        action_id: BridgeActionId,
    },
}

/// An event emitted by a bridge contract and reported by a listener.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct BridgedAssetAmount {
    pub denom: String,
    pub amount: u128,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AuthMessage {
    AddPublicKey { key: PublicKey },
    RemovePublicKey { key: PublicKey },
    AddWallet { wallet: String },
    RemoveWallet { wallet: String },
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum BankMessage {
    Transfer {
        asset: AssetId,
        dest: AccountId,
        amount: Decimal,
    },
    Withdraw {
        asset: AssetId,
        chain: ExternalChain,
        dest: Wallet,
        amount: Decimal,
    },
}

/// Information defining the initial state of an app.
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug, Clone)]
pub struct GenesisInfo {
    /// Unique identifier for this application, never changes.
    pub kolme_ident: String,
    /// Public key of the processor for this app
    pub processor: PublicKey,
    /// Public keys of the listeners for this app
    pub listeners: BTreeSet<PublicKey>,
    /// How many of the listeners are needed to approve a reported bridge event?
    pub needed_listeners: usize,
    /// Public keys of the approvers for this app
    pub approvers: BTreeSet<PublicKey>,
    /// How many of the approvers are needed to approve a bridge action?
    pub needed_approvers: usize,
    /// Initial configuration of different chains
    pub chains: BTreeMap<ExternalChain, ChainConfig>,
}

impl GenesisInfo {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.listeners.len() >= self.needed_listeners);
        anyhow::ensure!(self.needed_listeners > 0);
        anyhow::ensure!(self.approvers.len() >= self.needed_approvers);
        anyhow::ensure!(self.needed_approvers > 0);
        Ok(())
    }
}

#[derive(Default, Debug)]
pub struct MessageOutput {
    pub logs: Vec<String>,
    pub loads: Vec<BlockDataLoad>,
    pub actions: Vec<ExecAction>,
}

/// Input and output for a single data load while processing a block.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct BlockDataLoad {
    /// Description of the request
    pub request: String,
    /// The resulting value
    pub response: String,
}

/// A specific action to be taken as a result of an execution.
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub enum ExecAction {
    Transfer {
        chain: ExternalChain,
        recipient: Wallet,
        funds: Vec<AssetAmount>,
    },
}
impl ExecAction {
    pub(crate) fn to_payload(
        &self,
        chain: ExternalChain,
        configs: &BTreeMap<ExternalChain, ChainConfig>,
        id: BridgeActionId,
    ) -> Result<String> {
        match chain {
            ExternalChain::OsmosisTestnet
            | ExternalChain::NeutronTestnet
            | ExternalChain::OsmosisLocal => {
                #[derive(serde::Serialize)]
                struct Payload {
                    id: BridgeActionId,
                    messages: Vec<cosmwasm_std::CosmosMsg>,
                }
                let message = match self {
                    ExecAction::Transfer {
                        chain: chain2,
                        recipient,
                        funds,
                    } => {
                        assert_eq!(&chain, chain2);
                        let mut coins = vec![];
                        for AssetAmount { id, amount } in funds {
                            let denom = configs
                                .get(&chain)
                                .context("Missing chain")?
                                .assets
                                .iter()
                                .find(|(_name, config)| config.asset_id == *id)
                                .context("Unsupported asset ID")?
                                .0;
                            let denom = denom.0.clone();
                            coins.push(cosmwasm_std::Coin {
                                denom,
                                amount: Uint128::new(*amount),
                            });
                        }
                        cosmwasm_std::CosmosMsg::Bank(cosmwasm_std::BankMsg::Send {
                            to_address: recipient.0.clone(),
                            amount: coins,
                        })
                    }
                };

                let payload = Payload {
                    id,
                    messages: vec![message],
                };
                let payload = serde_json::to_string(&payload)?;
                Ok(payload)
            }
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct AssetAmount {
    pub id: AssetId,
    pub amount: u128, // FIXME use a Decimal representation
}

/// Notifications that can come from the Kolme framework to components.
///
/// TODO this will ultimately be incorporated into a p2p network of events.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(
    serialize = "",
    deserialize = "AppMessage: serde::de::DeserializeOwned"
))]
pub enum Notification<AppMessage> {
    NewBlock(Arc<SignedBlock<AppMessage>>),
    /// A claim by a submitter that it has instantiated a bridge contract.
    GenesisInstantiation {
        chain: ExternalChain,
        contract: String,
    },
    /// Broadcast a transaction to be included in the chain.
    Broadcast {
        tx: Arc<SignedTransaction<AppMessage>>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::dec;

    #[test]
    fn to_decimal() {
        let config_two = AssetConfig {
            decimals: 2,
            asset_id: AssetId(1),
        };
        let config_six = AssetConfig {
            decimals: 6,
            asset_id: AssetId(1),
        };
        let config_eight = AssetConfig {
            decimals: 8,
            asset_id: AssetId(1),
        };

        assert_eq!(
            config_two.to_decimal(1234567890).unwrap(),
            dec! {12345678.9}
        );

        assert_eq!(
            config_six.to_decimal(1234567890).unwrap(),
            dec! {1234.56789}
        );

        assert_eq!(
            config_eight.to_decimal(1234567890).unwrap(),
            dec! {12.3456789}
        );
    }

    #[test]
    fn to_u128() {
        let config_two = AssetConfig {
            decimals: 2,
            asset_id: AssetId(1),
        };
        let config_six = AssetConfig {
            decimals: 6,
            asset_id: AssetId(1),
        };
        let config_eight = AssetConfig {
            decimals: 8,
            asset_id: AssetId(1),
        };

        assert_eq!(
            config_two.to_u128(dec!(12.34)).unwrap(),
            (dec!(12.34), 1234)
        );
        assert_eq!(
            config_two.to_u128(dec!(12.3456)).unwrap(),
            (dec!(12.34), 1234)
        );

        assert_eq!(
            config_six.to_u128(dec!(12.3456)).unwrap(),
            (dec!(12.3456), 12345600)
        );
        assert_eq!(
            config_six.to_u128(dec!(12.3456789)).unwrap(),
            (dec!(12.345678), 12345678)
        );

        assert_eq!(
            config_eight.to_u128(dec!(12.3456789)).unwrap(),
            (dec!(12.3456789), 1234567890)
        );
    }
}
