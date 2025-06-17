# External Data Handling

<!-- toc -->

## The problem with oracles

In standard blockchain architecture, smart contracts are unable to directly load any data from outside the blockchain. As a result, any data that a smart contract will use must be provided on-chain. The standard solution for this is _oracles_: smart contracts or other mechanisms that bridge external data to the chain. This can be used for providing such information as:

* Pricing for futures markets
* Real-world results for predictions markets
* Live odds for gambling applications

While oracles allow such applications to work, they introduce a number of complications:

1. You're limited to what data you can pull into your application. By contrast, with non-blockchain applications, your application can pull in data from any data source it can access.
2. Oracles can be a source of security holes, specifically around the robustness of their upgrade mechanism, and censorship attacks through congestion of the underlying chain.
3. Updates to oracles on many blockchains represent a significant gas cost to the operators.

Kolme takes a different approach to loading external data.

## Data Fetching Mechanism

Kolme allows an application to load up any piece of data from any source, just like non-blockchain applications. Normally, this would defeat reproducibility and transparency. However, Kolme's approach requires that all external data loads be logged in the block itself when produced by the processor.

As an example, consider the pricing information from the Pyth Network, which includes cryptographic Wormhole signatures attesting to the validity of the data. A Kolme application can pull in these attestations. The Kolme framework will automatically include the full attestation in the block itself, and any node in the network will be able to rerun the block, validating that the cryptographic signatures match and that the output of running the transaction is identical.

With this approach, a Kolme application can automatically leverage existing oracle data on any existing blockchain. But additionally, a Kolme application can pull in data from any other source. While ideally this data should be verifiable--either via querying an external source or validating signatures--Kolme leaves the application developers the latitude to choose their own security and trust models.

As an example, some applications may require access to proprietary data sources that cannot be validated by external parties. Kolme is unopinionated about this. It is the decision of each application how its trust model works, and users of an application can make informed decisions about how much trust to place in the application authors and validators.

## Process

- During transaction execution, the processor fetches data via HTTP requests to specified endpoints (e.g., Pyth’s price feed API, a custom sports odds service), as part of the deterministic Rust code defined by the application, per [Core Chain Mechanics](core-mechanics.md).
- Fetched data is included in the block’s data load field, alongside cryptographic signatures or other verification metadata (e.g., API keys, checksums) to ensure authenticity.
- Data loads are recorded in blocks with the transaction, metadata, logs, and state hashes, ensuring all nodes can access and validate the data during block verification.

## Validation

To maintain security and trust, Kolme ensures external data is verifiable by all nodes, eliminating reliance on centralized oracles and addressing concerns about data integrity.

- Non-processor nodes (listeners, approvers, community nodes) validate data loads during block verification by checking cryptographic signatures or re-fetching data from the same endpoint, as outlined in [Triadic Security Model](triadic-security.md).
- If signatures are invalid or re-fetched data differs, nodes reject the block, flagging potential processor errors or tampering, with planned watchdogs enhancing detection, per [Watchdogs](watchdogs.md).
- By rejecting new blocks, validators are able to stop fund transfers from being approved, protecting user funds.

## Advantages

Kolme’s external data handling offers significant benefits over traditional blockchain platforms, addressing developer pain points:

- **No Oracle Delays**: Unlike Ethereum or Solana, which rely on oracles like Chainlink that introduce latency and potential failures, Kolme’s direct HTTP fetching delivers near-instant data access, critical for time-sensitive apps like trading or betting.
- **Flexible Data Sources**: Supports any data provider—public oracles (Pyth, Chainlink), proprietary APIs, or custom feeds—without requiring specialized oracle contracts, enabling apps like sports betting with unique data needs.
- **On-Chain Transparency**: Data loads and signatures are stored in blocks, verifiable by all nodes, unlike off-chain oracle systems that obscure data provenance, enhancing trust for users and auditors.
- **Deterministic Execution**: Ensures consistent data across nodes, maintaining blockchain integrity, as detailed in [Core Chain Mechanics](core-mechanics.md).
- **Multichain Scalability**: Easily integrates data for new chains (e.g., Aptos) without app rewrites, supporting Kolme’s extensible multichain design, per [External Chain Resilience](external-chain-resilience.md).
- **No Gas Costs**: Data fetching incurs no fees, unlike gas-heavy oracle calls on shared blockchains, allowing cost-free, complex data operations.

These advantages make Kolme a developer-friendly platform for building data-driven, multichain applications with security and performance.

## Implementation Considerations

Developers integrating external data into Kolme apps should consider:

- **Source Reliability**: Choose stable, high-availability data providers (e.g., Pyth for price feeds) to minimize fetch failures, which trigger transaction failures and notifications, per [Failed Transactions](failed-transactions.md).
- **Signature Strategy**: Use signed data where possible to reduce re-fetching overhead, balancing performance and security. For unsigned data, ensure endpoints are consistent to avoid validation failures.
- **Caching**: Implement app-level caching in Rust logic for frequently accessed data (e.g., stable sports odds), reducing external requests while maintaining determinism.
- **Multichain Data**: Design data fetching to support multiple chains’ ecosystems, leveraging Kolme’s flexibility to add new sources as chains are integrated, per [External Chain Resilience](external-chain-resilience.md).
- **Error Handling**: Handle fetch failures gracefully in Rust code, using Kolme’s failure notifications to inform users, ensuring a robust user experience.

These considerations ensure efficient, secure data integration, aligning with Kolme’s high-performance, transparent architecture.
