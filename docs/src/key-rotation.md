# Key rotation

<!-- toc -->

Note: at time of writing, nothing on this page is implemented. The current implemented strategy is that the processor, listener, and approvers keys and quorums are all defined in the code itself, become the genesis block, and never change. Consider this document a design doc for how key rotation will ultimately be implemented.

## Motivation

There are three groups of specially recognized public keys within Kolme: the processor node, the listener set, and the approver set. Each set has its own quorum rules, requiring a certain number of members from the set to perform their operations. Since the goal of the processor is to allow fast, centralized block production, the processor has only one key and operates autonomously.

Key rotation recognizes the fact that, at some point in the future, these keys may need to be replaced. Our design must handle these cases:

* Normal key rotation for security or hardware migration for a single operator (processor, listener, or approver). This should not require any assistance from other operators.
* A non-responsive or misbehaving operator needs to be replaced. Misbehaving can either mean:
    * The original operator is issuing incorrect data (e.g., a listener reporting on fund transfers that never happened).
    * A key has been compromised and is now being abused by a third party attacker.

In any of these cases, we need to both _initiate_ a key rotation, and then _execute_ a key rotation.

## Initiating key rotation

The use cases above can roughly be divided into "key replaces itself" and "others replace key." The former is a normal maintenance operation for network maintenance and does not require additional approval. The latter is a response to a security threat to the network, and requires quorum to initiate. Kolme provides two different routes for initiating key rotation.

### Self replacement

In the self replacement case, we have a single message that says "replace me as the processor, listener, or approver." The message fails if the signing key is not currently a member of one of those sets. If the same key is used in multiple sets, each set would require a separate message to initiate the change.

No further action is needed to initiate key rotation. As this point, the chain can proceed with the execute case.

### Change the set

Instead of replacing a single key, this message initiates a complete replacement of the current set of keys. The new set may contain any set of keys, including keys used in previous sets. This message includes:

* Processor key
* Listener keys and quorum requirement
* Approver keys and quorum requirement

Any member of the processor, listener, or approvers sets may propose a set change. Each set change gets its own unique ID (potentially the block height it was issued at), allowing multiple proposals to exist simultaneously to avoid a misbehaving set member from disrupting the voting process.

Any members of the current set can submit a message voting for the change. (Question: should we also support voting against?) Voting requires 2 out of 3 of the processor, listener, and approver sets to vote in favor of the change. For the listener and approver sets, a normal quorum is needed for the group to vote in favor of the change.

Once a change proposal receives enough votes, it is approved and can move on to execution. At that point, all previous proposals are canceled.

## Executing the change

Each accepted change is stored within the `FrameworkState`, in a `MerkleMap` with monotonically increasing keys. This sequence of changes includes the full signature history. The motivation of this is that, by just observing this history, you can prove the current set of keys. This allows for a secure [fast sync](./node-sync.md), requiring only trust in the original set of signers.

Immediately upon executing the change, the `FrameworkState` is also updated with the modified key set. All listener and approver actions will require a quorum from the new set of keys. If the processor key changed, the next block will be signed by that new key.

Each block that executes a key set change will also emit an external chain action to be performed. This will update the contract with the new set of keys. Note that this necessitates that all bridge contracts track not just the processor and approver keys for normal execution. It also means the contracts will need to be aware of listeners to validate a "change the set" action, which may rely upon listener votes to execute.
