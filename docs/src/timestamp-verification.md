# Timestamp verification

Every transaction submitted to the Kolme network includes a timestamp of when--on the client machine--the transaction was generated. Similarly, every block contains a timestamp of the processor's machine time when producing the block.

Timestamps are generally considered "best effort" in Kolme, and shouldn't overall be used for any security-sensitive topics. Kolme does not take significant efforts to avoid clock skew among network participants. That said, we do implement the following basic verification checks:

* No nodes accept timestamps from the future. To account for clock skew, we define "future" as "more than 2 minutes ahead of the current machine clock time." Accepting here means accepting a transaction into a mempool, or accepting a block from the processor (checking the timestamps on both the block and its transaction).
* Block time must be monotonically increasing. Each subsequent block must take place after the preceeding block.
* The difference between a block's timestamp and its transaction's timestamp must always be, at most, 2 minutes. This also means that all transactions in the mempool can be flushed after 2 minutes, and should be resubmitted.

**NOTE** these checks have not been implemented at time of writing. This is currently a design document outlining plans.

## Relying on timestamp

While timestamps are in general best effort, if an application needs to rely on a timestamp it can do so. This follows the general trust model of Kolme: trusting the processor with significant autonomy. Consider, however, that a processor has the option of significantly changing timestamps, so that timestamps used for things like calculating payouts may be manipulated. This is probably fine for things like interest charges, where two minutes won't make much difference. If, however, this is used for something where a minute of difference is significant, it opens an attack vector for a processor to abuse timestamps.

## Watchdogs

We haven't built out the watchdog component yet. Its idea would be to observe the network and, if it observes potentially abusive operations, raise an alert. For timestamp verification, one possibility would be to track the transaction timestamps in the blockchain and, if they regularly arrive out of order, consider the possibility that the processor is reordering transactions.

Note that even this idea isn't fullproof, since it could simply be a consequence of clock skew among clients. More real world testing would be needed to ascertain if this is a good idea or not.
