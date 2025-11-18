# Watchdogs

<!-- toc -->

> Status: planned. The watchdog roles described below are not yet implemented; they outline the intended responsibilities for a future monitoring component.

## Monitoring Role

Kolme’s planned watchdog nodes enhance network integrity by monitoring validator behavior and system operations across multichain ecosystems like Solana, Ethereum, and Near.

- **Function**:
  - Observe processor outputs (blocks, failure notifications), listener confirmations, and approver authorizations to detect anomalies (e.g., censorship, invalid state hashes), as detailed in [Core Chain Mechanics](core-mechanics.md).
  - Validate transactions, data loads, and state transitions, ensuring compliance with protocol rules, per [Triadic Security Model](triadic-security.md).
- **Scope**:
  - Monitor gossip messages (e.g., transactions, blocks) for consistency and fairness, per [Gossip](gossip.md).
  - Verify bridge contract events (e.g., deposits, withdrawals) to maintain multichain trust, per [External Chain Resilience](external-chain-resilience.md).
- **Operation**:
  - Run independently, syncing via slow or fast sync to access chain state, per [Node Synchronization](node-sync.md).
  - Report discrepancies to the network or external auditors, enhancing transparency.

## Detection Capabilities

Watchdogs are designed to identify and flag potential issues, ensuring robust security and reliability.

- **Processor Errors**:
  - Detect invalid blocks (e.g., incorrect state hashes, unsigned transactions) by re-executing transactions, per [External Data Handling](external-data.md).
  - Flag censorship if valid transactions are excluded from blocks, per [Failed Transactions](failed-transactions.md).
- **Validator Misbehavior**:
  - Identify inconsistencies in listener confirmations or approver authorizations (e.g., quorum violations), per [Triadic Security Model](triadic-security.md).
  - Monitor key rotation and version upgrade proposals for unauthorized actions, per [Key Rotation](key-rotation.md) and [Version Upgrades](version-upgrades.md).
- **Multichain Issues**:
  - Verify bridge contract event processing to prevent fraudulent deposits or withdrawals, supporting Solana, Ethereum, Near, or new chains like Aptos, per [External Chain Resilience](external-chain-resilience.md).
  - Ensure chain migrations maintain integrity, per [Node Synchronization](node-sync.md).
- **Storage and Sync**:
  - Check storage consistency (e.g., MerkleMap state) and sync processes for errors, per [Storage](storage.md) and [Node Synchronization](node-sync.md).

## Reporting Mechanism

The reporting flow for watchdog findings is still being designed. The current expectation is:

- **Alerts**:
  - Broadcast signed alerts via libp2p gossip for detected issues (e.g., invalid blocks, quorum failures), informing nodes and users, per [Gossip](gossip.md).
  - Depending on how the feature is implemented, alerts may later be written on-chain for auditability.
- **User and Validator Feedback**:
  - Notifications would reach user interfaces via API servers.
  - Validators (processor, listeners, approvers) would receive alerts to address issues, per [Triadic Security Model](triadic-security.md).
- **Security**:
  - Alerts use cryptographic signatures for authenticity, verified by nodes, per [External Data Handling](external-data.md).

## Advantages

If implemented as planned, watchdogs would provide:

- **Enhanced Security**: Detect and flag validator errors or misbehavior, ensuring trust, per [Triadic Security Model](triadic-security.md).
- **Transparency**: Broadcast signed alerts, with potential on-chain recording for auditability, per [Core Chain Mechanics](core-mechanics.md).
- **Resilience**: Operate independently so monitoring persists during failures, per [High Availability](high-availability.md).
- **Multichain Integrity**: Validate bridge events and chain migrations (e.g., Cosmos), per [External Chain Resilience](external-chain-resilience.md).
- **Proactive Monitoring**: Surface issues like censorship or storage errors early, per [Storage](storage.md).
- **Scalability**: Support high-throughput apps with minimal overhead, per [Node Synchronization](node-sync.md).
