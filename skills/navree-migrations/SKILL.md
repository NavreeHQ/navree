---
name: navree-migrations
description: "Writes and runs database migrations in a Navree app with the schema builder (`navree::schema`): `navree make:migration`, `schema.create` and `schema.table` blueprints, column types and modifiers per database, indexes and foreign keys, changing, renaming and dropping columns, SQLite table rebuilds, module-owned `MigrationSet`s and their order, transactions and `within_transaction`, `migrate` with `--pretend` and `--step`, `migrate:status`, rollbacks, `migrate:refresh` and `migrate:fresh`, the production guard, `Migrator` in tests, and squashing with `schema:dump`. Use when adding or changing a table, column, index or foreign key, when a migration fails, runs in the wrong order or half-applies on MySQL, or when running, rolling back, previewing or squashing migrations."
---

# Migrations

A migration is a Rust type with `up` (apply a change) and `down` (reverse
it), written with the schema builder, which writes the SQL for PostgreSQL
(with TimescaleDB), MySQL, MariaDB and SQLite. Each module owns its
migrations as one `MigrationSet`; one migrator runs every set of the app and
records what ran in `navree_migrations` (`module`, `name`, `batch`,
`applied_at`). Navree's own modules ship their tables the same way
(`AuthModule` brings `users` in the `auth` set).

## Steps: a new table

1. `navree make:migration deals create_deals_table` writes
   `src/modules/deals/migrations/m<YYYYMMDD>_<HHMMSS>_create_deals_table.rs`
   with `schema.create("deals", ..)` (UUID key, timestamps) and
   `drop_if_exists` in `down`, and adds it to the module's set (creating
   `migrations/mod.rs` and the `m.migrations(..)` line in `wire` the first
   time). `navree make:model deals Deal --migration` does it with a model.
2. Add the columns, indexes and foreign keys.
3. If a foreign key points at another module's table, add that set to
   `.after([..])`.
4. `navree migrate` (or boot with `database.migrate_on_boot = true`), then
   `navree migrate:refresh` to check that `down` undoes `up`.
5. `cargo test`: `TestApp::builder().temp_database()` migrates a fresh
   SQLite file per test.

Names pick the template: `create_<table>_table` (or `create_<table>`),
`<change>_to_<table>` / `_from_` / `_in_` (`schema.table(..)` up and down),
anything else empty.

## Create a table

```rust
//! Creates the `deals` table.

use navree::schema::prelude::*;

pub struct Migration;

impl navree::schema::Migration for Migration {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("deals", |t| {
                t.uuid_primary();
                t.foreign_uuid("company_id").constrained().restrict_on_delete();
                t.foreign_uuid("owner_id").constrained_on("users", "id");
                t.string("title");
                t.big_integer("amount_cents");
                t.string("stage").length(16).default("lead").index();
                t.timestamps_tz();
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("deals").await
    }
}
```

Columns are `NOT NULL` unless `.nullable()`. The blueprint is checked
before any SQL runs (duplicate columns, two primary keys, a modifier the
type doesn't take, a type the database lacks), and each mistake says how to
fix it. `references/columns-and-indexes.md` lists every column type per
database, the modifiers and the index and foreign-key options.

## Change a table

```rust
use navree::schema::prelude::*;

pub struct Migration;

impl navree::schema::Migration for Migration {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .table("deals", |t| {
                t.char("currency", 3).default("USD");   // add
                t.text("title").change();                // change: the whole new definition
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .table("deals", |t| {
                t.drop_column("currency");
                t.string("title").change();
            })
            .await
    }
}
```

- Commands in `table` run in the order written: `rename_column(from, to)`,
  `drop_column(["a", "b"])`, `drop_index(["stage", "created_at"])`,
  `drop_unique("name")`, `drop_foreign(["company_id"])`,
  `drop_constrained_foreign_id("owner_id")`, `rename_index(from, to)`,
  `drop_check("deals_amount_check")`.
- Check constraints: `t.check("deals_amount_check", "amount >= 0")` (raw
  SQL, may name several columns; name it `<table>_..`, MySQL wants names
  unique per database). `ALTER TABLE .. ADD CONSTRAINT` on PostgreSQL and
  MySQL (enforced from 8.0.16; older ones get a notice), inside `CREATE
  TABLE` on SQLite, where adding or dropping one later rebuilds the table.
- `change()` replaces the column's definition: a default or `nullable` left
  out is removed.
- SQLite can't alter most things in place: the builder **rebuilds** the
  table (copying rows, indexes, triggers). Adding a `NOT NULL` column
  without a default to a table with rows fails: give it a default, or add
  it nullable, fill it, then `change()` it.
- Data: `schema.statement("UPDATE deals SET stage = 'won' WHERE closed_at IS NOT NULL")`.
  Keep data changes in SQL, not models: the migration must keep working
  after the models change.
- Read first when a migration must be safe to run twice:
  `schema.has_table`, `has_column`, `when_table_doesnt_have_column(table,
  column, |t| ..)`, `columns`, `indexes`, `foreign_keys`.

## The module's set

```rust
// src/modules/deals/migrations/mod.rs
use navree::db::MigrationSet;
use navree::schema::migration;

mod m20261026_093000_create_deals_table;
mod m20261027_120000_add_currency_to_deals;

/// Recorded under `deals` in `navree_migrations`: keep the name stable.
pub fn migrations() -> MigrationSet {
    MigrationSet::new("deals")
        .after(["companies", "auth"])
        .migration(migration!(m20261026_093000_create_deals_table))
        .migration(migration!(m20261027_120000_add_currency_to_deals))
}

// src/modules/deals/mod.rs, in the module's wire:
// m.migrations(migrations::migrations());
```

- Sets run in dependency order (`after`), then alphabetically; migrations
  inside a set in name order. An `after` entry naming a set the app doesn't
  have is ignored; cycles and duplicate names stop the run before anything
  runs.
- Framework sets: `auth` (users and tokens), `access` (after `auth`),
  `audit`, `queue`, `notify`, `schedule`, `session`.

## Transactions

SQLite and PostgreSQL run each migration in a transaction with its record
row: a failure leaves nothing behind. MySQL and MariaDB commit each DDL
statement: a migration that fails part way keeps what ran, so keep one
change per migration or make `up` re-runnable. Statements that can't run in
a transaction (an index's `.online()`, `CREATE INDEX CONCURRENTLY`;
refreshing a continuous aggregate) need:

```rust
impl navree::schema::Migration for Migration {
    fn within_transaction(&self) -> bool {
        false
    }
    // up and down as usual
}
```

## Running

| Command | Does |
|---|---|
| `navree migrate` | pending migrations as one batch (`--step`: one batch each; `--seed`; `--schema-path`) |
| `navree migrate --pretend` | prints each pending migration's SQL for this database, runs nothing |
| `navree migrate:status` | every migration, batch and status (`--pending`, `--json`) |
| `navree migrate:rollback` | the last batch (`--step N`, `--batch N`, `--pretend`) |
| `navree migrate:reset` | reverts everything |
| `navree migrate:refresh` | reverts everything (or `--step N`) and migrates (`--seed`) |
| `navree migrate:fresh` | **drops every table**, migrates (`--seed`, `--drop-views`, `--drop-types`) |
| `navree migrate:install` | creates `navree_migrations` |
| `navree schema:dump` | squashes: writes `database/schema/<driver>-schema.sql` (`--path`) |

- Rollback, reset, refresh and fresh refuse to run when `env` is
  `production`, unless `--force`. `migrate` runs (it is the release step).
- Every command boots the app: with `migrate_on_boot = true` a rollback
  first applies what's pending, so run
  `APP_DATABASE__MIGRATE_ON_BOOT=false navree migrate:rollback`. Turn
  `migrate_on_boot` off in production and run `./<app> migrate` before the
  new version takes traffic.
- `schema:dump` reads the database's catalogs (no `pg_dump` needed); an
  empty database loads the dump, then runs only newer migrations. Commit
  the dump, keep the migration files, dump each database you run.

## In tests

```rust
use navree::db::{MigrateOptions, Migrator};

let report = Migrator::run(app.ctx(), &MigrateOptions { pretend: true, ..Default::default() }).await?;
assert!(report.migrations.iter().all(|m| !m.statements.is_empty()));

app.run_command("migrate:rollback", &["--step", "1"]).await?;
let status = Migrator::status(app.ctx()).await?;
assert!(!status.last().unwrap().applied);
```

`Schema::offline(Target::postgres(17))` records a migration's SQL for any
database without connecting (`schema.statements()`, `schema.notices()`);
`Target::mysql((8, 4))`, `Target::mariadb((11, 4))`, `Target::sqlite((3, 46))`.

## Common mistakes

- Editing a migration that has run anywhere but your machine: add a new
  one.
- A foreign key to another module's table without `.after([..])`: the
  table doesn't exist yet.
- `change()` without repeating `nullable()` or the default: they are
  removed.
- A model's `Uuid` columns on MySQL: `uuid_primary()` and `foreign_uuid` are
  `CHAR(36)` there, which refuses the 16 bytes a model writes (error 1366).
  Branch on `schema.target().await?.dialect() == Dialect::MySql` and use
  `t.binary("id").length(16).fixed().primary()` there.
- Several DDL changes in one MySQL migration: a failure half-applies it.
- A foreign key to a TimescaleDB hypertable's `id`: its key includes the
  time column.
- `migrate:fresh` against a database with tables you care about: it drops
  every table, including ones no migration created.

## Docs

- https://navree.com/docs/database/migrations/
- https://navree.com/docs/database/running-migrations/
- https://navree.com/docs/database/schema-builder/
- https://navree.com/docs/database/columns/
- https://navree.com/docs/database/indexes-and-foreign-keys/
- https://navree.com/docs/database/squashing-migrations/
- https://navree.com/docs/database/timescaledb/
