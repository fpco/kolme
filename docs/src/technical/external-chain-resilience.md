# External Chain Resilience

<!-- toc -->

## Minimal Dependency

Kolme minimizes reliance on external blockchains, ensuring applications remain operational during disruptions in either external chains or node providers providing data from them. Additionally, by minimizing the logic living on external chains, Kolme applications can easily and naturally live on multiple chains or migrate to other chains as business needs evolve.

- **Scope**:
  - External chains are used only for payment on-ramps (deposits) and off-ramps (withdrawals) via bridge contracts and ephemeral key verification, as detailed in [Core Chain Mechanics](core-mechanics.md).
  - Core app operations (e.g., transaction processing, state updates) run independently on Kolme’s dedicated chain.
- **Impact**:
  - Downtime or congestion on external chains delays only deposits and withdrawals, not in-app actions like trades or bets, ensuring user experience continuity.
  - Supports multichain apps by maintaining functionality during chain migrations (e.g., adding Cosmos), per [Node Synchronization](node-sync.md).
- **Validation**:
  - Listeners and approvers validate bridge events (e.g., deposits, withdrawals) as transactions, ensuring security without external chain dependency, per [Triadic Security Model](triadic-security.md).

## Bridge Contracts

Bridge contracts facilitate secure fund movements between Kolme and external blockchains and association of ephemeral keys with user wallets.

- **Function**:
  - Handle deposits (users lock funds on external chains, listeners confirm to Kolme) and withdrawals (approvers authorize fund releases), as described in [Core Chain Mechanics](core-mechanics.md).
  - Securely associate new public keys with an external wallet, allowing users to continue to rely on their preferred wallet provider for private key management.
  - Processed as transactions, gossiped via libp2p, ensuring decentralized validation, per [Gossip](gossip.md).
- **Resilience**:
  - External chain downtime delays bridge operations but not core app functionality, supporting high-availability apps, per [High Availability](high-availability.md).
  - Atomic updates during key rotations or chain migrations prevent mismatches, per [Key Rotation](key-rotation.md).

## Advantages

Kolme’s external chain resilience offers several benefits for developers building multichain applications:

- **Uninterrupted Operations**: Minimal dependency ensures core app functionality persists during external chain disruptions, unlike shared blockchains, per [High Availability](high-availability.md).
- **Seamless Migration**: Adding or switching chains requires minimal app changes, supporting scalability (e.g., Cosmos), per [Node Synchronization](node-sync.md).
- **Security**: Quorum-validated bridge events and signed contracts ensure trust, per [Triadic Security Model](triadic-security.md).
- **Transparency**: Bridge transactions are recorded on-chain, verifiable by nodes, enhancing trust, per [Core Chain Mechanics](core-mechanics.md).
- **Flexibility**: Supports diverse ecosystems (Solana, Ethereum, Near) with easy chain additions, per [Gossip](gossip.md).
- **Efficiency**: Fast sync and atomic updates minimize migration overhead, per [Version Upgrades](version-upgrades.md).

These advantages make Kolme a robust platform for building high-performance, fault-tolerant decentralized applications.
