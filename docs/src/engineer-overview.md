# Engineer overview

Kolme is a Rust library for building blockchain apps with their own chains, driven by three core pieces—listeners, a processor, and approvers—working together to break free from smart contract limits. You write full Rust code, use any server-side tools you want—like databases or whatever else—and run fast with no delays, all while keeping it secure and connected.  

<!-- toc -->

## Core chain

The chain itself is made up of a series of blocks. Unlike other blockchains, a Kolme chain's blocks:

1. Have no specific timing. The processor can produce a new block at any time.
2. Always contain exactly one transaction.

The core of the Kolme codebase revolves around managing this chain. The core is made up of only one mutating function: adding a new block to the chain. Each block must be signed with the processor's signing key.

Beyond that, the core layer of Kolme provides a series of data lookups, the ability to subscribe to notifications (such as new blocks), and the ability to simulate running a transaction.

## Components

On top of this core, Kolme adds in *components*. Components leverage the core logic and provide additional functionality. Some components are integral parts of the system, such as the processor, listener, and approver components. Other components, like the gossip component (for peer to peer communication), are needed by most systems, but are technically optional.

In addition, individual applications can write their own components to handle custom needs. For example, indexers, API servers, bots, cron jobs, and more, can all be implemented as custom components within a Kolme application.

The upside of this is that you can easily extend your application without needing to split logic into multiple code bases, and without the need for error-prone network communications with an external blockchain node.

## External Data Handling

Kolme lets you load external data straight into your app. Each data load is recorded as part of the blockchain, allowing other nodes to validate the data itself, as well as rerun the transaction to confirm the processor has produced a valid state. This allows the processor code to run quickly and pull in the data it needs, without compromising security or relying on centralization of trust.

## High Availability Processor

Kolme’s processor runs in a high availability cluster for production apps. It uses leader election to allow multiple processor nodes to remain running, while ensuring only one of them produces blocks at a time. This provides a hot standby capability to ensure your application isn't taken down by a single machine failure.

## Network Syncing

Kolme ties it all together with a gossip network built on libp2p for discovering other nodes. Components use it to receive events like new blocks, query missing blocks if they’re behind, and share transactions with the processor, keeping everything in sync, fast, and reliable.
