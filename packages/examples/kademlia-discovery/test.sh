#!/usr/bin/env bash

set -euxo pipefail

cargo build --release

docker rm -f kademlia-test-validators
trap "docker rm -f kademlia-test-validators" EXIT
docker run --rm -d --name kademlia-test-validators -p 5400:5400 -v "$(pwd)/../../../target/release":/host:ro ubuntu /host/kademlia-discovery validators 5400
cargo run --release client /ip4/127.0.0.1/udp/5400/quic-v1
