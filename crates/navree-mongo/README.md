# navree-mongo

Navree on MongoDB: connections and pools, the ORM's document driver, the schema builder on collections, and migrations.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is opt-in. Turn on the `mongo` feature of `navree` to get `navree::mongo`:

```sh
cargo add navree --features mongo
```

You can also depend on it alone with `cargo add navree-mongo`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/orm/mongodb/)
- [API reference on docs.rs](https://docs.rs/navree-mongo)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-mongo)

## Rust version and licence

This crate needs Rust 1.95 or later.

Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
