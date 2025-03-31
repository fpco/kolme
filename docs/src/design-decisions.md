# Design decisions

Eventually we'll get this page more well organized. For now, it's a place to put any design decisions we come up with that don't fit nicely into another page.

* We will not include any failed transactions in the chain history. Since we don't batch transactions for publishing in a block, we can get away with simply rejecting invalid transactions. It will be important to include censorship detection to make sure the processor isn't simply omitting transactions it doesn't like.
