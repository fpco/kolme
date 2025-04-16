check:
    cargo check --workspace --tests

clippy:
    cargo clippy --no-deps --workspace --tests -- -Dwarnings

fmt:
    cargo fmt --all --check

lint: fmt check clippy

test:
    cargo sqlx database reset -y --source packages/kolme/migrations
    cargo sqlx migrate run --source packages/kolme/migrations
    cargo sqlx prepare --workspace
    cargo test

build-optimizer-image:
    ./.ci/build-optimizer-image.sh

build-contracts: build-optimizer-image
    docker run --rm -v "$(pwd)":/code \
      --mount type=volume,source="$(basename "$(pwd)")_cache",target=/target \
      --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
      optimizer:1.84
