# navree-schema: design

The schema builder and the migration language of Navree: tables, columns,
indexes and foreign keys described in Rust, compiled to the right DDL for
PostgreSQL (with TimescaleDB), MySQL/MariaDB and SQLite, and run through the
migrator that `navree-db` owns.

```rust
use navree::schema::prelude::*;

pub struct Migration;

impl navree::schema::Migration for Migration {
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
(its configuration section), nothing else from Navree, and on no database
driver: it reaches a database only through its own [`SchemaConnection`]
trait (a dialect, `execute`, and `query` returning [`SchemaRow`]s of
[`SchemaValue`]s, errors as [`SchemaError`]). `navree-db` (L3) depends on it
and implements the trait for its `Pool`, `Transaction`, `Uow` and `Db`:

```
L4+ framework crates (audit, queue, notify, ...) ──► navree-schema, navree-db
L3  navree-db ── runs migrations, owns the commands ──► navree-schema
L2  navree-schema ── builds, compiles, introspects and dumps schemas
```

Why below `navree-db` and not above it:

- **The runner stays in `navree-db`.** Connections, the pool, the unit of
  work, `MigrationSet`, the `navree_migrations` table and the `migrate*`
  commands are `navree-db`'s. The runner hands schema migrations their
  context (options, statements, notices, the pretend reader) and registers
  `schema:dump`, so it must see this crate.
- **Framework crates use it for their own tables.** The audit log, job
  history, notification and webhook delivery logs are time-series tables
  that become hypertables on TimescaleDB. Those crates live at L4 and up, so
  the schema builder has to sit below them.
- **Nothing here needs the pool.** The builder talks to any
  [`SchemaConnection`] (navree-db's pools, transactions, units of work and
  `Db`), so it works inside the unit of work, inside the migrator, and
  offline (no database at all). The tests use a small SQLx connection of
  their own (`tests/common/conn.rs`).

The facade re-exports it as `navree::schema` (feature `schema`, in `full`;
`db` turns it on). `navree_db::navree_schema` re-exports it too.

## 2. Module layout

| Module | Contents |
|---|---|
| `schema` | [`Schema`]: the async entry point (`create`, `table`, `rename`, `drop`, `drop_if_exists`, introspection, TimescaleDB helpers, `dump`, `load`, raw statements), its statement log and notices, offline and pretending modes |
| `blueprint` | [`Blueprint`]: the table being described: every column type, the helpers (`id`, `timestamps`, `morphs`, ...), indexes, foreign keys, drops and renames, table options, the time-series engine (`hypertable`) |
| `column` | [`ColumnDefinition`] (the data) and [`ColumnBuilder`] (the fluent handle every column method returns: modifiers, inline indexes, `constrained`) |
| `index` | [`IndexDefinition`], [`ForeignKeyDefinition`] and their builders, [`ReferentialAction`], the [`Columns`] and [`IndexRef`] argument types |
| `value` | [`DefaultValue`] and [`Expression`] (raw SQL) |
| `interval` | [`Interval`]: `"7 days"` parsed once, rendered per database |
| `target` | [`Dialect`], [`Target`] (dialect, server version, TimescaleDB version), [`Version`] |
| `grammar` | One compiler per dialect (`postgres`, `mysql`, `sqlite`) plus `timescale`; pure functions from a blueprint, a target and (for SQLite rebuilds and hypertable conversions) the table's current state to statements and notices |
| `timescale` | [`Hypertable`], [`Compression`], [`ContinuousAggregate`], [`RefreshPolicy`] |
| `inspect` | Catalog queries per dialect and their results: [`TableInfo`], [`ViewInfo`], [`ColumnInfo`], [`IndexInfo`], [`ForeignKeyInfo`], [`TimeDimension`] (a hypertable's time column, type and chunk interval); target detection; the states rebuilds and conversions read |
| `rebuild` | SQLite table rebuilds: the table state, and the small SQL tokenizer that keeps definitions, constraints, indexes and triggers intact |
| `dump` | Schema dumps read from the catalogs (`dump/sqlite.rs`, `dump/mysql.rs`, `dump/postgres.rs`), [`DumpOptions`] |
| `script` | [`split_statements`]: an SQL script cut into statements per dialect |
| `migration` | [`Migration`] (what migration files implement), [`SchemaMigration`] (the adapter the migrator runs), [`MigrationContext`], the [`migration!`] macro |
| `options` | [`SchemaOptions`]: configuration section `schema` |
| `error` | [`SchemaError`] |

## 3. From a blueprint to SQL

1. `schema.create("deals", |t| ...)` builds a [`Blueprint`]. Column methods
   push a [`ColumnDefinition`]; everything else pushes a command (`Index`,
   `Foreign`, `DropColumn`, `RenameColumn`, `Rename`, `Drop`, ...). In
   `create` mode the columns form the `CREATE TABLE`; in `table` (alter)
   mode each column is itself a command, so `t.string("slug")` followed by
   `t.unique(["slug"])` runs in that order.
2. Indexes declared on columns (`.unique()`, `.index()`, `.primary()`)
   become index commands when the blueprint is compiled, after the
   declared commands.
3. The blueprint is validated (duplicate columns, two primary keys,
   `virtual_as` with `stored_as`, a foreign key without a table, a decimal
   scale larger than its precision, modifiers that do not apply to the
   column's type, ...), each mistake with a message saying how to fix it.
4. The grammar of the [`Target`] turns the commands into statements and
   notices. Grammars are pure functions, which is what makes every dialect
   testable without a server.
5. [`Schema`] runs the statements in order on its connection, logs each one
   ([`statements`](Schema::statements), and the migration context when
   there is one), and reports notices ([`notices`](Schema::notices), the
   log, the migration context) such as "SQLite does not support
   TimescaleDB: `events` is a regular table with an index on `occurred_at`".

`Schema::offline(target)` records statements without a database: previews,
tests and docs, and the snapshot tests of every dialect.
`Schema::new(conn).pretending()` reads the real database but records its
changes instead of running them.

### Targets

A [`Target`] is detected once per `Schema` from the connection
(`server_version_num`, `pg_extension` and `pg_available_extensions`,
`shared_preload_libraries` and the role's superuser flag on PostgreSQL;
`VERSION()` on MySQL and MariaDB; `sqlite_version()`), or given:
`Target::postgres(17).with_timescaledb((2, 17))`, `Target::mysql((8, 4))`,
`Target::mariadb((11, 4))`, `Target::sqlite((3, 46))`. Version checks give
precise errors instead of failing SQL (virtual generated columns need
PostgreSQL 18, `NULLS NOT DISTINCT` PostgreSQL 15, `RENAME COLUMN` MySQL
8.0.3 or MariaDB 10.5.2, expression defaults MySQL 8.0.13, vectors MySQL 9).

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
| `enumeration(values)` | `VARCHAR(255) CHECK (... IN (...))` | `ENUM(...)` | `VARCHAR CHECK (... IN (...))` |
| `set(values)` | not supported (error) | `SET(...)` | not supported (error) |
| `ip_address`, `mac_address` | `INET`, `MACADDR` | `VARCHAR(45)`, `VARCHAR(17)` | `VARCHAR` |
| `geometry`, `geography` | `GEOMETRY(type, srid)` (PostGIS) | `GEOMETRY`/subtype `SRID n` | `GEOMETRY` |
| `vector(n)`, `tsvector` | `VECTOR(n)` (pgvector), `TSVECTOR` | `VECTOR(n)` (MySQL 9, MariaDB 11.7), error | error |
| `year` | `INTEGER` | `YEAR` | `INTEGER` |
| `raw_column(def)` | as written | as written | as written |

Time columns default to six fractional digits (`schema.default_time_precision`).

Modifiers: `nullable`, `not_null`, `default`, `use_current`,
`use_current_on_update` (MySQL), `on_update` (MySQL), `unsigned` (MySQL),
`comment` (PostgreSQL as `COMMENT ON`, MySQL inline, ignored by SQLite),
`collation`, `charset` (MySQL), `after` / `first` (MySQL), `virtual_as` /
`stored_as` (generated columns), `generated_as` / `generated_as_with` /
`always` (identity, PostgreSQL), `starting_value`, `invisible` (MySQL),
`change` (alter an existing column; the definition is complete, so a
default or `nullable` left out is removed), `using` (PostgreSQL type
conversions), `primary`, `unique`, `index`, `fulltext`, `spatial_index`,
`without_*` (drop a column's conventional index with `change`).

Defaults are typed: integers and floats render bare, strings quoted (with
backslashes escaped on MySQL), booleans as `TRUE`/`FALSE` on PostgreSQL and
`1`/`0` elsewhere, [`Expression`]s as written; MySQL gets parentheses around
defaults on `TEXT`, `BLOB`, `JSON` and geometry columns (8.0.13+).

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
- dropping a column that a primary key, a foreign key, a table constraint
  or an inline `UNIQUE` uses (and any drop before 3.35).

The rebuild reads the table (`pragma_table_xinfo`, `pragma_index_list`,
`pragma_index_info`, `pragma_foreign_key_list`, `sqlite_master.sql` for
collations, checks and generated expressions), applies the blueprint's
commands to that state, then runs: create `__temp__{table}` with the new
definition, copy the surviving columns, drop the old table, rename the new
one (with `legacy_alter_table` on, so views and triggers are left alone),
recreate the indexes and triggers. Consecutive commands that need a rebuild
share one; a drop that follows a pending rebuild is part of it.

Dropping the old table must not fire foreign-key actions on the tables that
reference it, and `PRAGMA foreign_keys` has no effect inside a transaction.
So `navree-db` runs SQLite migrations on a connection of their own opened
with foreign keys off, one transaction per migration, and runs
`PRAGMA foreign_key_check` before each commit: a migration that leaves a
dangling reference fails and rolls back. Outside the migrator, a rebuild on
a connection with foreign keys on that other tables reference is refused
with an error explaining why, instead of deleting rows by cascade.

## 5. TimescaleDB

TimescaleDB is Navree's default database. Developers decide in code which
tables are hypertables:

```rust
schema
    .create_hypertable("events", "occurred_at", |t| {
        t.uuid_primary();
        t.foreign_uuid("workspace_id");
        t.timestamp_tz("occurred_at");
        t.jsonb("payload");
        let events = t.hypertable("occurred_at");
        events.chunk_interval("1 day").retain_for("90 days");
        events.compress().segment_by(["workspace_id"]).after("7 days");
    })
    .await?;

let mut audit = Hypertable::on("created_at");
audit.chunk_interval("1 day").partition_by("workspace_id", 4).migrate_data();
audit.compress().segment_by(["workspace_id"]).order_by("created_at DESC").after("7 days");
audit.retain_for("365 days");
schema.convert_to_hypertable("audit_logs", &audit).await?;
```

A [`Hypertable`] and its [`Compression`] are configured in place (`&mut`
setters returning `&mut Self`, like the blueprint they belong to);
`&mut Hypertable` converts into a `Hypertable`, so a one-expression chain
can be passed as it is built. `t.hypertable(...)` works inside `create` and
`table` (where it converts the table); naming the time column of a
hypertable already declared returns it.

- **Detection and the extension.** The target knows the installed
  TimescaleDB version. When the extension is available but not installed
  (the server preloads the library and the role is a superuser),
  `schema.timescaledb = "auto"` (the default) runs `CREATE EXTENSION IF NOT
  EXISTS timescaledb` before the first hypertable, if
  `schema.create_extensions` allows it. `"require"` turns the fallback on
  PostgreSQL into an error (other databases still fall back, so tests on
  SQLite run the same migrations); `"off"` never uses TimescaleDB.
- **Hypertables.** `create_hypertable(table, by_range(time, interval))` and
  `add_dimension(table, by_hash(column, n))` on TimescaleDB 2.13 and
  later; the positional form before. Integer time columns take integer
  chunk intervals, temporal ones intervals; a mismatch is an error.
  Converting an existing table adds `if_not_exists => TRUE`, and a table
  with rows needs `.migrate_data()`.
- **Primary keys and unique indexes.** TimescaleDB enforces uniqueness per
  chunk, so every primary key and unique index must include the time column
  and the space-partition column. In `create`, a primary key that misses
  them is widened to `(id, time[, partition])` (the default
  [`KeyStrategy::Extend`], with a notice) on every database, so one
  migration gives the same keys everywhere (SQLite's `AUTOINCREMENT` key
  stays alone, with a notice); with `KeyStrategy::Reject` it is an error. A
  unique index that misses them is an error on every database. Converting an
  existing table on TimescaleDB rebuilds the primary key the same way, or
  stops naming every foreign key of another table that points at it.
- **Policies.** `chunk_interval` (`set_chunk_interval` for existing
  tables), compression (`ALTER TABLE ... SET (timescaledb.compress,
  timescaledb.compress_segmentby, timescaledb.compress_orderby)` and
  `add_compression_policy`), `retain_for` (`add_retention_policy`),
  `reorder_by(index)` (`add_reorder_policy`), each with its
  `remove_*_policy` on [`Schema`].
- **Continuous aggregates.** [`ContinuousAggregate`] takes either SQL or a
  structured description (source table, `time_bucket(width, column, alias)`,
  groups, aggregates, a filter). On TimescaleDB it is `CREATE MATERIALIZED
  VIEW ... WITH (timescaledb.continuous) AS ... WITH NO DATA` plus
  `add_continuous_aggregate_policy` from its [`RefreshPolicy`];
  `refresh_continuous_aggregate` and the policy operations exist on
  [`Schema`].
- **Reading hypertables back.** [`Schema::is_hypertable`] and
  [`Schema::time_dimension`] (a [`TimeDimension`]: the time column, its
  type and the chunk interval) read `timescaledb_information`. Without
  TimescaleDB they answer `false` and `None` with no query beyond the
  target's detection (none at all on a MySQL or SQLite connection), so a
  conversion that may run twice checks first, and `navree-db`'s
  `prune_time_series` drops whole chunks only when the pruned column is the
  table's time dimension, casting its cutoff to that column's type.
- **Engines: one migration runs everywhere.** A table that declares a
  hypertable gets the right DDL on each database: a hypertable on
  TimescaleDB; on plain PostgreSQL, MySQL/MariaDB and SQLite a regular table
  with indexes on `(time DESC)` and `(partition, time DESC)` (unless an
  index already starts with them), and a notice naming every policy skipped.
  MySQL also takes table options (`t.engine("InnoDB")`, charset,
  collation). A described continuous aggregate becomes a plain view whose
  bucket compiles to `date_bin` on PostgreSQL 14+ (`date_trunc` for
  months and years), epoch arithmetic or date formatting on MySQL and
  SQLite, aligned like `time_bucket` (Monday 2000-01-03); an aggregate given
  as SQL that calls `time_bucket` is an error there, naming the fix.

## 6. Migrations

A migration file is a type implementing [`Migration`]:

```rust
pub trait Migration: Send + Sync + 'static {
    fn up(&self, schema: &Schema<'_>) -> impl Future<Output = SchemaResult> + Send;
    fn down(&self, schema: &Schema<'_>) -> impl Future<Output = SchemaResult> + Send;
    fn within_transaction(&self) -> bool { true }
}
```

(written with `async fn` in the impl). A module lists its migration files in
its `MigrationSet`, each with `migration!(file)`, which names the migration
after its file:

```rust
// src/modules/deals/migrations/mod.rs
use navree::db::MigrationSet;
use navree::schema::migration;

mod m20261026_120000_create_deals_table;
mod m20261027_093000_add_stage_to_deals;

pub fn migrations() -> MigrationSet {
    MigrationSet::new("deals")
        .after(["companies"])
        .migration(migration!(m20261026_120000_create_deals_table))
        .migration(migration!(m20261027_093000_add_stage_to_deals))
}
```

[`SchemaMigration`], what `migration!` builds, is a [`NamedMigration`]
(the object-safe face of a [`Migration`] with its name), which is what
`MigrationSet` holds and the migrator runs; `MigrationSet::entries`
hands them to runners of their own (the workspace module's per-tenant
runner).

`navree make:migration <module> <name>` (the CLI) writes a schema
migration: `create_<table>[_table]` gets a `create`/`drop_if_exists` pair,
`<change>_to|from|in_<table>[_table]` a `table` pair, anything else an
empty pair; the file is declared and added to the module's set with
`migration!`. `navree make:module` and `navree new` write schema migrations
too.

### The runner (in navree-db)

`navree_db::Migrator`, and the commands `DatabaseModule` registers:

| Command | Behaviour |
|---|---|
| `migrate [--pretend] [--step] [--seed] [--schema-path P] [--force]` | Runs the pending migrations as one batch (`--step`: one batch each). On an empty database (no tables, or only an empty migrations table) it first loads the schema dump, `{schema.dump_path}/{driver}-schema.sql` or `--schema-path`, then runs only the migrations the dump does not record |
| `migrate:install` | Creates `navree_migrations` |
| `migrate:rollback [--step N \| --batch N] [--pretend] [--force]` | Reverts the last batch, the last N migrations (across batches), or batch N, newest first |
| `migrate:reset [--pretend] [--force]` | Reverts every migration, newest first |
| `migrate:refresh [--step N] [--seed] [--force]` | Reset (or roll back N), then migrate |
| `migrate:fresh [--seed] [--drop-views] [--drop-types] [--step] [--schema-path P] [--force]` | Drops every table (then views; PostgreSQL types with `--drop-types`, an error elsewhere checked before anything is dropped), then migrates |
| `migrate:status [--pending]` | Every migration with its batch, or the pending ones |
| `schema:dump [--path P]` | Writes the schema and the `navree_migrations` rows to `{schema.dump_path}/{driver}-schema.sql` |

- **Context.** Each migration runs inside a [`MigrationContext`]: its
  `Schema` uses the app's [`SchemaOptions`] (section `schema`), and the
  statements it runs and the notices it reports land in the
  `MigrationReport` the commands print.
- **Pretend.** Nothing runs, not even the migrations table. Schema
  migrations read the real database through the context's reader (so
  SQLite rebuilds and hypertable conversions plan against the real tables)
  and record what they would change (`Schema::pretending` through the
  context). A migration that depends on an
  earlier pending one's tables cannot be planned and says so.
- **Transactions.** A migration runs in a navree-db transaction (on the
  direct connection when there is one) where DDL is transactional
  (PostgreSQL, SQLite; on MySQL when `within_transaction_on_mysql` asks),
  with its `navree_migrations` row in the same transaction, so a failure
  leaves nothing behind. `within_transaction() == false` opts out (`CREATE
  INDEX CONCURRENTLY`, refreshing a continuous aggregate).
- **SQLite.** Migrations run on a connection of their own with foreign keys
  off, and `PRAGMA foreign_key_check` must pass before each commit
  (section 4). In-memory databases use the pool.
- **Production.** `migrate:rollback`, `migrate:reset`, `migrate:refresh` and
  `migrate:fresh` refuse to run when the configuration's `env` (or
  `app.env`) is `production`, unless `--force` is given.
- **Dumps.** `schema:dump` reads the catalogs; no client tool is needed
  (section 7). The dump loads in one transaction (on MySQL, on one pinned
  connection). Migration files stay in the code (they are compiled in);
  the ones the dump records are skipped.

## 7. Schema dumps

[`Schema::dump`] writes the structure of the database as an SQL script,
read from its catalogs:

- **SQLite**: the `CREATE` statements in `sqlite_master` (tables, indexes,
  views, triggers), without SQLite's own and virtual-table shadow tables.
- **MySQL / MariaDB**: `SHOW CREATE TABLE` (without the `AUTO_INCREMENT=n`
  counter), routines and triggers (without definers, between
  `DELIMITER ;;` and `DELIMITER ;`), views (without definers and default
  options) after the views they read, all between `SET
  FOREIGN_KEY_CHECKS=0` and `=1`.
- **PostgreSQL**: from the catalogs of the schemas on the search path,
  skipping every object an extension owns: schemas, extensions, enum,
  range, domain and composite types, functions and procedures
  (`pg_get_functiondef`), sequences and their owners, tables (identity
  options, generated columns, collations, partitions, `UNLOGGED`, storage
  options), keys and checks (`pg_get_constraintdef`), indexes
  (`pg_get_indexdef`), foreign keys, views and materialized views in
  dependency order, triggers, row-level security (enabled, forced,
  policies) and comments. `SET LOCAL check_function_bodies = false` lets
  functions come before the tables they read.
- **TimescaleDB**: hypertables re-created with `create_hypertable(...,
  create_default_indexes => FALSE)` (the dump has every index) and
  `add_dimension`, integer-now functions, compression settings, continuous
  aggregates (not as plain views) in dependency order, and the compression,
  retention, reorder and refresh policies from `timescaledb_information`;
  the app's own jobs are listed in a comment.
- **Data**: the rows of [`DumpOptions::data`] tables as `INSERT`s, each
  value written as a literal by the database itself.

[`split_statements`] cuts a script into statements the way each database's
client does (strings, quoted identifiers, comments, PostgreSQL dollar quotes
and `BEGIN ATOMIC` bodies, SQLite trigger bodies, MySQL `DELIMITER`), and
[`Schema::load`] runs them. A dump loaded into an empty database and dumped
again gives the same script; the real-server tests check it on every
database.

## 8. Errors

[`SchemaError`] says what failed, where, and how to fix it:

- `Database { statement, source }`: the statement that failed and the
  driver's error;
- `Unsupported { dialect, feature, hint }`: e.g. "SQLite does not support
  vector columns (column `embedding`): store embeddings in a `binary`
  column, or use PostgreSQL with pgvector";
- `Definition { table, message }`: an invalid blueprint;
- `Hypertable { table, message }`: a key that cannot include the time
  column, a missing or non-time time column, TimescaleDB required but
  absent, rows without `migrate_data`;
- `Interval { input }`: an interval that does not parse;
- `Offline { operation }`: an operation that needs a live database;
- `Missing { message }`: a table, column or index that does not exist;
- `Dump { message }`: a dump that could not be read or written.

Inside the migrator it converts to `DbErr::Migration` with the same text.

## 9. Testing

- **Snapshots per dialect** (`tests/schema/`, `insta`): every column type,
  modifier, index, foreign key, alteration, TimescaleDB statement and
  fallback compiled for PostgreSQL, TimescaleDB, MySQL, MariaDB and SQLite.
- **SQLite end to end** (`tests/schema/sqlite.rs`): create, alter (every
  rebuild case, with rows, triggers, partial indexes and `AUTOINCREMENT`
  surviving), introspection, pretending, hypertable and aggregate
  fallbacks, dumps loaded and dumped again.
- **Real servers** (`tests/servers/`, features `postgres` and `mysql`,
  variables `NAVREE_TEST_POSTGRES_URL` and `NAVREE_TEST_MYSQL_URL`): DDL,
  alterations with rows, introspection, hypertables with dimensions,
  compression, policies and continuous aggregates, conversions, fallbacks,
  and dump round trips, each test in a database of its own; skipped with a
  notice when the variable is unset.
- **The runner** (`navree-db`'s `tests/db/schema_migrations.rs`): steps, batches, rollbacks, reset, refresh,
  pretend, SQLite rebuilds of referenced tables, the foreign-key check,
  options from configuration, dumps loaded into empty databases, and every
  command with its option errors and the production guard.

## 10. Extension points

- **Document databases.** The blueprint is public data
  ([`Blueprint::columns`], [`Blueprint::commands`],
  [`Blueprint::hypertable_definition`], the [`Hypertable`] getters, and
  [`ContinuousAggregate::query`] as an [`AggregateSpec`]), so a document
  driver (the ORM's MongoDB crate) can compile it into collection commands:
  a time-series collection for a hypertable (time field, granularity from
  the chunk interval, TTL from retention), a JSON-schema validator from the
  columns, indexes from the index commands, a `$group` view for a described
  aggregate.
- **The query builder.** The ORM's `time_bucket`, `first` and `last` can
  reuse [`Interval`] and [`Target`].
- **Framework tables.** The framework's time-series tables are
  hypertables: each module adds a schema migration (a [`SchemaMigration`]
  in its set) that converts them with `migrate_data` and
  `KeyStrategy::Extend`, unless [`Schema::is_hypertable`] says they already
  are: `audit_logs` (`created_at`, 7 days), `webhook_deliveries` and
  `webhook_attempts` (`created_at`, 1 day; attempts reference their endpoint
  instead of their delivery, since a hypertable's key includes its time),
  `fleet_jobs` and `fleet_metrics` (Unix milliseconds, 1 hour and 1 day),
  `schedule_runs` (`tick`, 1 day) and Lighthouse's samples, occurrences and
  aggregates (1 day; also in `lighthouse.database_url`'s database at boot,
  when it runs TimescaleDB). Their `down` keeps the hypertable, which
  TimescaleDB cannot turn back into a plain table in place. Their prune
  commands go through `navree_db::prune_time_series`. The facade's
  `time_series` suite checks all of it on real TimescaleDB and MySQL
  servers. Every framework migration is a schema migration; the ones that
  create the framework's tables kept their names and spell out the column
  types the databases already have (unbounded `varchar` on PostgreSQL,
  `binary(16)` UUIDs on MySQL where they were), so new databases get the
  same columns as the ones already migrated, and those find nothing
  pending (the facade's `production_migrations` suite checks the
  upgrade).
