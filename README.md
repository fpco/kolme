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

The project uses SQLx with compile-time checks, so the SQLite database and schema must exist before building. You can use the `just` command to set things up:

```sh
just test
```

This will:
- Create `local-test.sqlite3` if it doesn’t exist
- Run the SQL migrations
- Build and test the project


## License
This project is licensed under the GNU Affero General Public License v3.0 (AGPLv3). See the [LICENSE](LICENSE) file for details.
