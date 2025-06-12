# Version Upgrades

<!-- toc -->

A cornerstone of the security and transparency of Kolme is reproducibility in block production. This means that, given the same input chain state and the same block information (transaction and data loads), Kolme must guarantee byte-identical results for the various states it produces. This is the most important reason for including framework and app state hashes in the serialized blocks.

New versions of an application will almost always introduce changes in this binary output, either through changes in app logic, updates to underlying libraries, or modifications to the data storage format. To handle this in a reproducible, transparent manner, we explicit track version upgrades within the chain.

## Versions within Kolme

Kolme uses a simple string to represent versions. The motivation for this is that we only care about versions _matching_ or _not matching_, not whether a version is older or newer.

Versions appear in four different places:

* When creating a `Kolme` value, we specify the current version of the codebase. We call this the `code_version`.
* The framework state tracks the current version of the code used by the chain. This is the `chain_version`.
* The genesis information for a chain contains the initial value for the `chain_version`.
* The upgrade procedure (described below) includes admin messages that set the version, which will eventually be used to update the framework state's `chain_version`.

All operations that work on processing blocks (e.g., executing transactions, producing new blocks) check if the current chain version matches the running code version. If not, execution is blocked, since this may result in incorrect state representations. This necessitates the usage of fast sync, as described below.

## The upgrade process

Upgrading is handled as an admin message, where the validator set must propose and vote on a migration to a new code version. This follows the same voting procedure as used for other admin messages like validator set changes, namely that once 2 out of 3 groups within the validator set agree, the change goes through.

Let's consider a situation where we have a chain that started at version `v1` and is trying to upgrade to `v2`. The process works as follows:

1. One of the validators (e.g., one of the listeners) proposes an upgrade to `v2`. This updates the framework state's proposals data structure. This is generally handled by the upgrader component, described below.
2. Other validators detect and vote in favor of this proposal until a quorum is reached. (This is also handled by the upgrader component.) Once the quorum is reached, the processor running the `v1` code will produce one final block that switches the framework state's `code_version` to `v2`. At this point, the `v1` processor no longer produces any more blocks, since it's running the wrong code version.
3. Nodes on the network running `v2` are unable to execute any blocks that have occurred so far, since they are all using chain version `v1`. Instead, these nodes must use fast sync to transfer the entirety of the framework and app state directly to those nodes.
4. Once validators transfer the framework and app state, they resume chain operation as usual.

## Upgrader component

The upgrader component is a recommended component for all validators to run. It handles the logic of the upgrade procedure above, namely:

1. Check if the desired and actual chain version differ
2. Check if an existing proposal exists to move to the desired chain version
3. Voting on the existing proposal if it exists
4. Creating the proposal if it doesn't

Applications should accept runtime parameters (e.g., environment variables or command line arguments) to indicate if a version upgrade is desired. Note that the upgrader component must be running on the _old_ version of the code, e.g. the `v1` processors, listeners, and approvers from the example above.

Once the upgrade process is complete, the old nodes should be taken down, as they will simply drain network bandwidth by performing state transfers.

## Ensuring high availability

To keep high availability, we recommend the following deployment strategy:

1. Publish a new version of the executable with the new code version.
2. Launch a parallel set of all validator nodes running this new code version (`v2`).
3. Modify the existing validator nodes to begin running the upgrader component, setting the desired version to `v2`.
4. Wait for the chain to upgrade to `v2` (should be very fast once the validators are reconfigured).
5. Shut down the old `v1` validators.
