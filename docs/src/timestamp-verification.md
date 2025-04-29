# Timestamp verification

Every transaction submitted to the Kolme network includes a timestamp of when--on the client machine--the transaction was generated. Similarly, every block contains a timestamp of the processor's machine time when producing the block.

Timestamps are generally considered "best effort" in Kolme, and shouldn't overall be used for any security-sensitive topics. Kolme does not take significant efforts to avoid clock skew among network participants. That said, we do implement the following basic verification checks:

* No nodes accept timestamps from the future. To account for clock skew, we define "future" as "more than 2 minutes ahead of the current machine clock time." Accepting here means accepting a transaction into a mempool, or accepting a block from the processor (checking the timestamps on both the block and its transaction).
* Block time must be monotonically increasing. Each subsequent block must take place after the preceeding block.
* The difference between a block's timestamp and its transaction's timestamp must always be, at most, 2 minutes. This also means that all transactions in the mempool can be flushed after 2 minutes, and should be resubmitted.

**NOTE** these checks have not been implemented at time of writing. This is currently a design document outlining plans.
