# navree-plugin-dakiya

Dakiya mail plugin for Navree apps. It is not an official Navree package.

This crate is in the Navree repository as a worked example, built from the public API only. To use it in your own app, copy the folder and set the `navree` line to `navree = { version = "1.4", features = ["http-client", "mail"] }`, with the same features.

## Install

From a local checkout inside your app:

```sh
navree plugin:add --path plugins/dakiya
```

Once published to crates.io:

```sh
navree plugin:add navree-plugin-dakiya
```

## Configuration

Select the transport in `config/mail.toml` and configure its endpoint under `[mail.dakiya]`:

```toml
[mail]
from = "Harpreet Sandhu <harpreet@ludhiana.example>"
transport = "dakiya"

[mail.dakiya]
url = "https://api.dakiya.example/v1/send"
# Set APP_MAIL__DAKIYA__API_KEY in .env or the environment.
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
