# Failed transactions

There are two different categories of "failed transactions" within Kolme: transactions submitted within Kolme that are rejected by the processor, and transactions that fail on an external chain. We'll cover each of these categories separately.

<!-- toc -->

## Kolme chain transaction failures

The process for submitting a transaction for inclusion in a block is:

* Broadcast the proposed, signed transaction to any node in the network
* Node uses the gossip component to share that proposed transaction with other nodes
* One of the processor nodes

Eventually we'll get this page more well organized. For now, it's a place to put any design decisions we come up with that don't fit nicely into another page.

* We will not include any failed transactions in the chain history. Since we don't batch transactions for publishing in a block, we can get away with simply rejecting invalid transactions. It will be important to include censorship detection to make sure the processor isn't simply omitting transactions it doesn't like.

## External chain failures

### User deposits

### Failed actions/withdrawals

### Invalid funds deposited
