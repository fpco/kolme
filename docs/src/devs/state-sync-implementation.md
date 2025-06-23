# State sync implementation

<!-- toc -->

## Motivation

State sync is a feature of Kolme that allows nodes to transfer not just raw block information, but also the full framework and application state. This allows for:

* Rapid synchronization of new nodes, without requiring running through the entire chain history and executing each transaction.
* Accounts for version upgrades, where one version of the code base cannot run both old and new blocks.

The basic mechanism for state sync is simple:

* Find the latest block height (or a specific block height you're interested in).
* Use gossip to ask the network who has that block.
* Use libp2p's request/response to transfer that block.

The naive way of doing this is to serialize the entirety of the block and all of its Merkle data (framework state, app state, and logs) in one message. There are two problems with this:

1. It means that synchronizing additional blocks requires transfer all data again, bypassing the data deduplication logic of `merkle-map`.
2. For large enough stores, we blow past libp2p's 16mb buffer limit, making state transfer impossible.

Therefore, we need something more intelligent.

## Goal

The goal of state sync is:

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

## `kolme::gossip::state_sync` module

The heart of this implementation lives in the `kolme::gossip::state_sync` module. This module provides a state machine with an API exposed to `kolme::gossip`. The API allows the gossip code itself to perform simple things:

* Request which pieces of data need to be requested from the network
* Submit data from the network back to the state machine

At the gossip layer, we also keep a "trigger" mechanism, which will retrigger a request for state sync data:

* On a fixed interval, by default 5 seconds
* Any time new data comes in

This allows us to batch data loads while still proceeding quickly through processing the data.

But the heart of the mechanism, and the heart of this document, is an explanation of the state machine itself.

## The state machine

The state machine needs to keep track of two different kinds of data: blocks and layers. Blocks can depend on layers, and layers can depend on other layers.

* Blocks depend on layers that are defined in their block state.
* Layers depend on their children layers.

The state machine is based on three basic concepts:

* Needed data: these are pieces of data that we need from the network.
* Pending data: this is data that has been read from the network but can't yet be written to storage.
* Reverse data: tracks the reverse dependencies from a layer to both parent layers and blocks.

### Populating needed data

The state machine starts off empty: we don't need any data. The first interaction we have is when gossip notifies us that it wants a block. At that point, we add in the block height as a piece of needed data, both in `needed_blocks` and `queue` (see [the queue](#the-queue) below).

Once gossip receives data on a block, it will move that block from the needed data to pending data. It will also add the block data layers to needed data, so that gossip can query and populate the raw merkle layers.

### Populating pending data

When we move a block or layer from needed to pending, we need to make three different modifications:

* Remove the block height or layer hash from needed
* Add the block or layer data to pending
* Populate the reverse map from children layers back to the current block or layer

For all dependency layers, we populate them into needed data, thus creating our recursive traversal structure.

And finally, each time we receive a layer, we process it by traversing up.

### Traversing up

Each time we get a new layer, we check if all of its children are already present in the data store. If so, we're ready to write this layer to the data store too. The process is:

* Write the layer to the store
* Remove the layer from pending
* Recursively perform this process on any parent layers (tracked through the reverse data)
* Perform this process on any blocks depending on this layer

For blocks, the process is much simpler. We check that all three pieces of block data are in fact in the store and, if so, we write the block to storage. Done!

### The queue

It would theoretically be possible to simply traverse the needed data maps to find which pieces of data need to be requested. Instead, however, we keep a queue as a `VecDeque`, which allows us to continue cycling through all pending data fairly. This may not actually be that important in practice, but it seemed prudent during implementation.

While traversing the queue, we check to see if the data is still needed. If not, we drop it from the queue. This prevents us from needing to traverse the entire queue each time we move a layer or block from needed to pending, a nice CPU vs memory trade-off.

### Short-circuiting

The process described above is slightly inefficient. In many cases, we'll receive layers that can be immediately written to storage because all of their children are present. Less frequently, in fact perhaps never, the same may occur with a block. (It's unlikely to happen with a block since the framework state for a block should change in every single block, due to an incremented nonce in each block.)

In any event, in the code that adds blocks and layers to pending, there are checks to see if all children layers are already present and, if so, going directly to processing, bypassing the pending queue. This also avoids a potential bug of blocks and layers getting "lost" in pending.

### Peer discovery

Finally, we need to know how to find peers that have the data we need. We do this in two ways:

1. When gossip originally gets a response from a peer providing information on a block, we tag that peer as the peer we'll request data from. This gets stored in `RequestStatus`.
2. If a piece of data remains in `needed` for too long (by default, 5 seconds), we assume the peer we're talking to is slow or offline, and then we request additional peers from the network. This is tracked by the `request_new_peers` field of `DataRequest`.
