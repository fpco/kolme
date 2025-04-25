# Node sync

TODO

* Slow mode: synchronize each block, run the transaction locally
* Fast mode: grab latest block and download all its Merkle hashes
* Slow mode: more secure
* Fast mode: still secured by processor signature
* Fast mode is necessary for version upgrades (can't rerun old blocks)
* Key rotation reference: need to carry proof that new processor is the right processor
