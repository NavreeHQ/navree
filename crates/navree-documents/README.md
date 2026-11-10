# navree-documents

Navree DocumentsModule: PDF from HTML templates or a typed builder, DOCX output, and PDF merge, stamp and text extraction.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is opt-in. Turn on the `documents` feature of `navree` to get `navree::documents`:

```sh
cargo add navree --features documents
```

You can also depend on it alone with `cargo add navree-documents`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/packages/documents/)
- [API reference on docs.rs](https://docs.rs/navree-documents)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-documents)

## Rust version and licence

This crate needs Rust 1.95 or later.

Navree's own code is licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option. The licence expression of this crate is `(MIT OR Apache-2.0) AND OFL-1.1`.

It bundles the Noto fonts (OFL-1.1); the licence text is in `fonts/OFL.txt`.
