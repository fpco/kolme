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
    PROCESSOR_BLOCK_DB=psql://postgres:postgres@localhost:45921/postgres cargo test

sqlx-prepare: sqlx-prepare-sqlite sqlx-prepare-postgres

[working-directory: "packages/kolme-store-sqlite"]
sqlx-prepare-sqlite $DATABASE_URL="sqlite:///tmp/kolme-prepare-db.sqlite3":
    cargo sqlx database reset -y
    cargo sqlx migrate run
    cargo sqlx prepare

[working-directory: "packages/kolme-store-postgresql"]
sqlx-prepare-postgres $DATABASE_URL="postgres://postgres:postgres@localhost:45921/postgres": postgres
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
