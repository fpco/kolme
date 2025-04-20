# example-six-sigma

This is a sample app developed with the Kolme framework.

## Building

```bash
export SQLX_OFFLINE=true
cargo build -p example-six-sigma
```

## Running

First we need to setup the localosmosis environment using docker compose

```
cd packages/integration-tests
docker compose up localosmosis
```

Deploy the bridge contract

```bash
just build-contracts
cosmos contract store-code ./artifacts/kolme_cosmos_bridge.wasm \
  --wallet "notice oak worry limit wrap speak medal online prefer cluster roof addict wrist behave treat actual wasp year salad speed social layer crew genius" \
  --cosmos-grpc grpc://localhost:9090 \
  --chain-id localosmosis \
  --gas-coin uosmo \
  --hrp osmo
```

Then we can run each Kolme service separately (the order does not matter).

```bash
cargo run --bin example-six-sigma serve processor
cargo run --bin example-six-sigma serve listener
cargo run --bin example-six-sigma serve approver
cargo run --bin example-six-sigma serve submitter
cargo run --bin example-six-sigma serve api-server
```

Or all at once with the following

```bash
cargo run --bin example-six-sigma serve
```
