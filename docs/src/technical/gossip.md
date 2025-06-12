# Gossip

<!-- toc -->

## Libp2p Integration

Kolme leverages libp2p, a peer-to-peer networking framework, to enable efficient, decentralized communication between nodes.

- **Role**:
  - Facilitates broadcasting of proposed transactions, produced blocks, and other notifications between nodes (processor, listeners, approvers, community nodes), as detailed in [Core Chain Mechanics](core-mechanics.md).
  - Ensures reliable message delivery for validator coordination and chain synchronization, per [Triadic Security Model](triadic-security.md).
- **Configuration**:
  - Nodes establish peer connections using libp2p’s discovery mechanisms, forming a gossip network for real-time data exchange.
- **Security**:
  - Relies on cryptographic signatures for all transactions, blocks, and security messages (e.g., listener confirmations, approver authorizations) to ensure authenticity and integrity.

## Message Types

Kolme’s gossip network handles several message types critical for chain operation and coordination, ensuring robust, transparent communication.

- **Transactions**:
  - Users submit signed transactions (containing messages, nonces, signatures) to the mempool via libp2p, where the processor selects them for block inclusion, as described in [Core Chain Mechanics](core-mechanics.md).
  - Broadcast to all nodes for validation and mempool synchronization, supporting high-throughput apps.
- **Blocks**:
  - Processor broadcasts signed blocks (containing transactions, data loads, logs, state hashes) to all nodes for validation, per [Triadic Security Model](triadic-security.md).
  - Ensures nodes stay synchronized, supporting slow and fast sync processes, per [Node Synchronization](node-sync.md).
- **Failure Notifications**:
  - Processor broadcasts signed notifications for failed transactions (e.g., invalid nonce, data fetch errors), informing users and nodes, as outlined in [Failed Transactions](failed-transactions.md).
  - Planned watchdogs will monitor notifications for fairness, per [Watchdogs](watchdogs.md).
- **Validator Messages**:
  - Listeners broadcast signed confirmations of external chain events (e.g., deposits), and approvers broadcast withdrawal authorizations, processed as transactions, per [External Chain Resilience](external-chain-resilience.md).
  - Supports quorum-based approvals for administrative tasks (e.g., upgrades, key rotations), per [Version Upgrades](version-upgrades.md) and [Key Rotation](key-rotation.md).

## Network Resilience

Kolme’s gossip network is designed for resilience, ensuring continuous operation even under adverse conditions.

- **Peer Redundancy**:
  - Nodes maintain multiple peer connections, ensuring message delivery despite node failures or network partitions, per [High Availability](high-availability.md).
- **Message Retransmission**:
  - Libp2p retransmits messages to ensure delivery, handling temporary network issues without data loss.
  - Critical for validator messages (e.g., listener confirmations) to maintain quorum-based security, per [Triadic Security Model](triadic-security.md).
- **Bandwidth Optimization**:
  - Gossip protocol minimizes redundant transmissions by propagating messages efficiently across the network, supporting high-throughput apps with minimal overhead.
  - Lightweight block downloads (excluding state data) further optimize bandwidth, per [Node Synchronization](node-sync.md).
  - Bandwidth-intensive operations are sent directly to other nodes via libp2p's request/response mechanism, avoiding spamming the entire network with broadcast messages.

## Advantages

Kolme’s gossip network offers several benefits for developers building scalable, multichain applications:

- **Decentralized Communication**: Libp2p enables peer-to-peer message exchange, eliminating single points of failure, unlike centralized relay systems in some blockchains.
- **High Throughput**: Efficient broadcasting supports high-frequency apps.
- **Security**: Cryptographic signatures on all messages ensure data integrity and authenticity.
- **Resilience**: Peer redundancy and retransmission maintain network operation during failures, supporting zero-downtime apps, per [High Availability](high-availability.md).
- **Transparency**: All gossip messages (e.g., blocks, notifications) are verifiable on-chain, enhancing trust, per [Triadic Security Model](triadic-security.md).

These advantages make Kolme a robust platform for building high-performance, fault-tolerant decentralized applications.
