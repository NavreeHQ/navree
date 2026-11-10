# navree-db

Navree DatabaseModule: its own data layer on SQLx for PostgreSQL/TimescaleDB, MySQL/MariaDB and SQLite, with migrations, seeders and pagination.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is part of `navree`'s default `full` feature set, as `navree::db`:

```sh
cargo add navree
```

You can also depend on it alone with `cargo add navree-db`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/database/getting-started/)
- [API reference on docs.rs](https://docs.rs/navree-db)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-db)

## Rust version and licence

This crate needs Rust 1.95 or later.

Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
