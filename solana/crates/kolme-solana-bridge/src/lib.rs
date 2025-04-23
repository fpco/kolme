use borsh::{BorshDeserialize, BorshSerialize};
use kolme_solana_bridge_client::{
    InitializeIxData,  RegularMsgIxData, SignedMsgIxData,
    State, Token, Message, BridgeMessage, Secp256k1Pubkey,
    Secp256k1Signature, Payload, INITIALIZE_IX, REGULAR_IX,
    SIGNED_IX, TOKEN_HOLDER_SEED
};
use solbox::{
    token::{self, state::{TokenAccount, Mint}},
    token2022,
    pubkey::declare_id,
    pinocchio::{
        account_info::{AccountInfo, Ref},
        instruction::{AccountMeta, Instruction, Seed, Signer},
        pubkey::{Pubkey, find_program_address},
        program_error::ProgramError,
        ProgramResult,
        syscalls,
        cpi,
    },
    AccountReq,
    Context,
    PdaData,
    PdaDerivation,
    log, log_base64
};

#[cfg(test)]
mod tests;

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

mod ata_program {
    use super::declare_id;

    declare_id!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
}

#[cfg(not(feature = "no-entrypoint"))]
use solbox::pinocchio::entrypoint;

#[cfg(not(feature = "no-entrypoint"))]
entrypoint!(process_instruction);

// PDAs
const STATE_DERIVATION: PdaDerivation = PdaDerivation::with_bump(&[b"state"], 254);
type StatePda = PdaData<State, 0>;

type TokenHolderPda = PdaData<TokenHolder, 1>;

const SHA256_DIGEST_SIZE: usize = 32;

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Sha256([u8; SHA256_DIGEST_SIZE]);

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Secp256k1RecoverError {
    InvalidHash = 1000,
    InvalidRecoveryId  = 1001,
    InvalidSignature = 1002,
    SignatureNotNormalized = 1003,
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
            log!("Instruction: Initialize");
            initialize(ctx, instruction_data)?;
        }
        REGULAR_IX => {
            log!("Instruction: Regular message");
            regular(ctx, instruction_data)?;
        }
        SIGNED_IX => {
            log!("Instruction: Signed message");
            signed(ctx, instruction_data)?;
        }
        _ => {
            log!("Error: unknown instruction")
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
        // We start events at ID 1, since the instantiation itself is event 0.
        next_event_id: 1,
        next_action_id: 0,
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

    let (sender_acc, state_acc, funds) = if ctx.accounts.len() == 2 {
        let sender_acc = ctx.account(0, AccountReq::SIGNER)?;
        let state_acc = ctx.account(1, AccountReq::WRITABLE)?;

        (sender_acc, state_acc, vec![])
    } else if ctx.accounts.len() < 8 {
        return Err(ProgramError::NotEnoughAccountKeys);
    } else {
        const ACCOUNTS_PER_TRANSFER: usize = 4;

        let sender_acc = ctx.account(0, AccountReq::SIGNER)?;
        let state_acc = ctx.account(1, AccountReq::WRITABLE)?;
        ctx.assert_is_system_program(2)?;
        ctx.assert_is_token_program(3)?;

        let token_accounts_len = ctx.accounts.len() - 4;

        if token_accounts_len % ACCOUNTS_PER_TRANSFER != 0 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let mut index = 4;
        let num_transfers = token_accounts_len / ACCOUNTS_PER_TRANSFER;

        if data.transfer_amounts.len() != num_transfers {
            return Err(RegularIxError::TransferAmountsMismatch.into());
        }

        let mut funds = Vec::with_capacity(num_transfers);

        for i in 0..num_transfers {
            let mint_acc = ctx.account(index, AccountReq::empty())?;
            let sender_ata_acc = ctx.account(index + 1, AccountReq::WRITABLE)?;
            let holder_acc = ctx.account(index + 2, AccountReq::WRITABLE)?;
            let holder_ata_acc = ctx.account(index + 3, AccountReq::WRITABLE)?;

            // SAFETY: The owner of this cannot be changed here.
            let token_program = unsafe { *mint_acc.owner() };

            if token_program != token::ID && token_program != token2022::ID {
                log!("Mint is not owned by a token program.");

                return Err(ProgramError::IllegalOwner);
            }

            let (holder_ata_acc_address, _) = find_program_address(
                &[
                    &holder_acc.key().as_slice(),
                    &token_program.as_slice(),
                    &mint_acc.key().as_slice(),
                ],
                &ata_program::ID
            );

            if holder_ata_acc.key() != &holder_ata_acc_address {
                return Err(ProgramError::InvalidArgument);
            }

            // SAFETY: We are not keeping a reference or have called .assign() at all.
            unsafe {
                if holder_ata_acc.owner() != mint_acc.owner() {
                    log!("Holder ATA not created or illegal account.");

                    return Err(ProgramError::IllegalOwner);
                }

                if sender_ata_acc.owner() != mint_acc.owner() {
                    log!("Sender ATA not created or illegal account.");

                    return Err(ProgramError::IllegalOwner);
                }
            }

            {
                let sender_ata = ata_data(&sender_ata_acc)?;

                if sender_ata.mint() != mint_acc.key() || sender_ata.owner() != sender_acc.key() {
                    return Err(RegularIxError::WrongATA.into());
                }
            }

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

            {
                let holder_ata = ata_data(&holder_ata_acc)?;

                if holder_ata.has_close_authority() || holder_ata.has_delegate() {
                    return Err(RegularIxError::CannotHaveCloseAuthorityOrDelegate.into());
                }

                if holder_ata.mint() != mint_acc.key() || holder_ata.owner() != holder_acc.key() {
                    return Err(RegularIxError::ProgramIsNotOwner.into());
                }
            }

            let amount = data.transfer_amounts[i];
            funds.push(Token {
                mint: *mint_acc.key(),
                amount
            });

            let decimals = mint_data(&mint_acc)?.decimals();
            let transfer = token::instructions::TransferChecked {
                from: &sender_ata_acc,
                mint: &mint_acc,
                to: &holder_ata_acc,
                authority: &sender_acc,
                amount,
                decimals
            };

            execute_transfer(transfer, &token_program)?;

            index += ACCOUNTS_PER_TRANSFER;
        }

        (sender_acc, state_acc, funds)
    };

    let mut state_pda: StatePda = ctx.load_pda(&state_acc, STATE_DERIVATION.seeds)?;
    let id = state_pda.data.next_event_id;
    state_pda.data.next_event_id += 1;

    state_pda.serialize_into(&state_acc)?;

    let msg = BridgeMessage {
        id,
        wallet: *sender_acc.key(),
        ty: Message::Regular {
            keys: data.keys,
            funds,
        }
    };

    let mut bytes = Vec::with_capacity(borsh::object_length(&msg).map_err(|_| ProgramError::BorshIoError)?);
    msg.serialize(&mut bytes).map_err(|_| ProgramError::BorshIoError)?;

    log_base64(&[bytes.as_slice()]);

    Ok(())
}

fn signed(ctx: Context, instruction_data: &[u8]) -> Result<(), ProgramError> {
    if ctx.accounts.len() < 3 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    let sender_acc = ctx.account(0, AccountReq::SIGNER)?;
    let state_acc = ctx.account(1, AccountReq::WRITABLE)?;

    let data = SignedMsgIxData::try_from_slice(instruction_data).map_err(|_| ProgramError::BorshIoError)?;
    let mut state_pda: StatePda = ctx.load_pda(&state_acc, STATE_DERIVATION.seeds)?;

    let payload = Payload::try_from_slice(&data.payload).map_err(|_| ProgramError::BorshIoError)?;

    if payload.id != state_pda.data.next_action_id {
        return Err(SignedIxError::IncorrectOutgoingId.into());
    }

    state_pda.data.next_action_id += 1;
    let incoming_id = state_pda.data.next_event_id;
    state_pda.data.next_event_id += 1;

    state_pda.serialize_into(&state_acc)?;

    let executors_len = u8::try_from(data.executors.len()).map_err(|_| SignedIxError::TooManyExecutors)?;
    if executors_len < state_pda.data.needed_executors {
        return Err(SignedIxError::InsufficientSignatures.into());
    }

    let hash = sha256(&data.payload);
    let recovered_key = secp256k1_recover(&hash, data.processor.recovery_id, &data.processor.signature)?;

    if recovered_key.to_sec1_bytes() != state_pda.data.processor {
        return Err(SignedIxError::ProcessorKeyMismatch.into());
    }

    let mut keys = Vec::with_capacity(data.executors.len());

    for e in data.executors {
        let key = secp256k1_recover(&hash, e.recovery_id, &e.signature)?.to_sec1_bytes();

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

    let ix_accounts = &ctx.accounts[3..];

    if payload.accounts.len() != ix_accounts.len() {
        return Err(SignedIxError::AccountMetaAndPassedAccountsMismatch.into());
    }

    let mut accounts = Vec::with_capacity(payload.accounts.len());

    for (i, a) in payload.accounts.iter().enumerate() {
        accounts.push(AccountMeta {
            pubkey: &a.pubkey,
            is_writable: a.is_writable,
            is_signer: payload.signer.as_ref().map_or(false, |x| usize::from(x.index) == i)
        });
    }

    let ix = Instruction {
        program_id: &payload.program_id,
        data: &payload.instruction_data,
        accounts: &accounts
    };

    let ix_accounts: Vec<&AccountInfo> = ix_accounts.iter().map(|x| x).collect();

    if let Some(signer) = payload.signer {
        let seeds: Vec<Seed> = signer.seeds.iter().map(|x| Seed::from(x.as_slice())).collect();
        let signer = Signer::from(seeds.as_slice());

        cpi::slice_invoke_signed(&ix, &ix_accounts, &[signer])?;
    } else {
        cpi::slice_invoke(&ix, &ix_accounts)?;
    }

    let msg = BridgeMessage {
        id: incoming_id,
        wallet: *sender_acc.key(),
        ty: Message::Signed {
            action_id: payload.id
        }
    };

    let mut bytes = Vec::with_capacity(borsh::object_length(&msg).map_err(|_| ProgramError::BorshIoError)?);
    msg.serialize(&mut bytes).map_err(|_| ProgramError::BorshIoError)?;

    log_base64(&[bytes.as_slice()]);

    Ok(())
}

// A slightly modified version of TokenAccount::from_account_info to support Token 2022.
fn ata_data(account_info: &AccountInfo) -> Result<Ref<TokenAccount>, ProgramError> {
    if account_info.data_len() < TokenAccount::LEN {
        return Err(ProgramError::InvalidAccountData);
    }

    if !account_info.is_owned_by(&token::ID) &&
        !account_info.is_owned_by(&token2022::ID) {
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(Ref::map(account_info.try_borrow_data()?, |data| unsafe {
        TokenAccount::from_bytes(&data[..TokenAccount::LEN])
    }))
}

pub fn mint_data(account_info: &AccountInfo) -> Result<Ref<Mint>, ProgramError> {
    if account_info.data_len() != Mint::LEN {
        log!("Invalid mint data len.");

        return Err(ProgramError::InvalidAccountData);
    }

    if !account_info.is_owned_by(&token::ID) &&
        !account_info.is_owned_by(&token2022::ID) {
        log!("Invalid mint data");
        return Err(ProgramError::InvalidAccountData);
    }

    Ok(Ref::map(account_info.try_borrow_data()?, |data| unsafe {
        Mint::from_bytes(data)
    }))

}

fn execute_transfer(transfer: token::instructions::TransferChecked<'_>, token_program: &Pubkey) -> ProgramResult {
    use std::mem::MaybeUninit;

    const UNINIT_BYTE: MaybeUninit<u8> = MaybeUninit::<u8>::uninit();

    #[inline(always)]
    fn write_bytes(destination: &mut [MaybeUninit<u8>], source: &[u8]) {
        for (d, s) in destination.iter_mut().zip(source.iter()) {
            d.write(*s);
        }
    }

    let account_metas: [AccountMeta; 4] = [
        AccountMeta::writable(transfer.from.key()),
        AccountMeta::readonly(transfer.mint.key()),
        AccountMeta::writable(transfer.to.key()),
        AccountMeta::readonly_signer(transfer.authority.key()),
    ];

    // Instruction data layout:
    // -  [0]: instruction discriminator (1 byte, u8)
    // -  [1..9]: amount (8 bytes, u64)
    // -  [9]: decimals (1 byte, u8)
    let mut instruction_data = [UNINIT_BYTE; 10];

    // Set discriminator as u8 at offset [0]
    write_bytes(&mut instruction_data, &[12]);
    // Set amount as u64 at offset [1..9]
    write_bytes(&mut instruction_data[1..9], &transfer.amount.to_le_bytes());
    // Set decimals as u8 at offset [9]
    write_bytes(&mut instruction_data[9..], &[transfer.decimals]);

    let instruction = Instruction {
        program_id: token_program,
        accounts: &account_metas,
        data: unsafe { std::slice::from_raw_parts(instruction_data.as_ptr() as _, 10) },
    };

    cpi::invoke(
        &instruction,
        &[transfer.from, transfer.mint, transfer.to, transfer.authority],
    )
}

fn secp256k1_recover(
    hash: &Sha256,
    recovery_id: u8,
    signature: &Secp256k1Signature,
) -> Result<Secp256k1Pubkey, Secp256k1RecoverError> {
    if !signature.is_normalized() {
        return Err(Secp256k1RecoverError::SignatureNotNormalized);
    }

    let mut buf = std::mem::MaybeUninit::<[u8; Secp256k1Pubkey::LEN]>::uninit();

    unsafe {
        let errno = syscalls::sol_secp256k1_recover(
            hash.0.as_ptr(),
            recovery_id as u64,
            signature.0.as_ptr(),
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
    let mut result = std::mem::MaybeUninit::<[u8; SHA256_DIGEST_SIZE]>::uninit();
    let entries = &[bytes];

    unsafe {
        syscalls::sol_sha256(
            entries as *const _ as *const u8,
            entries.len() as u64,
            result.as_mut_ptr() as *mut u8,
        );

        Sha256(result.assume_init())
    }
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
