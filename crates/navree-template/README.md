# navree-template

Navree's template engine: Jinja syntax, autoescaping by mode, a sandboxed value model and caps on depth, loops, work, output and allocation.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is opt-in. Turn on the `template` feature of `navree` to get `navree::template`:

```sh
cargo add navree --features template
```

You can also depend on it alone with `cargo add navree-template`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/digging-deeper/views/)
- [API reference on docs.rs](https://docs.rs/navree-template)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-template)

## Rust version and licence

This crate needs Rust 1.95 or later.

Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
