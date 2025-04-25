use crate::*;

#[derive(thiserror::Error, Debug)]
pub enum CoreStateError {
    #[error("The chain '{chain}' is not supported")]
    ChainNotSupported { chain: ExternalChain },
    #[error("The asset '{asset_id}' on chain '{chain}' is not supported")]
    AssetNotSupported {
        chain: ExternalChain,
        asset_id: AssetId,
    },
}

/// Raw framework state that can be serialized to the database.
#[derive(Clone, Debug)]
pub struct FrameworkState {
    pub(super) processor: PublicKey,
    // FIXME replace BTreeSet with MerkleSet, and implement a MerkleSet datatype.
    pub(super) listeners: BTreeSet<PublicKey>,
    pub(super) needed_listeners: usize,
    pub(super) approvers: BTreeSet<PublicKey>,
    pub(super) needed_approvers: usize,
    pub(super) chains: ChainStates,
    pub(super) accounts: Accounts,
}

impl MerkleSerialize for FrameworkState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let FrameworkState {
            processor,
            listeners,
            needed_listeners,
            approvers,
            needed_approvers,
            chains,
            accounts,
        } = self;
        serializer.store(processor)?;
        serializer.store(listeners)?;
        serializer.store(needed_listeners)?;
        serializer.store(approvers)?;
        serializer.store(needed_approvers)?;
        serializer.store(chains)?;
        serializer.store(accounts)?;
        Ok(())
    }
}

impl MerkleDeserialize for FrameworkState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(FrameworkState {
            processor: deserializer.load()?,
            listeners: deserializer.load()?,
            needed_listeners: deserializer.load()?,
            approvers: deserializer.load()?,
            needed_approvers: deserializer.load()?,
            chains: deserializer.load()?,
            accounts: deserializer.load()?,
        })
    }
}

impl FrameworkState {
    pub(super) fn new(
        GenesisInfo {
            kolme_ident: _,
            processor,
            listeners,
            needed_listeners,
            approvers,
            needed_approvers,
            chains,
        }: &GenesisInfo,
    ) -> Self {
        FrameworkState {
            processor: *processor,
            listeners: listeners.clone(),
            needed_listeners: *needed_listeners,
            approvers: approvers.clone(),
            needed_approvers: *needed_approvers,
            chains: ChainStates::from(chains.clone()),
            accounts: Accounts::default(),
        }
    }

    pub(super) fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.listeners.len() >= self.needed_listeners);
        anyhow::ensure!(self.needed_listeners > 0);
        anyhow::ensure!(self.approvers.len() >= self.needed_approvers);
        anyhow::ensure!(self.needed_approvers > 0);
        Ok(())
    }

    pub(super) fn get_asset_config(
        &self,
        chain: ExternalChain,
        asset_id: AssetId,
    ) -> Result<&AssetConfig, CoreStateError> {
        self.chains
            .get(chain)?
            .config
            .assets
            .values()
            .find(|config| config.asset_id == asset_id)
            .ok_or(CoreStateError::AssetNotSupported { chain, asset_id })
    }

    pub(super) fn instantiate_args(&self) -> InstantiateArgs {
        InstantiateArgs {
            processor: self.processor,
            listeners: self.listeners.clone(),
            needed_listeners: self.needed_listeners,
            approvers: self.approvers.clone(),
            needed_approvers: self.needed_approvers,
        }
    }
}
