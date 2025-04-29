#!/usr/bin/env bash

nohup solana-test-validator > /dev/null &
SOL_VALIDATOR_PID=$!

echo "Waiting for solana-test-validator to start"
sleep 5

RUST_LOG=info,kolme=debug,six_sigma=debug cargo t bridge_transfer -- --ignored --nocapture

echo "Shutting down solana-test-validator"
kill $SOL_VALIDATOR_PID

rm -rf test-ledger/
rm example-solana-cosmos-bridge.sqlite3
