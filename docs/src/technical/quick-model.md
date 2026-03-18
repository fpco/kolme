# Quick Model

High-level model; implementation details may vary by chain/component.

Kolme is an appchain framework: each app runs on its own public chain, with app logic in deterministic Rust.

- Transactions are gossiped through the network mempool, and the processor validates signature + nonce, executes, and emits 1 transaction per block on demand. Ordering is processor policy (typically arrival-order + nonce validity), not a global fee-market auction.
- The processor is a single logical block-producer role (HA cluster with operator-controlled failover; only one active signer at a time), while other nodes can re-execute blocks to verify correctness.
- Block production is processor-authoritative for liveness; safety is enforced operationally via quorum governance (e.g., key rotation/replacement), not autonomous fork competition.
- State is split into framework state (balances, nonces, validator sets, proposals, chain version) and app state; both are hash-committed in blocks and updated atomically per transaction.
- Security roles: listeners (observe external-chain bridge events), processor (executes/blocks), approvers (authorize outbound bridge actions), submitters (best-effort relayers that broadcast approved actions externally).
- Listeners and approvers are quorum multisig sets using aggregated individual signatures (not threshold crypto), with chain-specific finality handling (e.g., Solana finalized commitment); there is no generic configurable N-block confirmation-depth rule in current code.
- Inbound bridge flow: external event -> listener quorum confirmation -> processor ingests and executes on Kolme.
- Outbound bridge flow: Kolme emits ordered bridge actions -> approver quorum signs -> submitter relays to external chain.
- Admin/security changes require 2 of 3 validator groups (processor, listeners, approvers).
- Core app execution is independent of external-chain liveness; outages mainly delay deposits/withdrawals and bridge settlement.
- Trust model (high level): safety depends on quorum honesty in listener/approver groups plus operator control of processor key lifecycle.
