# Core Chain Mechanics

<!-- toc -->

## Blocks

Kolme’s blockchain is an append-only chain of blocks, each containing exactly one transaction and signed by the processor. Unlike traditional blockchains with fixed block times (e.g., Ethereum’s 12-second slots), Kolme blocks are produced on-demand, optimizing performance for each app’s needs and eliminating delays from scheduled intervals.

- **Structure**:
  - **Metadata**: Includes timestamp, block height, and previous block hash, ensuring chain continuity and ordering.
  - **Transaction**: The single transaction processed in the block, containing one or more messages (see Transactions).
  - **Data Loads**: External data fetched during execution (e.g., Pyth price feeds, sports odds), stored with cryptographic signatures for verification.
  - **Logs**: Execution outputs, such as trade confirmations or error messages, for debugging and auditing.
  - **State Hashes**: Hashes of the updated framework and application state, ensuring efficient validation without storing full state in blocks.
- **Purpose**: Blocks record state transitions, enabling other nodes to validate the processor’s outputs and maintain a consistent, transparent chain. The on-demand production supports high-performance apps, addressing concerns about congestion or delays.

## Transactions

Transactions are the core units of action in Kolme, representing signed instructions submitted to the blockchain for processing. They are distinct from messages, clarifying a common point of confusion for developers.

- **Definition**:
  - A transaction is a signed package containing:
    - **Messages**: One or more individual actions, such as placing a trade, proposing a software upgrade, or transferring funds. Messages are app-specific (e.g., execute a swap), administrative (e.g., rotate a validator key), or related to fund transfers (e.g., deposit via bridge contract).
    - **Nonce**: A unique sequence number per account, ensuring transaction ordering and preventing double-spending.
    - **Signature**: Cryptographic signature from the account’s private key, verifying authenticity.
  - Transactions are broadcast to the mempool via libp2p, where the processor selects them for inclusion in blocks.
- **Nonces**:
  - Stored in the framework state’s account mapping (within a MerkleMap), which maps account IDs to account information, including the next expected nonce.
  - Updated atomically during transaction execution, ensuring correctness without scanning blockchain history. For example, if an account’s next nonce is 5, a transaction with nonce 6 is rejected, triggering a failure notification.
  - This addresses concerns about transaction ordering, as nonces provide a clear, efficient mechanism to prevent out-of-order or duplicate submissions.
- **Validation**:
  - The processor checks the signature, nonce, and message validity before execution.
  - Invalid transactions (e.g., wrong nonce, malformed messages) are dropped, with signed notifications broadcast via libp2p to inform users, as detailed in [failed transactions](failed-transactions.md).

## Processor Role

The processor is a critical component of Kolme’s architecture, responsible for executing transactions and producing blocks. Its design ensures high availability and reliability while addressing concerns about centralized block production.

- **Function**:
  - **Validation**: Checks each transaction’s signature, nonce, and message validity to ensure it can be executed.
  - **Execution**: Runs the transaction’s messages using deterministic Rust code, updating the framework and application state.
  - **Block Production**: Creates a signed block containing the transaction, metadata, data loads, logs, and state hashes, then broadcasts it via libp2p.
  - **State Updates**: Applies changes to the MerkleMap-based framework and app state, ensuring consistency across nodes.
- **Sole Block Producer**:
  - Only the processor produces signed blocks, preventing multiple nodes from creating conflicting blocks and risking hard forks. This design enables multiple processor executables to run in a high-availability cluster, with a PostgreSQL advisory lock (construct lock) ensuring one active processor at a time.
  - This addresses confusion about block production, as other nodes validate blocks but cannot produce them, maintaining decentralization through validation.
- **Validation by Other Nodes**:
  - Non-processor nodes (e.g., listeners, approvers, or community nodes) execute transactions locally to verify the processor’s blocks, checking state hashes and execution results.
  - This ensures the processor’s outputs are trustworthy, with planned watchdog nodes monitoring for discrepancies, as described in [watchdogs](watchdogs.md).
- **High Availability**:
  - The processor runs in a cluster of three nodes across availability zones, with the construct lock coordinating leadership. If the active processor fails, another node takes over, ensuring zero downtime, as detailed in [high availability](high-availability.md).

## Framework State

The framework state is the cumulative system state resulting from all transactions since the genesis block, providing a verifiable snapshot of Kolme’s operations.

- **Components**:
  - **Account Balances**: Tracks funds for each account ID, updated during deposits, withdrawals, or app-specific actions.
  - **Validator Sets**: Lists keys for the processor, listeners (with quorum), and approvers (with quorum), modified via key rotations or upgrades.
  - **Nonces**: Maps account IDs to the next expected nonce, ensuring transaction ordering.
  - **Admin Proposal State**: Stores pending proposals (e.g., upgrades, key set changes) in a MerkleMap, supporting multiple proposals with first-come, first-served approval.
  - **Chain Version (`chain_version`)**: Tracks the code version used by the chain (e.g., `v1`), stored in the MerkleMap and genesis block, ensuring reproducibility during upgrades, as described in [version upgrades](version-upgrades.md).
- **Storage**:
  - Held in a MerkleMap, a base-16 balanced tree optimized for efficient hashing, cloning, and updates, capable of handling large datasets (hundreds of MBs or GBs).
  - Only state hashes are stored in blocks, as full state inclusion would exceed storage and network limits, addressing scalability concerns.
- **Role**:
  - Provides a single source of truth for the system, enabling nodes to validate processor outputs and maintain consistency.
  - The framework state is not just a block number but a comprehensive snapshot of the system, crucial for developers building robust apps.

## App State

The app state is the application-specific data defined by the developer, stored separately from the framework state but also in a MerkleMap.

- **Structure**: Arbitrary data structures tailored to the app’s needs, such as trade histories for a swaps platform, bet outcomes for a betting app, or liquidity pools for an AMM.
- **Storage**: Held in a MerkleMap for efficient updates and hashing, with state hashes included in blocks alongside framework state hashes.
- **Updates**: Modified during transaction execution based on the app’s Rust logic, ensuring deterministic results across nodes.
- **Purpose**: Allows developers to manage custom data flexibly, supporting complex app requirements without impacting system state. This separation clarifies concerns about state management, as app state is isolated but validated similarly to framework state.

## Multichain Context

Kolme’s chain mechanics support multichain integration with blockchains like Solana, Ethereum, and Near, enabling apps to serve users across ecosystems. Transactions involving external chains (e.g., deposits, withdrawals) are processed via bridge contracts, validated by listeners and approvers, as detailed in [external chain resilience](external-chain-resilience.md). Kolme’s design allows developers to add new chains (e.g., Aptos, Cosmos) without rewriting apps, ensuring future-proof scalability. The framework state tracks bridge-related data (e.g., validator sets for external chains), while the processor handles on-chain execution, maintaining security and transparency across multichain operations.

## Key Features

Kolme’s core mechanics enable several developer-friendly features:

- **Instant Transaction Processing**: On-demand block production eliminates delays, ideal for high-performance apps.
- **No Gas Costs or Execution Limits**: Process complex logic without fees or constraints, leveraging Rust’s capabilities.
- **Deterministic Execution**: Ensures consistent results across nodes, critical for blockchain integrity.
- **Scalable State Management**: MerkleMap and state hashes support large datasets without storage/network bloat.
- **Transparent Validation**: Public blocks and state hashes enable node verification, with watchdogs enhancing trust.
- **Multichain Flexibility**: Seamless integration with external chains, with easy migration to new chains.

These features address common developer concerns, such as performance, cost, and multichain complexity, making Kolme a robust platform for building decentralized apps.
