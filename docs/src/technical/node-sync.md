# Node Synchronization

<!-- toc -->

## Slow Sync

Slow sync is Kolme’s most trustless synchronization method, ideal for nodes requiring maximum security, such as validators or auditors verifying applications like trading platforms or cross-chain swaps across ecosystems like Solana, Ethereum, and Near.

- **Process**:
  - Nodes process every block sequentially from the genesis block, validating each transaction, data load, and state transition, as detailed in [Core Chain Mechanics](core-mechanics.md).
  - Validates signatures, nonces, external data (e.g., Pyth price feeds), and state hashes against the processor’s outputs, per [External Data Handling](external-data.md).
- **Bandwidth Usage**:
  - Heavy during initial load or catching up, as it downloads and verifies all block data, including transactions, metadata, logs, and data loads.
  - Light after catching up, as nodes only download new blocks, which exclude state data (only state hashes are included), reducing bandwidth needs significantly.
- **Characteristics**:
  - Trustless, relying solely on cryptographic verification without assuming processor integrity, addressing concerns about centralized trust.
- **Use Cases**:
  - Suitable for high-security nodes (e.g., listeners, approvers) ensuring chain integrity, per [Triadic Security Model](triadic-security.md).
  - Supports multichain apps by validating bridge contract events, ensuring consistency across Solana, Ethereum, Near, or new chains like Aptos, per [External Chain Resilience](external-chain-resilience.md).
- **Performance**:
  - Slower during initial sync due to comprehensive validation, but optimized for ongoing operation with lightweight block downloads after catching up.

## Fast Sync

Fast sync is Kolme’s quickest synchronization method, designed for rapid node onboarding, such as new validators or ephemeral nodes, offering support for optional block validation for added security.

- **Process**:
  - Can either transfer full framework and application state, signed by the processor, from an existing node or shared storage, or download individual blocks and execute locally, depending on which will be faster, as outlined in [Core Chain Mechanics](core-mechanics.md).
  - Nodes verify the processor’s signature on the state (if transferred), then begin processing new blocks without validating historical transactions by default.
  - Once caught up, fast sync uses block-by-block execution for new blocks, similar to slow sync, validating transactions and data loads with minimal bandwidth, as only block data (not state) is downloaded.
  - **Optional Validation**: Nodes can validate historical block data (e.g., transactions, data loads) post-sync, re-executing a subset or all blocks to confirm state integrity, balancing speed and security, per [Triadic Security Model](triadic-security.md).
- **Characteristics**:
  - Optimizes the trade-off between bandwidth and compute during initial sync, enabling nodes to join in seconds or minutes, depending on state size (hundreds of MBs or GBs), with block-by-block execution post-catchup further reducing bandwidth needs.
  - Requires trust in the processor’s signature if state is transferred, less secure than slow sync unless optional validation is enabled, suitable for trusted environments, per [High Availability](high-availability.md).
- **Use Cases**:
  - Critical for rapid startup in high-availability clusters, such as processor standbys or API servers, per [High Availability](high-availability.md).
  - Supports multichain apps by quickly aligning with external chain states (e.g., bridge events), facilitating chain migrations (e.g., adding Cosmos), per [External Chain Resilience](external-chain-resilience.md).
  - Used during version upgrades to transfer state to new nodes, as detailed in [Version Upgrades](version-upgrades.md).
- **Performance**:
  - Fastest sync method, with optional validation and block-by-block execution providing flexibility for scenarios prioritizing speed or enhanced verification.

## Startup

Kolme’s node startup process leverages persistent storage and sync methods to minimize downtime and ensure rapid deployment, supporting robust, multichain applications.

- **Persistent Storage**:
  - Nodes use persistent storage volumes to store recent blocks and state (framework and app), enabling quick restarts by loading cached data, as described in [Storage](storage.md).
  - Persistent storage reduces reliance on full sync, maintaining availability during failures, per [High Availability](high-availability.md).
- **Sync Selection**:
  - Nodes with persistent storage resume with slow sync for new blocks, minimizing startup time with lightweight block downloads.
  - New or ephemeral nodes use fast sync for rapid onboarding, optionally validating block data for added security, ideal for high-availability setups.
  - Security-focused nodes (e.g., listeners) opt for slow sync to validate all data, supporting trustless operations across Solana, Ethereum, Near.
- **Multichain**:
  - Startup aligns with external chain states via bridge contract events, ensuring seamless multichain operation and easy additions (e.g., Aptos), per [External Chain Resilience](external-chain-resilience.md).
  - Fast sync accelerates recovery during chain migrations, maintaining uptime.
- **Monitoring**:
  - Planned watchdogs will monitor sync processes, detecting delays or errors, enhancing reliability, per [Watchdogs](watchdogs.md).

## Advantages

Kolme’s node synchronization offers several benefits for developers building scalable, multichain applications:

- **Flexible Sync Options**: Slow and fast sync cater to trustless validation or rapid onboarding, with fast sync’s optional block validation and block-by-block execution adding security and efficiency, unlike one-size-fits-all methods on shared blockchains.
- **High Availability**: Fast sync and persistent storage ensure quick startup and recovery, supporting zero-downtime apps, per [High Availability](high-availability.md).
- **Multichain Scalability**: Sync methods align with external chain states, enabling seamless operation across Solana, Ethereum, Near, and new chains like Cosmos, per [External Chain Resilience](external-chain-resilience.md).
- **Transparency**: Slow sync validates all data on-chain, while fast sync’s signed state, optional validation, and block-by-block execution maintain verifiable integrity, per [Triadic Security Model](triadic-security.md).
- **Performance**: Optimized trade-off between bandwidth and compute supports large-scale apps, with both sync methods lightweight post-catchup, unlike resource-intensive syncs on Ethereum or Solana.
- **Resilience**: Sync processes tolerate external chain disruptions, maintaining core app functionality, enhancing Kolme’s reliability.

These advantages make Kolme a developer-friendly platform for building high-performance, fault-tolerant decentralized applications.