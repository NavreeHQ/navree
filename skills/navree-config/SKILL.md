---
name: navree-config
description: "Configures a Navree app: `.env`, `config/app.toml` (or a JSON file) and `APP_*` environment variables and their precedence, how values convert to your types, the file size and nesting limits, the application key and its rotation, typed `ConfigSection`s with validation and `Secret<T>`, reading `Config`, custom sources and configuration in tests. Use when adding a setting or a config section for a module, reading an environment variable, setting APP_KEY, keeping secrets out of logs, or when a setting doesn't take effect or a boot error names a section."
---

# Configuration

Every setting lives in one tree, built at boot from sources; each module
reads its section as a typed struct. A wrong type or a failed validation
stops the boot with a message naming the section (never the value).

## Where settings come from

`src/main.rs` passes `ConfigSources::defaults()`, which reads, in order:

1. `config/app.toml` (shared settings; committed; no secrets),
2. `config/<section>.toml`, one file per section, in name order (`config/mail.toml`
   holds the keys of `[mail]`, without the header),
3. `config/env/<env>.toml`, the file of the environment the app runs in,
4. `.env` (this machine's secrets; never committed; commit `.env.example`),
5. the process's `APP_*` environment variables.

**Later sources win.** So a key in `.env` overrides the same key in
`config/app.toml`, and a real environment variable overrides both. Set each
key in one place: a new app's `.env` holds only `APP_KEY` and `APP_MAIL__FROM`
and leaves the TOML keys (`http.addr`, `database.url`, `log.level`,
`mail.transport` and the rest) in the TOML file. When a key is set in both
with different values, the boot logs a warning that names the key, the `.env`
entry and the file, and says the `.env` value is used. Paths are relative to
the working directory: run the app from its root.

Look at the result with `navree config:show [section] [--defaults]
[--origin] [--fallbacks] [--json]` (secrets are masked, there is no option to
show them; `--fallbacks` lists the environment variables a key falls back to,
such as `REDIS_URL`, and whether each is set) and check it with
`navree config:check [--env production]`: all invalid sections at once, keys
and sections that nothing reads (usually typos; the boot warns about them
too), keys set in both `.env` and a file, and for production the unsafe
settings (no `app.key`, a SQLite `database.url`, `mail.transport = "log"`).
Both run without booting the app. `navree config:schema [--out <file>]`
writes a JSON Schema of the whole tree for the editor, and
`navree config:publish <section> [--force] [--stdout]` writes
`config/<section>.toml` with every key commented out beside its default, and
`navree config:publish --plugin <namespace> [--force]` writes one for every
section a plugin registers.
`navree gate` and `navree deploy` run `config:check`, and
`app.strict_config = true` makes the boot refuse an unknown key or a failed
setting instead of logging it.

The `app` section is typed (`config.app()`, `AppOptions`): `app.name`,
`app.url` (an absolute http or https address), `app.version`, `app.env`,
`app.key`, `app.previous_keys`. `APP_URL`, `APP_NAME`, `APP_VERSION`,
`APP_ENV`, `APP_KEY` and `APP_PREVIOUS_KEYS` set them. `config.environment()`
returns the `Environment`; ask `behaves_like_production()` instead of
comparing names.

Tables merge key by key; arrays, scalars and `null` replace whole; a source
that swaps a table for a scalar (or back) replaces the whole subtree. A
missing file is skipped; a malformed one fails the boot with file, line and
column. `.file("x.json")` reads JSON (an object, no comments); any other name
is TOML. Files over 1 MiB and nesting past 32 levels (files, `APP_A__B__...`
names, `.set` keys) fail the boot; `.max_file_size(bytes)` and
`.max_depth(levels)` change the limits.

## Naming

`APP_` + section + `__` + key, upper case: `APP_DATABASE__URL` is
`database.url`, `APP_HTTP__CORS__ALLOWED_ORIGINS` is
`http.cors.allowed_origins`, `APP_KEY` is the app key. A single `_` stays
in the name (`APP_HTTP__PER_IP_RATE_LIMIT`). Values are strings converted by
the section's types; settings that take a list also accept a comma-separated
string (`APP_HTTP__TRUSTED_PROXIES=127.0.0.1,::1`). Navree reads `.env`
itself and never copies it into the process environment, so
`std::env::var` doesn't see `.env` entries. Only `APP_` names are read
(`DATABASE_URL`, `PATH` and the rest never reach the tree, in `.env` either).

## How values convert

The same rules for every source: integers take integers, whole floats
(`30.0`, not `2.5`), digit strings (`"8080"`, `"+80"`, not `" 80"` or
`"0x50"`) and `true`/`yes`/`on` as 1; `bool` takes `true`/`false`, `yes`/`no`,
`on`/`off`, `1`/`0` in any case or a number; `String` takes any scalar;
`Vec<T>` takes arrays only, never a string; `Option<T>` is a missing key or
`null`, not `""`; enums match variant names exactly (case counts). Errors
name the section, the key (`servers[1].port`) and the file or variable the
value came from, and never a string value.

## The application key

```sh
navree key:generate     # prints base64:<32 random bytes>
```

`navree new` already wrote a generated key into the new app's `.env`.
`key:generate` prints another one, and `key:generate --write` puts it in
`.env` when `APP_KEY` is empty there (`--force` replaces a key that is set:
move the old one to `APP_PREVIOUS_KEYS` first).

It encrypts data (AES-256-GCM: two-factor secrets, session cookies) and is
the base of the derived keys for JWTs (unless `auth.jwt.secret`), signed URLs
(unless `http.signing_key`) and the realtime relay (unless
`realtime.secret`). Every process of the app needs the same key. Without
one the app boots with a warning (so `key:generate` works), but encryption
fails and tokens/links use a random key per process. To rotate, move the old
key to `APP_PREVIOUS_KEYS` (comma-separated, newest first) and set the new
one: previous keys only decrypt; derived keys (JWTs, signed links) change,
so users sign in again.

## Your own section

```rust
use navree::config::ConfigSection;
use navree::contracts::Secret;
use serde::Deserialize;

/// The `billing` section (`[billing]` in config/app.toml, `APP_BILLING__*`).
#[derive(Clone, Debug, Deserialize, ConfigSection)]
#[config(key = "billing", validate = Self::check)]
#[serde(default)]                       // a missing section or key takes the defaults
pub struct BillingOptions {
    /// The ISO code of the currency of invoices.
    pub currency: String,
    /// Days of a free trial.
    pub trial_days: u32,
    /// The payment provider's key.
    pub api_key: Option<Secret<String>>, // APP_BILLING__API_KEY; prints [redacted]
}

impl Default for BillingOptions {
    fn default() -> Self {
        Self { currency: "EUR".to_owned(), trial_days: 14, api_key: None }
    }
}

impl BillingOptions {
    fn check(&self) -> Result<(), String> { // a failure stops the boot
        if self.currency.len() == 3 {
            Ok(())
        } else {
            Err("currency must be a 3-letter ISO code".to_owned())
        }
    }
}
```

The derive writes the key, the check, the defaults (with `#[serde(default)]`
and `Default`) and a description of every field from its doc comment, which
`config:show --defaults`, `config:schema`, `config:publish` and the
configuration reference use. Register the section in the module with
`#[module(config = [BillingOptions])]` (or `m.config::<BillingOptions>()` in
`wire`, from the prelude's `ModuleBuilderConfigExt`), then inject
`Arc<BillingOptions>` into the module's providers. A module that builds its
options itself registers the section with `m.config_section::<T>()` so the
commands still know its keys, and a module that reads `billing.<provider>`
claims them with `m.config_claim("billing")`. Read a secret with
`.expose()`. To let callers pass options in code too, write a configurable
module (`BillingModule::for_root(options)`; see `navree-modules-and-di`).

## Reading values directly

Inject `Arc<navree::config::Config>`: `config.get_string("app.url")`,
`config.get_bool("feature.x")`, `config.section::<T>("key")`,
`config.section_or_default::<T>("key")`, `config.typed::<T>()` (a
`ConfigSection`, validated). Its `Debug` output masks keys containing a word
of the list the whole framework shares (also the logger, Logbook, Lighthouse,
Fleet and the audit trail: `password`, `passwd`, `pwd`, `passphrase`,
`secret`, `token`, `authorization`, `cookie`, `credential`, `dsn`, `api_key`,
`apikey`, `private_key` and `connection_string`) or `key`, URL passwords and
secret-looking URL parameters (`?token=`): give secrets secret-looking names.

## Custom sources and environments

`ConfigSources::defaults()` already reads a file per environment:
`config/env/<env>.toml` is a whole tree laid over `config/app.toml` and the
section files, so it holds what differs in `production`, `staging` or
`testing`. `<env>` is the first of the real `APP_ENV` variable, `APP_ENV` in
`.env` and `app.env` in `config/app.toml`; with none of them it is `local`.
The overlay replaces on purpose, so a key it sets again is not reported as set
in two places. `.env` and `APP_*` variables still win over it. A file
`config/<section>.toml` holds the keys of one section without its header.

To read other places, build the sources yourself:

```rust
use navree::config::ConfigSources;

pub fn sources() -> ConfigSources {
    ConfigSources::new()
        .file("config/app.toml")
        .section_file("billing", "config/plans.toml") // keys of [billing]
        .dotenv(".env")
        .env()
}
```

`.set(key, value)` applies one value above every source;
`.env_prefixed("MYAPP")` reads another prefix (`""` would read every variable). Release builds default to
JSON logs and no template reloading; debug builds to pretty logs and reloading.

## Framework sections

`app` (`key`, `previous_keys`, `url`), `http` (`addr`, `api_prefix`,
`trusted_proxies`, `per_ip_rate_limit`, `rate_limit_ipv6_prefix` (64: IPv6
clients share a limit per /64), `body_limit_bytes`, `timeout_secs`,
`cors.allowed_origins`, `security_headers`, `server` (connection limits: see
navree-deployment), `multipart` (`max_parts`, `max_field_bytes`,
`max_file_bytes`, `max_header_bytes`)), `log`, `hashing`, `validation`,
`database`, `schema`, `orm`, `auth`, `audit`, `cache`, `queue`, `mail`,
`notify`, `storage`, `http_client`, `realtime`, `view`, `api`, `observe`,
`session`, `time`, `workspace`; the other modules read a section named
after them. Each module's skill lists its keys.

## In tests

`TestApp::builder()` reads **no** sources (no `.env`, no TOML, no
environment). Set what the test needs:

```rust,ignore
let app = TestApp::builder()
    .config("billing.trial_days", 30)
    .module(BillingModule)
    .build()
    .await;
assert_eq!(app.get::<InvoiceService>().await.trial_days(), 30);
```

To boot the whole app, build sources with `ConfigSources::new().set(..)`
(not `defaults()`, which reads the machine's `APP_*` variables).

## Common mistakes

- Setting a key in both `.env` and `config/app.toml`: `.env` wins and the
  boot warns; keep each key in one place.
- A typo in a key or section name: it is not an error, only a boot warning
  (`navree config:check` fails on it).
- Secrets in `config/app.toml` (committed): use `.env` or the environment,
  and `Secret<T>` in your section.
- A section struct without `#[serde(default)]` and a missing key: the boot fails.
- Expecting `APP_X=a,b` to fill a `Vec<String>`: strings never become lists
  unless the section deserializes the string itself.
- A fractional number for an integer field (`timeout_secs = 2.5`): rejected,
  not rounded.
- Different `APP_KEY`s for `serve`, `queue:work` and `schedule:work`.
- `http.per_ip_rate_limit = 0` (fails the boot): only `null` (in code or
  test config) turns it off.

## Docs

- https://navree.com/docs/getting-started/configuration/
- https://navree.com/docs/security/encryption-and-hashing/
