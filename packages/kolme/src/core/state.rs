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
    pub(super) admin_proposal_state: MerkleLockable<AdminProposalState>,
    pub(super) version: String,
}

impl MerkleSerialize for FrameworkState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let FrameworkState {
            validator_set: config,
            chains,
            accounts,
            admin_proposal_state: key_rotation_state,
            version,
        } = self;
        serializer.store(config)?;
        serializer.store(chains)?;
        serializer.store(accounts)?;
        serializer.store(key_rotation_state)?;
        serializer.store(version)?;
        Ok(())
    }
}

impl MerkleDeserialize for FrameworkState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(FrameworkState {
            validator_set: deserializer.load()?,
            chains: deserializer.load()?,
            accounts: deserializer.load()?,
            admin_proposal_state: deserializer.load()?,
            version: deserializer.load()?,
        })
    }
}

/// Manages the state of admin proposals.
#[derive(Default, Clone, Debug)]
pub struct AdminProposalState {
    /// Next admin proposal ID to be issued.
    pub next_admin_proposal_id: AdminProposalId,
    /// Currently in-flight proposals
    pub proposals: BTreeMap<AdminProposalId, PendingProposal>,
}

/// Status of an in-flight admin proposals.
#[derive(Clone, Debug)]
pub struct PendingProposal {
    pub payload: ProposalPayload,
    pub approvals: BTreeMap<PublicKey, SignatureWithRecovery>,
}

impl PendingProposal {
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

impl MerkleSerialize for AdminProposalState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self {
            next_admin_proposal_id,
            proposals,
        } = self;
        serializer.store(next_admin_proposal_id)?;
        serializer.store(proposals)?;
        Ok(())
    }
}
impl MerkleDeserialize for AdminProposalState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            next_admin_proposal_id: deserializer.load()?,
            proposals: deserializer.load()?,
        })
    }
}

impl MerkleSerialize for PendingProposal {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self { payload, approvals } = self;
        serializer.store(payload)?;
        serializer.store(approvals)?;
        Ok(())
    }
}
impl MerkleDeserialize for PendingProposal {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            payload: deserializer.load()?,
            approvals: deserializer.load()?,
        })
    }
}

impl MerkleSerialize for ProposalPayload {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        match self {
            ProposalPayload::NewSet(new_set) => {
                serializer.store_byte(0);
                serializer.store(new_set)?;
            }
            ProposalPayload::MigrateContract(migrate) => {
                serializer.store_byte(1);
                serializer.store(migrate)?;
            }
            ProposalPayload::Upgrade(upgrade) => {
                serializer.store_byte(2);
                serializer.store(upgrade)?;
            }
        }
        Ok(())
    }
}
impl MerkleDeserialize for ProposalPayload {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        match deserializer.pop_byte()? {
            0 => Ok(ProposalPayload::NewSet(deserializer.load()?)),
            1 => Ok(ProposalPayload::MigrateContract(deserializer.load()?)),
            2 => Ok(ProposalPayload::Upgrade(deserializer.load()?)),
            byte => Err(MerkleSerialError::UnexpectedMagicByte { byte }),
        }
    }
}

impl FrameworkState {
    pub(super) fn new(
        GenesisInfo {
            kolme_ident: _,
            validator_set,
            chains,
            version,
        }: &GenesisInfo,
    ) -> Self {
        FrameworkState {
            validator_set: MerkleLockable::new(validator_set.clone()),
            chains: ChainStates::from(chains.clone()),
            accounts: Accounts::default(),
            admin_proposal_state: MerkleLockable::new(AdminProposalState::default()),
            version: version.clone(),
        }
    }

    pub fn get_validator_set(&self) -> &ValidatorSet {
        self.validator_set.as_ref()
    }

    pub fn get_admin_proposal_state(&self) -> &AdminProposalState {
        self.admin_proposal_state.as_ref()
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

    pub fn get_chain_version(&self) -> &String {
        &self.version
    }
}
