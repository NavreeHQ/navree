# navree-plugin-notices

Notice board for a Navree app. It is not an official Navree package.

This crate is in the Navree repository as a worked example, built from the public API only. To use it in your own app, copy the folder and set the `navree` line to `navree = { version = "1.4", features = ["helm", "orm", "queue", "schedule", "ui", "view"] }`, with the same features.

## Install

In your app:

```sh
navree plugin:add --path plugins/notices
```

Or wire it by hand in `Cargo.toml` and `src/app.rs`:

```toml
navree = { version = "1.4", features = ["helm", "orm", "queue", "schedule", "ui", "view"] }
navree-plugin-notices = { path = "plugins/notices" }
```

```rust
.module(navree_plugin_notices::NoticesModule::from_config())
```

## Configuration

Section `[notices]` in `config/notices.toml` (published by `navree config:publish --plugin notices`):

```toml
[notices]
enabled = true
keep_days = 30
digest_from = "notices@ludhiana-society.example"
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
