default:
    just --list

check:
    cargo check --workspace --tests --all-features

clippy:
    cargo clippy --no-deps --workspace --tests -- -Dwarnings

fmt:
    cargo fmt --all --check

lint: fmt check clippy

postgres:
    docker compose -f ./packages/integration-tests/docker-compose.yml down
    docker compose -f ./packages/integration-tests/docker-compose.yml up -d postgres

test:
    cargo run --bin test-runner

[working-directory: "packages/kolme-store-postgresql"]
sqlx-prepare $DATABASE_URL="postgres://postgres:postgres@localhost:45921/postgres": postgres
    # TODO: On my end I need this so that docker has time to launch the container
    sleep 3
    cargo sqlx database reset -y
    cargo sqlx migrate run
    cargo sqlx prepare

build-optimizer-image:
    ./.ci/build-optimizer-image.sh

build-contracts:
    docker run --rm -v "$(pwd)":/code \
      --mount type=volume,source="$(basename "$(pwd)")_cache",target=/target \
      --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
      ghcr.io/fpco/kolme/cosmwasm-optimizer:1.84

[working-directory: "packages/integration-tests"]
drop-integration-tests-db:
    rm -rf six-sigma-app.fjall

[working-directory: "packages/integration-tests"]
setup-localosmo:
    cargo run --release --example setup-localosmo

[working-directory: "packages/integration-tests"]
run-integration-tests: setup-localosmo
    RUST_LOG=info,kolme=debug,six_sigma=debug cargo t -- --ignored --nocapture

[working-directory: "packages/kolme"]
run-store-tests $PROCESSOR_BLOCK_DB="postgres://postgres:postgres@localhost:45921/postgres": sqlx-prepare
    cargo test --test store
