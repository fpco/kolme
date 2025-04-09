use solana_pubkey::Pubkey;
use solana_message::Message;
use solana_instruction::{account_meta::AccountMeta, Instruction};
use solana_hash::Hash;
use borsh::BorshSerialize;

use crate::{InitializeIxData, INITIALIZE_IX};

const SYSTEM: Pubkey = Pubkey::from_str_const("11111111111111111111111111111111");

pub fn init_ix(
    program_id: Pubkey,
    blockhash: &Hash,
    sender: Pubkey,
    data: &InitializeIxData
) -> borsh::io::Result<Message> {
    let mut bytes = Vec::with_capacity(1 + borsh::object_length(data)?);
    bytes.push(INITIALIZE_IX);
    data.serialize(&mut bytes)?;

    let state_pda = derive_state_pda(&program_id);
    let accounts = vec![
        AccountMeta::new(sender, true),
        AccountMeta::new(SYSTEM, false),
        AccountMeta::new(state_pda, false)
    ];

    Ok(Message::new_with_blockhash(
        &[Instruction {
            program_id,
            accounts,
            data: bytes
        }],
        Some(&sender),
        blockhash,
    ))
}

pub fn derive_state_pda(program_id: &Pubkey) -> Pubkey {
    let (pubkey, _) = Pubkey::find_program_address(&[b"state"], program_id);

    pubkey
}
