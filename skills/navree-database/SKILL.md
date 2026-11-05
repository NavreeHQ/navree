---
name: navree-database
description: "Works with the database layer of a Navree app: the `database` configuration and its databases (TimescaleDB by default, PostgreSQL, MySQL/MariaDB, SQLite), the per-request unit of work (`Uow`), transactions in jobs and commands, models with UUID v7 keys whose writes record change events, seeders and `db:seed`, pagination, raw statements on an `Executor`, and TimescaleDB hypertables. Models and queries are the `navree-orm` skill, migrations the `navree-migrations` skill. Use when configuring the database, writing data outside a request, running raw SQL, writing a seeder, or debugging \"database is locked\", 409 conflicts or writes that are not saved."
---

# The database

Apps work with their data through the ORM (`#[derive(Model)]` models: load
the `navree-orm` skill) and write migrations with the schema builder (the
`navree-migrations` skill). Both run on navree-db, which this skill covers.
TimescaleDB (PostgreSQL with hypertables for
time series) is the default database: `navree new` writes its URL and turns
on the `postgres` feature of `navree`, and `navree dock up` runs it. Plain
PostgreSQL uses the same feature; MySQL and MariaDB the `mysql` feature
(`mysql://` or `mariadb://` URLs; MySQL 8's default accounts sign in over
TLS); SQLite is the opt-in `sqlite` feature (the bundled library: tests,
`navree new --database sqlite`, and the fallback when `database.url` is
unset; without the feature that fallback stops the boot with "enable the
`sqlite` feature of navree-db", and a PostgreSQL or MySQL build compiles no
SQLite). An app on PostgreSQL or MySQL turns `sqlite` on in
`[dev-dependencies]` only, for its tests. navree-db provides: a
connection pool from the `database` section (on SQLx), a **unit of
work** per request (one lazy transaction, committed when the response status
is below 400), the migrator, seeders and pagination, and the **change
events** models record for every create, save and delete (the audit log).
Everything that touches the database takes an `Executor`: `&Uow` in a
request, `&Db` (which joins the current unit of work) elsewhere.

## Steps: a new table

1. `navree make:model posts Tag --migration` writes
   `src/modules/posts/models/tag.rs` (a `#[derive(Model)]` struct) and the
   `create_tags_table` migration, and adds the migration to the module's
   `MigrationSet`. (A new module from `navree make:module posts --crud` has
   both already.)
2. Add the columns to the model **and** to the migration.
3. Write a service that takes `&Uow` and creates, saves and deletes records
   through it.
4. Run it: migrations apply at boot in development
   (`database.migrate_on_boot = true`), or run `navree migrate`.
5. Test with `TestApp::builder().temp_database()` (a fresh SQLite file per
   test, migrated at boot).

## Model

```rust
use chrono::{DateTime, Utc};
use navree::orm::Model;
use serde::Serialize;
use uuid::Uuid;

#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)] // Serialize: change events and JSON
#[model(table = "posts", timestamps, subject)]
pub struct Post {
    pub id: Uuid,                    // a UUID v7 is generated when the record is created
    pub owner_id: Option<Uuid>,
    pub name: String,
    pub published: bool,
    pub created_at: DateTime<Utc>,   // filled in and touched by `timestamps`
    pub updated_at: DateTime<Utc>,
}
```

Relations, scopes, casts and soft deletes are declared on the model (the
`navree-orm` skill). Keep secrets out of a model that handlers serialize
(`#[serde(skip)]`, or `#[model(hidden)]`).

## Service: reads and writes in the unit of work

```rust,ignore
use navree::access::{Accessible, Read};
use navree::db::{Page, PageQuery, Uow};
use navree::injectable;
use navree::orm::access::AccessibleQuery as _;
use navree::orm::{OrmError, Record};

#[injectable]
pub struct PostService; // a shared singleton: request state comes in as parameters

impl PostService {
    pub async fn list(&self, uow: &Uow, scope: Accessible<Post, Read>, page: PageQuery) -> Result<Page<Record<Post>>, OrmError> {
        scope
            .query()
            .order_by_desc(Post::CREATED_AT)
            .order_by_desc(Post::ID) // always end with a unique column
            .paginate(uow, page)
            .await
    }

    pub async fn create(&self, uow: &Uow, input: CreatePost) -> Result<Record<Post>, OrmError> {
        Post::create(uow, Post {
            owner_id: uow.actor(),               // the signed-in user, if any
            name: input.name,
            ..Post::default()                    // id and timestamps are filled in
        })
        .await
    }

    pub async fn update(&self, uow: &Uow, mut post: Record<Post>, input: UpdatePost) -> Result<Record<Post>, OrmError> {
        if let Some(name) = input.name {
            post.name = name;
        }
        post.save(uow).await?;                   // writes the changed columns, records them
        Ok(post)
    }

    pub async fn delete(&self, uow: &Uow, mut post: Record<Post>) -> Result<(), OrmError> {
        post.delete(uow).await?;
        Ok(())
    }
}
```

The controller passes the request's unit of work: `async fn store(&self,
uow: Uow, ...)` then `self.posts.create(&uow, input).await?`.

| Call | Does | Change event |
|---|---|---|
| `Post::create(&uow, post)` | inserts, returns the `Record<Post>` | `created`, the whole record |
| `record.save(&uow)`, `record.update(&uow, \|p\| ..)` | writes the dirty columns (a clean record writes nothing) | `updated`, only the changed fields |
| `record.delete(&uow)` | deletes (sets `deleted_at` on a soft-deleting model) | `deleted` |
| `Post::find(&uow, id)`, `Post::find_or_fail(&uow, id)` | `Option<Record<Post>>`, or the record / `ModelNotFound` (404 over HTTP) | none |
| `Post::query().where_key(id).lock_for_update().first(&uow)` | the record, row locked on PostgreSQL and MySQL | none |

Mass writes (`Post::query()....update(..)`, `delete()`, `insert` of many
rows, `upsert`) and raw statements run one statement and record no change
events; call `uow.record_change(ChangeEvent { .. })` when one should be
audited.

## Which executor

| Code | Executor |
|---|---|
| handler, or service taking `&Uow` | `&uow` |
| service without `&Uow` | `&db` (`Arc<Db>` injected): joins the current unit of work if there is one, else the pool (a read replica for reads) |
| command, seeder, test assertions | `&db`, or a unit of work from `db.transaction(..)` / `db.uow(..)` |

Raw SQL takes the same executors, as `navree::db::Statement`s:

```rust,ignore
use navree::db::{Executor as _, ExecutorExt as _, Statement};

// `?` placeholders on every database; values are bound, never formatted in.
let published: i64 = uow
    .fetch_scalar(&Statement::portable("SELECT COUNT(*) FROM posts WHERE published = ?", [true.into()]))
    .await?;
uow.execute(&Statement::portable(
    "UPDATE posts SET published = ? WHERE owner_id = ?",
    [false.into(), owner.into()],
))
.await?;
```

`Statement::new(backend, sql, values)` is for SQL written for one database
and `Statement::text(sql)` for SQL with no values. `fetch_all` and
`fetch_optional` return `Row`s, read with `row.get::<T>("column")`.

Never write through the pool (`db.pool()`, a `Transaction` from
`db.begin()`) during a request: it escapes the rollback and, on SQLite, waits
on the request's own write lock ("database is locked" after 5 s).

## Outside a request

```rust,ignore
// A command or job: open a unit of work, run the work in its scope, then
// commit or roll back. The actor is who the audit log records.
let db = ctx.get::<Db>().await?;               // in a job: ctx.app.get::<Db>()
let uow = db.uow(Some(user_id), None).await;   // None: no actor
let result = uow
    .scope(async {
        let mut post = Post::find_or_fail(&uow, id).await?;
        post.delete(&uow).await?;
        Ok::<_, OrmError>(())
    })
    .await;
match result {
    Ok(()) => uow.commit().await?,
    Err(error) => {
        uow.rollback().await?;
        return Err(error.into());
    }
}
```

`db.transaction(|uow| async move { .. })` is the shorter form for statement
work: its closure returns `Result<_, DbError>` (an `OrmError` from a model
call does not convert into it, so model work uses the form above).

`uow.after_commit(move || Box::pin(async move { .. }))` runs work only after
a successful commit (broadcasts, calls to other systems). Queued jobs, queued
mail and notifications already wait for the commit.

## Migrations

Load the `navree-migrations` skill to write or run migrations. In short: a
migration implements `navree::schema::Migration` (`async fn up` and `down`
taking `&Schema`), written with the schema builder; each module registers
one `MigrationSet` (`m.migrations(migrations::migrations())`), ordered with
`.after([..])`. `references/migrations.md` has a summary and
`references/timescaledb.md` time-series tables (hypertables). Never edit a
migration that has run anywhere: add a new one.

```sh
navree migrate               # pending migrations, as one batch (--step, --pretend, --seed)
navree migrate:rollback      # the last batch; --step N, --batch N (APP_DATABASE__MIGRATE_ON_BOOT=false first)
navree migrate:fresh --seed  # drops EVERY table; development only
```

## Seeders, factories and pagination

See `references/seeding-and-pagination.md` for a `Seeder` (`db:seed`),
model factories and cursor pagination. In short: `navree make:seeder posts
demo_posts` writes and registers a seeder (`m.seeder::<DemoPostsSeeder>()`);
a root seeder (`is_root() -> true`) runs the others with `ctx.call::<S>()`;
seeders fill tables with model factories
(`Post::factory().count(10).create(db)`, the `navree-orm` skill). Queries
paginate with `query.paginate(&uow, page)`, which returns `Page<Record<M>>`
(`{data, meta}`), and `query.cursor_paginate(&uow, cursor)`, which returns a
`CursorPage` (`{data, next_cursor, prev_cursor}`).

## Configuration

```toml
[database]
url = "postgres://navree:navree@127.0.0.1:5432/my_app"  # TimescaleDB (`postgres`); mysql://... (`mysql`); sqlite://storage/app.db?mode=rwc
max_connections = 10
migrate_on_boot = false                    # a new app sets true for development
slow_query_ms = 200
test_before_acquire = false                # true: ping every checkout (one round trip each)
idle_timeout_secs = 600                    # 0: keep idle connections
max_lifetime_secs = 1800                   # 0: no limit
transaction_settings = true                # false: no navree.actor / @navree_actor per transaction
# read_urls = ["postgres://.../replica"]   # read replicas; `sticky = true` reads from the writer after a write
# direct_url = "postgres://.../direct"     # past a transaction-mode pooler: migrations run on it

[database.mysql]                           # MySQL and MariaDB only
strict = true                              # strict SQL mode plus ONLY_FULL_GROUP_BY (default)
# isolation_level = "read_committed"       # the server's (REPEATABLE READ) when unset
# modes = ["STRICT_ALL_TABLES"]            # the exact SQL mode instead of `strict`

# [database.postgres]                      # search_path, timezone, application_name,
#                                          # synchronous_commit, isolation_level, ssl_mode
# [database.sqlite]                        # journal_mode, busy_timeout_ms, foreign_keys, synchronous, pragmas
# [database.connections.analytics]         # a named connection (`db.connection("analytics")`), same keys
# url = "postgres://.../analytics"
```

`navree db:monitor` shows the connections each database has open and
`navree db [connection]` opens its own client (`psql`, `mysql`, `sqlite3`).

On MySQL the unit of work sets `@navree_actor` and `@navree_request_id`
(cleared before commit or rollback) where PostgreSQL uses `set_config`
(neither when `transaction_settings = false`), the UUIDs models
write are 16 bytes, for `BINARY(16)` columns (the schema builder's
`uuid` columns are `CHAR(36)` there: see `navree-migrations`), and workspace
isolation is query scoping only (no row-level security): every workspace
query must go through `WorkspaceRepo` or a `#[model(workspace)]` model.

Override in production with `APP_DATABASE__URL` (an environment variable;
`.env` also overrides the TOML value). `Arc<Db>` is injectable
anywhere (`DatabaseModule` is global).

## Time series

Navree's own time-series tables are hypertables on TimescaleDB, converted
by their modules' migrations: `audit_logs`, `webhook_deliveries`,
`webhook_attempts`, `fleet_jobs`, `fleet_metrics`, `schedule_runs`,
`lighthouse_samples`, `lighthouse_occurrences`, `lighthouse_aggregates`.
Their primary keys include the time column there, so nothing may reference
them by `id` alone. Prune old rows with
`navree::db::prune_time_series(&executor, table, time_column, cutoff)` with
a `Db`, `Transaction` or `Uow`: whole chunks go on a hypertable, one
`DELETE` elsewhere (it returns the rows removed, or a `DbError`). See
`references/timescaledb.md`.

## Common mistakes

- Forgetting `m.migrations(..)` in the module's `wire`, or the set's
  `.after([..])` for tables it references.
- Not ordering a paginated query, or ordering by a non-unique column only.
- Keeping a row stream (`Executor::fetch`) of a unit of work in a struct or
  spawned task past the request: the stream holds the transaction's
  connection, so every later statement and the commit wait until it ends or
  is dropped. Drop streams before the unit of work ends.
- In-memory SQLite (`sqlite::memory:`): each pool connection sees a
  different database. Tests use temp files.
- Hand-rolling soft deletes: a model does all of it with
  `#[model(soft_deletes)]` (queries leave deleted rows out, `delete` sets
  `deleted_at`, route binding answers 404).
- Expecting navree-validation's plain `exists` rule to skip soft-deleted
  rows (the ORM's `exists` rule does).
- Running app commands (`migrate`, `key:generate`) before `navree dock up`:
  the app can't connect to TimescaleDB.
- A foreign key to a hypertable's `id`: its key is `(id, time)`, so the
  conversion stops. Reference a regular table (the endpoint, the device)
  and delete the dependent rows when pruning.

## Docs

- https://navree.com/docs/database/getting-started/
- https://navree.com/docs/database/entities-and-repositories/ (models and records in the unit of work)
- https://navree.com/docs/database/unit-of-work/
- https://navree.com/docs/database/migrations/
- https://navree.com/docs/database/running-migrations/
- https://navree.com/docs/database/timescaledb/
- https://navree.com/docs/database/seeding-and-factories/
- https://navree.com/docs/database/pagination/
- https://navree.com/docs/orm/getting-started/
