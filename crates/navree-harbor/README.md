# navree-harbor

Navree Harbor: server provisioning over SSH, zero-downtime releases with health checks and rollbacks, and push-to-deploy, from `harbor.toml`.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is opt-in. Turn on the `harbor` feature of `navree` to get `navree::harbor`:

```sh
cargo add navree --features harbor
```

You can also depend on it alone with `cargo add navree-harbor`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/tools/harbor/)
- [API reference on docs.rs](https://docs.rs/navree-harbor)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-harbor)

## Rust version and licence

This crate needs Rust 1.95 or later.

Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
