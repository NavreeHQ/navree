# Axum (baseline for Rust: the plain Rust stack with no framework on top)

| | |
|---|---|
| Runtime | Rust 1.99.1 (`rust:1-bookworm` build stage, `debian:bookworm-slim` runtime); release profile `opt-level = 3`, `lto = "thin"`, `codegen-units = 1` |
| Framework | Axum 0.8.9 (hyper 1.12, Tokio 1.54.1) |
| Workers | One process on Tokio's default multi-thread runtime (`#[tokio::main]`): one worker thread per CPU, 8 under the benchmark's cpuset |
| Database | sqlx 0.9.0 (Postgres, runtime query functions, not the compile-time macros), one `PgPool` of 64 connections |
| JWT | jsonwebtoken 11.1.0 with the `aws_lc_rs` backend (HS256; `exp`, `iss` and `sub` required, `iss` must be `navree`), then the user row is loaded |
| Validation | validator 0.21.0 (derive) on serde-deserialized input |

Notes:
- Plain, idiomatic Axum: `State<AppState>` (pool plus JWT key), an `AuthUser` extractor (`FromRequestParts`), `ValidQuery<T>` / `ValidJson<T>` extractors that wrap axum's `Query` / `Json` and run `Validate` (the pattern from axum's `validator` example), and one `AppError` enum implementing `IntoResponse`. No tower-http layers, no tracing, no request logging.
- Extractors run in argument order, so authentication (401) always comes before validation.
- Status codes: validator failures are 422. Rejections keep axum's own status, re-rendered as JSON: an unparseable query string or malformed JSON is 400; well-formed JSON of the wrong shape (missing field, fractional amount, unknown stage) is 422.
- `stage` deserializes into a Rust enum, so an unknown stage is a deserialization error rather than a validator rule.
- `/queries` runs its N queries one after another in a plain loop, and the deals list runs the count, then the page query (one join for the owners). Both are sequential, as straightforward handler code is; Express runs them concurrently.
- sqlx prepares statements and caches them per connection.
- New deal ids are UUID v4. `created_at`/`updated_at` come from the database's `now()`, returned by `RETURNING`.
- jsonwebtoken 10+ requires a crypto backend feature. Its README's install line uses `aws_lc_rs`, so this app does too (Navree installs its own HMAC-only provider on RustCrypto `hmac`/`sha2`).
- sqlx has no TLS feature enabled, since the benchmark database is plain TCP.
