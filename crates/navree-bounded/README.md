# navree-bounded

Navree's bounded in-process cache: sharded locks, a strict capacity, SIEVE eviction, TTL and TTI, and single-flight loading.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is always part of `navree`, the framework crate, as `navree::bounded`:

```sh
cargo add navree
```

You can also depend on it alone with `cargo add navree-bounded`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/architecture/crate-hierarchy/)
- [API reference on docs.rs](https://docs.rs/navree-bounded)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-bounded)

## Rust version and licence

This crate needs Rust 1.95 or later.

Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
