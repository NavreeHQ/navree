# Actix Web

| | |
|---|---|
| Runtime | Rust 1.99.1 (`rust:1-bookworm` build stage, `debian:bookworm-slim` runtime); release profile `opt-level = 3`, `lto = "thin"`, `codegen-units = 1` |
| Framework | Actix Web 4.18.1 (actix-http 3.20, actix-server 2.10, Tokio 1.54.1), default features |
| Workers | Actix's default: one worker per CPU (`available_parallelism()`, 8 under the benchmark's cpuset), each a single-threaded Tokio runtime |
| Database | sqlx 0.9.0 (Postgres, runtime query functions, not the compile-time macros), one `PgPool` of 64 connections created in `main` and shared by every worker through `web::Data` |
| JWT | jsonwebtoken 11.1.0 with the `aws_lc_rs` backend (HS256; `exp`, `iss` and `sub` required, `iss` must be `navree`), then the user row is loaded |
| Validation | validator 0.21.0 (derive) on serde-deserialized input |

Notes:
- Idiomatic Actix: `#[get]`/`#[post]` route macros, `web::Data<AppState>`, an `AuthUser` extractor (`FromRequest`), `web::Query` / `web::Json` with `QueryConfig` / `JsonConfig` error handlers that render JSON, `validate()?` in the handler, and one `AppError` enum implementing `ResponseError`. No middleware, no request logging.
- Status codes: validator failures are 422. Actix's extractors give 400 for anything they can't deserialize: an unparseable query string, malformed JSON, or well-formed JSON of the wrong shape (missing field, fractional amount, unknown stage).
- Actix polls a handler's extractors together. A missing or invalid token fails on the first poll, so it is always 401 first. But a well-signed token for an unknown user, combined with an unparseable query string or body, can get the 400 while the user row is still loading. Validator rules run in the handler, after authentication.
- `stage` deserializes into a Rust enum, so an unknown stage is a deserialization error rather than a validator rule.
- `/queries` runs its N queries one after another in a plain loop, and the deals list runs the count, then the page query (one join for the owners). Both are sequential, as straightforward handler code is; Express runs them concurrently.
- One pool shared by all workers, as Actix apps usually set up sqlx. Each connection's socket stays registered with the worker runtime that opened it, so a connection used by another worker is woken through that worker's I/O driver.
- sqlx prepares statements and caches them per connection.
- New deal ids are UUID v4. `created_at`/`updated_at` come from the database's `now()`, returned by `RETURNING`.
- jsonwebtoken 10+ requires a crypto backend feature. Its README's install line uses `aws_lc_rs`, so this app does too (Navree's workspace uses `rust_crypto`).
- sqlx has no TLS feature enabled, since the benchmark database is plain TCP. actix-web's default compression features are compiled in, but no `Compress` middleware is installed.
