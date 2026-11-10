# navree-micro

Navree MicroModule: request-response messages and events between services, with the same modules, guards, validation and unit of work as HTTP routes.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is opt-in. Turn on the `micro` feature of `navree` to get `navree::micro`:

```sh
cargo add navree --features micro
```

You can also depend on it alone with `cargo add navree-micro`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/microservices/introduction/)
- [API reference on docs.rs](https://docs.rs/navree-micro)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-micro)

## Rust version and licence

This crate needs Rust 1.95 or later.

Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
