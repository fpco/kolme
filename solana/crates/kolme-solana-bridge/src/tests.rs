use litesvm::{LiteSVM, types::TransactionResult};
use litesvm_token::{
    spl_token::state::Account as SplAccount,
    CreateMint, MintTo, CreateAssociatedTokenAccount,
    get_spl_account
};
use solana_instruction::{account_meta::AccountMeta, error::InstructionError, Instruction};
use solana_transaction_error::TransactionError;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;
use spl_associated_token_account_client as spl_client;
use borsh::BorshSerialize;
use k256::{ecdsa, elliptic_curve::rand_core::OsRng};
use sha_256::Sha256;

use crate::{
    InitializeIxData, RegularMsgIxData, SignedMsgIxData, Payload,
    Secp256k1Pubkey, Secp256k1Signature, Signature, InstructionAccount,
    SignedIxError, INITIALIZE_IX, REGULAR_IX, SIGNED_IX, TOKEN_HOLDER_SEED
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
    keys: Box<[ecdsa::SigningKey; KEYS_LEN]>,
    token: Pubkey,
    token_owner: Keypair,
}

impl Program {
    fn new() -> Self {
        let mut svm = LiteSVM::new().with_spl_programs();

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

        let token_owner = Keypair::new();
        svm.airdrop(&token_owner.pubkey(), 1000000000).unwrap();

        let token = CreateMint::new(&mut svm, &token_owner).send().unwrap();

        Self { svm, keys, token, token_owner }
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

    fn regular(&mut self, sender: &Keypair, data: &RegularMsgIxData, token_mints: &[Pubkey]) -> TransactionResult {
        assert_eq!(data.transfer_amounts.len(), token_mints.len());

        let mut bytes = Vec::with_capacity(1 + borsh::object_length(data).unwrap());
        bytes.push(REGULAR_IX);
        data.serialize(&mut bytes).unwrap();

        let accounts = if token_mints.len() == 0 {
            vec![AccountMeta::new(sender.pubkey(), true)]
        } else {
            let mut accounts = Vec::with_capacity(4 + token_mints.len() * 4);
            accounts.push(AccountMeta::new(sender.pubkey(), true));
            accounts.push(AccountMeta::new(STATE_PDA, false));
            accounts.push(AccountMeta::new(SYSTEM, false));
            accounts.push(AccountMeta::new(TOKEN, false));

            let sender_pk = sender.pubkey();

            for mint in token_mints {
                accounts.push(AccountMeta::new_readonly(*mint, false));

                let sender_ata = spl_client::address::get_associated_token_address(&sender_pk, mint);
                accounts.push(AccountMeta::new(sender_ata, false));

                let holder = token_holder_acc(mint, &sender_pk);
                accounts.push(AccountMeta::new(holder, false));

                let holder_ata = spl_client::address::get_associated_token_address(&holder, mint);
                accounts.push(AccountMeta::new(holder_ata, false));

                if self.svm.get_account(&holder).is_none() {
                    CreateAssociatedTokenAccount::new(&mut self.svm, sender, mint).owner(&holder).send().unwrap();
                }
            }

            accounts
        };

        let tx = self.make_tx(sender, accounts, bytes);
        let res = self.svm.send_transaction(tx);

        if res.is_ok() {
            self.svm.expire_blockhash();
        }

        res
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
        let res = self.svm.send_transaction(tx);

        if res.is_ok() {
            self.svm.expire_blockhash();
        }

        res
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

        let (sig, rec) = self.keys[PROCESSOR_KEY].sign_prehash_recoverable(&hash).unwrap();
        let processor = Signature {
            signature: Secp256k1Signature(sig.to_bytes().into()),
            recovery_id: rec.to_byte()
        };

        let mut executors = Vec::with_capacity(executor_indices.len());

        for i in executor_indices {
            let i = *i;
            assert_ne!(i, PROCESSOR_KEY);

            let (sig, rec) = self.keys[i].sign_prehash_recoverable(&hash).unwrap();
            let signature = Signature {
                signature: Secp256k1Signature(sig.to_bytes().into()),
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

    fn mint(&mut self, to: &Pubkey, amount: u64) {
        MintTo::new(&mut self.svm, &self.token_owner, &self.token, to, amount).send().unwrap()
    }

    fn make_ata(&mut self, acc: &Keypair) -> Pubkey {
        CreateAssociatedTokenAccount::new(&mut self.svm, acc, &self.token).send().unwrap()
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
    let instruction_data = vec![3, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]];

    let payload = Payload {
        id,
        program_id: TOKEN.to_bytes(),
        accounts,
        instruction_data
    };

    (payload, account_metas)
}

fn token_holder_acc(mint: &Pubkey, sender: &Pubkey) -> Pubkey {
    let seeds = &[TOKEN_HOLDER_SEED, mint.as_array().as_slice(), sender.as_array().as_slice()];
    let (holder, _) = Pubkey::find_program_address(seeds, &crate::ID.into());

    holder
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
fn sending_funds_works() {
    let mut p = Program::new();
    let sender = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();
    p.init_default(&sender).unwrap();

    let sender_ata_acc = p.make_ata(&sender);
    let holder_acc = token_holder_acc(&p.token, &sender.pubkey());
    let holder_ata_acc = spl_client::address::get_associated_token_address(&holder_acc, &p.token);

    let mint_amount = 10_00000000; // 10
    let transfer_amount = mint_amount / 2; // 5

    p.mint(&sender_ata_acc, mint_amount);

    let sender_ata: SplAccount = get_spl_account(&p.svm, &sender_ata_acc).unwrap();
    assert_eq!(sender_ata.amount, mint_amount);

    let data = RegularMsgIxData {
        keys: vec![],
        transfer_amounts: vec![transfer_amount]
    };

    p.regular(&sender, &data, &[p.token]).unwrap();

    let holder_ata: SplAccount = get_spl_account(&p.svm, &holder_ata_acc).unwrap();
    assert_eq!(holder_ata.mint, p.token);
    assert_eq!(holder_ata.owner, holder_acc);
    assert_eq!(holder_ata.amount, transfer_amount);

    let sender_ata: SplAccount = get_spl_account(&p.svm, &sender_ata_acc).unwrap();
    assert_eq!(sender_ata.amount, transfer_amount);

    p.regular(&sender, &data, &[p.token]).unwrap();

    let holder_ata: SplAccount = get_spl_account(&p.svm, &holder_ata_acc).unwrap();
    assert_eq!(holder_ata.amount, mint_amount);

    let sender_ata: SplAccount = get_spl_account(&p.svm, &sender_ata_acc).unwrap();
    assert_eq!(sender_ata.amount, 0);
}

#[test]
fn signing_multiple_tokens_work() {
    let mut p = Program::new();
    let sender = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();
    p.init_default(&sender).unwrap();

    let token_owner = Keypair::new();
    p.svm.airdrop(&token_owner.pubkey(), 1000000000).unwrap();

    let token_2 = CreateMint::new(&mut p.svm, &token_owner).send().unwrap();

    let mint_amount = 10_00000000; // 10

    let sender_ata_acc_1 = CreateAssociatedTokenAccount::new(&mut p.svm, &sender, &p.token).send().unwrap();
    let sender_ata_acc_2 = CreateAssociatedTokenAccount::new(&mut p.svm, &sender, &token_2).send().unwrap();

    MintTo::new(&mut p.svm, &p.token_owner, &p.token, &sender_ata_acc_1, mint_amount).send().unwrap();
    MintTo::new(&mut p.svm, &token_owner, &token_2, &sender_ata_acc_2, mint_amount).send().unwrap();

    let transfer_amounts = &[mint_amount, mint_amount / 2];
    let data = RegularMsgIxData {
        keys: vec![],
        transfer_amounts: transfer_amounts.into()
    };

    p.regular(&sender, &data, &[p.token, token_2]).unwrap();

    for (i, token) in [p.token, token_2].iter().enumerate() {
        let holder_acc = token_holder_acc(token, &sender.pubkey());
        let holder_ata_acc = spl_client::address::get_associated_token_address(&holder_acc, token);

        let holder_ata: SplAccount = get_spl_account(&p.svm, &holder_ata_acc).unwrap();
        assert_eq!(&holder_ata.mint, token);
        assert_eq!(holder_ata.owner, holder_acc);
        assert_eq!(holder_ata.amount, transfer_amounts[i]);
    }

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

    let meta = p.signed(&sender, &data, &metas).unwrap_err();

    // This error occurs on the SPL program invocation because we didn't provide
    // the relevant accounts to it which means that signature checks have passed.
    assert_eq!(meta.err, TransactionError::InstructionError(0, InstructionError::MissingAccount));
}

#[test]
fn insufficient_signatures_are_rejected() {
    let mut p = Program::new();
    let sender = Keypair::new();
    let receiver = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();

    p.init_default(&sender).unwrap();

    let (payload, metas) = transfer_payload(0, sender.pubkey(), receiver.pubkey(), sender.pubkey(), 1000);
    let data = p.make_signed_msg(&payload, &[EXECUTOR2_KEY]);

    let meta = p.signed(&sender, &data, &metas).unwrap_err();
    assert_eq!(meta.err, TransactionError::InstructionError(0, InstructionError::Custom(SignedIxError::InsufficientSignatures as u32)));
}

#[test]
fn false_processor_signature_is_rejected() {
    let mut p = Program::new();
    let sender = Keypair::new();
    let receiver = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();

    p.init_default(&sender).unwrap();

    let (payload, metas) = transfer_payload(0, sender.pubkey(), receiver.pubkey(), sender.pubkey(), 1000);
    let mut data = p.make_signed_msg(&payload, &[EXECUTOR1_KEY, EXECUTOR3_KEY]);

    let mut rng = OsRng;
    let fake_key = ecdsa::SigningKey::random(&mut rng);

    let mut bytes = Vec::with_capacity(borsh::object_length(&payload).unwrap());
    payload.serialize(&mut bytes).unwrap();

    let mut sha256 = Sha256::new();
    let hash = sha256.digest(&bytes);

    let (sig, rec) = fake_key.sign_prehash_recoverable(&hash).unwrap();
    data.processor = Signature {
        signature: Secp256k1Signature(sig.to_bytes().into()),
        recovery_id: rec.to_byte()
    };

    let meta = p.signed(&sender, &data, &metas).unwrap_err();
    assert_eq!(meta.err, TransactionError::InstructionError(0, InstructionError::Custom(SignedIxError::ProcessorKeyMismatch as u32)));

    let (payload, metas) = transfer_payload(0, sender.pubkey(), receiver.pubkey(), sender.pubkey(), 1000);
    let mut data = p.make_signed_msg(&payload, &[EXECUTOR1_KEY, EXECUTOR3_KEY]);

    let (other_payload, _) = transfer_payload(1, sender.pubkey(), receiver.pubkey(), sender.pubkey(), 1000);
    let other_data = p.make_signed_msg(&other_payload, &[EXECUTOR1_KEY, EXECUTOR3_KEY]);

    data.processor = other_data.processor;

    let meta = p.signed(&sender, &data, &metas).unwrap_err();
    assert_eq!(meta.err, TransactionError::InstructionError(0, InstructionError::Custom(SignedIxError::ProcessorKeyMismatch as u32)));
}

#[test]
fn false_executor_signature_is_rejected() {
    let mut p = Program::new();
    let sender = Keypair::new();
    let receiver = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();

    p.init_default(&sender).unwrap();

    let (payload, metas) = transfer_payload(0, sender.pubkey(), receiver.pubkey(), sender.pubkey(), 1000);
    let mut data = p.make_signed_msg(&payload, &[EXECUTOR1_KEY, EXECUTOR4_KEY]);

    let mut rng = OsRng;
    let fake_key = ecdsa::SigningKey::random(&mut rng);

    let mut bytes = Vec::with_capacity(borsh::object_length(&payload).unwrap());
    payload.serialize(&mut bytes).unwrap();

    let mut sha256 = Sha256::new();
    let hash = sha256.digest(&bytes);

    let (sig, rec) = fake_key.sign_prehash_recoverable(&hash).unwrap();
    data.executors[1] = Signature {
        signature: Secp256k1Signature(sig.to_bytes().into()),
        recovery_id: rec.to_byte()
    };

    let meta = p.signed(&sender, &data, &metas).unwrap_err();
    assert_eq!(meta.err, TransactionError::InstructionError(0, InstructionError::Custom(SignedIxError::NonExecutorKey as u32)));

    let (payload, metas) = transfer_payload(0, sender.pubkey(), receiver.pubkey(), sender.pubkey(), 1000);
    let mut data = p.make_signed_msg(&payload, &[EXECUTOR1_KEY, EXECUTOR4_KEY]);

    let (other_payload, _) = transfer_payload(1, sender.pubkey(), receiver.pubkey(), sender.pubkey(), 1000);
    let mut other_data = p.make_signed_msg(&other_payload, &[EXECUTOR1_KEY, EXECUTOR4_KEY]);

    data.executors[1] = other_data.executors.pop().unwrap();

    let meta = p.signed(&sender, &data, &metas).unwrap_err();
    assert_eq!(meta.err, TransactionError::InstructionError(0, InstructionError::Custom(SignedIxError::NonExecutorKey as u32)));
}
