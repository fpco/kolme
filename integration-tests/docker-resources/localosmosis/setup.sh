#!/bin/sh

### lvn-hasky-dragon: Taken from https://github.com/osmosis-labs/osmosis/blob/aec51f84bfd841bc42bf5dc55602fd0d933da7dd/tests/localosmosis/scripts/setup.sh#L1

CHAIN_ID=localosmosis
OSMOSIS_HOME=$HOME/.osmosisd
CONFIG_FOLDER=$OSMOSIS_HOME/config
MONIKER=val

# See https://github.com/osmosis-labs/osmosis/blob/415f64ab1eada52d8cd6d75f9f5c96861fe70f93/tests/localosmosis/README.md?plain=1#L120
MNEMONIC="bottom loan skill merry east cradle onion journey palm apology verb edit desert impose absurd oil bubble sweet glove shallow size build burst effort"
MNEMONIC_TEST1="notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius"
MNEMONIC_TEST2="quality vacuum heart guard buzz spike sight swarm shove special gym robust assume sudden deposit grid alcohol choice devote leader tilt noodle tide penalty"
POOLSMNEMONIC="traffic cool olive pottery elegant innocent aisle dial genuine install shy uncle ride federal soon shift flight program cave famous provide cute pole struggle"

edit_genesis () {

    GENESIS=$CONFIG_FOLDER/genesis.json

    # Update staking module
    dasel put string -f $GENESIS '.app_state.staking.params.bond_denom' -v 'uosmo'
    dasel put string -f $GENESIS '.app_state.staking.params.unbonding_time' -v '240s'

    # Update bank module
    dasel put string -f $GENESIS '.app_state.bank.denom_metadata.[].description' -v 'Registered denom uion for localosmosis testing'
    dasel put string -f $GENESIS '.app_state.bank.denom_metadata.[0].denom_units.[].denom' -v 'uion'
    dasel put string -f $GENESIS '.app_state.bank.denom_metadata.[0].denom_units.[0].exponent' -v 0
    dasel put string -f $GENESIS '.app_state.bank.denom_metadata.[0].base' -v 'uion'
    dasel put string -f $GENESIS '.app_state.bank.denom_metadata.[0].display' -v 'uion'
    dasel put string -f $GENESIS '.app_state.bank.denom_metadata.[0].name' -v 'uion'
    dasel put string -f $GENESIS '.app_state.bank.denom_metadata.[0].symbol' -v 'uion'

    dasel put string -f $GENESIS '.app_state.bank.denom_metadata.[].description' -v 'Registered denom uosmo for localosmosis testing'
    dasel put string -f $GENESIS '.app_state.bank.denom_metadata.[1].denom_units.[].denom' -v 'uosmo'
    dasel put string -f $GENESIS '.app_state.bank.denom_metadata.[1].denom_units.[0].exponent' -v 0
    dasel put string -f $GENESIS '.app_state.bank.denom_metadata.[1].base' -v 'uosmo'
    dasel put string -f $GENESIS '.app_state.bank.denom_metadata.[1].display' -v 'uosmo'
    dasel put string -f $GENESIS '.app_state.bank.denom_metadata.[1].name' -v 'uosmo'
    dasel put string -f $GENESIS '.app_state.bank.denom_metadata.[1].symbol' -v 'uosmo'

    # Update crisis module
    dasel put string -f $GENESIS '.app_state.crisis.constant_fee.denom' -v 'uosmo'

    # Update gov module
    dasel put string -f $GENESIS '.app_state.gov.voting_params.voting_period' -v '60s'
    dasel put string -f $GENESIS '.app_state.gov.params.min_deposit.[0].denom' -v 'uosmo'

    # Update epochs module
    dasel put string -f $GENESIS '.app_state.epochs.epochs.[1].duration' -v "60s"

    # Update poolincentives module
    dasel put string -f $GENESIS '.app_state.poolincentives.lockable_durations.[0]' -v "120s"
    dasel put string -f $GENESIS '.app_state.poolincentives.lockable_durations.[1]' -v "180s"
    dasel put string -f $GENESIS '.app_state.poolincentives.lockable_durations.[2]' -v "240s"
    dasel put string -f $GENESIS '.app_state.poolincentives.params.minted_denom' -v "uosmo"

    # Update incentives module
    dasel put string -f $GENESIS '.app_state.incentives.lockable_durations.[0]' -v "1s"
    dasel put string -f $GENESIS '.app_state.incentives.lockable_durations.[1]' -v "120s"
    dasel put string -f $GENESIS '.app_state.incentives.lockable_durations.[2]' -v "180s"
    dasel put string -f $GENESIS '.app_state.incentives.lockable_durations.[3]' -v "240s"
    dasel put string -f $GENESIS '.app_state.incentives.params.distr_epoch_identifier' -v "hour"

    # Update mint module
    dasel put string -f $GENESIS '.app_state.mint.params.mint_denom' -v "uosmo"
    dasel put string -f $GENESIS '.app_state.mint.params.epoch_identifier' -v "hour"

    # Update poolmanager module
    dasel put string -f $GENESIS '.app_state.poolmanager.params.pool_creation_fee.[0].denom' -v "uosmo"

    # Update txfee basedenom
    dasel put string -f $GENESIS '.app_state.txfees.basedenom' -v "uosmo"

    # Update wasm permission (Nobody or Everybody)
    dasel put string -f $GENESIS '.app_state.wasm.params.code_upload_access.permission' -v "Everybody"

    # Update concentrated-liquidity (enable pool creation)
    dasel put bool -f $GENESIS '.app_state.concentratedliquidity.params.is_permissionless_pool_creation_enabled' -v true

    # Enlarge Tx size from default 1MiB.
    dasel put int -v 4194304 -f "${CONFIG_FOLDER}/config.toml" -r toml mempool.max_tx_bytes

    # Enable CORS
    sed -i "s/enabled-unsafe-cors = false/enabled-unsafe-cors = true/" "$CONFIG_FOLDER/app.toml"
    sed -i "s/cors_allowed_origins = \[\]/cors_allowed_origins = \[\"\*\"\]/" "$CONFIG_FOLDER/config.toml"
    sed -E -i "/timeout_(propose|prevote|precommit|commit)/s/[0-9]+m?s/260ms/" "$CONFIG_FOLDER/config.toml"

    # Increase max gas allowed in mempool
    sed -i 's/max-gas-wanted-per-tx = ".*"/max-gas-wanted-per-tx = "30000000"/' "$CONFIG_FOLDER/app.toml"

    dasel put int -v 100000000 -f "${CONFIG_FOLDER}/app.toml" -r toml api.rpc-max-body-bytes
    dasel put int -v 100000000 -f "${CONFIG_FOLDER}/config.toml" -r toml rpc.max_body_bytes
}

add_genesis_accounts () {
    osmosisd add-genesis-account osmo12smx2wdlyttvyzvzg54y2vnqwq2qjateuf7thj 100000000000uosmo,100000000000uion,100000000000stake,100000000000uusdc,100000000000uweth --home $OSMOSIS_HOME
    # note such large amounts are set for e2e tests on FE
    osmosisd add-genesis-account osmo1cyyzpxplxdzkeea7kwsydadg87357qnahakaks 9999999999999999999999999999999999999999999999999uosmo,9999999999999999999999999999999999999999999999999uion,100000000000stake,100000000000uusdc,100000000000uweth --home $OSMOSIS_HOME
    osmosisd add-genesis-account osmo18s5lynnmx37hq4wlrw9gdn68sg2uxp5rgk26vv 1000000000000000uosmo,1000000000000000uion,1000000000000000stake,1000000000000000uusdc,1000000000000000uweth --home $OSMOSIS_HOME
    osmosisd add-genesis-account osmo1qwexv7c6sm95lwhzn9027vyu2ccneaqad4w8ka 1000000000000000uosmo,1000000000000000uion,1000000000000000stake,1000000000000000uusdc,1000000000000000uweth --home $OSMOSIS_HOME
    osmosisd add-genesis-account osmo14hcxlnwlqtq75ttaxf674vk6mafspg8xwgnn53 1000000000000000uosmo,1000000000000000uion,1000000000000000stake,1000000000000000uusdc,1000000000000000uweth --home $OSMOSIS_HOME
    osmosisd add-genesis-account osmo12rr534cer5c0vj53eq4y32lcwguyy7nndt0u2t 1000000000000000uosmo,1000000000000000uion,1000000000000000stake,1000000000000000uusdc,1000000000000000uweth --home $OSMOSIS_HOME
    osmosisd add-genesis-account osmo1nt33cjd5auzh36syym6azgc8tve0jlvklnq7jq 1000000000000000uosmo,1000000000000000uion,1000000000000000stake,1000000000000000uusdc,1000000000000000uweth --home $OSMOSIS_HOME
    osmosisd add-genesis-account osmo10qfrpash5g2vk3hppvu45x0g860czur8ff5yx0 1000000000000000uosmo,1000000000000000uion,1000000000000000stake,1000000000000000uusdc,1000000000000000uweth --home $OSMOSIS_HOME
    osmosisd add-genesis-account osmo1f4tvsdukfwh6s9swrc24gkuz23tp8pd3e9r5fa 1000000000000000uosmo,1000000000000000uion,1000000000000000stake,1000000000000000uusdc,1000000000000000uweth --home $OSMOSIS_HOME
    osmosisd add-genesis-account osmo1myv43sqgnj5sm4zl98ftl45af9cfzk7nhjxjqh 1000000000000000uosmo,1000000000000000uion,1000000000000000stake,1000000000000000uusdc,1000000000000000uweth --home $OSMOSIS_HOME
    osmosisd add-genesis-account osmo14gs9zqh8m49yy9kscjqu9h72exyf295afg6kgk 1000000000000000uosmo,1000000000000000uion,1000000000000000stake,1000000000000000uusdc,1000000000000000uweth --home $OSMOSIS_HOME
    osmosisd add-genesis-account osmo1jllfytsz4dryxhz5tl7u73v29exsf80vz52ucc 10000000000000000uosmo,10000000000000000uion,10000000000000000stake,10000000000000000uusdc,10000000000000000uweth --home $OSMOSIS_HOME
    osmosisd add-genesis-account osmo1ssnwszx2dutvlrpwwgefpfyswgrrm059d5mcfe 10000000000000000uosmo,10000000000000000uion,10000000000000000stake,10000000000000000uusdc,10000000000000000uweth --home $OSMOSIS_HOME

    echo $MNEMONIC | osmosisd keys add $MONIKER --recover --keyring-backend=test --home $OSMOSIS_HOME
    echo $POOLSMNEMONIC | osmosisd keys add pools --recover --keyring-backend=test --home $OSMOSIS_HOME
    echo $MNEMONIC_TEST1 | osmosisd keys add lo-test1 --recover --keyring-backend=test --home $OSMOSIS_HOME
    echo $MNEMONIC_TEST2 | osmosisd keys add lo-test2 --recover --keyring-backend=test --home $OSMOSIS_HOME
    osmosisd gentx $MONIKER 500000000uosmo --keyring-backend=test --chain-id=$CHAIN_ID --home $OSMOSIS_HOME

    osmosisd collect-gentxs --home $OSMOSIS_HOME
}

edit_config () {

    # Remove seeds
    dasel put string -f $CONFIG_FOLDER/config.toml '.p2p.seeds' -v ''

    # Expose the rpc
    dasel put string -f $CONFIG_FOLDER/config.toml '.rpc.laddr' -v "tcp://0.0.0.0:26657"

    # Expose pprof for debugging
    # To make the change enabled locally, make sure to add 'EXPOSE 6060' to the root Dockerfile
    # and rebuild the image.
    dasel put string -f $CONFIG_FOLDER/config.toml '.rpc.pprof_laddr' -v "0.0.0.0:6060"
}

enable_cors () {
    # Enable cors on RPC
    dasel put string -f $CONFIG_FOLDER/config.toml -v "*" '.rpc.cors_allowed_origins.[]'
    dasel put string -f $CONFIG_FOLDER/config.toml -v "Accept-Encoding" '.rpc.cors_allowed_headers.[]'
    dasel put string -f $CONFIG_FOLDER/config.toml -v "DELETE" '.rpc.cors_allowed_methods.[]'
    dasel put string -f $CONFIG_FOLDER/config.toml -v "OPTIONS" '.rpc.cors_allowed_methods.[]'
    dasel put string -f $CONFIG_FOLDER/config.toml -v "PATCH" '.rpc.cors_allowed_methods.[]'
    dasel put string -f $CONFIG_FOLDER/config.toml -v "PUT" '.rpc.cors_allowed_methods.[]'

    # Enable unsafe cors and swagger on the api
    dasel put bool -f $CONFIG_FOLDER/app.toml -v "true" '.api.swagger'
    dasel put bool -f $CONFIG_FOLDER/app.toml -v "true" '.api.enabled-unsafe-cors'

    # Enable cors on gRPC Web
    dasel put bool -f $CONFIG_FOLDER/app.toml -v "true" '.grpc-web.enable-unsafe-cors'

    # Enable SQS & route caching
    dasel put string -f $CONFIG_FOLDER/app.toml -v "false" '.osmosis-sqs.is-enabled'
    dasel put string -f $CONFIG_FOLDER/app.toml -v "false" '.osmosis-sqs.route-cache-enabled'

    dasel put string -f $CONFIG_FOLDER/app.toml -v "redis" '.osmosis-sqs.db-host'

    # Bind it to 0.0.0.0
    sed -i 's/localhost/0.0.0.0/' "$CONFIG_FOLDER/app.toml"
}

if [[ ! -d $CONFIG_FOLDER ]]
then
    echo $MNEMONIC | osmosisd init -o --chain-id=$CHAIN_ID --home $OSMOSIS_HOME --recover $MONIKER
    edit_genesis
    add_genesis_accounts
    edit_config
    enable_cors
fi

osmosisd start --home $OSMOSIS_HOME &

wait
