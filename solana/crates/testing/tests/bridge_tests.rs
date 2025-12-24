use std::{fs, process::Command};

use borsh::BorshSerialize;
use kolme_solana_bridge_client::{
    derive_git_rev_pda, derive_upgrade_authority_pda, instruction::error::InstructionError,
    keypair::Keypair, signer::Signer, spl_client,
};
use litesvm_token::{
    get_spl_account, spl_token::state::Account as SplAccount, CreateAssociatedTokenAccount,
    CreateMint, MintTo,
};
use sha_256::Sha256;
use shared::{
    cryptography::SecretKey,
    solana::{ExecuteAction, Payload, RegularMsgIxData, SignedAction},
};
use solana_transaction_error::TransactionError;
use testing::{
    token_holder_acc, Program, APPROVER1_KEY, APPROVER2_KEY, APPROVER3_KEY, APPROVER4_KEY, ID,
    TOKEN,
};

#[test]
fn must_init_first() {
    let mut p = Program::new();
    let sender = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();

    let payload = Payload {
        id: 0,
        action: SignedAction::Execute(ExecuteAction {
            program_id: TOKEN.to_bytes(),
            accounts: vec![],
            instruction_data: vec![],
            signer: None,
        }),
    };

    let (data, metas) = p.make_signed_msg(&payload, &[APPROVER1_KEY, APPROVER3_KEY]);

    let meta = p.signed(&sender, &data, &metas).unwrap_err();
    assert_eq!(
        meta.err,
        TransactionError::InstructionError(0, InstructionError::UninitializedAccount)
    );
}

#[test]
fn sending_funds_works() {
    let mut p = Program::new();
    let sender = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();
    p.init_default(&sender).unwrap();

    let sender_ata_acc = p.make_ata(&sender);
    let holder_acc = token_holder_acc(&p.token);
    let holder_ata_acc = spl_client::address::get_associated_token_address(&holder_acc, &p.token);

    let mint_amount = 10_00000000; // 10
    let transfer_amount = mint_amount / 2; // 5

    p.mint(&sender_ata_acc, mint_amount);

    let sender_ata: SplAccount = get_spl_account(&p.svm, &sender_ata_acc).unwrap();
    assert_eq!(sender_ata.amount, mint_amount);

    let data = RegularMsgIxData {
        keys: vec![],
        transfer_amounts: vec![transfer_amount],
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

    let sender_ata_acc_1 = CreateAssociatedTokenAccount::new(&mut p.svm, &sender, &p.token)
        .send()
        .unwrap();
    let sender_ata_acc_2 = CreateAssociatedTokenAccount::new(&mut p.svm, &sender, &token_2)
        .send()
        .unwrap();

    CreateAssociatedTokenAccount::new(&mut p.svm, &sender, &token_2)
        .owner(&token_holder_acc(&token_2))
        .send()
        .unwrap();

    MintTo::new(
        &mut p.svm,
        &p.token_owner,
        &p.token,
        &sender_ata_acc_1,
        mint_amount,
    )
    .send()
    .unwrap();

    MintTo::new(
        &mut p.svm,
        &token_owner,
        &token_2,
        &sender_ata_acc_2,
        mint_amount,
    )
    .send()
    .unwrap();

    let transfer_amounts = &[mint_amount, mint_amount / 2];
    let data = RegularMsgIxData {
        keys: vec![],
        transfer_amounts: transfer_amounts.into(),
    };

    p.regular(&sender, &data, &[p.token, token_2]).unwrap();

    for (i, token) in [p.token, token_2].iter().enumerate() {
        let holder_acc = token_holder_acc(token);
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

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();
    p.init_default(&sender).unwrap();

    let payload = p.transfer_payload(0, sender.pubkey(), 100);
    let (data, metas) = p.make_signed_msg(&payload, &[APPROVER1_KEY, APPROVER3_KEY]);

    let meta = p.signed(&sender, &data, &metas).unwrap_err();

    // This error occurs on the SPL program invocation because we didn't provide
    // the relevant accounts to it which means that signature checks have passed.
    assert_eq!(
        meta.err,
        TransactionError::InstructionError(0, InstructionError::InvalidAccountData)
    );
}

#[test]
fn signed_transfer() {
    let mut p = Program::new();
    let sender = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();
    p.init_default(&sender).unwrap();

    let sender_ata_acc = p.make_ata(&sender);
    let holder_acc = token_holder_acc(&p.token);
    let holder_ata_acc = spl_client::address::get_associated_token_address(&holder_acc, &p.token);

    let mint_amount = 10_00000000; // 10
    let transfer_amount = mint_amount / 2; // 5

    p.mint(&sender_ata_acc, mint_amount);

    let data = RegularMsgIxData {
        keys: vec![],
        transfer_amounts: vec![transfer_amount],
    };

    p.regular(&sender, &data, &[p.token]).unwrap();

    let holder_ata: SplAccount = get_spl_account(&p.svm, &holder_ata_acc).unwrap();
    assert_eq!(holder_ata.amount, transfer_amount);

    let payload = p.transfer_payload(0, sender.pubkey(), transfer_amount);
    let (data, metas) = p.make_signed_msg(&payload, &[APPROVER1_KEY, APPROVER3_KEY]);

    p.signed(&sender, &data, &metas).unwrap();

    let sender_ata: SplAccount = get_spl_account(&p.svm, &sender_ata_acc).unwrap();
    assert_eq!(sender_ata.amount, mint_amount);

    let holder_ata: SplAccount = get_spl_account(&p.svm, &holder_ata_acc).unwrap();
    assert_eq!(holder_ata.amount, 0);
}

#[test]
fn insufficient_signatures_are_rejected() {
    let mut p = Program::new();
    let sender = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();

    p.init_default(&sender).unwrap();

    let payload = p.transfer_payload(0, sender.pubkey(), 1000);
    let (data, metas) = p.make_signed_msg(&payload, &[APPROVER2_KEY]);

    let meta = p.signed(&sender, &data, &metas).unwrap_err();
    assert_eq!(
        meta.err,
        TransactionError::InstructionError(0, InstructionError::Custom(0))
    );
}

#[test]
fn false_processor_signature_is_rejected() {
    let mut p = Program::new();
    let sender = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();

    p.init_default(&sender).unwrap();

    let payload = p.transfer_payload(0, sender.pubkey(), 1000);
    let (mut data, metas) = p.make_signed_msg(&payload, &[APPROVER1_KEY, APPROVER3_KEY]);

    let fake_key = SecretKey::random();

    let mut bytes = Vec::with_capacity(borsh::object_length(&payload).unwrap());
    payload.serialize(&mut bytes).unwrap();

    let mut sha256 = Sha256::new();
    let hash = sha256.digest(&bytes);

    data.processor = fake_key.sign_prehash_recoverable(&hash).unwrap();

    let meta = p.signed(&sender, &data, &metas).unwrap_err();
    assert_eq!(
        meta.err,
        TransactionError::InstructionError(0, InstructionError::Custom(2))
    );

    let payload = p.transfer_payload(0, sender.pubkey(), 1000);
    let (mut data, metas) = p.make_signed_msg(&payload, &[APPROVER1_KEY, APPROVER3_KEY]);

    let other_payload = p.transfer_payload(1, sender.pubkey(), 1000);
    let (other_data, _) = p.make_signed_msg(&other_payload, &[APPROVER1_KEY, APPROVER3_KEY]);

    data.processor = other_data.processor;

    let meta = p.signed(&sender, &data, &metas).unwrap_err();
    assert_eq!(
        meta.err,
        TransactionError::InstructionError(0, InstructionError::Custom(2))
    );
}

#[test]
fn false_executor_signature_is_rejected() {
    let mut p = Program::new();
    let sender = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();

    p.init_default(&sender).unwrap();

    let payload = p.transfer_payload(0, sender.pubkey(), 1000);
    let (mut data, metas) = p.make_signed_msg(&payload, &[APPROVER1_KEY, APPROVER4_KEY]);

    let fake_key = SecretKey::random();

    let mut bytes = Vec::with_capacity(borsh::object_length(&payload).unwrap());
    payload.serialize(&mut bytes).unwrap();

    let mut sha256 = Sha256::new();
    let hash = sha256.digest(&bytes);

    data.approvers[1] = fake_key.sign_prehash_recoverable(&hash).unwrap();

    let meta = p.signed(&sender, &data, &metas).unwrap_err();
    assert_eq!(
        meta.err,
        TransactionError::InstructionError(0, InstructionError::Custom(0))
    );

    let payload = p.transfer_payload(0, sender.pubkey(), 1000);
    let (mut data, metas) = p.make_signed_msg(&payload, &[APPROVER1_KEY, APPROVER4_KEY]);

    let other_payload = p.transfer_payload(1, sender.pubkey(), 1000);
    let (mut other_data, _) = p.make_signed_msg(&other_payload, &[APPROVER1_KEY, APPROVER4_KEY]);

    data.approvers[1] = other_data.approvers.pop().unwrap();

    let meta = p.signed(&sender, &data, &metas).unwrap_err();
    assert_eq!(
        meta.err,
        TransactionError::InstructionError(0, InstructionError::Custom(0))
    );
}

#[test]
fn upgrade() {
    let mut p = Program::new();
    let sender = Keypair::new();
    let spill = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();

    p.init_default(&sender).unwrap();

    let program_bytes = fs::read("../../target/deploy/kolme_solana_bridge.so").unwrap();

    let authority = derive_upgrade_authority_pda(&ID);
    let buffer = p.make_buffer(program_bytes, &authority);

    let payload = p.upgrade_payload(0, buffer, spill.pubkey());
    let (data, metas) = p.make_signed_msg(&payload, &[APPROVER3_KEY, APPROVER4_KEY, APPROVER1_KEY]);

    p.signed(&sender, &data, &metas).unwrap();

    // Spill account should exist now
    p.svm.get_account(&spill.pubkey()).unwrap();
}

#[test]
fn contains_correct_git_rev() {
    let mut p = Program::new();
    let sender = Keypair::new();

    p.svm.airdrop(&sender.pubkey(), 1000000000).unwrap();

    p.init_default(&sender).unwrap();

    let git_rev_pda = derive_git_rev_pda(&ID);
    let acc = p.svm.get_account(&git_rev_pda).unwrap();

    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();

    let expected_hash = std::str::from_utf8(&output.stdout).unwrap().trim();

    // We skip the discriminator and bump seed bytes + the u32 that encodes the length of the string
    let hash = std::str::from_utf8(&acc.data[6..]).unwrap();
    assert_eq!(hash, expected_hash);
}
