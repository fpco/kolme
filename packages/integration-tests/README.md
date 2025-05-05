# Kolme integration tests

Prerequisites for running the tests:
 - Docker
 - [Just command runner](https://just.systems/man/en/packages.html)
 - The [Cosmos CLI](https://github.com/fpco/cosmos-rs/tree/main)
 - [Solana dev environment](https://solana.com/docs/intro/installation)

All tests currently rely on localosmosis. To set that up before running any of the tests run:
 - First, in the root of the repo execute `just build-contracts`
 - `docker compose up -d`
 - `COSMOS_NETWORK=osmosis-local cosmos contract store-code --wallet "notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius" ../../artifacts/kolme_cosmos_bridge.wasm`

## Six Sigma
To run the six sigma test execute `just run-tests`. Currently the test doesn't cleanup
application DB so subsequent runs could fail as a workaround test could be run with DB
dropped using `just drop-db run-tests`

## Solana-Cosmos bridge
To run the Solana-Cosmos bridge test execute the `solana-bridge-tests.sh` script.
