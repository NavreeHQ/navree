---
name: navree-new-app
description: "Creates and runs a new Navree app: installing the navree CLI, `navree new` and its options (`--database`, TimescaleDB by default), the starter kits (`--kit saas|api|micro`: a multi-tenant SaaS with a Next.js front end, the same API alone, or a microservice workspace), starting the database with `navree dock up`, APP_KEY, `navree dev`, the generated files, the first feature module and adding Navree to an existing Cargo package. Use when the user wants to start a Navree project, scaffold a Rust API, a SaaS or a microservice with Navree, run a fresh app or kit, or asks what `navree new` generated."
---

# Create and run a Navree app

## Prerequisites

- Rust 1.95 or newer (edition 2024), from rustup.
- A C compiler for the Rust toolchain: the `navree` tool and any build with
  the `sqlite` feature (and an app's tests) compile SQLite from source. On
  Windows that means the MSVC build tools.
- Docker or Podman: `navree dock up` runs the app's database, TimescaleDB
  (not needed with `--database sqlite`).
- Optional: `rustfmt` (generators format their output with it), Node.js
  20.9+ for the `--ui next` front end.
- No OpenSSL, CMake or installed database server: TLS is rustls, and Dock
  runs the database in a container.

## 1. Install the CLI

```sh
cargo install --git https://github.com/NavreeHQ/navree navree-cli
# or, from a clone (pins the Navree version your apps build against):
git clone https://github.com/NavreeHQ/navree
cargo install --path navree/crates/navree-cli
navree --version
```

## 2. Create the app

```sh
navree new blog                                  # API only, on TimescaleDB
navree new shop --ui next                        # plus a Next.js front end in web/
navree new blog --database sqlite                # a SQLite file, nothing to install
navree new blog --navree-path ~/code/navree      # depend on that checkout
```

- The name uses lower-case letters and digits separated by single `-` or
  `_`, starts with a letter, and is not a Rust keyword, `std`, `test` or
  `navree`. The directory must not exist or must be empty.
- The app depends on Navree by `path`: the `--navree-path` checkout, else
  the checkout the CLI was built from. Only when neither exists does it
  write `navree = "1.0.0"` (the CLI's own version). For an app you keep,
  install the CLI from a clone or pass `--navree-path`; Cargo's git
  checkouts are a cache.
- `navree new` doesn't run Cargo. It runs `git init` (no commit) unless the
  directory is inside a repository already or git is missing. The app is
  its own Cargo workspace (remove `[workspace]` from its `Cargo.toml` to
  join an enclosing one).
- `--database` picks the database: `timescaledb` (the default: PostgreSQL
  with hypertables), `postgres`, `mysql` (also MariaDB) or `sqlite`. It sets
  `database.url`, the `navree` dependency's driver feature (`postgres`,
  `mysql` or `sqlite`; the app compiles only that driver, and `[dev-dependencies]`
  turns `sqlite` on because the tests run on SQLite files) and, for TimescaleDB and PostgreSQL, a
  `dock.toml` whose `postgres` service serves that very URL
  (`postgres://navree:navree@127.0.0.1:5432/<crate_name>`).

### Or start from a starter kit

When the user wants a product rather than a blank app, use a kit. Pick by
shape:

| Want | Command | Writes |
|---|---|---|
| Multi-tenant SaaS with a web app | `navree new acme --kit saas` | The API (accounts, two-factor, workspaces, members and invitations, billing, social sign-in, Helm at `/admin`, an example `projects` feature, a demo seeder) plus `web/`: Next.js, shadcn/ui and a backend-for-frontend holding the tokens in httpOnly cookies |
| The same API for mobile or other clients | `navree new acme --kit api` | The SaaS kit's API without `web/`, and a committed `openapi.json` |
| A service on a broker | `navree new billing --kit micro` | A Cargo workspace: `crates/billing` (an `invoices.get` message handler, a `deal.won` event handler, health and metrics over HTTP) and `crates/billing-contracts` (the payloads and events clients share), NATS configured |

```sh
navree new acme --kit saas
cd acme
navree dock up                 # TimescaleDB and Mailpit (micro: TimescaleDB and NATS)
navree key:generate            # then set APP_KEY in .env
navree migrate:fresh --seed    # saas/api: tables and the `demo` workspace (micro: navree migrate)
pnpm --dir web install         # saas only: the web app's packages
navree dev                     # API on :8080, web app on :3000
```

- Kits need Docker or Podman for Dock, and the SaaS kit needs Node.js 22+
  and pnpm. `.env` already holds Dock's settings (`APP_DATABASE__URL`, ...)
  between `# dock:start` and `# dock:end`; `web/.env.local` is a copy of
  `web/.env.example`.
- Demo accounts (saas/api): `owner@<app>.test` (also a super admin for
  `/admin`) and `member@<app>.test`, password `password123`, workspace
  `demo`.
- `--ui next` only goes with `--kit saas` (where it changes nothing);
  with `api` or `micro` it is an error.
- The kits run on TimescaleDB (the default) or, with `--database postgres`,
  plain PostgreSQL (`timescaledb = false` in `dock.toml`); `mysql` and
  `sqlite` are for the minimal app.
- `navree new` runs `git init` (no commit) when git is installed and the
  directory is not inside a repository, and writes a git-ignored
  `harbor.key` for a kit. The saas kit's `web/app/globals.css` lists the
  Tailwind sources (`source(none)` plus `@source` per folder): new web
  folders that hold class names need their own `@source` line.
- The kit is recorded in `Cargo.toml`: `[package.metadata.navree] kit`
  and `kit_version` (`[workspace.metadata.navree]` for `micro`). Compare
  with a fresh `navree new acme-latest --kit saas` to adopt template fixes.
- Read the generated `AGENTS.md` before changing a kit app: it lists the
  layout, the commands and the conventions (workspace-scoped features copy
  `src/modules/projects/`, whose `entity.rs` is a navree-orm model declared
  `#[model(workspace, subject)]` with a schema-builder migration, read and
  written through `WorkspaceRepo`; micro handlers copy
  `crates/<service>/src/modules/invoices/handlers.rs`).

## 3. Start the database, set the application key and run

```sh
cd blog
navree dock up               # TimescaleDB on 127.0.0.1:5432 (Docker or Podman)
navree key:generate          # prints base64:... ; boots the app, so the first run compiles everything
```

Every app command boots the app, which connects to the database: start it
first.

Paste the key into `.env` as `APP_KEY=base64:...`. Without it, tokens and
signed URLs use a random key per process (every restart signs everyone out)
and encryption fails.

```sh
navree dev                   # build, serve on http://127.0.0.1:8080, rebuild on save
curl http://127.0.0.1:8080/health/live        # {"status":"up"}
```

Open http://127.0.0.1:8080/api/docs for the generated API reference.
`navree dev` watches `src/`, `config/`, `migrations/`, `crates/`,
`Cargo.toml`, `.env` and `build.rs`; a failed build keeps the previous server running.
Options: `--no-web`, `--debounce-ms 300`, and serve arguments after `--`
(`navree dev -- --addr 0.0.0.0:8080`).

## What `navree new` writes

```text
AGENTS.md / CLAUDE.md   conventions for coding agents (CLAUDE.md imports AGENTS.md)
.env, .env.example      APP_KEY, http addr, database url, log level, mail settings
config/app.toml         http, database (migrate_on_boot = true), schema (timescaledb = "auto"), log, mail, queue, auth, api
dock.toml               [services.postgres]: TimescaleDB for `navree dock up`
src/main.rs             <crate>::app(ConfigSources::defaults()).run()
src/lib.rs              pub mod abilities; pub mod modules; pub use app::app;
src/app.rs              every platform module configured once (Lighthouse and Logbook included); `// navree:modules`
src/abilities.rs        ability rules; `// navree:policies`
src/modules/users/      a profile at GET/PUT /api/v1/profile on top of AuthModule
tests/app.rs            boots the whole app on a temporary SQLite database
storage/                runtime data (logs, uploads; the database with --database sqlite)
web/                    with --ui next: Next.js with a backend-for-frontend
```

Routes that exist at once: `POST /api/auth/register|login|refresh|logout`,
`GET /api/auth/me`, the account routes, `GET /api/me/abilities`,
`/api/openapi.json`, `/api/docs`, `/health/live`, `/health/ready`,
`/metrics`, and the profile routes. Registration is on
(`auth.registration = true` in `config/app.toml`).

## 4. Add the first feature

```sh
navree make:module posts --crud      # model, migration, DTOs, CRUD controller, service, tests
cargo test                           # the generated tests pass as they are
navree routes                        # shows /api/v1/posts with its access rules
```

The new module is added to `src/app.rs` and declared in
`src/modules/mod.rs`. Its routes check abilities on `Post`: grant
`posts.read`/`posts.create`/... to a role, or add rules in
`src/abilities.rs` (see the `navree-access-control` skill). Pending
migrations run at boot in development (`database.migrate_on_boot = true`);
run `navree migrate` explicitly elsewhere.

## Add Navree to an existing package

```sh
cargo add --git https://github.com/NavreeHQ/navree navree
```

Also add `serde` (derive), `chrono` and `uuid` (`serde`, `v7`): serde's
derives name their own crate. (The ORM's derives, `#[derive(Validate)]`,
migrations and database drivers come with `navree`.) A minimal app:

```rust
use navree::prelude::*;

#[injectable]
pub struct GreetingController;

#[controller("/greeting", version = "1")]
impl GreetingController {
    #[get("/{name}")]
    #[public]
    async fn show(&self, Path(name): Path<String>) -> Json<String> {
        Json(format!("Hello, {name}!"))
    }
}

#[module(controllers = [GreetingController])]
pub struct GreetingModule;

fn main() -> Result<(), BoxError> {
    Navree::app()
        .module(ConfigModule::for_root(ConfigSources::defaults()))
        .module(HttpModule::from_config()) // GET /api/v1/greeting/{name}
        .module(GreetingModule)
        .run()
}
```

## Checks

```sh
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
navree check                         # the module graph, without booting
```

For a kit, also:

```sh
navree openapi:export openapi.json   # saas/api: must leave the committed file unchanged (CI checks it)
pnpm --dir web lint && pnpm --dir web typecheck && pnpm --dir web test && pnpm --dir web build   # saas
cargo test --workspace && navree micro:patterns                   # micro, at the workspace root
```

## Common mistakes

- `navree help` prints the CLI's help; `navree list` prints the app's commands.
- Setting `APP_DATABASE__URL` or `APP_HTTP__ADDR` in `.env` has no effect:
  `config/app.toml` sets those keys and `.env` only fills missing ones. Edit
  the TOML file or use a real environment variable.
- Every app command boots the app, so `key:generate`, `migrate` and
  `routes` need a working configuration and a running database: run
  `navree dock up` first ("cannot connect to postgres://navree:***@127.0.0.1:5432/..."
  means it isn't running).
- Tests don't need Dock: `TestApp` gives each test its own SQLite file, and
  the same migrations run there (hypertables become regular tables).
- Leaving `migrate_on_boot = true` in production: turn it off with
  `APP_DATABASE__MIGRATE_ON_BOOT=false` and run `migrate` as a deploy step.
- Kits keep the database URL out of `config/app.toml`, so `.env` (Dock's
  settings) fills it: PostgreSQL must be up (`navree dock up`) before
  `navree migrate`, `key:generate` or `dev`. Their tests need no services:
  each boots on its own SQLite file (and `MicroFake` in `micro`).
- In the `micro` workspace, run `navree dev`, `check`, `migrate` and
  `micro:*` at the root (its `default-members` is the service), but the
  generators (`make:module`, `make:migration`, ...) in `crates/<service>`.
  Use `cargo fmt`, not `cargo fmt --all`: `--all` also reformats path
  dependencies such as a local Navree checkout.
- After changing a DTO in the SaaS kit, run `pnpm --dir web gen:api`: it
  exports `openapi.json` and regenerates `web/lib/api/schema.d.ts`. Commit
  both, or CI's OpenAPI drift check and the web type check fail.
- `billing.provider = "fake"` is for development only; production sets
  `APP_BILLING__PROVIDER=stripe` (see `harbor.toml`) with the keys in the
  environment.

## Docs

- https://navree.com/docs/getting-started/installation/
- https://navree.com/docs/getting-started/starter-kits/
- https://navree.com/docs/getting-started/your-first-app/
- https://navree.com/docs/getting-started/directory-structure/
- https://navree.com/docs/getting-started/configuration/
