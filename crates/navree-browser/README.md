# navree-browser

Navree browser testing: drive Chrome or Chromium over the DevTools Protocol from Rust tests, with automatic waiting and failure screenshots.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is opt-in. Turn on the `browser` feature of `navree` to get `navree::browser`:

```sh
cargo add navree --features browser
```

You can also depend on it alone with `cargo add navree-browser`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/packages/browser-testing/)
- [API reference on docs.rs](https://docs.rs/navree-browser)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-browser)

## Rust version and licence

This crate needs Rust 1.95 or later.

Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
