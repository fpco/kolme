# High Availability

<!-- toc -->

## Processor High Availability

Kolme’s processor, responsible for transaction execution and block production, is designed for maximum uptime through a robust, fault-tolerant cluster configuration, ensuring continuous operation for applications like trading platforms or cross-chain swaps.

- **Cluster Setup**:
  - Runs as a cluster of three nodes deployed across different availability zones to mitigate regional outages, as detailed in [Core Chain Mechanics](core-mechanics.md).
  - A PostgreSQL advisory lock, known as the construct lock, ensures only one processor node produces signed blocks at a time, preventing forks by coordinating leadership exclusively for the processor, as it’s the only component requiring a single active process.
- **Failover**:
  - If the active processor node fails (e.g., due to hardware issues or network disruptions), the construct lock is released, and another node in the cluster assumes leadership, achieving zero downtime.
  - Hot standbys are always ready, with state synchronized via shared storage (e.g., Fjall volumes) or fast sync, per [Node Synchronization](node-sync.md).
- **Security**:
  - The construct lock ensures a single block producer, addressing concerns about fork risks, while other nodes validate blocks, per [Triadic Security Model](triadic-security.md).
  - High availability supports multichain operations (Solana, Ethereum, Near), with no impact on core chain functionality during external disruptions, as noted in [External Chain Resilience](external-chain-resilience.md).

## Listeners and Approvers

Listeners and approvers, critical for validating external chain events (e.g., deposits, withdrawals), are designed for resilience, ensuring Kolme apps remain operational even during partial validator downtime.

- **Deployment Options**:
  - Can run as single instances, relying on quorum-based resilience (e.g., 2-of-3 signatures), or as replicated nodes for enhanced uptime, supporting ecosystems like Solana, Ethereum, and Near.
  - Quorum configurations tolerate individual node failures, ensuring external event validation continues, as described in [Triadic Security Model](triadic-security.md).
- **Impact of Downtime**:
  - **Listeners**: Downtime delays deposit confirmations from external chains but does not affect core app operations (e.g., transaction processing, state updates), reinforcing external chain resilience, per [External Chain Resilience](external-chain-resilience.md).
  - **Approvers**: Downtime delays withdrawal authorizations but allows in-app transactions to proceed uninterrupted, critical for user experience in high-traffic apps.
- **Scalability**:
  - Replicated setups scale with demand, supporting multichain apps that add new chains (e.g., Aptos, Cosmos) without reengineering, aligning with Kolme’s flexible design.

## Other Services

Kolme’s supporting services—API servers and indexers—are architected for high availability, ensuring robust access and data processing for applications.

- **API Servers**:
  - Scale horizontally behind load balancers, handling ephemeral queries for user interfaces or external integrations (e.g., Web3 wallet interactions).
  - Use persistent storage (e.g., PostgreSQL) for critical data or ephemeral storage for transient queries, ensuring fault tolerance.
  - Support multichain user access (Solana, Ethereum, Near) with no downtime impact from external chain issues, per [Wallets and Keys](wallets-keys.md).
- **Indexers**:
  - Process blockchain data in batches, storing results in shared databases (e.g., PostgreSQL) for high availability and efficient querying.
  - Scale horizontally to handle large transaction volumes, supporting apps like trading or betting with heavy data needs.
  - Maintain consistency across multichain operations, enabling seamless chain migrations, as noted in [External Chain Resilience](external-chain-resilience.md).
- **Resilience**:
  - Load balancers and shared storage ensure service continuity during node failures, with rapid recovery via persistent volumes or fast sync.

## Startup and Recovery

Kolme’s high-availability design extends to node startup and recovery, minimizing downtime and ensuring rapid deployment for new or restarting nodes.

- **Persistent Storage**:
  - Nodes use Fjall volumes to retain recent blocks and state (framework and app), reducing sync time for restarts, as detailed in [Storage](storage.md).
  - Persistent storage ensures quick recovery after failures, maintaining app availability.
- **Fast Sync**:
  - New or ephemeral nodes use fast sync to load the full framework and application state, signed by the processor, enabling rapid onboarding, per [Node Synchronization](node-sync.md).
  - Fast sync supports multichain apps by quickly aligning with external chain states (e.g., bridge contract events), ensuring no disruption during chain migrations.
- **Recovery**:
  - Failed nodes restart with Fjall volumes or fast sync, with HA clusters ensuring other nodes handle operations during recovery.
  - Planned watchdogs will monitor recovery processes, detecting delays or errors, per [Watchdogs](watchdogs.md).

## Advantages

Kolme’s high-availability design offers several benefits for developers building robust, multichain applications:

- **Zero Downtime**: Processor clusters and quorum-based validators ensure continuous operation, critical for apps like trading or betting, unlike shared blockchains prone to congestion delays.
- **Fault Tolerance**: HA clusters, replicated services, and persistent storage tolerate failures without disrupting app functionality, supporting Solana, Ethereum, Near integrations.
- **Scalability**: Horizontal scaling of API servers and indexers handles high transaction volumes, with flexible chain migration (e.g., adding Cosmos) maintaining uptime, per [External Chain Resilience](external-chain-resilience.md).
- **Rapid Recovery**: Persistent volumes and fast sync minimize startup time, ensuring quick restoration after failures, unlike slower sync methods on traditional platforms.
- **Multichain Resilience**: External chain downtime affects only deposits/withdrawals, not core operations, with easy chain additions enhancing adaptability.
- **Transparency**: HA operations are recorded on-chain, verifiable by nodes, ensuring trust, as described in [Triadic Security Model](triadic-security.md).

These advantages make Kolme a reliable platform for building high-performance, fault-tolerant decentralized applications.