# navree-plugin-godam

Godam disks plugin for Navree apps. It is not an official Navree package.

This crate is in the Navree repository as a worked example, built from the public API only. To use it in your own app, copy the folder and update both `navree` entries in its `Cargo.toml` (`[dependencies]` and `[dev-dependencies]`) to `version = "1.4", default-features = false`, keeping each entry's `features` list. When copying the folder outside the Navree repository, also remove `tests/conformance.rs` and the four `navree-*` path entries in `[dev-dependencies]` (`tests/boots.rs` and `tests/contract.rs` depend on `navree` alone).

## Install

From a local checkout inside your app:

```sh
navree plugin:add --path plugins/godam
```

Once published to crates.io:

```sh
navree plugin:add navree-plugin-godam
```

## Configuration

Select `driver = "godam"` on a disk in `config/storage.toml`:

```toml
[disks.invoices]
driver = "godam"
prefix = "ludhiana/"
```

`GodamDisk::new` wraps an in-memory `object_store` under `PrefixStore`; pass another `object_store` implementation to `GodamDisk::from_store` or `GodamDisk::with_store` to serve a disk from a remote store.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
