# High availability

Kolme is designed to make high availability deployments of services possible, natural, and easy. The Kolme framework is built around horizontal scaling of nodes within a logical group to provide for high availability. Special care must be taken during version upgrades, which are handled separately.

<!-- toc -->

## Normal operations

Under normal, non-upgrade conditions, there are four different sets of Kolme nodes that may be running:

1. Processor
2. Listener
3. Approver
4. App-specific services (indexers, API servers, etc.)

While there are many ways to run each of these in a high-availability setup, let's review the standard Kolme recommendations.

### Listeners and approvers

These are the easiest to run in a high availability setup. Downtime for a listener or approver does not result in any downtime for the application itself. Instead, if sufficient nodes are down from either set to break the quorum, downtime will result in delays in fund transfers (listeners will block deposits, approvers will block withdrawals).

There are two ways to approach this:

#### Single copy

Don't worry about downtime, and just run one copy per signing key. This relies on the inherent protection provided by a multisignature set. Further, even if multiple nodes go down, the result is simply a delay, not an outage.

One thing to keep in mind with this approach is the quorum rules for a set. If you have a 2 of 2 listener set, for example, downtime from either node would result in blocked deposits. In essence, you have a disaster management OR relationship: if either node A or node B goes down, the entire set cannot proceed.

On the other hand, with a 3 of 5 listener set, you would need to have 3 nodes all fail at the same time before deposits become blocked.

#### Replicas

Alternatively, if you want to provide even stronger uptime guarantees, you can run multiple replicas. This has the downside of requiring more hardware, and will result in redundant external blockchain queries (for listeners) and redundant Kolme transactions being generated. However, both of these are ultimately hardware problems: extra queries to an external blockchain requires more hardware, and the redundant Kolme transactions will simply be rejected.

### Processors

Processors are the trickiest component from a high availability standpoint. Unlike approvers and listeners, we cannot simply let individual processors work in parallel. They'll produce conflicting blocks and could lead to a forked chain. While some blockchains--like Bitcoin--explicitly support chain forking and reconciliation, Kolme does not. Our performance model is based around irrefutable block production.

On the other hand, running just a single processor node isn't an option either. Unlike listeners and approvers, any downtime on the processor front results in the inability to perform any operations on the Kolme chain.

Our recommended high availability deployment, therefore, is:

* Run a cluster of 3 processor nodes in different availability zones (or equivalent for your hosting platform).
* Use the PostgreSQL [data store](storage.md). This provides a construction lock, which the processor will use to ensure it is the only nodes attempting to produce a block at a given time. Furthermore, it will use table uniqueness rules to ensure only one version of a block is saved to durable storage.

### Other services

The HA story for other services depends greatly on what they're doing. For example:

* An API server may simply be able to run multiple replicas of the service behind a load balancer. Without any data management, just ephemeral queries against the most recent block, the service would be trivially scalable.
* An indexer may want to take a batch approach: have one process responsible for filling up a SQL database, and a high availability cluster of machines serving data from that shared database.

## Startup time

When a node first launches, it will need to catch up to the latest state of the block. This will significantly impact startup time, which is especially important for high availability, ephemeral services. We have a few methods to speed this up:

1. Use persistent volumes with the Fjall data store. When a service restarts, it will be able to continue from its most recently stored block.
2. Use fully ephemeral storage and synchronize from other nodes in the network at launch. [Fast sync](node-sync.md) can speed this up by trusting the network's most recent app state.
3. While not currently supported, we may choose to add a data storage backend that stores the full chain state in shared storage. See discussions in [the storage page](storage.md).

## Version upgrades

Version upgrades represent a problem from Kolme, since the app logic is intrinsically linked with the version of the code. This is different from other smart contract chains, where the app logic lives in a binary blob--the contract code--which is separate from the underlying chain.

Our goal for version upgrades is to be _explicit_ and provide a zero-downtime upgrade process. Each release of the code has a version string associated with it. Let's consider a case of a chain running version 0.1.0, and wanting to upgrade to 0.1.1.

The first problem is reproducibility of the chain state. Most likely, there will be subtle (or not-so-subtle) differences in the state produced by old and new versions of the code. We need to ensure old versions of the code are never used for processing new blocks, and new versions of the code are never used for processing old blocks.

Let's step through the process.

1. App team releases the first version of the chain, running version 0.1.0. This ships with a source code release at commit A. That commit is audited. Processors, listeners, and approvers build binaries (or get checksummed binaries that have been audited) and begin running the chain. The genesis block includes the information "running version 0.1.0."
2. The app team continues working on new features and passes the code to auditors for review. After changes, we end up with a final commit B, defining code version 0.1.1.
3. The app team creates a new commit based on the original commit A. This commit just includes a new config setting for "next code version is called 0.1.1 and is ready." Call this commit A1.
4. The processor, approver, and listener teams deploy binaries built from commit B. These binaries will launch and observe the network, but will not do anything yet.
5. Once all teams report the new clusters are ready, deploy code version A1 to the current running clusters.
6. Upon launch, the new processor will produce a special transaction, a "propose upgrade" transaction. This will specify that the processor is ready to upgrade to version 0.1.1.
7. Upon seeing that transaction, listeners and approvers will send their own "confirm upgrade" transactions, providing signatures that they are ready to upgrade and in agreement with the upgrade.
8. When a quorum of listeners and approvers confirm the upgrade, the processor will produce a final "upgrade initiated." From that moment, none of the nodes running code version 0.1.0 will perform any actions, and the nodes running code version 0.1.1 (from commit B) will immediately take over.
9. After confirming that the upgrade has gone through correctly, the old clusters can be spun down.

There are tweaks that could potentially be made to the plan above. For example, instead of running two different clusters, we could have a single cluster with a rollout strategy, and set up the new code versions to crash on the old code. As we develop out the DevOps scripts we use, we will update docs for our most up-to-date recommendations.

One final question here is how do the new nodes synchronize state with the old chain version. Firstly, we will need a guarantee that the data storage is backwards compatible. It must be possible for the new version of the code to load old state versions.

We'll also need fast sync. This is the ability to download complete state data from other nodes in the chain. This is how old and new nodes will be able to transfer data to each other. This introduces some trust assumptions, in particular trusting that the processor's most recently published block is valid. We could also use some shared storage of blocks between old and new versions of the nodes so that the only trust is between your old and new processes. And finally, we could have special "trust this block" messages sent over the network to facilitate fast sync without centralized trust.

We'll flesh out the sync decisions as we implement the mechanism.
