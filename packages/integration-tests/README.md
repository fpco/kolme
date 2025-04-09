To run the six-sigma test start localosmosis in Docker first with `docker compose up -d`
then you need to deploy the contract using
```
COSMOS_NETWORK=local cosmos contract store-code --wallet "notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius" ../artifacts/kolme_cosmos_bridge.wasm
```
this assumes that you have the following config for localosmosis:
```
[network.local]
grpc = "http://127.0.0.1:9090"
chain-id = "localosmosis"
gas-coin = "uosmo"
hrp = "osmo"
```
in `~/.config/cosmos-rs/config.toml`.

could run the test using `just run-tests`. Currently the test doesn't cleanup
application DB so subsequent runs could fail as a workaround test could be run with DB
dropped using `just drop-db run-tests`
