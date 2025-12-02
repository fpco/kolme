# Versioning policy

<!-- toc -->

Kolme is a Rust library, and as such can follow a standard semver-inspired versioning scheme for backwards compatibility of the API. However, due to the nature of how Kolme is used, the story is a bit more complicated. In particular:

1. Changes to Kolme may impact downstream applications using its APIs. This is the standard versioning issues libraries face.
2. New versions of Kolme may modify any number of protocol components exposed to the outside world, network APIs and serialization formats being the most obvious.
3. When applications themselves make changes, they may change their own APIs, serialization, or application logic, all of which would make it impossible to reproducibly reexecute prior transactions.

The last point is mostly out of scope for this document. It is a responsibility of application authors, and is enabled by the [version upgrade system](../technical/version-upgrades.md) provided by Kolme. Please see that document for a better understanding of the goals in this document.

The purpose of this document is to ensure:

1. Developers of Kolme make changes in a way that allows for backwards compatibility with old serialized data.
2. We have a clear signposting mechanism for communicating breaking changes that downstream application developers need to handle.

## Single version number

In theory, we could use multiple version numbers for Kolme:

1. Version each sublibrary (like `merkle-map` separately).
2. Version the serialized block format.
3. Version the network API.

And we may ultimately decide to go in that direction. However, we're early in Kolme's development, and that level of complexity isn't currently warranted. Instead, we currently simply track one version number of Kolme: the version of the `kolme` crate itself. This represents all different pieces of the system as one.

We follow [Semantic Versioning (SemVer)](https://semver.org/) for this version number.

## Library versioning policy

As a Rust library, Kolme does not need to reinvent any wheels. We can follow standard Rust versioning rules. These are [documented at length](https://doc.rust-lang.org/cargo/reference/semver.html).

However, at its current state of development, Kolme does _not strive to keep stable APIs_. It is primarily an internal FP Block tool used for our internal development. As such, we strive to reduce unnecessary code breakage, but need not insist on such compatibility.

This _will_ change at some point in the future, but not yet.

As a result of this, we currently have no specific policy around whether a change below results in a major, minor, or patch version bump. We'll refine this over time.

## Application versioning impact

Applications maintain a version string to indicate compatibility with old block production, as discussed in the version upgrading guide. As a simple, conservative measure: any time you release any new version of the application, you should bump the application version number (code/chain version) and go through the full version upgrade process.

Technically, however, you only need to perform such a version bump if a change could result in differences in block production. In practice, almost any change could result in that, even a simple bump to a decimal library (since it may result in slightly different arithmetic results).

Unless explicitly stated otherwise, any change discussed below should be considered as _requiring a new application version_.

### Application versioning scheme recommendation

While you can theoretically use any string for the application version, we recommend a semantic versioning-like scheme to take full advantage of the tooling within the Kolme ecosystem. Our recommended format is `x.y.z` for standard releases, with an optional `a` suffix for hotfixes (e.g., `x.y.z.a`).

A key principle is to always increment version numbers sequentially. Never reuse or roll back a version number.

- **Major (`x`):** For sweeping, high-impact changes that alter large portions of the system's behavior, such as introducing a major new feature.
- **Minor (`y`):** For smaller feature additions that still require a chain upgrade.
- **Patch (`z`):** For bug fixes or polish that require redeployment, even if the scope is limited.
- **Hotfix (`a`):** Reserved for rare scenarios where a narrowly scoped patch is needed for a live network (e.g., mainnet). This is typically used when a fix already exists on your default branch (e.g., `main`), but you cannot deploy the entire branch and must apply only the critical fix.

Examples of this scheme include `1.0.0`, `1.0.1`, and `1.2.0`.

#### Tooling Compatibility

Kolme uses the [`version_compare`](https://docs.rs/version-compare/latest/version_compare/) crate to parse version strings. If you opt for a different versioning scheme, you must ensure it is compatible with this library. You can verify your version string using `kolme-cli`:

```
$ kolme-cli check-version 1.0.0
Supported

$ kolme-cli check-version abc
Not Supported
```

For a complete list of supported version formats, please refer to the `version-compare` [test cases](https://gitlab.com/timvisee/version-compare/-/blob/0c261ec99783236a5acfc511f62253ba2d3f21b8/src/test.rs#L20).

## Changing Merkle serialization

Merkle serialization is the most important piece of Kolme to maintain compatibility for. Without this, old block data will be unreadable by newer versions of the library.

Here are some basic rules:

* Any data structures that may be modified in the future should provide `MerkleSerialize`/`MerkleDeserialize` impls, instead of their `Raw` variants.
* Any modification to the serialized data must result in a bump to the `merkle_version` method's return value.
* As a strong recommendation, new fields should be added at the end of a data structure.
* Any newly added fields can be serialized as normal, but when deserializing, you need to check the version number and ensure the field is parsed only for versions it was serialized in.
* New fields must include a fallback value for parsing old data. This could either be via wrapping with `Option`, or providing a default value.

If all that seems a bit abstract, the easiest way to understand it is via the merkle-map versioning test code, e.g.:

```rust
#[derive(Clone, PartialEq, Eq, Debug)]
struct Person0 {
    name: String,
    age: u16,
}

impl MerkleSerialize for Person0 {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self { name, age } = self;
        serializer.store(name)?;
        serializer.store(age)?;
        Ok(())
    }

    fn merkle_version() -> usize {
        0
    }
}

impl MerkleDeserialize for Person0 {
    fn merkle_deserialize(
        deserializer: &mut merkle_map::MerkleDeserializer,
        version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            name: deserializer.load()?,
            age: deserializer.load()?,
        })
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Person1 {
    name: String,
    age: u16,
    street: String,
}

const DEFAULT_STREET: &str = "Default street";

impl From<Person0> for Person1 {
    fn from(Person0 { name, age }: Person0) -> Self {
        Self {
            name,
            age,
            street: DEFAULT_STREET.to_owned(),
        }
    }
}

impl MerkleSerialize for Person1 {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self { name, age, street } = self;
        serializer.store(name)?;
        serializer.store(age)?;
        serializer.store(street)?;
        Ok(())
    }

    fn merkle_version() -> usize {
        1
    }
}

impl MerkleDeserialize for Person1 {
    fn merkle_deserialize(
        deserializer: &mut merkle_map::MerkleDeserializer,
        version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            name: deserializer.load()?,
            age: deserializer.load()?,
            street: if version == 0 {
                DEFAULT_STREET.to_owned()
            } else {
                deserializer.load()?
            },
        })
    }
}
```

## Changes to logs

Changing log messages may seem like something that doesn't affect downstream. However, it's something that can cause breakage in two ways:

1. Some log messages may be relied upon and parsed by downstream tools.
2. Since hashes of logs are stored in blocks, any change in logging will impact reproducibility of blocks.

Make sure that any change to logs is well documented in the changelog.

## Changes to messages

This is more obvious than logs. Any change to built-in messages (admin, fund transfer, etc.) will result in changes to transactions and therefore blocks. This doesn't just apply to the API itself, but any change in the handling may result in differences in binary output.

## Modifying gossip

Gossip modifications are less severe than the changes above. They impact the network protocol, but do not directly affect block production. Keeping compatibility with the immediately prior version of gossip is a good thing for seamless migrations.

## Changelog

The changelog for Kolme is maintained in `CHANGELOG.md` at the repository root, following the [Keep a Changelog](https://keepachangelog.com/en/1.0.0/) format.

- The initial version of the changelog is generated using [git-cliff](https://github.com/orhun/git-cliff), which parses the commit history and creates a structured changelog.
- After the initial generation, the changelog is updated manually by the team for each release or significant change.
- There is no required commit message convention; all changelog updates are made directly in `CHANGELOG.md` as part of the release process.
- When making changes, update the relevant sections ("Added", "Changed", "Fixed", etc.) in `CHANGELOG.md` to reflect what has been done since the last release.
- The changelog is committed to the repository and should be kept up to date as part of the release process.

We're going to follow the "bump right before" strategy of bumping version numbers in Cargo.toml files just before cutting a release. That means that the repo will always have the _newest released version number_ in the Cargo.toml files.
