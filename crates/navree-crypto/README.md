# navree-crypto

Navree CryptoModule: AES-256-GCM encryption with key rotation, Argon2id hashing, token and JOSE helpers, TOTP and a QR code encoder.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is part of `navree`'s default `full` feature set, as `navree::crypto`:

```sh
cargo add navree
```

You can also depend on it alone with `cargo add navree-crypto`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/security/encryption-and-hashing/)
- [API reference on docs.rs](https://docs.rs/navree-crypto)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-crypto)

## Rust version and licence

This crate needs Rust 1.95 or later.

Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
