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
    pub(super) validator_set: MerkleLockable<ValidatorSet>,
    pub(super) chains: ChainStates,
    pub(super) accounts: Accounts,
    pub(super) key_rotation_state: MerkleLockable<KeyRotationState>,
}

impl MerkleSerialize for FrameworkState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let FrameworkState {
            validator_set: config,
            chains,
            accounts,
            key_rotation_state,
        } = self;
        serializer.store(config)?;
        serializer.store(chains)?;
        serializer.store(accounts)?;
        serializer.store(key_rotation_state)?;
        Ok(())
    }
}

impl MerkleDeserialize for FrameworkState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(FrameworkState {
            validator_set: deserializer.load()?,
            chains: deserializer.load()?,
            accounts: deserializer.load()?,
            key_rotation_state: deserializer.load()?,
        })
    }
}

/// Manages the state of key rotation activities.
#[derive(Default, Clone, Debug)]
pub struct KeyRotationState {
    /// Next change set ID to be issued.
    pub next_change_set_id: ChangeSetId,
    /// Currently in-flight proposals
    pub change_sets: BTreeMap<ChangeSetId, PendingChangeSet>,
}

/// Status of an in-flight key rotation.
#[derive(Clone, Debug)]
pub struct PendingChangeSet {
    pub validator_set: TaggedJson<ValidatorSet>,
    pub approvals: BTreeMap<PublicKey, SignatureWithRecovery>,
}
impl PendingChangeSet {
    /// Do we have enough approvals to meet the current validator set rules?
    pub(crate) fn has_sufficient_approvals(&self, validator_set: &ValidatorSet) -> bool {
        let mut group_approvals = 0;
        if self.approvals.contains_key(&validator_set.processor) {
            group_approvals += 1;
        }
        if self.fulfills_groups(&validator_set.listeners, validator_set.needed_listeners) {
            group_approvals += 1;
        }
        if self.fulfills_groups(&validator_set.approvers, validator_set.needed_approvers) {
            group_approvals += 1;
        }

        group_approvals >= 2
    }

    fn fulfills_groups(&self, group: &BTreeSet<PublicKey>, needed: u16) -> bool {
        let mut approvals = 0;
        for member in group {
            if self.approvals.contains_key(member) {
                approvals += 1;
            }
        }
        approvals >= needed
    }
}

impl MerkleSerialize for KeyRotationState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self {
            next_change_set_id,
            change_sets,
        } = self;
        serializer.store(next_change_set_id)?;
        serializer.store(change_sets)?;
        Ok(())
    }
}
impl MerkleDeserialize for KeyRotationState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            next_change_set_id: deserializer.load()?,
            change_sets: deserializer.load()?,
        })
    }
}

impl MerkleSerialize for PendingChangeSet {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self {
            validator_set,
            approvals,
        } = self;
        serializer.store(validator_set)?;
        serializer.store(approvals)?;
        Ok(())
    }
}
impl MerkleDeserialize for PendingChangeSet {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            validator_set: deserializer.load()?,
            approvals: deserializer.load()?,
        })
    }
}

impl FrameworkState {
    pub(super) fn new(
        GenesisInfo {
            kolme_ident: _,
            validator_set,
            chains,
        }: &GenesisInfo,
    ) -> Self {
        FrameworkState {
            validator_set: MerkleLockable::new(validator_set.clone()),
            chains: ChainStates::from(chains.clone()),
            accounts: Accounts::default(),
            key_rotation_state: MerkleLockable::new(KeyRotationState::default()),
        }
    }

    pub fn get_validator_set(&self) -> &ValidatorSet {
        self.validator_set.as_ref()
    }

    pub fn get_key_rotation_state(&self) -> &KeyRotationState {
        self.key_rotation_state.as_ref()
    }

    pub fn get_chain_states(&self) -> &ChainStates {
        &self.chains
    }

    pub(super) fn validate(&self) -> Result<(), ValidatorSetError> {
        self.get_validator_set().validate()
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
}
