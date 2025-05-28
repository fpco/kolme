use std::{ops::Deref, str::FromStr};

use base64::Engine;
use borsh::{BorshDeserialize, BorshSerialize};
use kolme_solana_bridge_client::{
    init_tx, instruction::account_meta::AccountMeta, keypair::Keypair, pubkey::Pubkey, signed_tx,
    InitializeIxData, Payload, Secp256k1PubkeyCompressed, Secp256k1Signature, Signature,
    SignedMsgIxData,
};

use super::*;

pub async fn instantiate(
    client: &SolanaClient,
    keypair: &Keypair,
    program_id: &str,
    args: ValidatorSet,
) -> Result<()> {
    let needed_approvers = u8::try_from(args.needed_approvers)?;
    let mut executors = Vec::with_capacity(args.approvers.len());

    for a in args.approvers {
        executors.push(Secp256k1PubkeyCompressed(a.as_bytes().deref().try_into()?));
    }

    let data = InitializeIxData {
        needed_executors: needed_approvers,
        processor: Secp256k1PubkeyCompressed(args.processor.as_bytes().deref().try_into()?),
        executors,
    };

    let mut bytes =
        Vec::with_capacity(1 + borsh::object_length(&data).map_err(|e| anyhow::anyhow!(e))?);
    data.serialize(&mut bytes).map_err(|e| anyhow::anyhow!(e))?;
    tracing::info!("Sending initialize instruction. Data: {:?}", bytes);

    let program_pubkey = Pubkey::from_str(program_id)?;
    let blockhash = client.get_latest_blockhash().await?;
    let tx = init_tx(program_pubkey, blockhash, keypair, &data).map_err(|x| anyhow::anyhow!(x))?;

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
) -> Result<String> {
    let payload_bytes = base64::engine::general_purpose::STANDARD.decode(&payload_b64)?;
    let payload: Payload = BorshDeserialize::try_from_slice(&payload_bytes)
        .map_err(|x| anyhow::anyhow!("Error deserializing Solana bridge payload: {:?}", x))?;

    let program_id = Pubkey::from_str(program_id)?;
    let mut metas: Vec<AccountMeta> = Vec::with_capacity(1 + payload.accounts.len());
    metas.push(AccountMeta {
        pubkey: Pubkey::new_from_array(payload.program_id),
        is_writable: true,
        is_signer: false,
    });

    metas.extend(payload.accounts.iter().map(|x| AccountMeta {
        pubkey: Pubkey::new_from_array(x.pubkey),
        is_writable: x.is_writable,
        is_signer: false,
    }));

    let mut executors = Vec::with_capacity(approvals.len());

    for a in approvals.values() {
        let sig = Signature {
            signature: Secp256k1Signature(a.sig.to_bytes().deref().try_into()?),
            recovery_id: processor.recid.to_byte(),
        };

        executors.push(sig);
    }

    let data = SignedMsgIxData {
        processor: Signature {
            signature: Secp256k1Signature(processor.sig.to_bytes().deref().try_into()?),
            recovery_id: processor.recid.to_byte(),
        },
        executors,
        payload: payload_b64,
    };

    let mut bytes =
        Vec::with_capacity(1 + borsh::object_length(&data).map_err(|e| anyhow::anyhow!(e))?);
    data.serialize(&mut bytes).map_err(|e| anyhow::anyhow!(e))?;
    tracing::info!("Sending signed instruction. Data: {:?}", bytes);

    let blockhash = client.get_latest_blockhash().await?;
    let tx =
        signed_tx(program_id, blockhash, keypair, &data, &metas).map_err(|x| anyhow::anyhow!(x))?;

    match client.send_and_confirm_transaction(&tx).await {
        Ok(sig) => Ok(sig.to_string()),
        Err(e) => {
            tracing::error!(
                "Solana submitter failed to execute signed transaction: {}",
                e
            );

            Err(anyhow::anyhow!(e))
        }
    }
}
