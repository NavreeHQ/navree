# navree-schema: design

The schema builder and the migration language of Navree: tables, columns,
indexes and foreign keys described in Rust, compiled to the right DDL for
PostgreSQL (with TimescaleDB), MySQL/MariaDB and SQLite, and run through the
migrator that `navree-db` owns.

```rust
use navree_schema::{Migration, Schema, SchemaResult};

pub struct CreateDeals;

impl Migration for CreateDeals {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        schema
            .create("deals", |t| {
                t.uuid_primary();
                t.foreign_uuid("company_id").constrained().cascade_on_delete();
                t.string("title");
                t.decimal("amount", 12, 2);
                t.timestamps_tz();
                t.soft_deletes_tz();
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("deals").await
    }
}
```

## 1. Place in the framework

`navree-schema` is a library at **layer 2**: it depends on `navree-config`
(its configuration section) and on SeaORM's connection traits, nothing else
from Navree. `navree-db` (L3) depends on it:

```
L4+ framework crates (audit, queue, notify, ...) ──► navree-schema, navree-db
L3  navree-db ── runs migrations, owns the commands ──► navree-schema
L2  navree-schema ── builds and compiles schema changes
```

Why below `navree-db` and not above it:

- **The runner stays in `navree-db`.** Connections, the pool, the unit of
  work, `MigrationSet`, the `navree_migrations` table and the `migrate*`
  commands are `navree-db`'s. The runner needs to hand schema migrations
  their context (options, notices) and to register `schema:dump`, so it must
  see this crate.
- **Framework crates use it for their own tables.** The audit log, job
  history, notification and webhook delivery logs are time-series tables
  that become hypertables on TimescaleDB. Those crates live at L4 and up, so
  the schema builder has to sit below them.
- **Nothing here needs the pool.** The builder talks to any SeaORM
  connection, transaction or migration manager through
  [`SchemaConnection`], so it works inside the unit of work, inside the
  migrator, and offline (no database at all).

## 2. Module layout

| Module | Contents |
|---|---|
| `schema` | [`Schema`]: the async entry point (`create`, `table`, `rename`, `drop`, `drop_if_exists`, introspection, TimescaleDB helpers, raw statements), statement log and notices |
| `blueprint` | [`Blueprint`]: the table being described: every column type, the helpers (`id`, `timestamps`, `morphs`, ...), indexes, foreign keys, drops and renames, table options |
| `column` | [`ColumnDefinition`] (the data) and [`ColumnBuilder`] (the fluent handle every column method returns: modifiers, inline indexes, `constrained`) |
| `index` | [`IndexDefinition`], [`ForeignKeyDefinition`] and their builders, [`ReferentialAction`] |
| `value` | [`DefaultValue`] and [`Expression`] (raw SQL) |
| `interval` | [`Interval`]: `"7 days"` parsed once, rendered per database |
| `target` | [`Dialect`], [`Target`] (dialect, server version, TimescaleDB version), [`Version`] |
| `grammar` | One compiler per dialect (`postgres`, `mysql`, `sqlite`) plus `timescale`; pure functions from a blueprint and a target to SQL |
| `timescale` | [`Hypertable`], [`Compression`], [`ContinuousAggregate`], [`RefreshPolicy`] |
| `inspect` | Catalog queries per dialect and their results: [`TableInfo`], [`ViewInfo`], [`ColumnInfo`], [`IndexInfo`], [`ForeignKeyInfo`] |
| `rebuild` | SQLite table rebuilds: the current table state and the twelve-step copy |
| `migration` | [`Migration`] (the trait migration files implement), [`SchemaMigration`] (the adapter the migrator runs), [`MigrationContext`], the `migration!` and `migrations!` macros |
| `options` | [`SchemaOptions`]: configuration section `schema` |
| `dump` | `schema:dump`: the schema of a database as SQL, per dialect, with the migration rows |
| `error` | [`SchemaError`] |

## 3. From a blueprint to SQL

1. `schema.create("deals", |t| ...)` builds a [`Blueprint`]. Column methods
   push a [`ColumnDefinition`]; everything else pushes a command (`Index`,
   `Foreign`, `DropColumn`, `RenameColumn`, `Rename`, `Drop`, ...). In
   `create` mode the columns form the `CREATE TABLE`; in `table` (alter)
   mode each column is itself a command, so `t.string("slug")` followed by
   `t.unique(["slug"])` runs in that order.
2. Fluent indexes (`.unique()`, `.index()`, `.primary()` on a column)
   become index commands when the blueprint is compiled.
3. The grammar of the [`Target`] turns the commands into statements. The
   grammar is a pure function of the blueprint, the target and (for SQLite
   rebuilds) the current table state, which is what makes every dialect
   testable without a server.
4. [`Schema`] executes the statements in order on its connection, logs each
   one (`schema.statements()`), and collects notices (`schema.notices()`)
   such as "TimescaleDB is not installed: `events` is a regular table".

`Schema::offline(target)` records statements without a database: it is
the pretend mode for tests, docs and previews, and it is how the snapshot
tests generate SQL for every dialect.

### Targets

A [`Target`] is detected once per `Schema` from the connection
(`SELECT version()`, `sqlite_version()`, `pg_extension`), or given:
`Target::postgres("17").with_timescaledb("2.17")`, `Target::mysql("8.4")`,
`Target::mariadb("11.4")`, `Target::sqlite("3.46")`. Version checks give
precise errors instead of failing SQL (virtual generated columns need
PostgreSQL 18, `RENAME COLUMN` needs MySQL 8.0.3 or MariaDB 10.5.2, ...).

### Types per dialect

| Blueprint | PostgreSQL | MySQL / MariaDB | SQLite |
|---|---|---|---|
| `id()` / `big_increments` | `BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY` | `BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY` | `INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL` |
| `uuid` / `uuid_primary` | `UUID` | `CHAR(36)` (`UUID` on MariaDB 10.7+) | `VARCHAR` |
| `ulid` / `ulid_primary` | `CHAR(26)` | `CHAR(26)` | `VARCHAR` |
| `string(n)` / `char(n)` | `VARCHAR(n)` / `CHAR(n)` | same | `VARCHAR` |
| `text`, `medium_text`, `long_text` | `TEXT` | `TEXT`, `MEDIUMTEXT`, `LONGTEXT` | `TEXT` |
| `tiny/small/medium/integer/big_integer` | `SMALLINT`/`SMALLINT`/`INTEGER`/`INTEGER`/`BIGINT` | `TINYINT`...`BIGINT` (`UNSIGNED`) | `INTEGER` |
| `decimal(p, s)` | `DECIMAL(p, s)` | `DECIMAL(p, s)` | `NUMERIC` |
| `float(p)`, `double` | `FLOAT(p)`, `DOUBLE PRECISION` | `FLOAT(p)`, `DOUBLE` | `FLOAT`, `DOUBLE` |
| `boolean` | `BOOLEAN` | `TINYINT(1)` | `TINYINT(1)` |
| `date`, `time(p)`, `time_tz(p)` | `DATE`, `TIME(p) WITHOUT/WITH TIME ZONE` | `DATE`, `TIME(p)` | `DATE`, `TIME` |
| `date_time(p)` / `timestamp(p)` | `TIMESTAMP(p) WITHOUT TIME ZONE` | `DATETIME(p)` / `TIMESTAMP(p)` | `DATETIME` |
| `date_time_tz(p)` / `timestamp_tz(p)` | `TIMESTAMP(p) WITH TIME ZONE` | `DATETIME(p)` / `TIMESTAMP(p)` | `DATETIME` |
| `json` / `jsonb` | `JSON` / `JSONB` | `JSON` | `TEXT` |
| `binary` | `BYTEA` | `BLOB` / `VARBINARY(n)` / `BINARY(n)` | `BLOB` |
| `enum_(values)` | `VARCHAR(255) CHECK (... IN (...))` | `ENUM(...)` | `VARCHAR CHECK (... IN (...))` |
| `set(values)` | not supported (error) | `SET(...)` | not supported (error) |
| `ip_address`, `mac_address` | `INET`, `MACADDR` | `VARCHAR(45)`, `VARCHAR(17)` | `VARCHAR` |
| `geometry`, `geography` | `GEOMETRY(type, srid)` (PostGIS) | `GEOMETRY`/subtype `SRID n` | `GEOMETRY` |
| `vector(n)`, `tsvector` | `VECTOR(n)` (pgvector), `TSVECTOR` | `VECTOR(n)` (MySQL 9), error | error |
| `year` | `INTEGER` | `YEAR` | `INTEGER` |
| `raw_column(def)` | as written | as written | as written |

Modifiers: `nullable`, `default`, `use_current`, `use_current_on_update`
(MySQL), `unsigned` (MySQL), `comment` (PostgreSQL as `COMMENT ON`, MySQL
inline, ignored by SQLite), `collation`, `charset` (MySQL), `after` /
`first` (MySQL), `virtual_as` / `stored_as` (generated columns),
`generated_as` / `always` (identity, PostgreSQL), `starting_value`,
`invisible` (MySQL), `change` (alter an existing column), `using`
(PostgreSQL type conversions), `primary`, `unique`, `index`, `fulltext`,
`spatial_index`.

Defaults are typed: integers and floats render bare, strings quoted (with
backslashes escaped on MySQL), booleans as `TRUE`/`FALSE` on PostgreSQL and
`1`/`0` elsewhere, [`Expression`]s as written. MySQL needs parentheses
around a default on `TEXT`, `BLOB`, `JSON` and geometry columns (8.0.13+),
and the grammar adds them.

Generated index names are `{table}_{columns}_{kind}` (`deals_company_id_foreign`,
`deals_title_index`), lower-cased with `-` and `.` replaced by `_`. Names
longer than 63 bytes (the PostgreSQL limit; MySQL's is 64) are shortened to
54 bytes plus `_` and an 8-digit hash, the same on every database, so
`drop_index(["a", "b"])` finds them.

## 4. SQLite: alterations by rebuild

SQLite's `ALTER TABLE` adds, renames and (from 3.35) drops plain columns.
Everything else is done by rebuilding the table, as SQLite documents:

- changing a column, adding or dropping a primary key or a foreign key;
- adding a column SQLite refuses to add in place: a stored generated
  column, a `NOT NULL` column without a default, a column whose default is
  an expression (`CURRENT_TIMESTAMP`, `(...)`), a primary key;
- dropping a column that an index, a primary key or a foreign key uses
  (and any drop before 3.35).

The rebuild reads the table (`pragma_table_xinfo`, `pragma_index_list`,
`pragma_index_info`, `pragma_foreign_key_list`, `sqlite_master.sql` for
collations and generated expressions), applies the blueprint's commands to
that state, then runs: create `__temp__{table}` with the new definition,
copy the surviving columns, drop the old table, rename the new one (with
`legacy_alter_table` on, so views and triggers are left alone), recreate
the indexes. Consecutive commands that need a rebuild share one.

Dropping the old table must not fire foreign-key actions on the tables that
reference it, and `PRAGMA foreign_keys` has no effect inside a transaction.
So `navree-db` runs SQLite migrations on a dedicated connection opened with
foreign keys off, still one transaction per migration, and runs
`PRAGMA foreign_key_check` before each commit: a migration that leaves a
dangling reference fails and rolls back. Outside the migrator, a rebuild on
a connection with foreign keys on that other tables reference is refused
with an error explaining why, instead of deleting rows by cascade.

## 5. TimescaleDB

TimescaleDB is Navree's default database. Developers decide in code which
tables are hypertables:

```rust
schema.create_hypertable("events", "occurred_at", |t| {
    t.uuid_primary();
    t.foreign_uuid("workspace_id");
    t.timestamp_tz("occurred_at");
    t.jsonb("payload");
}).await?;

let mut audit = Hypertable::on("created_at");
audit.chunk_interval("1 day").partition_by("workspace_id", 4).migrate_data();
audit.compress().segment_by(["workspace_id"]).order_by("created_at DESC").after("7 days");
audit.retain_for("365 days");
schema.convert_to_hypertable("audit_logs", &audit).await?;
```

or `t.hypertable("occurred_at")` inside `create` and `table`, which returns
the same [`Hypertable`] builder.

- **Detection and the extension.** The target knows the installed
  TimescaleDB version (`pg_extension`). When the extension is available
  but not installed, `schema.timescaledb = "auto"` (the default) runs
  `CREATE EXTENSION IF NOT EXISTS timescaledb` when the server preloads the
  library and the role may create it; otherwise the table falls back (see
  engines). `"require"` turns every fallback into an error; `"off"` never
  uses TimescaleDB.
- **Hypertables.** `create_hypertable(table, by_range(time, interval),
  if_not_exists, migrate_data)` and `add_dimension(table, by_hash(column,
  n))` on TimescaleDB 2.13 and later; the older `create_hypertable(table,
  time, partitioning_column, number_partitions, chunk_time_interval)` form
  before. Integer time columns take integer chunk intervals.
- **Primary keys and unique indexes.** TimescaleDB enforces uniqueness per
  chunk, so every primary key and unique index must include the time column
  and the space-partition column. In `create`, a primary key that misses
  them is widened to `(id, time[, partition])` (the default
  [`KeyStrategy::Extend`]); with `KeyStrategy::Reject` it is an error. When
  converting an existing table, the helper reads its keys: it rebuilds the
  primary key the same way, and stops with an error naming every unique
  index, and every foreign key of another table that points at the key it
  would change, together with the fix. The docs recommend time-ordered ids
  (UUIDv7 or ULID) for time-series tables and show both strategies.
- **Policies.** `chunk_interval` (`set_chunk_time_interval` for existing
  tables), `compress()` (`ALTER TABLE ... SET (timescaledb.compress,
  timescaledb.compress_segmentby, timescaledb.compress_orderby)` and
  `add_compression_policy`), `retain_for` (`add_retention_policy`),
  `reorder_by(index)` (`add_reorder_policy`), each with a `remove_*`
  counterpart on [`Schema`].
- **Continuous aggregates.** [`ContinuousAggregate`] takes either SQL or a
  structured description (source table, `time_bucket(width, column)`,
  groups, aggregates). On TimescaleDB it is `CREATE MATERIALIZED VIEW ...
  WITH (timescaledb.continuous) AS ... WITH NO DATA` plus
  `add_continuous_aggregate_policy` from its [`RefreshPolicy`].
- **Engines: one migration runs everywhere.** A table that declares a
  hypertable gets the right DDL on each database: a hypertable on
  TimescaleDB; on plain PostgreSQL, MySQL and SQLite a regular table with an
  index on `(time DESC)` (and `(partition, time DESC)`), MySQL's `ENGINE`
  option when one is set, and a notice for each feature that needs
  TimescaleDB (compression, retention, reorder). A continuous aggregate
  becomes a plain view, always current; the structured form compiles
  `time_bucket` to `date_bin` on PostgreSQL 14+, and to epoch arithmetic or
  date formatting on MySQL and SQLite, so the view works there too.

## 6. Migrations

A migration file is a type implementing [`Migration`]:

```rust
pub trait Migration: Send + Sync + 'static {
    fn up(&self, schema: &Schema<'_>) -> impl Future<Output = SchemaResult> + Send;
    fn down(&self, schema: &Schema<'_>) -> impl Future<Output = SchemaResult> + Send;
    fn within_transaction(&self) -> bool { true }
}
```

(`async fn` in the impl.) Modules list their migration files with
`migrations!`, which declares the file modules and returns the module's
`MigrationSet`, named after the files:

```rust
// src/modules/deals/migrations/mod.rs
navree::schema::migrations! {
    module: "deals",
    after: ["companies"],
    files: [
        m20261026_120000_create_deals_table,
        m20261027_093000_add_stage_to_deals,
    ],
}
```

`migration!(file)` wraps one file for an existing `MigrationSet` chain, so
schema migrations and older SeaORM migrations share a set. The adapter,
[`SchemaMigration`], implements SeaORM's `MigrationTrait`, which is what
`MigrationSet` (and the workspace module's per-tenant runner) runs, so every
runner runs schema migrations without changes.

### The runner (in navree-db)

`navree-db`'s `Migrator` and commands, extended for this crate:

| Command | Behaviour |
|---|---|
| `migrate [--pretend] [--step] [--seed] [--schema-path P]` | Runs pending migrations as one batch (`--step`: one batch each). On a database where nothing has run, loads `database/schema/{driver}-schema.sql` first when it exists |
| `migrate:install` | Creates `navree_migrations` |
| `migrate:rollback [--step N \| --batch N] [--pretend]` | Reverts the last batch, the last N migrations, or one batch |
| `migrate:reset [--pretend]` | Reverts every migration, newest first |
| `migrate:refresh [--step N] [--seed]` | Reset (or roll back N), then migrate |
| `migrate:fresh [--seed] [--step] [--drop-views] [--drop-types] [--schema-path P]` | Drops every table (and views, PostgreSQL types), then migrates |
| `migrate:status [--pending]` | Every migration with its batch, or the pending ones |
| `schema:dump [--path P] [--without-migration-data]` | Writes the schema and the `navree_migrations` rows to `database/schema/{driver}-schema.sql` |

- **Pretend.** Migrations run against a recording connection (SeaORM's
  proxy connection): statements are collected and printed, queries are
  answered by the real database, so `has_table` and SQLite rebuilds see
  the real schema. Nothing is written, not even the migrations table.
- **Context.** While a migration runs, the runner sets a task-local
  [`MigrationContext`] carrying the app's [`SchemaOptions`] and a notice
  sink; schema migrations read it, and the commands print the notices.
- **SQLite.** Migrations run on a dedicated connection with foreign keys
  off and `PRAGMA foreign_key_check` before each commit (section 4).
- **Squashing.** `schema:dump` writes plain SQL: SQLite from
  `sqlite_master`, MySQL from `SHOW CREATE TABLE` / `SHOW CREATE VIEW`,
  PostgreSQL from `pg_dump --schema-only` (TimescaleDB's internal schemas
  excluded, the extension, hypertables, dimensions, compression settings,
  policies and continuous aggregates re-created from
  `timescaledb_information`), followed by the migration rows as `INSERT`s.
  Migration files stay in the code (they are compiled in); recorded ones are
  skipped.

`navree make:migration <module> <name>` (the CLI) writes a schema migration
from a template (`create_<table>_table` gets a `create`/`drop_if_exists`
pair, `add_<x>_to_<table>` a `table` pair) and adds it to the module's
`migrations!` list.

## 7. Errors

[`SchemaError`] says what failed, where, and how to fix it:

- `Database { statement, source }`: the statement that failed and the
  driver's error;
- `Unsupported { dialect, feature, hint }`: e.g. "`set` columns are a MySQL
  type; use `enum_` or a `json` column";
- `Definition { table, message }`: an invalid blueprint (two primary keys,
  `virtual_as` and `stored_as` together, a foreign key without a table);
- `Hypertable { table, message }`: a key that cannot include the time
  column, a missing time column, TimescaleDB required but absent;
- `Interval { input }`: an interval that does not parse;
- `Offline { operation }`: an operation that needs a live database.

Inside the migrator it converts to `DbErr::Migration` with the same text.

## 8. Testing

- **Snapshots per dialect** (`insta`): every column type, modifier, index,
  foreign key, alteration and TimescaleDB statement compiled for
  PostgreSQL, TimescaleDB, MySQL, MariaDB and SQLite.
- **SQLite end to end**: create, alter (including every rebuild case, with
  data surviving and foreign keys intact), introspection, and full
  migrator runs through a booted app: batches, steps, rollback, reset,
  refresh, fresh, status, pretend, dump and load.
- **Real servers**, gated by `NAVREE_TEST_POSTGRES_URL` (TimescaleDB),
  `NAVREE_TEST_MYSQL_URL`: the same scenarios against live databases,
  skipped when the variables are unset.

## 9. Extension points

- The ORM's query builder adds `time_bucket`, `first` and `last`; it can
  reuse [`Interval`] and [`Target`].
- A document driver (MongoDB) can compile a [`Blueprint`] (its columns are
  public data) into collection commands: a time-series collection for a
  hypertable, a JSON-schema validator from the columns, TTL from retention.
- Framework crates move their tables to schema migrations in the adoption
  phase; their existing SeaORM migrations keep running meanwhile.
