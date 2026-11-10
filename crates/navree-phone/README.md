# navree-phone

Navree phone numbers: parsing, validation, number types and formatting (E.164, international, national) on libphonenumber's numbering plans.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is opt-in. Turn on the `phone` feature of `navree` to get `navree::phone`:

```sh
cargo add navree --features phone
```

You can also depend on it alone with `cargo add navree-phone`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/basics/phone-numbers/)
- [API reference on docs.rs](https://docs.rs/navree-phone)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-phone)

## Rust version and licence

This crate needs Rust 1.95 or later.

Navree's own code is licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option. The licence expression of this crate is `(MIT OR Apache-2.0) AND Apache-2.0`.

It holds libphonenumber's numbering plans (Apache-2.0); the notice is in `THIRD_PARTY.md`.
