# BridgeV1 E2E (Anvil)

This setup runs a deterministic local Ethereum JSON-RPC node for end-to-end testing.
During image build, `BridgeV1` is deployed and Anvil state is snapshotted.
Container startup loads that snapshot, so the contract is already deployed.

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

## Rebuild (redeploy contract into snapshot)

```bash
cd e2e
docker compose -f compose.yaml build --no-cache
```
Note: image build compiles contracts internally with `forge build` before deployment.

## Quick Check

```bash
cast block-number --rpc-url http://localhost:8545
cast code <bridge_contract_address> --rpc-url http://localhost:8545
```

## Notes for Kolme

- Configure Kolme's Ethereum RPC client to point at `http://host.docker.internal:8545` when Kolme itself runs in Docker.
- Use `http://localhost:8545` when Kolme runs directly on the host.

## Relevant Identifiers

The values below are valid for this setup only when using this mnemonic:
```
test test test test test test test test test test test junk
```
Check the actual mnemonic in the `/bootstrap/mnemonic.txt` file.

- `<admin_address>`: `0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266`
- `<admin_private_key>`: `0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80`
- `<bridge_contract_address>`: `0x5FbDB2315678afecb367f032d93F642f64180aa3`
