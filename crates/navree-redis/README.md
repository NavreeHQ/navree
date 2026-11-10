# navree-redis

Navree RedisModule: a shared Redis connection manager, and Redis drivers for the cache, locks, queue, sessions and rate limits.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Use it

It is opt-in. Turn on the `redis` feature of `navree` to get `navree::redis`:

```sh
cargo add navree --features redis
```

You can also depend on it alone with `cargo add navree-redis`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/digging-deeper/redis/)
- [API reference on docs.rs](https://docs.rs/navree-redis)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-redis)

## Rust version and licence

This crate needs Rust 1.95 or later.

Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
