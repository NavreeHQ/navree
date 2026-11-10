# navree-grpc

Navree GrpcModule: tonic services in modules with dependency injection, guards and access rules; gRPC-Web, Connect, health and reflection.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is opt-in. Turn on the `grpc` feature of `navree` to get `navree::grpc`:

```sh
cargo add navree --features grpc
```

You can also depend on it alone with `cargo add navree-grpc`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/microservices/grpc/)
- [API reference on docs.rs](https://docs.rs/navree-grpc)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-grpc)

## Rust version and licence

This crate needs Rust 1.95 or later.

Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
