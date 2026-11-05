---
name: navree-dock
description: "Runs a Navree app's local services with Dock: `dock.toml`, `navree dock:add`, `navree dock up/down/status/logs/env/shell`, `navree dock:remove`, the settings Dock gives the app (`APP_DATABASE__URL`, `APP_MAIL__TRANSPORT`, `REDIS_URL`, ...), the `.env` block, Docker or Podman (`NAVREE_DOCK_RUNTIME`) and CI. Use when the app needs its database (TimescaleDB, Navree's default, or plain PostgreSQL), Redis, Meilisearch, Mailpit, MinIO or NATS on a developer machine or in CI, when `navree dock` fails (port taken, not ready, Docker not answering), or when tests must run against TimescaleDB."
---

# Dock: local services for a Navree app

Dock (crate `navree-dock`, behind the `navree` CLI) starts TimescaleDB (or
plain PostgreSQL), Redis, Meilisearch, Mailpit, MinIO and NATS in containers
from `dock.toml`, waits until each is ready, and gives the app their
settings. It generates a Compose file (`.navree/dock/compose.yaml`, never
edit it) and runs it with `docker compose` or `podman compose`. Nothing in
the app's code changes.

A new app runs on TimescaleDB: `navree new` writes a `dock.toml` with
`[services.postgres]` and the matching `database.url`, so `navree dock up`
is the first command in a new app (apps made with `--database sqlite` need
no services, and `--database mysql` writes no `dock.toml`: Dock does not run
MySQL). Add more services when the app uses search, a broker, object
storage or a real mail inbox.

## Steps

1. Add the services (creates `dock.toml` next to `Cargo.toml`; keeps comments):

   ```sh
   navree dock:add postgres      # prints "Your app needs navree's `postgres` feature." when it is off
   navree dock:add mailpit
   ```

2. For the database, the facade's driver must be on in `Cargo.toml`:
   `navree = { ..., features = ["postgres"] }` (a new app has it).
3. Start them and run the app:

   ```sh
   navree dock up                # pulls, starts, waits, writes the .env block
   navree migrate
   navree dev                    # gets the settings as environment variables
   ```

4. Commit `dock.toml`. Never commit `.env`; `.navree/` ignores itself.

## dock.toml template

```toml
# Local services for crm-api. `navree dock up` starts them.
# project = "crm-api"   # container/volume prefix (default: package name)
# runtime = "auto"      # docker | podman | auto; NAVREE_DOCK_RUNTIME overrides
# write_env = true      # write the settings into .env

[services.postgres]           # TimescaleDB 2.31.0 on PostgreSQL 17 (timescale/timescaledb:2.31.0-pg17)
version = "17"               # the PostgreSQL major under TimescaleDB: 16, 17 or 18
database = "crm_api"         # default: package name with `_`
# timescaledb = false        # plain PostgreSQL: postgres:<version>, any tag
# image = "postgis/postgis:17-3.5"  # TimescaleDB is off for images that aren't TimescaleDB's
# user = "navree"
# password = "navree"
# extensions = ["vector"]    # created after timescaledb; with timescaledb = false, switches to pgvector/pgvector:pg17
# port = 5432

[services.redis]             # maxmemory_mb = 256

[services.mailpit]           # port (SMTP) = 1025, ui_port = 8025

[services.meilisearch]       # master_key = "dock-meilisearch-master-key"

[services.minio]
bucket = "crm-uploads"       # created once ready; access_key, secret_key, console_port

[services.nats]
jetstream = true             # monitor_port = 8222
```

Every service takes `image`, `version`, `port` and `env`. Unknown keys and
services are errors naming the table and the allowed keys.

## Settings the app gets

| Service | Variables (defaults) |
|---|---|
| postgres | `APP_DATABASE__URL=postgres://navree:navree@127.0.0.1:5432/<database>` |
| redis | `REDIS_URL=redis://127.0.0.1:6379` |
| meilisearch | `APP_SEARCH__ENGINE=meilisearch`, `APP_SEARCH__URL=http://127.0.0.1:7700`, `APP_SEARCH__KEY` |
| mailpit | `APP_MAIL__TRANSPORT=smtp://127.0.0.1:1025` (inbox on :8025) |
| minio | `AWS_ENDPOINT_URL=http://127.0.0.1:9000`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION=us-east-1` |
| nats | `APP_MICRO__URL=nats://127.0.0.1:4222` (`micro.transport = "nats"` stays in `config/app.toml`) |

A service's `env` table **replaces** its defaults; values take `{host}`,
`{port}` and the service's own placeholders (postgres `{user}`
`{password}` `{database}`; meilisearch `{key}`; mailpit `{ui_port}`; minio
`{bucket}` `{user}` `{password}` `{console_port}`; nats `{monitor_port}`):

```toml
[services.postgres]
env = { APP_DATABASE__URL = "postgres://{user}:{password}@{host}:{port}/{database}?application_name=crm-dev" }
```

They reach the app three ways:

- `navree dev` and every forwarded command (`navree migrate`,
  `navree queue:work`, ...) set them as environment variables (a variable
  already set in the shell wins). Env vars beat `config/app.toml`.
- `navree dock up` writes them into `.env` between `# dock:start ...` and
  `# dock:end` (replaced each run; `write_env = false` turns it off). `.env`
  overrides `config/app.toml`, so the block applies even when the TOML file
  sets the same key; real environment variables override both.
- `navree dock env` prints `NAME=value` lines (`--export` for a shell).

## Commands

```sh
navree dock up [service...]               # start, wait for readiness (60 s), write .env
navree dock down [service...] [--volumes] # stop and remove; --volumes deletes data
navree dock status                        # state, image, addresses
navree dock logs <service> [--follow]
navree dock env [--export]                # needs no Docker
navree dock shell <service>               # psql, redis-cli, nats-box, else sh
navree dock:add <service>
navree dock:remove <service> [--volumes]
```

## CI (GitHub Actions)

```yaml
      - run: cargo install --git https://github.com/NavreeHQ/navree navree-cli --locked
      - run: navree dock up postgres nats
      - run: navree dock env >> "$GITHUB_ENV"
      - run: cargo test --locked
```

## TimescaleDB

`[services.postgres]` starts the server with `shared_preload_libraries =
timescaledb` and telemetry off (also on a volume a plain PostgreSQL image
created), then runs `CREATE EXTENSION IF NOT EXISTS "timescaledb"` in the
database before the `extensions` list. Migrations decide which tables
become hypertables (see the `navree-database` skill). Going back to plain
PostgreSQL, or to another major version, needs an empty volume:
`navree dock down postgres --volumes`.

## Tests on TimescaleDB

`TestApp` reads neither `.env` nor the environment, so tests run on SQLite
(a new app turns the `sqlite` feature on in its `[dev-dependencies]` for
this; hypertables become regular tables). For a test that needs TimescaleDB
(`first`/`last`, month buckets, continuous aggregates), pass the URL
yourself and skip when there is none:

```rust,ignore
let Ok(url) = std::env::var("APP_DATABASE__URL") else {
    eprintln!("skipped: no APP_DATABASE__URL");
    return;
};
let app = TestApp::builder()
    .config("database.url", url)
    .config("database.migrate_on_boot", true)
    .module(DatabaseModule::from_config())
    .build()
    .await;
```

Locally, `cargo test` does not go through the CLI: run
`eval "$(navree dock env --export)"` first.

## Using the library

`navree::dock` (facade feature `dock`, opt-in, not in `full`) or the
`navree-dock` crate: `DockFile::load(root)` / `DockFile::parse(text,
package)`, `file.env()`, `app_env(root)`, `Dock::open(root,
ProcessExecutor::new(root))` with `up`, `down`, `status`, `logs`, `shell`.
Tests drive `Dock` with `FakeExecutor` (records commands, scripted answers,
virtual clock) instead of Docker:

```rust,ignore
use navree_dock::{CmdOutput, Dock, DockFile, FakeExecutor};

let file = DockFile::parse("[services.postgres]\n", "crm-api")?;
let exec = FakeExecutor::new().respond_once("pg_isready", CmdOutput::failed(2, "no response"));
let dock = Dock::new(dir.path(), file, exec)?;
dock.up(&[], &mut Vec::new())?;
assert!(dock.executor().commands().iter().any(|c| c.contains("compose")));
```

## Checks

- `navree dock env` prints the settings you expect (no Docker needed).
- `navree dock up` ends with `Wrote N settings to .env` or `.env already has
  the N settings.`; `navree dock status` shows `running`.
- `navree migrate` succeeds against TimescaleDB (the app has the `postgres`
  feature) without "does not support TimescaleDB" notices.

## Gotchas

- **Port taken**: `up` stops before starting anything and names the port,
  the key to set and the container holding it. Another app's services are
  usually the cause: give this app other ports (`port = 5433`); settings
  follow.
- **Docker not answering** (Docker Desktop stopped, `podman machine` not
  started): Dock gives up after 30 s with that message instead of hanging.
- **`podman compose`** needs a provider: install `podman-compose` or
  `docker-compose`. Docker needs Compose v2 (`docker compose`).
- **Not ready within 60 s**: the error carries the last 20 log lines; a
  common cause is an old volume from another PostgreSQL major version:
  `navree dock down postgres --volumes`, then `up` again.
- **`version` with TimescaleDB** is the PostgreSQL major (`"16"`, `"17"`,
  `"18"`); anything else is an error that suggests `timescaledb = false`.
- **PostgreSQL 18+** keeps data in `/var/lib/postgresql`; Dock mounts the
  right path from the version, but a volume made by 17 won't start 18.
- **Credentials** may use letters, digits, `.`, `_`, `~`, `-` only; MinIO's
  `secret_key` needs 8+ characters; buckets are lower-case S3 names.
- **MinIO image**: MinIO stopped publishing images; Dock runs
  `pgsty/minio` (a maintained MinIO build, same API, console and `mcli`).
- **`.env` keys below the dock block win** over it; `up` prints a note.
- Never use Dock on a server: ports bind to 127.0.0.1 with development
  passwords. Production services come from Harbor.
