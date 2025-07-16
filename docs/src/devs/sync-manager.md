# Sync manager

<!-- toc -->

## Motivation

One of the core functionalities of Kolme's network layer (the Gossip component) is the ability to sync blocks. Block syncing can occur in two different ways:

* Block sync, where the raw block data is gossiped between nodes, and each node independently executes the transactions to generate blockchain state.
* State sync, where the entirety of the state is gossiped between nodes.

Block sync is great for the common case of staying up to date with the chain, and provides for better verification by insisting on executing transactions locally. However, state sync is vital because it allows for:

* Rapid synchronization of new nodes, without requiring running through the entire chain history and executing each transaction.
* Accounts for version upgrades, where one version of the code base cannot run both old and new blocks.

The logic for performing both block and state sync is surprisingly complex, exacerbated by interacting with libp2p and needing to deal with various network limitations such as rate limiting and data caps. To handle this complexity, Gossip has its own state machine, the sync manager, which handles the process of:

* Determining which block needs to be synced
* Determining whether to use block or state sync, based on config parameters and local versus remote chain state
* Tracking the complex process of collecting merkle layers for state sync
* Inserting new data into the data store

This document gives an overview of the sync manager to help developers understand how it works.

## State sync: naive version

The basic mechanism for state sync is simple:

* Find the latest block height (or a specific block height you're interested in).
* Use gossip to ask the network who has that block.
* Use libp2p's request/response to transfer that block.

The naive way of doing this is to serialize the entirety of the block and all of its Merkle data (framework state, app state, and logs) in one message. There are two problems with this:

1. It means that synchronizing additional blocks requires transferring all data again, bypassing the data deduplication logic of `merkle-map`.
2. For large enough stores, we blow past libp2p's 16mb buffer limit, making state transfer impossible.

Therefore, we need something more intelligent.

## State sync: real version

The real process of state sync is:

* Get the raw information on a block, which includes the Merkle hashes of the framework state, app state, and logs. (From now on, to avoid breaking fingers during typing, we'll just call these three "block state.")
* Request a single layer of the Merkle data for that block state. A single layer consists of the serialized content of that layer, plus the hashes of any children of that layer.
* Recursively download all children until we get to layers that either have no children, or for which we already have all the children.
* Store those "leaf layers" in the Merkle store.
* Traverse back up the tree, writing successive parents to the store as we discover all their children.
* When we finally store the entirety of the block state, write the block itself.

It's important that we only write a layer or block _after_ we get all the children layers. Otherwise, we would leave behind an inconsistent state in the Merkle store. For efficiency, we keep the invariant that a store must _always_ have all the children layers for any layer written in the store. This avoids the need for full recursive decent when resaving a Merkle map.

## Complexities

Unfortunately, there are quite a few complexities in implementing this:

1. `libp2p`'s implementation is highly state-machine focused. You can't simply write a recursive decent algorithm to continue querying additional layers. Instead, you need a state machine to track the progress.
2. We need to deal with the possibility of timeouts and nodes going offline.
3. We also want to be able to do this in a CPU-efficient way. Retraversing all pending layers each time we write a new layer would not be acceptable.

## `kolme::gossip::sync_manager` module

The heart of this implementation lives in the `kolme::gossip::sync_manager` module. This module provides a state machine with an API exposed to `kolme::gossip`. The API allows the gossip code itself to perform simple things:

* Request which pieces of data need to be requested from the network
* Submit data from the network back to the state machine

The sync manager provides a `Trigger` which the gossip layer subscribes to. Every time new work is available, the trigger is tripped, causing more network requests to be made. This allows us to batch data loads while still proceeding quickly through processing the data.

But the heart of the mechanism, and the heart of this document, is an explanation of the state machine itself.

## The state machine

The state machine explicitly tracks the syncing of only one block at a time. This is to deal with the realities of rate limiting within libp2p. Blocks are processed in ascending order.

A block first goes into the *needed* state, which means we need to download the raw block data from the network. Once we have the block, the sync manager determines whether we should perform a block or state sync. If we can perform a block sync, no further data downloads are needed, we execute the block, and we store the executed data in our data store.

However, if we need to perform a state sync, the entirety of the state data needs to be downloaded as well. We transition that block into the pending state and begin processing the layers.

We start off by setting the three top level layers (framework state, application state, and logs) as needed layers. Needed layers are requested from the network, and arrive with their payload and a list of their children.

Once we have all the children of a layer in the data store, we can add the parent. However, if a layer arrives with children we don't yet have, we have to:

* Add the parent layer to the pending layers map
* Add a reverse dependency from each child to the parent
* Add all the missing children to the needed layers list

The list of needed layers is fed (in rate-limited chunks) to gossip to request from the network. As each layer arrives, gossip calls back into sync manager to add the layer.

Each time a layer is completed and written to storage, we also need to "traverse up":

* Check all reverse dependencies to find the parents waiting for this layer
* For each parent, check if all of its children are now in the data store
* If so, recursively perform this process on the parent

Once all needed layers are downloaded, we have all of our block state available, and we can finally write the block to storage and proceed with any further block syncing.

## Peer discovery

We need to know how to find peers that have the data we need. We do this in two ways:

1. When gossip originally gets a response from a peer providing information on a block, we tag that peer as the peer we'll request data from. This gets stored in `RequestStatus`.
2. If a piece of data remains in `needed` for too long (by default, 5 seconds), we assume the peer we're talking to is slow or offline, and then we request additional peers from the network. This is tracked by the `request_new_peers` field of `DataRequest`.

## Choosing blocks

Gossip provides three different sync modes which impact how sync manager chooses which blocks to sync:

1. In block sync mode, we only ever use block sync, not state sync. To make that work, we must (1) be running the same code version as the chain version, and (2) need to synchronize all blocks from the beginning of the chain.
2. In state sync mode, we try to jump to the latest block via state sync. From then on, if possible, we'll use block sync to stay up to date. Additionally, if we're missing old blocks, the `wait_for_block` API call in Kolme will trigger a state sync of that older block.
3. In archive mode, we synchronize blocks using state sync, but ensure we have a full chain history from the beginning. This is good for running an archive node, thus the sync mode name.
