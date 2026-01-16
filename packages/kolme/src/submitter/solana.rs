use std::str::FromStr;

use base64::Engine;
use borsh::BorshDeserialize;
use kolme_solana_bridge_client::{
    init_tx, instruction::account_meta::AccountMeta, keypair::Keypair, pubkey::Pubkey, signed_tx,
    ComputeBudgetInstruction,
};
use shared::solana::{InitializeIxData, Payload, SignedAction, SignedMsgIxData};

use super::*;

pub async fn instantiate(
    client: &SolanaClient,
    keypair: &Keypair,
    program_id: &str,
    set: ValidatorSet,
) -> Result<()> {
    tracing::info!("Instantiate new contract: {program_id}");

    let data = InitializeIxData { set };

    let program_pubkey = Pubkey::from_str(program_id)?;
    let blockhash = client.get_latest_blockhash().await?;
    let tx = init_tx(program_pubkey, blockhash, keypair, &data).map_err(KolmeError::from)?;

    client.send_and_confirm_transaction(&tx).await?;

    Ok(())
}

pub async fn execute(
    client: &SolanaClient,
    keypair: &Keypair,
    program_id: &str,
    processor: SignatureWithRecovery,
    approvals: &BTreeMap<PublicKey, SignatureWithRecovery>,
    payload_b64: String,
    fee_per_cu: Option<u64>,
) -> Result<String, KolmeError> {
    let payload_bytes = base64::engine::general_purpose::STANDARD.decode(&payload_b64)?;
    let payload: Payload =
        BorshDeserialize::try_from_slice(&payload_bytes).map_err(KolmeError::from)?;

    tracing::info!(
        "Executing signed message on bridge {program_id}: {:?}",
        payload
    );

    let program_id = Pubkey::from_str(program_id)?;
    let metas = if let SignedAction::Execute(ref action) = payload.action {
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

    let data = SignedMsgIxData {
        processor,
        approvers: approvals.values().copied().collect(),
        payload: payload_b64,
    };

    let blockhash = client.get_latest_blockhash().await?;
    tracing::info!("Constructing and sending tx using blockhash {blockhash}");
    let mut leading_instructions = vec![];
    if let Some(fee_per_cu) = fee_per_cu {
        leading_instructions.push(ComputeBudgetInstruction::set_compute_unit_price(fee_per_cu));
    }
    let tx = signed_tx(
        leading_instructions,
        program_id,
        blockhash,
        keypair,
        &data,
        &metas,
    )
    .map_err(KolmeError::from)?;

    match client.send_and_confirm_transaction(&tx).await {
        Ok(sig) => Ok(sig.to_string()),
        Err(e) => {
            tracing::error!(
                "Solana submitter failed to execute signed transaction: {}, error kind: {:?}",
                e,
                e.root_cause()
                    .downcast_ref::<solana_rpc_client_api::client_error::Error>()
                    .map(|e| &e.kind)
            );
            Err(e.into())
        }
    }
}
