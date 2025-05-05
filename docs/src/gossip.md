# Gossip

The gossip component is used in virtually all node software. It uses the `libp2p` library for establishing a peer to peer network. This network is used for handling all coordination among nodes, in particular:

* Peer discovery, using both mDNS (for local networks) and Kademlia (for globally distributed peers).
* GossipSub for gossiping notifications between nodes. This covers use cases like:
    * Broadcasting a transaction to the processor for inclusion in a block.
    * Processor providing newly produced blocks.
    * Notification of failed transactions.
    * Alert notifications about potential chain manipulation.
* A request/response system for more sophisticated communication. In particular, this is used for [synchronizing nodes](./node-sync.md).
