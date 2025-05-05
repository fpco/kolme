# Kolme third-party validator guide

This guide will help you set up and run Kolme's supporting services as a "validator" in the network.
As part of Kolme trust model, validators can run two services that work alongside the processor to
maintain security and reliability of the blockchain.

Kolme has the following three components

1. **Listeners**: Watch for external events (like deposits on other blockchains) and relay them to the Kolme chain
2. **Approvers**: Validate operations and sign off on fund transfers before they occur
3. **Processor**: Produces blocks and executes transactions (runs in a high-availability cluster)

As a validator, you'll be running listeners and approvers to ensure the security and reliability of
the network while the processor handles block production.

## Technical requirements

You'll need a machine that will fulfill the following specs:

- A dedicated public IP in order to communicate with the rest of the chain through libp2p.
- At least 100GB of storage.
- At least 2 CPU cores and 8GB of RAM.

## Setup

1. Download the kolme app binary from Github (TODO with link)

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
   ./target/release/kolme run-listener --config config.toml
   ```

## Running an approver service

Approvers validate operations and specifically focus on authorizing fund transfers from bridge
contracts.

### Approver Configuration

1. Create the config file TODO

2. Start the approver service:
   ```bash
   ./target/release/kolme run-approver --config config.toml
   ```

## Troubleshooting Common Issues

TODO

## Useful resources

- Slack/Discord channel: TODO
- Github repo: TODO
- Emails/Contact points: TODO
