LOCALOSMOSIS_VERSION := "a013a07d2bbff37bb72b6c3134854c7622666d84"
POSTGRES_VERSION := "15.3-alpine"

default:
    just --list --unsorted

check:
    cargo check --workspace --tests --all-features

clippy:
    cargo clippy --no-deps --workspace --tests -- -Dwarnings

fmt:
    cargo fmt --all --check

lint: fmt check clippy

stop-postgres:
	docker stop kolme_pg && docker rm kolme_pg

psql:
    env PGPASSWORD=postgres psql -U postgres -h localhost -p 45921

postgres $DATABASE_URL="postgres://postgres:postgres@localhost:45921/postgres":
	-just stop-postgres
	docker run --name kolme_pg -d -it --cpus="0.5" --memory="512m" -e POSTGRES_PASSWORD=postgres -p 45921:5432 postgres:{{POSTGRES_VERSION}}
	sleep 3	# To resolve issue in CI
	cd packages/kolme-store && sqlx database reset -y

stop-localosmosis:
	docker stop localosmosis && docker rm localosmosis

localosmosis:
	-just stop-localosmosis
	docker run --name localosmosis -d -it --cpus="1" --memory="512m" -p 26657:26657 -p 1317:1317 -p 9090:9090 -p 9091:9091 ghcr.io/fpco/cosmos-images/localosmosis:{{LOCALOSMOSIS_VERSION}}

test $PROCESSOR_BLOCK_DB="psql://postgres:postgres@localhost:45921/postgres":
	just postgres
	just localosmosis
	just cargo-test
	just cargo-contract-tests
	just cargo-slow-tests

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

[working-directory: "packages/kolme"]
run-store-tests $PROCESSOR_BLOCK_DB="postgres://postgres:postgres@localhost:45921/postgres": sqlx-prepare
    cargo test --test store

changelog:
    git-cliff -c .git-cliff.toml -o new-changelog.md --tag-pattern "v[0-9]*"

# cargo compile
cargo-compile:
	cargo test --workspace --no-run --locked

# Cargo test
cargo-test:
	cat contract-test-list.txt stress-test-list.txt | xargs -I {} echo --skip {} | xargs cargo nextest run --workspace --locked --

# Contract related tests
cargo-contract-tests:
	xargs -a contract-test-list.txt cargo nextest run --workspace --profile=ci --locked --

# Slow tests
cargo-slow-tests:
	xargs -a stress-test-list.txt cargo nextest run --workspace --locked --

# Cache docker images by saving it under wasm
cache-docker-images:
	mkdir -p wasm/images
	-[ -f wasm/images/localosmosis_{{LOCALOSMOSIS_VERSION}}.tar ] || docker pull ghcr.io/fpco/cosmos-images/localosmosis:{{LOCALOSMOSIS_VERSION}} && docker save ghcr.io/fpco/cosmos-images/localosmosis:{{LOCALOSMOSIS_VERSION}} > wasm/images/localosmosis_{{LOCALOSMOSIS_VERSION}}.tar
	-[ -f wasm/images/postgres_{{POSTGRES_VERSION}}.tar ] || docker pull postgres:{{POSTGRES_VERSION}} && docker save postgres:{{POSTGRES_VERSION}} > wasm/images/postgres_{{POSTGRES_VERSION}}.tar
	-docker load -i ./wasm/images/localosmosis_{{LOCALOSMOSIS_VERSION}}.tar
	-docker load -i ./wasm/images/postgres_{{POSTGRES_VERSION}}.tar
