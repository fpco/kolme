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

test: postgres
    cargo sqlx database reset -y --source packages/kolme/migrations
    cargo sqlx migrate run --source packages/kolme/migrations
    cargo sqlx prepare --workspace
    PROCESSOR_BLOCK_DB=psql://postgres:postgres@localhost:45921/postgres cargo test

build-optimizer-image:
    ./.ci/build-optimizer-image.sh

build-contracts:
    docker run --rm -v "$(pwd)":/code \
      --mount type=volume,source="$(basename "$(pwd)")_cache",target=/target \
      --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
      ghcr.io/fpco/kolme/cosmwasm-optimizer:1.84
