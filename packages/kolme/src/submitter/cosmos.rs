use ::cosmos::{ContractAdmin, Cosmos, HasAddressHrp, SeedPhrase, TxBuilder};
use shared::cosmos::{ExecuteMsg, InstantiateMsg};

use super::*;

pub async fn instantiate(
    cosmos: &Cosmos,
    seed_phrase: &SeedPhrase,
    code_id: u64,
    set: ValidatorSet,
) -> Result<String> {
    let msg = InstantiateMsg { set };

    let wallet = seed_phrase.with_hrp(cosmos.get_address_hrp())?;
    let contract = cosmos
        .make_code_id(code_id)
        .instantiate(
            &wallet,
            "Kolme Framework Bridge Contract".to_owned(),
            vec![],
            &msg,
            ContractAdmin::Sender,
        )
        .await?;

    tracing::info!("Instantiate new contract: {contract}");

    let res = TxBuilder::default()
        .add_update_contract_admin(&contract, &wallet, &contract)
        .sign_and_broadcast(cosmos, &wallet)
        .await?;

    tracing::info!(
        "Updated admin on {contract} to its own address in tx {}",
        res.txhash
    );

    Ok(contract.to_string())
}

pub async fn execute(
    cosmos: &Cosmos,
    seed_phrase: &SeedPhrase,
    contract: &str,
    processor: SignatureWithRecovery,
    approvals: &BTreeMap<PublicKey, SignatureWithRecovery>,
    payload: &str,
) -> Result<String> {
    tracing::info!("Executing signed message on bridge: {contract}");

    let msg = ExecuteMsg::Signed {
        processor,
        approvers: approvals.values().copied().collect(),
        payload: payload.to_owned(),
    };

    let contract = cosmos.make_contract(contract.parse()?);
    let res = contract
        .execute(
            &seed_phrase.with_hrp(contract.get_address_hrp())?,
            vec![],
            msg,
        )
        .await;

    match res {
        Ok(tx) => Ok(tx.txhash),
        Err(e) => {
            tracing::error!(
                "Cosmos submitter failed to execute signed transaction: {}",
                e
            );

            Err(anyhow::anyhow!(e))
        }
    }
}
