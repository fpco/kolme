mod accounts;
mod error;

use crate::core::CoreStateError;
use std::{collections::HashMap, fmt::Display, str::FromStr, sync::OnceLock};

use base64::Engine;
use cosmwasm_std::{Binary, CosmosMsg, Uint128};
use kolme_store::{BlockHashes, HasBlockHashes};

use crate::*;

pub use accounts::{Account, Accounts, AccountsError};
pub use error::KolmeError;

pub type SolanaClient = solana_client::nonblocking::rpc_client::RpcClient;
pub type SolanaPubsubClient = solana_client::nonblocking::pubsub_client::PubsubClient;

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
    strum::Display,
    strum::AsRefStr,
    strum::EnumString,
)]
#[strum(serialize_all = "kebab-case")]
pub enum ExternalChain {
    OsmosisTestnet,
    NeutronTestnet,
    OsmosisLocal,
    SolanaMainnet,
    SolanaTestnet,
    SolanaDevnet,
    SolanaLocal,
    #[cfg(feature = "pass_through")]
    PassThrough,
}

impl ToMerkleKey for ExternalChain {
    fn to_merkle_key(&self) -> MerkleKey {
        self.as_ref().to_merkle_key()
    }
}

impl FromMerkleKey for ExternalChain {
    fn from_merkle_key(bytes: &[u8]) -> Result<Self, MerkleSerialError> {
        std::str::from_utf8(bytes)
            .map_err(MerkleSerialError::custom)?
            .parse()
            .map_err(MerkleSerialError::custom)
    }
}

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
pub enum SolanaChain {
    Mainnet,
    Testnet,
    Devnet,
    Local,
}

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
pub enum CosmosChain {
    OsmosisTestnet,
    NeutronTestnet,
    OsmosisLocal,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum ChainName {
    Cosmos,
    Solana,
    #[cfg(feature = "pass_through")]
    PassThrough,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum ChainKind {
    Cosmos(CosmosChain),
    Solana(SolanaChain),
    #[cfg(feature = "pass_through")]
    PassThrough,
}

impl CosmosChain {
    pub const fn name() -> ChainName {
        ChainName::Cosmos
    }

    pub async fn make_client(self) -> Result<cosmos::Cosmos> {
        let network = match self {
            Self::OsmosisTestnet => cosmos::CosmosNetwork::OsmosisTestnet,
            Self::NeutronTestnet => cosmos::CosmosNetwork::NeutronTestnet,
            Self::OsmosisLocal => cosmos::CosmosNetwork::OsmosisLocal,
        };

        Ok(network.builder_with_config().await?.build()?)
    }
}

#[derive(Default)]
pub struct SolanaEndpoints {
    pub regular: HashMap<SolanaChain, Arc<str>>,
    pub pubsub: HashMap<SolanaChain, Arc<str>>,
}

pub enum SolanaClientEndpoint {
    Static(&'static str),
    Arc(Arc<str>),
}

impl SolanaEndpoints {
    pub fn get_regular_endpoint(&self, chain: SolanaChain) -> SolanaClientEndpoint {
        self.regular.get(&chain).map_or(
            SolanaClientEndpoint::Static(match chain {
                SolanaChain::Mainnet => "https://api.mainnet-beta.solana.com",
                SolanaChain::Testnet => "https://api.testnet.solana.com",
                SolanaChain::Devnet => "https://api.devnet.solana.com",
                SolanaChain::Local => "http://localhost:8899",
            }),
            |s| SolanaClientEndpoint::Arc(s.clone()),
        )
    }

    pub fn get_pubsub_endpoint(&self, chain: SolanaChain) -> SolanaClientEndpoint {
        self.pubsub.get(&chain).map_or(
            SolanaClientEndpoint::Static(match chain {
                SolanaChain::Mainnet => "wss://api.mainnet-beta.solana.com",
                SolanaChain::Testnet => "wss://api.testnet.solana.com",
                SolanaChain::Devnet => "wss://api.devnet.solana.com/",
                SolanaChain::Local => "ws://localhost:8900",
            }),
            |s| SolanaClientEndpoint::Arc(s.clone()),
        )
    }
}

impl SolanaClientEndpoint {
    pub fn make_client(self) -> SolanaClient {
        SolanaClient::new(match self {
            SolanaClientEndpoint::Static(url) => url.to_owned(),
            SolanaClientEndpoint::Arc(url) => (*url).to_owned(),
        })
    }

    pub async fn make_pubsub_client(self) -> Result<SolanaPubsubClient> {
        match self {
            SolanaClientEndpoint::Static(url) => SolanaPubsubClient::new(url).await,
            SolanaClientEndpoint::Arc(url) => SolanaPubsubClient::new(&url).await,
        }
        .map_err(anyhow::Error::from)
    }
}

impl SolanaChain {
    pub const fn name() -> ChainName {
        ChainName::Solana
    }
}

impl ExternalChain {
    pub fn name(self) -> ChainName {
        match ChainKind::from(self) {
            ChainKind::Cosmos(_) => CosmosChain::name(),
            ChainKind::Solana(_) => SolanaChain::name(),
            #[cfg(feature = "pass_through")]
            ChainKind::PassThrough => ChainName::PassThrough,
        }
    }

    pub fn to_cosmos_chain(self) -> Option<CosmosChain> {
        match ChainKind::from(self) {
            ChainKind::Cosmos(chain) => Some(chain),
            ChainKind::Solana(_) => None,
            #[cfg(feature = "pass_through")]
            ChainKind::PassThrough => None,
        }
    }

    pub fn to_solana_chain(self) -> Option<SolanaChain> {
        match ChainKind::from(self) {
            ChainKind::Cosmos(_) => None,
            ChainKind::Solana(chain) => Some(chain),
            #[cfg(feature = "pass_through")]
            ChainKind::PassThrough => None,
        }
    }
}

impl From<CosmosChain> for ExternalChain {
    fn from(c: CosmosChain) -> ExternalChain {
        match c {
            CosmosChain::OsmosisTestnet => ExternalChain::OsmosisTestnet,
            CosmosChain::NeutronTestnet => ExternalChain::NeutronTestnet,
            CosmosChain::OsmosisLocal => ExternalChain::OsmosisLocal,
        }
    }
}

impl From<SolanaChain> for ExternalChain {
    fn from(c: SolanaChain) -> ExternalChain {
        match c {
            SolanaChain::Mainnet => ExternalChain::SolanaMainnet,
            SolanaChain::Testnet => ExternalChain::SolanaTestnet,
            SolanaChain::Devnet => ExternalChain::SolanaDevnet,
            SolanaChain::Local => ExternalChain::SolanaLocal,
        }
    }
}

impl From<ExternalChain> for ChainKind {
    fn from(c: ExternalChain) -> ChainKind {
        match c {
            ExternalChain::OsmosisTestnet => ChainKind::Cosmos(CosmosChain::OsmosisTestnet),
            ExternalChain::NeutronTestnet => ChainKind::Cosmos(CosmosChain::NeutronTestnet),
            ExternalChain::OsmosisLocal => ChainKind::Cosmos(CosmosChain::OsmosisLocal),
            ExternalChain::SolanaMainnet => ChainKind::Solana(SolanaChain::Mainnet),
            ExternalChain::SolanaTestnet => ChainKind::Solana(SolanaChain::Testnet),
            ExternalChain::SolanaDevnet => ChainKind::Solana(SolanaChain::Devnet),
            ExternalChain::SolanaLocal => ChainKind::Solana(SolanaChain::Local),
            #[cfg(feature = "pass_through")]
            ExternalChain::PassThrough => ChainKind::PassThrough,
        }
    }
}

impl MerkleSerialize for ExternalChain {
    fn merkle_serialize(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> std::result::Result<(), MerkleSerialError> {
        serializer.store(self.as_ref())
    }
}

impl MerkleDeserialize for ExternalChain {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        deserializer
            .load_str()?
            .parse()
            .map_err(MerkleSerialError::custom)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ChainConfig {
    pub assets: BTreeMap<AssetName, AssetConfig>,
    pub bridge: BridgeContract,
}
impl ChainConfig {
    fn into_state(self) -> ChainState {
        ChainState {
            config: self,
            next_action_id: BridgeActionId::start(),
            pending_actions: MerkleMap::new(),
            next_event_id: BridgeEventId::start(),
            pending_events: MerkleMap::new(),
            assets: BTreeMap::new(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChainState {
    pub config: ChainConfig,
    /// The next action ID Kolme will emit.
    pub next_action_id: BridgeActionId,
    /// Actions which have not yet been confirmed as landing on-chain.
    pub pending_actions: MerkleMap<BridgeActionId, PendingBridgeAction>,
    /// The next expected event ID from the bridge contract.
    ///
    /// This represents the next ID that will be added to `pending_events`.
    /// It does not mean that all events before this have been processed
    /// already.
    pub next_event_id: BridgeEventId,
    /// Events which have not been fully processed.
    ///
    /// We always process events in order. If a later event has
    /// sufficient signatures, but an earlier event hasn't, they
    /// will both remain pending.
    pub pending_events: MerkleMap<BridgeEventId, PendingBridgeEvent>,
    /// Balance of different assets in the bridge contract.
    ///
    /// This field is used to ensure that sufficient balances are available
    /// for fund transfers. Listeners can update these values by submitting
    /// known token balances. This can account for tokens that were added
    /// by direct transfer, without trigger a smart contract execution.
    pub assets: BTreeMap<AssetId, Decimal>,
}

impl ChainState {
    pub(crate) fn deposit(&mut self, asset_id: AssetId, amount: Decimal) -> Result<()> {
        let old = self.assets.entry(asset_id).or_default();
        *old = old.checked_add(amount).with_context(|| {
            format!("Overflow while depositing asset {asset_id}, amount == {amount}")
        })?;
        Ok(())
    }

    pub(crate) fn withdraw(&mut self, asset_id: AssetId, amount: Decimal) -> Result<()> {
        let old = self.assets.entry(asset_id).or_default();
        *old = old.checked_sub(amount).with_context(|| {
            format!("Insufficient funds while withdrawing asset {asset_id}, amount == {amount}")
        })?;
        Ok(())
    }
}

impl MerkleSerialize for ChainState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let ChainState {
            config,
            next_action_id,
            pending_actions,
            next_event_id,
            pending_events,
            assets,
        } = self;
        serializer.store(config)?;
        serializer.store(next_action_id)?;
        serializer.store(pending_actions)?;
        serializer.store(next_event_id)?;
        serializer.store(pending_events)?;
        serializer.store(assets)?;
        Ok(())
    }
}

impl MerkleDeserialize for ChainState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(ChainState {
            config: deserializer.load()?,
            next_action_id: deserializer.load()?,
            pending_actions: deserializer.load()?,
            next_event_id: deserializer.load()?,
            pending_events: deserializer.load()?,
            assets: deserializer.load()?,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct PendingBridgeAction {
    pub payload: String,
    /// Approvals from the approvers
    pub approvals: BTreeMap<PublicKey, SignatureWithRecovery>,
    /// Processor signature finalizing this operation
    pub processor: Option<SignatureWithRecovery>,
}

impl MerkleSerialize for PendingBridgeAction {
    fn merkle_serialize(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> std::result::Result<(), MerkleSerialError> {
        let PendingBridgeAction {
            payload,
            approvals,
            processor,
        } = self;
        serializer.store(payload)?;
        serializer.store(approvals)?;
        serializer.store(processor)?;
        Ok(())
    }
}

impl MerkleDeserialize for PendingBridgeAction {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(PendingBridgeAction {
            payload: deserializer.load()?,
            approvals: deserializer.load()?,
            processor: deserializer.load()?,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct PendingBridgeEvent {
    pub event: BridgeEvent,
    /// Attestations from the listeners
    pub attestations: BTreeSet<PublicKey>,
}

impl MerkleSerialize for PendingBridgeEvent {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self {
            event,
            attestations,
        } = self;
        serializer.store_json(event)?;
        serializer.store(attestations)?;
        Ok(())
    }
}

impl MerkleDeserialize for PendingBridgeEvent {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            event: deserializer.load_json()?,
            attestations: deserializer.load()?,
        })
    }
}

impl MerkleSerialize for ChainConfig {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let ChainConfig { assets, bridge } = self;
        serializer.store(assets)?;
        serializer.store(bridge)?;
        Ok(())
    }
}

impl MerkleDeserialize for ChainConfig {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            assets: deserializer.load()?,
            bridge: deserializer.load()?,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct AssetConfig {
    pub decimals: u8,
    pub asset_id: AssetId,
}

impl MerkleSerialize for AssetConfig {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let AssetConfig { decimals, asset_id } = self;
        serializer.store(decimals)?;
        serializer.store(asset_id)?;
        Ok(())
    }
}

impl MerkleDeserialize for AssetConfig {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            decimals: deserializer.load()?,
            asset_id: deserializer.load()?,
        })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum AssetError {
    #[error("Could not convert {amount} to decimal using {decimals} decimal points: {source}")]
    CouldNotConvertToDecimal {
        source: rust_decimal::Error,
        amount: u128,
        decimals: u8,
    },
    #[error("Amount {amount} too large to convert to decimal: {source}")]
    U128TooLarge {
        source: std::num::TryFromIntError,
        amount: u128,
    },
    #[error("Cannot convert {amount} to integer because it's negative: {source}")]
    ToU128WithNegative {
        source: std::num::TryFromIntError,
        amount: Decimal,
    },
}

impl AssetConfig {
    pub(crate) fn to_decimal(self, amount: u128) -> Result<Decimal, AssetError> {
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
    pub(crate) fn to_u128(self, amount: Decimal) -> Result<(Decimal, u128), AssetError> {
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
    NeededSolanaBridge { program_id: String },
    Deployed(String),
}

impl MerkleSerialize for BridgeContract {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        match self {
            BridgeContract::NeededCosmosBridge { code_id } => {
                serializer.store_byte(0);
                serializer.store(code_id)?;
            }
            BridgeContract::NeededSolanaBridge { program_id } => {
                serializer.store_byte(1);
                serializer.store(program_id)?;
            }
            BridgeContract::Deployed(addr) => {
                serializer.store_byte(2);
                serializer.store(addr)?;
            }
        }
        Ok(())
    }
}

impl MerkleDeserialize for BridgeContract {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        match deserializer.pop_byte()? {
            0 => Ok(Self::NeededCosmosBridge {
                code_id: deserializer.load()?,
            }),
            1 => Ok(Self::NeededSolanaBridge {
                program_id: deserializer.load()?,
            }),
            2 => Ok(Self::Deployed(deserializer.load()?)),
            byte => Err(MerkleSerialError::UnexpectedMagicByte { byte }),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum GenesisAction {
    InstantiateCosmos {
        chain: CosmosChain,
        code_id: u64,
        validator_set: ValidatorSet,
    },
    InstantiateSolana {
        chain: SolanaChain,
        program_id: String,
        validator_set: ValidatorSet,
    },
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

impl MerkleSerializeRaw for AssetId {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store(&self.0)
    }
}

impl MerkleDeserializeRaw for AssetId {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load().map(Self)
    }
}

#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug,
)]
pub struct AssetName(pub String);

impl MerkleSerializeRaw for AssetName {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store(&self.0)
    }
}
impl MerkleDeserializeRaw for AssetName {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load().map(Self)
    }
}

#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct AccountId(pub u64);
impl AccountId {
    pub fn next(self) -> AccountId {
        AccountId(self.0 + 1)
    }
}

impl MerkleSerializeRaw for AccountId {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store(&self.0)
    }
}

impl MerkleDeserializeRaw for AccountId {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load().map(Self)
    }
}

impl Display for AccountId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl ToMerkleKey for AccountId {
    fn to_merkle_key(&self) -> MerkleKey {
        self.0.to_merkle_key()
    }
}

impl FromMerkleKey for AccountId {
    fn from_merkle_key(bytes: &[u8]) -> std::result::Result<Self, MerkleSerialError> {
        u64::from_merkle_key(bytes).map(AccountId)
    }
}

#[derive(
    serde::Serialize,
    serde::Deserialize,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Copy,
    Hash,
    Debug,
    Default,
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

impl MerkleSerializeRaw for AccountNonce {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store(&self.0)
    }
}

impl MerkleDeserializeRaw for AccountNonce {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load().map(Self)
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

    pub fn prev(self) -> Option<Self> {
        self.0.checked_sub(1).map(BlockHeight)
    }

    pub fn start() -> BlockHeight {
        BlockHeight(0)
    }

    pub(crate) fn is_start(&self) -> bool {
        self.0 == 0
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

impl MerkleSerializeRaw for BlockHeight {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> std::result::Result<(), MerkleSerialError> {
        serializer.store(&self.0)
    }
}

impl MerkleDeserializeRaw for BlockHeight {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> std::result::Result<Self, MerkleSerialError> {
        deserializer.load().map(Self)
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

impl ToMerkleKey for Wallet {
    fn to_merkle_key(&self) -> MerkleKey {
        self.0.to_merkle_key()
    }
}

impl FromMerkleKey for Wallet {
    fn from_merkle_key(bytes: &[u8]) -> Result<Self, MerkleSerialError> {
        String::from_merkle_key(bytes).map(Self)
    }
}

impl Display for Wallet {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl MerkleSerializeRaw for Wallet {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store(&self.0)
    }
}

impl MerkleDeserializeRaw for Wallet {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load().map(Wallet)
    }
}

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

    pub fn hash(&self) -> BlockHash {
        BlockHash(self.0.message_hash())
    }

    pub fn height(&self) -> BlockHeight {
        self.0.message.as_inner().height
    }

    pub fn tx(&self) -> &SignedTransaction<AppMessage> {
        &self.0.message.as_inner().tx
    }

    pub fn as_inner(&self) -> &Block<AppMessage> {
        self.0.message.as_inner()
    }
}

impl<AppMessage> MerkleSerialize for SignedBlock<AppMessage> {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store(&self.0)
    }
}

impl<AppMessage: serde::de::DeserializeOwned> MerkleDeserialize for SignedBlock<AppMessage> {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load().map(Self)
    }
}

impl<T> PartialEq for SignedBlock<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> Eq for SignedBlock<T> {}

impl<T> HasBlockHashes for SignedBlock<T> {
    fn get_block_hashes(&self) -> BlockHashes {
        let inner = self.0.message.as_inner();
        BlockHashes {
            framework_state: inner.framework_state,
            app_state: inner.app_state,
            logs: inner.logs,
        }
    }
}

/// The hash of a [Block].
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockHash(pub Sha256Hash);
impl BlockHash {
    pub(crate) fn genesis_parent() -> BlockHash {
        static LOCK: OnceLock<BlockHash> = OnceLock::new();
        *LOCK.get_or_init(|| BlockHash(Sha256Hash::hash("genesis parent")))
    }
}

impl Display for BlockHash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// The hash of a [Transaction].
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TxHash(pub Sha256Hash);

impl Display for TxHash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
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
    /// The log messages generated when executing this block.
    ///
    /// This will be a `Vec<Vec<String>>`, where the outer Vec
    /// is per-message and the inner contains each log.
    pub logs: Sha256Hash,
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

impl<AppMessage> SignedTransaction<AppMessage> {
    /// Get the hash of the transaction
    pub fn hash(&self) -> TxHash {
        TxHash(self.0.message_hash())
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
    pub max_height: Option<BlockHeight>,
}

impl<AppMessage: serde::Serialize> Transaction<AppMessage> {
    pub fn sign(self, key: &SecretKey) -> Result<SignedTransaction<AppMessage>> {
        Ok(SignedTransaction(TaggedJson::new(self)?.sign(key)?))
    }
}

/// A helper struct for building transactions.
#[derive(Debug, Clone)]
pub struct TxBuilder<AppMessage> {
    pub messages: Vec<Message<AppMessage>>,
    pub max_height: Option<BlockHeight>,
}

impl<AppMessage> Default for TxBuilder<AppMessage> {
    fn default() -> Self {
        TxBuilder {
            messages: vec![],
            max_height: None,
        }
    }
}

impl<AppMessage> TxBuilder<AppMessage> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_message(mut self, message: Message<AppMessage>) -> Self {
        self.messages.push(message);
        self
    }

    pub fn with_max_height(mut self, max_height: BlockHeight) -> Self {
        self.max_height = Some(max_height);
        self
    }
}

impl<AppMessage> From<Vec<Message<AppMessage>>> for TxBuilder<AppMessage> {
    fn from(messages: Vec<Message<AppMessage>>) -> Self {
        TxBuilder {
            messages,
            max_height: None,
        }
    }
}

/// An individual message included in a transaction.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum Message<AppMessage> {
    #[serde(alias = "Genesis")]
    Genesis(GenesisInfo),
    #[serde(alias = "App")]
    App(AppMessage),
    #[serde(alias = "Listener")]
    Listener {
        chain: ExternalChain,
        event_id: BridgeEventId,
        event: BridgeEvent,
    },
    /// Approval from a single approver for a bridge action
    #[serde(alias = "Approve")]
    Approve {
        chain: ExternalChain,
        action_id: BridgeActionId,
        signature: SignatureWithRecovery,
    },
    /// Final approval from the processor to confirm approvals from approvers.
    #[serde(alias = "ProcessorApprove")]
    ProcessorApprove {
        chain: ExternalChain,
        action_id: BridgeActionId,
        processor: SignatureWithRecovery,
        approvers: Vec<SignatureWithRecovery>,
    },
    #[serde(alias = "Auth")]
    Auth(AuthMessage),
    #[serde(alias = "Bank")]
    Bank(BankMessage),
    #[serde(alias = "Admin")]
    Admin(AdminMessage),
}

impl<AppMessage: serde::de::DeserializeOwned> FromStr for Message<AppMessage> {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

/// An event emitted by a bridge contract and reported by a listener.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub enum BridgeEvent {
    /// A bridge was instantiated
    Instantiated { contract: String },
    /// Regular action performed by the user
    Regular {
        wallet: Wallet,
        funds: Vec<BridgedAssetAmount>,
        keys: Vec<PublicKey>,
    },
    Signed {
        wallet: Wallet,
        action_id: BridgeActionId,
    },
}

/// An event emitted by a bridge contract and reported by a listener.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BridgedAssetAmount {
    pub denom: String,
    pub amount: u128,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AuthMessage {
    AddPublicKey { key: PublicKey },
    RemovePublicKey { key: PublicKey },
    AddWallet { wallet: Wallet },
    RemoveWallet { wallet: Wallet },
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

/// Admin messages sent by validators.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AdminMessage {
    /// The sending key is replacing itself with a new key.
    SelfReplace(Box<SignedTaggedJson<SelfReplace>>),
    /// Replace the complete validator set.
    ///
    /// Must be proposed by one of the members of an existing set.
    NewSet {
        validator_set: Box<SignedTaggedJson<ValidatorSet>>,
    },
    /// Initiate a contract migration.
    MigrateContract(Box<SignedTaggedJson<MigrateContract>>),
    /// Upgrade to a new code version
    ///
    /// Note that, technically speaking, we don't need the internal
    /// signature for this upgrade, since unlike the other admin messages
    /// above, we don't need to pass along signatures to external chains.
    /// However, for consistency and simplicity of the code, we follow the
    /// same format for this message too.
    Upgrade(Box<SignedTaggedJson<Upgrade>>),
    /// Vote to approve an admin proposal.
    Approve {
        admin_proposal_id: AdminProposalId,
        signature: SignatureWithRecovery,
    },
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct MigrateContract {
    pub chain: ExternalChain,
    pub new_code_id: u64,
    pub message: serde_json::Value,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Upgrade {
    pub desired_version: String,
}

impl AdminMessage {
    pub fn self_replace(
        validator_type: ValidatorType,
        replacement: PublicKey,
        current: &SecretKey,
    ) -> Result<Self> {
        let self_replace = SelfReplace {
            validator_type,
            replacement,
        };
        let json = TaggedJson::new(self_replace)?;
        let signed = json.sign(current)?;
        Ok(AdminMessage::SelfReplace(Box::new(signed)))
    }

    pub fn new_set(set: ValidatorSet, proposer: &SecretKey) -> Result<Self> {
        let json = TaggedJson::new(set)?;
        let signed = json.sign(proposer)?;
        Ok(AdminMessage::NewSet {
            validator_set: Box::new(signed),
        })
    }

    pub fn upgrade(desired_version: impl Into<String>, proposer: &SecretKey) -> Result<Self> {
        let json = TaggedJson::new(Upgrade {
            desired_version: desired_version.into(),
        })?;
        let signed = json.sign(proposer)?;
        Ok(AdminMessage::Upgrade(Box::new(signed)))
    }

    pub fn approve(
        admin_proposal_id: AdminProposalId,
        payload: &ProposalPayload,
        validator: &SecretKey,
    ) -> Result<Self> {
        let signature = validator.sign_recoverable(payload.as_bytes())?;
        Ok(AdminMessage::Approve {
            admin_proposal_id,
            signature,
        })
    }
}

/// The action that should be taken for this proposal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposalPayload {
    NewSet(TaggedJson<ValidatorSet>),
    MigrateContract(TaggedJson<MigrateContract>),
    Upgrade(TaggedJson<Upgrade>),
}
impl ProposalPayload {
    pub(super) fn as_bytes(&self) -> &[u8] {
        match self {
            ProposalPayload::NewSet(set) => set.as_bytes(),
            ProposalPayload::MigrateContract(migrate) => migrate.as_bytes(),
            ProposalPayload::Upgrade(upgrade) => upgrade.as_bytes(),
        }
    }
}

/// Monotonically increasing identifier for proposed validator set changes.
#[derive(
    serde::Serialize,
    serde::Deserialize,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Copy,
    Hash,
    Debug,
    Default,
)]
pub struct AdminProposalId(pub u64);

impl AdminProposalId {
    pub fn next(self) -> AdminProposalId {
        AdminProposalId(self.0 + 1)
    }
}

impl Display for AdminProposalId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl MerkleSerializeRaw for AdminProposalId {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store(&self.0)
    }
}
impl MerkleDeserializeRaw for AdminProposalId {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load().map(Self)
    }
}

/// Information defining the initial state of an app.
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug, Clone)]
pub struct GenesisInfo {
    /// Unique identifier for this application, never changes.
    pub kolme_ident: String,
    /// The rules defining the validator set for this app.
    pub validator_set: ValidatorSet,
    /// Initial configuration of different chains
    pub chains: ConfiguredChains,
    /// Initial version of the code the chain starts with.
    pub version: String,
}

impl GenesisInfo {
    pub fn validate(&self) -> Result<()> {
        self.validator_set.validate()?;
        Ok(())
    }
}

#[derive(PartialEq, Default, Debug, Clone)]
pub struct ChainStates(MerkleMap<ExternalChain, ChainState>);

impl From<ConfiguredChains> for ChainStates {
    fn from(c: ConfiguredChains) -> Self {
        ChainStates(c.0.into_iter().map(|(k, v)| (k, v.into_state())).collect())
    }
}

impl MerkleSerialize for ChainStates {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store(&self.0)
    }
}

impl MerkleDeserialize for ChainStates {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load().map(Self)
    }
}

impl ChainStates {
    pub fn get(&self, chain: ExternalChain) -> Result<&ChainState, CoreStateError> {
        self.0
            .get(&chain)
            .ok_or(CoreStateError::ChainNotSupported { chain })
    }

    pub fn get_mut(&mut self, chain: ExternalChain) -> Result<&mut ChainState, CoreStateError> {
        self.0
            .get_mut(&chain)
            .ok_or(CoreStateError::ChainNotSupported { chain })
    }

    pub fn iter(&self) -> impl Iterator<Item = (ExternalChain, &ChainState)> {
        self.0.iter().map(|(k, v)| (*k, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = ExternalChain> + '_ {
        self.0.keys().copied()
    }
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Default, Debug, Clone)]
pub struct ConfiguredChains(pub(crate) BTreeMap<ExternalChain, ChainConfig>);

impl ConfiguredChains {
    pub fn insert_solana(&mut self, chain: SolanaChain, config: ChainConfig) -> Result<()> {
        use kolme_solana_bridge_client::pubkey::Pubkey;

        match &config.bridge {
            BridgeContract::NeededCosmosBridge { .. } => {
                return Err(anyhow::anyhow!(
                    "Trying to configure a Cosmos contract as a Solana bridge."
                ))
            }
            BridgeContract::NeededSolanaBridge { program_id } => Pubkey::from_str(program_id)?,
            BridgeContract::Deployed(program_id) => Pubkey::from_str(program_id)?,
        };

        self.0.insert(chain.into(), config);

        Ok(())
    }

    pub fn insert_cosmos(&mut self, chain: CosmosChain, config: ChainConfig) -> Result<()> {
        use cosmos::Address;

        match &config.bridge {
            BridgeContract::NeededSolanaBridge { .. } => {
                return Err(anyhow::anyhow!(
                    "Trying to configure a Solana program as a Cosmos bridge."
                ))
            }
            BridgeContract::NeededCosmosBridge { .. } => (),
            BridgeContract::Deployed(program_id) => {
                Address::from_str(program_id)?;
            }
        }

        self.0.insert(chain.into(), config);

        Ok(())
    }

    #[cfg(feature = "pass_through")]
    pub fn insert_pass_through(&mut self, config: ChainConfig) -> Result<()> {
        if let BridgeContract::Deployed(_) = config.bridge {
            if self
                .0
                .get(&ExternalChain::PassThrough)
                .is_some_and(|existing| *existing != config)
            {
                Err(anyhow::anyhow!(
                    "Multiple pass-through bridges are not supported"
                ))
            } else {
                self.0.insert(ExternalChain::PassThrough, config);
                Ok(())
            }
        } else {
            Err(anyhow::anyhow!(
                "Pass-through bridge can't require Cosmos or Solana bridge contract"
            ))
        }
    }
}

/// Input and output for a single data load while processing a block.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BlockDataLoad {
    /// Description of the request
    pub request: String,
    /// The resulting value
    pub response: String,
}

/// A specific action to be taken as a result of an execution.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub enum ExecAction {
    Transfer {
        chain: ExternalChain,
        recipient: Wallet,
        funds: Vec<AssetAmount>,
    },
    /// Replace a single validator using a self-replacement.
    SelfReplace(Box<SignedTaggedJson<SelfReplace>>),
    /// Replace the entire validator set.
    NewSet {
        validator_set: TaggedJson<ValidatorSet>,
        approvals: Vec<SignatureWithRecovery>,
    },
    MigrateContract {
        migrate_contract: TaggedJson<MigrateContract>,
    },
}

impl ExecAction {
    /// - Cosmos chains: returns a JSON string of a BridgeActionId and Vec<cosmwasm_std::CosmosMsg>
    /// - Solana chains: returns a base64 encoded string of a borsh serialized kolme_solana_bridge_client::Payload binary
    pub(crate) fn to_payload(
        &self,
        chain: ExternalChain,
        config: &ChainConfig,
        id: BridgeActionId,
    ) -> Result<String> {
        use kolme_solana_bridge_client::{pubkey::Pubkey as SolanaPubkey, TokenProgram};
        use shared::{cosmos, solana};

        match self {
            Self::Transfer {
                chain: chain2,
                recipient,
                funds,
            } => {
                assert_eq!(&chain, chain2);

                match chain.name() {
                    ChainName::Cosmos => {
                        let mut coins = vec![];
                        for AssetAmount { id, amount } in funds {
                            let denom = config
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

                        let message = cosmwasm_std::CosmosMsg::Bank(cosmwasm_std::BankMsg::Send {
                            to_address: recipient.0.clone(),
                            amount: coins,
                        });

                        let payload = serde_json::to_string(&cosmos::PayloadWithId {
                            id,
                            action: shared::cosmos::CosmosAction::Cosmos(vec![message]),
                        })?;

                        Ok(payload)
                    }
                    ChainName::Solana => {
                        let mut coins: Vec<(&str, u128)> = Vec::with_capacity(funds.len());
                        for coin in funds {
                            let asset = config
                                .assets
                                .iter()
                                .find(|(_name, config)| config.asset_id == coin.id)
                                .context("Unsupported asset ID")?;

                            coins.push((&asset.0 .0, coin.amount));
                        }

                        let program_id = match config.bridge.clone() {
                            BridgeContract::NeededCosmosBridge { .. } => unreachable!(),
                            BridgeContract::NeededSolanaBridge { program_id } => program_id,
                            BridgeContract::Deployed(program_id) => program_id,
                        };

                        // TODO: Need to support multiple signed messages (https://github.com/fpco/kolme/issues/106)
                        let mint = SolanaPubkey::from_str(coins[0].0)?;
                        let program_id = SolanaPubkey::from_str(&program_id)?;
                        let recipient = SolanaPubkey::from_str(&recipient.0)?;
                        let amount = u64::try_from(coins[0].1)?;

                        let payload = kolme_solana_bridge_client::transfer_payload(
                            id.0,
                            program_id,
                            // TODO: Determine the type of token being sent (https://github.com/fpco/kolme/issues/105)
                            TokenProgram::Legacy,
                            mint,
                            recipient,
                            amount,
                        );

                        serialize_solana_payload(&payload)
                    }
                    #[cfg(feature = "pass_through")]
                    ChainName::PassThrough => {
                        let payload = serde_json::to_string(&pass_through::Transfer {
                            bridge_action_id: id,
                            recipient: recipient.clone(),
                            funds: funds.clone(),
                        })?;
                        Ok(payload)
                    }
                }
            }
            ExecAction::SelfReplace(self_replace) => match chain.name() {
                ChainName::Cosmos => {
                    let payload = serde_json::to_string(&cosmos::PayloadWithId {
                        id,
                        action: cosmos::CosmosAction::SelfReplace {
                            rendered: self_replace.message.as_str().to_owned(),
                            signature: SignatureWithRecovery {
                                recid: self_replace.recovery_id,
                                sig: self_replace.signature,
                            },
                        },
                    })?;

                    Ok(payload)
                }
                ChainName::Solana => {
                    let payload = solana::Payload {
                        id: id.0,
                        action: solana::SignedAction::SelfReplace {
                            rendered: self_replace.message.as_str().to_owned(),
                            signature: SignatureWithRecovery {
                                recid: self_replace.recovery_id,
                                sig: self_replace.signature,
                            },
                        },
                    };

                    serialize_solana_payload(&payload)
                }
                #[cfg(feature = "pass_through")]
                ChainName::PassThrough => todo!(),
            },
            ExecAction::NewSet {
                validator_set,
                approvals,
            } => match chain.name() {
                ChainName::Cosmos => {
                    let payload = serde_json::to_string(&cosmos::PayloadWithId {
                        id,
                        action: shared::cosmos::CosmosAction::NewSet {
                            rendered: validator_set.as_str().to_owned(),
                            approvals: approvals.clone(),
                        },
                    })?;

                    Ok(payload)
                }
                ChainName::Solana => {
                    let payload = solana::Payload {
                        id: id.0,
                        action: solana::SignedAction::NewSet {
                            rendered: validator_set.as_str().to_owned(),
                            approvals: approvals.clone(),
                        },
                    };

                    serialize_solana_payload(&payload)
                }
                #[cfg(feature = "pass_through")]
                ChainName::PassThrough => todo!(),
            },
            ExecAction::MigrateContract { migrate_contract } => {
                match chain.name() {
                    ChainName::Cosmos => {
                        let contract_addr = match &config.bridge {
                        BridgeContract::Deployed(addr) => addr.clone(),
                        _ => anyhow::bail!("Unable to migrate contract for chain {chain}: contract isn't deployed")
                    };
                        let MigrateContract {
                            chain: _,
                            new_code_id,
                            message,
                        } = migrate_contract.as_inner();
                        let msg = CosmosMsg::Wasm(cosmwasm_std::WasmMsg::Migrate {
                            contract_addr,
                            new_code_id: *new_code_id,
                            msg: Binary::from(serde_json::to_vec(message)?),
                        });
                        let payload = serde_json::to_string(&cosmos::PayloadWithId {
                            id,
                            action: shared::cosmos::CosmosAction::Cosmos(vec![msg]),
                        })?;

                        Ok(payload)
                    }
                    ChainName::Solana => todo!(),
                    #[cfg(feature = "pass_through")]
                    ChainName::PassThrough => todo!(),
                }
            }
        }
    }
}

fn serialize_solana_payload(payload: &shared::solana::Payload) -> Result<String> {
    let len = borsh::object_length(&payload)
        .map_err(|x| anyhow::anyhow!("Error serializing Solana bridge payload: {:?}", x))?;

    let mut buf = Vec::with_capacity(len);
    borsh::BorshSerialize::serialize(&payload, &mut buf)
        .map_err(|x| anyhow::anyhow!("Error serializing Solana bridge payload: {:?}", x))?;

    let payload = base64::engine::general_purpose::STANDARD.encode(&buf);

    Ok(payload)
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
    /// A transaction failed in the processor.
    ///
    /// The message is signed by the processor. Only failed transactions
    /// signed by the real processor should be respected for dropping
    /// transactions from the mempool.
    FailedTransaction(Arc<SignedTaggedJson<FailedTransaction>>),
    /// Notification from the processor of the latest known block.
    LatestBlock(Arc<SignedTaggedJson<LatestBlock>>),
    /// Instructs nodes to remove a transaction from their mempool.
    ///
    /// This is necessary when a node broadcasts a transaction that has already
    /// been included in the blockchain. Other nodes, upon receiving this
    //  notification, should evict the specified transaction from their mempool
    /// to prevent broadcasting it again. The `TxHash` identifies the transaction
    /// to be evicted.
    EvictMempoolTransaction(Arc<SignedTaggedJson<TxHash>>),
}

impl<AppMessage> Notification<AppMessage> {
    pub(crate) fn hash(&self) -> Option<Sha256Hash> {
        match self {
            Notification::NewBlock(signed_block) => Some(signed_block.hash().0),
            Notification::GenesisInstantiation { .. } => None,
            Notification::FailedTransaction(signed_tagged_json) => {
                Some(signed_tagged_json.message_hash())
            }
            Notification::LatestBlock(signed_tagged_json) => {
                Some(signed_tagged_json.message_hash())
            }
            Notification::EvictMempoolTransaction(signed_tagged_json) => {
                Some(signed_tagged_json.message_hash())
            }
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FailedTransaction {
    pub txhash: TxHash,
    /// Block height we attempted to generate.
    pub proposed_height: BlockHeight,
    pub error: KolmeError,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LatestBlock {
    pub height: BlockHeight,
    /// When this message was generated.
    pub when: jiff::Timestamp,
}

/// Represents distinct occurrences in the core of Kolme that could be relevant to users.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogEvent {
    ProcessedBridgeEvent(LogBridgeEvent),
    NewAdminProposal(AdminProposalId),
    AdminProposalApproved(AdminProposalId),
    NewBridgeAction {
        chain: ExternalChain,
        id: BridgeActionId,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogBridgeEvent {
    Regular {
        bridge_event_id: BridgeEventId,
        account_id: AccountId,
    },
}

/// Whether we validate data loads during block processing.
///
/// Default: [DataLoadValidation::ValidateDataLoads].
#[derive(Default, PartialEq, Eq, Debug, Clone, Copy, Hash)]
pub enum DataLoadValidation {
    /// Validate that data loaded during a block is accurate.
    ///
    /// This may involve additional I/O, such as making HTTP requests.
    #[default]
    ValidateDataLoads,
    /// Trust that the loaded data is accurate.
    TrustDataLoads,
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;
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

    #[tokio::main]
    async fn idempotency_helper<T>(value: T) -> quickcheck::TestResult
    where
        T: MerkleSerializeRaw + MerkleDeserializeRaw + PartialEq,
    {
        let mut store = MerkleMemoryStore::default();
        let serialized = merkle_map::save(&mut store, &value).await.unwrap();
        let deserialized = merkle_map::load::<T, _>(&mut store, serialized)
            .await
            .unwrap();

        quickcheck::TestResult::from_bool(value == deserialized)
    }

    macro_rules! serializing_idempotency_for {
        ($value_type: ty, $test_name: ident) => {
            quickcheck! {
                fn $test_name(value: $value_type) -> quickcheck::TestResult {
                    idempotency_helper(value)
                }
            }
        };
    }
    serializing_idempotency_for!(AssetConfig, serialization_is_idempotent_for_assetconfig);
    serializing_idempotency_for!(AssetId, serialization_is_idempotent_for_assetid);
    serializing_idempotency_for!(AssetName, serialization_is_idempotent_for_assetname);
    serializing_idempotency_for!(AccountId, serialization_is_idempotent_for_accountid);
    serializing_idempotency_for!(AccountNonce, serialization_is_idempotent_for_accountnonce);
    serializing_idempotency_for!(
        BridgeContract,
        serialization_is_idempotent_for_bridgecontract
    );
    serializing_idempotency_for!(ChainConfig, serialization_is_idempotent_for_chainconfig);
    serializing_idempotency_for!(ExternalChain, serialization_is_idempotent_for_externalchain);
    serializing_idempotency_for!(Wallet, serialization_is_idempotent_for_wallet);
}
