# Migrations in Navree

A summary. The `navree-migrations` skill covers migrations in full: every
column type per database, indexes and foreign keys, the commands, testing
and squashing.

Each module owns its migrations as one `MigrationSet`. The app's migrator
runs every set: sets in dependency order (`.after([..])`, otherwise
alphabetical), migrations inside a set in name order (names start with a
timestamp). Applied migrations are recorded in `navree_migrations`
(`module`, `name`, `batch`, `applied_at`).

Migrations are written with the schema builder (`navree::schema`), which
writes the SQL for PostgreSQL (with TimescaleDB), MySQL, MariaDB and SQLite.
Every migration implements `navree::schema::Migration`.

## Generate

```sh
navree make:migration posts create_posts_table      # schema.create("posts", ..) + drop_if_exists
navree make:migration posts add_published_to_posts  # schema.table("posts", ..) up and down
navree make:migration posts backfill_slugs          # empty up/down
```

The file is `src/modules/posts/migrations/m<YYYYMMDD>_<HHMMSS>_<name>.rs`;
the generator adds `mod ...;` and `.migration(migration!(...))` to the set
in `migrations/mod.rs`, creating that file (and the `m.migrations(..)` line
in the module's `wire`) when it is the module's first migration.

## Create a table

```rust
use navree::schema::prelude::*;

pub struct Migration;

impl navree::schema::Migration for Migration {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("posts", |t| {
                t.uuid_primary();                                       // id UUID primary key
                t.foreign_uuid("owner_id").nullable().constrained_on("users", "id").null_on_delete();
                t.string("title");                                      // VARCHAR(255) NOT NULL
                t.text("body").nullable();
                t.boolean("published").default(false);
                t.string("slug").unique();
                t.timestamp_tz("created_at");
                t.timestamp_tz("updated_at");
                t.index(["owner_id", "created_at"]);
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("posts").await
    }
}
```

Match the model (on MySQL a `Uuid` the model writes needs
`t.binary(..).length(16).fixed()`, not `uuid`, which is `CHAR(36)` there):
`uuid` for `Uuid`, `timestamp_tz` for `DateTime<Utc>`,
`boolean` for `bool`, `big_integer` for `i64`, `string`/`text` for
`String`, `.nullable()` for `Option<T>`. Columns are `NOT NULL` unless
`.nullable()`. Helpers: `timestamps_tz()` (nullable created/updated_at),
`soft_deletes_tz()` (`deleted_at`), `morphs("subject")`.

## Alter a table

```rust,ignore
schema
    .table("posts", |t| {
        t.string("stage").default("draft").index();     // add
        t.text("title").change();                        // change: the definition is complete
        t.rename_column("body", "content");
        t.drop_column("legacy");
        t.drop_index(["owner_id", "created_at"]);
        t.drop_foreign(["owner_id"]);
    })
    .await
```

`down` undoes `up`. On SQLite a change rebuilds the table, keeping rows,
indexes and triggers (migrations run SQLite with foreign keys off and check
them before committing). Read the live schema with `schema.has_table`,
`has_column`, `columns`, `indexes`, `foreign_keys`; raw SQL with
`schema.statement(sql)`.

## Foreign keys

```rust,ignore
t.foreign_uuid("company_id").constrained().cascade_on_delete();     // companies.id
t.foreign("currency").references("code").on("currencies").restrict_on_delete();
```

Order the sets: `MigrationSet::new("deals").after(["companies"])`. A table
referencing `users` runs after `"auth"`. Framework sets: `auth` (users,
refresh and reset tokens), `access` (after `auth`), `audit`, `queue`,
`notify`, `schedule`, `session`. An `after` entry naming a set that isn't in
the app is ignored.

## Transactions

On SQLite and PostgreSQL each migration runs in its own transaction with its
record row, so a failed migration leaves nothing behind. For statements that
can't run in a transaction (`CREATE INDEX CONCURRENTLY` via an index's
`.online()`, refreshing a continuous aggregate), implement
`fn within_transaction(&self) -> bool { false }`.

MySQL and MariaDB commit DDL on their own, so there a migration runs outside
a transaction (a migration returns `true` from `within_transaction_on_mysql`
for a data migration: its rows then roll back with the
record row). A MySQL migration that fails part way keeps what ran before the
failure and isn't recorded: one DDL statement per migration, or an `up` that
can run again (check `has_table` or `has_column` first).

## Commands

| Command | Does |
|---|---|
| `navree migrate [--step] [--pretend] [--seed]` | runs pending migrations as one batch (`--step`: one each); `--pretend` prints the SQL |
| `navree migrate:status [--pending] [--json]` | every migration with its batch and status |
| `navree migrate:rollback [--step N \| --batch N] [--pretend]` | reverts the last batch, the last N migrations, or batch N |
| `navree migrate:reset [--pretend]` | reverts everything |
| `navree migrate:refresh [--step N] [--seed]` | reset (or N), then migrate |
| `navree migrate:fresh [--seed] [--drop-views] [--drop-types]` | drops every table, migrates. Never in production |
| `navree migrate:install` | creates the migrations table |
| `navree schema:dump [--path P]` | writes `database/schema/<driver>-schema.sql`; `migrate` loads it into an empty database |

Rollback, reset, refresh and fresh refuse to run with `env = "production"`
unless `--force`. Every command boots the app; with `migrate_on_boot =
true` it first applies pending migrations, so roll back with
`APP_DATABASE__MIGRATE_ON_BOOT=false navree migrate:rollback`. In
production turn `migrate_on_boot` off and run `./<app> migrate` as a deploy
step.

In tests, `app.run_command("migrate:refresh", &[])` checks the `down`
methods; `navree::db::Migrator::run(app.ctx(), &MigrateOptions { pretend:
true, ..Default::default() })` returns each migration's statements and
notices.
