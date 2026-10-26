# The Navree ORM

Date: 2026-10-26. Status: the user's direction; the builder's calls are marked.

## 1. What the user asked for

- A full ORM, as capable and as pleasant as the most expressive ORMs ever
  built: active-record models, a fluent query builder, every kind of
  relationship with eager loading, scopes, soft deletes, casts, accessors
  and mutators, model events and observers, collections, serialization,
  factories, pagination, chunking and streaming.
- Build whatever it needs around it and **use it everywhere**: it becomes
  the way Navree apps and Navree's own crates talk to the database.
- The reference for behaviour is the PHP source in
  `target/reference/laravel-framework/src/Illuminate/Database/` (Eloquent:
  `Eloquent/Model.php`, `Eloquent/Builder.php`, `Eloquent/Concerns/*`,
  `Eloquent/Relations/*`, `Eloquent/Casts`, `Eloquent/Factories`,
  `Query/Builder.php`, `Query/Grammars/*`). Read it to get the semantics
  and edge cases right. **Never mention it** in code, comments, docs, skills
  or commit messages (site rule: Navree is described on its own terms).

## 2. Builder's calls

- **Crate `navree-orm`** (L4: depends on navree-db, navree-events,
  navree-crypto, navree-contracts, navree-kernel; macros in
  `navree-macros` behind the `orm` part, or a `navree-orm-macros` crate if
  the macro crate's layer rules make that cleaner). Facade feature `orm`,
  in `full`, re-exported as `navree::orm` with the common items in the
  prelude.
- **Built on what navree-db already owns.** Connections, the pool, the unit
  of work (`Uow`: transaction, actor, request id, outbox, `set_config` for
  audit and workspace RLS), migrations and the existing repository API stay
  in navree-db. The ORM executes through that connection and inside the
  `Uow` when one is given, so audit, workspace row-level security and the
  outbox keep working. SQL is generated with `sea-query` (already a
  workspace dependency) for SQLite and PostgreSQL, rows decoded by name.
- **Rust-shaped, not a port.** Typed where it pays (columns as generated
  constants, typed keys, typed relations, `Result` everywhere, async),
  string-flexible where expressiveness matters (relation paths like
  `"company.owner"` for eager loading, raw expressions). No runtime
  reflection; derive macros generate the metadata.
- **Async and explicit about the connection.** Queries run on `&Db` or
  `&Uow` (`Deal::query().on(&uow)` / `.get(&db)` — the core agent picks one
  consistent style and documents it).
- **The existing SeaORM entities keep working.** The CRM and crates move to
  the ORM in the adoption phase; nothing breaks in between.

## 3. Feature checklist (definition of done)

Models
- `#[derive(Model)]` on a plain struct; `#[model(table = "deals")]`;
  primary key (integer, UUID, ULID, custom name), `timestamps` (created_at,
  updated_at, custom names, off), `soft_deletes`, `connection`,
  `per_page`, `touches = [...]`.
- Generated column constants and a typed column API; mass assignment
  (`fillable` / `guarded` attributes, `fill`, `force_fill`), `create`,
  `save`, `update`, `delete`, `refresh`, `fresh`, `replicate`, `touch`,
  `increment` / `decrement`, `first_or_create`, `first_or_new`,
  `update_or_create`, `upsert`, `insert`, `insert_or_ignore`,
  `delete_where`, `destroy(ids)`.
- Dirty tracking: `is_dirty`, `is_clean`, `was_changed`, `get_original`,
  `get_changes`, `get_dirty`.
- Attribute casts: bool, integers, decimal, float, string, date, datetime
  (UTC and tz-aware), timestamp, JSON (`serde_json::Value` or any serde
  type), arrays and maps, enums (string and integer backed), encrypted
  (navree-crypto) and encrypted JSON, hashed (Argon2), and custom casts via
  a `Cast` trait (with inbound-only casts).
- Accessors and mutators, computed/appended attributes, `hidden`,
  `visible`, `appends`, `to_json` / serde serialization honoring them, and
  integration with API resources (`navree-api`).

Query builder
- `where_` / `or_where` with operators, nested groups (closures), `where_in`,
  `where_not_in`, `where_null`, `where_not_null`, `where_between`,
  `where_column`, `where_date` / `month` / `year` / `time`, `where_like`,
  `where_json_contains` / JSON paths, `where_exists`, `where_raw`,
  `when` (conditional clauses), full-text where on PostgreSQL.
- `select`, `add_select`, subquery selects, `distinct`, joins (inner, left,
  right, cross, sub-joins, join clauses), unions, `group_by`, `having`,
  `order_by`, `latest`, `oldest`, `in_random_order`, `reorder`, `limit`,
  `offset`, `take`, `skip`, `for_page`, locking (`lock_for_update`,
  `shared_lock`), raw expressions and bindings everywhere.
- Retrieval: `get`, `first`, `first_or_fail` (404 through the error shape),
  `find`, `find_many`, `find_or_fail`, `sole`, `value`, `pluck` (with keys),
  `exists`, `doesnt_exist`, aggregates (`count`, `sum`, `avg`, `min`,
  `max`), `paginate` (Navree's page shape), `simple_paginate`,
  `cursor_paginate`, `chunk`, `chunk_by_id`, `each`, `lazy`, `lazy_by_id`,
  `cursor` (async stream).
- Local scopes (`#[scope]` methods callable on the builder), global scopes
  (trait + `without_global_scope(s)`), dynamic `when`.

Relationships
- has one, has many, belongs to, belongs to many (pivot table, pivot
  columns, pivot timestamps, custom pivot models, `as`), has one through,
  has many through, polymorphic one/many (`morph_one`, `morph_many`,
  `morph_to`), many-to-many polymorphic (`morph_to_many`,
  `morphed_by_many`), a morph map, `latest_of_many` / `oldest_of_many` /
  `of_many`, default models (`with_default`).
- Querying relations as builders (`deal.activities().where_(...)`),
  existence (`has`, `or_has`, `doesnt_have`, `where_has`,
  `where_doesnt_have`, `where_relation`, `where_belongs_to`), nested paths.
- Eager loading: `with("company.owner")`, constrained eager loads
  (closures), `with_count`, `with_sum` / `with_avg` / `with_min` /
  `with_max` / `with_exists`, lazy eager loading on models and collections
  (`load`, `load_missing`, `load_count`), automatic N+1 prevention option
  (strict mode that errors on lazy loading).
- Writing relations: `save`, `save_many`, `create`, `create_many`,
  `associate`, `dissociate`, `attach`, `detach`, `sync`,
  `sync_without_detaching`, `toggle`, `update_existing_pivot`, touching
  parents.

Lifecycle
- Model events: retrieved, creating, created, updating, updated, saving,
  saved, deleting, deleted, trashed, force_deleting, force_deleted,
  restoring, restored, replicating; cancellable "-ing" events; observers;
  `without_events`; dispatch through navree-events after commit when inside
  a `Uow`.
- Soft deletes: `with_trashed`, `only_trashed`, `restore`, `force_delete`,
  `trashed()`.
- Pruning: `Prunable` / `MassPrunable` models and the `model:prune` command.
- Strict modes: prevent lazy loading, prevent silently discarding
  attributes, prevent accessing missing attributes.

Collections
- `Collection<M>`: iteration, `map`, `filter`, `find`, `contains`,
  `key_by`, `group_by`, `pluck`, `model_keys`, `diff`, `intersect`,
  `unique`, `fresh`, `load`, `load_count`, `to_query`, serialization.

Integration
- Route model binding: a `Model<T>` (or `Bound<T>`) extractor that loads by
  route key (custom key, soft-delete aware, scoped to the current
  workspace) and returns 404 in the error shape.
- Workspaces: a global scope / trait that scopes models to the current
  workspace and fills `workspace_id` on create.
- Access control: `Accessible`/policies work with ORM models; SQL filters
  from navree-access apply to ORM queries.
- Factories: `Model::factory()` with states, sequences, `count`, `has` /
  `for` relationships, `recycle`, `make` vs `create`, afterMaking /
  afterCreating hooks; seeders; testing assertions
  (`assert_database_has`, `assert_database_missing`, `assert_model_exists`,
  `assert_soft_deleted`, `assert_database_count`).
- OpenAPI: models used as response schemas (respecting hidden/appends).
- CLI: `navree make:model <module> <Name> [--migration --factory --seeder --controller --policy]`.

## 3a. Databases, migrations and TimescaleDB (user direction, 2026-10-26 evening)

- **TimescaleDB is the first and default database.** `navree new` and the
  starter kits generate a PostgreSQL URL for a TimescaleDB server, Dock's
  default database service runs a TimescaleDB image, and the docs lead with
  it. Plain PostgreSQL, **MySQL/MariaDB** and SQLite (tests, quick starts)
  are fully supported. **MongoDB** is supported as a document driver under
  the same model API.
- **One driver abstraction.** The ORM core defines it: SQL dialects
  (PostgreSQL/TimescaleDB, MySQL, SQLite) through sea-query grammars, and a
  document-driver interface that MongoDB implements (filters, sort,
  projection, aggregation pipelines, `$lookup` or batched `$in` loads for
  relations, embedded documents through casts, sessions for transactions).
- **Migrations and the schema builder** (crate `navree-schema`, L3 or L4 as
  the layer rules allow), as expressive as the best migration systems:
  `Schema::create("deals", |t| { t.id(); t.foreign_id("company_id").constrained().cascade_on_delete(); t.string("title"); t.decimal("amount", 12, 2); t.timestamps(); t.soft_deletes(); })`,
  `Schema::table` (alter: add, change, rename, drop columns; indexes;
  foreign keys), `rename`, `drop`, `drop_if_exists`, `has_table`,
  `has_column`, every column type (big/small/tiny ints, unsigned, decimal,
  float, string/char/text sizes, boolean, date/time/datetime/timestamp(tz),
  json/jsonb, uuid, ulid, enum, binary, ip/mac, geometry where the database
  has it, generated/virtual columns), modifiers (nullable, default,
  use_current, unique, index, comment, after/first on MySQL, collation),
  `id()`, `uuid_primary()`, `ulid_primary()`, `foreign_id` /
  `foreign_uuid` / `constrained` / `cascade_on_*`, `morphs`,
  `nullable_morphs`, `timestamps`, `timestamps_tz`, `soft_deletes`,
  `remember_token`-style helpers where they fit Navree's auth. Migration
  files are Rust (`up` and `down`), discovered per module, recorded in a
  migrations table with batches; commands `navree make:migration`,
  `migrate`, `migrate:rollback [--step N]`, `migrate:reset`,
  `migrate:refresh`, `migrate:fresh [--seed]`, `migrate:status`,
  `--pretend` (print SQL), `schema:dump` (squash). It must keep running the
  existing `MigrationSet`s of the framework crates (adapt them, don't break
  them).
- **TimescaleDB helpers in migrations**, so developers decide in code which
  tables become hypertables and how:
  - `Schema::create_hypertable("events", "occurred_at", |t| {...})` or
    `t.hypertable("occurred_at")` inside `create`, and
    `Schema::convert_to_hypertable("audit_log", Hypertable::on("created_at").chunk_interval("1 day").partition_by("workspace_id", 4).migrate_data())`
    for existing tables.
  - **Primary keys and unique indexes:** a hypertable's unique constraints
    must include the time column. The helper checks the table and either
    rebuilds the primary key as `(id, time_column)` or explains the fix;
    the docs recommend time-ordered ids (UUIDv7 / ULID) for time-series
    tables and show both strategies.
  - Chunk interval (`chunk_interval`), space partitioning
    (`partition_by(column, partitions)`), compression
    (`compress().segment_by(...).order_by(...).after("7 days")`),
    retention (`retain_for("90 days")`), reorder policies, continuous
    aggregates (`Schema::create_continuous_aggregate(name, query, refresh policy)`)
    and the query builder's `time_bucket`, `first`/`last` helpers.
  - **Engines per table**: a table declares its storage engine and the
    helper picks the right DDL per database: a hypertable on TimescaleDB,
    a regular table with the right indexes on plain PostgreSQL/SQLite
    (with a clear notice), `ENGINE=InnoDB`-style options on MySQL, a native
    time-series collection on MongoDB. One migration runs everywhere.
  - A guide (docs) on which tables to convert: append-mostly, time-keyed,
    large tables (logs, events, audit, analytics, metrics, notification and
    delivery history, job runs) versus regular tables (users, workspaces,
    business records); chunk sizing; ids. Navree's own time-series tables
    (audit log, telemetry and Lighthouse events, webhook deliveries, job
    history) use these helpers and become hypertables on TimescaleDB.
- **MySQL in navree-db:** connections, the unit of work (session variables
  where PostgreSQL uses `set_config`; workspace isolation by query scoping
  since MySQL has no row-level security), pagination, repositories,
  migrations, all behind a `mysql` feature.
- Without database servers on this machine (Docker doesn't respond), SQL is
  verified by generation snapshots per dialect, and integration tests for
  PostgreSQL/TimescaleDB, MySQL and MongoDB are gated behind env vars
  (`NAVREE_TEST_POSTGRES_URL`, `NAVREE_TEST_MYSQL_URL`,
  `NAVREE_TEST_MONGODB_URL`) so CI with services runs them.

## 4. Phases and workers

1. **Core** (one worker, the critical path): the crate, `DESIGN.md` inside
   it (architecture and extension points for every later phase), the
   derive macro, connection/Uow execution, the query builder, retrieval,
   CRUD, dirty tracking, timestamps, the basic casts, collections,
   pagination, scopes plumbing, and stubs/traits where later phases plug
   in. Merged before phase 2 starts.
2. **Features** (in parallel, isolated worktrees, merged one at a time):
   - `orm-relations`: every relationship type, relation builders,
     existence queries, eager and lazy eager loading, aggregates, writing
     relations, pivots, morph map, strict lazy-loading mode.
   - `orm-lifecycle`: events and observers, soft deletes, global scopes,
     pruning and `model:prune`, strict modes, `without_events`.
   - `orm-attributes`: mass assignment, all casts incl. encrypted/hashed/
     custom, accessors/mutators, hidden/visible/appends, serialization,
     API resources and OpenAPI integration.
   - `orm-streams`: chunk/each/lazy/cursor streams, upsert and bulk writes,
     locking, joins/unions/subqueries completeness, PostgreSQL-specific
     JSON and full-text clauses, multiple connections.
   - `orm-factories`: factories, seeders, testing assertions, route model
     binding, workspace scoping trait, `make:model`.
3. **Docs and adoption** (after phase 2):
   - `orm-docs`: a new docs section "ORM" (getting started, models,
     relationships, collections, casts and accessors, serialization,
     factories, events, query builder) written from the code, plus
     `skills/navree-orm`; update database pages and the CRM tour.
   - `orm-adopt-crm`: port `apps/crm-api` to the ORM (behaviour and API
     contract identical; all CRM tests green).
   - `orm-adopt-crates`: move framework crates whose data access is
     model-shaped (workspace, auth tables, audit reads, notifications,
     queue failed jobs, the new ecosystem crates as they merge) onto the
     ORM where it simplifies them, without changing behaviour.

Every worker: TDD, SQLite and (when Docker answers) PostgreSQL, clippy
`-D warnings`, fmt, the dependency-graph test, and commit early and often
(`wip/<key>` branches) so a session limit never loses work.
