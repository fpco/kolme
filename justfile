mod kolme_test './packages/kolme-test/justfile'
mod store './packages/kolme-store/justfile'
mod benchmarks './packages/benchmarks/justfile'

# List all recipes
default:
    just --list --unsorted

# Clippy check
cargo-clippy-check:
    cargo clippy --no-deps --workspace --locked --tests --benches --examples -- -Dwarnings

# Rustfmt check
cargo-fmt-check:
    cargo fmt --all --check

# Format Rust code
fmt:
    cargo fmt --all

stop-postgres:
    docker stop kolme_pg && docker rm kolme_pg

psql:
    env PGPASSWORD=postgres psql -U postgres -h localhost -p 45921

postgres $DATABASE_URL="postgres://postgres:postgres@localhost:45921/postgres":
    -just stop-postgres
    docker run --name kolme_pg -d -it --cpus="0.5" --memory="512m" -e POSTGRES_PASSWORD=postgres -p 45921:5432 postgres:15.3-alpine
    sleep 3	# To resolve issue in CI
    cd packages/kolme-store && sqlx database reset -y

stop-localosmosis:
    docker stop localosmosis && docker rm localosmosis

localosmosis:
    -just stop-localosmosis
    docker run --name localosmosis -d -it --cpus="1" --memory="512m" -p 26657:26657 -p 1317:1317 -p 9090:9090 -p 9091:9091 ghcr.io/fpco/cosmos-images/localosmosis:a013a07d2bbff37bb72b6c3134854c7622666d84

test $PROCESSOR_BLOCK_DB="psql://postgres:postgres@localhost:45921/postgres":
    just postgres
    just localosmosis
    just cargo-test
    just cargo-contract-tests

sqlx-prepare $DATABASE_URL="postgres://postgres:postgres@localhost:45921/postgres": postgres
    just store::sqlx-prepare
    just kolme_test::sqlx-prepare
    just benchmarks::sqlx-prepare

# Build contracts
build-contracts:
    ./.ci/build-contracts.sh

[working-directory("packages/integration-tests")]
drop-integration-tests-db:
    rm -rf six-sigma-app.fjall

[working-directory("packages/kolme")]
run-store-tests $PROCESSOR_BLOCK_DB="postgres://postgres:postgres@localhost:45921/postgres": sqlx-prepare
    cargo test --test store

changelog:
    git-cliff -c .git-cliff.toml -o new-changelog.md --tag-pattern "v[0-9]*"

# cargo compile
cargo-compile:
    cargo test --workspace --no-run --locked

# Non contract test
cargo-test:
    cat contract-test-list.txt | xargs -I {} echo --skip {} | xargs cargo nextest run --workspace --locked --

# Contract related tests
cargo-contract-tests:
    xargs -a contract-test-list.txt cargo nextest run --workspace --profile=ci --locked --

# Stress test
stress-test:
    env KOLME_PROCESSOR_COUNT=10 KOLME_CLIENT_COUNT=100 LARGE_SYNC_PAYLOAD_SIZE=50000 cargo nextest run --workspace --locked -- multiple_processors large_sync

# Run exact test
run-exact-test target:
    cargo nextest run --no-capture -- --exact {{ target }}

# Run test
run-test target:
    cargo nextest run --no-fail-fast --no-capture -- {{ target }}

# Profile build timings
profile-build:
    cargo test --workspace --no-run --locked --timings

# Cargo inherit
inherit:
    cargo autoinherit --exclude-members solana

# Toml format
toml-format:
    tombi format
