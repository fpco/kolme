default:
    just --list

check:
    cargo check --workspace --tests --all-features

clippy:
    cargo clippy --no-deps --workspace --tests -- -Dwarnings

fmt:
    cargo fmt --all --check

lint: fmt check clippy

stop-postgres:
	docker stop kolme_pg && docker rm kolme_pg

postgres:
	-just stop-postgres
	docker run --name kolme_pg -d -it --cpus="0.5" --memory="512m" -e POSTGRES_PASSWORD=postgres -p 45921:5432 postgres:15.3-alpine
	sleep 3	# To resolve issue in CI
	cd packages/kolme-store && sqlx database reset -y

stop-localosmosis:
	docker stop localosmosis && docker rm localosmosis

localosmosis:
	-just stop-localosmosis
	docker run --name localosmosis -d -it --cpus="1" --memory="512m" -p 26657:26657 -p 1317:1317 -p 9090:9090 -p 9091:9091 ghcr.io/fpco/cosmos-images/localosmosis:3703be0654109bd04d6e4e1f7d2707ea905a28eb

test:
    cargo run --locked --bin test-runner

[working-directory: "packages/kolme-store"]
sqlx-prepare $DATABASE_URL="postgres://postgres:postgres@localhost:45921/postgres": postgres
    # TODO: On my end I need this so that docker has time to launch the container
    sleep 3
    cargo sqlx database reset -y
    cargo sqlx migrate run
    cargo sqlx prepare

build-contracts:
	./.ci/build-contracts.sh

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

changelog:
    git-cliff -c .git-cliff.toml -o new-changelog.md --tag-pattern "v[0-9]*"

# cargo compile
cargo-compile:
	cargo test --workspace --no-run --locked

# cargo test
cargo-test:
	cat contract-test-list.txt stress-test-list.txt | xargs -I {} echo --skip {} | xargs cargo test --workspace --locked --

# Contract related tests
cargo-contract-tests:
	xargs -a contract-test-list.txt cargo test --workspace --locked --
