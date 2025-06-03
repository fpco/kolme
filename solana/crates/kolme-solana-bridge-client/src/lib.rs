pub use solana_hash as hash;
pub use solana_instruction as instruction;
pub use solana_keypair as keypair;
pub use solana_message as message;
pub use solana_pubkey as pubkey;
pub use solana_signer as signer;
pub use solana_transaction as transaction;
pub use spl_associated_token_account_client as spl_client;

use borsh::BorshSerialize;
use hash::Hash;
use instruction::{account_meta::AccountMeta, Instruction};
use keypair::Keypair;
use message::Message;
use pubkey::Pubkey;
use signer::Signer;
use transaction::Transaction;

use shared::solana::{
    ExecuteAction, InitializeIxData, InstructionAccount, Payload, RegularMsgIxData, SignedAction,
    SignedMsgIxData, SignerAccount, INITIALIZE_IX, REGULAR_IX, SIGNED_IX, TOKEN_HOLDER_SEED,
};

const SYSTEM: Pubkey = Pubkey::from_str_const("11111111111111111111111111111111");
const TOKEN: Pubkey = Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const TOKEN_2022: Pubkey = Pubkey::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

#[derive(Clone, Copy, PartialEq, Hash, Debug)]
pub enum TokenProgram {
    Legacy,
    Token2022,
}

pub fn init_tx(
    program_id: Pubkey,
    blockhash: Hash,
    sender: &Keypair,
    data: &InitializeIxData,
) -> borsh::io::Result<Transaction> {
    let msg = init_ix(program_id, &blockhash, sender.pubkey(), data)?;

    Ok(Transaction::new(&[sender], msg, blockhash))
}

pub fn init_ix(
    program_id: Pubkey,
    blockhash: &Hash,
    sender: Pubkey,
    data: &InitializeIxData,
) -> borsh::io::Result<Message> {
    let mut bytes = Vec::with_capacity(1 + borsh::object_length(data)?);
    bytes.push(INITIALIZE_IX);
    data.serialize(&mut bytes)?;

    let state_pda = derive_state_pda(&program_id);
    let accounts = vec![
        AccountMeta::new(sender, true),
        AccountMeta::new(SYSTEM, false),
        AccountMeta::new(state_pda, false),
    ];

    Ok(Message::new_with_blockhash(
        &[Instruction {
            program_id,
            accounts,
            data: bytes,
        }],
        Some(&sender),
        blockhash,
    ))
}

pub fn signed_tx(
    program_id: Pubkey,
    blockhash: Hash,
    sender: &Keypair,
    data: &SignedMsgIxData,
    additional: &[AccountMeta],
) -> borsh::io::Result<Transaction> {
    let msg = signed_ix(program_id, &blockhash, sender.pubkey(), data, additional)?;

    Ok(Transaction::new(&[sender], msg, blockhash))
}

pub fn signed_ix(
    program_id: Pubkey,
    blockhash: &Hash,
    sender: Pubkey,
    data: &SignedMsgIxData,
    additional: &[AccountMeta],
) -> borsh::io::Result<Message> {
    let mut bytes = Vec::with_capacity(1 + borsh::object_length(data)?);
    bytes.push(SIGNED_IX);
    data.serialize(&mut bytes)?;

    let state_pda = derive_state_pda(&program_id);
    let mut accounts = vec![
        AccountMeta::new(sender, true),
        AccountMeta::new(state_pda, false),
        AccountMeta::new(SYSTEM, false),
    ];

    accounts.extend_from_slice(additional);

    Ok(Message::new_with_blockhash(
        &[Instruction {
            program_id,
            accounts,
            data: bytes,
        }],
        Some(&sender),
        blockhash,
    ))
}

pub fn regular_tx(
    program_id: Pubkey,
    token_program: TokenProgram,
    blockhash: Hash,
    sender: &Keypair,
    data: &RegularMsgIxData,
    token_mints: &[Pubkey],
) -> borsh::io::Result<Transaction> {
    let msg = regular_ix(
        program_id,
        token_program,
        &blockhash,
        sender.pubkey(),
        data,
        token_mints,
    )?;

    Ok(Transaction::new(&[sender], msg, blockhash))
}

pub fn regular_ix(
    program_id: Pubkey,
    token_program: TokenProgram,
    blockhash: &Hash,
    sender: Pubkey,
    data: &RegularMsgIxData,
    token_mints: &[Pubkey],
) -> borsh::io::Result<Message> {
    let mut bytes = Vec::with_capacity(1 + borsh::object_length(data)?);
    bytes.push(REGULAR_IX);
    data.serialize(&mut bytes)?;

    let accounts = if token_mints.is_empty() {
        vec![
            AccountMeta::new(sender, true),
            AccountMeta::new(derive_state_pda(&program_id), false),
        ]
    } else {
        let token_program = token_program.program_id();

        let mut accounts = Vec::with_capacity(4 + token_mints.len() * 4);
        accounts.push(AccountMeta::new(sender, true));
        accounts.push(AccountMeta::new(derive_state_pda(&program_id), false));
        accounts.push(AccountMeta::new(SYSTEM, false));
        accounts.push(AccountMeta::new(TOKEN, false));

        for mint in token_mints {
            accounts.push(AccountMeta::new_readonly(*mint, false));

            let sender_ata = spl_client::address::get_associated_token_address_with_program_id(
                &sender,
                mint,
                &token_program,
            );
            accounts.push(AccountMeta::new(sender_ata, false));

            let holder = derive_token_holder_acc(&program_id, mint);
            accounts.push(AccountMeta::new(holder, false));

            let holder_ata = spl_client::address::get_associated_token_address_with_program_id(
                &holder,
                mint,
                &token_program,
            );
            accounts.push(AccountMeta::new(holder_ata, false));
        }

        accounts
    };

    Ok(Message::new_with_blockhash(
        &[Instruction {
            program_id,
            accounts,
            data: bytes,
        }],
        Some(&sender),
        blockhash,
    ))
}

pub fn derive_state_pda(program_id: &Pubkey) -> Pubkey {
    let (pubkey, _) = Pubkey::find_program_address(&[b"state"], program_id);

    pubkey
}

pub fn derive_token_holder_acc(program_id: &Pubkey, mint: &Pubkey) -> Pubkey {
    let seeds = token_holder_seeds(mint);
    let (holder, _) = Pubkey::find_program_address(&seeds, program_id);

    holder
}

pub fn transfer_payload(
    id: u64,
    program_id: Pubkey,
    token_program: TokenProgram,
    mint: Pubkey,
    to: Pubkey,
    amount: u64,
) -> Payload {
    let holder_seeds = token_holder_seeds(&mint);

    let (authority, bump) = Pubkey::find_program_address(&holder_seeds, &program_id);
    let seeds = vec![
        Vec::from(holder_seeds[0]),
        Vec::from(holder_seeds[1]),
        Vec::from(&[bump]),
    ];

    let token_program = token_program.program_id();
    let from_ata = spl_client::address::get_associated_token_address_with_program_id(
        &authority,
        &mint,
        &token_program,
    );
    let to_ata = spl_client::address::get_associated_token_address_with_program_id(
        &to,
        &mint,
        &token_program,
    );

    let accounts = vec![
        InstructionAccount {
            pubkey: from_ata.to_bytes(),
            is_writable: true,
        },
        InstructionAccount {
            pubkey: to_ata.to_bytes(),
            is_writable: true,
        },
        InstructionAccount {
            pubkey: authority.to_bytes(),
            is_writable: false,
        },
    ];

    let bytes = amount.to_le_bytes();
    let instruction_data = vec![
        3, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ];

    Payload {
        id,
        action: SignedAction::Execute(ExecuteAction {
            program_id: TOKEN.to_bytes(),
            accounts,
            instruction_data,
            signer: Some(SignerAccount {
                index: 2, // authority must be the signer
                seeds,
            }),
        }),
    }
}

fn token_holder_seeds(mint: &Pubkey) -> [&[u8]; 2] {
    [TOKEN_HOLDER_SEED, mint.as_array().as_slice()]
}

impl TokenProgram {
    #[inline]
    pub fn program_id(&self) -> Pubkey {
        match self {
            Self::Legacy => TOKEN,
            Self::Token2022 => TOKEN_2022,
        }
    }
}
