mod setup;

use std::{net::SocketAddr, str::FromStr, time::Duration};

use cosmos::{Address, AddressHrp, HasAddress, SeedPhrase, Wallet};
use rust_decimal::Decimal;
use tokio::task;

use example_solana_cosmos_bridge::{BridgeMessage, SolanaCosmosBridgeApp, ASSET_ID};
use kolme::*;
use kolme_solana_bridge_client::{keypair::Keypair, signer::Signer};
use setup::{
    airdrop, cosmos_deposit_and_register, cosmos_send_osmo, deploy_solana_bridge, kolme_state,
    make_cosmos_client, make_osmo_token, make_solana_client, solana_deposit_and_register,
    solana_key_registration, solana_mint_to, wait_until, wait_until_init,
};
use shared::cosmos::KeyRegistration as CosmosKeyRegistration;

const DB_PATH: &str = "example-solana-cosmos-bridge.fjall";
const DUMMY_CODE_VERSION: &str = "dummy code version";
const COSMOS_SUBMITTER_SEED: &str = "notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius";

const LOCALHOST: &str = "http://localhost:3000";

#[test_log::test(tokio::test)]
#[ignore = "depends on local Solana validator and localosmosis thus hidden from default tests"]
async fn bridge_transfer() {
    deploy_solana_bridge().await.unwrap();

    let mint_amount: u64 = 12_000000;
    let hrp = AddressHrp::from_str("osmo").unwrap();

    let http_client = reqwest::Client::new();
    let solana_client = make_solana_client();
    let cosmos_client = make_cosmos_client().await.unwrap();

    let mut rng = rand::thread_rng();
    let key_solana = SecretKey::random(&mut rng);
    let key_cosmos = SecretKey::random(&mut rng);

    let user_solana = Keypair::new();
    let user_cosmos = Wallet::generate(hrp).unwrap();

    let solana_submitter = Keypair::new();
    let cosmos_submitter = SeedPhrase::from_str(COSMOS_SUBMITTER_SEED).unwrap();
    let submitter_wallet = cosmos_submitter.with_hrp(hrp).unwrap();

    let (osmo, _) = futures::join!(
        async { make_osmo_token(solana_client.clone()).await.unwrap() },
        async {
            airdrop(&solana_client, &solana_submitter.pubkey(), 10000000)
                .await
                .unwrap()
        },
    );

    let store = KolmeStore::new_fjall(DB_PATH).unwrap();
    let kolme = Kolme::new(SolanaCosmosBridgeApp::default(), DUMMY_CODE_VERSION, store)
        .await
        .unwrap();

    let kolme_clone = kolme.clone();
    let socket_addr = SocketAddr::from_str("[::]:3000").unwrap();

    let handle = task::spawn(async move {
        example_solana_cosmos_bridge::serve(
            kolme_clone,
            solana_submitter,
            cosmos_submitter,
            socket_addr,
        )
        .await
    });

    futures::join!(
        async {
            wait_until_init(&http_client).await.unwrap();
        },
        async {
            airdrop(&solana_client, &user_solana.pubkey(), 2000000)
                .await
                .unwrap();
            solana_mint_to(&osmo, &user_solana.pubkey(), mint_amount)
                .await
                .unwrap();
        }
    );

    let state = kolme_state(&http_client).await.unwrap();
    let cosmos_bridge_addr = state.bridge_address(ExternalChain::OsmosisLocal).unwrap();

    cosmos_send_osmo(
        &cosmos_client,
        &submitter_wallet,
        &Address::from_str(cosmos_bridge_addr).unwrap(),
        mint_amount as u128,
    )
    .await
    .unwrap();

    let send_amount = mint_amount / 2;
    let send_amount_dec = Decimal::new(send_amount as i64, 6);

    solana_deposit_and_register(
        &solana_client,
        &user_solana,
        &osmo,
        send_amount,
        vec![solana_key_registration(&user_solana.pubkey(), &key_solana)],
    )
    .await
    .unwrap();

    wait_until(&http_client, 2000, |state| {
        state
            .balance(AccountId(1), ASSET_ID)
            .is_some_and(|x| x == send_amount_dec)
    })
    .await
    .unwrap();

    tracing::info!(
        "Bridging {} OSMO from Solana ({}) to Cosmos ({}).",
        send_amount_dec,
        user_solana.pubkey().to_string(),
        user_cosmos.get_address().to_string()
    );
    example_solana_cosmos_bridge::broadcast(
        BridgeMessage::ToCosmos {
            to: user_cosmos.get_address(),
            amount: send_amount_dec,
        },
        key_solana,
        LOCALHOST,
    )
    .await
    .unwrap();

    tracing::info!(
        "Confirming {} OSMO was successfully bridged to Cosmos.",
        send_amount_dec
    );
    loop {
        tokio::time::sleep(Duration::from_secs(4)).await;
        let balances = cosmos_client
            .all_balances(user_cosmos.get_address())
            .await
            .unwrap();

        if balances.is_empty() {
            continue;
        }

        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].denom.as_str(), "uosmo");
        assert_eq!(balances[0].amount.as_str(), send_amount.to_string());
        break;
    }

    let send_amount = send_amount / 2;
    let send_amount_dec = Decimal::new(send_amount as i64, 6);

    cosmos_deposit_and_register(
        &cosmos_client,
        cosmos_bridge_addr,
        &user_cosmos,
        send_amount as u128,
        vec![CosmosKeyRegistration::new(&user_cosmos.get_address_string(), &key_cosmos).unwrap()],
    )
    .await
    .unwrap();

    tracing::info!("Waiting until {} OSMO is available.", send_amount_dec);
    wait_until(&http_client, 2000, |state| {
        state
            .balance(AccountId(2), ASSET_ID)
            .is_some_and(|x| x == send_amount_dec)
    })
    .await
    .unwrap();

    let ata = osmo.get_associated_token_address(&user_solana.pubkey());
    let acc = osmo.get_account_info(&ata).await.unwrap().base;
    let balance_before = acc.amount;

    tracing::info!(
        "Bridging {} OSMO from Cosmos ({}) to Solana ({}).",
        send_amount_dec,
        user_cosmos.get_address().to_string(),
        user_solana.pubkey().to_string()
    );
    example_solana_cosmos_bridge::broadcast(
        BridgeMessage::ToSolana {
            to: user_solana.pubkey(),
            amount: send_amount_dec,
        },
        key_cosmos,
        LOCALHOST,
    )
    .await
    .unwrap();

    tracing::info!(
        "Confirming {} OSMO was successfully bridged to Solana.",
        send_amount_dec
    );
    loop {
        tokio::time::sleep(Duration::from_secs(4)).await;
        let acc = osmo.get_account_info(&ata).await.unwrap().base;

        if acc.amount == balance_before + send_amount {
            break;
        }
    }

    handle.abort();
}
