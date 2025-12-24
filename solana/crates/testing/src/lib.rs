use std::{collections::BTreeSet, fs};

use base64::Engine;
use borsh::BorshSerialize;
use litesvm::{types::TransactionResult, LiteSVM};
use litesvm_loader::deploy_upgradeable_program;
use litesvm_token::{CreateAssociatedTokenAccount, CreateMint, MintTo};
use sha_256::Sha256;
use solana_feature_set::FeatureSet;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_sysvar::clock::Clock;

use kolme_solana_bridge_client::{
    derive_token_holder_acc, derive_upgrade_authority_pda, init_tx,
    instruction::account_meta::AccountMeta,
    keypair::Keypair,
    pubkey::{declare_id, Pubkey},
    regular_tx, signed_tx,
    signer::Signer,
    transaction::Transaction,
    transfer_payload, upgrade_payload, TokenProgram,
};
use shared::{
    cryptography::SecretKey,
    solana::{InitializeIxData, Payload, RegularMsgIxData, SignedAction, SignedMsgIxData},
    types::ValidatorSet,
};

pub const SECRET: &str =
    "3ZCSwfXPMVSMZixtmKnnAQKLgvL9TTthFLLH6Ls36FAnh2tnhHtP2RDQQmEPiY1zU4B3kzCmLLk6twwHP8wT9Mv2";
declare_id!("GjBMbDbKpLS9jB1wp35UawQSD1upMcawyLKSiEnw5Qci");

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

pub struct Program {
    pub svm: LiteSVM,
    pub keys: Box<[SecretKey; KEYS_LEN]>,
    pub token: Pubkey,
    pub token_owner: Keypair,
    pub deployer: Keypair,
}

impl Program {
    pub fn new() -> Self {
        let mut feature_set = FeatureSet::all_enabled();
        feature_set.deactivate(&solana_feature_set::disable_new_loader_v3_deployments::id());

        let mut svm = LiteSVM::new()
            .with_feature_set(feature_set)
            .with_builtins()
            .with_sysvars()
            .with_spl_programs()
            .with_lamports(1_000_000_000_000_000);

        let deployer = Keypair::new();
        svm.airdrop(&deployer.pubkey(), 20000000000).unwrap();

        let program_keypair = Keypair::from_base58_string(SECRET);
        let program_bytes = fs::read("../../target/deploy/kolme_solana_bridge.so").unwrap();
        deploy_upgradeable_program(&mut svm, &deployer, &program_keypair, &program_bytes).unwrap();

        let keys = unsafe {
            let mut keys = Box::<[SecretKey; KEYS_LEN]>::new_uninit();

            for i in 0..KEYS_LEN {
                (*keys.as_mut_ptr()).as_mut_slice()[i] = SecretKey::random();
            }

            keys.assume_init()
        };

        let token_owner = Keypair::new();
        svm.airdrop(&token_owner.pubkey(), 1000000000).unwrap();

        let token = CreateMint::new(&mut svm, &token_owner).send().unwrap();

        // Create the program holder for this token.
        let holder = token_holder_acc(&token);
        CreateAssociatedTokenAccount::new(&mut svm, &token_owner, &token)
            .owner(&holder)
            .send()
            .unwrap();

        Self {
            svm,
            keys,
            token,
            token_owner,
            deployer,
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
        let blockhash = self.svm.latest_blockhash();

        let authority = derive_upgrade_authority_pda(&ID);
        let tx = Transaction::new_signed_with_payer(
            &[
                solana_loader_v3_interface::instruction::set_upgrade_authority(
                    &ID,
                    &self.deployer.pubkey(),
                    Some(&authority),
                ),
            ],
            Some(&self.deployer.pubkey()),
            &[&self.deployer],
            blockhash,
        );

        self.svm.send_transaction(tx).unwrap();

        let tx = init_tx(ID, blockhash, sender, data).unwrap();
        let res = self.svm.send_transaction(tx);

        if res.is_ok() {
            self.svm.expire_blockhash();
        }

        res
    }

    #[allow(clippy::result_large_err)]
    pub fn regular(
        &mut self,
        sender: &Keypair,
        data: &RegularMsgIxData,
        token_mints: &[Pubkey],
    ) -> TransactionResult {
        let blockhash = self.svm.latest_blockhash();
        let tx = regular_tx(
            ID,
            TokenProgram::Legacy,
            blockhash,
            sender,
            data,
            token_mints,
        )
        .unwrap();

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
        let blockhash = self.svm.latest_blockhash();
        let tx = signed_tx([], ID, blockhash, sender, data, additional).unwrap();

        let res = self.svm.send_transaction(tx);

        if res.is_ok() {
            self.svm.expire_blockhash();
        }

        res
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

    pub fn upgrade_payload(&self, id: u64, buffer: Pubkey, spill: Pubkey) -> Payload {
        upgrade_payload(id, ID.into(), buffer, spill)
    }

    pub fn make_buffer(&mut self, program_bytes: Vec<u8>, authority: &Pubkey) -> Pubkey {
        let buffer_keypair = Keypair::new();
        let temp_authority = Keypair::new();

        let size = UpgradeableLoaderState::size_of_buffer(program_bytes.len());
        let lamports = self.svm.minimum_balance_for_rent_exemption(size);

        let create_ixs = solana_loader_v3_interface::instruction::create_buffer(
            &self.deployer.pubkey(),
            &buffer_keypair.pubkey(),
            &temp_authority.pubkey(),
            lamports,
            program_bytes.len(),
        )
        .unwrap();

        let blockhash = self.svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &create_ixs,
            Some(&self.deployer.pubkey()),
            &[&self.deployer, &buffer_keypair],
            blockhash,
        );

        self.svm.send_transaction(tx).unwrap();

        const CHUNK_SIZE: usize = 1000;

        for (i, chunk) in program_bytes.chunks(CHUNK_SIZE).enumerate() {
            let offset = i * CHUNK_SIZE;

            let write_ix = solana_loader_v3_interface::instruction::write(
                &buffer_keypair.pubkey(),
                &temp_authority.pubkey(),
                offset as u32,
                Vec::from(chunk),
            );

            let tx = Transaction::new_signed_with_payer(
                &[write_ix],
                Some(&self.deployer.pubkey()),
                &[&self.deployer, &temp_authority],
                blockhash,
            );

            self.svm.send_transaction(tx).unwrap();
        }

        let tx = Transaction::new_signed_with_payer(
            &[
                solana_loader_v3_interface::instruction::set_buffer_authority(
                    &buffer_keypair.pubkey(),
                    &temp_authority.pubkey(),
                    authority,
                ),
            ],
            Some(&self.deployer.pubkey()),
            &[&self.deployer, &temp_authority],
            blockhash,
        );

        self.svm.send_transaction(tx).unwrap();
        self.svm.expire_blockhash();

        let current = self.svm.get_sysvar::<Clock>().slot;
        self.svm.warp_to_slot(current + 1);

        buffer_keypair.pubkey()
    }
}

pub fn token_holder_acc(mint: &Pubkey) -> Pubkey {
    derive_token_holder_acc(&ID, mint)
}
