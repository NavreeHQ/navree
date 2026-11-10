# navree-markdown

Navree's Markdown reader: CommonMark read into events with no raw HTML, no script link and no unbounded document.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is opt-in. Turn on the `markdown` feature of `navree` to get `navree::markdown`:

```sh
cargo add navree --features markdown
```

You can also depend on it alone with `cargo add navree-markdown`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/components/markdown/)
- [API reference on docs.rs](https://docs.rs/navree-markdown)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-markdown)

## Rust version and licence

This crate needs Rust 1.95 or later.

Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
