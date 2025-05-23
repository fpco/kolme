use std::collections::BTreeSet;

use base64::Engine;
use borsh::BorshSerialize;
use litesvm::{types::TransactionResult, LiteSVM};
use litesvm_token::{CreateAssociatedTokenAccount, CreateMint, MintTo};
use sha_256::Sha256;

use kolme_solana_bridge_client::{
    instruction::{account_meta::AccountMeta, Instruction},
    keypair::Keypair,
    message::Message,
    pubkey::{declare_id, Pubkey},
    signer::Signer,
    spl_client,
    transaction::Transaction,
    transfer_payload, TokenProgram,
};
use shared::{
    cryptography::{SecretKey, ThreadRng},
    solana::{
        InitializeIxData, Payload, RegularMsgIxData, SignedAction, SignedMsgIxData, INITIALIZE_IX,
        REGULAR_IX, SIGNED_IX, TOKEN_HOLDER_SEED,
    },
    types::ValidatorSet,
};

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

pub const KEYS_LEN: usize = 7;
pub const PROCESSOR_KEY: usize = 0;
pub const APPROVER1_KEY: usize = 1;
pub const APPROVER2_KEY: usize = 2;
pub const APPROVER3_KEY: usize = 3;
pub const APPROVER4_KEY: usize = 4;
pub const LISTENER1_KEY: usize = 5;
pub const LISTENER2_KEY: usize = 6;

pub const SYSTEM: Pubkey = Pubkey::from_str_const("11111111111111111111111111111111");
pub const TOKEN: Pubkey = Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const STATE_PDA: Pubkey =
    Pubkey::from_str_const("846CuSccLQ6jQSsvwNCoBkvvGXPGGm7ivVmT3a3GVkk7");

pub struct Program {
    pub svm: LiteSVM,
    pub keys: Box<[SecretKey; KEYS_LEN]>,
    pub token: Pubkey,
    pub token_owner: Keypair,
}

impl Program {
    pub fn new() -> Self {
        let mut svm = LiteSVM::new().with_spl_programs();

        let program_id: Pubkey = ID.into();
        svm.add_program_from_file(program_id, "../../target/deploy/kolme_solana_bridge.so")
            .unwrap();

        let keys = unsafe {
            let mut keys = Box::<[SecretKey; KEYS_LEN]>::new_uninit();
            let mut rng = ThreadRng::default();

            for i in 0..KEYS_LEN {
                (*keys.as_mut_ptr()).as_mut_slice()[i] = SecretKey::random(&mut rng);
            }

            keys.assume_init()
        };

        let token_owner = Keypair::new();
        svm.airdrop(&token_owner.pubkey(), 1000000000).unwrap();

        let token = CreateMint::new(&mut svm, &token_owner).send().unwrap();

        Self {
            svm,
            keys,
            token,
            token_owner,
        }
    }

    #[allow(clippy::result_large_err)]
    pub fn init_default(&mut self, sender: &Keypair) -> TransactionResult {
        let mut approvers = BTreeSet::new();
        let mut listeners = BTreeSet::new();

        for i in APPROVER1_KEY..=APPROVER4_KEY {
            approvers.insert(self.keys[i].public_key());
        }

        for i in LISTENER1_KEY..=LISTENER2_KEY {
            listeners.insert(self.keys[i].public_key());
        }

        let data = InitializeIxData {
            set: ValidatorSet {
                processor: self.keys[PROCESSOR_KEY].public_key(),
                approvers,
                listeners,
                needed_approvers: 2,
                needed_listeners: 1,
            },
        };

        self.init(sender, &data)
    }

    #[allow(clippy::result_large_err)]
    pub fn init(&mut self, sender: &Keypair, data: &InitializeIxData) -> TransactionResult {
        let mut bytes = Vec::with_capacity(1 + borsh::object_length(data).unwrap());
        bytes.push(INITIALIZE_IX);
        data.serialize(&mut bytes).unwrap();

        let accounts = vec![
            AccountMeta::new(sender.pubkey(), true),
            AccountMeta::new(SYSTEM, false),
            AccountMeta::new(STATE_PDA, false),
        ];
        let tx = self.make_tx(sender, accounts, bytes);

        self.svm.send_transaction(tx)
    }

    #[allow(clippy::result_large_err)]
    pub fn regular(
        &mut self,
        sender: &Keypair,
        data: &RegularMsgIxData,
        token_mints: &[Pubkey],
    ) -> TransactionResult {
        assert_eq!(data.transfer_amounts.len(), token_mints.len());

        let mut bytes = Vec::with_capacity(1 + borsh::object_length(data).unwrap());
        bytes.push(REGULAR_IX);
        data.serialize(&mut bytes).unwrap();

        let accounts = if token_mints.is_empty() {
            vec![
                AccountMeta::new(sender.pubkey(), true),
                AccountMeta::new(STATE_PDA, false),
            ]
        } else {
            let mut accounts = Vec::with_capacity(4 + token_mints.len() * 4);
            accounts.push(AccountMeta::new(sender.pubkey(), true));
            accounts.push(AccountMeta::new(STATE_PDA, false));
            accounts.push(AccountMeta::new(SYSTEM, false));
            accounts.push(AccountMeta::new(TOKEN, false));

            let sender_pk = sender.pubkey();

            for mint in token_mints {
                accounts.push(AccountMeta::new_readonly(*mint, false));

                let sender_ata =
                    spl_client::address::get_associated_token_address(&sender_pk, mint);
                accounts.push(AccountMeta::new(sender_ata, false));

                let holder = token_holder_acc(mint, &sender_pk);
                accounts.push(AccountMeta::new(holder, false));

                let holder_ata = spl_client::address::get_associated_token_address(&holder, mint);
                accounts.push(AccountMeta::new(holder_ata, false));

                if self.svm.get_account(&holder).is_none() {
                    CreateAssociatedTokenAccount::new(&mut self.svm, sender, mint)
                        .owner(&holder)
                        .send()
                        .unwrap();
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

    #[allow(clippy::result_large_err)]
    pub fn signed(
        &mut self,
        sender: &Keypair,
        data: &SignedMsgIxData,
        additional: &[AccountMeta],
    ) -> TransactionResult {
        let mut bytes = Vec::with_capacity(1 + borsh::object_length(data).unwrap());
        bytes.push(SIGNED_IX);
        data.serialize(&mut bytes).unwrap();

        let mut accounts = vec![
            AccountMeta::new(sender.pubkey(), true),
            AccountMeta::new(STATE_PDA, false),
            AccountMeta::new(SYSTEM, false),
        ];

        accounts.extend_from_slice(additional);

        let tx = self.make_tx(sender, accounts, bytes);
        let res = self.svm.send_transaction(tx);

        if res.is_ok() {
            self.svm.expire_blockhash();
        }

        res
    }

    pub fn make_tx(
        &self,
        sender: &Keypair,
        accounts: Vec<AccountMeta>,
        data: Vec<u8>,
    ) -> Transaction {
        let blockhash = self.svm.latest_blockhash();
        let msg = Message::new_with_blockhash(
            &[Instruction {
                program_id: ID.into(),
                accounts,
                data,
            }],
            Some(&sender.pubkey()),
            &blockhash,
        );

        Transaction::new(&[sender], msg, blockhash)
    }

    pub fn make_signed_msg(
        &self,
        payload: &Payload,
        executor_indices: &[usize],
    ) -> (SignedMsgIxData, Vec<AccountMeta>) {
        let mut bytes = Vec::with_capacity(borsh::object_length(payload).unwrap());
        payload.serialize(&mut bytes).unwrap();

        let bytes = base64::engine::general_purpose::STANDARD.encode(&bytes);

        let mut sha256 = Sha256::new();
        let hash = sha256.digest(bytes.as_bytes());

        let processor = self.keys[PROCESSOR_KEY]
            .sign_prehash_recoverable(&hash)
            .unwrap();

        let mut approvers = Vec::with_capacity(executor_indices.len());

        for i in executor_indices {
            let i = *i;
            assert_ne!(i, PROCESSOR_KEY);

            let signature = self.keys[i].sign_prehash_recoverable(&hash).unwrap();
            approvers.push(signature);
        }

        let data = SignedMsgIxData {
            processor,
            approvers,
            payload: bytes,
        };

        let metas = if let SignedAction::Execute(action) = &payload.action {
            let mut metas: Vec<AccountMeta> = Vec::with_capacity(1 + action.accounts.len());
            metas.push(AccountMeta {
                pubkey: Pubkey::new_from_array(action.program_id),
                is_writable: true,
                is_signer: false,
            });

            metas.extend(action.accounts.iter().map(|x| AccountMeta {
                pubkey: Pubkey::new_from_array(x.pubkey),
                is_writable: x.is_writable,
                is_signer: false,
            }));

            metas
        } else {
            vec![]
        };

        (data, metas)
    }

    pub fn mint(&mut self, to: &Pubkey, amount: u64) {
        MintTo::new(&mut self.svm, &self.token_owner, &self.token, to, amount)
            .send()
            .unwrap()
    }

    pub fn make_ata(&mut self, acc: &Keypair) -> Pubkey {
        CreateAssociatedTokenAccount::new(&mut self.svm, acc, &self.token)
            .send()
            .unwrap()
    }

    pub fn transfer_payload(&self, id: u64, to: Pubkey, amount: u64) -> Payload {
        transfer_payload(id, ID.into(), TokenProgram::Legacy, self.token, to, amount)
    }
}

pub fn token_holder_acc(mint: &Pubkey, sender: &Pubkey) -> Pubkey {
    let seeds = &[
        TOKEN_HOLDER_SEED,
        mint.as_array().as_slice(),
        sender.as_array().as_slice(),
    ];
    let (holder, _) = Pubkey::find_program_address(seeds, &ID.into());

    holder
}
