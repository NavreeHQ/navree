# navree-plugin-notices

Notice board for a Navree app. It is not an official Navree package.

This crate is in the Navree repository as a worked example, built from the public API only. To use it in your own app, copy the folder and update both `navree` entries in its `Cargo.toml` (`[dependencies]` and `[dev-dependencies]`) to `version = "1.4", default-features = false`, keeping each entry's `features` list.

## Install

In your app:

```sh
navree plugin:add --path plugins/notices
```

Or wire it by hand in `Cargo.toml` (adding `notices`'s six features alongside the `navree` features your app already uses) and `src/app.rs`:

```toml
navree = { version = "1.4", features = ["helm", "orm", "queue", "schedule", "ui", "view"] } # plus your app's own features
navree-plugin-notices = { path = "plugins/notices" }
```

```rust
.module(navree_plugin_notices::NoticesModule::from_config())
```

## Configuration

Keys in `config/notices.toml` (published by `navree config:publish --plugin notices`; `config/notices.toml` is already scoped to `[notices]`, so keys sit at the top level):

```toml
enabled = true
keep_days = 30
digest_from = "notices@ludhiana-society.example"
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
