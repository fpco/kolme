check:
    cargo check --workspace --tests

clippy:
    cargo clippy --no-deps --workspace --tests -- -Dwarnings

fmt:
    cargo fmt --all --check

lint: fmt check clippy

test:
    cargo sqlx database reset -y --source kolme/migrations
    cargo sqlx migrate run --source kolme/migrations
    cargo sqlx prepare --workspace
    cargo test

build-contracts:
    docker run --rm -v "$(pwd)":/code \
      --mount type=volume,source="$(basename "$(pwd)")_cache",target=/target \
      --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
      cosmwasm/optimizer:0.16.1
