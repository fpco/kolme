use std::{sync::Arc, time::Duration, collections::btree_map::BTreeMap};

use tokio::{fs, process::Command};
use anyhow::{Result, ensure};
use kolme_solana_bridge_client::{
    pubkey::Pubkey,
    signer::Signer,
    keypair::Keypair,
    RegularMsgIxData, Secp256k1PubkeyCompressed, TokenProgram, derive_token_holder_acc
};
use cosmos::{Cosmos, CosmosNetwork};
use solana_client::nonblocking::rpc_client::{RpcClient as SolanaClient};
use spl_token_client::{
    client::{ProgramRpcClient, ProgramRpcClientSendTransaction},
    token::Token,
    spl_token_2022::amount_to_ui_amount_string
};
use solana_commitment_config::CommitmentConfig;
use kolme::*;

pub type SolanaToken = Token<ProgramRpcClientSendTransaction>;

const API_SERVER_ADDR: &str = "http://localhost:3000";

const BRIDGE_PUBKEY: Pubkey = Pubkey::from_str_const("7Y2ftN9nSf4ubzRDiUvcENMeV4S695JEFpYtqdt836pW");
const BRIDGE_SEED: &str = "artist output bronze steak monkey bachelor nephew october noble title else matter";

pub const AUTHORITY_PUBKEY: Pubkey = Pubkey::from_str_const("BjBcFpiEDrWqnwcKzoto5eFKvRWJbXoxaXT52MBUvWBm");
const AUTHORITY_SEED: &str = "ostrich fortune body empower spider buzz become exhibit ship used hazard quarter";
pub const AUTHORITY_SEED_BYTES: &[u8] = &[205,175,145,238,231,154,37,165,125,85,28,195,15,253,232,178,61,236,34,209,242,135,80,208,145,8,249,218,27,140,167,153,159,99,65,241,6,182,41,41,205,53,38,157,224,248,3,43,214,243,105,46,23,105,218,162,120,135,85,88,153,44,66,60];

const OSMO_PUBKEY: Pubkey = Pubkey::from_str_const("osmof7hTFAuNjwMCcxVNThBDDftMNjiLR2cidDQzvwQ");
const OSMO_SEED_BYTES: &[u8] = &[162,178,232,41,6,244,223,253,143,148,130,111,210,6,186,57,228,38,205,153,89,48,38,133,167,110,85,174,78,54,79,27,12,2,32,56,11,166,157,98,85,18,70,157,43,44,82,215,54,42,106,50,252,90,221,116,19,6,253,46,149,225,28,31];

const TOKEN_PROGRAM: TokenProgram = TokenProgram::Legacy;

#[derive(serde::Deserialize)]
pub struct KolmeState {
    pub next_height: BlockHeight,
    pub next_genesis_action: Option<GenesisAction>,
    pub bridges: BTreeMap<ExternalChain, ChainConfig>,
    pub balances: BTreeMap<AccountId, BTreeMap<AssetId, Decimal>>,
}

pub async fn deploy_solana_bridge() -> Result<()> {
    let workdir = env!("CARGO_MANIFEST_DIR");

    if !fs::try_exists(format!("{workdir}/solana/sbf-out/kolme_solana_bridge.so")).await? {
        tracing::info!("Building Solana bridge program...");

        let mut cmd = Command::new("cargo");
        cmd.current_dir(workdir)
            .args([
                "build-sbf",
                "--manifest-path",
                "../../solana/crates/kolme-solana-bridge/Cargo.toml",
                "--sbf-out-dir",
                "./solana/sbf-out"
            ])
            .spawn()?
            .wait()
            .await?;
    }

    tracing::info!("Deploying Solana Kolme bridge program ({}).", BRIDGE_PUBKEY.to_string());

    // Unfortunately the Solana libs do not provide a way to programatically deploy
    // and the CLI deployment code is too unhinged to easily port:
    // https://github.com/anza-xyz/agave/blob/266ad4781481bddffcf6d3afa995dfaee2ea033e/cli/src/program.rs#L1275
    let mut cmd = Command::new("solana");
    cmd.current_dir(workdir)
        .args([
            "program",
            "deploy",
            "--program-id",
            "./solana/bridge_keypair.json",
            "./solana/sbf-out/kolme_solana_bridge.so",
        ])
        .spawn()?
        .wait()
        .await?;

    Ok(())
}

pub async fn make_osmo_token(client: Arc<SolanaClient>) -> Result<SolanaToken> {
    make_token(client, Keypair::from_bytes(OSMO_SEED_BYTES).unwrap(), 6).await
}

pub async fn make_token(client: Arc<SolanaClient>, token_signer: Keypair, decimals: u8) -> Result<SolanaToken> {
    let authority = Keypair::from_bytes(AUTHORITY_SEED_BYTES).unwrap();
    airdrop(&client, &authority.pubkey(), 10_000000).await?;

    let client = ProgramRpcClient::new(client, ProgramRpcClientSendTransaction::default());
    let token = SolanaToken::new(
        Arc::new(client),
        &TOKEN_PROGRAM.program_id(),
        &token_signer.pubkey(),
        Some(decimals),
        Arc::new(authority.insecure_clone())
    );

    tracing::info!("Creating mint ({}).", token_signer.pubkey().to_string());
    token.create_mint(&authority.pubkey(), None, vec![], &vec![token_signer, authority]).await?;

    Ok(token)
}

pub fn make_solana_client() -> Arc<SolanaClient> {
    Arc::new(SolanaClient::new("http://localhost:8899".into()))
}

pub async fn make_cosmos_client() -> Result<Cosmos> {
    Ok(CosmosNetwork::OsmosisLocal.builder_with_config().await?.build()?)
}

pub async fn airdrop(client: &SolanaClient, to: &Pubkey, amount: u64) -> Result<()> {
    client.request_airdrop(to, amount).await?;

    tracing::info!("Waiting for confirmation on airdrop to {} for {} SOL.", to.to_string(), amount_to_ui_amount_string(amount, 6));
    client.wait_for_balance_with_commitment(to, Some(amount), CommitmentConfig::finalized()).await?;

    Ok(())
}

pub async fn mint_to(token: &SolanaToken, to: &Pubkey, amount: u64) -> Result<()> {
    let ata = token.get_associated_token_address(to);
    let acc = token.get_or_create_associated_account_info(to).await?.base;
    assert_eq!(acc.owner, *to);
    assert_eq!(acc.mint, *token.get_address());

    let authority = Keypair::from_bytes(AUTHORITY_SEED_BYTES).unwrap();

    let info = token.get_mint_info().await?.base;
    tracing::info!(
        "Minting {} {} to {} (ATA: {}).",
        amount_to_ui_amount_string(amount, info.decimals),
        token.get_address().to_string(),
        to.to_string(),
        ata.to_string()
    );

    token.mint_to(&ata, &AUTHORITY_PUBKEY, amount, &[authority]).await?;

    Ok(())
}

pub async fn deposit(client: &SolanaClient, sender: &Keypair, token: &SolanaToken, amount: u64) -> Result<()> {
    deposit_and_register(client, sender, token, amount, vec![]).await
}

pub async fn deposit_and_register(
    client: &SolanaClient,
    sender: &Keypair,
    token: &SolanaToken,
    amount: u64,
    keys: Vec<Secp256k1PubkeyCompressed>
) -> Result<()> {
    let holder = derive_token_holder_acc(&BRIDGE_PUBKEY, token.get_address(), &sender.pubkey());
    let holder_acc = token.get_or_create_associated_account_info(&holder).await?.base;
    assert_eq!(holder_acc.owner, holder);
    assert_eq!(holder_acc.mint, *token.get_address());

    // token.get_or_create_associated_account_info(&sender.pubkey()).await?.base;

    let data = RegularMsgIxData {
        keys,
        transfer_amounts: vec![amount],
    };

    tracing::info!("{} depositing to Solana bridge contract.", sender.pubkey().to_string());

    let blockhash = client.get_latest_blockhash().await?;
    let tx = kolme_solana_bridge_client::regular_tx(BRIDGE_PUBKEY, TOKEN_PROGRAM, blockhash, sender, &data, &[*token.get_address()])?;
    client.send_and_confirm_transaction(&tx).await?;

    Ok(())
}

pub fn bridge_pubkey_from_hex(pubkey: &str) -> Secp256k1PubkeyCompressed {
    let bytes = hex::decode(pubkey).expect("Invalid pubkey hex bytes.");

    Secp256k1PubkeyCompressed(bytes.try_into().expect("Invalid Secp256k1 pubkey"))
}

pub async fn kolme_state(client: &reqwest::Client) -> Result<KolmeState> {
    let resp = client.get(API_SERVER_ADDR).send().await?;
    ensure!(resp.status().is_success());

    resp.json::<KolmeState>().await.map_err(Into::into)
}

pub async fn wait_until_init(client: &reqwest::Client) -> Result<()> {
    tracing::info!("Waiting for bridges to be initialized...");

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;

        let state = kolme_state(client).await?;
        if state.next_genesis_action.is_none() {
            return Ok(());
        }
    }
}
