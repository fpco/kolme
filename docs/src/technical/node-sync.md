# Node Synchronization

<!-- toc -->

## Slow Sync

Slow sync is Kolme’s most trustless synchronization method.

- **Process**:
  - Nodes process every block sequentially from the genesis block, validating each transaction, data load, and state transition, as detailed in [Core Chain Mechanics](core-mechanics.md).
  - Validates signatures, nonces, external data, and state hashes against the processor’s outputs.
- **Bandwidth Usage**:
  - Heavy during initial load or catching up, as it downloads and verifies all block data, including transactions, metadata, logs, and data loads.
  - Light after catching up, as nodes only download new blocks, which exclude state data (only state hashes are included).
- **Characteristics**:
  - Trustless, relying solely on cryptographic verification without assuming processor integrity.
  - Slower during initial sync due to comprehensive validation, but optimized for ongoing operation with lightweight block downloads after catching up.

## Fast Sync

Fast sync (state sync) trades trust for speed by downloading the current framework and application state instead of replaying all historical blocks.

- **Process**:
  - Nodes download the Merkle layers for framework state, app state, and logs from peers via gossip’s request/response flow until the current state is reconstructed.
  - Once caught up, they resume normal block-by-block execution for new blocks.
- **Characteristics**:
  - Fastest way to join a network, but it requires trusting the peers that provided the state until you optionally re-execute historical blocks to verify the result.
  - Works well when instances share storage (e.g., Postgres plus Fjall) or when provisioning a new node that needs to come online quickly.

## Startup

Kolme’s node startup process leverages persistent storage and sync methods to minimize downtime and ensure rapid deployment for new or restarting nodes.

- **Persistent Storage**:
  - Nodes use persistent storage volumes to store recent blocks and state (framework and app), enabling quick restarts by loading cached data, as described in [Storage](storage.md).
  - Persistent storage reduces reliance on full sync, maintaining availability during failures, per [High Availability](high-availability.md).
- **Sync Selection**:
  - Nodes with persistent storage resume with slow sync for new blocks, minimizing startup time with lightweight block downloads.
  - New or ephemeral nodes can use fast sync for rapid onboarding, then optionally revalidate blocks if needed.
  - Security-focused nodes (e.g., listeners) typically opt for slow sync to validate all data.
- **Monitoring**:
  - Planned watchdogs will monitor sync processes, detecting delays or errors, enhancing reliability, per [Watchdogs](watchdogs.md).

## Advantages

Kolme offers two clear sync paths:

- **Trustless replay**: Slow sync validates everything from genesis.
- **Fast catch-up**: Fast sync downloads Merkle layers to get a verified state hash quickly, with the option to replay later if you need full assurance.
