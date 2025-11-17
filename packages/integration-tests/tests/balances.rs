use std::{collections::BTreeMap, net::TcpListener, str::FromStr};

use cosmos::{
    proto::cosmos::bank::v1beta1::MsgSend, Coin, CosmosNetwork, HasAddress, HasAddressHrp,
    SeedPhrase, TxBuilder,
};
use integration_tests::{get_cosmos_connection, prepare_local_contract};
use kolme::*;
use rust_decimal::dec;
use testtasks::TestTasks;

/// In the future, move to an example and convert the binary to a library.
#[derive(Clone)]
pub struct SampleKolmeApp {
    pub genesis: GenesisInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct SampleState {}

impl MerkleSerialize for SampleState {
    fn merkle_serialize(
        &self,
        _serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        Ok(())
    }
}

impl MerkleDeserialize for SampleState {
    fn merkle_deserialize(
        _deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(SampleState {})
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SampleMessage {
    GiveOsmo { amount: Decimal },
}

const DUMMY_CODE_VERSION: &str = "dummy code version";

const OSMO_ASSET_ID: AssetId = AssetId(1);

impl SampleKolmeApp {
    fn new(code_id: u64, validator: PublicKey) -> Self {
        let mut chains = ConfiguredChains::default();
        let mut assets = BTreeMap::new();
        assets.insert(
            AssetName("uosmo".to_owned()),
            AssetConfig {
                decimals: 6,
                asset_id: OSMO_ASSET_ID,
            },
        );
        chains
            .insert_cosmos(
                CosmosChain::OsmosisLocal,
                ChainConfig {
                    assets,
                    bridge: BridgeContract::NeededCosmosBridge { code_id },
                },
            )
            .unwrap();
        let genesis = GenesisInfo {
            kolme_ident: "Dev code".to_owned(),
            validator_set: ValidatorSet {
                processor: validator,
                listeners: std::iter::once(validator).collect(),
                needed_listeners: 1,
                approvers: std::iter::once(validator).collect(),
                needed_approvers: 1,
            },
            chains,
            version: DUMMY_CODE_VERSION.to_owned(),
        };

        Self { genesis }
    }
}

impl KolmeApp for SampleKolmeApp {
    type State = SampleState;
    type Message = SampleMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
    }

    fn new_state(&self) -> anyhow::Result<Self::State> {
        Ok(SampleState {})
    }

    async fn execute(
        &self,
        _ctx: &mut ExecutionContext<'_, Self>,
        _msg: &Self::Message,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn test_balances() {
    init_logger(true, None);
    TestTasks::start(test_balances_inner, ()).await;
}

async fn test_balances_inner(testtasks: TestTasks, (): ()) {
    let cosmos = get_cosmos_connection().await.unwrap();
    let submitter_seed = SeedPhrase::random();
    let submitter_wallet = submitter_seed
        .with_hrp(CosmosNetwork::OsmosisLocal.get_address_hrp())
        .unwrap();
    let user_seed = SeedPhrase::random();
    let user_wallet = user_seed
        .with_hrp(CosmosNetwork::OsmosisLocal.get_address_hrp())
        .unwrap();
    let master_wallet = SeedPhrase::from_str("osmosis-local")
        .unwrap()
        .with_hrp(CosmosNetwork::OsmosisLocal.get_address_hrp())
        .unwrap();
    for wallet in [&submitter_wallet, &user_wallet] {
        let mut builder = TxBuilder::default();
        builder.add_message(MsgSend {
            from_address: master_wallet.get_address_string(),
            to_address: wallet.get_address_string(),
            amount: vec![Coin {
                denom: "uosmo".to_owned(),
                amount: "20000000".to_owned(),
            }],
        });
        builder
            .sign_and_broadcast(&cosmos, &master_wallet)
            .await
            .unwrap();
    }

    let validator = SecretKey::random();
    let orig_code_id = prepare_local_contract(&submitter_wallet)
        .await
        .unwrap()
        .get_code_id();
    let kolme = Kolme::new(
        SampleKolmeApp::new(orig_code_id, validator.public_key()),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();

    testtasks.spawn_persistent(Processor::new(kolme.clone(), validator.clone()).run());
    testtasks.try_spawn_persistent(Submitter::new_cosmos(kolme.clone(), submitter_seed).run());
    testtasks.try_spawn_persistent(
        Listener::new(kolme.clone(), validator.clone()).run(ChainName::Cosmos),
    );
    testtasks.try_spawn_persistent(Approver::new(kolme.clone(), validator).run());

    let api_server_port = TcpListener::bind("0.0.0.0:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    testtasks.try_spawn_persistent(ApiServer::new(kolme.clone()).run(("::", api_server_port)));

    kolme
        .wait_for_block(BlockHeight::start().next())
        .await
        .unwrap();

    let cosmos = kolme.get_cosmos(CosmosChain::OsmosisLocal).await.unwrap();

    // OK, setup is done.
    let contract = match &kolme
        .read()
        .get_bridge_contracts()
        .get(ExternalChain::OsmosisLocal)
        .unwrap()
        .config
        .bridge
    {
        BridgeContract::NeededCosmosBridge { .. } => unreachable!(),
        BridgeContract::NeededSolanaBridge { .. } => unreachable!(),
        BridgeContract::Deployed(addr) => addr.parse().unwrap(),
    };
    let contract = cosmos.make_contract(contract);

    // Ensure we start with no funds
    assert_eq!(
        kolme
            .read()
            .get_framework_state()
            .get_chain_states()
            .get(ExternalChain::OsmosisLocal)
            .unwrap()
            .assets,
        BTreeMap::new()
    );
    assert_eq!(
        cosmos.all_balances(contract.get_address()).await.unwrap(),
        vec![]
    );

    // Confirm that no account is set up yet
    let res = tokio::time::timeout(tokio::time::Duration::from_millis(500), async {
        reqwest::get(format!(
            "http://127.0.0.1:{api_server_port}/account-id/wallet/{user_wallet}?timeout=0"
        ))
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<kolme::api_server::AccountIdResp>()
        .await
        .unwrap()
    })
    .await
    .unwrap();
    assert_eq!(res, kolme::api_server::AccountIdResp::NotFound {});

    // Deposit some funds and make sure the balances update correctly
    let secret = SecretKey::random();
    let tx_response = contract
        .execute(
            &user_wallet,
            vec![Coin {
                denom: "uosmo".to_owned(),
                amount: "5".to_owned(),
            }],
            shared::cosmos::ExecuteMsg::Regular {
                keys: vec![KeyRegistration {
                    signature: secret
                        .sign_recoverable(user_wallet.get_address_string())
                        .unwrap(),
                    key: secret.public_key(),
                }],
            },
        )
        .await
        .unwrap();
    let bridge_event_ids = kolme::utils::cosmos::parse_bridge_event_ids(&tx_response);
    assert_eq!(bridge_event_ids.len(), 1);
    kolme
        .wait_for_bridge_event(ExternalChain::OsmosisLocal, bridge_event_ids[0])
        .await
        .unwrap();
    assert_eq!(
        kolme
            .read()
            .get_framework_state()
            .get_chain_states()
            .get(ExternalChain::OsmosisLocal)
            .unwrap()
            .assets,
        std::iter::once((OSMO_ASSET_ID, dec![0.000005])).collect()
    );
    assert_eq!(
        cosmos.all_balances(contract.get_address()).await.unwrap(),
        vec![Coin {
            denom: "uosmo".to_owned(),
            amount: "5".to_owned(),
        }]
    );

    // Check the account ID endpoint
    let expected_account_id = tokio::time::timeout(
        tokio::time::Duration::from_millis(500),
        kolme
            .read()
            .wait_account_for_wallet(&Wallet(user_wallet.get_address_string())),
    )
    .await
    .unwrap()
    .unwrap();
    let res = reqwest::get(format!(
        "http://127.0.0.1:{api_server_port}/account-id/wallet/{user_wallet}"
    ))
    .await
    .unwrap()
    .error_for_status()
    .unwrap()
    .json::<kolme::api_server::AccountIdResp>()
    .await
    .unwrap();
    assert_eq!(
        res,
        kolme::api_server::AccountIdResp::Found {
            account_id: expected_account_id
        }
    );
    let res2 = reqwest::get(format!(
        "http://127.0.0.1:{api_server_port}/account-id/pubkey/{}",
        secret.public_key()
    ))
    .await
    .unwrap()
    .error_for_status()
    .unwrap()
    .json::<kolme::api_server::AccountIdResp>()
    .await
    .unwrap();
    assert_eq!(res, res2);
    let res3 = reqwest::get(format!(
        "http://127.0.0.1:{api_server_port}/account-id/{expected_account_id}"
    ))
    .await
    .unwrap()
    .error_for_status()
    .unwrap()
    .json::<kolme::api_server::AccountResp>()
    .await
    .unwrap();
    assert_eq!(
        res3,
        kolme::api_server::AccountResp::Found {
            wallets: std::iter::once(Wallet(user_wallet.get_address_string())).collect(),
            pubkeys: std::iter::once(secret.public_key()).collect()
        }
    );

    // TODO attempt to withdraw more than 5uosmo and confirm that
    // the system waits until more deposits come in. Feature for later.

    let old_user_uosmo = cosmos
        .all_balances(user_wallet.get_address())
        .await
        .unwrap()
        .into_iter()
        .find_map(|Coin { denom, amount }| {
            if denom == "uosmo" {
                Some(amount.parse::<u128>().unwrap())
            } else {
                None
            }
        })
        .unwrap();
    let block = kolme
        .sign_propose_await_transaction(
            &secret,
            vec![Message::Bank(BankMessage::Withdraw {
                asset: OSMO_ASSET_ID,
                chain: ExternalChain::OsmosisLocal,
                dest: Wallet(user_wallet.get_address_string()),
                amount: dec![0.000002],
            })],
        )
        .await
        .unwrap();
    let action_id = match &*kolme.get_log_events_for(block.height()).await.unwrap() {
        [LogEvent::NewBridgeAction { chain, id }] => {
            assert_eq!(chain, &ExternalChain::OsmosisLocal);
            *id
        }
        events => panic!("{events:?}"),
    };
    kolme
        .wait_for_action_finished(ExternalChain::OsmosisLocal, action_id)
        .await
        .unwrap();
    let new_user_uosmo = cosmos
        .all_balances(user_wallet.get_address())
        .await
        .unwrap()
        .into_iter()
        .find_map(|Coin { denom, amount }| {
            if denom == "uosmo" {
                Some(amount.parse::<u128>().unwrap())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(old_user_uosmo + 2, new_user_uosmo);
    assert_eq!(
        cosmos.all_balances(contract.get_address()).await.unwrap(),
        vec![Coin {
            denom: "uosmo".to_owned(),
            amount: "3".to_owned(),
        }]
    );
    assert_eq!(
        kolme
            .read()
            .get_framework_state()
            .get_chain_states()
            .get(ExternalChain::OsmosisLocal)
            .unwrap()
            .assets,
        std::iter::once((OSMO_ASSET_ID, dec![0.000003])).collect()
    );
}
