# Intro

Kolme is a Rust-powered blockchain framework that lets you build fast, secure apps with no delays or limits—run your own chain, connect anywhere, and ship products in record time.

## Why Kolme?

Kolme lets you build blockchain apps that are fast, secure, and free from the usual limits. Your app gets its own chain, runs at top speed, and connects anywhere, with built-in safeguards you can trust. It’s all about getting stuff done without the wait or hassle.  

## What Makes Kolme Different

Most blockchain apps split their work across slow smart contracts and clunky external servers, forcing delays, limits, and extra steps. Kolme gives your app its own chain, running everything in one fast, flexible Rust program.

Unlike smart contract, there's no need to fit your execution into a given CPU, storage, or gas limit. You can also securely load external data while processing, bypassing the need for oracles.

And by limiting the chain data to just your own application's operations, offchain processing becomes simpler and faster. And with no external node requirements to access the Kolme state, reliability issues from failing nodes and request throttling are no longer concerns.

## How Kolme Stays Secure

Kolme splits trust across three groups:

* **Listeners** watch for real events, such as fund transfers on external blockchains
* The **processor** produces blocks, and by running as a single node with no fixed block time, can run as fast as a centralized server
* **Approvers** validate the operations of the processor and sign off on fund transfers before they occur.

The system gains speed by centralizing its processing, while maintaining security with checks and balances from the listeners and approvers. Unlike fully centralized servers, all operations on Kolme remain transparent and auditable.
