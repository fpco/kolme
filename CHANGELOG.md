# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this changelog is generated automatically from commit history using [git-cliff](https://github.com/orhun/git-cliff).


## [Unreleased]




<!-- 0 -->🚀 Features

- Implements PurePostgres store (58d93a1…)


<!-- 1 -->🐛 Bug Fixes

- Adds discard pattern for unused fields of BlockAlreadyInDb error (350850b…)

- Uses block merkle hash for collision detection (e63d09c…)

- Fixes comparison to detect hash collision (18661bf…)

- Bubbles db error upstream on constraint error (eaa811f…)

- Removes superfluous comments (b8e6628…)

- Uses tempdir for fjall and removes "--features store_tests" flag from justfile (efdddba…)

- Renames error variant for better grammar (1e946b7…)

- Removes unnecessary clear of tempdir (a8a5d3d…)

- Uses OnceLock to model a missing `latest_block` (3cafbbf…)

- Renames constructor methods (384d533…)

- Executes block insertion and merkle insertion in a single transaction (747ef9c…)

- Uses Relaxed ordering instead of Acquire (2079183…)


<!-- 10 -->💼 Other

- Fixes formatting issues (1d6e0bb…)


<!-- 2 -->🚜 Refactor

- Adds BlockDoubleInserted variant for notifying height and hash collisions when storing blocks (ebf080e…)

- Updates add_block methods on all stores to return a KolmeStoreError type on failure (20529a9…)

- Ignores BlockDoubleInserted error from underlying store (14dd6f5…)

- Rename error variants to be more descriptive (d3d4889…)

- Remove unneeded async/await (d1fe800…)


<!-- 6 -->🧪 Testing

- Implements stores to check double collision's safe behavior (031be79…)

- Uses conditional yield to allow race condition (f53afb7…)

- Adds command for running store tests (f89a69d…)

- Tests new PurePostgres's behavior with testapp and multiple-processors (83ece9d…)


<!-- 7 -->⚙️ Miscellaneous Tasks

- Fixes CICD issues (c1ced28…)

- Fixes CICD issues (b406c81…)



## [v0.1.0] - 2025-06-13




<!-- 0 -->🚀 Features

- Types: implement sqlx's Decode and Encode traits for BlockHeight (244f357…)

- Tests: introduce serializing_idempotency_for_type macro and create prop tests for primitive types (d179508…)

- Add paste (14edb9f…)

- Tests: add serialization idempotency test for usize (13880be…)

- Tests: cover Option(T) in serialization idempotency tests (b85291c…)

- Tests: cover serialization idempotency for BTreeSet (ab87d39…)

- Tests: implement serialization idempotency prop tests for maps (3f363b9…)

- Tests: serialization idempotency test for slices (ecf1ec3…)

- Tests: implement Arbitrary::shrink() for MerkleMap (b7eccd0…)

- Tests: implement Arbitrary::shrink() for [T] (4d6de14…)

- Move quickcheck to workspace dependencies (af30195…)

- Add quickcheck to kolme package dev deps (80f5a48…)

- Kolme::core: serialization idempotence coverage for simple wrappers (f6ac69b…)

- Kolme::core: serialization idempotence for BridgeContract (4315f70…)

- Kolme::core: serialization idempotence test for AssetConfig (e6855dc…)

- Kolme::core: serialization idempotence test for ChainConfig (20f0098…)

- Kolme::core: serialization idempotence for ExternalChain (469784a…)

- KolmeApp::state: add Debug trait requirement (5bef6c8…)

- Bool: implement MerkleSerialize/MerkleDeserialize (43b827b…)

- Merkle-map: add jiff from workspace (597f60d…)

- Jiff::Timestamp: implement MerkleSerialize/MerkleDeserialize (6ce39d1…)

- Merkle-map: MerkleSerialize/MerkleDeserialize for Timestamp idempotence test (063f751…)

- Merkle-map: add smallvec (99dbb3c…)

- Implement MerkleSerialize/MerkleDeserialize for SmallVec (3daafc0…)

- Merkle-map: cover SmallVec serialization idempotence (4f59368…)

- Implement MerkleSerialize/MerkleDeserialize for u128 (954ec2b…)

- Serialization idempotence property test for u128 (fe56e3f…)

- Bump rust-toolchain to 1.84 (2f620e4…)

- Run fmt and clippy stages after build stage (ce0147d…)

- Gossip: add local_display_name field (86e9c71…)

- GossipBuilder: by default set disable_ip6 and disable_quic to true (1120da7…)

- Get kademlia-discovery test to use docker-compose (0d81ab7…)

- Kademlia-discovery: set timeout of 30 sec for network waiging (aa89566…)

- GossipBuilder: make mDNS optional (d3f99f5…)

- Kademlia-discovery: move out of the Docker and disable mDNS (bb5a5b4…)

- Kademlia-discovery: move all important code to lib.rs (f94a97c…)

- Kolme dev: add kademlia-discovery (090820d…)

- Kolme: move kademlia-discovery test to the main test suite (7e034bf…)

- Kademlia-discovery: use TestTasks to run validator and client (56fc7f3…)

- Kademlia-discovery: use DNS name for bootstrapping (cb9d871…)

- Kolme::core: lock serialization format for newtype wrappers over primitives (a5e3a0a…)

- Merkle-map: cover 2-tuple [de]serialization (7c226fc…)

- Merkle-map: better test for 2-tuple [de]serialization (ff066ec…)


<!-- 1 -->🐛 Bug Fixes

- Move paste to dev dependencies (f287d25…)

- Quickcheck test for Timestamp serialization - use nanosecond precision (e24ecaf…)

- MerkleSerialize for SmallVec: keep data in stack (ba7020c…)

- MerkleDeserialize for SmallVec: keep data in stack (a95da6d…)

- MerkleDeserialize for SmallVec: match serialization (c92d984…)

- MerkleSerialize for SmallVec: remove Clone binding (481aa69…)

- MerkleDeserialize for SmallVec: variable length (03eafef…)

- Gossip::handle_response(): switch response message log level (9d55320…)

- Run kademlia-validators after main test suite (2011484…)

- MerkleSerializeRaw for 2-tuple (c67ea4c…)

- 2-tuples: replace MerkleSerialialize with MerkleSerializeRaw (f354ca0…)


<!-- 10 -->💼 Other

- Solana bridge tests (cb88d72…)

- Make the "client" feature a default (7a07832…)

- Remove now unused clap dependency. (8b2fa12…)

- Getting a tx only returns the height (d8542bd…)

- Add hidapi dependency (a6d502d…)

- Make network discovery more reliable. (e87ee57…)


<!-- 2 -->🚜 Refactor

- Enhance error handling and clean up unused notifications in testapp (58d74e4…)

- Update parameter passing in next_message_as_json function (a81a8d7…)

- Move quickcheck! import to use statement (d7ab575…)

- Tests: move map serialization idempotency tests out of macro (f3b8829…)


<!-- 7 -->⚙️ Miscellaneous Tasks

- Compose: set restart to no for localosmosis and postgres (7e0707c…)

- GossipMessage: fix typo (e2a93e1…)


---
For previous versions and more details, see the project repository.
