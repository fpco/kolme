use ::cosmos::{ContractAdmin, Cosmos, HasAddressHrp, SeedPhrase, TxBuilder};
use shared::cosmos::{ExecuteMsg, InstantiateMsg};

use super::*;

pub async fn instantiate(
    cosmos: &Cosmos,
    seed_phrase: &SeedPhrase,
    code_id: u64,
    args: InstantiateArgs,
) -> Result<String> {
    let needed_approvers = u16::try_from(args.needed_approvers)?;
    let msg = InstantiateMsg {
        processor: args.processor,
        approvers: args.approvers,
        needed_approvers,
    };

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
        .sign_and_broadcast(&cosmos, &wallet)
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
    approvers: Vec<SignatureWithRecovery>,
    payload: String,
) -> Result<String> {
    let msg = ExecuteMsg::Signed {
        processor,
        approvers,
        payload,
    };

    let contract = cosmos.make_contract(contract.parse()?);
    let tx = contract
        .execute(
            &seed_phrase.with_hrp(contract.get_address_hrp())?,
            vec![],
            msg,
        )
        .await?;

    Ok(tx.txhash)
}
