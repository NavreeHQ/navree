# Navree's data layer on SQLx

Date: 2026-10-28. Status: phase 1 (architecture and foundation) built on
`build/data-layer`; the porting phases below are planned, not done.

## 1. Decisions and starting point

The ownership decisions this spec turns into architecture:

1. **Own SQL generation.** navree-orm renders its own SQL; sea-query leaves.
2. **navree-db runs directly on SQLx.** Pools, transactions and the unit of
   work are navree-db's own code on SQLx; SeaORM leaves.
3. **Port migrations and entities.** Every SeaORM migration becomes a
   navree-schema migration, every SeaORM entity a navree-orm model (or plain
   navree-db statements where a crate sits below the ORM).
4. **SQLite becomes opt-in.** TimescaleDB (PostgreSQL) stays the default;
   SQLite, today compiled into every build, becomes a feature.

MongoDB (navree-mongo) is not affected beyond one value type (§2.6).

Where the workspace stands (survey of 2026-10-28, every number counted in
the source):

| | Count |
|---|---|
| Crates touching SeaORM, sea-query or sea-orm-migration | 45 (36 declare sea-orm, 33 sea-orm-migration, 2 sea-query; 7 more use `navree_db::sea_orm`, 2 only mention it in docs) |
| SeaORM migrations (`impl MigrationTrait`) | 39 in `src/` of 25 crates; 83 counting tests, in 32 crates; 2 more in navree-cli templates |
| navree-schema migrations | 17 in `src/` of 11 crates (plus 2 in crm-core); navree-schema is referenced by 17 crates, declared by 3 |
| SeaORM entities (`DeriveEntityModel`) | 60 real ones in 17 crates (40 already also derive `navree_orm::Model`, 20 SeaORM-only); 52 more in tests |
| Lines naming SeaORM / sea-query | 945 in `src/` (232 files), 613 in tests (195 files), 48 in navree-cli templates, 148 in 44 docs pages |
| Production DDL built with sea-query | about 82 `Table::create`, 17 `Table::alter`, 77 `Index::create`, 31 `ForeignKey::create` |
| `uow.tx()` / `.conn()` / `ConnectionTrait` / `DbErr` / `DbBackend` lines in `src/` | 228 / 184 / 157 / 579 / 220 |
| navree-orm files naming sea-query | 10 (`sql/{mod,select,filter,write,timescale}.rs` hold 190 of its 198 builder lines; `value.rs` re-exports `sea_query::Value`) |

## 2. Target architecture

```text
 L2  navree-schema   schema builder, DDL grammars, migrations, dumps     (no SeaORM; own SchemaConnection)
 L3  navree-db       engine on SQLx: Backend, Value, Statement, Row,     (no SeaORM)
                     DbError, Pool, Transaction, Executor; Db, Uow,
                     migrator, health, query log
 L4  navree-orm      models, builders, IR, render (AST + grammars),      (no sea-query)
                     execution through navree-db's Executor
 L4+ crates          navree-orm models where the layer rules allow,
                     navree-db statements below/beside the ORM,
                     navree-schema migrations everywhere
```

### 2.1 navree-db: the engine (`crates/navree-db/src/engine/`)

Everything below exists on `build/data-layer` and is exported at the crate
root.

| Item | What it is |
|---|---|
| `Backend` | `Postgres` (and TimescaleDB), `MySql` (and MariaDB), `Sqlite`; picks itself from a URL; quoting (`quote_identifier`, schema-qualified names split), placeholders (`$n` / `?`), capabilities (`supports_returning`, `supports_row_locks`, `max_bind_values`), whether its driver is compiled in. |
| `Value`, `Kind`, `HasKind` | What a statement binds and a row holds: `Null(Kind)` (typed NULLs, which PostgreSQL needs), booleans, signed and unsigned integers of every width, `f32`/`f64`, exact `Decimal`, text, bytes, JSON, UUIDs, dates, times, naive date-times, UTC instants and instants with an offset. `From` for every Rust type and `Option<T>` (a `None` keeps its kind); `to_json`, `to_sql_literal` (logs only: values are always bound). |
| `FromValue` | Reading a Rust type from a `Value`: exact first, then tolerant where databases store the same thing differently (any integer width that fits, integers from whole decimals, booleans from `0`/`1`, UUIDs from text or 16 bytes, date-times/decimals/JSON from text). `NULL` reads only as `Option`. |
| `Statement` | SQL and its values. `Statement::new(backend, sql, values)` takes SQL in the backend's own placeholders (what a renderer produces); `Statement::portable(sql, values)` takes `?` placeholders, rewritten per backend when it runs (`??` is a literal `?`, quoted text and comments are left alone, the count is checked); `Statement::text(sql)` binds nothing. A native statement of another backend is refused. `Display` writes the values in, for logs. |
| `Row`, `ColumnIndex` | One row: `get::<T>(name or index)`, `value(..)` (dynamic, by the column's type per backend: PostgreSQL by type name incl. arrays and pgvector `vector` as JSON; MySQL by column type, `TINYINT(1)` as `I8`; SQLite by what the cell holds), `values`, `columns`, `to_json`, `Row::from_values` for rows built by hand. |
| `DbError`, `ErrorKind` | Classified errors, the same way on every backend: `NotFound`, `UniqueViolation`, `ForeignKeyViolation`, `NotNullViolation`, `CheckViolation`, `Deadlock`, `SerializationFailure`, `LockUnavailable` (NOWAIT, lock wait timeout, SQLite busy), `Timeout`, `ConnectionLost`, `PoolTimeout`, `PoolClosed`, `Connect`, `Decode`, `InvalidStatement`, `Unsupported`, `TransactionDone`, `Database`, `Other`; `code()` (PostgreSQL SQLSTATE, MySQL error number, SQLite extended code), `constraint()`, `is_retryable()`. Messages never carry bound values. |
| `Executor`, `ExecutorExt` | What statements run on (object safe): `execute` (→ `Executed { rows_affected, last_insert_id }`), `fetch_all`, `fetch_optional`, `fetch_one`, `fetch` (a `RowStream`), `execute_script` (unprepared, several statements). `ExecutorExt::fetch_scalar` / `fetch_column` read typed values. Implemented for `Pool`, `Transaction`, `Uow`, `Db`, and `&E`, `Arc<E>`, `Box<E>`. |
| `Pool` | One SQLx pool with the settings of a `DbOptions`: sizes, acquire timeout, idle and lifetime limits, an idle ping (or `test_before_acquire`), SQLite pragmas (journal mode, busy timeout, foreign keys, `synchronous`, any pragma), PostgreSQL session settings (`search_path`, `TimeZone` after connect, `synchronous_commit`, default isolation, application name, TLS mode), MySQL's session (SQL mode, isolation level, UTC, `utf8mb4`, TLS when offered). `begin`, `begin_with`, `ping`, `stats`, `close`. |
| `Transaction`, `TxOptions` | A transaction or savepoint: `TxOptions` (isolation level, read only, SQLite `IMMEDIATE`; MySQL sets the level before `BEGIN`), `savepoint()` nests (only from the innermost open level), `commit` (`RELEASE` for a savepoint; refused while a savepoint inside is open), `rollback` (also of open savepoints inside). Shared through `&self`: statements on one transaction run one at a time; a stream holds the connection until it ends or is dropped. A handle dropped unfinished rolls its level back before the next statement of another handle, or when the connection goes back to the pool (rollbacks are queued per open level; SQLite's worker lowers the depth asynchronously, so the depth is read once). |
| Instrumentation (`engine/report.rs`) | Every statement, `BEGIN`, savepoint, `COMMIT` and `ROLLBACK` is timed and reported to the connection's watchers (`QueryLog::capture`, `Db::on_query`, the unit of work's `query_stats` and `Db::when_querying_for_longer_than`), runs the `before_executing` / `before_starting_transaction` hooks, emits `QueryExecuted` telemetry (now with the row count) and the `navree_db::query` logs (debug with `log_queries`, warn above `slow_query_ms`). SeaORM's statements reach the same reporter through its metric callback. |

**`Db` on the engine.** `Db::connect` opens navree-db's own pools (writer,
read replicas; the direct connection on first use; named connections
through `DbConnections`). `Db::pool()` / `Db::read_pool()` hand them out
(the replica in turn, the writer inside `Db::using_writer`), `Db::begin` /
`begin_with` begin transactions, `Db::ping` checks the writer, and `Db` is
an `Executor` that routes like the ORM: into the task's unit of work on the
same pool when its transaction is open (reads and writes), and for writes
(`execute`, `execute_script`) also when it has not begun yet (the write
begins it), else on the writer. Connection errors name the database without
its password and the feature to turn on for a driver that is not compiled
in.

**Units of work.** One `Uow` per request, job run or command, as today:
lazy, current for its task (`Uow::current`, `scope`), its actor, request id,
impersonator and workspace, change events and observers, after-commit
hooks, statistics, `commit` / `rollback` / `rollback_as_committed`. Per
backend, unchanged: SQLite begins `IMMEDIATE`; PostgreSQL sets
`navree.actor` and `navree.request_id` with `set_config(.., true)` for the
transaction; MySQL sets `@navree_actor` / `@navree_request_id` after `BEGIN`
and clears them before the transaction ends (the connection goes back to
the pool). New on the engine:

- `Uow` is an `Executor` (its transaction, begun on the first statement);
  `execute` marks the unit of work as having written (sticky reads).
- `Uow::savepoint()` nests a savepoint; `Uow::set_local(name, value)` sets a
  transaction-local setting for triggers, row-level security and views:
  `set_config(name, value, true)` on PostgreSQL (`navree.workspace`,
  `search_path`), the session variable `@name` on MySQL (cleared with the
  unit of work's own before the transaction ends), nothing on SQLite.
- `ChangeObserver::observe_with(&Uow, &[ChangeEvent])` writes through the
  unit of work; `observe(&DatabaseTransaction, ..)` keeps working and by
  default runs `observe_with`.
- **The outbox.** A transactional outbox needs nothing more: rows written
  through the unit of work commit or roll back with the business rows, and
  an `after_commit` hook relays them once they are durable (navree-micro's
  transactional outbox works this way).

**Two transactions during the port.** SeaORM code holds a concrete
`sea_orm::DatabaseTransaction` (`Uow::tx()` returns `Arc<DatabaseTransaction>`;
69 lines in 11 crates name the type, 6 `ChangeObserver`s take it). SeaORM's
transaction cannot be built on a connection it did not open, and navree-db
cannot share one with it, so for the length of the port:

- A unit of work runs on **SeaORM's transaction by default**, begun on the
  SeaORM view of navree-db's pool (`from_sqlx_*_pool`: the same
  connections). navree-db's statements on it (the `Uow` as an `Executor`,
  `Db` joining it) run in that same transaction through the bridge
  (`navree_db::compat::SeaExecutor`: values converted to SeaORM's, rows read
  back from the driver rows SeaORM holds).
- A `Db` made with `with_native_units_of_work(true)` gives units of work on
  **navree-db's own transaction**. `Uow::tx()` then fails (there is no SeaORM
  transaction to hand out), observers run through `observe_with`.

Crates port one at a time while both kinds coexist; once no crate takes
SeaORM's transaction, navree-db flips to its own and deletes the bridge
(§4, step 8).

**Read/write connections and health.** Unchanged in behaviour, now on
navree-db's pools: read replicas in turn, sticky reads after a unit of
work wrote, `Db::using_writer`, the direct connection past a
transaction-mode pooler (migrations, `LISTEN`, advisory locks), named
connections, `PoolStats` from navree-db's pool, the `database` health check
and `db:monitor` (`open_connections`, `server_version`, `is_mariadb`,
`has_timescaledb`).

**SeaORM's surface in navree-db today** (each item has its replacement in
§3): `Db::conn()` / `read_conn()` return SeaORM views over navree-db's own
pools; `Db::from_connection` still takes a SeaORM connection (a scripted one
has no `Pool`; navree-db's statements then go through it); `Repo`,
`paginate`, `seed::Factory`, `Reader`, the text helpers, `prune_time_series`,
`LoggedQuery`'s value type, `map_db_error(&DbErr)` (navree-db errors convert
to the `DbErr` SeaORM would return, so it classifies them alike) and the
migrator's `MigrationTrait` path.

### 2.2 navree-orm's own SQL renderer (`crates/navree-orm/src/render/`)

Built on `build/data-layer-render`, merged into `build/data-layer`. No
sea-query or SeaORM inside (a unit test reads the sources).

- `ast.rs`: the syntax tree. `Select` (DISTINCT / DISTINCT ON, items with
  aliases, `*` and `t.*`, FROM table, alias, derived table or table
  function, joins incl. FULL, lateral and left lateral, WHERE, GROUP BY,
  HAVING, ORDER BY with NULLS FIRST/LAST, LIMIT/OFFSET, UNION / INTERSECT /
  EXCEPT with orders and limits on parts and on the whole, WITH and WITH
  RECURSIVE, window functions, row locks with `OF`, NOWAIT, SKIP LOCKED,
  MySQL's execution-time hint); `Insert` (rows, INSERT .. SELECT, DEFAULT
  VALUES, insert-or-ignore, upserts with `WHERE`, RETURNING); `Update` (SET
  expressions, FROM / joined update, ORDER BY + LIMIT where allowed,
  RETURNING); `Delete`; `Truncate`. Expressions: columns, bound values, raw
  fragments with portable `?`, `{}` templates, functions, aggregates with
  DISTINCT, operators with precedence-aware parentheses, AND/OR/NOT,
  IS [NOT] NULL, [NOT] IN lists (empty lists stay valid) and subqueries,
  BETWEEN, LIKE with ESCAPE, case-insensitive LIKE per dialect, EXISTS,
  CAST (portable types and named ones), scalar subqueries, tuples, CASE,
  JSON paths as text or JSON per dialect (negative indexes too), the upsert's
  proposed row (`excluded.c` / `VALUES(c)`), `DEFAULT`.
- `writer.rs`: SQL buffer, binds, `$n` / `?` numbering across nested
  queries, identifier quoting with doubled quotes, schema-qualified names as
  parts.
- `grammar/{mod,postgres,mysql,sqlite}.rs`: shared SQL as `Grammar`
  defaults with a precedence model; each dialect overrides what differs.
- `lower/`: the ORM's IR lowered into the tree: `render_select`,
  `render_aggregate`, `render_exists`, `render_insert`, `render_update`,
  `render_update_from`, `render_delete`, `render_truncate`, with the
  signatures and semantics of `sql::compile_*`. `Where::Sql` (a sea-query
  condition from navree-access) answers `Unsupported` until navree-access is
  ported.

Evidence that it can replace sea-query:

| | SQLite | PostgreSQL + TimescaleDB | MySQL 8.4 |
|---|---:|---:|---:|
| Differential cases, renderer vs today's compiler, run on the server | 191 | 191 | 189 |
| same rows (non-empty) | 131 (127) | 147 (143) | 138 (134) |
| same writes (affected rows and table contents) | 27 | 27 | 25 |
| refused by both | 23 | 12 | 17 |
| failed on both at execution | 6 | 4 | 6 |
| expected differences (compiler bugs, below) | 4 | 1 | 3 |
| mismatches | 0 | 0 | 0 |
| Hand-built trees with expected rows, same rows on all three | 40 run, 4 refused as expected | 42 run, 2 refused | 42 run, 2 refused |

Plus 41 snapshot tests (133 snapshots, the three dialects each) and, with
the test-only feature `render-sql-for-tests` routing `compile_*` to
`render_*`, the whole ORM suite: lib 147/147, `orm` 265/265, `strict_apps`
6/6, `servers` 41/41 on PostgreSQL and MySQL; the only behaviour failures are
3 `integration::access` tests (navree-access's `Where::Sql`), the other 31
failures are SQL text (snapshots). The runner fails when an expected
difference stops differing, and a planted `<` → `<=` bug was caught three
times. Today's compiler gets five things wrong that the renderer gets right
(MySQL JSON keys with `"` or `\`; schema-qualified names quoted as one
identifier; SQLite unions nested in unions; SQLite union parts with a
per-group limit; MySQL raw window orders with NULLS FIRST/LAST).

**The switch** (§4, steps 2-3): `sql::compile_*` become the `render_*`
functions; the sea-query compiler and its `build_any` go; the 26 + 4 SQL
snapshots that change are reviewed against the differential results;
`Value` becomes navree-db's; `Row` wraps `navree_db::Row`; `exec.rs`
executes through navree-db's `Executor` (pools, transactions, units of work,
savepoints, streams, PostgreSQL `SET LOCAL statement_timeout` in a
savepoint); `OrmError::Db` carries a `DbError`.

### 2.3 navree-schema: the only migration system

navree-schema already compiles DDL itself (grammars for PostgreSQL with
TimescaleDB, MySQL and SQLite, SQLite table rebuilds, hypertables,
compression, retention, continuous aggregates, dumps and loads); SeaORM is
only its executor (18 lines). The end state:

- **Connection.** `SchemaConnection` keeps its shape (`backend`, `execute`,
  `query`) on navree-schema's own types: its `Dialect` instead of
  `DbBackend`, a small `SchemaRow` (column names and `Null` / integer /
  float / text / bytes / bool values, what the catalog readers need) instead
  of `QueryResult`, `SchemaError` instead of `DbErr`. navree-schema stays at
  L2 and depends on neither navree-db nor SQLx; navree-db implements
  `SchemaConnection` for `Pool`, `Transaction`, `Uow` and `Db`.
- **Migrations.** `navree_schema::Migration` (`up`, `down`,
  `within_transaction`) is the only migration trait. `MigrationSet` holds
  them directly (`migration!` and `SchemaMigration::new(name, M)` stay as the
  ways to add one); the `MigrationTrait` adapter goes.
- **Discovery.** As today: each module contributes its `MigrationSet`
  (`ModuleBuilderDbExt::migrations`), sets run in `after` order then by
  module name, migrations by name. `navree make:migration` writes a
  navree-schema migration file (navree-cli's templates move off
  `MigrationTrait`).
- **The migrations table.** `navree_migrations` (module, name, batch, run
  time) is unchanged, so deployed databases keep their history: a ported
  migration keeps its exact name and is not run again.
- **Batches and commands.** `migrate`, `migrate:install`,
  `migrate:rollback [--step N]`, `migrate:reset`, `migrate:refresh`,
  `migrate:fresh [--seed]`, `migrate:status`, `--pretend`, `schema:dump`,
  `schema:load` keep their behaviour; each migration runs in a navree-db
  transaction (SQLite and PostgreSQL by default, MySQL when it asks), its
  `navree_migrations` row in the same transaction; `--pretend` records SQL
  with `Schema::pretending` instead of SeaORM's mock connection; migrations
  run on the direct connection when there is one.

### 2.4 Entities

- The 40 entities that already derive `navree_orm::Model` drop the SeaORM
  derive, `ActiveModel` and `Relation` code; their callers move to the
  model API.
- The 20 SeaORM-only entities (navree-access 6, navree-mail 5, navree-sso 3,
  navree-ai 2, navree-flags, navree-i18n, navree-settings, evidence-app 1
  each) become navree-orm models where the crate may depend on navree-orm,
  and navree-db statements where it may not: navree-mail is L3, below the
  ORM; navree-access and navree-validation are optional dependencies of
  navree-orm, so they cannot depend on it. Same-layer crates whose
  dependency-graph row lacks navree-orm (navree-schedule, navree-session)
  either gain the edge (no cycle: the ORM does not depend on them) or use
  statements; the porting step decides per crate.

### 2.5 SQL fragments across crates

sea-query's `Condition` / `SimpleExpr` is today the currency for SQL filters
built outside the ORM (navree-access compiles abilities to it,
navree-time builds zone expressions with it, navree-helm takes `Select<E>`
closures). They move to navree-orm's render tree where the crate may use
navree-orm, and to `Statement::portable` fragments (SQL with `?` and its
values) where it may not; `navree_orm::Filter::raw` accepts such a fragment,
replacing `Where::Sql`.

### 2.6 MongoDB

navree-mongo runs the ORM's IR through its `DocumentDriver`; it does not use
SeaORM. Its one dependency on sea-query is the value that carries an
`ObjectId` (`sea_query::Value::Enum`). When navree-orm's `Value` becomes
navree-db's, navree-db's `Value` gains an enum variant (`Enum { type_name,
value }`, bound as text on SQL backends, cast to the named type on
PostgreSQL), and navree-mongo moves to it in the same change.

## 3. Public API changes

| SeaORM item today | Replacement |
|---|---|
| `navree_db::sea_orm`, `navree_db::sea_orm_migration` (re-exports) | removed; `navree_db::{Backend, Value, Statement, Row, Executor, ExecutorExt, Transaction, TxOptions, Pool, DbError, ErrorKind, FromValue}` |
| `DatabaseConnection` (`Db::conn`, `Db::read_conn`) | `Db` as an `Executor`, `Db::pool()`, `Db::read_pool()` |
| `DatabaseTransaction` (`Uow::tx`, `Uow::open_tx`, `Reader::Transaction`) | `Uow` as an `Executor`; `Uow::savepoint()`; `Transaction` from `Db::begin` |
| `ConnectionTrait` (`execute_raw`, `query_one_raw`, `query_all_raw`, `execute_unprepared`) | `Executor::{execute, fetch_optional, fetch_all, execute_script}` |
| `TransactionTrait::begin`, `begin_with_options` | `Db::begin`, `Db::begin_with(TxOptions)`, `Transaction::savepoint` |
| `StreamTrait::stream_raw` | `Executor::fetch` (`RowStream`) |
| `Statement::from_sql_and_values`, `from_string` | `Statement::new(backend, sql, values)`, `Statement::portable(sql, values)`, `Statement::text(sql)` |
| `QueryResult`, `try_get`, `try_get_by_index` | `Row::get::<T>(name or index)`, `Row::value`, `FromValue` |
| `ExecResult` (`rows_affected`, `last_insert_id`) | `Executed { rows_affected, last_insert_id }` |
| `sea_orm::Value` / `sea_query::Value` (also `navree_orm::Value`) | `navree_db::Value` (`navree_orm::Value` re-exports it) |
| `DbErr`, `SqlErr`, `RuntimeErr` | `DbError` with `ErrorKind` (`is_unique_violation`, `is_foreign_key_violation`, `is_not_found`, `is_retryable`, `code`, `constraint`) |
| `DbBackend` (`Db::backend`) | `Backend` (`Executor::backend`) |
| `map_db_error(&DbErr)` | `map_db_error(&DbError)` |
| `LoggedQuery { values: Vec<sea_orm::Value>, backend: DbBackend }` | `values: Vec<Value>`, `backend: Backend` |
| `ChangeObserver::observe(&DatabaseTransaction, ..)` | `ChangeObserver::observe_with(&Uow, ..)` (renamed `observe` once the old one goes) |
| `Repo<E>` | navree-orm model API (`Model::create`, `record.save(&uow)`, `delete`), which records the same change events |
| `paginate(Select<E>, ..)`, `cursor_paginate` | `Builder::paginate`, `cursor_paginate` (same `Page` shape) |
| `seed::Factory { type Entity }`, `create`, `create_many` | navree-orm factories |
| `contains_ignore_case` / `eq_ignore_case(DbBackend, Expr) -> Expr` | the same helpers returning a render-tree expression (and a `Statement` fragment) |
| `prune_time_series<C: ConnectionTrait>` | `prune_time_series(&impl Executor, ..)` |
| `EntityTrait`, `ActiveModel`, `DeriveEntityModel`, `Select<E>`, `ColumnTrait`, `QueryFilter`, `Related` | `#[derive(Model)]`, `Builder<M>`, `Record<M>`, typed columns, relations |
| `sea_query::{Query, Expr, Condition, Func, Alias, ..}` | `navree_orm::render` tree, `navree_orm::table(..)`, `Statement::portable` |
| `MigrationTrait`, `SchemaManager`, `MigrationName` | `navree_schema::{Migration, Schema}`, `migration!`, `SchemaMigration::new` |
| `MigrationSet::migration(impl MigrationTrait)` | `MigrationSet::migration(SchemaMigration::new(name, m))` / `migration!` |
| navree-schema `SchemaConnection` (`DbBackend`, `Statement`, `QueryResult`, `DbErr`) | the same trait on navree-schema's `Dialect`, `SchemaRow`, `SchemaError` |
| `navree::db::sea_orm` (facade path) | `navree::db` items above |

Crate APIs that hand SeaORM types to other crates and so fix the order of
the port: navree-auth's `User` / `user::Entity` (10+ crates), navree-workspace's
entities and `WorkspaceScoped: EntityTrait` scoped repositories,
navree-access's `Accessible::apply(Select<E>)` and `SubjectRecords` for
`E: EntityTrait`, navree-search's `Searchable: EntityTrait` and
`SearchContext(Arc<DatabaseTransaction>)`, navree-helm's `Resource { type
Entity }`, navree-media's `MediaStore(&DatabaseTransaction)`,
navree-queue's `push_observed(&DatabaseTransaction)`, navree-notify's
`StoredNotifications<C: ConnectionTrait>`, navree-validation's
`passes<C: ConnectionTrait>`, navree-time's `ZoneSql { backend: DbBackend }`,
navree-webhooks' `WebhookContext::conn()`, and the 25 error types that wrap
`DbErr`.

## 4. Porting plan

Each step merges on its own with every test green; a crate's port changes
its migrations (same names), entities, stores and public types together.
Sizes are the survey's counts; effort is for one agent with the real test
servers. Models: **Sonnet** for mechanical ports (migrations to the schema
builder, dual-derived entities, raw-SQL stores with clear tests), **Opus**
where the design moves (shared APIs, the ORM core, navree-db, the
migrator, code generators).

| Step | What | Size | Effort | Model |
|---|---|---|---|---|
| 0 | **Foundation** (this branch): navree-db's engine on SQLx, units of work on either transaction, the bridge; navree-orm's renderer, lowering, snapshot and differential tests | done | done | Opus |
| 1 | **navree-schema off SeaORM**: `SchemaConnection` on its own types, implemented by navree-db's executors; `MigrationSet` holding navree-schema migrations; the migrator on navree-db transactions; `--pretend` through `Schema::pretending`. The `MigrationTrait` path keeps running until step 6 ends | navree-schema 13.5k LOC (18 SeaORM lines), navree-db migrate 1.9k | 1 day | Opus |
| 2 | **navree-orm SQL switch**: `compile_*` → `render_*`, delete the sea-query compiler, review the 30 changed snapshots, port navree-orm's `access.rs` to render-tree conditions (`Where::Sql` goes with navree-access in step 5) | 10 files, 198 builder lines | half a day | Opus |
| 3 | **navree-orm on navree-db's engine**: `Value` → navree-db's (+ `Enum` variant, navree-mongo in lockstep), `Row` on `navree_db::Row`, `Field::from_sql`, `exec.rs` on `Executor` (pools, units of work, savepoints, streams, timeouts, lost-connection retries), `OrmError::Db(DbError)`, pgvector values | 19 files, 213 `Value` lines, 39 `QueryResult` lines; navree-mongo 3 lines | 2 days | Opus |
| 4 | **Wave 0-2 leaf crates** (topological order of the survey): navree-time (zone expressions), navree-session (store, 1 migration), navree-schedule (store, 1 migration), navree-validation (`unique`/`exists` on statements), navree-mail (5 entities, 1 migration, outbox and delivery log; L3: statements), navree-micro (outbox/inbox store, 1 migration) | 3.5k / 2.0k / 1.8k / 5.4k / 14.2k / 17.1k LOC; 4-44 SeaORM lines each | 2 days | Sonnet (navree-mail: Opus for the entity-to-statement design) |
| 5 | **Shared APIs**: navree-access (6 entities, abilities to fragments, `SubjectRecords`, `Accessible::apply`), navree-auth (5 dual entities, 6 migrations, `User`), navree-audit (`ChangeObserver`, hypertable), navree-workspace (9 dual entities, 5 + 9 migrations, scoped repositories, row-level security settings through `Uow::set_local`) | access 5.3k, auth 13.4k, audit 1.8k, workspace 18.7k LOC; 46 / 31 / 18 / 97 SeaORM lines | 3 days | Opus |
| 6 | **Wave 4-5 crates on the ported APIs**: navree-queue (raw-SQL stores, 4 migrations), navree-notify, navree-oauth (8 dual entities), navree-billing (5 dual), navree-flags, navree-settings, navree-social, navree-sso (3 entities), navree-webhooks (4 dual, hypertables), navree-workflows (raw-SQL stores), navree-fleet, navree-i18n, navree-ai (2 entities, pgvector), navree-integrations, navree-media (`MediaStore`), navree-logbook (observer only), navree-graphql / navree-grpc / navree-mcp (tests only) | 17 crates, 1-40 SeaORM lines each in `src/` | 4-5 days, in parallel worktrees | Sonnet; Opus for navree-search (pgvector, full text, `Searchable` derive) |
| 7 | **Wave 6-7 and apps**: navree-search, navree-helm (admin panel generic over entities → over models), navree-lighthouse (5 dual entities, 142 entity-API lines), navree-sheets (raw-SQL store), navree-directory, navree-domains, evidence-app, crm-* (2 call sites) | helm 10.4k, lighthouse 11.4k, search 10.2k, sheets 9.0k LOC | 3 days | Opus for navree-helm and navree-search, Sonnet for the rest |
| 8 | **navree-db's last SeaORM surface**: units of work on navree-db's own transaction only, `Uow::tx` and the bridge removed, `ChangeObserver::observe_with` → `observe`, `Repo` / `paginate` / `Factory` / `Reader` / text helpers / `LoggedQuery` / `map_db_error` on navree-db types, the `MigrationTrait` path and the mock-based tests replaced by the real-server ones, re-exports removed | navree-db 80 SeaORM lines in `src/`, 87 in tests | 1-2 days | Opus |
| 9 | **Generators and docs**: navree-cli templates (6 entity templates, 2 migration templates, 4 `Cargo.toml.j2`), `make resource`, navree-macros' `Searchable` derive (reads `#[sea_orm]`), docs site (148 lines in 44 pages), skills | 25 template files, 2 macro files | 1 day | Sonnet (templates, docs), Opus (derive) |
| 10 | **Remove the dependencies**: sea-orm, sea-orm-migration, sea-query from the workspace; Cargo.lock, deny and vet updated; the dependency-graph test refuses any `sea-*` crate | workspace manifest | an hour | Sonnet |
| 11 | **SQLite opt-in** (§5) | manifests of every crate with SQLite tests | half a day | Sonnet |

Per-crate detail, in the order the survey derived from the dependency graph
(SeaORM migrations / entities / SeaORM lines in `src/`):

| Wave | Crates |
|---|---|
| 0 | navree-schema (0 / 0 / 18), navree-time (0 / 0 / 4), navree-cli (templates), navree-macros (derive) |
| 1 | navree-db (0 + 1 test / 0 / 80) |
| 2 | navree-access (1 / 6 / 46), navree-mail (1 / 5 / 44), navree-schedule (1 / 0 / 6), navree-session (1 / 0 / 4), navree-validation (0 / 0 / 2) |
| 3 | navree-orm (0 / 0 / 193), navree-micro (1 / 0 / 4), navree-grpc (tests) |
| 4 | navree-audit (2 / 1 / 18), navree-auth (6 / 5 / 31), navree-mongo (0 / 0 / 3), navree-notify (1 / 1 / 26), navree-queue (4 / 0 / 10), navree-workspace (5 / 9 / 97) |
| 5 | navree-ai (1 / 2 / 14), navree-billing (1 / 5 / 27), navree-flags (1 / 2 / 17), navree-fleet (1 / 0 / 6), navree-graphql (tests), navree-i18n (1 / 1 / 11), navree-integrations (0 / 0 / 2), navree-logbook (observer), navree-mcp (tests), navree-media (0 / 0 / 7), navree-oauth (1 / 8 / 40), navree-search (1 / 0 / 27), navree-settings (1 / 1 / 15), navree-social (1 / 1 / 13), navree-sso (1 / 3 / 19), navree-webhooks (1 / 4 / 36), navree-workflows (2 / 0 / 7) |
| 6 | navree-directory (0 / 0 / 2), navree-helm (0 / 0 / 17), navree-lighthouse (1 / 5 / 43), navree-sheets (1 / 0 / 8) |
| 7-9 | navree-domains (0 / 0 / 1), navree facade (tests and the `db::sea_orm` path), evidence-app (1 / 1 / 10) |

Total: about 20 agent-days; with steps 4, 6 and 7 split across parallel
worktrees (one crate each, merged one at a time), about 8-10 calendar days.

## 5. SQLite as an opt-in feature

Today SQLite is compiled into every build: the workspace's `sqlx`
dependency turns on `sqlite-bundled` (the SQLite amalgamation, 168k lines of
C, 84% of the C compiled into a Linux build), and SeaORM's workspace
features turn on `sqlx-sqlite`.

On this branch navree-db has a `sqlite` feature (in its `default` for now);
its SQLite driver code, connection options and row reading are behind it,
and a SQLite URL without it fails with "enable the `sqlite` feature of
navree-db". `cargo clippy -p navree-db --no-default-features --features
postgres` (and `mysql`, and none) builds clean.

The flip (step 11, after SeaORM is gone, since SeaORM's features would keep
SQLite in):

1. The workspace `sqlx` dependency drops `sqlite-bundled`; navree-db's
   `sqlite` feature is the one place that turns it on.
2. navree-db's `default` becomes empty; the facade gets `sqlite = ["db",
   "navree-db/sqlite"]` beside `postgres` and `mysql`, outside `full`'s
   required set (the facade's `full` keeps all three so the workspace build
   still tests everything).
3. Crates whose tests run on SQLite turn it on in their dev-dependencies
   (`navree-db = { workspace = true, features = ["sqlite"] }`), and
   navree-testing's test databases ask for it; navree-logbook's own SQLite
   store gets a feature of its own.
4. `navree new` writes a TimescaleDB URL and the `postgres` feature; the
   SQLite quick start adds `sqlite`. `DbOptions`' default URL stays the
   SQLite file, so an app without the feature gets the message above.
5. The dependency-graph test checks that `navree` with `postgres` and
   without `sqlite` has no `libsqlite3-sys`.

## 6. Risks and how the foundation answers them

- **Two transactions in one request.** Avoided by construction: a unit of
  work has one transaction, SeaORM's or navree-db's, and both kinds of code
  run in it (§2.1).
- **Value binding drift between SeaORM and navree-db.** navree-db binds
  each kind the way SeaORM did on each backend (UUIDs as 16 bytes on MySQL
  and SQLite, decimals as text on SQLite, typed NULLs on PostgreSQL), so
  rows written by either are read by both; the engine tests round-trip
  every kind on all three servers.
- **Dropped transactions.** SQLx rolls back nothing by itself for a
  connection taken out of a pool; navree-db queues one rollback per open
  level and lets later handles finish it (tested: dropped transactions,
  dropped savepoints, streams dropped half way, on all three servers).
- **Error classification.** MySQL's SQLSTATEs are too coarse, so its error
  number is the code; constraint kinds come from the driver first, codes
  second; navree-db errors convert to the `DbErr` SeaORM would have
  returned, so `map_db_error` answers the same 404/409/500.
- **SQL text changes.** The renderer's output differs from sea-query's in
  text only, where the differential tests show the same rows; the snapshot
  changes are reviewed in step 2.
