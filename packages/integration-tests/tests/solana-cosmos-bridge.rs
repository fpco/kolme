use std::{net::SocketAddr, str::FromStr, time::Duration};

use cosmos::{Address, AddressHrp, HasAddress, SeedPhrase, Wallet};
use example_solana_cosmos_bridge::CODE_VERSION;
use rust_decimal::{dec, Decimal};
use tokio::task;

use integration_tests::setup::{
    airdrop, cosmos_deposit_and_register, cosmos_send_osmo, deploy_solana_bridge, kolme_state,
    make_cosmos_client, make_osmo_token, make_solana_client, solana_deposit_and_register,
    solana_mint_to, wait_until, wait_until_init,
};
use kolme::*;
use kolme_solana_bridge_client::{keypair::Keypair, signer::Signer};
use shared::types::KeyRegistration;

const COSMOS_SUBMITTER_SEED: &str = "notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius";

const LOCALHOST: &str = "http://localhost:3000";

#[tokio::test]
#[ignore = "depends on local Solana validator and localosmosis thus hidden from default tests"]
async fn bridge_transfer() {
    init_logger(true, None);
    use example_solana_cosmos_bridge::{BridgeMessage, SolanaCosmosBridgeApp, ASSET_ID};

    deploy_solana_bridge().await.unwrap();

    let mint_amount: u64 = 12_000000;
    let hrp = AddressHrp::from_str("osmo").unwrap();

    let http_client = reqwest::Client::new();
    let solana_client = make_solana_client();
    let cosmos_client = make_cosmos_client().await.unwrap();

    let key_solana = SecretKey::random();
    let key_cosmos = SecretKey::random();

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

    let store = KolmeStore::new_in_memory();
    let kolme = Kolme::new(SolanaCosmosBridgeApp::default(), CODE_VERSION, store)
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
        },
        async {
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
        vec![KeyRegistration::solana(user_solana.pubkey().to_bytes(), &key_solana).unwrap()],
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
        vec![KeyRegistration::cosmos(&user_cosmos.get_address_string(), &key_cosmos).unwrap()],
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

#[tokio::test]
#[ignore = "depends on local Solana validator which needs to be restarted after each test thus hidden from default tests"]
async fn solana_listener_catchup() {
    init_logger(true, None);
    use example_six_sigma::{SixSigmaApp, StoreType};

    deploy_solana_bridge().await.unwrap();

    let http_client = reqwest::Client::new();
    let client = make_solana_client();
    let user = Keypair::new();
    let user_key = SecretKey::random();
    let submitter = Keypair::new();

    let (osmo, _) = futures::join!(
        async { make_osmo_token(client.clone()).await.unwrap() },
        async {
            airdrop(&client, &submitter.pubkey(), 10000000)
                .await
                .unwrap()
        },
    );

    let socket_addr = SocketAddr::from_str("[::]:3000").unwrap();
    let mut tasks = example_six_sigma::run_tasks(
        SixSigmaApp::new_solana(submitter),
        socket_addr,
        StoreType::InMemory,
        None,
        None,
    )
    .await
    .unwrap();

    let mint_amount = 10000000; // 10
    let send_amount = 2000000; // 2
    let send_amount_dec = Decimal::new(send_amount as i64, 6);

    futures::join!(
        async {
            wait_until_init(&http_client).await.unwrap();
        },
        async {
            airdrop(&client, &user.pubkey(), 2000000).await.unwrap();
        },
        async {
            solana_mint_to(&osmo, &user.pubkey(), mint_amount)
                .await
                .unwrap();
        }
    );

    tracing::info!("Depositing and registering");
    solana_deposit_and_register(
        &client,
        &user,
        &osmo,
        send_amount,
        vec![KeyRegistration::solana(user.pubkey().to_bytes(), &user_key).unwrap()],
    )
    .await
    .unwrap();

    tracing::info!(
        "Confirming Kolme has registered {} OSMO for user.",
        send_amount_dec
    );
    wait_until(&http_client, 2000, |state| {
        state
            .balance(AccountId(1), AssetId(1))
            .is_some_and(|x| x == send_amount_dec)
    })
    .await
    .unwrap();

    let listener = tasks.listener.clone().unwrap();
    listener.abort();

    for _ in 0..2 {
        solana_deposit_and_register(&client, &user, &osmo, send_amount, vec![])
            .await
            .unwrap();
    }

    assert!(listener.is_finished());
    tasks.spawn_listener();

    solana_deposit_and_register(&client, &user, &osmo, send_amount, vec![])
        .await
        .unwrap();

    tracing::info!("Confirming Kolme listener has caught up.");
    wait_until(&http_client, 2000, |state| {
        state
            .balance(AccountId(1), AssetId(1))
            .is_some_and(|x| x == send_amount_dec * dec!(4))
    })
    .await
    .unwrap();

    // Test for off by 1 errors...
    let listener = tasks.listener.clone().unwrap();
    listener.abort();

    solana_deposit_and_register(&client, &user, &osmo, send_amount, vec![])
        .await
        .unwrap();

    assert!(listener.is_finished());
    tasks.spawn_listener();

    tracing::info!("Confirming Kolme listener has caught up.");
    wait_until(&http_client, 2000, |state| {
        tracing::info!("{:?}", state.balances);
        state
            .balance(AccountId(1), AssetId(1))
            .is_some_and(|x| x == send_amount_dec * dec!(5))
    })
    .await
    .unwrap();
}
