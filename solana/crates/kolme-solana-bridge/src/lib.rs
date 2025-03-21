use pinocchio_pubkey::declare_id;
use solbox::{
    token,
    pinocchio::{
        account_info::AccountInfo,
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
        program_error::ProgramError,
        ProgramResult,
        syscalls,
        cpi,
        msg,
    },
    AccountReq,
    Context,
    borsh::{BorshDeserialize, BorshSerialize},
    PdaData,
    PdaDerivation,
};

#[cfg(test)]
mod tests;

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

#[cfg(not(feature = "no-entrypoint"))]
use solbox::pinocchio::entrypoint;

#[cfg(not(feature = "no-entrypoint"))]
entrypoint!(process_instruction);

// Instructions
const INITIALIZE_IX: u8 = 0;
const REGULAR_IX: u8 = 1;
const SIGNED_IX: u8 = 2;

// PDAs
const STATE_DERIVATION: PdaDerivation = PdaDerivation::with_bump(&[b"state"], 254);
type StatePda = PdaData<State, 0>;

const TOKEN_HOLDER_SEED: &[u8] = b"token_holder";
type TokenHolderPda = PdaData<TokenHolder, 1>;

const SECP256K1_PUBLIC_KEY_LENGTH: usize = 64;
const SHA256_DIGEST_SIZE: usize = 32;

#[derive(BorshDeserialize, BorshSerialize)]
pub struct InitializeIxData {
    pub needed_executors: u8,
    pub processor: Secp256k1Pubkey,
    pub executors: Vec<Secp256k1Pubkey>,
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct RegularMsgIxData {
    pub keys: Vec<Pubkey>,
    pub transfer_amounts: Vec<u64>
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct SignedMsgIxData {
    /// Signature from the processor
    pub processor: Signature,
    /// Signatures from the executors
    pub executors: Vec<Signature>,
    /// The raw payload to execute
    pub payload: Vec<u8>,
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct Signature {
    pub signature: Vec<u8>,
    pub recovery_id: u8,
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct Payload {
    /// Monotonically increasing ID to ensure messages are sent in the correct order.
    /// It must be included in the payload in order to prevent anyone to from re-submitting
    /// a previously successfully executed message but with a different ID.
    pub id: u32,
    pub program_id: Pubkey,
    pub accounts: Vec<InstructionAccount>,
    pub instruction_data: Vec<u8>
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct InstructionAccount {
    pub pubkey: Pubkey,
    pub is_writable: bool,
}

#[repr(u32)]
pub enum InitIxError {
    NoExecutorsProvided = 0,
    TooManyExecutors = 1,
    InsufficientExecutors = 2,
}

#[repr(u32)]
pub enum RegularIxError {
    WrongATA = 0,
    TransferAmountsMismatch = 1,
    CannotHaveCloseAuthorityOrDelegate = 2,
    ProgramIsNotOwner = 3,
}

#[repr(u32)]
pub enum SignedIxError {
    InsufficientSignatures = 0,
    TooManyExecutors = 1,
    NonExecutorKey = 2,
    DuplicateExecutorKey = 3,
    ProcessorKeyMismatch = 4,
    IncorrectOutgoingId = 5,
    AccountMetaAndPassedAccountsMismatch = 6
}

#[derive(BorshDeserialize, BorshSerialize, Clone, Eq, PartialEq)]
pub struct Secp256k1Pubkey([u8; SECP256K1_PUBLIC_KEY_LENGTH]);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Sha256([u8; SHA256_DIGEST_SIZE]);

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Secp256k1RecoverError {
    InvalidHash = 1000,
    InvalidRecoveryId  = 1001,
    InvalidSignature = 1002,
}

#[derive(BorshDeserialize, BorshSerialize)]
struct State {
    processor: Secp256k1Pubkey,
    executors: Vec<Secp256k1Pubkey>,
    needed_executors: u8,
    next_to_kolme_id: u32,
    next_from_kolme_id: u32,
}

#[derive(BorshDeserialize, BorshSerialize)]
struct TokenHolder;

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let Some((instruction, instruction_data)) = instruction_data.split_first() else {
        return Err(ProgramError::InvalidInstructionData);
    };

    let ctx = Context::new(program_id, accounts);

    match *instruction {
        INITIALIZE_IX => {
            msg!("Instruction: Initialize");
            initialize(ctx, instruction_data)?;
        }
        REGULAR_IX => {
            msg!("Instruction: Regular message");
            regular(ctx, instruction_data)?;
        }
        SIGNED_IX => {
            msg!("Instruction: Signed message");
            signed(ctx, instruction_data)?;
        }
        _ => {
            msg!("Error: unknown instruction")
        }
    }

    Ok(())
}

fn initialize(ctx: Context, instruction_data: &[u8]) -> Result<(), ProgramError> {
    ctx.assert_accounts_len(3)?;
    let payer_acc = ctx.account(0, AccountReq::SIGNER)?;
    ctx.assert_is_system_program(1)?;
    let state_acc = ctx.account(2, AccountReq::WRITABLE)?;

    let ix = InitializeIxData::try_from_slice(instruction_data).map_err(|_| ProgramError::BorshIoError)?;
    let state = State {
        processor: ix.processor,
        executors: ix.executors,
        needed_executors: ix.needed_executors,
        next_to_kolme_id: 0,
        next_from_kolme_id: 0,
    };

    if state.executors.is_empty() {
        return Err(InitIxError::NoExecutorsProvided.into());
    }

    let executors_len = u8::try_from(state.executors.len()).map_err(|_| InitIxError::TooManyExecutors)?;
    if executors_len < state.needed_executors {
        return Err(InitIxError::InsufficientExecutors.into());
    }

    let state_pda: StatePda = ctx.init_pda(&payer_acc, &state_acc, STATE_DERIVATION, state)?;
    state_pda.serialize_into(&state_acc)?;

    Ok(())
}

fn regular(ctx: Context, instruction_data: &[u8]) -> Result<(), ProgramError> {
    let data = RegularMsgIxData::try_from_slice(instruction_data).map_err(|_| ProgramError::BorshIoError)?;

    let sender_acc = if ctx.accounts.len() == 1 {
         ctx.account(0, AccountReq::SIGNER)?
    } else if ctx.accounts.len() < 7 {
        return Err(ProgramError::NotEnoughAccountKeys);
    } else {
        const ACCOUNTS_PER_TRANSFER: usize = 4;

        let sender_acc = ctx.account(0, AccountReq::SIGNER)?;
        ctx.assert_is_system_program(1)?;
        ctx.assert_is_token_program(2)?;

        let token_accounts_len = ctx.accounts.len() - 3;

        if token_accounts_len % ACCOUNTS_PER_TRANSFER != 0 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let mut index = 3;
        let num_transfers = token_accounts_len / ACCOUNTS_PER_TRANSFER;

        if data.transfer_amounts.len() != num_transfers {
            return Err(RegularIxError::TransferAmountsMismatch.into());
        }

        for i in 0..num_transfers {
            let mint_acc = ctx.account(index, AccountReq::empty())?;
            let sender_ata_acc = ctx.account(index + 1, AccountReq::WRITABLE)?;
            let holder_acc = ctx.account(index + 2, AccountReq::WRITABLE)?;
            let holder_ata_acc = ctx.account(index + 3, AccountReq::WRITABLE)?;

            // SAFETY: We are not keeping a reference or have called .assign() at all.
            unsafe {
                if mint_acc.owner() != &token::ID {
                    return Err(ProgramError::IllegalOwner);
                }
            }

            let sender_ata = token::state::TokenAccount::from_account_info(&sender_ata_acc)?;
            let holder_ata = token::state::TokenAccount::from_account_info(&holder_ata_acc)?;

            let mut was_init = false;
            let seeds = &[TOKEN_HOLDER_SEED, mint_acc.key().as_slice(), sender_acc.key().as_slice()];
            let holder: TokenHolderPda = ctx.load_or_init_pda(
                &sender_acc,
                &holder_acc,
                PdaDerivation::new(seeds),
                || {
                    was_init = true;
                    TokenHolder
                }
            )?;

            if was_init {
                holder.serialize_into(&holder_acc)?;
            }

            if sender_ata.mint() != mint_acc.key() || sender_ata.owner() != sender_acc.key() {
                return Err(RegularIxError::WrongATA.into());
            }

            if holder_ata.has_close_authority() || holder_ata.has_delegate() {
                return Err(RegularIxError::CannotHaveCloseAuthorityOrDelegate.into());
            }

            if holder_ata.mint() != mint_acc.key() || holder_ata.owner() != ctx.program_id {
                return Err(RegularIxError::ProgramIsNotOwner.into());
            }

            token::instructions::Transfer {
                from: &sender_ata_acc,
                to: &holder_ata_acc,
                authority: &sender_acc,
                amount: data.transfer_amounts[i]
            }.invoke()?;

            index += ACCOUNTS_PER_TRANSFER;
        }

        sender_acc
    };

    Ok(())
}

fn signed(ctx: Context, instruction_data: &[u8]) -> Result<(), ProgramError> {
    if ctx.accounts.len() < 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    let sender_acc = ctx.account(0, AccountReq::SIGNER)?;
    let state_acc = ctx.account(1, AccountReq::WRITABLE)?;

    let data = SignedMsgIxData::try_from_slice(instruction_data).map_err(|_| ProgramError::BorshIoError)?;
    let mut state_pda: StatePda = ctx.load_pda(&state_acc, STATE_DERIVATION.seeds)?;

    let payload = Payload::try_from_slice(&data.payload).map_err(|_| ProgramError::BorshIoError)?;

    if payload.id != state_pda.data.next_from_kolme_id {
        return Err(SignedIxError::IncorrectOutgoingId.into());
    }

    state_pda.data.next_from_kolme_id += 1;
    let incoming_id = state_pda.data.next_to_kolme_id;
    state_pda.data.next_to_kolme_id += 1;

    state_pda.serialize_into(&state_acc)?;

    let executors_len = u8::try_from(data.executors.len()).map_err(|_| SignedIxError::TooManyExecutors)?;
    if executors_len < state_pda.data.needed_executors {
        return Err(SignedIxError::InsufficientSignatures.into());
    }

    let hash = sha256(&data.payload);
    let recovered_key = secp256k1_recover(&hash, data.processor.recovery_id, &data.processor.signature)?;

    if recovered_key != state_pda.data.processor {
        return Err(SignedIxError::ProcessorKeyMismatch.into());
    }

    let mut keys = Vec::with_capacity(data.executors.len());

    for e in data.executors {
        let key = secp256k1_recover(&hash, e.recovery_id, &e.signature)?;

        if !state_pda.data.executors.contains(&key) {
            return Err(SignedIxError::NonExecutorKey.into());
        }

        keys.push(key);
    }

    for (i, key) in keys.iter().enumerate() {
        if keys.iter().skip(i + 1).any(|x| x == key) {
            return Err(SignedIxError::DuplicateExecutorKey.into());
        }
    }

    let ix_accounts = &ctx.accounts[2..];

    if payload.accounts.len() != ix_accounts.len() {
        return Err(SignedIxError::AccountMetaAndPassedAccountsMismatch.into());
    }

    let mut accounts = Vec::with_capacity(payload.accounts.len());

    for a in &payload.accounts {
        accounts.push(AccountMeta {
            pubkey: &a.pubkey,
            is_writable: a.is_writable,
            is_signer: sender_acc.key() == &a.pubkey,
        });
    }

    let ix = Instruction {
        program_id: &payload.program_id,
        data: &payload.instruction_data,
        accounts: &accounts
    };

    let ix_accounts: Vec<&AccountInfo> = ix_accounts.iter().map(|x| x).collect();
    cpi::slice_invoke(&ix, &ix_accounts)?;

    Ok(())
}

fn secp256k1_recover(
    hash: &Sha256,
    recovery_id: u8,
    signature: &[u8],
) -> Result<Secp256k1Pubkey, Secp256k1RecoverError> {
    let mut buf = std::mem::MaybeUninit::<[u8; SECP256K1_PUBLIC_KEY_LENGTH]>::uninit();

    unsafe {
        let errno = syscalls::sol_secp256k1_recover(
            hash.0.as_ptr(),
            recovery_id as u64,
            signature.as_ptr(),
            buf.as_mut_ptr() as *mut u8,
        );

        match errno {
            0 => Ok(Secp256k1Pubkey(buf.assume_init())),
            1 => Err(Secp256k1RecoverError::InvalidHash),
            2 => Err(Secp256k1RecoverError::InvalidRecoveryId),
            3 => Err(Secp256k1RecoverError::InvalidSignature),
            _ => panic!("Unsupported Secp256k1RecoverError"),
        }
    }
}

fn sha256(bytes: &[u8]) -> Sha256 {
    let mut result = [0; SHA256_DIGEST_SIZE];
    let entries = &[bytes];

    unsafe {
        syscalls::sol_sha256(
            entries as *const _ as *const u8,
            entries.len() as u64,
            result.as_mut_ptr(),
        );
    }

    Sha256(result)
}

impl From<InitIxError> for ProgramError {
    #[inline]
    fn from(error: InitIxError) -> Self {
        ProgramError::Custom(error as u32)
    }
}

impl From<RegularIxError> for ProgramError {
    #[inline]
    fn from(error: RegularIxError) -> Self {
        ProgramError::Custom(error as u32)
    }
}

impl From<SignedIxError> for ProgramError {
    #[inline]
    fn from(error: SignedIxError) -> Self {
        ProgramError::Custom(error as u32)
    }
}

impl From<Secp256k1RecoverError> for ProgramError {
    #[inline]
    fn from(error: Secp256k1RecoverError) -> Self {
        ProgramError::Custom(error as u32)
    }
}
