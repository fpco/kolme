# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this changelog adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Implement a `Trigger` mechanism. (80cd518…)
- Improved state sync and archiving
  - Incremental state sync (a9ceea1…)
  - Sync older blocks on demand via state sync. (909536f…)
  - Add the archiver component for synchronizing older blocks. (f6ed91d…)
- New config mechanism for listeners in gossip, using the `GossipListener` type.

### Fixed

- Remove linker line from cargo configuration (828b467…)

### Changed

- Replaced the prior Kolme store setup with a simplified mechanism that combines Kolme and Merkle stores. We now have three stores available: in memory (for testing), pure Fjall (for a single machine), and pure PostgreSQL (for shared server storage with construction lock).
- Adds BlockDoubleInserted variant for notifying height and hash collisions when storing blocks
- Updates add_block methods on all stores to return a KolmeStoreError type on failure
- Ignores BlockDoubleInserted error from underlying store
- Rename error variants to be more descriptive
- Remove unneeded async/await
- Remove mDNS support entirely. Tests all use Kademlia discovery.

## [v0.1.0] - 2025-06-13

This is the initial release of Kolme for internal usage. Some highlights of implementation are included below, but overall the changelog here is: first working version used by a downstream project.

### Added

- merkle-map
- Core Kolme abstraction
- Processor, listener, and approver validator implementation
- libp2p-based gossip with Kademlia discovery
- Support for Cosmos and Solana bridge contracts
- Key rotation
- Docs site
- Version upgrades and Upgrader component
