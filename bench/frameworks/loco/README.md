# Loco (Rails for Rust: Axum + SeaORM)

| | |
|---|---|
| Runtime | Rust 1.99.1 (`rust:1-bookworm` build stage, `debian:bookworm-slim` runtime); release profile `opt-level = 3`, `lto = "thin"`, `codegen-units = 1` |
| Framework | Loco 1.2.0 (`loco-rs`, on Axum 0.8.9, hyper 1.12, Tokio 1.54.1), `LOCO_ENV=production` |
| Workers | One process on Tokio's default multi-thread runtime (`#[tokio::main]`): one worker thread per CPU, 8 under the benchmark's cpuset. Started in Loco's `ServerOnly` mode, as `cargo loco start` does |
| Database | SeaORM 2.0.4 on sqlx 0.9.0 (Postgres), Loco's pool from `database:` in `config/production.yaml`: `max_connections: 64`, `min_connections: 1` |
| JWT | jsonwebtoken 10.4.0 (`rust_crypto`, the version loco-rs 1.2 links for its own `auth::jwt`) in a custom extractor on top of Loco's token lookup and `auth.jwt.secret` config; HS256, `exp`/`iss`/`sub` required, `iss` must be `navree`, then the user row is loaded |
| Validation | validator 0.20.0 (derive) through Loco's `JsonValidateWithMessage` (body) and `QueryValidateWithMessage` (query string) extractors |

## Layout

The structure `loco new` generates for a REST API app with Postgres (`--db postgres --bg async --assets none`), minus what the benchmark doesn't use:

- `src/app.rs`: the `Hooks` impl (routes, boot, `after_context`).
- `src/controllers/{home,world,deals}.rs`: thin handlers returning `format::json` / `format::render()`.
- `src/models/_entities/`: SeaORM entities for `world`, `users` (Navree's table, mapped as it is: `id uuid`, `password_hash`, two-factor columns) and `deals`, in the compact format `cargo loco db entities` writes, with the `deals.owner_id` → `users.id` relation.
- `src/models/{world,users,deals}.rs`: finders and creation on `impl Model` (`find_random`, `find_by_id`, `page_with_owners`, `create`), the `before_save` hook that fills a new deal's id and timestamps, and the `CreateParams` body struct.
- `src/views/`: response types; entities never go on the wire.
- `src/extractors/auth.rs`: the `CurrentUser` extractor.
- `src/data/settings.rs`: the typed `settings:` block (`jwt_issuer`).
- `migration/`: the generated migration crate with an empty migration list (`create_app` needs a `MigratorTrait` type; the tables already exist and `auto_migrate` is `false`).
- `config/production.yaml`: the only environment the image runs.

Dropped from the generated app: the users migration, the auth controller and its mailers, the example worker and tasks, fixtures, tests, `dtos/` (ts-rs bindings) and the development/test configs.

## Production setup

- `loco-rs` with `default-features = false, features = ["with-db", "auth"]`: no SQLite driver, no job-queue backends (`worker`), no in-memory cache, no `cli` (clap).
- `src/bin/main.rs` does what `cargo loco start` does, through `loco_rs::boot`: load `config/<LOCO_ENV>.yaml`, start Loco's logger, `create_app` in `ServerOnly` mode, `boot::start` on `server.binding:server.port` (0.0.0.0:8080). The reason for not using `loco_rs::cli::main`: it builds one `AppContext` to read the config and `create_app` builds a second, so the CLI process keeps an extra database pool (one idle connection at `min_connections: 1`) beside the serving pool, which would break the 64-connection cap.
- Config values come from the environment through Loco's `<%= get_env(...) %>` interpolation: `DATABASE_URL`, `JWT_SECRET`, `JWT_ISSUER`.
- Logger: Loco's tracing subscriber, level `warn`, JSON format, `pretty_backtrace: false`.
- Pool: `connect_timeout`, `idle_timeout` (500 ms each) and `min_connections: 1` are the production template's defaults. SeaORM uses `connect_timeout` as the pool's acquire timeout.
- `workers.mode: BackgroundAsync`, as `--bg async` generates (the default, `BackgroundQueue`, refuses to boot without a `queue:` block). No workers are registered.

### Middleware

Loco's default production stack, with one change: the `logger` middleware (tower-http `TraceLayer`, a span per request) is disabled in config. What runs, outermost first:

| Middleware | State | What it does per request |
|---|---|---|
| `powered_by` | on (default) | adds `x-powered-by: loco.rs` |
| `fallback` | off (default in production) | (dev 404 page) |
| `request_id` | on (default) | reads or generates `x-request-id` (UUID v4) and echoes it |
| `logger` | **off** (`server.middlewares.logger.enable: false`) | (request trace spans) |
| `secure_headers` | off (default) | |
| `static_assets` | off (default; no assets) | (would serve files from disk) |
| `timeout_request` | off (default) | |
| `compression` | off (default) | |
| `remote_ip` | off (default) | |
| `etag` | on (default) | answers `304` when `If-None-Match` equals the response's `ETag` |
| `catch_panic` | on (default) | turns a handler panic into a 500 |
| `cors` | off (default) | |
| `limit_payload` | on (default, 2 MB) | axum's `DefaultBodyLimit` |

Plus the monitoring routes `AppRoutes::with_default_routes()` adds (`/_ping`, `/_health`, `/_readiness`). No enabled middleware touches the disk; the two that would (`static_assets`, `fallback` with a file) are off by default in production.

## Endpoints and status codes

- `/db`, `/queries`: `world::Model::find_random` (`Entity::find_by_id`, one `SELECT ... WHERE id = $1 LIMIT 1` each). `/queries` runs its N queries one after another in a plain loop.
- `GET /api/deals`: `CurrentUser`, then `QueryValidateWithMessage<ListParams>` (`page` ≥ 1, `per_page` 1..100, defaults 1 and 20), then `deals::Model::page_with_owners`: `Entity::find().find_also_related(users::Entity)` ordered by `created_at DESC, id DESC`, paged by Loco's `query::fetch_page`, which counts first (`SELECT COUNT(*) ... FROM (the query without ORDER BY/LIMIT)`) and then loads the page with one `LEFT JOIN users`. The SPEC's `meta` (`current_page`, `per_page`, `total`, `last_page`) is built from Loco's `PagerMeta`.
- `POST /api/deals`: `CurrentUser`, then `JsonValidateWithMessage<CreateParams>`; `deals::Model::create` inserts through the `ActiveModel` (`INSERT ... RETURNING`), and `before_save` sets the UUID v4 id and `created_at`/`updated_at` from the app clock. `201` via `format::render().status(StatusCode::CREATED).json(...)`.
- Errors are Loco's `Error` rendered by Loco (JSON bodies):
  - bad, expired, wrong-issuer or missing token, unknown user: `Error::Unauthorized` → `401 {"error":"unauthorized",...}`;
  - a validator rule fails (empty or long title, amount out of range, unknown stage, `page=0`, `per_page=101`): `Error::Validation` → `400 {"errors":{field:[...]}}`;
  - an unparseable query string (`page=abc`): `400`;
  - malformed JSON: `400`; well-formed JSON of the wrong shape (missing field, fractional amount): axum's `JsonRejection` status, `422`.
- Extractors run in argument order, so authentication (401) always comes before validation.

## Judgement calls

- **Custom token extractor instead of `auth::JWT` / `auth::JWTWithUser`.** Loco's extractors decode Loco's own claims (`pid`), decode `auth.jwt.secret` as base64, default to HS512 and don't check `iss`, so they can't validate Navree's token. `CurrentUser` keeps the Loco pieces that fit: `extract_token(get_jwt_from_config(ctx)?, parts)` (honours `auth.jwt.location`) and the secret from `auth.jwt.secret`; it decodes with `jsonwebtoken` (as `loco_rs::auth::jwt` does), with `leeway = 0` like Loco's own validation. The `DecodingKey` and `Validation` are built once in `Hooks::after_context` and kept in `ctx.shared_store`; the extractor reads them from there per request.
- **jsonwebtoken 10.4 and validator 0.20, not 11.1 / 0.21.** validator is forced: Loco's `ValidatorTrait` (what `JsonValidate*` / `QueryValidate*` require) is implemented for validator 0.20's `Validate`. jsonwebtoken 10 is the major loco-rs links, so there is one JWT stack in the binary; 11 would build a second copy for no functional difference.
- **No `cli` feature.** See *Production setup*: same boot path, one pool instead of two, and no clap in the build.
- **`stage` is a `String` with a custom validator rule**, not a Rust enum: `cargo loco db entities` maps `varchar(20)` to `String`, and a rule keeps the error a per-field validation error (400) instead of a deserialization error.
- **Owner via `find_also_related` (LEFT JOIN, all `users` columns)**, the ORM's standard eager load for a `belongs_to`; a partial model selecting only `id, name` would be a hand-tuned query.
- New deals' timestamps come from the app clock in `before_save` (Loco's generated `before_save` already owns `updated_at`); the table has no column defaults.
