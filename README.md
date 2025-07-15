# Kolme Framework

Documentation for the Kolme Framework is managed in our mdbook, available:

* [Within the repository](docs/src/SUMMARY.md)
* [On a hosted site](https://kolme.fpblock.com)

## Getting Started

To build and run the project locally:

1. **Install prerequisites**

You'll need `just` (a command runner) and `sqlx-cli`:

```sh
cargo install just
cargo install sqlx-cli
```

2. **Run the setup and tests**

The project uses SQLx with compile-time checks, so the PostgreSQL database and schema must exist before building. You can use the `just` command to set things up:

```sh
just sqlx-prepare
```

This will:
- Launch PostgreSQL inside a Docker container
- Apply schema migrations to that database
- Use the `cargo sqlx prepare` to generate cached query information


## License

This project is licensed under the MIT license. See the [LICENSE](LICENSE) file for details.
