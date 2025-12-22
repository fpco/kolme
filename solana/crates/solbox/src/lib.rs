pub mod bpf_loader_upgradeable;

pub use pinocchio;
pub use pinocchio_pubkey as pubkey;
pub use pinocchio_system as system;
pub use pinocchio_token as token;

use bitflags::bitflags;
use borsh::{BorshDeserialize, BorshSerialize};
use once_cell::unsync::Lazy;
use pinocchio::{
    account_info::AccountInfo,
    instruction::{Seed, Signer},
    program_error::ProgramError,
    pubkey::{create_program_address, find_program_address, Pubkey},
    syscalls,
    sysvars::{
        clock,
        rent::{self, Rent},
        Sysvar,
    },
};
use smallvec::SmallVec;

pub mod token2022 {
    use pinocchio_pubkey::declare_id;

    declare_id!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
}

pub type Result<T> = std::result::Result<T, ProgramError>;

pub struct Context<'a> {
    /// The currently executed program pubkey.
    pub program_id: &'a Pubkey,
    /// The raw unchecked accounts that were passed to the entrypoint.
    pub accounts: &'a [AccountInfo],
    rent: Lazy<Result<Rent>>,
}

pub trait PdaDataSerialize {
    fn serialized_size(&self) -> u64;
    fn serialize(&self, storage: &mut [u8]) -> Result<()>;
}

pub trait PdaDataDeserialize: Sized {
    fn deserialize(storage: &[u8]) -> Result<Self>;
}

pub struct PdaData<T, const D: u8> {
    pub bump: u8,
    pub data: T,
}

#[derive(Clone, Copy, Debug)]
pub struct PdaDerivation<'a> {
    /// Seeds bytes **excluding** a bump seed.
    pub seeds: &'a [&'a [u8]],
    /// A bump seed can be provided if the account is known ahead of time.
    /// If deriving accounts based on instruction input this should most likely be [`None`].
    pub bump: Option<u8>,
}

bitflags! {
    pub struct AccountReq: u8 {
        const SIGNER = 1;
        const WRITABLE = 1 << 1;
        const EXECUTABLE = 1 << 2;
    }
}

#[macro_export]
macro_rules! log {
    ($msg:expr) => { $crate::log($msg) };
    ($($arg:tt)*) => ($crate::log(&format!($($arg)*)));
}

#[inline]
pub fn log(msg: &str) {
    unsafe {
        syscalls::sol_log_(msg.as_ptr(), msg.len() as u64);
    }
}

/// Print some slices as base64.
#[inline]
pub fn log_base64(data: &[&[u8]]) {
    unsafe {
        syscalls::sol_log_data(data as *const _ as *const u8, data.len() as u64);
    }
}

#[inline]
pub fn log_pubkey(pubkey: &Pubkey) {
    unsafe { syscalls::sol_log_pubkey(pubkey as *const _ as *const u8) };
}

#[inline]
pub fn assert_owner(acc: &AccountInfo, owner_key: &Pubkey) -> Result<()> {
    if !acc.is_owned_by(owner_key) {
        return Err(ProgramError::InvalidAccountOwner);
    }

    Ok(())
}

impl<'a> Context<'a> {
    #[inline]
    pub fn new(program_id: &'a Pubkey, accounts: &'a [AccountInfo]) -> Self {
        Self {
            program_id,
            accounts,
            rent: Lazy::new(Rent::get),
        }
    }

    #[inline]
    pub fn rent(&self) -> Result<Rent> {
        Lazy::force(&self.rent).as_ref().map_err(|e| *e).cloned()
    }

    #[inline]
    pub fn assert_accounts_len(&self, len: usize) -> Result<()> {
        if self.accounts.len() != len {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        Ok(())
    }

    #[inline]
    pub fn account(&self, index: usize, req: AccountReq) -> Result<AccountInfo> {
        let acc = self.accounts[index];

        let not_ok = (req.contains(AccountReq::SIGNER) & !acc.is_signer())
            | (req.contains(AccountReq::WRITABLE) & !acc.is_writable())
            | (req.contains(AccountReq::EXECUTABLE) & !acc.executable());

        if not_ok {
            return Err(ProgramError::InvalidArgument);
        }

        Ok(acc)
    }

    #[inline]
    pub fn assert_is_system_program(&self, index: usize) -> Result<AccountInfo> {
        let account = self.accounts[index];

        if account.key() != &system::ID {
            return Err(ProgramError::InvalidArgument);
        }

        Ok(account)
    }

    #[inline]
    pub fn assert_is_token_program(&self, index: usize) -> Result<AccountInfo> {
        let account = self.accounts[index];

        if account.key() != &token::ID && account.key() != &token2022::ID {
            return Err(ProgramError::InvalidArgument);
        }

        Ok(account)
    }

    #[inline]
    pub fn assert_is_loader_v3(&self, index: usize) -> Result<AccountInfo> {
        let account = self.accounts[index];

        if account.key() != &bpf_loader_upgradeable::ID {
            return Err(ProgramError::InvalidArgument);
        }

        Ok(account)
    }

    #[inline]
    pub fn assert_is_rent_sysvar(&self, index: usize) -> Result<AccountInfo> {
        let account = self.accounts[index];

        if account.key() != &rent::RENT_ID {
            return Err(ProgramError::InvalidArgument);
        }

        Ok(account)
    }

    #[inline]
    pub fn assert_is_clock_sysvar(&self, index: usize) -> Result<AccountInfo> {
        let account = self.accounts[index];

        if account.key() != &clock::CLOCK_ID {
            return Err(ProgramError::InvalidArgument);
        }

        Ok(account)
    }

    /// Will create and initialize the account with the given data. Will return an error if the program is already the owner (account was already initialized).
    /// Will check if the provided PDA address is legitimate by deriving its key and comparing it to the provided account.
    pub fn init_pda<T: PdaDataSerialize, const D: u8>(
        &self,
        payer: &AccountInfo,
        pda: &AccountInfo,
        derivation: PdaDerivation,
        data: T,
    ) -> Result<PdaData<T, D>> {
        if pda.owner() == self.program_id {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        let (address, bump) = if let Some(bump) = derivation.bump {
            let bump_slice = &[bump];
            let mut seeds = SmallVec::<[&[u8]; 4]>::from_iter(derivation.seeds.iter().copied());

            // Can't use chain() like below because the compiler thinks it's being passed &u8 even when bump_slice is explicity typed as &[u8]...
            seeds.push(bump_slice);

            let address = create_program_address(seeds.as_slice(), self.program_id)?;

            (address, bump)
        } else {
            find_program_address(derivation.seeds, self.program_id)
        };

        if pda.key() != &address {
            return Err(ProgramError::InvalidArgument);
        }

        let bump_slice = &[bump];
        let bump_seed = [Seed::from(bump_slice)];
        let seeds: SmallVec<[Seed; 4]> = SmallVec::from_iter(
            derivation
                .seeds
                .iter()
                .map(|x| Seed::from(*x))
                .chain(bump_seed),
        );

        let rent = self.rent()?;
        let space = data.serialized_size() + 2; // +1 for discriminator byte and +1 for bump seed
        let signers = [Signer::from(seeds.as_slice())];

        let existing_lamports = pda.lamports();

        if existing_lamports == 0 {
            system::instructions::CreateAccount {
                from: payer,
                to: pda,
                lamports: rent.minimum_balance(space as usize).max(1),
                space,
                owner: self.program_id,
            }
            .invoke_signed(&signers)?;

            return Ok(PdaData { bump, data });
        }

        // Anyone can transfer lamports to accounts before they're initialized - in that case, creating the account won't work.
        // In order to get around it, we need to fund the account with enough lamports to be rent exempt,
        // then allocate the required space and set the owner to the current program.

        let required_lamports = rent
            .minimum_balance(space as usize)
            .max(1)
            .saturating_sub(existing_lamports);

        if required_lamports > 0 {
            system::instructions::Transfer {
                from: payer,
                to: pda,
                lamports: required_lamports,
            }
            .invoke()?;
        }

        system::instructions::Allocate {
            account: pda,
            space,
        }
        .invoke_signed(&signers)?;

        system::instructions::Assign {
            account: pda,
            owner: self.program_id,
        }
        .invoke_signed(&signers)?;

        Ok(PdaData { bump, data })
    }

    /// Will load the existing data or return an error if program isn't owner or data is empty (account is uninitialized).
    /// The latter case means that account ownership was transferred from another program (or strictly speaking the PDA wasn't initialized using this library's code).
    /// We currently don't have logic to handle this scenario but if we did it would be a separate method to explicitly opt-out of this data check.
    /// Will check if the provided PDA address is legitimate by deriving its key and comparing it to the provided account.
    pub fn load_pda<T: PdaDataDeserialize, const D: u8>(
        &self,
        pda: &AccountInfo,
        seeds: &[&[u8]],
    ) -> Result<PdaData<T, D>> {
        // We check if data_len() is zero because it's possible for another program to have transferred ownership over.
        // In such case it has to have set the storage to 0 and we catch this early.
        if pda.owner() != self.program_id || pda.data_len() == 0 {
            return Err(ProgramError::UninitializedAccount);
        }

        // Since the account contains data that it means that it was initialized before.
        // We can verify that it was derived using the canonical bump by calling the much cheaper create_program_address()
        assert!(pda.lamports() != 0);
        let data = PdaData::<T, D>::deserialize_from(pda)?;

        let bump_slice = &[data.bump];
        let mut seeds = SmallVec::<[&[u8]; 4]>::from_iter(seeds.iter().copied());

        // Can't use chain() because the compiler thinks it's being passed &u8 even when bump_slice is explicity typed as &[u8]...
        seeds.push(bump_slice);

        let address = create_program_address(seeds.as_slice(), self.program_id)?;

        if pda.key() != &address {
            return Err(ProgramError::InvalidArgument);
        }

        Ok(data)
    }

    /// Will create the account if it hasn't been initialized. Otherwise will load the existing data.
    /// Will check if the provided PDA address is legitimate by deriving its key and comparing it to the provided account.
    pub fn load_or_init_pda<T: PdaDataSerialize + PdaDataDeserialize, const D: u8>(
        &self,
        payer: &AccountInfo,
        pda: &AccountInfo,
        derivation: PdaDerivation,
        init: impl FnOnce() -> T,
    ) -> Result<PdaData<T, D>> {
        // Note: load_pda() also checks for data_len being 0 (see the comment there).
        // Here we don't because we still want to treat the account as initialized in such case.

        if pda.owner() == self.program_id {
            self.load_pda(pda, derivation.seeds)
        } else {
            self.init_pda(payer, pda, derivation, init())
        }
    }
}

impl<'a> PdaDerivation<'a> {
    #[inline]
    pub const fn new(seeds: &'a [&'a [u8]]) -> Self {
        Self { seeds, bump: None }
    }

    #[inline]
    pub const fn with_bump(seeds: &'a [&'a [u8]], bump: u8) -> Self {
        Self {
            seeds,
            bump: Some(bump),
        }
    }

    pub fn as_seeds(&'a self) -> SmallVec<[Seed<'a>; 4]> {
        SmallVec::<[Seed<'a>; 4]>::from_iter(self.seeds.iter().map(|x| Seed::from(*x)))
    }
}

impl<T: PdaDataSerialize, const D: u8> PdaData<T, D> {
    pub fn serialize_into(&self, acc: &AccountInfo) -> Result<()> {
        let bytes = &mut *acc.try_borrow_mut_data()?;

        if bytes.len() < 2 {
            return Err(ProgramError::AccountDataTooSmall);
        }

        bytes[0] = D;
        bytes[1] = self.bump;

        PdaDataSerialize::serialize(&self.data, &mut bytes[2..])
    }

    /// Store the PDA data into the provided account. If the account has insufficient space to store the data,
    /// it will be resized to fit using the provided `payer` account to pay for rent if needed.
    ///
    /// - This method should **not** be used on unitialized accounts! Will return an error if so.
    /// - Maximum permitted size increase is [`pinocchio::account_info::MAX_PERMITTED_DATA_INCREASE`].
    pub fn serialize_into_growable(
        &self,
        acc: &AccountInfo,
        payer: &AccountInfo,
        rent: &Rent,
    ) -> Result<()> {
        let data_size = (self.data.serialized_size() + 2) as usize; // +2 for discriminant and bump seed bytes.
        let bytes = &mut *acc.try_borrow_mut_data()?;

        // Protect against using this method to initialize an account.
        if bytes.is_empty() {
            return Err(ProgramError::UninitializedAccount);
        } else if bytes.len() >= data_size {
            bytes[0] = D;
            bytes[1] = self.bump;

            return PdaDataSerialize::serialize(&self.data, &mut bytes[2..]);
        }

        let needed_space = data_size - bytes.len();
        let _ = bytes; // Drop the borrow because resize() needs it.

        let existing_lamports = acc.lamports();
        let required_lamports = rent
            .minimum_balance(needed_space)
            .max(1)
            .saturating_sub(existing_lamports);

        if required_lamports > 0 {
            system::instructions::Transfer {
                from: payer,
                to: acc,
                lamports: required_lamports,
            }
            .invoke()?;
        }

        acc.resize(data_size)?;

        let bytes = &mut *acc.try_borrow_mut_data()?;
        bytes[0] = D;
        bytes[1] = self.bump;

        PdaDataSerialize::serialize(&self.data, &mut bytes[2..])
    }
}

impl<T: PdaDataDeserialize, const D: u8> PdaData<T, D> {
    pub fn deserialize_from(acc: &AccountInfo) -> Result<Self> {
        let bytes = &*acc.try_borrow_data()?;

        if bytes.len() < 2 {
            return Err(ProgramError::AccountDataTooSmall);
        }

        let discriminator = bytes[0];
        let bump = bytes[1];

        if discriminator != D {
            return Err(ProgramError::InvalidAccountData);
        }

        let data = PdaDataDeserialize::deserialize(&bytes[2..])?;

        Ok(Self { bump, data })
    }
}

impl<T: BorshSerialize + Sized> PdaDataSerialize for T {
    #[inline]
    fn serialized_size(&self) -> u64 {
        borsh::object_length(self).unwrap() as u64
    }

    #[inline]
    fn serialize(&self, mut storage: &mut [u8]) -> Result<()> {
        BorshSerialize::serialize(self, &mut storage).map_err(|_| ProgramError::BorshIoError)
    }
}

impl<T: BorshDeserialize + Sized> PdaDataDeserialize for T {
    #[inline]
    fn deserialize(storage: &[u8]) -> Result<Self> {
        BorshDeserialize::try_from_slice(storage).map_err(|_| ProgramError::BorshIoError)
    }
}
