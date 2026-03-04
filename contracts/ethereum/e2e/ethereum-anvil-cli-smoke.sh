#!/usr/bin/env bash
set -euo pipefail

MNEMONIC_FILE="${MNEMONIC_FILE:-/bootstrap/mnemonic.txt}"

if [[ ! -f "${MNEMONIC_FILE}" ]]; then
  echo "Mnemonic file not found: ${MNEMONIC_FILE}"
  exit 1
fi

if [[ ! -f /bootstrap/bridge.address ]]; then
  echo "Bridge address file not found: /bootstrap/bridge.address"
  exit 1
fi

MNEMONIC="$(cat "${MNEMONIC_FILE}")"
BRIDGE_ADDRESS="$(cat /bootstrap/bridge.address)"

echo "Verify RPC is reachable"
cast block-number >/dev/null

echo "Verify bridge contract code is deployed"
CODE="$(cast code "${BRIDGE_ADDRESS}")"
if [[ "${CODE}" == "0x" ]]; then
  echo "No contract code found at ${BRIDGE_ADDRESS}"
  exit 1
fi

echo "Verify get_config() call works"
cast call \
  "${BRIDGE_ADDRESS}" \
  "get_config()(bytes,bytes[],uint16,bytes[],uint16,uint64,uint64)" \
  >/dev/null

echo "Execute action 0 with valid signatures"
payload="$(cast abi-encode "x(uint64,bytes)" "0" "0x6e6f6f70")"
payload_hash="0x$(printf '%b' "$(echo "${payload#0x}" | sed 's/../\\x&/g')" | sha256sum | awk '{print $1}')"
processor_sig="$(cast wallet sign --mnemonic "${MNEMONIC}" --mnemonic-index 0 --no-hash "${payload_hash}")"
approver_sig="$(cast wallet sign --mnemonic "${MNEMONIC}" --mnemonic-index 1 --no-hash "${payload_hash}")"

execute_tx_hash="$(
  cast send \
    "${BRIDGE_ADDRESS}" \
    "execute(bytes,bytes,bytes[])" \
    "${payload}" \
    "${processor_sig}" \
    "[${approver_sig}]" \
    --mnemonic "${MNEMONIC}" \
    --mnemonic-index 2 \
    --json | jq -r '.transactionHash'
)"

if [[ -z "${execute_tx_hash}" || "${execute_tx_hash}" == "null" ]]; then
  echo "Failed to submit execute transaction"
  exit 1
fi

echo "Verify replay of same action is rejected"
if cast send \
  "${BRIDGE_ADDRESS}" \
  "execute(bytes,bytes,bytes[])" \
  "${payload}" \
  "${processor_sig}" \
  "[${approver_sig}]" \
  --mnemonic "${MNEMONIC}" \
  --mnemonic-index 2 >/dev/null 2>&1; then
  echo "Second execute unexpectedly succeeded; nextActionId did not advance"
  exit 1
fi

echo "Ethereum Anvil CLI execute smoke check passed for ${BRIDGE_ADDRESS} in tx ${execute_tx_hash}"
