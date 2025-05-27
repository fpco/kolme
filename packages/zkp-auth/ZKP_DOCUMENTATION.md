# Kolme ZKP Authentication Documentation

The `kolme-zkp-auth` crate provides comprehensive Zero-Knowledge Proof functionality for social authentication.

## 🏠 Main Library (`kolme_zkp_auth`)

### Core Types

#### `SocialPlatform`
Enum for supported social platforms: Twitter, GitHub, Discord, Google

#### `SocialIdentity`
User's social identity information (never stored on-chain)

#### `SocialIdentityCommitment`
Cryptographic commitment to a social identity

#### `ZkProof`
Zero-knowledge proof with proof data and public inputs

### Main Components

- **`Prover`** - Generates ZKP proofs for social identity ownership
- **`Verifier`** - Verifies ZKP proofs without revealing identity
- **Error handling** - Comprehensive error types with `ZkpAuthError`

## 🔐 ZKP Prover (`prover.rs`)

### Key Features

- **Proof Generation**: Generate proof of social identity ownership
- **Ownership Proofs**: Prove ownership with challenge-response
- **Key Management**: Load/set proving keys for Groth16 proof system
- **Commitment Computation**: Cryptographic commitments using SHA256 + field arithmetic

### Implementation Details

- Uses **Groth16 ZK-SNARKs** for efficient proof generation
- **BLS12-381 curve** for cryptographic operations
- **SHA256 hashing** for commitment computation
- **Random nonce generation** for commitment uniqueness

## 🔍 ZKP Verifier (`verifier.rs`)

### Key Features

- **Proof Verification**: Verify identity proofs without revealing identity
- **Ownership Verification**: Verify ownership with challenge-response
- **Batch Verification**: Efficient verification of multiple proofs
- **Key Management**: Load/set verifying keys for Groth16 verification

### Implementation Details

- **Prepared verifying keys** for optimized verification
- **Platform validation** ensures proof matches expected platform
- **Public input validation** verifies commitment format
- **Batch processing** for efficient multiple proof verification

## ⚡ ZKP Circuits (`circuits/`)

### Social Identity Circuit (`social_identity.rs`)

#### Purpose
Proves ownership of social identity without revealing the actual identity.

#### Circuit Inputs

**Private Inputs (Witnesses):**
- `social_id` - Social platform user ID
- `platform_secret` - Platform-specific secret
- `nonce` - Random value for commitment uniqueness

**Public Inputs:**
- `platform` - Social platform identifier
- `commitment` - Cryptographic commitment to the identity

#### Circuit Constraints

The circuit enforces:
1. **Non-zero constraints** - Prevents trivial proofs
2. **Commitment verification** - Ensures prover knows preimage
3. **Platform consistency** - Validates platform matching

### Selective Disclosure Circuit (`selective_disclosure.rs`)

#### Purpose
Prove specific attributes about identity without revealing the full identity.

#### Features
- **Age verification** - Prove age range without revealing exact age
- **Attribute comparisons** - Compare attributes against thresholds
- **Multi-attribute proofs** - Prove multiple attributes simultaneously
- **Privacy preservation** - Reveal only necessary information

## 🌐 Social OAuth Integration (`social/`)

### Provider Registry (`mod.rs`)

#### `SocialProvider` Trait
Common interface for all social platform integrations

#### `ProviderRegistry`
Manages multiple social providers

### Supported Platforms

#### Twitter (`twitter.rs`)
- **OAuth 2.0** integration with Twitter API
- **User profile** retrieval and validation
- **Rate limiting** and error handling

#### GitHub (`github.rs`)
- **OAuth 2.0** integration with GitHub API
- **User profile** and email verification
- **Organization membership** validation

#### Discord (`discord.rs`)
- **OAuth 2.0** integration with Discord API
- **User profile** and guild information
- **Bot integration** support

#### Google (`google.rs`)
- **OAuth 2.0** integration with Google APIs
- **User profile** from Google+ or Gmail
- **Email verification** and domain validation

### OAuth Client (`oauth_client.rs`)

#### Features
- **Token management** - OAuth token handling and validation
- **Rate limiting** - Built-in rate limiting for API calls
- **Error handling** - Comprehensive OAuth error handling
- **Retry logic** - Automatic retry for transient failures

## 🔒 Cryptographic Primitives

### Poseidon Hash (`hash.rs`)

#### Purpose
ZK-friendly hash function optimized for use in zero-knowledge circuits.

#### Features
- **Circuit-friendly** - Efficient computation in ZK circuits
- **Field element operations** - Native field arithmetic
- **Secure parameters** - Cryptographically secure constants
- **Batch processing** - Hash multiple inputs efficiently

### Simple Hash (`hash_simple.rs`)

#### Purpose
Standard hash functions for non-circuit operations.

#### Features
- **SHA256 integration** - Standard cryptographic hashing
- **Utility functions** - Common hashing operations
- **Performance optimized** - Fast hashing for large data

## 🔑 Key Management (`keys.rs`)

### Trusted Setup

#### Features
- **Proving key generation** - Generate keys for proof creation
- **Verifying key generation** - Generate keys for proof verification
- **Key serialization** - Save/load keys to/from disk
- **Security validation** - Verify key integrity

### Security Considerations

- **Trusted setup ceremony** - Secure multi-party computation
- **Key verification** - Validate key correctness
- **Secure storage** - Protect keys from unauthorized access
- **Key rotation** - Support for key updates

## 🛠️ Error Handling (`errors.rs`)

### Comprehensive Error Types

#### Platform Errors
- **`InvalidPlatform`** - Unsupported social platform
- **`SocialProviderError`** - Platform-specific API errors

#### ZKP Errors
- **`ProofGenerationFailed`** - ZKP proof generation errors
- **`ProofVerificationFailed`** - ZKP proof verification errors
- **`CircuitSynthesisError`** - Circuit constraint errors
- **`TrustedSetupNotInitialized`** - Missing proving/verifying keys

#### OAuth Errors
- **`OAuthValidationFailed`** - OAuth token validation errors
- **`TokenValidation`** - Token format or expiration errors
- **`InvalidToken`** - Malformed or invalid tokens
- **`InvalidCode`** - Invalid authorization codes

#### System Errors
- **`SerializationError`** - Data serialization/deserialization errors
- **`RequestError`** - Network request failures
- **`KeyNotFound`** - Missing cryptographic keys
- **`IOError`** - File system operation errors

## 🧪 Testing and Examples

### Comprehensive Test Suite

#### Unit Tests
- **Component isolation** - Test individual components
- **Edge case coverage** - Handle boundary conditions
- **Error path testing** - Verify error handling

#### Integration Tests
- **End-to-end flows** - Complete authentication workflows
- **Multi-platform testing** - Test all supported platforms
- **Performance testing** - Benchmark proof generation/verification

#### Circuit Tests
- **Constraint satisfaction** - Verify circuit correctness
- **Invalid input handling** - Test circuit security
- **Performance benchmarks** - Measure circuit efficiency

### Setup Utilities

#### Production Setup (`setup.rs`)
Generate trusted setup for production

#### Mock Setup (`setup_mock.rs`)
Generate mock setup for testing

## 📋 Key Documentation Features

### Production-Ready

- **Real cryptographic implementations** using arkworks ecosystem
- **Groth16 ZK-SNARKs** for efficient proof generation and verification
- **BLS12-381 elliptic curve** for security and performance
- **Poseidon hash function** for ZK-friendly operations
- **Comprehensive error handling** with detailed error types

### Privacy-Preserving

- **Zero-knowledge proofs** - Prove identity ownership without revealing identity
- **Selective disclosure** - Reveal only necessary attributes about identity
- **Commitment schemes** - Cryptographic commitments to user identities
- **No on-chain storage** - User identities never stored publicly or on blockchain

### OAuth Integration

- **Multi-platform support** - Twitter, GitHub, Discord, Google integrations
- **Standard OAuth flows** - Authorization code flow implementation
- **Token validation** - Secure OAuth token verification and management
- **Rate limiting** - Built-in API rate limiting and retry logic
- **Error resilience** - Comprehensive error handling for OAuth failures

### Performance Optimized

- **Efficient circuits** - Optimized constraint systems for fast proving
- **Batch verification** - Process multiple proofs efficiently
- **Prepared keys** - Pre-processed verifying keys for faster verification
- **Memory efficient** - Minimal memory footprint for large-scale deployment

This ZKP authentication system provides a complete, production-ready solution for privacy-preserving social authentication using cutting-edge zero-knowledge proof technology. The system enables users to prove their social media identity ownership without revealing their actual identities, supporting selective disclosure of attributes while maintaining full privacy and security.

## 🔮 Prediction Timestamping Integration

The ZKP authentication system seamlessly integrates with prediction timestamping services, enabling users to:

- **Submit anonymous predictions** with cryptographic commitments
- **Prove prediction ownership** without revealing identity
- **Verify prediction authenticity** using zero-knowledge proofs
- **Maintain privacy** while establishing credibility

This integration leverages the existing social identity commitments and ZKP circuits to provide tamper-proof, privacy-preserving prediction verification.