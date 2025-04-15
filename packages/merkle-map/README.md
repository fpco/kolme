# Merkle Map

A `HashMap`-like data structure, but:

* It's a base-16 tree
* Each subtree can be hashed and stored separately
* Cloning the structure is cheap by putting data behind `Arc`s

The goal is to allow for efficient management of data structures where we need to store and maintain historical versions. In particular, this is driven by the needs of the Kolme blockchain framework.
