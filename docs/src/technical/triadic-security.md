# Triadic Security Model

<!-- toc -->

## Validator Groups

Kolme’s triadic security model distributes responsibilities across three validator groups—listeners, a processor, and approvers—forming a checks-and-balances system that ensures trust, transparency, and resilience, rivaling the security of blockchains like Solana, Ethereum, and Near while avoiding centralized risks. Each group has distinct roles in maintaining the integrity of the Kolme chain and its multichain integrations.

### Processor

- **Role**: Executes transactions and produces signed blocks for the application’s dedicated blockchain. Validates transaction signatures, nonces, and message validity, runs deterministic Rust code, updates framework and application state, and broadcasts blocks via libp2p, as detailed in [Core Chain Mechanics](core-mechanics.md).
- **Key Characteristics**:
  - Uses a single private key for block production, ensuring clarity and consistency.
  - Runs in a high-availability cluster of three nodes across availability zones, coordinated by a PostgreSQL advisory lock (construct lock) to prevent forks, as described in [High Availability](high-availability.md).
  - Sole block producer to avoid conflicting blocks, addressing concerns about forks. Other nodes validate blocks but cannot produce them, preserving decentralization through verification.
- **Security Contribution**: Centralizes block production for efficiency while relying on listeners and approvers for external validations, balancing performance and security.

### Listeners

- **Role**: Monitor bridge contracts on external blockchains (Solana, Ethereum, Near) for events like deposits, public key associations, or withdrawal requests. Submit signed confirmations to the processor when a quorum agrees, for inclusion in the Kolme chain.
- **Key Characteristics**:
  - Operate as a multi-signature group with a configurable quorum (e.g., 2-of-3 signatures), ensuring resilience to node failures.
  - Can run as single instances or replicas, with quorum-based design tolerating downtime without disrupting chain operations, per [High Availability](high-availability.md).
  - Downtime delays deposit confirmations but not core app functionality, reinforcing external chain resilience, as noted in [External Chain Resilience](external-chain-resilience.md).
- **Security Contribution**: Provide decentralized validation of external events, preventing fraudulent bridge actions and ensuring trust.

### Approvers

- **Role**: Validate outgoing actions, such as withdrawals to external blockchains, by signing off on bridge contract transactions, ensuring legitimacy and sufficient funds.
- **Key Characteristics**:
  - Multi-signature group with configurable quorum, enhancing fault tolerance.
  - Single or replicated nodes, with downtime delaying withdrawals but not app operations, per [High Availability](high-availability.md).
  - Collaborate with listeners to secure external fund movements.
- **Security Contribution**: Act as gatekeepers for external actions, protecting against unauthorized withdrawals and maintaining user trust.

## Quorum-Based Approval

Administrative tasks, such as software upgrades, validator key rotations, or validator set changes, use a quorum-based approval process for flexibility and robustness.

- **Mechanism**:
  - Requires agreement from at least two of the three validator groups (processor, listeners, approvers). All three can propose and approve actions, addressing concerns about quorum flexibility and preventing single-group dominance.
  - Proposals are stored in the framework state’s admin proposal state (a MerkleMap mapping proposal IDs to details), supporting multiple proposals with first-come, first-served approval, as detailed in [Core Chain Mechanics](core-mechanics.md).
  - Approvals update the framework state and, for external actions (e.g., validator set changes), emit atomic bridge contract updates, per [External Chain Resilience](external-chain-resilience.md).
- **Examples**:
  - **Upgrades**: A listener proposes a new `chain_version` (e.g., `v2`), the processor and approvers approve, updating the framework state, as described in [Version Upgrades](version-upgrades.md).
  - **Key Rotations**: An approver proposes a validator set change, listeners and processor approve, updating bridge contracts atomically, per [Key Rotation](key-rotation.md).
- **Security Benefit**: Quorum-based approval ensures no single group can unilaterally alter the chain, while flexibility allows rapid adaptation, supporting multichain operations and chain migrations (e.g., adding Aptos, Cosmos) without compromising security.

## Security Features

Kolme’s triadic model incorporates several features to enhance security and transparency:

- **Transparent Chains**: All transactions, including user actions (e.g., trades, bets) and administrative messages (e.g., upgrades, key rotations), are recorded on the public blockchain, verifiable by any node, developer, or auditor, ensuring trust across ecosystems like Solana, Ethereum, and Near.
- **Decentralized Validation**: Non-processor nodes (listeners, approvers, community nodes) validate blocks by re-executing transactions and checking state hashes, confirming the processor’s integrity without producing blocks.
- **Quorum Resilience**: The two-of-three quorum tolerates downtime or compromise of one group, maintaining chain operations and external integrations, critical for high-availability apps.
- **Planned Watchdogs**: Future watchdog nodes will monitor processor behavior, verifying failed transaction notifications, state hash accuracy, and quorum compliance, detecting issues like censorship or state manipulation, as outlined in [Watchdogs](watchdogs.md).
- **Multichain Security**: Bridge contracts secure fund movements across external chains, with listeners and approvers validating events, ensuring consistency and trust. Kolme’s design simplifies adding new chains (e.g., Cosmos) while preserving on-chain security, per [External Chain Resilience](external-chain-resilience.md).

These features address developer concerns about centralized risks, validator reliability, and multichain integrity, making Kolme a secure platform for building decentralized applications.
