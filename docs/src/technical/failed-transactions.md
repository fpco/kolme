# Failed transactions

There are two different categories of "failed transactions" within Kolme: transactions submitted within Kolme that are rejected by the processor, and transactions that fail on an external chain. We'll cover each of these categories separately.

<!-- toc -->

## Kolme chain transaction failures

The process for submitting a transaction for inclusion in a block is:

* Broadcast the proposed, signed transaction to any node in the network
* Node uses the gossip component to share that proposed transaction with other nodes
* One of the processor nodes picks up the transaction
* The processor attempts to execute the transaction
* If the execution is successful
    * The processor produces a new block
    * The processor gossips that block to all nodes in the network
    * The nodes are able to observe that the transaction has been added and remove it from their mempools

If, however, the execution is unsuccessful, what do we do? Many blockchains will include the transaction as a failed transaction within a block. We could elect to do that as well in Kolme. However, doing so would unnecessarily bloat the size of our chain, something we're trying to avoid.

Instead, we simply drop the transaction, together with a gossiped notification indicating that the transaction failed. At that point, the processor's job is done.

For security and censorship protection, other nodes in the network _should_ confirm that the attempt to run that transaction fails. The purpose of this is to detect if the processor is unfairly rejecting transactions.

Current plan: This will ultimately be added as part of the watchdog component.

## External chain failures

External chain failures cover any kind of failure case with the bridge contracts on external chains.

### User deposits

User deposits can fail due to insufficient gas, insufficient user funds, or something else. In all such cases, either no transaction is generated, or a failed transaction is generated. In any event, these transactions should be completely ignored by listeners. They do not generate a new bridge event ID and are not relayed to the Kolme chain.

### Invalid funds deposited

A variation on the above is when a user deposits unsupported funds in a bridge contract. In the current design, those funds will simply be lost within the contract. This may sound surprising, but falls in line with standard behavior for unsupported transfers into a contract (e.g., using a `MsgBank` to transfer funds into a contract in Cosmos does not trigger execute messages).

We have potential alternatives to consider, such as:

* Keeping a list of permitted received coins/tokens, and rejecting transactions without them.
* Providing an administrative "send untracked tokens" feature for recovery of funds.

We'll wait for sufficient demand before implementing such a change.

### Failed actions/withdrawals

Kolme blocks can emit actions to be run on external chains. These actions have the potential to fail. Kolme provides a mechanism for one common failure mode (insufficient funds), and a back door for fixing any other failing action.

One thing to note in particular is that actions must be executed in ascending order. That means that if action 56 fails, no other transactions will be able to proceed. Therefore, unblocking a broken action is a requirement for correct chain operation.

#### Insufficient funds

It's possible for a Kolme application to generate a fund transfer message that cannot be supported on chain. A simple example would be a multichain application supporting USDC. A client can legitimately deposit 1,000 USDC on chain A, then issue a withdrawal request for chain B, essentially turning Kolme into a token bridge.

We need to hold off on issuing an action until there are sufficient funds on chain B. This may require generating an external bridge transaction, for instance, that will move USDC from one chain to another (probably using the [cross-chain transfer protocol](https://developers.circle.com/stablecoins/cctp-getting-started)).

To allow for this, we have the following model (**note** not yet implemented!):

* For each chain, we maintain an internal accounting balance of tokens held by the bridge contract. This can be calculated by summing deposits and withdrawals.
* Additionally, we provide listeners with a special message type to synchronize balances. This can be used to account for a token transfer initiated outside the normal deposit flow.
* When a transaction generates a fund transfer action, we check if there are sufficient funds to transfer. If so, we immediately emit the action. Otherwise, we add it to a FIFO queue of pending transfers per chain.
* Each time the balance of funds on a chain changes, we check the pending transfers queue and emit as many actions as we can currently support.

#### Hard override

Ideally no other actions should ever fail, assuming all code is written correctly. Given that such an assumption is guaranteed to be proven false, we include a hard override mechanism. This is a manual workaround for a stalled action.

Any approver may issue a message to either skip a bridge action, or replace a bridge action with a new action. The other approvers must confirm this message, with a final confirmation by the processor. Once the processor confirms the change, the new action is emitted by the chain, and the submitter components will attempt to broadcast the new transaction (or skip it).

Note that this is a fully manual process, intended to be used in exceptional circumstances.

### Hard fork/reverted transaction

If a blockchain has a hard fork or reverts a transaction we have already observed, we risk solvency of the system. For all bridge events added to Kolme that no longer exist on the destination chain (**note** not yet implemented):

* A listener can send a message (manually) requesting that an event be reverted.
* Once a quorum of listeners vote in agreement, the processor must confirm and add a new block to the chain.
* That new block will roll back the next expected event ID to the previous one.
* And, if possible, funds remaining in the account will be burned to revert the transaction. Unfortunately, if the funds have already been used, this will be impossible, and may introduce an insolvency issue.

The best defense against this is to ensure that listeners wait for sufficient confirmations on transactions before submitting them to Kolme.
