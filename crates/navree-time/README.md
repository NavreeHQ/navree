# navree-time

Navree TimeModule: tenant time zones, local days, send windows, holidays and local-time SQL.

Part of [Navree](https://navree.com), the complete application framework for Rust.

## Time zone tables

The zone transitions, standing rules, links, legacy aliases and country zones in `src/tables.rs` are compiled from the public-domain IANA time zone database (`tzdata`), with weekend days from Unicode CLDR. No system `zoneinfo` files are read at run time.

To update the tables for a new IANA release, unpack `tzdata` into a folder outside the repository and run:

```sh
cargo xtask tz-tables path/to/tzdata
```

Pass `--check` to verify that `src/tables.rs` matches the unpacked release without writing.

<!-- crate-readme:start -->
## Use it

It is part of `navree`'s default `full` feature set, as `navree::time`:

```sh
cargo add navree
```

You can also depend on it alone with `cargo add navree-time`.

## Documentation

- [The guide on navree.com](https://navree.com/docs/digging-deeper/time-zones/)
- [API reference on docs.rs](https://docs.rs/navree-time)
- [Source and issues on GitHub](https://github.com/NavreeHQ/navree/tree/main/crates/navree-time)

## Rust version and licence

This crate needs Rust 1.95 or later.

Licensed under either of the MIT licence ([LICENSE-MIT](LICENSE-MIT)) or the Apache licence, version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), at your option.
<!-- crate-readme:end -->
