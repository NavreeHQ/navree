# navree-pdf

Navree's PDF object layer: a reader held to a memory and work budget, a writer, and text extraction in reading order.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is opt-in. Turn on the `pdf` feature of `navree` to get `navree::pdf`:

```sh
cargo add navree --features pdf
```

You can also depend on it alone with `cargo add navree-pdf`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/packages/documents/)
- [API reference on docs.rs](https://docs.rs/navree-pdf)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-pdf)

## Rust version and licence

This crate needs Rust 1.95 or later.

Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
