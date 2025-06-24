# Storage

<!-- toc -->

## Understanding blocks

Each block in a Kolme action is made up of essentially five parts:

1. Metadata about the block: timestamp, signature, block height, previous block hash, etc.
2. The transaction itself: a request from a client to perform some state transformation. This includes messages (individual actions to take, which may be standard Kolme messages or app-specific messages), a submission timestamp, and signature information. (Note: in Kolme, each block always contains exactly one transaction.)
3. Any data loads required to reexecute the messages in the transaction.
4. Logs generating when running messages.
5. The new state of the blockchain. More on this below.

Our goal with Kolme is to maintain a lean chain of blocks that can be used to fully replay the chain history and validate that the state of the blockchain--as claimed by the processor that produces the blocks--is accurate.

## What is state?

Now let's return to a question from above: what's the state of the blockchain? It comes down to two pieces of data:

* Framework state: information that Kolme itself maintains about your app chain, such as account balances, public key associations, and bridge contract addresses.
* App state: fully defined by the application. This can be any arbitrary data that an application decides to store.

There are some interesting properties about these pieces of data:

* They can be relatively large, easily in the hundreds of megabytes for active applications.
* The data will change on nearly every single block. For example, the framework state will change every block due to usage of nonces from accounts.
* Most of the data, however, will remain unchanged.
* We need to be able to cheaply clone this data in memory for executing transactions. We need to cheaply clone because we need to be able to maintain the old state, either for supporting concurrent queries or rolling back a failed transaction.
* Since the data is large, we don't want to store the data itself inside a block. Therefore, we store only a hash of the data in a block. As a result, we need to be able to cheaply hash these states.

The storage mechanism of Kolme is built around optimizing for these properties.

## MerkleMap

The core data structure we leverage is a `MerkleMap`. This is a Rust data structure with a `BTreeMap`-like API. Internally, it is a base16 tree with aggressive caching, clone-on-write functionality, and various other optimizations. It won't be faster than a `BTreeMap` or `HashMap` for most operations. However, it provides an incredibly cheap `clone` (just an `Arc` clone) and does not require recomputing hashes for unchanged subtrees.
By using the `merkle-map` package for maintaining framework and app state, a Kolme application gets aggressive data sharing, further reducing the total storage size needed for holding onto state from multiple blocks, without requiring pruning of the data. `MerkleMap` data can also efficiently be transferred over a network, allowing for [fast sync](node-sync.md).

## Pluggable storage

Kolme offers a pluggable storage backend mechanism. Each storage backend needs to hold onto essentially three pieces of data:

* A mapping between hashes and payloads. This is used by `MerkleMap` for storage of state data.
* The block history itself. This contains just the hashes referencing the state, not the state itself.
* Some way of efficiently determining what the latest block is. This may be a separate field, or could be derived from the block history itself (such as a `MAX` query on a SQL database).

Additionally, some storage mechanisms provide a mechanism for a construction lock. This is used to allow multiple processors to run in parallel with only one producing a block at a time. This is part of Kolme's [high availability](high-availability.md) mechanism.

The following storage backends are currently provided.

### In memory

This is a simple storage mechanism intended only for testing. However, it could potentially be useful for ephemeral services. All data is kept in memory. There is a trivial construction lock provided to allow for better simulated testing.

### Fjall

Uses the [Fjall](https://docs.rs/fjall) crate as a local filesystem key-value store. This backend does not provide any construction lock mechanism.

### PostgreSQL

The PostgreSQL backend is primarily intended for high availability processors. It still uses Fjall for Merkle hash storage for efficiency, since in early testing using a PostgreSQL table for storing hashes was too inefficient.

Side note: It may be worth revisiting this in the future, and at the very least have a background synchronization job between hashes in the local Fjall store and the PostgreSQL database. This would allow for faster launch of new nodes in a cluster without needing to synchronize data from other nodes on the network.

In addition to providing storage of the block data within a PostgreSQL table, this backend also provides a construction lock. It leverages advisory locks in PostgreSQL.
