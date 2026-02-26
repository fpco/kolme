#!/bin/bash
set -e

MNEMONIC_FILE="${MNEMONIC_FILE:-/bootstrap/mnemonic.txt}"


if [ -f "$MNEMONIC_FILE" ]; then
  ANVIL_MNEMONIC="$(cat "$MNEMONIC_FILE")"
else
  ANVIL_MNEMONIC="test test test test test test test test test test test junk"
fi

if [[ "$1" == "show" ]]
then
  if [ -f /bootstrap/bridge.address ]; then
    echo "Bridge deployed address: $(cat /bootstrap/bridge.address)"
  fi

  echo "Mnemonic source: ${MNEMONIC_FILE}"
  echo "Derived accounts (index -> address / private key):"

  account_index=0
  while [ "$account_index" -lt "3" ]; do
    addr="$(cast wallet address --mnemonic "$ANVIL_MNEMONIC" --mnemonic-index "$account_index")"
    pk="$(cast wallet private-key --mnemonic "$ANVIL_MNEMONIC" --mnemonic-index "$account_index")"
    echo "  [$account_index] ${addr} / ${pk}"
    account_index=$((account_index + 1))
  done
  exit 0

elif [[ "$1" == "run" ]] || [[ "$1" == "" ]]
then
  exec anvil --mnemonic "$ANVIL_MNEMONIC" --host 0.0.0.0 --port 8545 --chain-id 31337 --load-state /bootstrap/state.json
fi

exec "$@"
