# navree-graphql

Navree GraphqlModule: a GraphQL API served from your modules on async-graphql, with access rules, dataloaders and subscriptions.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is opt-in. Turn on the `graphql` feature of `navree`:

```sh
cargo add navree --features graphql
```

You can also depend on it alone with `cargo add navree-graphql`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/microservices/graphql/)
- [API reference on docs.rs](https://docs.rs/navree-graphql)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-graphql)

## Rust version and licence

This crate needs Rust 1.95 or later.

Navree's own code is licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option. The licence expression of this crate is `(MIT OR Apache-2.0) AND MIT`.

It bundles GraphiQL and React (MIT); their licence texts are in `assets/`.
