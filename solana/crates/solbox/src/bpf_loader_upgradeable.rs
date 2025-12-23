use pinocchio::{
    account_info::AccountInfo,
    program_error::ProgramError,
    pubkey::{find_program_address, Pubkey},
};
use pinocchio_pubkey::declare_id;
use serde_derive::{Deserialize, Serialize};

use crate::{assert_owner, Result};

declare_id!("BPFLoaderUpgradeab1e11111111111111111111111");

#[derive(Deserialize, Serialize, PartialEq, Eq, Clone, Copy, Debug)]
pub enum State {
    /// Account is not initialized.
    Uninitialized,
    /// A Buffer account.
    Buffer {
        /// Authority address
        authority_address: Option<Pubkey>,
        // The raw program data follows this serialized structure in the
        // account's data.
    },
    /// A Program account.
    Program {
        /// Address of the ProgramData account.
        programdata_address: Pubkey,
    },
    // A ProgramData account.
    ProgramData {
        /// Slot that the program was last modified.
        slot: u64,
        /// Address of the Program's upgrade authority.
        upgrade_authority_address: Option<Pubkey>,
        // The raw program data follows this serialized structure in the
        // account's data.
    },
}

pub fn get_program_data_address(program_id: &Pubkey) -> Pubkey {
    find_program_address(&[program_id.as_ref()], &ID).0
}

pub fn get_state(acc: &AccountInfo) -> Result<State> {
    assert_owner(acc, &ID)?;

    let data = acc.try_borrow_data()?;
    let state = bincode::deserialize(&data).map_err(|_| ProgramError::InvalidAccountData)?;

    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_loader_v3_interface::state::UpgradeableLoaderState;

    #[test]
    fn bpf_loader_state_serde() {
        let bytes = bincode::serialize(&UpgradeableLoaderState::Uninitialized).unwrap();
        let state = State::Uninitialized;

        assert_eq!(bytes, bincode::serialize(&state).unwrap());
        assert_eq!(bincode::deserialize::<State>(&bytes).unwrap(), state);

        let bytes = bincode::serialize(&UpgradeableLoaderState::Buffer {
            authority_address: None,
        })
        .unwrap();
        let state = State::Buffer {
            authority_address: None,
        };

        assert_eq!(bytes, bincode::serialize(&state).unwrap());
        assert_eq!(bincode::deserialize::<State>(&bytes).unwrap(), state);

        let bytes = bincode::serialize(&UpgradeableLoaderState::Buffer {
            authority_address: Some([1; 32].into()),
        })
        .unwrap();
        let state = State::Buffer {
            authority_address: Some([1; 32]),
        };

        assert_eq!(bytes, bincode::serialize(&state).unwrap());
        assert_eq!(bincode::deserialize::<State>(&bytes).unwrap(), state);

        let bytes = bincode::serialize(&UpgradeableLoaderState::Program {
            programdata_address: [2; 32].into(),
        })
        .unwrap();
        let state = State::Program {
            programdata_address: [2; 32],
        };

        assert_eq!(bytes, bincode::serialize(&state).unwrap());
        assert_eq!(bincode::deserialize::<State>(&bytes).unwrap(), state);

        let bytes = bincode::serialize(&UpgradeableLoaderState::ProgramData {
            slot: 1234,
            upgrade_authority_address: Some([2; 32].into()),
        })
        .unwrap();
        let state = State::ProgramData {
            slot: 1234,
            upgrade_authority_address: Some([2; 32]),
        };

        assert_eq!(bytes, bincode::serialize(&state).unwrap());
        assert_eq!(bincode::deserialize::<State>(&bytes).unwrap(), state);
    }
}
