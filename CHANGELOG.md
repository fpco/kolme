# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this changelog adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Implements PurePostgres store

### Fixed
- Adds discard pattern for unused fields of BlockAlreadyInDb error
- Uses block merkle hash for collision detection
- Fixes comparison to detect hash collision
- Bubbles db error upstream on constraint error
- Removes superfluous comments
- Uses tempdir for fjall and removes "--features store_tests" flag from justfile
- Renames error variant for better grammar
- Removes unnecessary clear of tempdir
- Uses OnceLock to model a missing `latest_block`
- Renames constructor methods
- Executes block insertion and merkle insertion in a single transaction
- Uses Relaxed ordering instead of Acquire

### Changed
- Adds BlockDoubleInserted variant for notifying height and hash collisions when storing blocks
- Updates add_block methods on all stores to return a KolmeStoreError type on failure
- Ignores BlockDoubleInserted error from underlying store
- Rename error variants to be more descriptive
- Remove unneeded async/await

### Tests
- Implements stores to check double collision's safe behavior
- Uses conditional yield to allow race condition
- Adds command for running store tests
- Tests new PurePostgres's behavior with testapp and multiple-processors

### Miscellaneous
- Fixes formatting issues
- Fixes CICD issues

## [v0.1.0] - 2025-06-13

### Added
- Types: implement sqlx's Decode and Encode traits for BlockHeight
- Tests: introduce serializing_idempotency_for_type macro and create prop tests for primitive types
- Add paste
- Tests: add serialization idempotency test for usize
- Tests: cover Option(T) in serialization idempotency tests
- Tests: cover serialization idempotency for BTreeSet
- Tests: implement serialization idempotency prop tests for maps
- Tests: serialization idempotency test for slices
- Tests: implement Arbitrary::shrink() for MerkleMap
- Tests: implement Arbitrary::shrink() for [T]
- Move quickcheck to workspace dependencies
- Add quickcheck to kolme package dev deps
- Kolme::core: serialization idempotence coverage for simple wrappers
- Kolme::core: serialization idempotence for BridgeContract
- Kolme::core: serialization idempotence test for AssetConfig
- Kolme::core: serialization idempotence test for ChainConfig
- Kolme::core: serialization idempotence for ExternalChain
- KolmeApp::state: add Debug trait requirement
- Bool: implement MerkleSerialize/MerkleDeserialize
- Merkle-map: add jiff from workspace
- Jiff::Timestamp: implement MerkleSerialize/MerkleDeserialize
- Merkle-map: MerkleSerialize/MerkleDeserialize for Timestamp idempotence test
- Merkle-map: add smallvec
- Implement MerkleSerialize/MerkleDeserialize for SmallVec
- Merkle-map: cover SmallVec serialization idempotence
- Implement MerkleSerialize/MerkleDeserialize for u128
- Serialization idempotence property test for u128
- Bump rust-toolchain to 1.84
- Run fmt and clippy stages after build stage
- Gossip: add local_display_name field
- GossipBuilder: by default set disable_ip6 and disable_quic to true
- Get kademlia-discovery test to use docker-compose
- Kademlia-discovery: set timeout of 30 sec for network waiging
- GossipBuilder: make mDNS optional
- Kademlia-discovery: move out of the Docker and disable mDNS
- Kademlia-discovery: move all important code to lib.rs
- Kolme dev: add kademlia-discovery
- Kolme: move kademlia-discovery test to the main test suite
- Kademlia-discovery: use TestTasks to run validator and client
- Kademlia-discovery: use DNS name for bootstrapping
- Kolme::core: lock serialization format for newtype wrappers over primitives
- Merkle-map: cover 2-tuple [de]serialization
- Merkle-map: better test for 2-tuple [de]serialization

### Fixed
- Move paste to dev dependencies
- Quickcheck test for Timestamp serialization - use nanosecond precision
- MerkleSerialize for SmallVec: keep data in stack
- MerkleDeserialize for SmallVec: keep data in stack
- MerkleDeserialize for SmallVec: match serialization
- MerkleSerialize for SmallVec: remove Clone binding
- MerkleDeserialize for SmallVec: variable length
- Gossip::handle_response(): switch response message log level
- Run kademlia-validators after main test suite
- MerkleSerializeRaw for 2-tuple
- 2-tuples: replace MerkleSerialialize with MerkleSerializeRaw

### Changed
- Enhance error handling and clean up unused notifications in testapp
- Update parameter passing in next_message_as_json function
- Move quickcheck! import to use statement
- Tests: move map serialization idempotency tests out of macro

### Removed
- Remove now unused clap dependency.
- Remove unneeded async/await

### Tests
- Compose: set restart to no for localosmosis and postgres
- GossipMessage: fix typo
- Solana bridge tests
- Make the "client" feature a default
- Getting a tx only returns the height
- Add hidapi dependency
- Make network discovery more reliable.
