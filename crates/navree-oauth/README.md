# navree-oauth

Navree OAuthServerModule: an OAuth 2.1 authorization server and OpenID Connect provider with scopes, consent and device codes.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is opt-in. Turn on the `oauth` feature of `navree` to get `navree::oauth`:

```sh
cargo add navree --features oauth
```

You can also depend on it alone with `cargo add navree-oauth`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/packages/oauth-server/)
- [API reference on docs.rs](https://docs.rs/navree-oauth)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-oauth)

## Rust version and licence

This crate needs Rust 1.95 or later.

Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
