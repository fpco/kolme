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

TODO

### User deposits

### Failed actions/withdrawals

### Invalid funds deposited
