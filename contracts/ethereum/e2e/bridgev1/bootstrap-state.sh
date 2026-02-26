#!/usr/bin/env bash
set -euo pipefail

on_error() {
  local line="$1"
  local cmd="$2"
  echo "bootstrap-state.sh failed at line ${line}: ${cmd}" >&2
}
trap 'on_error "${LINENO}" "${BASH_COMMAND}"' ERR

mkdir -p /bootstrap
printf "%s" "${ANVIL_MNEMONIC}" > /bootstrap/mnemonic.txt

MNEMONIC="$(cat /bootstrap/mnemonic.txt)"
if ! cast wallet address --mnemonic "$MNEMONIC" --mnemonic-index 0 >/dev/null 2>&1; then
  echo "Invalid ANVIL_MNEMONIC: must be a valid BIP-39 mnemonic phrase." >&2
  exit 1
fi
DEPLOYER_ADDRESS="$(cast wallet address --mnemonic "$MNEMONIC" --mnemonic-index 0)"
DEPLOYER_PRIVATE_KEY="$(cast wallet private-key --mnemonic "$MNEMONIC" --mnemonic-index 0)"
VALIDATOR_KEY="0x021111111111111111111111111111111111111111111111111111111111111111"

BYTECODE="$(jq -r '.bytecode.object' out/Bridge.sol/BridgeV1.json)"
CTOR_ARGS="$(
  cast abi-encode \
    "constructor(address,bytes,bytes[],uint16,bytes[],uint16,uint64,uint64)" \
    "$DEPLOYER_ADDRESS" \
    "$VALIDATOR_KEY" \
    "[$VALIDATOR_KEY]" \
    "1" \
    "[$VALIDATOR_KEY]" \
    "1" \
    "0" \
    "0"
)"
echo -n "${BYTECODE}${CTOR_ARGS#0x}" > /tmp/bridgev1.initcode
cast compute-address --nonce 0 "$DEPLOYER_ADDRESS" > /bootstrap/bridgev1.address

anvil \
  --host 127.0.0.1 \
  --port 8545 \
  --chain-id 31337 \
  --mnemonic "$MNEMONIC" \
  --state /bootstrap/state.json \
  --state-interval 1 \
  >/tmp/anvil.log 2>&1 &
ANVIL_PID=$!
trap 'kill -INT "$ANVIL_PID" || true' EXIT

sleep 5

cast send \
  --rpc-url http://127.0.0.1:8545 \
  --private-key "$DEPLOYER_PRIVATE_KEY" \
  --create "$(cat /tmp/bridgev1.initcode)"

sleep 2
kill -INT "$ANVIL_PID" || true
sleep 1
test -f /bootstrap/state.json
