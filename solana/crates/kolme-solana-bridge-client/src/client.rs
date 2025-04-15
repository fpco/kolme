use borsh::BorshSerialize;
use solana_hash::Hash;
use solana_instruction::{account_meta::AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;
use spl_associated_token_account_client as spl_client;

use crate::{
    InitializeIxData, InstructionAccount, Payload, SignedMsgIxData, SignerAccount, INITIALIZE_IX,
    SIGNED_IX, TOKEN_HOLDER_SEED,
};

const SYSTEM: Pubkey = Pubkey::from_str_const("11111111111111111111111111111111");
const TOKEN: Pubkey = Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
// const TOKEN_2022: Pubkey = Pubkey::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

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
    data.serialize(&mut bytes).unwrap();

    let state_pda = derive_state_pda(&program_id);
    let mut accounts = vec![
        AccountMeta::new(sender, true),
        AccountMeta::new(state_pda, false),
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

pub fn derive_state_pda(program_id: &Pubkey) -> Pubkey {
    let (pubkey, _) = Pubkey::find_program_address(&[b"state"], program_id);

    pubkey
}

pub fn derive_token_holder_acc(program_id: &Pubkey, mint: &Pubkey, user: &Pubkey) -> Pubkey {
    let seeds = token_holder_seeds(mint, user);
    let (holder, _) = Pubkey::find_program_address(&seeds, program_id);

    holder
}

pub fn transfer_payload(
    id: u64,
    program_id: Pubkey,
    mint: Pubkey,
    to: Pubkey,
    amount: u64,
) -> Payload {
    let holder_seeds = token_holder_seeds(&mint, &to);

    let (authority, bump) = Pubkey::find_program_address(&holder_seeds, &program_id);
    let seeds = vec![
        Vec::from(holder_seeds[0]),
        Vec::from(holder_seeds[1]),
        Vec::from(holder_seeds[2]),
        Vec::from(&[bump]),
    ];

    let from_ata = spl_client::address::get_associated_token_address(&authority, &mint);
    let to_ata = spl_client::address::get_associated_token_address(&to, &mint);

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
        program_id: TOKEN.to_bytes(),
        accounts,
        instruction_data,
        signer: Some(SignerAccount {
            index: 2, // authority must be the signer
            seeds,
        }),
    }
}

fn token_holder_seeds<'a>(mint: &'a Pubkey, user: &'a Pubkey) -> [&'a [u8]; 3] {
    [
        TOKEN_HOLDER_SEED,
        mint.as_array().as_slice(),
        user.as_array().as_slice(),
    ]
}
