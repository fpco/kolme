use std::{fs, mem, path::PathBuf};

use litesvm::{LiteSVM, types::TransactionResult};
use solana_account::Account;
use solana_instruction::{account_meta::AccountMeta, error::InstructionError, Instruction};
use solana_transaction_error::TransactionError;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_pubkey::{pubkey, Pubkey};
use solana_signer::Signer;
use solana_transaction::Transaction;
use borsh::BorshSerialize;
use k256::{ecdsa, elliptic_curve::rand_core::OsRng};
use sha_256::Sha256;

use crate::{
    InitializeIxData, RegularMsgIxData, SignedMsgIxData, Payload,
    Secp256k1Pubkey, Signature, InstructionAccount, INITIALIZE_IX,
    REGULAR_IX, SIGNED_IX, TOKEN_HOLDER_SEED
};

const KEYS_LEN: usize = 5;
const PROCESSOR_KEY: usize = 0;
const EXECUTOR1_KEY: usize = 1;
const EXECUTOR2_KEY: usize = 2;
const EXECUTOR3_KEY: usize = 3;
const EXECUTOR4_KEY: usize = 4;

const SYSTEM: Pubkey = Pubkey::from_str_const("11111111111111111111111111111111");
const TOKEN: Pubkey = Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const STATE_PDA: Pubkey = Pubkey::from_str_const("846CuSccLQ6jQSsvwNCoBkvvGXPGGm7ivVmT3a3GVkk7");

struct Program {
    svm: LiteSVM,
    keys: Box<[ecdsa::SigningKey; KEYS_LEN]>
}

struct TokenSend {
    /// The token to send.
    mint: Pubkey,
    /// ATA of the sender. The owner of the token account must be the signer of the instruction.
    sender_ata: Pubkey,
    /// PDA that will be the owner of `holder_ata`.
    holder: Pubkey,
    /// ATA that belongs to the `holder` PDA.
    holder_ata: Pubkey,
}

impl Program {
    fn new() -> Self {
        let mut svm = LiteSVM::new();

        let program_id: Pubkey = crate::ID.into();
        svm.add_program_from_file(program_id, "../../target/deploy/kolme_solana_bridge.so").unwrap();

        let keys = unsafe {
            let mut keys = Box::<[ecdsa::SigningKey; KEYS_LEN]>::new_uninit();
            let mut rng = OsRng;

            for i in 0..KEYS_LEN {
                (&mut *keys.as_mut_ptr()).as_mut_slice()[i] = ecdsa::SigningKey::random(&mut rng);
            }

            keys.assume_init()
        };

        Self { svm, keys }
    }

    fn init_default(&mut self, sender: &Keypair) -> TransactionResult {
        let mut executors = Vec::with_capacity(KEYS_LEN - 1);
        for i in 1..KEYS_LEN {
            executors.push(Secp256k1Pubkey(self.keys[i].verifying_key().to_encoded_point(false).as_bytes()[1..].try_into().unwrap()));
        }

        let data = InitializeIxData {
            needed_executors: ((KEYS_LEN - 1) / 2) as u8,
            processor: Secp256k1Pubkey(self.keys[PROCESSOR_KEY].verifying_key().to_encoded_point(false).as_bytes()[1..].try_into().unwrap()),
            executors
        };

        self.init(sender, &data)
    }

    fn init(&mut self, sender: &Keypair, data: &InitializeIxData) -> TransactionResult {
        let mut bytes = Vec::with_capacity(1 + borsh::object_length(data).unwrap());
        bytes.push(INITIALIZE_IX);
        data.serialize(&mut bytes).unwrap();

        let accounts = vec![
            AccountMeta::new(sender.pubkey(), true),
            AccountMeta::new(SYSTEM, false),
            AccountMeta::new(STATE_PDA, false)
        ];
        let tx = self.make_tx(sender, accounts, bytes);

        self.svm.send_transaction(tx)
    }

    fn regular(&mut self, sender: &Keypair, data: &RegularMsgIxData, tokens: &[TokenSend]) -> TransactionResult {
        assert_eq!(data.transfer_amounts.len(), tokens.len());

        let mut bytes = Vec::with_capacity(1 + borsh::object_length(data).unwrap());
        bytes.push(REGULAR_IX);
        data.serialize(&mut bytes).unwrap();

        let accounts = if tokens.len() == 0 {
            vec![AccountMeta::new(sender.pubkey(), true)]
        } else {
            let mut accounts = Vec::with_capacity(3 + tokens.len() * 4);
            accounts.push(AccountMeta::new(sender.pubkey(), true));
            accounts.push(AccountMeta::new(SYSTEM, false));
            accounts.push(AccountMeta::new(TOKEN, false));

            for token in tokens {
                accounts.push(AccountMeta::new_readonly(token.mint, false));
                accounts.push(AccountMeta::new(token.sender_ata, false));

                let sender_pk = sender.pubkey();
                let seeds = &[TOKEN_HOLDER_SEED, token.mint.as_array().as_slice(), sender_pk.as_array().as_slice()];
                let (holder, _) = Pubkey::find_program_address(seeds, &crate::ID.into());
                accounts.push(AccountMeta::new(holder, false));

                accounts.push(AccountMeta::new(token.holder_ata, false));
            }

            accounts
        };

        let tx = self.make_tx(sender, accounts, bytes);

        self.svm.send_transaction(tx)
    }

    fn signed(&mut self, sender: &Keypair, data: &SignedMsgIxData, additional: &[AccountMeta]) -> TransactionResult {
        let mut bytes = Vec::with_capacity(1 + borsh::object_length(data).unwrap());
        bytes.push(SIGNED_IX);
        data.serialize(&mut bytes).unwrap();

        let mut accounts = vec![
            AccountMeta::new(sender.pubkey(), true),
            AccountMeta::new(STATE_PDA, false)
        ];

        accounts.extend_from_slice(additional);

        let tx = self.make_tx(sender, accounts, bytes);

        self.svm.send_transaction(tx)
    }

    fn make_tx(&self, sender: &Keypair, accounts: Vec<AccountMeta>, data: Vec<u8>) -> Transaction {
        let blockhash = self.svm.latest_blockhash();
        let msg = Message::new_with_blockhash(
            &[Instruction {
                program_id: crate::ID.into(),
                accounts,
                data
            }],
            Some(&sender.pubkey()),
            &blockhash,
        );

        Transaction::new(&[sender], msg, blockhash)
    }

    fn make_signed_msg(&self, payload: &Payload, executor_indices: &[usize]) -> SignedMsgIxData {
        let mut bytes = Vec::with_capacity(borsh::object_length(payload).unwrap());
        payload.serialize(&mut bytes).unwrap();

        let mut sha256 = Sha256::new();
        let hash = sha256.digest(&bytes);

        let (sig, rec) = self.keys[PROCESSOR_KEY].sign_recoverable(&hash).unwrap();
        let processor = Signature {
            signature: sig.to_vec(),
            recovery_id: rec.to_byte()
        };

        let mut executors = Vec::with_capacity(executor_indices.len());

        for i in executor_indices {
            let i = *i;
            assert_ne!(i, PROCESSOR_KEY);

            let (sig, rec) = self.keys[i].sign_recoverable(&hash).unwrap();
            let signature = Signature {
                signature: sig.to_vec(),
                recovery_id: rec.to_byte()
            };

            executors.push(signature);
        }

        SignedMsgIxData {
            processor,
            executors,
            payload: bytes
        }
    }
}

fn transfer_payload(id: u32, from: Pubkey, to: Pubkey, authority: Pubkey, amount: u64) -> (Payload, [AccountMeta; 3]) {
    let account_metas: [AccountMeta; 3] = [
        AccountMeta::new(from, false),
        AccountMeta::new(to, false),
        AccountMeta::new_readonly(authority, true),
    ];

    let accounts: Vec<InstructionAccount> = account_metas.iter()
        .map(|x| InstructionAccount {
            pubkey: x.pubkey.to_bytes(),
            is_writable: x.is_writable
        })
        .collect();

    let bytes = amount.to_le_bytes();
    let mut instruction_data = vec![3, bytes[0], bytes[1], bytes[2], bytes[3]];

    let payload = Payload {
        id,
        program_id: TOKEN.to_bytes(),
        accounts,
        instruction_data
    };

    (payload, account_metas)
}

#[test]
fn must_init_first() {
    let mut p = Program::new();
    let sender = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();

    let payload = Payload {
        id: 0,
        program_id: TOKEN.to_bytes(),
        accounts: vec![],
        instruction_data: vec![],
    };

    let data = p.make_signed_msg(&payload, &[EXECUTOR1_KEY, EXECUTOR3_KEY]);

    let meta = p.signed(&sender, &data, &[]).unwrap_err();
    assert_eq!(meta.err, TransactionError::InstructionError(0, InstructionError::UninitializedAccount));
}

#[test]
fn signing_works() {
    let mut p = Program::new();
    let sender = Keypair::new();
    let receiver = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();

    p.init_default(&sender).unwrap();

    let (payload, metas) = transfer_payload(0, sender.pubkey(), receiver.pubkey(), sender.pubkey(), 1000);
    let data = p.make_signed_msg(&payload, &[EXECUTOR1_KEY, EXECUTOR3_KEY]);

    let res = p.signed(&sender, &data, &metas);
    panic!("{:#?}", res);
}
