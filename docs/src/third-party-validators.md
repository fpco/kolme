# Kolme third-party validator guide

This guide will help you set up and run Kolme's supporting services as a "validator" in the network.

## Kolme architecture

Unlike traditional blockchains that force developers to split logic between slow smart contracts and
external servers, Kolme consolidates everything into a single, high-performance application chain
with:

- No smart contract limits: Run any code you want, without gas limits or storage constraints.
- Direct external data integration: Pull in real-world data during processing, with full
  verification.
- Single-transaction blocks: Each block contains exactly one transaction, created on-demand.
- Component-based design: Modular Rust framework with plug-and-play components sharing state.

### The Triadic Trust Model

At the heart of Kolme's security is its triadic trust model with three key components:

- Listeners: Continuously observe bridge contracts on external chains for relevant events (e.g., fund deposits), 
  sign them, and submit them to the processor.
- Processor: Produces blocks and executes transactions (runs in a high-availability cluster). By
  centralizing processing, Kolme achieves speeds comparable to traditional databases while maintaining
  blockchain transparency. 
- Approvers: Validate blocks produced by the processor. Specifically verify and sign off on outbound 
  actions (e.g., withdrawals), ensuring actions are legitimate before they are submitted to external chains.

As a validator, you'll be running listeners and approvers to ensure the security and reliability of
the network while the processor handles block production. Your role is critical in maintaining the
integrity of the system.

## Technical requirements

You'll need a machine that will fulfill the following specs:

- A dedicated public IP in order to communicate with the rest of the chain through libp2p.
- At least 100GB of storage.
- At least 2 CPU cores and 8GB of RAM.

## Setup

1. Download the six-sigma app binary from Github (TODO with link)

2. TODO How to join the network

3. TODO Configuration files or environment variables needed

4. Decide on a storage backend 

## Running a Listener Service

Listeners are responsible for watching external blockchains for events like deposits and relaying
them to the Kolme chain.

### Listener Configuration

1. Create the config file TODO

2. Start the listener service:
   ```bash
   $ ./six-sigma-app run-listener --config config.toml
   ```

## Running an approver service

Approvers validate operations and specifically focus on authorizing fund transfers from bridge
contracts.

### Approver Configuration

1. Create the config file TODO

2. Start the approver service:
   ```bash
   $ ./six-sigma-app run-approver --config config.toml
   ```

## Troubleshooting Common Issues

TODO

## Useful resources

- Slack/Discord channel: TODO
- Github repo: TODO
- Emails/Contact points: TODO
