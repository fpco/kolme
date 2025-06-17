# Watchdogs

<!-- toc -->

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

Watchdogs provide transparent reporting to maintain trust and facilitate rapid issue resolution.

- **Alerts**:
  - Broadcast signed alerts via libp2p gossip for detected issues (e.g., invalid blocks, quorum failures), informing nodes and users, per [Gossip](gossip.md).
  - Alerts are recorded on-chain for auditability, enhancing transparency, per [Core Chain Mechanics](core-mechanics.md).
- **User and Validator Feedback**:
  - Notifications reach user interfaces via API servers, enabling corrective actions, per [High Availability](high-availability.md).
  - Validators (processor, listeners, approvers) receive alerts to address issues, per [Triadic Security Model](triadic-security.md).
- **Security**:
  - Alerts use cryptographic signatures for authenticity, verified by nodes, per [External Data Handling](external-data.md).

## Advantages

Kolme’s watchdog system offers several benefits for developers building secure, multichain applications:

- **Enhanced Security**: Detects and flags validator errors or misbehavior, ensuring trust, per [Triadic Security Model](triadic-security.md).
- **Transparency**: On-chain alerts and verifiable reports maintain auditability, per [Core Chain Mechanics](core-mechanics.md).
- **Resilience**: Independent operation ensures monitoring persists during failures, supporting zero-downtime apps, per [High Availability](high-availability.md).
- **Multichain Integrity**: Validates bridge events and chain migrations (e.g., Cosmos), per [External Chain Resilience](external-chain-resilience.md).
- **Proactive Monitoring**: Early detection of issues like censorship or storage errors improves reliability, per [Storage](storage.md).
- **Scalability**: Supports high-throughput apps with minimal overhead, per [Node Synchronization](node-sync.md).

These advantages make Kolme a robust platform for building high-performance, fault-tolerant decentralized applications.