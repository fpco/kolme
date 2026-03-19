# Bridge E2E (Anvil)

This setup runs a deterministic local Ethereum JSON-RPC node for end-to-end testing.
Container startup runs Anvil with a deterministic mnemonic.
The bridge is not pre-deployed; tests or smoke checks should deploy it explicitly.

## Start

```bash
cd e2e
docker compose -f compose.yaml up -d --build
```

## Stop

```bash
cd e2e
docker compose -f compose.yaml down
```

## Rebuild

```bash
cd e2e
docker compose -f compose.yaml build --no-cache
```
Note: image build compiles contracts internally with `forge build`.

## Quick Check

```bash
cast block-number --rpc-url http://localhost:8545
cast code <bridge_contract_address> --rpc-url http://localhost:8545
```

## Notes for Kolme

- Configure Kolme's Ethereum RPC client to point at `http://host.docker.internal:8545` when Kolme itself runs in Docker.
- Use `http://localhost:8545` when Kolme runs directly on the host.

## Deposit Flow

Deposits are submitted via `regular(tokens, amounts, keys)`.

### ERC20

1. Call `approve(<bridge_address>, <amount>)` on the token contract.
2. Call `regular([<token_address>], [<amount>], <keys>)` on the bridge contract with `msg.value = 0`.

### ETH

1. Call `regular([address(0)], [<amount_wei>], <keys>)` on the bridge contract with `msg.value = <amount_wei>`.

Notes:
- Plain ETH transfers to the bridge are unsupported and revert.
- `keys` are user secp256k1 pubkeys provided with the deposit event payload.

## Ethereum Denom Format (Kolme side)

- Native ETH denom: `eth` (lowercase).
- ERC20 denom: canonical lowercase EVM address (`0x...`).

## Relevant Identifiers

The values below are valid for this setup only when using this mnemonic:
```
test test test test test test test test test test test junk
```
Check the actual mnemonic in the `/bootstrap/mnemonic.txt` file.

- `<admin_address>`: `0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266`
- `<admin_private_key>`: `0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80`
