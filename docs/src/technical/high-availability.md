# High Availability

<!-- toc -->

## Processor High Availability

Kolme’s processor is the only component that produces signed blocks, so keeping it online is critical. High availability is achieved operationally rather than through protocol-level leader election.

- **Cluster Setup**:
  - Run multiple processor binaries pointed at the same Postgres-backed store. The store uses a PostgreSQL advisory lock (the construct lock) so that only one instance is actively producing blocks, as described in [Core Chain Mechanics](core-mechanics.md).
  - There is no fixed node count; deploy as many instances as you need based on your own monitoring and restart strategy.
- **Failover**:
  - If the active processor dies, another instance can take over after acquiring the construct lock. The time to fail over depends on your process supervisor and whether instances already have the necessary state available locally.
  - Sharing a Postgres store plus a synchronized Fjall merkle store keeps hand-offs fast; otherwise, the new leader must perform state sync before producing blocks, per [Node Synchronization](node-sync.md).
- **Security**:
  - The construct lock prevents multiple processors from producing forks while still allowing other nodes to validate every block, per [Triadic Security Model](triadic-security.md).

## Listeners and Approvers

Listeners and approvers, critical for validating external chain events (e.g., deposits, withdrawals), are designed for resilience, ensuring Kolme apps remain operational even during partial validator downtime.

- **Deployment Options**:
  - Can run as single instances, relying on quorum-based resilience (e.g., 2-of-3 signatures), or as replicated nodes for enhanced uptime, supporting ecosystems like Solana, Ethereum, and Near.
  - Quorum configurations tolerate individual node failures, ensuring external event validation continues, as described in [Triadic Security Model](triadic-security.md).
- **Impact of Downtime**:
  - **Listeners**: Downtime delays deposit confirmations from external chains but does not affect core app operations (e.g., transaction processing, state updates), reinforcing external chain resilience, per [External Chain Resilience](external-chain-resilience.md).
  - **Approvers**: Downtime delays withdrawal authorizations but allows in-app transactions to proceed uninterrupted, critical for user experience in high-traffic apps.
- **Scalability**:
  - Replicated setups scale with demand; quorum thresholds should be tuned to tolerate expected failure domains.

## Other Services

Kolme’s supporting services—API servers and indexers—follow standard service-deployment patterns rather than protocol-specific HA guarantees.

- **API Servers**:
  - Can be scaled horizontally behind a load balancer. They are stateless aside from their backing data stores, so standard rolling-deploy practices apply.
- **Indexers**:
  - Process blockchain data and store derived results in a database such as PostgreSQL. Availability comes from running multiple workers or using managed database HA features.
- **Resilience**:
  - Use persistent volumes for local caches when they improve startup times; otherwise plan for fresh state acquisition via sync.

## Startup and Recovery

Kolme’s high-availability design extends to node startup and recovery, minimizing downtime and ensuring rapid deployment for new or restarting nodes.

- **Persistent Storage**:
  - Nodes use Fjall volumes to retain recent blocks and state (framework and app), reducing sync time for restarts, as detailed in [Storage](storage.md).
  - Persistent storage ensures quick recovery after failures, maintaining app availability.
- **Fast Sync**:
  - New or ephemeral nodes use fast/state sync to download framework and application state from peers instead of replaying every block, enabling rapid onboarding, per [Node Synchronization](node-sync.md).
  - The trade-off is trust in the state provider until the node replays blocks (if desired) to verify.
- **Recovery**:
  - Failed nodes restart with Fjall volumes or fast sync; other validator groups continue to operate while a replacement catches up.
  - Monitoring to detect slow recovery is deployment-specific today; watchdogs remain a planned improvement, per [Watchdogs](watchdogs.md).

## Advantages

Kolme’s HA story is practical and deployment-driven:

- **Single active processor**: The construct lock prevents forks while allowing multiple hot spares.
- **Graceful degradation**: Listener/approver quorums tolerate individual node failures; outages primarily delay deposits/withdrawals.
- **Faster restarts**: Persisting Fjall data and using state sync reduces catch-up time after a crash.
- **Standard service patterns**: API servers and indexers follow conventional HA practices; no bespoke orchestration is required.
