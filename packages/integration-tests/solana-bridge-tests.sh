#!/usr/bin/env bash
set -euo pipefail

(cd ../../solana; just solana-test-validator)
trap 'rm -rf test-ledger/; echo; echo "Shutting down solana-test-validator"; killall solana-test-validator' EXIT

RUST_LOG=info,kolme=debug,six_sigma=debug cargo t $1 -- --ignored --nocapture
