# External Data Handling

<!-- toc -->

## Data Fetching Mechanism

Kolme enables applications to securely fetch external data—such as price feeds from Pyth, oracles from Chainlink, or proprietary APIs for sports betting odds—integrating real-world information into on-chain logic with transparency and reliability. This is critical for apps like trading platforms, cross-chain swaps, or betting systems operating across ecosystems like Solana, Ethereum, and Near.

- **Process**:
  - During transaction execution, the processor fetches data via HTTP requests to specified endpoints (e.g., Pyth’s price feed API, a custom sports odds service), as part of the deterministic Rust code defined by the application, per [Core Chain Mechanics](core-mechanics.md).
  - Fetched data is included in the block’s data load field, alongside cryptographic signatures or other verification metadata (e.g., API keys, checksums) to ensure authenticity.
  - Data loads are recorded in blocks with the transaction, metadata, logs, and state hashes, ensuring all nodes can access and validate the data during block verification.
- **Determinism**:
  - The processor ensures data fetching is deterministic by using consistent endpoints and parameters, producing identical results for the same transaction across nodes.
  - If external sources return inconsistent data (e.g., due to network issues), the processor retries or fails the transaction, broadcasting a failure notification via libp2p, as described in [Failed Transactions](failed-transactions.md).
- **Multichain Context**:
  - Data fetching supports multichain apps by integrating with sources relevant to Solana, Ethereum, Near, or other chains (e.g., Aptos, Cosmos). Developers can add new chains and their data sources without rewriting apps, per [External Chain Resilience](external-chain-resilience.md).

## Validation

To maintain security and trust, Kolme ensures external data is verifiable by all nodes, eliminating reliance on centralized oracles and addressing concerns about data integrity.

- **Node Verification**:
  - Non-processor nodes (listeners, approvers, community nodes) validate data loads during block verification by checking cryptographic signatures or re-fetching data from the same endpoint, as outlined in [Triadic Security Model](triadic-security.md).
  - If signatures are invalid or re-fetched data differs, nodes reject the block, flagging potential processor errors or tampering, with planned watchdogs enhancing detection, per [Watchdogs](watchdogs.md).
- **Signature Options**:
  - **Cryptographic Signatures**: Used for sources like Pyth, where data is signed by the provider, enabling nodes to verify authenticity without re-fetching.
  - **Re-fetching**: For unsigned data (e.g., public APIs), nodes re-fetch during validation, ensuring consistency. Kolme’s design minimizes latency by caching stable data where appropriate.
  - **Custom Verification**: Apps can define verification logic in Rust (e.g., checksums, API key validation), stored in blocks for transparency.
- **Security Benefit**: Validation ensures data integrity without single points of failure, supporting secure multichain operations and seamless chain migrations (e.g., adding Cosmos data sources).

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
