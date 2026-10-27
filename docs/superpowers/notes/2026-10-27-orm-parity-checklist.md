# Navree ORM: parity checklist

Date: 2026-10-27. Scope: `navree-orm`, `navree-orm-macros`, `navree-orm-mongodb`, `navree-schema`, and the parts of `navree-db` they build on. Brief: `docs/superpowers/specs/2026-10-26-navree-orm-design.md`. Architecture: `crates/navree-orm/DESIGN.md` and `crates/navree-schema/DESIGN.md`.

This file is the definition of done for the Navree ORM and its database layer. Each line names one capability, the API it has in Navree, the track that owns it, and notes on the Rust design or on what replaces a behaviour that does not carry over. Every ORM track works through its lines; a final audit checks every line.

## How to use this file

- A line is done when the capability is implemented and tested: on SQLite always; with a per-dialect SQL snapshot wherever the SQL differs between databases; and against real servers through the gated suites (`NAVREE_TEST_POSTGRES_URL` for PostgreSQL with TimescaleDB, `NAVREE_TEST_MYSQL_URL`, `NAVREE_TEST_MONGODB_URL`), sourced from `target/test-databases.env` when that file exists.
- A line whose API reads `n/a →` describes a behaviour that does not carry over to Rust or to Navree as it is. The replacement after the arrow is the deliverable: implement it (or confirm it exists), test it, document it, then tick the line. Notes that call one form of a capability n/a name its replacement the same way, and the replacement is tested with the line.
- Tick a line (`- [x]`) in the commit that lands it and append ` — Tests: <path>::<test>` so the audit can find the proof.
- Names are the target API. A track that settles on a different name updates the line, and its DESIGN.md, in the same commit. Names already merged in `navree-orm` or `navree-schema` win where they differ from this file.
- Line format: `- [ ] **ID** Capability. — API: names — Track: owner — Notes: details`. IDs are stable; a new line takes the next free number of its area.
- `exec` is any executor: `&Db`, `&Uow`, a connection or a transaction.

## Tracks

| Track | Owns |
|---|---|
| `orm-core` | Crate wiring, `#[derive(Model)]`, records, CRUD, dirty tracking, timestamps, the basic casts, the query builder and its IR, retrieval, aggregates, pagination, collections, scope plumbing, raw statements, transactions and the unit-of-work integration, errors, typed columns, SQLite and PostgreSQL dialect basics, `time_bucket`, `first` and `last` |
| `orm-relations` | Every relation type, relation builders, pivots, writing relations, existence queries, eager and lazy eager loading, relation aggregates, the morph map, touching parents, the lazy-loading strict mode |
| `orm-lifecycle` | Model events and observers, soft deletes, runtime global scopes, pruning and `model:prune`, strict modes, quiet operations, broadcasting model changes, `OrmModule` and the `[orm]` options |
| `orm-attributes` | Mass assignment, the advanced casts, accessors and mutators, visibility and serialization, API resources, OpenAPI schemas, models in queued jobs |
| `orm-streams` | Chunks and streams, bulk writes (insert, upsert, insert-or-ignore), locks, unions, derived tables and sub-joins, lateral joins, PostgreSQL JSON and full-text, write dialect variants, named and read/write connections, query logs and connection hooks |
| `orm-factories` | Factories, seeders, testing assertions and test-database strategies, route model binding, workspace scoping, access-control integration, `make:model`, `make:factory`, `make:seeder`, `model:show` |
| `schema` | navree-schema: blueprints, column types, modifiers, indexes, foreign keys, introspection, schema dialects, migrations, the migration and database commands, the TimescaleDB DDL helpers |
| `mysql` | MySQL and MariaDB: connections and sessions in navree-db, the unit of work's session variables, workspace isolation without row-level security, the MySQL query dialect, real-server tests |
| `mongodb` | `navree-orm-mongodb`: the document driver, sessions, the schema builder and migrations on MongoDB, time-series collections, real-server tests |
| `timescale-default` | TimescaleDB as the default database (generators, Dock, docs), time-series query helpers beyond the core, hypertable-aware models, chunk tooling, Navree's own time-series tables, the guide, real-server tests |

The docs track (`orm-docs`) and the adoption tracks (`orm-adopt-crm`, `orm-adopt-crates`) work from this list as well but own no lines in it.

## Counts

883 lines in 50 areas; 40 of them are n/a lines whose replacement is the deliverable.

### Per area

| Section | Area | Prefix | Lines | n/a | Tracks |
|---|---|---|---:|---:|---|
| A1 | Model definition and metadata | `MOD` | 30 | 3 | orm-core 20, orm-streams 1, orm-relations 4, orm-factories 1, orm-lifecycle 3, orm-attributes 1 |
| A2 | Records: persistence and instance API | `REC` | 29 | 1 | orm-core 20, orm-attributes 1, orm-lifecycle 5, orm-streams 2, orm-relations 1 |
| A3 | Dirty tracking | `DRT` | 11 | 0 | orm-core 9, orm-attributes 2 |
| A4 | Attribute access and mass assignment | `ATR` | 12 | 1 | orm-core 4, orm-attributes 8 |
| B1 | Casts and field types | `CST` | 31 | 4 | orm-core 9, orm-attributes 22 |
| B2 | Accessors, mutators and appended values | `ACC` | 8 | 3 | orm-attributes 8 |
| B3 | Serialization and visibility | `SER` | 15 | 2 | orm-attributes 14, orm-relations 1 |
| C1 | Conditions | `QWH` | 45 | 2 | orm-core 37, mysql 2, orm-streams 6 |
| C2 | Selects, joins, grouping, ordering, limits, unions, locks | `QCL` | 33 | 1 | orm-core 23, orm-streams 8, mysql 2 |
| C3 | Retrieval and aggregates | `QRT` | 24 | 2 | orm-core 23, orm-streams 1 |
| C4 | Writes through the builder | `QWR` | 25 | 1 | orm-streams 13, orm-core 11, orm-lifecycle 1 |
| C5 | Inspection and debugging | `QDB` | 10 | 2 | orm-core 10 |
| C6 | Pagination | `PAG` | 10 | 1 | orm-core 8, orm-relations 1, orm-attributes 1 |
| C7 | Chunking and streaming | `STR` | 11 | 1 | orm-streams 10, orm-relations 1 |
| C8 | Scopes | `SCP` | 13 | 2 | orm-core 9, orm-lifecycle 3, orm-relations 1 |
| D1 | Relation definitions | `REL` | 21 | 0 | orm-relations 21 |
| D2 | Relation builders | `RLB` | 13 | 0 | orm-relations 13 |
| D3 | Many-to-many pivots | `PIV` | 13 | 0 | orm-relations 13 |
| D4 | Writing relations | `RWR` | 16 | 1 | orm-relations 16 |
| D5 | Existence queries | `EXS` | 14 | 0 | orm-relations 14 |
| D6 | Eager loading and relation aggregates | `EAG` | 17 | 0 | orm-relations 17 |
| D7 | Polymorphism and the morph map | `MRP` | 7 | 0 | orm-relations 7 |
| E1 | Model events and observers | `EVT` | 15 | 1 | orm-lifecycle 14, orm-relations 1 |
| E2 | Soft deletes | `SFT` | 12 | 0 | orm-lifecycle 10, orm-core 2 |
| E3 | Pruning | `PRN` | 6 | 0 | orm-lifecycle 6 |
| E4 | Strict modes | `STC` | 8 | 0 | orm-lifecycle 5, orm-relations 3 |
| F1 | Model collections | `COL` | 26 | 4 | orm-core 25, orm-attributes 1 |
| G1 | Factories | `FAC` | 21 | 1 | orm-factories 21 |
| G2 | Seeders | `SED` | 4 | 0 | orm-factories 4 |
| G3 | Testing | `TST` | 14 | 0 | orm-factories 13, orm-core 1 |
| G4 | Integration | `INT` | 11 | 0 | orm-factories 9, orm-attributes 1, orm-lifecycle 1 |
| H1 | Connections and raw statements | `CON` | 31 | 4 | orm-streams 23, mysql 2, orm-core 6 |
| H2 | Transactions | `TXN` | 10 | 0 | orm-core 9, mysql 1 |
| H3 | Errors | `ERR` | 12 | 0 | orm-core 11, orm-streams 1 |
| I1 | Tables and introspection | `SCH` | 25 | 1 | schema 25 |
| I2 | Column types | `SCT` | 23 | 0 | schema 23 |
| I3 | Column modifiers | `SMD` | 15 | 1 | schema 15 |
| I4 | Indexes and foreign keys | `SIX` | 18 | 0 | schema 18 |
| I5 | Schema dialect behaviour | `SGR` | 19 | 0 | schema 19 |
| I6 | Migrations | `MIG` | 26 | 1 | schema 25, mysql 1 |
| I7 | Database commands | `CMD` | 15 | 0 | schema 13, orm-streams 2 |
| J1 | PostgreSQL | `DPG` | 23 | 0 | orm-core 10, orm-streams 13 |
| J2 | MySQL and MariaDB | `DMY` | 22 | 0 | mysql 22 |
| J3 | SQLite | `DSL` | 15 | 0 | orm-core 7, orm-streams 8 |
| K1 | TimescaleDB | `TSC` | 28 | 0 | timescale-default 15, schema 11, orm-core 2 |
| K2 | MongoDB | `MNG` | 28 | 0 | mongodb 28 |
| K3 | Typed columns | `TYP` | 10 | 0 | orm-core 10 |
| K4 | Workspace scoping | `WSP` | 13 | 0 | orm-factories 11, mysql 1, schema 1 |
| K5 | Access-control filters | `ACL` | 9 | 0 | orm-factories 7, orm-relations 1, orm-attributes 1 |
| K6 | Unit of work and outbox | `UOW` | 16 | 0 | orm-core 11, mysql 1, orm-lifecycle 2, orm-streams 2 |
| | **Total** | | **883** | **40** | |

### Per track

| Track | Lines | of which n/a |
|---|---:|---:|
| `orm-core` | 277 | 17 |
| `orm-relations` | 115 | 1 |
| `orm-lifecycle` | 50 | 4 |
| `orm-attributes` | 60 | 10 |
| `orm-streams` | 90 | 3 |
| `orm-factories` | 66 | 1 |
| `schema` | 150 | 3 |
| `mysql` | 32 | 1 |
| `mongodb` | 28 | 0 |
| `timescale-default` | 15 | 0 |
| **Total** | **883** | **40** |

## Part A. Models and records

### A1. Model definition and metadata (`MOD`)

- [ ] **MOD-001** Declare a model on a plain struct; the derive writes table, key, columns, casts and flags as static metadata. — API: `#[derive(Model)]`, `Model`, `ModelMeta`, `ColumnMeta` — Track: orm-core — Notes: no runtime reflection; `Model: Clone + Send + Sync + Serialize + 'static`; generated paths resolve to `::navree_orm` or the facade (`::navree::orm`), and depending on neither is a compile error naming the dependency to add.
- [ ] **MOD-002** Table name defaults to the snake_case plural of the type and can be set explicitly. — API: `#[model(table = "deals")]`, `M::meta().table` — Track: orm-core — Notes: pluralisation covers irregular nouns and `-y`/`-s`/`-x` endings, with documented examples. Changing a model's table at runtime is n/a (metadata is static): use `navree_orm::table(name)` for dynamic tables such as archives.
- [ ] **MOD-003** Primary key column name. — API: `#[model(primary_key = "code")]`, `M::key_name()`, `M::qualified_key_name()` — Track: orm-core
- [ ] **MOD-004** Key kinds: database-assigned integers, UUID v7, UUID v4, ULID, manual; inferred from the key's Rust type when not given. — API: `#[model(key_type = "int" | "uuid_v7" | "uuid_v4" | "ulid" | "manual")]`, `KeyKind` — Track: orm-core — Notes: the "incrementing" flag and the key type are one setting (`KeyKind::Increments` versus the rest); changing key name, type or kind at runtime is n/a (static metadata).
- [ ] **MOD-005** Tables without a primary key (logs, link tables, views). — API: `#[model(primary_key = false)]` — Track: orm-core — Notes: reads, mass writes and inserts work; `save` on an existing row, `delete`, `refresh` and `fresh` return `OrmError::MissingKey`.
- [ ] **MOD-006** Non-key columns that receive a generated unique id on insert. — API: field `#[model(generate = "uuid_v7" | "uuid_v4" | "ulid")]` — Track: orm-core — Notes: also filled by `upsert`, `fill_and_insert` and factories; cleared by `replicate`.
- [ ] **MOD-007** Custom id generator for the key or any generated column. — API: `#[model(generate = path::to::generator)]` (`fn() -> T`) — Track: orm-core
- [ ] **MOD-008** Timestamps: on or off, custom column names, created-at only. — API: `#[model(timestamps)]`, `#[model(timestamps(created_at = "created_on", updated_at = false))]` — Track: orm-core
- [ ] **MOD-009** Timestamp helpers and the clock. — API: `M::created_at_column()`, `M::updated_at_column()`, `M::qualified_created_at_column()`, `M::qualified_updated_at_column()`, `M::uses_timestamps()`, `record.set_created_at(ts)`, `record.set_updated_at(ts)`, `ExecContext::now()` — Track: orm-core — Notes: values come from the app's `Clock` (`FixedClock` in tests), truncated to microseconds so stored and in-memory values agree on every database.
- [ ] **MOD-010** Suspend timestamps for every model, or for listed models, inside a scope. — API: `without_timestamps(fut)`, `without_timestamps_on::<(Deal, Company)>(fut)`, `is_ignoring_timestamps::<M>()` — Track: orm-core — Notes: task-local, so concurrent requests are unaffected; also suspends touching parents.
- [ ] **MOD-011** Soft-delete column declaration and helpers. — API: `#[model(soft_deletes)]`, `#[model(soft_deletes = "removed_at")]`, `M::deleted_at_column()`, `M::qualified_deleted_at_column()`, `M::is_soft_deletable()` — Track: orm-core
- [ ] **MOD-012** Default connection per model. — API: `#[model(connection = "analytics")]`, `M::connection_name()` — Track: orm-streams — Notes: resolved through `ExecContext::app` to the named `Db`. Moving one record to another connection at runtime is n/a: pass that connection's executor.
- [ ] **MOD-013** Page size per model. — API: `#[model(per_page = 25)]`, `M::per_page()` — Track: orm-core — Notes: changing it at runtime is n/a: pass a `PageQuery`.
- [ ] **MOD-014** Default values for new records. — API: `impl Default` on the struct, `Record::new(Deal { title, ..Default::default() })` — Track: orm-core — Notes: replaces default-attribute lists and per-instance initialisers; database defaults reach the record only for `#[model(generated)]` fields (MOD-016).
- [ ] **MOD-015** Column mapping: renamed fields, skipped fields, fields that default when a row lacks them. — API: `#[model(column = "...")]`, `#[model(skip)]`, `#[model(default)]` — Track: orm-core
- [ ] **MOD-016** Columns the database computes (defaults, triggers, generated columns, sequences) are never written and are read back after every insert and update. — API: field `#[model(generated)]` — Track: orm-core — Notes: `RETURNING` on PostgreSQL and SQLite; a select by key on MySQL and MariaDB (MariaDB inserts may use `RETURNING`); `replicate` clears them.
- [ ] **MOD-017** Default eager loads and relation counts on every query of a model. — API: `#[model(with = ["company"], with_count = ["notes"])]` — Track: orm-relations — Notes: `without(..)` and `without_eager_loads()` remove them per query.
- [ ] **MOD-018** Parents touched when a record is saved or deleted. — API: `#[model(touches = ["company"])]`, `M::touched_relations()` — Track: orm-relations
- [ ] **MOD-019** Morph name stored in polymorphic type columns. — API: `#[model(morph_name = "deal")]`, `M::morph_name()` — Track: orm-relations — Notes: defaults to the snake_case type name; renaming it needs a data migration (documented).
- [ ] **MOD-020** Route key column. — API: `#[model(route_key = "slug")]`, `M::route_key_name()`, `record.route_key()` — Track: orm-factories
- [ ] **MOD-021** Default foreign-key name of a model in relations (`company_id`). — API: `M::foreign_key_name()` — Track: orm-relations — Notes: snake_case type name, `_`, key column name.
- [ ] **MOD-022** Model-level declarations parsed by one derive: observers, global scopes, fillable/guarded, hidden/visible/appends, factory, resource. — API: `#[model(observers(..), global_scopes(..), fillable(..), guarded(..), hidden(..), visible(..), appends(..), factory = .., resource = ..)]` — Track: orm-core — Notes: each attribute is parsed by the owning track's macro module (DESIGN.md section 10); this line covers the wiring and the compile errors for unknown or misspelled attributes.
- [ ] **MOD-023** Register every model with the app so commands and tooling can find it (pruning, `model:show`, morph map, OpenAPI). — API: `ModuleBuilderOrmExt::model::<M>()`, the `OrmModule` registry — Track: orm-lifecycle — Notes: Rust cannot discover types at runtime; a module lists its models once.
- [ ] **MOD-024** Setup hooks that run once per model type (before and after setup, one per shared behaviour) and once per new instance, and resetting that setup. — API: n/a → registration in `OrmModule` at app boot (observers, scopes, listeners) and `Default` for per-instance setup — Track: orm-lifecycle — Notes: metadata is compile-time and nothing boots lazily, so there is no booted state to clear.
- [ ] **MOD-025** Custom collection type and custom query builder type per model. — API: n/a → extension traits on `Collection<Deal>` and `#[scopes]` traits on `Builder<Deal>` — Track: orm-core
- [ ] **MOD-026** Qualify columns with the model's table. — API: `Deal::TITLE.qualified()`, `M::qualify("title")`, `M::qualify_columns(&[..])` — Track: orm-core
- [ ] **MOD-027** Model metadata for tooling: table, key, columns with Rust types, nullability and casts, timestamps, soft deletes, relations, scopes, observers. — API: `M::meta()` — Track: orm-core — Notes: feeds `model:show`, OpenAPI schemas and the morph map.
- [x] **MOD-028** Storage date format per model. — API: n/a → the column type decides storage; text-stored date-times use `cast = "datetime_format(..)"` (CST-009) — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::every_cast_stores_its_form_and_reads_back, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts
- [ ] **MOD-029** Crate wiring: `navree-orm` and its macros crate, the `orm` facade feature (part of `full`), `navree::orm`, the prelude items, and rows in the dependency-graph test. — Track: orm-core
- [ ] **MOD-030** The ORM's module and options: a global `OrmModule` that registers observers, listeners, scopes, the morph map and commands, configured by the `[orm]` section (strict modes and their handlers). — API: `OrmModule`, `OrmOptions` — Track: orm-lifecycle

### A2. Records: persistence and instance API (`REC`)

- [ ] **REC-001** Build a new, unsaved record. — API: `Record::new(model)` — Track: orm-core
- [ ] **REC-002** Save: insert when new, update when it exists. — API: `record.save(exec)` — Track: orm-core — Notes: `saving`, then `creating` or `updating`; a clean existing record runs no UPDATE but still fires `saving`/`saved`; returns `Ok(false)` when a listener cancels.
- [ ] **REC-003** Create from a model value and return the record. — API: `Deal::create(exec, model)` — Track: orm-core
- [ ] **REC-004** Change fields with a closure and save. — API: `record.update(exec, |d| ..)` — Track: orm-core — Notes: `OrmError::NotPersisted` for a record that does not exist.
- [x] **REC-005** Fill from untyped attributes and save in one call. — API: `record.update_attributes(exec, attrs)` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/fill.rs::records_are_created_and_updated_from_attributes
- [ ] **REC-006** Delete a record (a soft delete on soft-deleting models). — API: `record.delete(exec)` — Track: orm-core — Notes: `deleting` (cancellable), touch parents, delete, `deleted`; `Ok(false)` when cancelled; a record that does not exist is a no-op.
- [ ] **REC-007** Destroy by keys, loading each record so its events fire; returns the count. — API: `Deal::destroy(exec, keys)` — Track: orm-core — Notes: accepts one key, a slice, or `collection.model_keys()`; an empty list runs no query.
- [ ] **REC-008** Hard-destroy by keys on soft-deleting models. — API: `Deal::force_destroy(exec, keys)` — Track: orm-lifecycle — Notes: behaves like `destroy` on models without soft deletes.
- [ ] **REC-009** Reload attributes and loaded relations in place. — API: `record.refresh(exec)` — Track: orm-core — Notes: reads through the writer, ignores global scopes, reloads loaded relations except pivots (the relation part is tested by orm-relations).
- [ ] **REC-010** Reload under a row lock. — API: `record.refresh_for_update(exec)` — Track: orm-streams
- [ ] **REC-011** Fresh copy from the database, optionally with relations. — API: `record.fresh(exec)`, `record.fresh_with(exec, ["company"])` — Track: orm-core — Notes: `Ok(None)` when the record was never stored or is gone.
- [ ] **REC-012** Clone into an unsaved record. — API: `record.replicate()`, `record.replicate_except(&[Deal::TITLE])` — Track: orm-core — Notes: drops key, timestamps, generated ids, generated columns and internal through keys; copies loaded relations; fires `replicating`.
- [ ] **REC-013** Replicate without events. — API: `record.replicate_quietly()` — Track: orm-lifecycle
- [ ] **REC-014** Identity comparison of two records. — API: `record.is(&other)`, `record.is_not(&other)` — Track: orm-core — Notes: same key, table and connection.
- [ ] **REC-015** Touch the updated-at column, or a named timestamp column. — API: `record.touch(exec)`, `record.touch_column(exec, Deal::LAST_ACTIVITY_AT)` — Track: orm-core — Notes: `Ok(false)` when the model has no timestamps.
- [ ] **REC-016** Touch without events. — API: `record.touch_quietly(exec)` — Track: orm-lifecycle
- [ ] **REC-017** Increment or decrement one column, optionally with extra assignments. — API: `record.increment(exec, Deal::VIEWS, 1)`, `record.decrement(..)`, `record.increment_with(exec, col, n, assignments)` — Track: orm-core — Notes: atomic `SET col = col + ?`; the in-memory value follows; `updated_at` set; `updating`/`updated` fire; only the touched columns are synced, other unsaved changes stay dirty; on an unsaved record it returns `NotPersisted` and never turns into a table-wide increment.
- [ ] **REC-018** Increment or decrement several columns in one statement. — API: `record.increment_each(exec, [(Deal::VIEWS, 1), (Deal::SCORE, 5)])`, `record.decrement_each(..)` — Track: orm-core
- [ ] **REC-019** Quiet increments. — API: `increment_quietly`, `decrement_quietly`, `increment_each_quietly`, `decrement_each_quietly` — Track: orm-lifecycle
- [ ] **REC-020** Create, save, update and delete without model events. — API: `Deal::create_quietly(exec, model)`, builder `create_quietly`, `save_quietly`, `update_quietly`, `delete_quietly` — Track: orm-lifecycle — Notes: a `without_events` scope around the call; change events for the audit log are still recorded.
- [ ] **REC-021** Transactional fail-fast variants of save, update and delete. — API: n/a → every write returns `Result`; run it on a `Uow` or inside `db.transaction(..)` for atomicity — Track: orm-core
- [ ] **REC-022** Insert ignoring unique conflicts, reporting whether a row was inserted. — API: `record.save_or_ignore(exec)`, `record.save_or_ignore_on(exec, &[cols])` — Track: orm-streams — Notes: refuses an existing record; generated keys come back through the dialect's insert-ignore-returning form.
- [ ] **REC-023** Save the record and every loaded relation, recursively. — API: `record.push(exec)`, `record.push_quietly(exec)` — Track: orm-relations
- [ ] **REC-024** Persistence flags. — API: `record.exists()`, `record.was_recently_created()` — Track: orm-core
- [ ] **REC-025** Key access. — API: `record.key()`, `record.set_key(k)`, typed `M::Key` — Track: orm-core
- [ ] **REC-026** Non-column values carried by a row: aggregates, `*_count`, pivot columns, selected expressions. — API: `record.extra::<T>(name)`, `record.extras()` — Track: orm-core
- [ ] **REC-027** The plain model inside a record. — API: `Deref`/`DerefMut` to `M`, `record.into_inner()` — Track: orm-core
- [ ] **REC-028** Insert flow: generate the key, set timestamps, insert, read a database key back. — API: internal (`persist.rs`) — Track: orm-core — Notes: `RETURNING` on PostgreSQL and SQLite, `last_insert_id` on MySQL (tested for real by the mysql track); `exists` is set before `created` fires so a listener can update the record.
- [ ] **REC-029** Update flow: only dirty columns, matched by the original key, so changing the key updates the stored row. — API: internal (`persist.rs`) — Track: orm-core

### A3. Dirty tracking (`DRT`)

- [ ] **DRT-001** Whether the record, or given columns, changed since the last sync. — API: `record.is_dirty()`, `record.is_dirty_on(&[cols])` — Track: orm-core
- [ ] **DRT-002** Whether the record, or given columns, are unchanged. — API: `record.is_clean()`, `record.is_clean_on(&[cols])` — Track: orm-core
- [ ] **DRT-003** Whether the last save changed the record or given columns. — API: `record.was_changed()`, `record.was_changed_on(&[cols])` — Track: orm-core
- [ ] **DRT-004** Original values: the whole model, one column, the stored representation. — API: `record.get_original()`, `record.original_value(col)`, `record.raw_original(col)` — Track: orm-core
- [ ] **DRT-005** Dirty columns with their new values. — API: `record.get_dirty()` — Track: orm-core
- [ ] **DRT-006** What the last save changed, and the values before it. — API: `record.get_changes()`, `record.get_previous()` — Track: orm-core
- [ ] **DRT-007** Sync original values and changes by hand, for all or some columns. — API: `record.sync_original()`, `record.sync_original_columns(&[cols])`, `record.sync_changes()` — Track: orm-core
- [ ] **DRT-008** Discard unsaved changes. — API: `record.discard_changes()` — Track: orm-core
- [ ] **DRT-009** Equivalence rules for dirty checks: floats within a small epsilon, decimals numerically, JSON structurally, date-times by instant, numeric strings by value, enums by stored value. — Track: orm-core — Notes: a new record is dirty in every column.
- [x] **DRT-010** Encrypted columns count as dirty while an old encryption key is still configured, so the next save re-encrypts them. — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/secrets.rs::records_read_under_a_previous_key_are_re_encrypted_on_save, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts
- [x] **DRT-011** Custom casts decide equivalence for their own values. — API: `Cast::eq_stored` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::custom_equivalence_and_rounding_decide_dirtiness

### A4. Attribute access and mass assignment (`ATR`)

- [ ] **ATR-001** Read and write fields. — API: struct fields through `Deref`/`DerefMut` — Track: orm-core — Notes: dynamic property access, array-style access and isset/unset on attributes are n/a: fields are typed.
- [ ] **ATR-002** Read or write one column by name with a dynamic value. — API: `record.value(col)`, `record.set_value(col, Value)` — Track: orm-core — Notes: used by timestamps, increments and refresh; unknown names are errors, never silently ignored.
- [ ] **ATR-003** All column values as stored, and replacing them in bulk. — API: `record.values()`, `record.set_values(values)` — Track: orm-core
- [ ] **ATR-004** Whether a model has a column. — API: `M::has_column("name")`, `M::meta().column("name")` — Track: orm-core
- [x] **ATR-005** Mass assignment from untyped attributes, honouring fillable and guarded. — API: `record.fill(attrs)`, `Attributes`, `attrs!{ "title" => "Acme" }` — Track: orm-attributes — Notes: values decode through each field's cast; only real columns can be filled, so unknown and JSON-path keys never reach SQL; typed input structs remain the recommended path. — Tests: crates/navree-orm/tests/orm/attributes/fill.rs::fill_sets_the_fillable_columns_and_leaves_the_rest, crates/navree-orm/tests/orm/attributes/fill.rs::untyped_input_is_converted_through_types_and_casts, crates/navree-orm/tests/orm/attributes/fill.rs::input_that_does_not_fit_changes_nothing, crates/navree-orm/tests/orm/attributes/fill.rs::unknown_columns_and_paths_into_plain_columns_never_reach_the_model, crates/navree-orm/tests/orm/attributes/fill.rs::request_bodies_deserialize_into_attributes, crates/navree/tests/orm_attributes.rs::the_attribute_layer_works_through_the_facade
- [x] **ATR-006** Fill ignoring guards. — API: `record.force_fill(attrs)` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/fill.rs::totally_guarded_models_refuse_fill_and_accept_force_fill
- [x] **ATR-007** Declare fillable and guarded columns and query them. — API: `#[model(fillable(title, amount))]`, `#[model(guarded(id))]`, `M::fillable()`, `M::guarded()`, `M::is_fillable(col)`, `M::is_guarded(col)`, `M::totally_guarded()` — Track: orm-attributes — Notes: a model that declares neither list is totally guarded: `fill` is an error naming the model, `force_fill` still works; `guarded(..)` alone makes every other column fillable. — Tests: crates/navree-orm/tests/orm/attributes/fill.rs::guards_are_declared_and_queried_statically, crates/navree-orm/tests/orm/attributes/fill.rs::guarded_models_fill_every_other_real_column, crates/navree-orm/tests/orm/attributes/fill.rs::totally_guarded_models_refuse_fill_and_accept_force_fill, crates/navree-orm/tests/compile_fail.rs::macro_misuse_is_reported_precisely
- [x] **ATR-008** Change fillable or guarded lists on one record at runtime. — API: n/a → `record.fill_only(attrs, &[cols])` for per-call allow-lists — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/fill.rs::fill_only_is_an_allow_list_for_one_call
- [x] **ATR-009** Turn guarding off inside a scope, or for a model. — API: `unguarded(fut)`, `is_unguarded()`, `#[model(unguarded)]` — Track: orm-attributes — Notes: task-local; process-wide on/off toggles are n/a so one request can never leak into another. — Tests: crates/navree-orm/tests/orm/attributes/fill.rs::unguarded_scopes_and_models_fill_every_real_column
- [x] **ATR-010** Fill a nested JSON value by path. — API: fill keys like `"meta->status"`, `record.set_json_path(Deal::META, "status", value)` — Track: orm-attributes — Notes: typed JSON casts are edited as Rust values; the path form serves `serde_json::Value` fields and untyped input. — Tests: crates/navree-orm/tests/orm/attributes/fill.rs::json_paths_edit_json_columns_by_value
- [x] **ATR-011** Subsets of attribute values. — API: `record.only(&[cols])`, `record.except(&[cols])` (JSON maps) — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::only_and_except_pick_values_whatever_the_visibility
- [x] **ATR-012** Create with mass assignment, with or without guards. — API: `Deal::create_from(exec, attrs)`, `Deal::force_create(exec, attrs)`, builder `create_from`, `force_create` — Track: orm-attributes — Notes: `force_create_quietly` is orm-lifecycle's `without_events` (EVT-009) around `force_create`, like the REC-020 family; the quiet form lands with that track. — Tests: crates/navree-orm/tests/orm/attributes/fill.rs::records_are_created_and_updated_from_attributes, crates/navree-orm/tests/orm/attributes/fill.rs::mutators_can_write_several_columns, crates/navree-orm/tests/orm/attributes/secrets.rs::custom_casts_read_the_apps_clock_and_configuration

## Part B. Casts, accessors and serialization

### B1. Casts and field types (`CST`)

- [ ] **CST-001** Native field types need no cast: `bool`, every integer, `f32`/`f64`, `Decimal`, `String`, `Vec<u8>`, `Uuid`, `Ulid`, `NaiveDate`, `NaiveTime`, `NaiveDateTime`, `DateTime<Utc>`, `DateTime<FixedOffset>`, `serde_json::Value`, `Option<T>`. — API: `Field` — Track: orm-core — Notes: replaces integer, real, float, double, string, boolean, date and datetime casts; booleans decode from MySQL `TINYINT(1)` and SQLite `0/1`; floats decode `Infinity`, `-Infinity` and `NaN`.
- [ ] **CST-002** Lenient decoding of loosely typed values (document rows, dynamic values): unix timestamps, date-only strings, RFC 3339 and database date-time formats, numbers stored as strings. — API: `Field::from_value` — Track: orm-core
- [x] **CST-003** Decimals with a fixed scale, rounded half away from zero on write. — API: `cast = "decimal(2)"` on `Decimal` fields — Track: orm-attributes — Notes: an unparsable value is an error naming the column (`Decode` when stored, `Cast` when given), not a panic; the scale is at most 28 (a compile error otherwise). — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::every_cast_stores_its_form_and_reads_back, crates/navree-orm/tests/orm/attributes/casts.rs::custom_equivalence_and_rounding_decide_dirtiness, crates/navree-orm/tests/orm/attributes/casts.rs::stored_values_that_do_not_fit_are_cast_errors_naming_the_column, crates/navree-orm/tests/sql/attributes.rs::conditions_bind_the_stored_form_of_cast_fields, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts, crates/navree-orm/tests/compile_fail.rs::macro_misuse_is_reported_precisely
- [ ] **CST-004** JSON for any serde type: structs, `Vec`, maps, sets, value objects. — API: `cast = "json"` (`AsJson`) — Track: orm-core — Notes: covers array, json, object and collection casts; `Option<T>` stores SQL `NULL`, never the JSON literal `null`.
- [ ] **CST-005** JSON with unescaped Unicode. — API: n/a → serde_json writes UTF-8 unescaped by default — Track: orm-core
- [ ] **CST-006** Enums stored by serde name (unit enums included). — API: `cast = "string_enum"` — Track: orm-core
- [ ] **CST-007** Integer-backed enums. — API: `cast = "int_enum"` (`Into<i64> + TryFrom<i64>`) — Track: orm-core
- [x] **CST-008** Lists and sets of enums stored as JSON arrays. — API: `cast = "json"` on `Vec<Stage>` / `BTreeSet<Stage>` — Track: orm-attributes — Notes: elements stored by serde name or integer; enum-typed array objects and enum collections need nothing else. — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::every_cast_stores_its_form_and_reads_back, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts
- [x] **CST-009** Date-times in text columns with a custom format. — API: `cast = "datetime_format(\"%Y-%m-%d %H:%M\")"` — Track: orm-attributes — Notes: also on `DateTime<FixedOffset>`, `NaiveDateTime`, `NaiveDate` and `NaiveTime`; the format is checked at compile time and used for serialized output; immutable date and date-time casts are n/a: Rust values only change through `&mut`. — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::every_cast_stores_its_form_and_reads_back, crates/navree-orm/tests/orm/attributes/casts.rs::custom_equivalence_and_rounding_decide_dirtiness, crates/navree-orm/tests/sql/attributes.rs::conditions_bind_the_stored_form_of_cast_fields, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts, crates/navree-orm/tests/compile_fail.rs::macro_misuse_is_reported_precisely
- [x] **CST-010** Unix timestamps in integer columns. — API: `cast = "unix_timestamp"` and `cast = "unix_timestamp_ms"` on `DateTime<Utc>` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::every_cast_stores_its_form_and_reads_back, crates/navree-orm/tests/orm/attributes/casts.rs::untyped_input_reaches_every_cast_in_its_plain_form, crates/navree-orm/tests/sql/attributes.rs::conditions_bind_the_stored_form_of_cast_fields, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts
- [x] **CST-011** Encrypted strings. — API: `cast = "encrypted"` — Track: orm-attributes — Notes: navree-crypto's `Encrypter` from `ExecContext::app`; decrypts with previous keys during rotation (see DRT-010). — Tests: crates/navree-orm/tests/orm/attributes/secrets.rs::encrypted_fields_are_stored_encrypted_and_read_plain, crates/navree-orm/tests/orm/attributes/secrets.rs::records_read_under_a_previous_key_are_re_encrypted_on_save, crates/navree-orm/tests/orm/attributes/secrets.rs::without_an_app_the_services_come_from_explicit_scopes, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts
- [x] **CST-012** Encrypted JSON of any serde type (lists, maps, objects, collections). — API: `cast = "encrypted_json"` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/secrets.rs::encrypted_fields_are_stored_encrypted_and_read_plain, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts
- [x] **CST-013** A different encrypter for one model. — API: `#[model(encrypter = "name")]` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/secrets.rs::a_model_can_name_its_encrypter
- [x] **CST-014** Hashed values. — API: `cast = "hashed"` — Track: orm-attributes — Notes: Argon2 via navree-crypto; a value that is already an Argon2 hash is kept when its parameters parse (a malformed Argon2 string or another algorithm's hash is an error); hashing runs when the record is saved, off the async threads; reading returns the hash. — Tests: crates/navree-orm/tests/orm/attributes/secrets.rs::hashed_fields_hold_the_hash_once_saved, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts
- [ ] **CST-015** Custom casts. — API: `trait Cast<T> { type Stored: Field; fn get(..); fn set(..) }`, `#[model(cast = path::MoneyCast)]` — Track: orm-core — Notes: a cast on an `Option<T>` field casts the inner value.
- [x] **CST-016** Casts that need the app (encrypter, hasher, clock, config). — API: `CastContext` passed to `Cast::get_with` / `Cast::set_with` — Track: orm-attributes — Notes: extends the core trait without changing built-in casts; resolved from the executor's app per operation, kept by the records it reads, `CastContext::scope` for code outside an app. — Tests: crates/navree-orm/tests/orm/attributes/secrets.rs::custom_casts_read_the_apps_clock_and_configuration, crates/navree-orm/tests/orm/attributes/secrets.rs::encrypted_fields_are_stored_encrypted_and_read_plain, crates/navree-orm/tests/orm/attributes/secrets.rs::without_an_app_the_services_come_from_explicit_scopes
- [x] **CST-017** Inbound-only casts: transform on write, read the stored value as is. — API: `InboundCast<T>` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::every_cast_stores_its_form_and_reads_back, crates/navree-orm/tests/sql/attributes.rs::conditions_bind_the_stored_form_of_cast_fields
- [x] **CST-018** Value types that pick their own cast. — API: `trait Castable { type Cast: Cast<Self>; }`, or `Field` implemented directly on a newtype — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::every_cast_stores_its_form_and_reads_back, crates/navree-orm/tests/orm/attributes/casts.rs::newtypes_that_are_fields_need_no_cast
- [x] **CST-019** Cast arguments. — API: `cast = "decimal(2)"`, `#[model(cast(path::Cast, args..))]` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::every_cast_stores_its_form_and_reads_back, crates/navree-orm/tests/compile_fail.rs::macro_misuse_is_reported_precisely
- [x] **CST-020** One field stored across several columns (money with currency, an address). — API: `MultiColumnCast<T>`, field `#[model(cast = MoneyColumns, columns(amount_cents: i64, currency: String))]` — Track: orm-attributes — Notes: each stored column still gets its typed constant for conditions. — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::fields_across_columns_track_each_column, crates/navree-orm/tests/orm/attributes/casts.rs::conditions_and_column_constants_go_through_the_casts, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts, crates/navree-orm/tests/compile_fail.rs::macro_misuse_is_reported_precisely
- [x] **CST-021** Custom serialization of a cast value in JSON output. — API: `Cast::serialize` (default: serde of `T`) — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::serialized_records_write_cast_outputs
- [x] **CST-022** Increment and decrement through a cast (money, counters in value objects). — API: `Cast::increment`, `Cast::decrement` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::increments_go_through_the_cast_and_the_database, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts
- [x] **CST-023** Caching of cast objects and opting out of it. — API: n/a → the field holds the value, so there is no cache to keep in sync — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::fields_across_columns_track_each_column
- [x] **CST-024** UUIDs and ULIDs stored as 16 bytes. — API: `cast = "binary_uuid"`, `cast = "binary_ulid"` — Track: orm-attributes — Notes: typed conditions encode through the cast; route binding parses the text form first. — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::every_cast_stores_its_form_and_reads_back, crates/navree-orm/tests/orm/attributes/casts.rs::conditions_and_column_constants_go_through_the_casts, crates/navree-orm/tests/sql/attributes.rs::conditions_bind_the_stored_form_of_cast_fields, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts
- [x] **CST-025** URLs stored as text. — API: `Field` for `url::Url` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::every_cast_stores_its_form_and_reads_back, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts
- [x] **CST-026** Vector embeddings. — API: `cast = "vector"` on `Vec<f32>` (the `Vector` cast) — Track: orm-attributes — Notes: pgvector's value on PostgreSQL with the `postgres` feature (its text form without it); `VECTOR` on MySQL 9 and MariaDB as little-endian `f32` bytes, the binary form both accept; SQLite stores the same bytes in a BLOB, the form its vector extensions read; non-finite values are refused. The real-server tests skip with a notice where the server has no vector type. — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::every_cast_stores_its_form_and_reads_back, crates/navree-orm/tests/sql/attributes.rs::vectors_take_each_databases_form, crates/navree-orm/tests/servers/main.rs::postgres_vectors, crates/navree-orm/tests/servers/main.rs::mysql_vectors
- [x] **CST-027** Fluent-object, string-helper and HTML-string casts. — API: n/a → `String`, `serde_json::Value` or a newtype implementing `Field`; templates escape output — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::newtypes_that_are_fields_need_no_cast, crates/navree-orm/tests/orm/attributes/serialize.rs::records_have_no_display_and_templates_get_their_values
- [x] **CST-028** Pluggable global JSON encoder and decoder. — API: n/a → serde attributes on the type (`#[serde(with = ..)]`) — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::records_serialize_with_serdes_keys_one_date_format_and_appends
- [x] **CST-029** Query-time casts for selected expressions and extras. — API: `builder.with_casts([("total", cast)])`, typed `record.extra::<T>()` — Track: orm-attributes — Notes: runtime cast merging on a model is n/a; per-query casts replace it. — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::query_time_casts_type_extra_columns, crates/navree-orm/tests/orm/attributes/serialize.rs::extras_and_relations_serialize_with_the_records_visibility, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts
- [ ] **CST-030** Typed conditions, updates and inserts encode values exactly as the field stores them (through its cast). — Track: orm-core
- [ ] **CST-031** Cast failures name the model and column; unknown cast names are compile errors. — API: `OrmError::Cast { model, column, message }` — Track: orm-core

### B2. Accessors, mutators and appended values (`ACC`)

- [x] **ACC-001** Computed values are plain methods on the model. — API: `impl Deal { fn weighted_amount(&self) -> Decimal }` — Track: orm-attributes — Notes: getters found by naming convention are n/a; a method is the accessor. — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::appends_change_per_record
- [x] **ACC-002** Computed values included in serialized output. — API: `#[accessors] impl Deal { #[accessor] fn weighted_amount(&self) -> Decimal }`, `#[model(appends(weighted_amount))]` — Track: orm-attributes — Notes: names checked at compile time. — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::records_serialize_with_serdes_keys_one_date_format_and_appends, crates/navree-orm/tests/orm/attributes/serialize.rs::appends_change_per_record, crates/navree-orm/tests/compile_fail.rs::macro_misuse_is_reported_precisely, crates/navree/tests/orm_attributes.rs::the_attribute_layer_works_through_the_facade
- [x] **ACC-003** Transform a stored column every time it is read. — API: n/a → a `Cast` (CST-015) or an accessor with its own name — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/casts.rs::every_cast_stores_its_form_and_reads_back, crates/navree-orm/tests/orm/attributes/serialize.rs::appends_change_per_record
- [x] **ACC-004** Transform a value when it is assigned by name (normalise an email, slugify a title). — API: field `#[model(set_with = path::normalize_email)]`, generated setter `record.set_email(v)` — Track: orm-attributes — Notes: runs for `fill`, `set_attribute` and the generated setter; `set_value` (stored values) and direct field assignment bypass it (documented). — Tests: crates/navree-orm/tests/orm/attributes/fill.rs::mutators_also_run_through_generated_setters_and_set_attribute, crates/navree/tests/orm_attributes.rs::the_attribute_layer_works_through_the_facade
- [x] **ACC-005** Mutators that write several columns. — API: `set_with` receives `&mut M` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/fill.rs::mutators_can_write_several_columns
- [x] **ACC-006** Caching an expensive computed value. — API: n/a → a `OnceLock` field marked `#[model(skip)]` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::expensive_accessors_cache_in_fields_that_are_not_columns
- [x] **ACC-007** Appended values per record. — API: `record.append(&["weighted_amount"])`, `record.set_appends(..)`, `record.merge_appends(..)`, `record.without_appends()`, `record.appends()`, `record.has_appended(name)` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::appends_change_per_record
- [x] **ACC-008** Introspection of accessors and mutators. — API: n/a → derive metadata (`M::accessors()`, `M::meta().appended_names()`, `ColumnMeta::has_mutator()`) — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::appends_change_per_record, crates/navree-orm/tests/orm/attributes/fill.rs::mutators_also_run_through_generated_setters_and_set_attribute

### B3. Serialization and visibility (`SER`)

- [x] **SER-001** Serialize a record honouring hidden, visible and appended values, loaded relations and extras. — API: `impl Serialize for Record<M>`, `record.to_json()`, `record.to_json_pretty()`, `record.to_value()` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::records_serialize_with_serdes_keys_one_date_format_and_appends, crates/navree-orm/tests/orm/attributes/serialize.rs::extras_and_relations_serialize_with_the_records_visibility
- [x] **SER-002** Hidden columns. — API: `#[model(hidden(password_hash))]`, field `#[model(hidden)]` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::records_serialize_with_serdes_keys_one_date_format_and_appends, crates/navree-orm/tests/orm/attributes/serialize.rs::extras_and_relations_serialize_with_the_records_visibility, crates/navree-orm/tests/orm/attributes/secrets.rs::encrypted_fields_are_stored_encrypted_and_read_plain, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts
- [x] **SER-003** Visible allow-list. — API: `#[model(visible(id, title))]` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::visible_lists_and_custom_date_formats_per_model
- [x] **SER-004** Change visibility per record. — API: `make_visible`, `make_visible_if`, `make_hidden`, `make_hidden_if`, `set_hidden`, `merge_hidden`, `set_visible`, `merge_visible`, `hidden()`, `visible()` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::visibility_changes_per_record
- [x] **SER-005** Loaded relations serialized under their snake_case names (`null` for an empty to-one, `[]` for an empty to-many), each related record applying its own visibility. — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::extras_and_relations_serialize_with_the_records_visibility
- [x] **SER-006** Relation counts and aggregates serialized as their columns (`activities_count`, `items_sum_amount`). — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::extras_and_relations_serialize_with_the_records_visibility
- [ ] **SER-007** Pivot values serialized under `pivot`, or under the name given with `as_`. — Track: orm-relations
- [x] **SER-008** Date-time output format (RFC 3339, UTC, microseconds) and a per-model override. — API: `#[model(serialize_dates_with = path)]` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::records_serialize_with_serdes_keys_one_date_format_and_appends, crates/navree-orm/tests/orm/attributes/serialize.rs::visible_lists_and_custom_date_formats_per_model, crates/navree-orm/tests/servers/main.rs::postgres_attribute_casts, crates/navree-orm/tests/servers/main.rs::mysql_attribute_casts
- [x] **SER-009** Guard against infinite recursion through cyclic relations. — API: n/a → relations hold owned snapshots and back-references hold a parent without its relations, so cycles cannot exist — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::extras_and_relations_serialize_with_the_records_visibility
- [x] **SER-010** Serialization failures name the model and column. — API: `OrmError::Serialize { model, column, message }` — Track: orm-attributes — Notes: `column` is an `Option`: `None` when the model as a whole fails. — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::serialization_failures_name_the_model_and_column, crates/navree-orm/src/error.rs::tests::serialize_errors_name_the_model_and_column
- [x] **SER-011** String conversion of a model, escaped for HTML. — API: n/a → no `Display` on records; templates escape — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::records_have_no_display_and_templates_get_their_values
- [x] **SER-012** Collections serialize as JSON arrays, each record applying its visibility. — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::collections_serialize_as_arrays_and_change_every_record
- [x] **SER-013** Turn a record or collection into an API resource. — API: `record.to_resource::<R>()`, `collection.to_resource_collection::<R>()`, `#[model(resource = R)]` — Track: orm-attributes — Notes: integrates `navree_api::ApiResource`, `resource_json`, the `__type` subject and `mask_fields`; resource-name guessing is n/a (explicit). — Tests: crates/navree-orm/tests/orm/attributes/resources.rs::records_become_resources_with_their_subject_type
- [x] **SER-014** OpenAPI schemas generated from models: hidden columns omitted, appended values added, casts and nullability reflected. — API: `schema_fn::<Record<Deal>>()`, `ModelSchema` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/resources.rs::model_schemas_describe_serialized_records, crates/navree-orm/tests/orm/attributes/resources.rs::the_openapi_document_carries_model_and_page_schemas
- [x] **SER-015** Serde attributes on model fields keep working for JSON output (`rename`, `skip_serializing_if`) without changing column names. — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::records_serialize_with_serdes_keys_one_date_format_and_appends, crates/navree-orm/tests/orm/attributes/resources.rs::records_read_from_the_database_serialize_like_new_ones

## Part C. The query builder

`exec` below is any executor: `&Db`, `&Uow`, a connection or a transaction (DESIGN.md section 6).

### C1. Conditions (`QWH`)

- [ ] **QWH-001** Compare a column to a value with `=`, `<>`/`!=`, `<`, `<=`, `>`, `>=`. — API: `where_(Deal::AMOUNT.gte(100))`, `or_where(..)`, `Column::eq/ne/gt/gte/lt/lte`, `Column::op(Op, v)` — Track: orm-core
- [ ] **QWH-002** Equality with a missing value becomes `IS NULL`. — API: `Column::eq_opt(Option<T>)` — Track: orm-core
- [ ] **QWH-003** Several conditions at once. — API: chained `where_`, `Filter::all([..])`, `Filter::any([..])`, `Filter::and`, `Filter::or` — Track: orm-core — Notes: replaces the list-of-pairs form.
- [ ] **QWH-004** Negated conditions. — API: `where_not(filter)`, `or_where_not(filter)`, `Filter::not` — Track: orm-core
- [ ] **QWH-005** Nested groups. — API: `where_group(|g| ..)`, `or_where_group`, `where_not_group` — Track: orm-core
- [ ] **QWH-006** Compare two columns. — API: `where_column(Deal::UPDATED_AT, Op::Gt, Deal::CREATED_AT)`, `or_where_column`, `Column::eq_column` — Track: orm-core
- [ ] **QWH-007** Compare a column with a subquery. — API: `where_sub(col, Op, subquery)` — Track: orm-core
- [ ] **QWH-008** Raw condition with bound values. — API: `where_raw("lower(name) = ?", [v])`, `or_where_raw`, `Filter::raw` — Track: orm-core — Notes: `?` placeholders are renumbered for PostgreSQL.
- [ ] **QWH-009** `IN` / `NOT IN` value lists. — API: `where_in`, `or_where_in`, `where_not_in`, `or_where_not_in`, `Column::is_in`, `Column::not_in` — Track: orm-core — Notes: an empty list is always false for `IN` and always true for `NOT IN`; lists above the dialect's bind limit are split.
- [ ] **QWH-010** `IN` / `NOT IN` subqueries. — API: `where_in_sub`, `where_not_in_sub` (+ `or_`) — Track: orm-core
- [ ] **QWH-011** Large integer key lists inlined as literals, so no bind-parameter limit applies. — API: `where_integer_in_raw`, `where_integer_not_in_raw` (+ `or_`) — Track: orm-core — Notes: used by eager loading; only integers are ever inlined.
- [ ] **QWH-012** `NULL` checks on one or several columns. — API: `where_null`, `where_not_null`, `or_where_null`, `or_where_not_null`, `Column::is_null`, `Column::is_not_null` — Track: orm-core
- [ ] **QWH-013** `BETWEEN` two values. — API: `where_between`, `where_not_between`, `or_where_between`, `or_where_not_between`, `Column::between`, `Column::not_between` — Track: orm-core
- [ ] **QWH-014** A column between two other columns. — API: `where_between_columns`, `where_not_between_columns` (+ `or_`) — Track: orm-core
- [ ] **QWH-015** A value between two columns. — API: `where_value_between(v, [a, b])`, `where_value_not_between` (+ `or_`) — Track: orm-core
- [ ] **QWH-016** Date-part conditions. — API: `where_date`, `where_time`, `where_day`, `where_month`, `where_year` (+ `or_`) — Track: orm-core — Notes: compiled per dialect (sections J1 to J3); typed `NaiveDate`, `NaiveTime` and integers.
- [ ] **QWH-017** Relative-date conditions on one or several columns. — API: `where_past`, `where_future`, `where_now_or_past`, `where_now_or_future`, `where_today`, `where_before_today`, `where_after_today`, `where_today_or_before`, `where_today_or_after` (+ `or_`) — Track: orm-core — Notes: "now" and "today" come from the app clock and timezone, so tests can freeze them.
- [ ] **QWH-018** `LIKE`, case-insensitive by default, with case-sensitive and negated forms. — API: `where_like`, `where_not_like`, `or_where_like`, `or_where_not_like`, `where_like_case_sensitive` (+ variants) — Track: orm-core
- [ ] **QWH-019** Null-safe equality. — API: `Column::null_safe_eq`, `where_null_safe_equals` (+ `or_`) — Track: orm-core
- [ ] **QWH-020** Byte-wise comparison. — API: `where_binary`, `where_not_binary` (+ `or_`) — Track: mysql — Notes: MySQL `BINARY` comparison; elsewhere equality is already byte-wise (documented).
- [ ] **QWH-021** `EXISTS` / `NOT EXISTS` subqueries. — API: `where_exists(q)`, `where_not_exists`, `or_where_exists`, `or_where_not_exists`, `Filter::exists` — Track: orm-core
- [ ] **QWH-022** Row-value (tuple) comparisons. — API: `where_row_values([a, b], Op::Lt, [x, y])`, `or_where_row_values` — Track: orm-core — Notes: needed by keyset pagination; SQLite 3.15+.
- [ ] **QWH-023** JSON contains / does not contain a value. — API: `where_json_contains`, `where_json_doesnt_contain` (+ `or_`) — Track: orm-core
- [ ] **QWH-024** JSON arrays overlap. — API: `where_json_overlaps`, `where_json_doesnt_overlap` (+ `or_`) — Track: orm-streams — Notes: native on MySQL; PostgreSQL through `jsonb_array_elements` (or `?|` for arrays of strings); SQLite through `json_each`.
- [ ] **QWH-025** A JSON path exists / does not exist. — API: `where_json_contains_key`, `where_json_doesnt_contain_key` (+ `or_`) — Track: orm-streams
- [ ] **QWH-026** JSON array length. — API: `where_json_length(col, Op, n)` (+ `or_`) — Track: orm-core
- [ ] **QWH-027** JSON path columns in conditions, selects and orders (`meta->status`, `meta->tags[0]`), including comparisons with JSON booleans. — API: `col("meta->status")`, `Column::json_path(..)` — Track: orm-core
- [ ] **QWH-028** Full-text search. — API: `where_full_text(cols, query, FullText::english().websearch())`, `or_where_full_text` — Track: orm-streams — Notes: PostgreSQL plain, phrase, websearch and raw modes with a language allow-list and direct `tsvector` columns; MySQL modes are the mysql track's (J2); SQLite `Unsupported`; MongoDB `$text` (section M2).
- [ ] **QWH-029** One condition applied to several columns: all must match, any may match, none may match. — API: `where_all(cols, ..)`, `where_any`, `where_none` (+ `or_`) — Track: orm-core
- [ ] **QWH-030** Bitwise conditions. — API: `where_bitwise(col, BitOp::And, mask)`, `having_bitwise` — Track: orm-core — Notes: PostgreSQL compiles to `(col & ?)::bool`.
- [ ] **QWH-031** Pattern operators beyond `LIKE`: `ILIKE`, regular expressions (case-sensitive and not, negated), `SIMILAR TO`. — API: `Column::ilike`, `Column::regex`, `Column::regex_case_insensitive`, `Column::not_regex`, `Column::similar_to` — Track: orm-streams — Notes: per-dialect forms (`~`, `~*`, `REGEXP`); SQLite needs a registered `regexp` function, otherwise `Unsupported`.
- [ ] **QWH-032** PostgreSQL JSON, array and range operators (`@>`, `<@`, `?`, `?|`, `?&`, `&&`, `||`, `-`, `#-`, `@?`, `@@`) and `IS DISTINCT FROM`. — API: typed helpers `Column::contains`, `contained_by`, `overlaps`, `has_key`, and `Filter::op_raw(col, op, v)` checked against a per-dialect allow-list — Track: orm-streams
- [ ] **QWH-033** `SOUNDS LIKE`. — API: `Column::sounds_like` — Track: mysql
- [ ] **QWH-034** Operators a dialect does not know are rejected. — API: `OrmError::InvalidQuery` naming the operator — Track: orm-core — Notes: the typed `Op` turns most mistakes into compile errors; raw operator strings are validated.
- [ ] **QWH-035** Conditions built by other Navree crates (navree-access list filters) or by the SQL builder directly. — API: `From<sea_query::Condition>`, `Filter::sql(expr)` — Track: orm-core
- [ ] **QWH-036** Conditions on the primary key, one key or many. — API: `where_key`, `where_key_not`, `or_where_key`, `or_where_key_not` — Track: orm-core
- [ ] **QWH-037** Exclude given records. — API: `except(&records)` — Track: orm-core
- [ ] **QWH-038** Condition and first row in one call. — API: `first_where(exec, filter)` — Track: orm-core
- [ ] **QWH-039** Conditional clauses. — API: `when(cond, |q| ..)`, `when_else(cond, f, g)`, `when_some(opt, |q, v| ..)`, `unless(cond, f)` — Track: orm-core
- [ ] **QWH-040** Merge the conditions of another builder as a group. — API: `merge_wheres(other)`, `where_builder(q)` — Track: orm-core
- [ ] **QWH-041** Methods named after columns (a "where title" method). — API: n/a → typed constants (`Deal::TITLE.eq(..)`) — Track: orm-core
- [ ] **QWH-042** Vector similarity conditions. — API: `where_vector_similar_to(col, &v, min_similarity)`, `where_vector_distance_less_than(col, &v, max)` (+ `or_`) — Track: orm-streams — Notes: pgvector cosine distance; ordered by similarity unless turned off; MariaDB vector distance; `Unsupported` elsewhere.
- [ ] **QWH-043** `HAVING` family. — API: `having`, `or_having`, `having_group`, `having_null`, `having_not_null`, `having_between`, `having_not_between` (each with `or_`), `having_raw`, `or_having_raw` — Track: orm-core
- [ ] **QWH-044** "Or" proxies that turn the next scope call into an `or` group. — API: n/a → `or_where_group(|q| q.scope(f))` — Track: orm-core
- [ ] **QWH-045** Enums and other typed values bind as their stored representation everywhere (conditions, updates, inserts, upserts). — Track: orm-core

### C2. Selects, joins, grouping, ordering, limits, unions, locks (`QCL`)

- [ ] **QCL-001** Select columns, with aliases. — API: `select([..])`, `Column::as_("alias")` — Track: orm-core
- [ ] **QCL-002** Add columns to the select. — API: `add_select([..])` — Track: orm-core
- [ ] **QCL-003** Raw select with bound values. — API: `select_raw(sql, values)` — Track: orm-core
- [ ] **QCL-004** Expressions and subqueries as columns. — API: `select_expr(expr, alias)`, `select_sub(q, alias)` — Track: orm-core
- [ ] **QCL-005** `DISTINCT` and `DISTINCT ON`. — API: `distinct()`, `distinct_on([..])` — Track: orm-core — Notes: `DISTINCT ON` is PostgreSQL only (`Unsupported` elsewhere).
- [ ] **QCL-006** `FROM` with an alias, derived tables and raw sources. — API: `from_as(alias)`, `from_sub(q, alias)`, `from_raw(sql, values)` — Track: orm-streams
- [ ] **QCL-007** Inner joins on columns, or on a value. — API: `join(table, a, Op, b)`, `join_where(table, a, Op, value)` — Track: orm-core
- [ ] **QCL-008** Left, right and cross joins, with value forms. — API: `left_join`, `left_join_where`, `right_join`, `right_join_where`, `cross_join` — Track: orm-core — Notes: SQLite before 3.39 has no right join: `Unsupported`, never wrong SQL.
- [ ] **QCL-009** Join clauses with several conditions. — API: `join_on(table, |j| j.on(..).or_on(..).where_(..).where_in(..).where_null(..))` — Track: orm-core
- [ ] **QCL-010** Joins to subqueries. — API: `join_sub(q, alias, ..)`, `left_join_sub`, `right_join_sub`, `cross_join_sub` — Track: orm-streams
- [ ] **QCL-011** Lateral joins. — API: `join_lateral(q, alias)`, `left_join_lateral(q, alias)` — Track: orm-streams — Notes: PostgreSQL and MySQL 8.0.14+; `Unsupported` on SQLite and MariaDB.
- [ ] **QCL-012** Straight joins. — API: `straight_join`, `straight_join_where`, `straight_join_sub` — Track: mysql
- [ ] **QCL-013** Index hints. — API: `use_index`, `force_index`, `ignore_index` — Track: mysql — Notes: SQLite honours `force_index` as `INDEXED BY`; PostgreSQL ignores hints (documented).
- [ ] **QCL-014** `GROUP BY` columns and expressions. — API: `group_by([..])`, `group_by_raw(sql, values)` — Track: orm-core
- [ ] **QCL-015** `ORDER BY` columns, expressions, subqueries and raw SQL, ascending or descending. — API: `order_by(col.asc())`, `order_by_desc`, `order_by_raw`, `order_by_sub` — Track: orm-core
- [ ] **QCL-016** `NULLS FIRST` / `NULLS LAST`. — API: `OrderBy::nulls_first()`, `nulls_last()` — Track: orm-core — Notes: emulated with an `IS NULL` sort key on MySQL.
- [ ] **QCL-017** Newest or oldest first by a column (the model's created-at column by default). — API: `latest()`, `oldest()`, `latest_by(col)`, `oldest_by(col)` — Track: orm-core
- [ ] **QCL-018** Random order, optionally seeded. — API: `in_random_order()`, `in_random_order_seeded(seed)` — Track: orm-core — Notes: MySQL `RAND(seed)`; PostgreSQL and SQLite ignore the seed (documented).
- [ ] **QCL-019** Order by a given sequence of values. — API: `in_order_of(col, values)` — Track: orm-core — Notes: a `CASE` expression; an empty list adds no order.
- [ ] **QCL-020** Replace existing orders. — API: `reorder()`, `reorder_by(col)`, `reorder_desc(col)` — Track: orm-core
- [ ] **QCL-021** Limits, offsets and page windows. — API: `limit`/`take`, `offset`/`skip`, `for_page(page, per_page)` — Track: orm-core
- [ ] **QCL-022** Keyset windows before or after a key. — API: `for_page_before_id(per_page, id)`, `for_page_after_id(per_page, id)` — Track: orm-core
- [ ] **QCL-023** Limit per group (top N per parent). — API: `group_limit(n, col)` — Track: orm-streams — Notes: `ROW_NUMBER() OVER (PARTITION BY ..)`; older MySQL, MariaDB and SQLite versions fall back as documented in J2/J3; used by eager loads with limits.
- [ ] **QCL-024** `UNION` / `UNION ALL`, with orders and limits on the union. — API: `union(q)`, `union_all(q)` — Track: orm-streams — Notes: counts, `exists` and pagination wrap unions as a derived table.
- [ ] **QCL-025** Row locks. — API: `lock_for_update()`, `shared_lock()`, `lock(LockMode)`, `.skip_locked()`, `.no_wait()`, `LockMode::NoKeyUpdate` — Track: orm-streams — Notes: PostgreSQL `FOR UPDATE`, `FOR SHARE`, `FOR NO KEY UPDATE`; MySQL `FOR UPDATE`, `LOCK IN SHARE MODE` (`FOR SHARE` on 8.0); SQLite none (writers serialise with `BEGIN IMMEDIATE`); `Uow::lock_for_update()` reports support.
- [ ] **QCL-026** Statement timeout. — API: `timeout(Duration)` — Track: orm-streams — Notes: MySQL execution-time hint on `SELECT`; PostgreSQL `SET LOCAL statement_timeout` around the statement; SQLite through a progress handler or `Unsupported`; expiry is `OrmError::Timeout`.
- [ ] **QCL-027** Raw expressions with bound values anywhere a column fits. — API: `raw(sql, values)`, `Expr::raw` — Track: orm-core
- [ ] **QCL-028** Functions and arithmetic in expressions. — API: `Expr::func(name, args)`, `lower`, `upper`, `coalesce`, `add`, `sub`, `mul`, `div`, `count_distinct` — Track: orm-core
- [ ] **QCL-029** Vector distance as a column and as an order. — API: `select_vector_distance(col, &v, alias)`, `order_by_vector_distance(col, &v)` — Track: orm-streams
- [ ] **QCL-030** Queries on a table without a model. — API: `navree_orm::table("deal_tags")` (`TableBuilder`) — Track: orm-core
- [ ] **QCL-031** Identifier quoting and alias parsing per dialect (`table.column`, `x as y`, reserved words, JSON paths). — Track: orm-core
- [ ] **QCL-032** Builder helpers that pass the builder to a callback (`tap`, `pipe`). — API: n/a → `query.scope(f)` or a plain function call — Track: orm-core
- [ ] **QCL-033** Start a query for a model, with its global scopes and default eager loads, plus static shortcuts for the common first calls. — API: `Deal::query()` → `Builder<Deal>`, `Deal::where_(filter)`, `Deal::all(exec)` — Track: orm-core — Notes: builder methods take `self` and return `Self`; terminal methods take the executor first.

### C3. Retrieval and aggregates (`QRT`)

- [ ] **QRT-001** Every matching record. — API: `get(exec)` → `Collection<M>`, `Deal::all(exec)` — Track: orm-core
- [ ] **QRT-002** The first record. — API: `first(exec)` → `Option<Record<M>>` — Track: orm-core
- [ ] **QRT-003** The first record or a not-found error (404 in Navree's error shape). — API: `first_or_fail(exec)` — Track: orm-core
- [ ] **QRT-004** The first record or a fallback. — API: n/a → `first(exec).await?.unwrap_or_else(..)` — Track: orm-core
- [ ] **QRT-005** Find by key. — API: `Deal::find(exec, key)`, `query().find(exec, key)` — Track: orm-core
- [ ] **QRT-006** Find by several keys. — API: `find_many(exec, keys)` — Track: orm-core — Notes: an empty key list returns an empty collection without a query.
- [ ] **QRT-007** Find or fail; for several keys every key must exist and the error lists the missing ones. — API: `find_or_fail(exec, key)`, `find_many_or_fail(exec, keys)` — Track: orm-core
- [ ] **QRT-008** Find or a new unsaved record. — API: `find_or_new(exec, key)` — Track: orm-core
- [ ] **QRT-009** Find or a fallback. — API: n/a → `find(exec, key).await?.unwrap_or_else(..)` — Track: orm-core
- [ ] **QRT-010** Exactly one result, by query or by key. — API: `sole(exec)`, `find_sole(exec, key)` — Track: orm-core — Notes: `ModelNotFound` or `MultipleRecordsFound { count }`.
- [ ] **QRT-011** One column of the first row, or of the only row. — API: `value::<T>(exec, col)`, `value_or_fail::<T>`, `sole_value::<T>` — Track: orm-core
- [ ] **QRT-012** One raw expression of the first row. — API: `raw_value::<T>(exec, sql, values)` — Track: orm-core
- [ ] **QRT-013** Pluck one column (cast applied), optionally keyed by another. — API: `pluck::<T>(exec, col)`, `pluck_keyed::<K, V>(exec, value_col, key_col)` — Track: orm-core — Notes: qualified and JSON-path columns decode by their last segment.
- [ ] **QRT-014** Join one column's values into a string. — API: `implode(exec, col, glue)` — Track: orm-core
- [ ] **QRT-015** Existence checks. — API: `exists(exec)`, `doesnt_exist(exec)` — Track: orm-core — Notes: callback forms are n/a (`if !q.exists(exec).await? { .. }`).
- [ ] **QRT-016** Aggregates. — API: `count(exec)`, `count_column(exec, col)`, `count_distinct`, `sum::<T>`, `avg::<T>`, `min::<T>`, `max::<T>`, `aggregate(exec, Agg, col)` — Track: orm-core — Notes: typed, lenient decoding (`SUM` is numeric on PostgreSQL, integer on SQLite); grouped, distinct and union queries are counted through a subquery; `average` alias n/a.
- [ ] **QRT-017** Keys of the matching records. — API: `model_keys(exec)` → `Vec<M::Key>` — Track: orm-core
- [ ] **QRT-018** Records from raw SQL. — API: `Deal::from_query(exec, raw(sql, values))` — Track: orm-core
- [ ] **QRT-019** Records from rows already fetched. — API: `Deal::hydrate(rows)`, `Record::from_row(&row)` — Track: orm-core
- [ ] **QRT-020** Records without running eager loads. — API: `get_models(exec)` — Track: orm-core
- [ ] **QRT-021** An unsaved record carrying the builder's pending attributes. — API: `make(model)` — Track: orm-core
- [ ] **QRT-022** Callbacks before compilation and after results return. — API: `before_query(f)`, `after_query(f)` — Track: orm-core
- [ ] **QRT-023** Read from the writer even when replicas exist. — API: `use_write()` — Track: orm-streams
- [ ] **QRT-024** Untyped rows from table queries. — API: `TableBuilder::get(exec)` → `Vec<Row>`, `Row::get::<T>(col)` — Track: orm-core

### C4. Writes through the builder (`QWR`)

- [ ] **QWR-001** Insert one or many rows without events. — API: `table(..).insert(exec, rows)`, `Deal::insert(exec, models)` — Track: orm-streams — Notes: one multi-row statement, split under the bind limit; no statement for an empty list; every row must have the same columns.
- [ ] **QWR-002** Insert and return the generated key. — API: `insert_get_id::<K>(exec, row)` — Track: orm-streams — Notes: `RETURNING` on PostgreSQL and SQLite 3.35+, `last_insert_id` on MySQL.
- [ ] **QWR-003** Insert ignoring conflicts. — API: `insert_or_ignore(exec, rows)` → affected count — Track: orm-streams
- [ ] **QWR-004** Insert ignoring conflicts and return the inserted rows, optionally only for given unique columns. — API: `insert_or_ignore_returning(exec, rows, returning, unique_by)` — Track: orm-streams
- [ ] **QWR-005** `INSERT … SELECT`, also ignoring conflicts. — API: `insert_using(exec, cols, query)`, `insert_or_ignore_using` — Track: orm-streams
- [ ] **QWR-006** Mass update with typed assignments. — API: `update(exec, [Deal::STAGE.set(Stage::Won)])`, `Column::set_expr`, `Column::set_null` — Track: orm-core — Notes: model builders add `updated_at`; returns affected rows; no model events.
- [ ] **QWR-007** Update JSON paths in place. — API: `update(exec, [col("meta->status").set("won")])` — Track: orm-streams — Notes: PostgreSQL `jsonb_set`, MySQL `json_set`, SQLite `json_set`/`json_patch`, MongoDB `$set` on dot paths.
- [ ] **QWR-008** Updates with joins, orders or limits. — API: `update` on a builder that has them — Track: orm-streams — Notes: MySQL multi-table update and `ORDER BY … LIMIT`; PostgreSQL `UPDATE … FROM` or `ctid IN (subquery)`; SQLite `rowid IN (subquery)`.
- [ ] **QWR-009** PostgreSQL `UPDATE … FROM` explicitly. — API: `update_from(exec, assignments)` — Track: orm-streams
- [ ] **QWR-010** Update the row matching some columns, or insert it. — API: `update_or_insert(exec, match, values)` — Track: orm-streams — Notes: values may depend on whether the row existed (closure taking a `bool`).
- [ ] **QWR-011** Upsert rows by unique columns, choosing the columns to update. — API: `upsert(exec, rows, unique_by, update_cols)` — Track: orm-streams — Notes: model builders add generated ids and timestamps and leave `created_at` out of the update; PostgreSQL and SQLite `ON CONFLICT … DO UPDATE SET c = excluded.c`; MySQL syntax in J2.
- [ ] **QWR-012** Mass increment and decrement of one or several columns, with extra assignments. — API: `increment(exec, col, n)`, `decrement`, `increment_each`, `decrement_each`, `increment_with` — Track: orm-core — Notes: model builders add `updated_at`; amounts may be floats or decimals.
- [ ] **QWR-013** Mass delete. — API: `delete(exec)`, `Deal::delete_where(exec, filter)` — Track: orm-core — Notes: soft-deleting models set `deleted_at` instead; `force_delete(exec)` removes rows (SFT-007); no model events.
- [ ] **QWR-014** Deletes with joins or limits. — API: `delete` on a builder that has them — Track: orm-streams — Notes: MySQL multi-table delete; PostgreSQL and SQLite through `ctid`/`rowid IN (subquery)`.
- [ ] **QWR-015** Delete one row of a table builder by id. — API: `table(..).delete_id(exec, id)` — Track: orm-core
- [ ] **QWR-016** Truncate a table. — API: `truncate(exec)` — Track: orm-streams — Notes: PostgreSQL `TRUNCATE … RESTART IDENTITY` (cascade opt-in), MySQL `TRUNCATE`, SQLite `DELETE` plus resetting its sequence.
- [ ] **QWR-017** Touch timestamps on every matching row. — API: `touch(exec)`, `touch_columns(exec, &[cols])` — Track: orm-core
- [ ] **QWR-018** Insert models with defaults, generated ids, timestamps and casts applied, without events. — API: `fill_and_insert(exec, models)`, `fill_and_insert_or_ignore`, `fill_and_insert_get_id`, `fill_for_insert` — Track: orm-streams
- [ ] **QWR-019** First matching record, or create it. — API: `first_or_create(exec, match, values)` — Track: orm-core — Notes: the create falls back to a select on a unique violation (QWR-022).
- [ ] **QWR-020** First matching record, or a new unsaved one. — API: `first_or_new(exec, match, values)` — Track: orm-core
- [ ] **QWR-021** Update the matching record, or create it. — API: `update_or_create(exec, match, values)` — Track: orm-core
- [ ] **QWR-022** Create first; on a unique violation return the existing row. — API: `create_or_first(exec, match, values)` — Track: orm-core — Notes: inside a transaction it runs in a savepoint so a PostgreSQL error does not abort the unit of work; values may be computed lazily.
- [ ] **QWR-023** Increment the matching record, or create it with a starting value. — API: `increment_or_create(exec, match, col, default, step)` — Track: orm-core
- [ ] **QWR-024** Create through a builder, applying its pending attributes. — API: `query.create(exec, model)` — Track: orm-core
- [ ] **QWR-025** Replace the builder's delete behaviour from a scope. — API: n/a → soft-delete-aware `delete` is built in; other behaviours come from a `GlobalScope` plus a trait method — Track: orm-lifecycle

### C5. Inspection and debugging (`QDB`)

- [ ] **QDB-001** The SQL and values a builder compiles to, per dialect. — API: `to_sql(Dialect)` → `Statement { sql, values }` — Track: orm-core
- [ ] **QDB-002** SQL with values inlined, for logs only. — API: `to_raw_sql(Dialect)` — Track: orm-core — Notes: literals quoted per dialect; operators that contain `?` survive; never executed.
- [ ] **QDB-003** Log the compiled SQL. — API: `dump()`, `dump_raw_sql()` (tracing, debug level) — Track: orm-core — Notes: dump-and-exit forms are n/a (`dbg!`).
- [ ] **QDB-004** Query plans. — API: `explain(exec)` → `Vec<Row>` — Track: orm-core — Notes: `EXPLAIN` on PostgreSQL and MySQL, `EXPLAIN QUERY PLAN` on SQLite.
- [ ] **QDB-005** Inspect builder parts. — API: `columns()`, `limit_value()`, `offset_value()`, `eager_loads()`, `into_ir()` — Track: orm-core — Notes: reading or editing raw binding lists is n/a: values live in the IR next to their clause.
- [ ] **QDB-006** Clone a builder, optionally without some parts. — API: `Clone`, `clone_without(&[Part::Orders, Part::Limit])` — Track: orm-core — Notes: clone callbacks are n/a.
- [ ] **QDB-007** Drop the model layer and keep the table query. — API: `into_table()` — Track: orm-core
- [ ] **QDB-008** Builder macros and runtime-registered builder methods. — API: n/a → extension traits on `Builder<M>` and `TableBuilder` — Track: orm-core
- [ ] **QDB-009** Accessors for the model instance, connection, grammar and row processor; row fetch modes. — API: n/a → the builder is typed by its model; `ExecContext`, `Dialect`; rows decode by name — Track: orm-core
- [ ] **QDB-010** Mistakes found while building are reported when the query runs, naming the clause. — API: `QueryIr::errors` → `OrmError::InvalidQuery` — Track: orm-core

### C6. Pagination (`PAG`)

- [ ] **PAG-001** Offset pagination with total and last page in Navree's page shape. — API: `paginate(exec, page)` → `Page<Record<M>>` (`{data, meta: {current_page, per_page, total, last_page}}`) — Track: orm-core — Notes: the page is a number (model `per_page`) or a `PageQuery`; pages past the end return empty `data`.
- [ ] **PAG-002** Offset pagination with a known total (no count query). — API: `paginate_with_total(exec, page, total)` — Track: orm-core
- [ ] **PAG-003** Correct totals for grouped, distinct, `HAVING` and union queries. — Track: orm-core — Notes: counted over a derived table with orders removed.
- [ ] **PAG-004** Simple pagination without a total. — API: `simple_paginate(exec, page)` → `SimplePage` (`has_more`) — Track: orm-core — Notes: fetches `per_page + 1` rows.
- [ ] **PAG-005** Cursor pagination over the builder's orders, the key appended as a tie-breaker. — API: `cursor_paginate(exec, CursorRequest)` → `CursorPage { data, next_cursor, prev_cursor }` — Track: orm-core — Notes: several order columns, mixed directions, previous pages by reversing, opaque cursors; orders on expressions that cannot be compared are refused.
- [ ] **PAG-006** Request extractors with bounds. — API: `PageQuery` (`page`, `per_page` capped by `MAX_PER_PAGE`), `CursorRequest` — Track: orm-core
- [ ] **PAG-007** Transform page items and keep the meta. — API: `Page::map(f)`, `SimplePage::map`, `CursorPage::map` — Track: orm-core
- [ ] **PAG-008** Paginate relations, including pivot columns. — API: `deal.tags().paginate(exec, page)`, `simple_paginate`, `cursor_paginate` — Track: orm-relations
- [ ] **PAG-009** Page links and custom parameter names. — API: n/a → Navree's page shape carries meta and clients build links; parameters are `page`, `per_page`, `cursor` — Track: orm-core
- [x] **PAG-010** OpenAPI schemas for page types. — API: `Page<T>`, `SimplePage<T>`, `CursorPage<T>` schemas — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/resources.rs::model_schemas_describe_serialized_records, crates/navree-orm/tests/orm/attributes/resources.rs::the_openapi_document_carries_model_and_page_schemas

### C7. Chunking and streaming (`STR`)

- [ ] **STR-001** Process results in chunks, with the option to stop. — API: `chunk(exec, n, |batch| async { Ok(ControlFlow::Continue(())) })` — Track: orm-streams — Notes: orders by key when no order is set; stops on `Break`.
- [ ] **STR-002** Map over chunks into one result. — API: `chunk_map(exec, n, f)` → `Vec<T>` — Track: orm-streams
- [ ] **STR-003** Visit each record, fetched in chunks, with the option to stop. — API: `each(exec, n, f)` — Track: orm-streams
- [ ] **STR-004** Chunk by key, safe while the callback updates rows, ascending or descending, on a custom column and alias. — API: `chunk_by_id(exec, n, f)`, `chunk_by_id_desc`, `chunk_by_column(exec, n, col, alias, f)` — Track: orm-streams
- [ ] **STR-005** Visit each record, chunked by key. — API: `each_by_id(exec, n, f)` — Track: orm-streams
- [ ] **STR-006** Async streams of records fetched in chunks, by offset or by key. — API: `lazy(exec, n)`, `lazy_by_id(exec, n)`, `lazy_by_id_desc(exec, n)` → `impl Stream<Item = Result<Record<M>, OrmError>>` — Track: orm-streams
- [ ] **STR-007** One query whose rows stream as they arrive. — API: `cursor(exec)` → `Stream` — Track: orm-streams — Notes: no eager loading on cursors (use `lazy`); holds a connection while the stream lives; `retrieved` fires per row.
- [ ] **STR-008** Eager loads applied to each chunk of `chunk`, `each` and `lazy`. — Track: orm-streams
- [ ] **STR-009** Chunks and streams on relations (belongs-to-many keeps pivot columns; through relations keep their keys). — API: `deal.tags().chunk(..)`, `chunk_map`, `each`, `chunk_by_id`, `chunk_by_id_desc`, `each_by_id`, `lazy`, `lazy_by_id`, `lazy_by_id_desc`, `cursor` — Track: orm-relations
- [ ] **STR-010** Stream rows of a raw statement. — API: `navree_orm::cursor(exec, raw(..))` — Track: orm-streams
- [ ] **STR-011** Lazy-collection helpers on streams (map, filter, take, chunk, remember). — API: n/a → `futures::StreamExt` adapters — Track: orm-streams

### C8. Scopes (`SCP`)

- [ ] **SCP-001** Local scopes as plain functions applied to a builder. — API: `query.scope(f)` — Track: orm-core
- [ ] **SCP-002** Named scope methods, with arguments, callable on the builder. — API: `#[scopes] impl Deal { #[scope] fn open(q: Builder<Self>) -> Builder<Self> }` → `Deal::query().open()` through `DealScopes` — Track: orm-core
- [ ] **SCP-003** A scope's conditions are grouped when they contain an `or`, so they never change the caller's conditions. — Track: orm-core
- [ ] **SCP-004** Call scopes by name at runtime and ask whether a scope exists. — API: n/a → typed scope methods; for runtime lists (API filters) match names in app code — Track: orm-core
- [ ] **SCP-005** Global scopes as types applied when the query runs. — API: `trait GlobalScope<M>` — Track: orm-core
- [ ] **SCP-006** Global scopes declared on the model. — API: `#[model(global_scopes(ActiveOnly))]` — Track: orm-core
- [ ] **SCP-007** Global scopes registered at runtime by type or as a named closure, and inspected. — API: `OrmModule::global_scope::<M>(name, f)`, `M::has_global_scope(name)`, `M::global_scope(name)`, `M::global_scopes()` — Track: orm-lifecycle
- [ ] **SCP-008** Remove one, all, or all-but-some global scopes for a query. — API: `without_global_scope::<S>()`, `without_global_scopes()` — Track: orm-core
- [ ] **SCP-009** Remove global scopes by name, keep only some, and list what was removed. — API: `without_global_scope_named("x")`, `without_global_scopes_except(..)`, `removed_scopes()` — Track: orm-lifecycle
- [ ] **SCP-010** Global scopes that add builder methods (soft deletes adding `with_trashed`). — API: n/a → trait methods on `Builder<M>` bounded by the model's capability (`M: SoftDeletes`) — Track: orm-lifecycle
- [ ] **SCP-011** Builders without global scopes and without default eager loads. — API: `M::query_without_scopes()`, `M::bare_query()` — Track: orm-core
- [ ] **SCP-012** Pending attributes: conditions that also fill records the builder creates. — API: `with_attributes([(col, v)])`, `with_attributes_unconditioned([(col, v)])` — Track: orm-core
- [ ] **SCP-013** Global scopes apply to relation queries, existence subqueries, relation aggregates and pagination counts. — Track: orm-relations

## Part D. Relationships

### D1. Relation definitions (`REL`)

- [ ] **REL-001** Relations are methods returning typed relation builders, registered by name so string paths (`"company.owner"`) resolve. — API: `#[relations] impl Deal { fn company(&self) -> BelongsTo<Deal, Company> { self.belongs_to(Deal::COMPANY_ID) } }` — Track: orm-relations
- [ ] **REL-002** Has one. — API: `self.has_one::<R>(R::DEAL_ID)`, `.local_key(col)` — Track: orm-relations — Notes: default foreign key `{parent}_id`, default local key the parent's key.
- [ ] **REL-003** Has many. — API: `self.has_many::<R>(R::DEAL_ID)`, `.local_key(col)` — Track: orm-relations
- [ ] **REL-004** Belongs to. — API: `self.belongs_to::<R>(Self::COMPANY_ID)`, `.owner_key(col)` — Track: orm-relations — Notes: the method name is the relation name used by touching and `associate`.
- [ ] **REL-005** Belongs to many through a pivot table. — API: `self.belongs_to_many::<R>("deal_tag")`, `.foreign_pivot_key(..)`, `.related_pivot_key(..)`, `.parent_key(..)`, `.related_key(..)` — Track: orm-relations — Notes: default pivot table: both singular snake names in alphabetical order joined by `_`.
- [ ] **REL-006** Has one through an intermediate model. — API: `self.has_one_through::<R, Through>(..)`, `.first_key`, `.second_key`, `.local_key`, `.second_local_key` — Track: orm-relations
- [ ] **REL-007** Has many through an intermediate model. — API: `self.has_many_through::<R, Through>(..)` with the same key options — Track: orm-relations
- [ ] **REL-008** Through relations composed from existing relations. — API: `self.through(Deal::company).has(Company::owner)` — Track: orm-relations — Notes: typed composition replaces name-based chaining.
- [ ] **REL-009** Polymorphic has one. — API: `self.morph_one::<R>("commentable")`, `.type_column(..)`, `.id_column(..)`, `.local_key(..)` — Track: orm-relations
- [ ] **REL-010** Polymorphic has many. — API: `self.morph_many::<R>("commentable")` — Track: orm-relations
- [ ] **REL-011** Polymorphic belongs to. — API: `self.morph_to("commentable")` → `MorphTo<Self>`, `.type_column(..)`, `.id_column(..)`, `.owner_key(..)` — Track: orm-relations — Notes: results are typed through the morph map (MRP-004).
- [ ] **REL-012** Polymorphic many to many. — API: `self.morph_to_many::<R>("taggable", "taggables")` — Track: orm-relations
- [ ] **REL-013** Inverse polymorphic many to many. — API: `self.morphed_by_many::<R>("taggable", "taggables")` — Track: orm-relations
- [ ] **REL-014** One record out of a has-many: latest, oldest, or by a custom aggregate over one or several columns, with extra constraints. — API: `.latest_of_many()`, `.oldest_of_many()`, `.latest_of_many_by(col)`, `.of_many([(col, Agg::Max)], |q| ..)` — Track: orm-relations — Notes: available on has-many, morph-many and has-many-through; compiled as a join on a grouped subquery.
- [ ] **REL-015** Turn a to-many relation into its to-one form. — API: `.one()` on has-many, has-many-through and morph-many — Track: orm-relations
- [ ] **REL-016** Default related record when none exists. — API: `.with_default()`, `.with_default_value(model)`, `.with_default_fn(|parent| ..)` — Track: orm-relations — Notes: belongs-to, has-one, has-one-through, morph-one and one-of-many relations.
- [ ] **REL-017** Back-reference to the parent on loaded children. — API: `.inverse("deal")` (alias `chaperone`), `.without_inverse()` — Track: orm-relations — Notes: children receive a snapshot of the parent without its relations, so there are no reference cycles.
- [ ] **REL-018** Relations added to a model by another module. — API: `RelationRegistry::define::<Deal>("invoices", ..)`, plus an extension trait for typed access — Track: orm-relations
- [ ] **REL-019** Self-referencing relations (parent and children in one table). — Track: orm-relations — Notes: existence and aggregate subqueries alias the inner table with a generated alias.
- [ ] **REL-020** Relations to models on another connection. — Track: orm-relations — Notes: eager loads run on the related model's connection; existence subqueries across connections are refused with `Unsupported` because they cannot join.
- [ ] **REL-021** Unknown relation names in a path are errors naming the model and relation. — API: `OrmError::RelationNotFound { model, relation }` — Track: orm-relations

### D2. Relation builders (`RLB`)

- [ ] **RLB-001** Query a relation with every builder method. — API: `deal.activities().where_(..).latest().get(exec)` — Track: orm-relations
- [ ] **RLB-002** Retrieval on relations. — API: `get`, `first`, `first_or_fail`, `first_where`, `sole`, `find`, `find_many`, `find_or_fail`, `find_sole`, `find_or_new`, `exists`, `count` on relation builders — Track: orm-relations — Notes: belongs-to-many and through relations qualify key columns with the related table.
- [ ] **RLB-003** Limits on a relation inside an eager load apply per parent. — API: `with_constrained("activities", |q| q.latest().limit(3))` — Track: orm-relations — Notes: compiled with `group_limit` (QCL-023).
- [ ] **RLB-004** Whether a record is the related record, without a query. — API: `deal.company().is(&company)`, `is_not` — Track: orm-relations — Notes: compares foreign key, table and connection.
- [ ] **RLB-005** Relation keys and names. — API: `foreign_key_name()`, `qualified_foreign_key_name()`, `owner_key_name()`, `local_key_name()`, `parent_key()`, `related_table()`, `relation_name()`, `morph_type_column()`, `morph_name()` — Track: orm-relations
- [ ] **RLB-006** Mass update of the related rows. — API: `deal.activities().update(exec, assignments)` — Track: orm-relations
- [ ] **RLB-007** Touch every related record. — API: `deal.tags().touch(exec)` — Track: orm-relations
- [ ] **RLB-008** Through relations including soft-deleted intermediate records. — API: `.with_trashed_parents()` — Track: orm-relations — Notes: trashed intermediate records are excluded by default.
- [ ] **RLB-009** All related keys. — API: `deal.tags().all_related_ids(exec)` — Track: orm-relations
- [ ] **RLB-010** First-or-new, first-or-create, update-or-create, create-or-first and increment-or-create on relations, with the foreign key filled in. — API: same names as QWR-019 to QWR-023 on relation builders — Track: orm-relations — Notes: belongs-to-many forms also take pivot values.
- [ ] **RLB-011** Restore-or-create and create-or-restore on relations of soft-deleting models. — API: `restore_or_create`, `create_or_restore` on relation builders — Track: orm-relations
- [ ] **RLB-012** Unsaved related records with the foreign key set. — API: `deal.activities().make(model)`, `make_many(models)` — Track: orm-relations
- [ ] **RLB-013** Upsert through has-many and morph-many, filling the foreign key (and morph type). — API: `deal.activities().upsert(exec, rows, unique_by, update)` — Track: orm-relations

### D3. Many-to-many pivots (`PIV`)

- [ ] **PIV-001** Load pivot columns with the related records. — API: `.with_pivot(["role", "added_at"])` — Track: orm-relations
- [ ] **PIV-002** Pivot timestamps, with custom column names. — API: `.with_timestamps()`, `.with_timestamps_named(created, updated)` — Track: orm-relations
- [ ] **PIV-003** Read pivot values on each related record, under `pivot` or a custom name. — API: `tag.pivot::<P>()`, `tag.pivot_value::<T>("role")`, `.as_("membership")` — Track: orm-relations
- [ ] **PIV-004** Typed pivot models with casts, events and optional incrementing ids. — API: `.using::<DealTag>()` where `DealTag: Model + Pivot` — Track: orm-relations
- [ ] **PIV-005** Conditions on pivot columns. — API: `where_pivot`, `where_pivot_in`, `where_pivot_not_in`, `where_pivot_between`, `where_pivot_not_between`, `where_pivot_null`, `where_pivot_not_null` (each with `or_`) — Track: orm-relations
- [ ] **PIV-006** A pivot condition that also fills new pivot rows. — API: `.with_pivot_value("role", "owner")` — Track: orm-relations
- [ ] **PIV-007** Order by pivot columns. — API: `order_by_pivot(col)`, `order_by_pivot_desc(col)` — Track: orm-relations
- [ ] **PIV-008** Pivot metadata. — API: `pivot_table()`, `foreign_pivot_key_name()`, `related_pivot_key_name()`, `pivot_columns()`, `has_pivot_column(col)`, `qualify_pivot_column(col)` — Track: orm-relations
- [ ] **PIV-009** Query the pivot table directly. — API: `deal.tags().pivot_query()` (`TableBuilder`), `pivot_query_for(id)` — Track: orm-relations
- [ ] **PIV-010** New pivot records, unsaved or marked existing. — API: `new_pivot(values)`, `new_existing_pivot(values)` — Track: orm-relations
- [ ] **PIV-011** Delete a pivot record by both keys when it has no id; pivot key names and timestamp detection. — API: `pivot.delete(exec)`, `pivot.foreign_key()`, `pivot.related_key()`, `pivot.has_timestamp_attributes()` — Track: orm-relations
- [ ] **PIV-012** Pivot records that know both models. — API: `.chaperone_pivot()`, `pivot.parent()`, `pivot.related()` — Track: orm-relations — Notes: snapshots, as in REL-017.
- [ ] **PIV-013** Polymorphic pivots carry the morph type. — API: `MorphPivot` — Track: orm-relations

### D4. Writing relations (`RWR`)

- [ ] **RWR-001** Save a related record through has-one, has-many, morph-one or morph-many, setting the foreign key (and morph type). — API: `deal.activities().save(exec, &mut activity)`, `save_quietly` — Track: orm-relations
- [ ] **RWR-002** Save several related records. — API: `save_many(exec, &mut [..])`, `save_many_quietly` — Track: orm-relations
- [ ] **RWR-003** Create related records. — API: `create(exec, model)`, `create_quietly`, `create_many(exec, models)`, `create_many_quietly` — Track: orm-relations
- [ ] **RWR-004** Force-create related records from untyped attributes. — API: `force_create(exec, attrs)`, `force_create_quietly`, `force_create_many`, `force_create_many_quietly` — Track: orm-relations
- [ ] **RWR-005** Associate and dissociate a belongs-to or morph-to (sets the key, and the type for morphs, and the loaded relation; saving is separate). — API: `deal.associate(Deal::company, &company)`, `deal.dissociate(Deal::company)` — Track: orm-relations
- [ ] **RWR-006** Attach by key, by record, or with pivot values. — API: `deal.tags().attach(exec, ids)`, `attach_with(exec, [(id, pivot_values)])` — Track: orm-relations — Notes: typed pivot models attach through their own `save`, so pivot events fire; touches parents.
- [ ] **RWR-007** Detach some or all. — API: `detach(exec, ids)`, `detach_all(exec)` → count — Track: orm-relations
- [ ] **RWR-008** Sync: attach the missing, detach the rest, update changed pivot values, and report what changed. — API: `sync(exec, ids)` → `SyncChanges { attached, detached, updated }` — Track: orm-relations
- [ ] **RWR-009** Sync without detaching. — API: `sync_without_detaching(exec, ids)` — Track: orm-relations
- [ ] **RWR-010** Sync with the same pivot values for every key. — API: `sync_with_pivot_values(exec, ids, values)` — Track: orm-relations
- [ ] **RWR-011** Toggle attachments. — API: `toggle(exec, ids)` → `{ attached, detached }` — Track: orm-relations
- [ ] **RWR-012** Update one existing pivot row. — API: `update_existing_pivot(exec, id, values)` — Track: orm-relations — Notes: sets the pivot's updated-at column.
- [ ] **RWR-013** Transactional fail-fast variants of attach, detach, sync, toggle and pivot updates. — API: n/a → run on a `Uow`; on a bare `Db` each call opens its own transaction — Track: orm-relations
- [ ] **RWR-014** Save or create many-to-many records and attach them with pivot values. — API: `deal.tags().save(exec, &mut tag, pivot)`, `save_many`, `create(exec, tag, pivot)`, `create_many`, `force_create` and quiet forms — Track: orm-relations
- [ ] **RWR-015** Touch parents on save and delete, and suspend touching inside a scope. — API: `#[model(touches(..))]`, `without_touching(fut)`, `without_touching_on::<(Company,)>(fut)`, `is_ignoring_touch::<M>()` — Track: orm-relations — Notes: parents are touched only when the child changed.
- [ ] **RWR-016** Attaching, detaching and syncing touch the parent when the relation is touched. — Track: orm-relations

### D5. Existence queries (`EXS`)

- [ ] **EXS-001** Records that have related records, through nested paths, with an optional count. — API: `has("activities")`, `has("company.owner")`, `has_count("activities", Op::Gte, 3)`, `or_has` — Track: orm-relations — Notes: `EXISTS` for "at least one", a count subquery otherwise.
- [ ] **EXS-002** Records without related records. — API: `doesnt_have(path)`, `or_doesnt_have(path)` — Track: orm-relations
- [ ] **EXS-003** Has related records matching conditions. — API: `where_has("activities", |q| ..)`, `or_where_has`, `where_has_count(path, f, Op, n)` — Track: orm-relations
- [ ] **EXS-004** Has no related records matching conditions. — API: `where_doesnt_have(path, f)`, `or_where_doesnt_have` — Track: orm-relations
- [ ] **EXS-005** Filter and eager-load with the same conditions. — API: `with_where_has(path, f)` — Track: orm-relations
- [ ] **EXS-006** One condition on a related column. — API: `where_relation("company", Company::NAME.eq(..))`, `or_where_relation`, `with_where_relation` — Track: orm-relations
- [ ] **EXS-007** No related record matching a column condition. — API: `where_doesnt_have_relation`, `or_where_doesnt_have_relation` — Track: orm-relations
- [ ] **EXS-008** Polymorphic existence for listed types, or every mapped type. — API: `has_morph(path, &[..])`, `or_has_morph`, `doesnt_have_morph`, `or_doesnt_have_morph`, `where_has_morph(path, types, |q, ty| ..)`, `or_where_has_morph`, `where_doesnt_have_morph`, `or_where_doesnt_have_morph` — Track: orm-relations
- [ ] **EXS-009** Polymorphic column conditions. — API: `where_morph_relation`, `or_where_morph_relation`, `where_morph_doesnt_have_relation`, `or_where_morph_doesnt_have_relation` — Track: orm-relations
- [ ] **EXS-010** A morph-to that points at a given record, or at any record of a type. — API: `where_morphed_to(path, &record)`, `where_not_morphed_to`, `or_where_morphed_to`, `or_where_not_morphed_to` — Track: orm-relations
- [ ] **EXS-011** Belongs to a given record or one of several. — API: `where_belongs_to(&company)`, `where_belongs_to_named(&company, "owner")`, `where_belongs_to_any(&companies)`, `or_where_belongs_to` — Track: orm-relations
- [ ] **EXS-012** Attached (many-to-many) to a given record or records. — API: `where_attached_to(&tag)`, `or_where_attached_to` — Track: orm-relations
- [ ] **EXS-013** Existence subqueries apply the related model's global scopes (soft deletes, workspace). — Track: orm-relations
- [ ] **EXS-014** Merge another query's constraints into a relation query. — API: `merge_constraints_from(q)` — Track: orm-relations

### D6. Eager loading and relation aggregates (`EAG`)

- [ ] **EAG-001** Eager load relation paths. — API: `with("company.owner")`, `with_all(["company", "activities"])` — Track: orm-relations — Notes: one batched `IN` query per level; integer keys inlined; no query when every key is empty.
- [ ] **EAG-002** Constrained eager loads. — API: `with_constrained("activities", |q| q.latest().limit(5))` — Track: orm-relations
- [ ] **EAG-003** Choose the columns of an eager-loaded relation in the path. — API: `with("company:id,name")` — Track: orm-relations — Notes: the keys needed for matching must be selected, otherwise an error names them.
- [ ] **EAG-004** Remove or replace eager loads. — API: `without("company")`, `with_only([..])`, `without_eager_load([..])`, `without_eager_loads()` — Track: orm-relations
- [ ] **EAG-005** Relation counts as `{name}_count`, with aliases and constraints. — API: `with_count("activities")`, `with_count_constrained("activities as open_count", |q| ..)` — Track: orm-relations
- [ ] **EAG-006** Relation aggregates. — API: `with_sum("items", "amount")`, `with_avg`, `with_min`, `with_max`, `with_exists`, `with_aggregate(path, col, Agg)` — Track: orm-relations — Notes: column `{relation}_{function}_{column}`; `with_exists` decodes as `bool`; values land in `extras`.
- [ ] **EAG-007** Eager load on a record after the fact. — API: `record.load(exec, "company.owner")`, `record.load_constrained(exec, path, f)`, `record.load_missing(exec, paths)` — Track: orm-relations
- [ ] **EAG-008** Relation aggregates on a record after the fact. — API: `load_count`, `load_sum`, `load_avg`, `load_min`, `load_max`, `load_exists`, `load_aggregate` — Track: orm-relations
- [ ] **EAG-009** Eager load and aggregates on a collection after the fact. — API: `collection.load(exec, ..)`, `load_missing`, `load_count`, `load_sum`, `load_avg`, `load_min`, `load_max`, `load_exists`, `load_aggregate` — Track: orm-relations
- [ ] **EAG-010** Nested eager loads per morph type. — API: `with_morph("subject", MorphWith::new().on::<Deal>(["company"]).on::<Contact>(["owner"]))`, `MorphWith::count_on::<Deal>(["notes"])` — Track: orm-relations
- [ ] **EAG-011** Per-type constraints on morph-to loads. — API: `MorphWith::constrain::<Deal>(|q| ..)` — Track: orm-relations
- [ ] **EAG-012** Morph loads after the fact. — API: `record.load_morph(exec, "subject", ..)`, `load_morph_count`, `load_morph_sum`, `load_morph_avg`, `load_morph_min`, `load_morph_max`, `load_morph_aggregate`; `collection.load_morph`, `collection.load_morph_count` — Track: orm-relations
- [ ] **EAG-013** Load a missing relation chain for the records of one type inside mixed collections. — API: `collection.load_missing_chain(..)` — Track: orm-relations
- [ ] **EAG-014** Loaded relations stored on records. — API: `record.relation::<T>(name)`, `set_relation`, `relation_loaded`, `unset_relation`, `relations()`, `set_relations`, `unset_relations`, `without_relations()`, `without_relation(name)` — Track: orm-relations
- [ ] **EAG-015** Eager loads respect the related model's global scopes and soft deletes; `with_trashed` works inside the constraint. — Track: orm-relations
- [ ] **EAG-016** Eager loading uses only batched `IN` queries, so it works on every driver, MongoDB included. — Track: orm-relations
- [ ] **EAG-017** Relation metadata by name, for tooling. — API: `M::relation_meta("company")` — Track: orm-relations

### D7. Polymorphism and the morph map (`MRP`)

- [ ] **MRP-001** Morph map from type names to models. — API: `OrmModule::morph_map(..)`, built from the `morph_name` of registered models — Track: orm-relations
- [ ] **MRP-002** Require a map entry for every polymorphic model. — API: `orm.require_morph_map = true` — Track: orm-relations — Notes: an unmapped model is `OrmError::MorphNotMapped { model }`.
- [ ] **MRP-003** Look up a model by name, and a name by model. — API: `MorphMap::model_for(name)`, `MorphMap::name_for::<M>()` — Track: orm-relations
- [ ] **MRP-004** Morph-to results are typed. — API: `Morphed` (an enum generated by `morph_types!`) or `AnyRecord::downcast::<M>()` — Track: orm-relations
- [ ] **MRP-005** Soft-delete visibility on morph-to loads, applied to every type. — API: `.with_trashed()`, `.without_trashed()`, `.only_trashed()` on `MorphTo` — Track: orm-relations
- [ ] **MRP-006** Morph id columns of integer, UUID or ULID type, matching the schema helpers (SCT-021). — Track: orm-relations
- [ ] **MRP-007** Changing a morph name is documented with a data migration that rewrites stored type names. — Track: orm-relations

## Part E. Lifecycle

### E1. Model events and observers (`EVT`)

- [ ] **EVT-001** The model events: `retrieved`, `creating`, `created`, `updating`, `updated`, `saving`, `saved`, `deleting`, `deleted`, `trashed`, `force_deleting`, `force_deleted`, `restoring`, `restored`, `replicating`. — API: `ModelEvent` — Track: orm-lifecycle
- [ ] **EVT-002** The "-ing" events can cancel the write. — API: listeners return `Flow::Cancel` → the write returns `Ok(false)` — Track: orm-lifecycle — Notes: cancellable: `saving`, `creating`, `updating`, `deleting`, `restoring`, `force_deleting`.
- [ ] **EVT-003** Observers: one async method per event, registered on the model or by a module. — API: `trait Observer<M>`, `#[model(observers(DealObserver))]`, `m.observe::<Deal>(DealObserver)` — Track: orm-lifecycle
- [ ] **EVT-004** Closure listeners per model and event. — API: `m.on_model::<Deal>(ModelEvent::Creating, f)` — Track: orm-lifecycle
- [ ] **EVT-005** Typed application events per model event, dispatched through navree-events. — API: `#[model(dispatches(created = DealCreated, deleted = DealDeleted))]` — Track: orm-lifecycle
- [ ] **EVT-006** Inside a unit of work, "-ing" events run in the transaction and "-ed" events run after commit; outside one they run at once. — API: `Uow::after_commit` — Track: orm-lifecycle
- [ ] **EVT-007** Observers that must run inside the transaction even for "-ed" events (to write related rows atomically). — API: `Observer::IN_TRANSACTION = true` — Track: orm-lifecycle
- [ ] **EVT-008** `retrieved` fires for every hydrated record, and costs nothing when no listener exists. — Track: orm-lifecycle
- [ ] **EVT-009** Run code with every model event suppressed. — API: `without_events(fut)` — Track: orm-lifecycle — Notes: task-local.
- [ ] **EVT-010** Custom model events beyond the built-in ones. — API: `ModelEvent::Custom(&'static str)`, `record.fire(exec, "archived")` — Track: orm-lifecycle
- [ ] **EVT-011** Listener inspection and flushing; swapping or removing the event dispatcher. — API: n/a → listeners belong to the app (each test boots its own); `EventFake` replaces dispatching in tests — Track: orm-lifecycle
- [ ] **EVT-012** Pivot events: attach, detach and pivot updates through a typed pivot model fire its model events. — Track: orm-relations
- [ ] **EVT-013** Mass updates and deletes fire no model events (documented, with the alternative of iterating records). — Track: orm-lifecycle
- [ ] **EVT-014** "-ed" listeners can see what changed (`was_changed`, `get_changes`, `get_original`). — Track: orm-lifecycle
- [ ] **EVT-015** Listener errors: an error in an "-ing" listener aborts the write and is returned; an error in an after-commit listener is logged and reported, never undoes the commit. — Track: orm-lifecycle

### E2. Soft deletes (`SFT`)

- [ ] **SFT-001** Deleting sets `deleted_at` (and `updated_at`) instead of removing the row, then fires `trashed`. — API: `record.delete(exec)` — Track: orm-lifecycle — Notes: the UPDATE itself is in core; this line covers the event order and `trashed`.
- [ ] **SFT-002** Queries hide soft-deleted rows by default. — API: the built-in `SoftDeletes` scope — Track: orm-core
- [ ] **SFT-003** Include, exclude or only show soft-deleted rows. — API: `with_trashed()`, `with_trashed_if(bool)`, `without_trashed()`, `only_trashed()` — Track: orm-lifecycle
- [ ] **SFT-004** Restore a record. — API: `record.restore(exec)`, `record.restore_quietly(exec)` — Track: orm-lifecycle — Notes: `restoring` (cancellable), clear `deleted_at`, save, `restored`.
- [ ] **SFT-005** Restore every matching row. — API: `Deal::query().only_trashed().where_(..).restore(exec)` — Track: orm-lifecycle
- [ ] **SFT-006** Force delete a record. — API: `record.force_delete(exec)`, `record.force_delete_quietly(exec)` — Track: orm-lifecycle — Notes: `force_deleting` (cancellable), `deleting`, delete the row, `deleted`, `force_deleted`.
- [ ] **SFT-007** Force delete every matching row. — API: `force_delete(exec)` on builders — Track: orm-lifecycle
- [ ] **SFT-008** Whether a record is soft-deleted. — API: `record.trashed()` — Track: orm-core
- [ ] **SFT-009** Restore-or-create and create-or-restore. — API: `restore_or_create(exec, match, values)`, `create_or_restore(exec, match, values)` — Track: orm-lifecycle
- [ ] **SFT-010** Whether a force delete is in progress, for listeners. — API: `record.is_force_deleting()` — Track: orm-lifecycle
- [ ] **SFT-011** On a model without soft deletes, force delete is a plain delete. — Track: orm-lifecycle
- [ ] **SFT-012** Soft deletes are honoured everywhere: route binding (opt-in `with_trashed`), relations and eager loads, through relations (`with_trashed_parents`), pruning (trashed rows included), factories (`trashed()` state), test assertions. — Track: orm-lifecycle — Notes: cross-checked by tests in each owning track.

### E3. Pruning (`PRN`)

- [ ] **PRN-001** Prunable models delete matching records one by one, with events and a `pruning` hook. — API: `impl Prunable for Deal { fn prunable(q: Builder<Self>) -> Builder<Self>; async fn pruning(&self, ctx) }`, `#[model(prunable)]` — Track: orm-lifecycle — Notes: chunked by key; soft-deleted rows included and force-deleted; a failing record is reported and the run continues.
- [ ] **PRN-002** Mass-prunable models delete each chunk with one statement, without events. — API: `#[model(mass_prunable)]`, `MassPrunable` — Track: orm-lifecycle
- [ ] **PRN-003** Prune one model in code and get the count. — API: `M::prune_all(exec, chunk_size)` — Track: orm-lifecycle
- [ ] **PRN-004** The pruning command. — API: `navree model:prune [--model M]... [--except M]... [--chunk 1000] [--pretend]` — Track: orm-lifecycle — Notes: `--pretend` prints how many rows each model would prune; models come from the registry (MOD-023).
- [ ] **PRN-005** Pruning events: starting and finished per run, rows pruned per model and chunk. — API: `ModelPruningStarting`, `ModelPruningFinished`, `ModelsPruned { model, count }` — Track: orm-lifecycle
- [ ] **PRN-006** Scheduled pruning documented with navree-schedule (daily by default in starter kits). — Track: orm-lifecycle

### E4. Strict modes (`STC`)

- [ ] **STC-001** One switch for every strict mode, on by default in development and tests. — API: `[orm] strict = true`, `OrmOptions::strict` — Track: orm-lifecycle
- [ ] **STC-002** Prevent lazy loading: loading a relation per record, for a record that came from a multi-record result, is an error. — API: `[orm] prevent_lazy_loading`, `record.relation_or_load::<T>(exec, name)` → `OrmError::LazyLoading { model, relation }` — Track: orm-relations — Notes: records fetched alone may still load lazily.
- [ ] **STC-003** Handle lazy-loading violations another way (log instead of fail). — API: `OrmModule::on_lazy_loading(f)` — Track: orm-relations
- [ ] **STC-004** Automatic eager loading: a lazy load on one record loads the relation for every record of the result it came from. — API: `[orm] auto_eager_load`, `collection.with_relationship_autoloading()`, `record.with_relationship_autoloading()` — Track: orm-relations
- [ ] **STC-005** Prevent silently discarding attributes that are not fillable. — API: `[orm] prevent_discarding_attributes` → `OrmError::MassAssignment { model, columns }` — Track: orm-lifecycle — Notes: `fill` (orm-attributes) consults this setting.
- [ ] **STC-006** Handle discarded attributes another way. — API: `OrmModule::on_discarded_attributes(f)` — Track: orm-lifecycle
- [ ] **STC-007** Prevent reading columns a partial select left out: a non-optional field missing from the row is an error instead of a default. — API: `[orm] prevent_missing_attributes` → `OrmError::MissingAttribute { model, column }` — Track: orm-lifecycle
- [ ] **STC-008** Handle missing attributes another way. — API: `OrmModule::on_missing_attribute(f)` — Track: orm-lifecycle

## Part F. Collections

### F1. Model collections (`COL`)

- [ ] **COL-001** A collection of records with iteration and conversions. — API: `Collection<M>`: `iter()`, `iter_mut()`, `IntoIterator`, `FromIterator`, `Extend`, `Index`, `len()`, `is_empty()`, `into_vec()` — Track: orm-core
- [ ] **COL-002** Find by key in memory. — API: `find(key)`, `find_many(&keys)`, `find_or_fail(key)` — Track: orm-core
- [ ] **COL-003** Contains a key, a record, or a record matching a predicate. — API: `contains(key)`, `contains_record(&r)`, `contains_where(f)`, `doesnt_contain(key)` — Track: orm-core
- [ ] **COL-004** All keys. — API: `model_keys()` — Track: orm-core
- [ ] **COL-005** Map, filter, reject, flat-map, for-each. — API: `map`, `filter`, `reject`, `flat_map`, `for_each` — Track: orm-core — Notes: `filter` keeps a `Collection<M>`; mapping to other types returns `Vec<T>`.
- [ ] **COL-006** Merge, diff, intersect and de-duplicate by key. — API: `merge(other)`, `diff(&other)`, `intersect(&other)`, `unique()`, `unique_by(f)` — Track: orm-core
- [ ] **COL-007** Keep or drop records by key. — API: `only(&keys)`, `except(&keys)` — Track: orm-core
- [ ] **COL-008** Key by, group by, count by, partition. — API: `key_by(f)`, `group_by(f)`, `count_by(f)`, `partition(f)` — Track: orm-core
- [ ] **COL-009** Pluck a column or a computed value, optionally keyed. — API: `pluck(col)`, `pluck_keyed(value_col, key_col)`, `pluck_with(f)` — Track: orm-core
- [ ] **COL-010** Records by key. — API: `dictionary()` → `HashMap<M::Key, Record<M>>` — Track: orm-core
- [ ] **COL-011** Sorting. — API: `sort_by(f)`, `sort_by_desc(f)`, `sort_by_column(col)`, `reverse()` — Track: orm-core
- [ ] **COL-012** Positional access. — API: `first()`, `first_or_fail()`, `last()`, `first_where(f)`, `nth(i)`, `get(i)`, `before(&r)`, `after(&r)`, `sole()`, `random()`, `shuffle()` — Track: orm-core
- [ ] **COL-013** Slicing and windows. — API: `take`, `skip`, `slice`, `chunk(n)`, `chunk_while(f)`, `split(n)`, `sliding(n)`, `for_page(page, per_page)`, `take_while`, `skip_while`, `take_until`, `skip_until` — Track: orm-core — Notes: many come from slices and iterators; listed so the docs show each mapping.
- [ ] **COL-014** Aggregates over a column. — API: `sum(col)`, `avg(col)`, `min(col)`, `max(col)`, `median(col)`, `mode(col)`, `percentage(f)` — Track: orm-core
- [ ] **COL-015** Column filters evaluated in memory (equality, in, between, null, strict comparisons). — API: `filter_where(Deal::STAGE.eq(Stage::Won))` — Track: orm-core — Notes: typed `Filter`s evaluated against loaded values; unsupported operators are errors.
- [ ] **COL-016** Duplicates. — API: `duplicates()`, `duplicates_by(f)` — Track: orm-core
- [ ] **COL-017** Folds, every, some, search. — API: `Iterator::fold`, `all`, `any`, `position` — Track: orm-core — Notes: covered by `Iterator`; documented.
- [ ] **COL-018** In-place changes: push, pop, prepend, put, pull, forget, splice, transform. — API: `push`, `pop`, `insert`, `remove`, `retain`, `iter_mut` — Track: orm-core
- [ ] **COL-019** Conditional pipelines on collections (`when`, `unless`, when empty, when not empty, pipe, tap). — API: n/a → plain Rust control flow — Track: orm-core
- [ ] **COL-020** Fresh copies of every record. — API: `collection.fresh(exec)`, `collection.fresh_with(exec, ["company"])` — Track: orm-core
- [ ] **COL-021** A builder matching exactly these records. — API: `collection.to_query()` → `Builder<M>` — Track: orm-core
- [x] **COL-022** Visibility and appends across a collection. — API: `make_hidden`, `merge_hidden`, `set_hidden`, `make_visible`, `merge_visible`, `set_visible`, `append`, `set_appends`, `without_appends` — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/serialize.rs::collections_serialize_as_arrays_and_change_every_record
- [ ] **COL-023** Operations whose results are not records (keys, flip, collapse, flatten, pad, zip, combine, dot and undot). — API: n/a → they return std types (`Vec`, `HashMap`), never `Collection<M>` — Track: orm-core
- [ ] **COL-024** Join a column into a string. — API: `implode(col, glue)` — Track: orm-core
- [ ] **COL-025** Higher-order proxies that call a method on every item. — API: n/a → closures (`for r in &mut c { .. }`) — Track: orm-core
- [ ] **COL-026** Generic collection constructors and type helpers (ranges, repeat, wrap and unwrap, from JSON, type assertions, instance-of filters). — API: n/a → `Collection<M>` comes from queries or `FromIterator`, and its element type is fixed — Track: orm-core

## Part G. Factories, seeders, testing and integration

### G1. Factories (`FAC`)

- [ ] **FAC-001** A factory per model with a definition. — API: `impl ModelFactory for DealFactory { type Model = Deal; fn definition(&self, ctx: &mut FactoryContext) -> Deal }`, `#[model(factory = DealFactory)]`, `Deal::factory()` — Track: orm-factories — Notes: `FactoryContext` gives the sequence index and fake data (`ctx.fake()`), seedable for reproducible tests.
- [ ] **FAC-002** How many records to build. — API: `.count(n)` — Track: orm-factories — Notes: a separate "times" constructor is n/a (`Deal::factory().count(n)`).
- [ ] **FAC-003** States: closures and named state functions, appended or prepended. — API: `.state(|d| d.stage = Stage::Won)`, `.state(DealFactory::won)`, `.prepend_state(..)` — Track: orm-factories
- [ ] **FAC-004** Set one column. — API: `.set(Deal::TITLE, "Acme")` — Track: orm-factories
- [ ] **FAC-005** Sequences, including one record per sequence item and cross-joined sequences. — API: `.sequence([..])`, `Sequence::new([..])` (index and count available to closures), `.for_each_sequence([..])`, `.cross_join_sequence([..], [..])` — Track: orm-factories
- [ ] **FAC-006** Child records through a has-many or has-one relation. — API: `.has(Activity::factory().count(3), "activities")` — Track: orm-factories — Notes: relation-named builder methods are n/a; `has(factory, name)` with an optional typed helper from `#[relations]`.
- [ ] **FAC-007** Many-to-many records attached with pivot values. — API: `.has_attached(Tag::factory().count(2), pivot_values, "tags")` — Track: orm-factories
- [ ] **FAC-008** Parent records through a belongs-to or morph-to relation, new or existing. — API: `.for_(Company::factory(), "company")`, `.for_record(&company, "company")` — Track: orm-factories — Notes: `for` is a reserved word.
- [ ] **FAC-009** Reuse existing records for every relation that needs one of their type. — API: `.recycle(&companies)` — Track: orm-factories — Notes: a random pick per created record.
- [ ] **FAC-010** After-making and after-creating hooks, removable, plus a configure hook on the factory. — API: `.after_making(f)`, `.after_creating(f)`, `.without_after_making()`, `.without_after_creating()`, `ModelFactory::configure` — Track: orm-factories
- [ ] **FAC-011** Build unsaved records. — API: `.make()` → `Collection<M>`, `.make_one()`, `.make_many(n)` — Track: orm-factories
- [ ] **FAC-012** Create records with their relations. — API: `.create(exec)`, `.create_one(exec)`, `.create_many(exec, n)` — Track: orm-factories
- [ ] **FAC-013** Create without model events. — API: `.create_quietly(exec)`, `.create_one_quietly(exec)`, `.create_many_quietly(exec, n)` — Track: orm-factories
- [ ] **FAC-014** Generated attributes only. — API: `.raw()` → `Vec<Attributes>` — Track: orm-factories
- [ ] **FAC-015** Deferred creation for use inside other definitions. — API: `.lazy()` → `LazyFactory` — Track: orm-factories
- [ ] **FAC-016** Bulk insert without events or relations. — API: `.insert(exec)` — Track: orm-factories
- [ ] **FAC-017** Soft-deleted state. — API: `.trashed()`, `.trashed_at(ts)` — Track: orm-factories
- [ ] **FAC-018** Do not create parent records referenced by a definition. — API: `.without_parents()`, `FactoryOptions::expand_relationships(false)` — Track: orm-factories
- [ ] **FAC-019** Create on a named connection. — API: `.connection("analytics")` — Track: orm-factories
- [ ] **FAC-020** Finding a model's factory, or a factory's model, by naming convention; flushing global factory state. — API: n/a → factories and models are linked by types; state lives in the app — Track: orm-factories
- [ ] **FAC-021** The existing `navree_db::Factory` and `create_many` keep working beside model factories. — Track: orm-factories

### G2. Seeders (`SED`)

- [ ] **SED-001** Seeders as types registered per module, running in module order. — API: `navree_db::Seeder`, `run_seeders` — Track: orm-factories — Notes: exists; examples move to model factories.
- [ ] **SED-002** A seeder calls others: once, silently, with parameters. — API: `ctx.call::<S>()`, `ctx.call_once::<S>()`, `ctx.call_silent::<S>()`, `ctx.call_with::<S>(params)` — Track: orm-factories
- [ ] **SED-003** Seeders that run without model events. — API: `Seeder::WITHOUT_MODEL_EVENTS = true` — Track: orm-factories
- [ ] **SED-004** The seed command. — API: `navree db:seed [name] [--database D] [--force]` — Track: orm-factories — Notes: a root seeder runs by default; production requires `--force`.

### G3. Testing (`TST`)

- [ ] **TST-001** Assert a table has, or lacks, rows matching values. — API: `assert_database_has(exec, "deals", [("title", v)])`, `assert_database_missing(..)`, typed forms `assert_database_has::<Deal>(exec, filter)` — Track: orm-factories — Notes: failures list the closest rows.
- [ ] **TST-002** Assert row counts. — API: `assert_database_count(exec, table, n)`, `assert_database_empty(exec, table)` — Track: orm-factories
- [ ] **TST-003** Assert a record is, or is not, soft-deleted. — API: `assert_soft_deleted(exec, &record)`, `assert_not_soft_deleted(exec, &record)` (also by table and values) — Track: orm-factories
- [ ] **TST-004** Assert a record exists or is gone. — API: `assert_model_exists(exec, &record)`, `assert_model_missing(exec, &record)` — Track: orm-factories
- [ ] **TST-005** Expect an exact number of queries. — API: `expect_query_count(n, fut)` — Track: orm-factories
- [ ] **TST-006** Compare JSON columns structurally in assertions. — API: `json_value(v)` — Track: orm-factories
- [ ] **TST-007** Run a seeder from a test. — API: `app.seed::<S>()` — Track: orm-factories
- [ ] **TST-008** A fresh database per test, cloned from a template migrated once. — API: `#[navree::test]` — Track: orm-factories — Notes: SQLite file copy, PostgreSQL `CREATE DATABASE … TEMPLATE`, MySQL schema copy.
- [ ] **TST-009** Transaction-per-test strategy, with after-commit callbacks still running. — API: `TestDb::transactional()` — Track: orm-factories
- [ ] **TST-010** Truncation strategy with excluded tables. — API: `TestDb::truncating(&["navree_migrations"])` — Track: orm-factories
- [ ] **TST-011** Refresh the database only for tests that touch it. — API: lazy `TestDb` — Track: orm-factories
- [ ] **TST-012** In-memory SQLite for fast tests. — API: `sqlite::memory:` support in the test harness — Track: orm-factories
- [ ] **TST-013** Real-server tests gated by `NAVREE_TEST_POSTGRES_URL`, `NAVREE_TEST_MYSQL_URL` and `NAVREE_TEST_MONGODB_URL`, loaded from `target/test-databases.env` when present, skipped with a notice otherwise. — Track: orm-factories
- [ ] **TST-014** SQL snapshots per dialect for every builder feature (statement and values for PostgreSQL, MySQL and SQLite). — Track: orm-core

### G4. Integration (`INT`)

- [ ] **INT-001** Route model binding by key. — API: `Bound<M>` extractor — Track: orm-factories — Notes: 404 in Navree's error shape; a malformed UUID or ULID is a 404 without a query.
- [ ] **INT-002** Bind by another column, per model or per route. — API: `#[model(route_key = "slug")]`, `Bound<M, K>` with a key marker type — Track: orm-factories
- [ ] **INT-003** Scoped child binding: `/companies/{company}/deals/{deal}` finds the deal through the company's relation. — API: `BoundChild<Company, Deal>` (relation `Company::deals`) — Track: orm-factories
- [ ] **INT-004** Binding soft-deleted records on routes that opt in. — API: `#[bind(with_trashed)]` — Track: orm-factories
- [ ] **INT-005** Custom binding queries. — API: `trait RouteBinding { fn route_binding_query(q: Builder<Self>, value: &str) -> Builder<Self> }` — Track: orm-factories
- [x] **INT-006** Records in queued jobs: serialize the identity (key, loaded relation names, connection) and reload it when the job runs. — API: `ModelRef<M>`, `ModelRefs<M>`, `ModelRef::restore(exec)` — Track: orm-attributes — Notes: a missing record fails the job with a clear error, or skips it when the job opts in. — Tests: crates/navree-orm/tests/orm/attributes/refs.rs::identities_round_trip_through_a_job_and_restore_records, crates/navree-orm/tests/orm/attributes/refs.rs::missing_records_fail_clearly_or_are_skipped_on_request
- [ ] **INT-007** Broadcast model changes (created, updated, trashed, restored, deleted) on realtime channels. — API: `#[model(broadcasts)]`, `trait Broadcasts { fn broadcast_on(..); fn broadcast_as(..); fn broadcast_with(..); fn broadcast_queue(..) }`, `without_broadcasting(fut)` — Track: orm-lifecycle — Notes: after commit, through navree-realtime, queued or immediate; default channel from the model name and key.
- [ ] **INT-008** Validation rules that query models: unique (ignoring the current record, with extra conditions) and exists. — API: navree-validation `unique::<Deal>(Deal::SLUG).ignore(id).where_(..)`, `exists::<Company>(Company::ID)` — Track: orm-factories
- [ ] **INT-009** Show a model: table, columns with types and casts, fillable and hidden flags, appended values, relations, events, observers, policy. — API: `navree model:show <Model> [--json]` — Track: orm-factories
- [ ] **INT-010** Generate a model and its companions. — API: `navree make:model <module> <Name> [--migration] [--factory] [--seeder] [--controller] [--policy] [--resource] [--pivot] [--all]` — Track: orm-factories
- [ ] **INT-011** Generate a factory and a seeder. — API: `navree make:factory <module> <Name> [--model M]`, `navree make:seeder <module> <Name>` — Track: orm-factories

## Part H. Connections, transactions and errors

### H1. Connections and raw statements (`CON`)

- [ ] **CON-001** One default database plus named connections, and a list of every configured connection. — API: `Db`, named `Db` providers (`[database.connections.analytics]`, `app.get_named::<Db>("analytics")`), `DatabaseModule::connections()` — Track: orm-streams
- [ ] **CON-002** Queries and models on a named connection. — API: `#[model(connection = "analytics")]`, passing that `Db` as the executor, `on_connection("analytics")` — Track: orm-streams
- [ ] **CON-003** Change the default connection inside a scope. — API: `using_connection("analytics", fut)` — Track: orm-streams — Notes: task-local.
- [ ] **CON-004** Connect at runtime from options. — API: `Db::connect(&DbOptions)` — Track: orm-streams — Notes: exists; covered by tests for every driver.
- [ ] **CON-005** Close a pool; lost connections are replaced by the pool. — API: `Db::close()` — Track: orm-streams
- [ ] **CON-006** Read replicas with sticky writes. — API: `[database] read_urls = [..]`, `sticky = true`, `use_write()`, `M::on_write_connection()`, `using_writer(fut)` — Track: orm-streams — Notes: after a write in a unit of work its reads use the writer; reads inside a transaction always do.
- [ ] **CON-007** A direct connection that bypasses a transaction-mode pooler, for migrations, advisory locks and `LISTEN`. — API: `[database] direct_url` — Track: orm-streams
- [ ] **CON-008** Drivers chosen by URL scheme and enabled by cargo features. — API: `sqlite:` built in, `postgres://` (`postgres`), `mysql://` and `mariadb://` (`mysql`), `mongodb://` (`navree-orm-mongodb`) — Track: orm-streams — Notes: custom SQL drivers and resolver extension points are n/a; document stores plug in through `DocumentDriver`.
- [ ] **CON-009** Connection URLs with credentials, host, port, database and options, redacted in logs. — API: `DbOptions::url`, `DbOptions::redacted_url()` — Track: orm-streams
- [ ] **CON-010** Pool settings: max and min connections, connect timeout, idle timeout, max lifetime. — API: `DbOptions` — Track: orm-streams
- [ ] **CON-011** PostgreSQL session settings: search path or schema, timezone, application name, synchronous commit, isolation level, SSL mode. — API: `[database.postgres]` — Track: orm-streams
- [ ] **CON-012** MySQL and MariaDB session settings: charset and collation, timezone, strict mode and SQL modes, isolation level, unix socket, SSL. — API: `[database.mysql]` (`strict` exists) — Track: mysql
- [ ] **CON-013** SQLite settings: WAL, busy timeout, foreign keys, synchronous, extra pragmas, in-memory databases, creating the file. — API: `[database.sqlite]` — Track: orm-streams — Notes: WAL, busy timeout and foreign keys exist; this line adds the rest.
- [ ] **CON-014** Raw selects returning rows, one row, or one value. — API: `navree_orm::select(exec, raw(..))`, `select_one`, `scalar::<T>` — Track: orm-core — Notes: reading from the writer goes through `use_write()` / `using_writer(fut)`.
- [ ] **CON-015** Raw statements: run, count affected rows, run several unprepared statements. — API: `navree_orm::statement(exec, raw(..))`, `affecting_statement`, `unprepared(exec, sql)` — Track: orm-core
- [ ] **CON-016** Several result sets from one call. — API: n/a → call procedures per result set; documented — Track: orm-core
- [ ] **CON-017** Dry run: collect the statements a closure would write. — API: `pretend(fut)` → `(T, Vec<Statement>)`, `without_pretending(fut)`, `is_pretending()` — Track: orm-core — Notes: writes are recorded, reads still run; used by `--pretend` options.
- [ ] **CON-018** Capture the queries of a scope, with SQL as sent or with values inlined. — API: `QueryLog::capture(fut)` → `Vec<LoggedQuery { sql, values, duration, connection }>`, `LoggedQuery::raw_sql()`; `[database] log_queries` — Track: orm-streams
- [ ] **CON-019** Listen to every executed statement. — API: `Db::on_query(f)` (`QueryExecuted { sql, duration, rows, connection }`) — Track: orm-streams — Notes: navree-telemetry spans use the same hook; values are not logged by default.
- [ ] **CON-020** React when the queries of one unit of work take longer than a threshold in total. — API: `when_querying_for_longer_than(Duration, f)`, `Uow::total_query_duration()`, `reset_total_query_duration()`, `allow_duration_handlers_again()` — Track: orm-streams
- [ ] **CON-021** Hooks before a statement runs and before a transaction begins. — API: `Db::before_executing(f)`, `Db::before_starting_transaction(f)` — Track: orm-streams — Notes: a statement-prepared event is n/a; `before_executing` replaces it.
- [ ] **CON-022** Pool statistics and the server's open connection count. — API: `Db::pool_stats()`, `Db::open_connections(exec)` — Track: orm-streams
- [ ] **CON-023** Quote a literal safely for a dialect. — API: `Dialect::quote_literal(&Value)` — Track: orm-core
- [ ] **CON-024** Server facts. — API: `Db::backend()`, `Db::server_version()`, `Db::database_name()`, `Db::is_mariadb()`, `Db::has_timescaledb()` — Track: orm-streams
- [ ] **CON-025** Table prefixes per connection. — API: n/a → PostgreSQL schemas or a separate database; prefixes would leak into raw SQL, row-level security policies and hypertable names — Track: orm-streams
- [ ] **CON-026** Detect lost connections and retry outside transactions. — API: `OrmError::is_lost_connection()` — Track: orm-streams — Notes: never retried inside a transaction.
- [ ] **CON-027** Connection events: established, and busy (open connections above a limit). — API: `ConnectionEstablished`, `DatabaseBusy { connection, open }` — Track: orm-streams
- [ ] **CON-028** Remember whether the unit of work wrote anything. — API: `Uow::has_modified_records()` — Track: orm-streams
- [ ] **CON-029** Analytics workloads on a MySQL secondary engine. — API: n/a → run the session setting through `statement` — Track: mysql
- [ ] **CON-030** Low-level handles: raw driver connections, separate read and write handles, reconnector callbacks, swapping grammars, processors or the resolver models use to find connections. — API: n/a → the pool owns connections; executors are explicit; tests boot their own app — Track: orm-streams
- [ ] **CON-031** Use the ORM without booting a Navree app (scripts, one-off tools). — API: `Db::connect(&DbOptions)` and executors work standalone; features that need the app (observers, encrypted casts, the morph map) say so in their errors — Track: orm-core

### H2. Transactions (`TXN`)

- [ ] **TXN-001** Run a closure in a transaction: commit on `Ok`, roll back on `Err` or panic. — API: `db.transaction(|uow| async { .. })` — Track: orm-core — Notes: exists; joins the current unit of work when there is one.
- [ ] **TXN-002** Retry on deadlocks and serialization failures. — API: `db.transaction_with(TransactionOptions::new().attempts(3), f)` — Track: orm-core — Notes: only concurrency errors are retried, including a commit that fails on one; a lost connection is never retried, and a nested transaction passes the error to the outermost one.
- [ ] **TXN-003** Nested transactions as savepoints. — API: `uow.savepoint(|uow| ..)`, nested `transaction` calls — Track: orm-core
- [ ] **TXN-004** Begin, commit and roll back by hand; roll back to a level; read the level. — API: `db.uow(..)`, `uow.commit()`, `uow.rollback()`, `uow.rollback_to(level)`, `uow.transaction_level()` — Track: orm-core
- [ ] **TXN-005** After-commit callbacks. — API: `uow.after_commit(f)`, `navree_orm::after_commit(f)` (current unit of work) — Track: orm-core — Notes: run at once when no transaction is open; callbacks registered inside a savepoint run when the outermost transaction commits and are dropped if that savepoint rolls back.
- [ ] **TXN-006** After-rollback callbacks. — API: `uow.after_rollback(f)` — Track: orm-core
- [ ] **TXN-007** Isolation levels and read-only transactions. — API: `TransactionOptions::isolation(Isolation::Serializable)`, `.read_only()` — Track: orm-core
- [ ] **TXN-008** Transaction events. — API: `TransactionBeginning`, `TransactionCommitting`, `TransactionCommitted`, `TransactionRolledBack` — Track: orm-core
- [ ] **TXN-009** A savepoint around statements expected to fail inside a transaction (unique-violation fallbacks), so PostgreSQL keeps the transaction usable. — API: internal `with_savepoint_if_needed` — Track: orm-core
- [ ] **TXN-010** MySQL commits implicitly on DDL: migrations there are not transactional, and the migrator says so. — Track: mysql

### H3. Errors (`ERR`)

- [ ] **ERR-001** Not found, with model and keys; 404 over HTTP. — API: `OrmError::ModelNotFound { model, keys }` — Track: orm-core
- [ ] **ERR-002** Several rows where one was expected. — API: `OrmError::MultipleRecordsFound { model, count }` — Track: orm-core
- [ ] **ERR-003** Unique violations with the constraint and columns when the database reports them; 409 over HTTP. — API: `OrmError::is_unique_violation()`, `unique_violation()` → `{ constraint, columns }` — Track: orm-core
- [ ] **ERR-004** Foreign-key violations; 409 over HTTP. — API: `OrmError::is_foreign_key_violation()` — Track: orm-core
- [ ] **ERR-005** Deadlocks and serialization failures recognised on every dialect. — API: `OrmError::is_concurrency_error()` — Track: orm-core
- [ ] **ERR-006** Database errors keep the SQL and connection name for logs, mask values, and never show SQL to HTTP clients. — API: `OrmError::Db` with its source chain — Track: orm-core
- [ ] **ERR-007** Decode errors name the model, column and selected columns. — API: `OrmError::Decode { model, column, message }` — Track: orm-core
- [ ] **ERR-008** Invalid queries and unsupported features name the clause, feature and driver. — API: `OrmError::InvalidQuery`, `OrmError::Unsupported { feature, driver }` — Track: orm-core
- [ ] **ERR-009** Operations that need a stored record or a key. — API: `OrmError::MissingKey { model }`, `OrmError::NotPersisted { model, action }` — Track: orm-core
- [ ] **ERR-010** One value expected but several columns selected (`value`, `pluck`). — API: `OrmError::InvalidQuery` — Track: orm-core
- [ ] **ERR-011** Not-found and several-found errors for table queries without a model. — API: `OrmError::RecordNotFound`, `OrmError::MultipleRecordsFound { model: None, .. }` — Track: orm-core
- [ ] **ERR-012** Statement timeouts. — API: `OrmError::Timeout` — Track: orm-streams

## Part I. Schema builder (navree-schema)

### I1. Tables and introspection (`SCH`)

- [ ] **SCH-001** Create a table. — API: `schema.create("deals", |t| ..)` — Track: schema
- [ ] **SCH-002** Alter a table. — API: `schema.table("deals", |t| ..)` — Track: schema — Notes: in alter mode each column and command runs in the order written.
- [ ] **SCH-003** Rename a table. — API: `schema.rename(from, to)`, `t.rename(to)` — Track: schema
- [ ] **SCH-004** Drop a table, or drop it if it exists. — API: `schema.drop(t)`, `schema.drop_if_exists(t)`, `t.drop()`, `t.drop_if_exists()` — Track: schema
- [ ] **SCH-005** Drop columns by name. — API: `schema.drop_columns(table, cols)` — Track: schema
- [ ] **SCH-006** Table options: storage engine, InnoDB shortcut, charset, collation, temporary tables, table comments. — API: `t.engine("InnoDB")`, `t.innodb()`, `t.charset(..)`, `t.collation(..)`, `t.temporary()`, `t.comment(..)` — Track: schema — Notes: engine and charset are MySQL; comments on PostgreSQL and MySQL; SQLite ignores comments.
- [ ] **SCH-007** Existence checks. — API: `has_table`, `has_view`, `has_column`, `has_columns`, `has_index(table, index, kind)`, `has_foreign_key` — Track: schema
- [ ] **SCH-008** Listings of tables (name, schema, size, comment, collation, engine), table names, views, user-defined types and schemas. — API: `tables()`, `table_listing()`, `views()`, `types()`, `schemas()` — Track: schema
- [ ] **SCH-009** Column introspection: name, type and type name, nullability, default, auto increment, comment, collation, generation expression. — API: `columns(table)`, `column(table, col)`, `column_listing(table)`, `column_type(table, col, full)` — Track: schema
- [ ] **SCH-010** Index and foreign-key introspection. — API: `indexes(table)`, `index_listing(table)`, `foreign_keys(table)` — Track: schema
- [ ] **SCH-011** Alterations that run only when a column or index exists, or does not. — API: `when_table_has_column`, `when_table_doesnt_have_column`, `when_table_has_index`, `when_table_doesnt_have_index` — Track: schema
- [ ] **SCH-012** Drop every table, view, or user-defined type and domain. — API: `drop_all_tables()`, `drop_all_views()`, `drop_all_types()` — Track: schema — Notes: TimescaleDB catalogs and extension objects are left alone.
- [ ] **SCH-013** Turn foreign-key enforcement off and on, or off for a closure. — API: `disable_foreign_key_constraints()`, `enable_foreign_key_constraints()`, `without_foreign_key_constraints(fut)` — Track: schema
- [ ] **SCH-014** Create or drop a database, and empty a SQLite database file. — API: `create_database(name)`, `drop_database_if_exists(name)`, `refresh_database_file(path)` — Track: schema — Notes: SQLite creates or deletes the file.
- [ ] **SCH-015** Install extensions when missing. — API: `ensure_extension("timescaledb")`, `ensure_vector_extension()` — Track: schema
- [ ] **SCH-016** Defaults for migrations: string length, time precision, morph key type. — API: `SchemaOptions { default_string_length, default_time_precision, morph_key_type }`, `morph_using_uuids()`, `morph_using_ulids()` — Track: schema
- [ ] **SCH-017** Schema-qualified names and the current schema. — API: `"reporting.deals"`, `current_schema()`, `current_schema_listing()` — Track: schema
- [ ] **SCH-018** Read and set SQLite pragmas. — API: `schema.pragma(key)`, `schema.set_pragma(key, value)` — Track: schema
- [ ] **SCH-019** Compile schema changes without a database (previews, docs, snapshot tests). — API: `Schema::offline(Target)`, `schema.statements()` — Track: schema
- [ ] **SCH-020** Notices for every fallback (a feature the database lacks). — API: `schema.notices()` — Track: schema
- [ ] **SCH-021** Add a group of columns after a given column. — API: `t.after("title", |t| ..)` — Track: schema
- [ ] **SCH-022** Blueprint contents as public data, so other compilers (MongoDB) can read them. — API: `Blueprint::columns()`, `commands()`, `added_columns()`, `changed_columns()` — Track: schema
- [ ] **SCH-023** Raw schema statements. — API: `schema.statement(sql)` — Track: schema
- [ ] **SCH-024** Swapping in a custom blueprint type. — API: n/a → extension traits on `Blueprint` — Track: schema
- [ ] **SCH-025** Target detection with version checks that fail with precise errors instead of bad SQL. — API: `Target::postgres("17").with_timescaledb("2.17")`, `Target::mysql("8.4")`, `Target::mariadb("11.4")`, `Target::sqlite("3.46")` — Track: schema

### I2. Column types (`SCT`)

- [ ] **SCT-001** Auto-increment keys. — API: `t.id()`, `big_increments`, `increments`, `integer_increments`, `medium_increments`, `small_increments`, `tiny_increments` — Track: schema — Notes: PostgreSQL identity columns, MySQL unsigned `AUTO_INCREMENT`, SQLite `INTEGER PRIMARY KEY AUTOINCREMENT`.
- [ ] **SCT-002** UUID and ULID primary keys. — API: `t.uuid_primary()`, `t.ulid_primary()` — Track: schema
- [ ] **SCT-003** Integers of every size, optionally auto-increment or unsigned. — API: `tiny_integer`, `small_integer`, `medium_integer`, `integer`, `big_integer` — Track: schema
- [ ] **SCT-004** Unsigned integers. — API: `unsigned_tiny_integer`, `unsigned_small_integer`, `unsigned_medium_integer`, `unsigned_integer`, `unsigned_big_integer` — Track: schema — Notes: unsigned only on MySQL; PostgreSQL and SQLite store signed values (ranges documented).
- [ ] **SCT-005** Foreign id columns. — API: `foreign_id("company_id")`, `foreign_uuid(..)`, `foreign_ulid(..)` → `ForeignIdColumn` — Track: schema
- [ ] **SCT-006** Foreign id columns for a model, typed and named from the model. — API: `t.foreign_id_for::<Company>()`, `foreign_uuid_for::<M>()`, `foreign_ulid_for::<M>()` — Track: schema — Notes: through a small trait defined in navree-schema and implemented by the model derive, so navree-schema does not depend on the ORM.
- [ ] **SCT-007** Strings and text. — API: `char(name, len)`, `string(name, len)`, `tiny_text`, `text`, `medium_text`, `long_text` — Track: schema
- [ ] **SCT-008** Floats and decimals. — API: `float(name, precision)`, `double`, `decimal(name, total, places)` — Track: schema
- [ ] **SCT-009** Booleans. — API: `boolean` — Track: schema
- [ ] **SCT-010** Enumerations and sets. — API: `enumeration(name, values)`, `set(name, values)` — Track: schema — Notes: `enum` is a reserved word; `set` is MySQL only and fails elsewhere with a hint.
- [ ] **SCT-011** JSON. — API: `json`, `jsonb` — Track: schema
- [ ] **SCT-012** Dates and times, with precision and time zones. — API: `date`, `date_time(name, precision)`, `date_time_tz`, `time`, `time_tz`, `timestamp`, `timestamp_tz`, `year` — Track: schema
- [ ] **SCT-013** Timestamp pairs. — API: `timestamps(precision)`, `nullable_timestamps`, `timestamps_tz`, `nullable_timestamps_tz`, `datetimes` — Track: schema
- [ ] **SCT-014** Soft-delete columns. — API: `soft_deletes(col, precision)`, `soft_deletes_tz`, `soft_deletes_datetime` — Track: schema
- [ ] **SCT-015** Binary, fixed or variable length. — API: `binary(name, length, fixed)` — Track: schema
- [ ] **SCT-016** UUID and ULID columns. — API: `uuid(name)`, `ulid(name, length)` — Track: schema
- [ ] **SCT-017** Network addresses. — API: `ip_address`, `mac_address` — Track: schema
- [ ] **SCT-018** Spatial types with subtype and SRID. — API: `geometry(name, subtype, srid)`, `geography(name, subtype, srid)` — Track: schema — Notes: PostGIS on PostgreSQL.
- [ ] **SCT-019** Computed columns. — API: `.virtual_as(expr)`, `.stored_as(expr)` — Track: schema — Notes: a separate computed-column type is n/a; the generated-column modifiers cover it.
- [ ] **SCT-020** Vectors and text-search vectors. — API: `vector(name, dimensions)`, `tsvector(name)` — Track: schema
- [ ] **SCT-021** Polymorphic column pairs with their index. — API: `morphs`, `nullable_morphs`, `numeric_morphs`, `nullable_numeric_morphs`, `uuid_morphs`, `nullable_uuid_morphs`, `ulid_morphs`, `nullable_ulid_morphs`, `morphs_with_index(name, index)` — Track: schema — Notes: the default key type comes from `SchemaOptions::morph_key_type`.
- [ ] **SCT-022** Remember-token column. — API: `remember_token()`, `drop_remember_token()` — Track: schema — Notes: kept for completeness; Navree auth keeps hashed tokens in its own tables (documented).
- [ ] **SCT-023** Raw column definitions and adding a column by type. — API: `raw_column(name, definition)`, `add_column(ColumnType, name)` — Track: schema

### I3. Column modifiers (`SMD`)

- [ ] **SMD-001** Nullable. — API: `.nullable()` — Track: schema
- [ ] **SMD-002** Typed defaults and expression defaults. — API: `.default(v)`, `.default(Expression::raw(".."))` — Track: schema — Notes: rendered per dialect; MySQL wraps defaults of `TEXT`, `BLOB`, `JSON` and geometry columns in parentheses (8.0.13+).
- [ ] **SMD-003** Current-timestamp default, and MySQL's on-update timestamp. — API: `.use_current()`, `.use_current_on_update()` — Track: schema
- [ ] **SMD-004** Unsigned. — API: `.unsigned()` — Track: schema
- [ ] **SMD-005** Auto increment and its starting value. — API: `.auto_increment()`, `.starting_value(n)`, `.from(n)` — Track: schema — Notes: MySQL `AUTO_INCREMENT = n`; PostgreSQL identity `START WITH` or a sequence reset.
- [ ] **SMD-006** Column comments. — API: `.comment(..)` — Track: schema — Notes: PostgreSQL `COMMENT ON`, MySQL inline.
- [ ] **SMD-007** Collation and charset. — API: `.collation(..)`, `.charset(..)` — Track: schema
- [ ] **SMD-008** Column position. — API: `.after(col)`, `.first()` — Track: schema — Notes: MySQL only.
- [ ] **SMD-009** Generated columns. — API: `.virtual_as(expr)`, `.stored_as(expr)` — Track: schema — Notes: version checks (virtual generated columns need PostgreSQL 18).
- [ ] **SMD-010** Identity columns. — API: `.generated_as(..)`, `.always()` — Track: schema — Notes: PostgreSQL.
- [ ] **SMD-011** Invisible columns. — API: `.invisible()` — Track: schema — Notes: MySQL 8.0.23+.
- [ ] **SMD-012** Change an existing column, optionally to another type, with a conversion expression. — API: `.change()`, `.column_type(ColumnType)`, `.using(expr)` — Track: schema — Notes: the full new definition is given; SQLite rebuilds; PostgreSQL uses `USING`.
- [ ] **SMD-013** Inline indexes on a column. — API: `.primary()`, `.unique()`, `.index()`, `.fulltext()`, `.spatial_index()`, `.vector_index()` (with optional names) — Track: schema
- [ ] **SMD-014** MySQL online DDL options for column changes. — API: `.instant()`, `.lock(DdlLock)` — Track: schema
- [ ] **SMD-015** Persisted computed columns of other engines. — API: n/a → `.stored_as(expr)` — Track: schema

### I4. Indexes and foreign keys (`SIX`)

- [ ] **SIX-001** Primary keys, composite and named. — API: `t.primary(cols)`, `t.primary_named(cols, name)` — Track: schema
- [ ] **SIX-002** Unique indexes, with PostgreSQL deferrable and `NULLS NOT DISTINCT` options. — API: `t.unique(cols)`, `t.unique_named(..)`, `.deferrable()`, `.initially_immediate()`, `.nulls_not_distinct()` — Track: schema
- [ ] **SIX-003** Plain indexes with an access method. — API: `t.index(cols)`, `t.index_named(..)`, `.algorithm("gin")` — Track: schema
- [ ] **SIX-004** Full-text indexes, with a language. — API: `t.fulltext(cols)`, `.language("english")` — Track: schema — Notes: PostgreSQL GIN over `to_tsvector`; MySQL `FULLTEXT`; SQLite error with a hint.
- [ ] **SIX-005** Spatial indexes, with an operator class. — API: `t.spatial_index(cols)`, `.operator_class(..)` — Track: schema
- [ ] **SIX-006** Vector indexes. — API: `t.vector_index(col)` — Track: schema — Notes: pgvector HNSW; MariaDB vector index.
- [ ] **SIX-007** Indexes on raw expressions. — API: `t.raw_index(expr, name)` — Track: schema
- [ ] **SIX-008** Build indexes without locking writes. — API: `.online()` — Track: schema — Notes: PostgreSQL `CREATE INDEX CONCURRENTLY`; the migration must opt out of its transaction (MIG-006).
- [ ] **SIX-009** MySQL index and foreign-key operation options. — API: `.inplace()`, `.lock(DdlLock)` — Track: schema
- [ ] **SIX-010** Index names generated the same way on every database, long names shortened with a hash. — API: `Schema::index_name(table, kind, cols)` — Track: schema
- [ ] **SIX-011** Drop indexes by name or by columns. — API: `drop_primary`, `drop_unique`, `drop_index`, `drop_fulltext`, `drop_spatial_index`, `drop_vector_index` — Track: schema
- [ ] **SIX-012** Rename an index. — API: `t.rename_index(from, to)` — Track: schema
- [ ] **SIX-013** Foreign keys on one or several columns, named or not. — API: `t.foreign(cols).references(cols).on(table)` — Track: schema
- [ ] **SIX-014** Constrained foreign ids. — API: `foreign_id("company_id").constrained()`, `.constrained_on(table, col)`, `.references(col).on(table)` — Track: schema
- [ ] **SIX-015** Referential actions and their shortcuts. — API: `.on_delete(Action)`, `.on_update(Action)`, `cascade_on_delete`, `restrict_on_delete`, `null_on_delete`, `no_action_on_delete`, `cascade_on_update`, `restrict_on_update`, `null_on_update`, `no_action_on_update` — Track: schema
- [ ] **SIX-016** Deferrable foreign keys and constraints added without validating existing rows. — API: `.deferrable()`, `.initially_immediate()`, `.initially_deferred()`, `.not_valid()` — Track: schema — Notes: PostgreSQL.
- [ ] **SIX-017** Drop foreign keys. — API: `drop_foreign(name | cols)`, `drop_constrained_foreign_id(col)`, `drop_foreign_id_for::<M>()`, `drop_constrained_foreign_id_for::<M>()` — Track: schema
- [ ] **SIX-018** Drop and rename helpers. — API: `drop_column(s)`, `rename_column(from, to)`, `drop_timestamps`, `drop_timestamps_tz`, `drop_soft_deletes`, `drop_soft_deletes_tz`, `drop_morphs` — Track: schema

### I5. Schema dialect behaviour (`SGR`)

- [ ] **SGR-001** PostgreSQL: identity columns for auto-increment keys. — Track: schema
- [ ] **SGR-002** PostgreSQL: enumerations as a checked `VARCHAR`; `set` refused with a hint. — Track: schema
- [ ] **SGR-003** PostgreSQL: column changes through `ALTER COLUMN … TYPE … USING`, with defaults and nullability changed separately. — Track: schema
- [ ] **SGR-004** PostgreSQL: comments through `COMMENT ON`. — Track: schema
- [ ] **SGR-005** PostgreSQL: index access methods and operator classes (GIN, GiST, HNSW). — Track: schema
- [ ] **SGR-006** PostgreSQL and SQLite: DDL inside the migration's transaction. — Track: schema
- [ ] **SGR-007** MySQL: table options (engine, charset, collation, starting `AUTO_INCREMENT`). — Track: schema
- [ ] **SGR-008** MySQL: `RENAME COLUMN` on 8.0.3+ and MariaDB 10.5.2+, the older full-definition form before. — Track: schema
- [ ] **SGR-009** MySQL: column positions, unsigned, on-update timestamps, invisible columns, `ALGORITHM` and `LOCK` clauses. — Track: schema
- [ ] **SGR-010** MySQL: native `ENUM` and `SET`; UUID columns as `CHAR(36)`, native `UUID` on MariaDB 10.7+. — Track: schema
- [ ] **SGR-011** MySQL: dropping every table runs with foreign-key checks off. — Track: schema
- [ ] **SGR-012** MariaDB: vector indexes and its own UUID and JSON handling. — Track: schema
- [ ] **SGR-013** SQLite: alterations by table rebuild (change a column, add or drop a primary or foreign key, add columns SQLite cannot add in place, drop columns before 3.35), keeping data, indexes and foreign-key integrity (`PRAGMA foreign_key_check`). — Track: schema
- [ ] **SGR-014** SQLite: foreign keys added after creation go through a rebuild. — Track: schema
- [ ] **SGR-015** SQLite: virtual generated columns added in place, stored ones through a rebuild. — Track: schema
- [ ] **SGR-016** SQLite: type mapping by affinity, enumerations as `CHECK` constraints, booleans as `TINYINT(1)`. — Track: schema
- [ ] **SGR-017** SQLite: dropping every table and view with foreign keys off. — Track: schema
- [ ] **SGR-018** SQLite: comments ignored; features SQLite lacks (sets, full-text and spatial indexes) fail with a hint. — Track: schema
- [ ] **SGR-019** Snapshot tests for every type, modifier, index, foreign key and alteration on PostgreSQL, TimescaleDB, MySQL, MariaDB and SQLite. — Track: schema

### I6. Migrations (`MIG`)

- [ ] **MIG-001** Migrations are Rust types with `up` and `down`. — API: `trait Migration { async fn up(&self, schema: &Schema<'_>) -> SchemaResult; async fn down(..) }` — Track: schema
- [ ] **MIG-002** Migrations are listed per module, named after their timestamped files, ordered by module dependencies. — API: `migrations! { module: "deals", after: ["companies"], files: [..] }`, `migration!(file)` — Track: schema
- [ ] **MIG-003** The framework crates' existing migration sets keep running, and schema migrations can join them. — API: `SchemaMigration` adapter, `MigrationSet` — Track: schema
- [ ] **MIG-004** Applied migrations are recorded in `navree_migrations` with batch numbers. — API: `MIGRATIONS_TABLE`, `migrate:install` — Track: schema
- [ ] **MIG-005** Run pending migrations as one batch, or one batch each. — API: `Migrator::up`, `--step` — Track: schema
- [ ] **MIG-006** Each migration runs in a transaction where the database allows it, and can opt out (concurrent index builds). — API: `Migration::WITHIN_TRANSACTION = false` — Track: schema
- [ ] **MIG-007** A migration can target a named connection. — API: `Migration::connection()` — Track: schema
- [ ] **MIG-008** Conditional migrations, skipped and reported. — API: `Migration::should_run(&self, ctx)` — Track: schema
- [ ] **MIG-009** Roll back the last batch, the last N migrations, or one batch by number. — API: `Migrator::rollback(Rollback::LastBatch | Steps(n) | Batch(n))` — Track: schema
- [ ] **MIG-010** Reset every migration, refresh (reset then migrate), fresh (drop everything then migrate). — API: `Migrator::reset`, `Migrator::refresh`, `Migrator::fresh` — Track: schema
- [ ] **MIG-011** Status: each migration with its batch, or only the pending ones. — API: `Migrator::status` → `MigrationStatus` — Track: schema
- [ ] **MIG-012** Pretend mode: print the SQL each migration would run, without writing anything (not even the migrations table). — API: `--pretend` — Track: schema — Notes: reads such as `has_table` still answer from the real database.
- [ ] **MIG-013** Squash: dump the schema and the migration rows to `database/schema/{driver}-schema.sql`; load that dump on a database where nothing has run. — API: `schema:dump`, `--schema-path` — Track: schema — Notes: PostgreSQL dumps recreate TimescaleDB hypertables, policies and continuous aggregates.
- [ ] **MIG-014** Generate migrations from templates: `create_<table>_table` gets a create/drop pair, `add_<x>_to_<table>` an alter pair; the file joins the module's list. — API: `navree make:migration <module> <name> [--create T] [--table T]` — Track: schema — Notes: templates can be overridden from `stubs/`; post-create hooks are n/a (the generator is a command).
- [ ] **MIG-015** Migration events. — API: `MigrationsStarted`, `MigrationsEnded`, `MigrationStarted`, `MigrationEnded`, `MigrationSkipped`, `NoPendingMigrations`, `SchemaDumped`, `SchemaLoaded`, `DatabaseRefreshed` — Track: schema
- [ ] **MIG-016** Skip given migrations (for tests). — API: `Migrator::without(&[names])` — Track: schema
- [ ] **MIG-017** Only one process migrates at a time. — API: an advisory lock per database (PostgreSQL `pg_advisory_lock`, MySQL `GET_LOCK`, SQLite a lock row) — Track: schema
- [ ] **MIG-018** `migrate` offers to create a missing database (SQLite file, PostgreSQL or MySQL `CREATE DATABASE`), or creates it with `--force`. — Track: schema
- [ ] **MIG-019** Graceful mode returns success even when a migration fails, after printing the error. — API: `--graceful` — Track: schema
- [ ] **MIG-020** Command output shows each migration with its duration and result. — Track: schema
- [ ] **MIG-021** Repository queries: ran migrations, migrations by batch, last batch, next batch number, log, delete, existence, drop. — API: `MigrationRepository` — Track: schema
- [ ] **MIG-022** Migration paths and registering extra paths. — API: n/a → migrations are compiled in and listed per module — Track: schema
- [ ] **MIG-023** Options and notices reach a running migration. — API: `MigrationContext::current()`, `ctx.notice(..)` — Track: schema
- [ ] **MIG-024** SQLite migrations run on a dedicated connection with foreign keys off and `PRAGMA foreign_key_check` before each commit. — Track: schema
- [ ] **MIG-025** Workspace runners (schema or database per workspace) run schema migrations unchanged. — Track: schema
- [ ] **MIG-026** MySQL migrator details: `DATETIME` applied-at column, fresh runs with foreign-key checks off, real-server tests. — Track: mysql

### I7. Database commands (`CMD`)

- [ ] **CMD-001** `navree migrate [--database D] [--force] [--pretend] [--seed] [--seeder S] [--step] [--graceful] [--schema-path P]`. — Track: schema
- [ ] **CMD-002** `navree migrate:install [--database D]`. — Track: schema
- [ ] **CMD-003** `navree migrate:rollback [--step N | --batch N] [--pretend] [--force]`. — Track: schema
- [ ] **CMD-004** `navree migrate:reset [--pretend] [--force]`. — Track: schema
- [ ] **CMD-005** `navree migrate:refresh [--step N] [--seed] [--seeder S] [--force]`. — Track: schema
- [ ] **CMD-006** `navree migrate:fresh [--seed] [--seeder S] [--step] [--drop-views] [--drop-types] [--schema-path P] [--force]`. — Track: schema
- [ ] **CMD-007** `navree migrate:status [--pending]`. — Track: schema
- [ ] **CMD-008** `navree schema:dump [--path P] [--without-migration-data]`. — Track: schema — Notes: deleting the squashed migration files (`--prune`) is n/a: files stay compiled in and recorded ones are skipped.
- [ ] **CMD-009** `navree db:wipe [--drop-views] [--drop-types] [--force]`: drop every table, view and type. — Track: schema
- [ ] **CMD-010** `navree db:show [--json] [--counts] [--views] [--types]`: platform, version, connection, size, tables with sizes and optional row counts, views, types. — Track: schema
- [ ] **CMD-011** `navree db:table [table] [--json]`: columns with types and attributes, indexes, foreign keys, size. — Track: schema
- [ ] **CMD-012** `navree db:monitor [--databases a,b] [--max N]`: open connections per database, dispatching `DatabaseBusy` above the limit. — Track: orm-streams
- [ ] **CMD-013** `navree db [connection] [--read] [--write]`: open the database's own client (`psql`, `mysql`, `sqlite3`, `mongosh`) with the configured credentials. — Track: orm-streams
- [ ] **CMD-014** Destructive commands refuse to run in production without confirmation or `--force`. — Track: schema
- [ ] **CMD-015** Every database command takes `--database` to pick a named connection. — Track: schema

## Part J. Query dialects

Each line is verified by a per-dialect SQL snapshot and, where a server is available, by the gated real-server suite.

### J1. PostgreSQL (`DPG`)

- [ ] **DPG-001** `LIKE` is case-sensitive and `ILIKE` case-insensitive; non-text columns are cast to text for pattern matches. — Track: orm-core
- [ ] **DPG-002** Date parts: `col::date`, `col::time`, `extract(day | month | year from col)`, JSON-path columns parenthesised. — Track: orm-core
- [ ] **DPG-003** JSON paths: `->` between segments, `->>` for the text of the last, integer segments as array indexes, booleans compared as `jsonb`. — Track: orm-core
- [ ] **DPG-004** JSON containment with `@>` on `jsonb` and array length with `jsonb_array_length`. — Track: orm-core
- [ ] **DPG-005** JSON key existence and overlaps (`?`, `?|`, `jsonb_exists`); positional `$n` parameters mean `?` operators never clash with placeholders. — Track: orm-streams
- [ ] **DPG-006** Full text: `to_tsvector(language, col) @@ plainto_tsquery | phraseto_tsquery | websearch_to_tsquery | to_tsquery`, several columns joined with `||`. — Track: orm-streams
- [ ] **DPG-007** Bitwise conditions compile to `(col & ?)::bool`, in `WHERE` and `HAVING`. — Track: orm-core
- [ ] **DPG-008** Locks: `FOR UPDATE`, `FOR SHARE`, `FOR NO KEY UPDATE`, with `SKIP LOCKED` and `NOWAIT`. — Track: orm-streams
- [ ] **DPG-009** Generated keys with `RETURNING`; insert-or-ignore with `ON CONFLICT DO NOTHING` (with a conflict target when rows are returned). — Track: orm-streams
- [ ] **DPG-010** Upserts with `ON CONFLICT (cols) DO UPDATE SET col = excluded.col`. — Track: orm-streams
- [ ] **DPG-011** Updates and deletes with joins or limits through `UPDATE … FROM` or `ctid IN (subquery)`. — Track: orm-streams
- [ ] **DPG-012** `TRUNCATE … RESTART IDENTITY`, with `CASCADE` on request. — Track: orm-streams
- [ ] **DPG-013** JSON updates with `jsonb_set(col::jsonb, '{a,b}', ?)`. — Track: orm-streams
- [ ] **DPG-014** `DISTINCT ON`. — Track: orm-core
- [ ] **DPG-015** Vector distance operators (pgvector `<=>`, `<->`). — Track: orm-streams
- [ ] **DPG-016** Typed binds: booleans as booleans, typed `NULL`s, UUIDs as `uuid`, JSON as `jsonb`. — Track: orm-core
- [ ] **DPG-017** Statement timeouts with `SET LOCAL statement_timeout`. — Track: orm-streams
- [ ] **DPG-018** Raw SQL output keeps operators that contain `?` intact. — Track: orm-core
- [ ] **DPG-019** Lateral joins. — Track: orm-streams
- [ ] **DPG-020** Random order with `RANDOM()`. — Track: orm-core
- [ ] **DPG-021** Open connections counted from `pg_stat_activity` (for `db:monitor`). — Track: orm-streams
- [ ] **DPG-022** Limit per group with `ROW_NUMBER()`. — Track: orm-streams
- [ ] **DPG-023** Every PostgreSQL behaviour above passes against a real server (`NAVREE_TEST_POSTGRES_URL`), locally and in CI. — Track: orm-core

### J2. MySQL and MariaDB (`DMY`)

- [ ] **DMY-001** Case-sensitive `LIKE` through a binary comparison; the default follows the column collation (documented). — Track: mysql
- [ ] **DMY-002** Null-safe equality with `<=>`. — Track: mysql
- [ ] **DMY-003** Date parts with `date()`, `time()`, `day()`, `month()`, `year()`. — Track: mysql
- [ ] **DMY-004** JSON paths with `json_unquote(json_extract(col, '$."a"'))`; booleans compared as JSON booleans. — Track: mysql
- [ ] **DMY-005** JSON containment `json_contains`, overlaps `json_overlaps`, key existence `json_contains_path(col, 'one', path)`, length `json_length`. — Track: mysql
- [ ] **DMY-006** Full text: `MATCH (cols) AGAINST (? IN NATURAL LANGUAGE MODE | IN BOOLEAN MODE | WITH QUERY EXPANSION)`. — Track: mysql
- [ ] **DMY-007** Limit per group with window functions on MySQL 8.0 and MariaDB 10.2+, an emulation before. — Track: mysql
- [ ] **DMY-008** Insert-or-ignore with `INSERT IGNORE` (also from a select); returned rows through MariaDB `RETURNING` or a select on the unique columns. — Track: mysql
- [ ] **DMY-009** Upserts with `ON DUPLICATE KEY UPDATE`, using a row alias on MySQL 8.0.19+ and `VALUES()` before. — Track: mysql
- [ ] **DMY-010** Random order with `RAND(seed)`. — Track: mysql
- [ ] **DMY-011** Locks: `FOR UPDATE`, `LOCK IN SHARE MODE` (`FOR SHARE` on 8.0), `SKIP LOCKED` and `NOWAIT` on 8.0. — Track: mysql
- [ ] **DMY-012** Lateral joins on MySQL 8.0.14+; `Unsupported` on MariaDB. — Track: mysql
- [ ] **DMY-013** JSON updates with `json_set(col, '$."a"', ?)`. — Track: mysql
- [ ] **DMY-014** Multi-table updates and deletes, and `ORDER BY … LIMIT` on single-table updates and deletes. — Track: mysql
- [ ] **DMY-015** Generated keys from `LAST_INSERT_ID()`; MariaDB 10.5+ `RETURNING` on inserts and deletes. — Track: mysql
- [ ] **DMY-016** Backtick identifier quoting and JSON path quoting. — Track: mysql
- [ ] **DMY-017** `SELECT` timeouts through the execution-time optimizer hint. — Track: mysql
- [ ] **DMY-018** Open connections from `Threads_connected`. — Track: mysql
- [ ] **DMY-019** MariaDB differences: JSON functions, its vector distance function, versions of window functions and `RETURNING`. — Track: mysql
- [ ] **DMY-020** Decoding: booleans from `TINYINT(1)`, decimals, date-times with fractional seconds; zero dates refused in strict mode. — Track: mysql
- [ ] **DMY-021** navree-db on MySQL: `Repo`, `paginate`, `cursor_paginate`, text helpers and errors (unique and foreign-key violations mapped to 409). — Track: mysql
- [ ] **DMY-022** Every MySQL behaviour above passes against real MySQL 8 and MariaDB 11 servers (`NAVREE_TEST_MYSQL_URL`), locally and in CI with both services. — Track: mysql

### J3. SQLite (`DSL`)

- [ ] **DSL-001** Case-sensitive `LIKE` through `GLOB` with the pattern translated (`%`→`*`, `_`→`?`, literal `*` and `?` escaped); plain `LIKE` is ASCII case-insensitive. — Track: orm-core
- [ ] **DSL-002** Null-safe equality with `IS`. — Track: orm-core
- [ ] **DSL-003** Date parts with `strftime` (`%Y-%m-%d`, `%H:%M:%S`, `%d`, `%m`, `%Y`), compared as text or integers as appropriate. — Track: orm-core
- [ ] **DSL-004** JSON paths with `json_extract`, length with `json_array_length`, containment through `json_each`, key existence through `json_type(..) IS NOT NULL`. — Track: orm-core
- [ ] **DSL-005** No row locks: lock clauses are dropped and writers serialise through `BEGIN IMMEDIATE`. — Track: orm-streams
- [ ] **DSL-006** Limit per group needs SQLite 3.25+. — Track: orm-streams
- [ ] **DSL-007** `INSERT OR IGNORE` (with `RETURNING` on 3.35+) and upserts with `ON CONFLICT … DO UPDATE SET col = excluded.col`. — Track: orm-streams
- [ ] **DSL-008** JSON updates grouped per column through `json_set` / `json_patch`. — Track: orm-streams
- [ ] **DSL-009** Updates and deletes with joins or limits through `rowid IN (subquery)`. — Track: orm-streams
- [ ] **DSL-010** Truncate as `DELETE FROM` plus resetting the table's autoincrement sequence. — Track: orm-streams
- [ ] **DSL-011** Union parts wrapped as `select * from (…)`. — Track: orm-streams
- [ ] **DSL-012** Full-text conditions are `Unsupported`; FTS5 tables are documented as the alternative. — Track: orm-streams
- [ ] **DSL-013** Decoding: booleans from `0/1`, date-times from ISO 8601 text, decimals from text or real. — Track: orm-core
- [ ] **DSL-014** The bind-variable limit (32766) is respected by `IN` lists and multi-row inserts. — Track: orm-core
- [ ] **DSL-015** `RETURNING` on SQLite 3.35+, `last_insert_rowid()` before. — Track: orm-core

## Part K. What Navree adds

### K1. TimescaleDB (`TSC`)

- [ ] **TSC-001** TimescaleDB is the default database: `navree new` and the starter kits generate a PostgreSQL URL for a TimescaleDB server. — Track: timescale-default
- [ ] **TSC-002** Dock's default database service runs a TimescaleDB image. — Track: timescale-default
- [ ] **TSC-003** The docs lead with TimescaleDB and present plain PostgreSQL, MySQL/MariaDB, SQLite and MongoDB as fully supported alternatives. — Track: timescale-default
- [ ] **TSC-004** Detect TimescaleDB and install the extension when allowed. — API: `[schema] timescaledb = "auto" | "require" | "off"`, `Db::has_timescaledb()` — Track: schema — Notes: `require` turns every fallback into an error.
- [ ] **TSC-005** Create a hypertable with its table. — API: `schema.create_hypertable(table, time_col, |t| ..)`, `t.hypertable(time_col)` — Track: schema
- [ ] **TSC-006** Convert an existing table, optionally moving its data into chunks. — API: `schema.convert_to_hypertable(table, &Hypertable::on(col).migrate_data())` — Track: schema
- [ ] **TSC-007** Chunk interval at creation and later. — API: `Hypertable::chunk_interval("1 day")`, `schema.set_chunk_interval(table, interval)` — Track: schema — Notes: integer time columns take integer intervals.
- [ ] **TSC-008** Space partitioning. — API: `Hypertable::partition_by("workspace_id", 4)` — Track: schema
- [ ] **TSC-009** Primary keys and unique indexes include the time and partition columns. — API: `KeyStrategy::Extend` (default), `KeyStrategy::Reject` — Track: schema — Notes: converting a table names every unique index and referencing foreign key that blocks it, with the fix.
- [ ] **TSC-010** Compression with segment-by, order-by and a policy delay; removable. — API: `Compression::new().segment_by([..]).order_by("..").after("7 days")`, `schema.compress(table, ..)`, `schema.remove_compression_policy(table)` — Track: schema
- [ ] **TSC-011** Retention. — API: `Hypertable::retain_for("90 days")`, `schema.add_retention_policy`, `schema.remove_retention_policy` — Track: schema
- [ ] **TSC-012** Reorder policies. — API: `Hypertable::reorder_by(index)`, `schema.add_reorder_policy`, `schema.remove_reorder_policy` — Track: schema
- [ ] **TSC-013** Continuous aggregates from SQL or a structured description, with refresh policies and real-time results. — API: `ContinuousAggregate::new(name, sql)`, `ContinuousAggregate::over(name, table).time_bucket(..).group_by(..).aggregate(..).where_raw(..).refresh(RefreshPolicy::every(..).start_offset(..).end_offset(..)).materialized_only(..)`, `schema.create_continuous_aggregate`, `drop_continuous_aggregate`, `refresh_continuous_aggregate` — Track: schema
- [ ] **TSC-014** One migration runs everywhere: without TimescaleDB a hypertable is a regular table with time indexes and a notice per missing feature, and a continuous aggregate is a plain view. — Track: schema — Notes: the structured form compiles `time_bucket` to `date_bin` on PostgreSQL 14+ and to epoch arithmetic on MySQL and SQLite.
- [ ] **TSC-015** Time buckets in the query builder, with origin, offset and timezone. — API: `Expr::time_bucket(width, col)`, `Expr::time_bucket_from(width, col, origin)` — Track: orm-core — Notes: native on TimescaleDB, `date_bin` on PostgreSQL 14+, epoch arithmetic on MySQL and SQLite.
- [ ] **TSC-016** First and last value by time. — API: `Expr::first(value, time)`, `Expr::last(value, time)` — Track: orm-core — Notes: emulated with ordered subqueries or window functions without TimescaleDB.
- [ ] **TSC-017** Gap filling and interpolation. — API: `Expr::time_bucket_gapfill(..)`, `Expr::locf(..)`, `Expr::interpolate(..)` — Track: timescale-default — Notes: TimescaleDB only; `Unsupported` elsewhere.
- [ ] **TSC-018** Models on hypertables: updates and deletes also match the time column so only one chunk is touched. — API: `#[model(time_column = "occurred_at")]` — Track: timescale-default
- [ ] **TSC-019** Read-only models over continuous aggregates and views. — API: `#[model(read_only)]` → writes return `OrmError::ReadOnly` — Track: timescale-default
- [ ] **TSC-020** Chunk maintenance and sizes. — API: `navree_orm::timescale::show_chunks(exec, table, older_than)`, `drop_chunks`, `compress_chunk`, `decompress_chunk`, `hypertable_size`, `approximate_row_count` — Track: timescale-default
- [ ] **TSC-021** `db:show` and `db:table` report hypertables, chunks, compression ratios and policies. — Track: timescale-default
- [ ] **TSC-022** Navree's own time-series tables become hypertables on TimescaleDB: audit log, telemetry and Lighthouse events, webhook deliveries, job history, notification delivery history, schedule runs. — Track: timescale-default — Notes: through the schema helpers, so they stay regular tables elsewhere.
- [ ] **TSC-023** A guide: which tables to convert (append-mostly, time-keyed, large) and which not (users, workspaces, business records), chunk sizing, time-ordered ids (UUIDv7, ULID). — Track: timescale-default
- [ ] **TSC-024** Workspace row-level security on hypertables (policies apply to every chunk), tested. — Track: timescale-default
- [ ] **TSC-025** The TimescaleDB suite runs against a real server (`NAVREE_TEST_POSTGRES_URL`), locally and in CI with a TimescaleDB service. — Track: timescale-default
- [ ] **TSC-026** Cursor pagination and key-chunked streams on hypertable models add a time-column predicate derived from the cursor, so each page reads only the chunks it needs. — API: `cursor_paginate`, `chunk_by_id`, `lazy_by_id` on models with `#[model(time_column)]` — Track: timescale-default
- [ ] **TSC-027** Maintenance commands: status of hypertables, chunks, policies and background jobs; refresh a continuous aggregate over a range; compress chunks older than an interval. — API: `navree timescale:status`, `navree timescale:refresh <aggregate> [--from T] [--to T]`, `navree timescale:compress <table> [--older-than I]` — Track: timescale-default
- [ ] **TSC-028** The `database` health check reports the TimescaleDB version and failing background jobs. — Track: timescale-default

### K2. MongoDB (`MNG`)

- [ ] **MNG-001** A document driver crate implementing the ORM's driver interface. — API: `navree-orm-mongodb`, `MongoDriver: DocumentDriver` — Track: mongodb
- [ ] **MNG-002** Executors for a client and for a session (transactions). — API: `MongoDb`, `MongoSession` (`Executor` → `Target::Document`) — Track: mongodb
- [ ] **MNG-003** Configuration: URL, database, pool, read preference, write concern. — API: `[database.mongodb]` — Track: mongodb
- [ ] **MNG-004** Value conversion: dates, decimals as Decimal128, UUIDs as binary subtype 4, JSON casts as embedded documents. — Track: mongodb
- [ ] **MNG-005** ObjectId keys. — API: `ObjectId: Field`, generated when empty — Track: mongodb
- [ ] **MNG-006** Conditions become filter documents: comparisons, `$in`/`$nin`, null and `$exists`, ranges, `LIKE` as an escaped `$regex` (case-insensitive by default), `$and`/`$or`/`$nor`, negation. — Track: mongodb
- [ ] **MNG-007** Date-part conditions through `$expr` with `$year`, `$month`, `$dayOfMonth` and time parts. — Track: mongodb
- [ ] **MNG-008** JSON paths as dot notation; JSON contains, length and key existence through `$all`/`$elemMatch`, `$size` and `$exists`. — Track: mongodb
- [ ] **MNG-009** Raw filter documents and aggregation pipelines. — API: `Filter::document(doc)`, `Builder::pipeline(stages)` — Track: mongodb
- [ ] **MNG-010** Projection, sort, skip, limit and distinct values. — Track: mongodb
- [ ] **MNG-011** Aggregates, group by and having through `$group` and `$match` stages. — Track: mongodb
- [ ] **MNG-012** Inserts returning ids. — Track: mongodb
- [ ] **MNG-013** Updates with `$set`, `$unset` and `$inc`; deletes. — Track: mongodb
- [ ] **MNG-014** Upserts and insert-or-ignore through bulk writes, ignoring duplicate-key errors when asked. — Track: mongodb
- [ ] **MNG-015** Relations load through batched `$in` queries, so eager loading is unchanged; `$lookup` is an optional optimisation. — Track: mongodb
- [ ] **MNG-016** Embedded documents and arrays through JSON casts. — Track: mongodb
- [ ] **MNG-017** Full-text search through `$text` and text indexes. — Track: mongodb
- [ ] **MNG-018** Time buckets through `$dateTrunc` in aggregations. — Track: mongodb
- [ ] **MNG-019** Pagination (count, skip and limit; cursors on sort keys) and streaming through driver cursors. — Track: mongodb
- [ ] **MNG-020** Features with no document equivalent (joins, unions, row locks, raw SQL, `EXISTS` subqueries) fail with `Unsupported` naming the feature. — Track: mongodb
- [ ] **MNG-021** Duplicate-key errors map to unique violations (409). — Track: mongodb
- [ ] **MNG-022** The schema builder on MongoDB: create, rename and drop collections, JSON-schema validators from blueprint columns, unique, compound, TTL and text indexes, `has_table`. — Track: mongodb
- [ ] **MNG-023** Hypertables become time-series collections (`timeField`, `metaField` from the partition column, granularity from the chunk interval, expiry from retention). — Track: mongodb
- [ ] **MNG-024** Migrations recorded in a collection, in transactions where the deployment supports them. — Track: mongodb
- [ ] **MNG-025** The unit of work on MongoDB: writes in the session's transaction, change events with actor and request id. — Track: mongodb
- [ ] **MNG-026** Workspace scoping through the global scope (no row-level security), tested. — Track: mongodb
- [ ] **MNG-027** Factories and test assertions work on MongoDB. — Track: mongodb
- [ ] **MNG-028** The MongoDB suite runs against a real server (`NAVREE_TEST_MONGODB_URL`), locally and in CI with a MongoDB replica-set service (transactions need one). — Track: mongodb

### K3. Typed columns (`TYP`)

- [ ] **TYP-001** Every column is a typed constant carrying its table, name, and the encoder and decoder of its field. — API: `Deal::STAGE: Column<Deal, Stage>` — Track: orm-core
- [ ] **TYP-002** Conditions type-check against the field type (`Deal::AMOUNT_CENTS.eq("x")` does not compile). — Track: orm-core
- [ ] **TYP-003** Optional fields give `Column<M, T>`; null checks are always available and comparisons take `T`. — Track: orm-core
- [ ] **TYP-004** Typed assignments for updates. — API: `Deal::STAGE.set(Stage::Won)`, `set_expr`, `set_null` — Track: orm-core
- [ ] **TYP-005** Typed orders and selects. — API: `Deal::CREATED_AT.desc()`, `select([Deal::ID, Deal::TITLE])` — Track: orm-core
- [ ] **TYP-006** Untyped escape hatches stay available. — API: `col("deals.title")`, `col("meta->status")`, `raw(..)` — Track: orm-core
- [ ] **TYP-007** Typed keys. — API: `M::Key`, `Deal::find(exec, key)` only accepts the key type — Track: orm-core
- [ ] **TYP-008** Rows decoded into plain structs. — API: `#[derive(FromRow)]`, `get_as::<T>(exec)`, `first_as::<T>(exec)` — Track: orm-core
- [ ] **TYP-009** App newtypes as column types. — API: `impl Field for Email`, `#[derive(Field)]` — Track: orm-core
- [ ] **TYP-010** A model's typed columns are accepted only by its own builder; columns of joined tables go through `col()` or a joined-column wrapper. — Track: orm-core

### K4. Workspace scoping (`WSP`)

- [ ] **WSP-001** Workspace-scoped models filter every query to the current workspace. — API: `#[model(workspace)]` (`BelongsToWorkspace`), a built-in global scope on `workspace_id` — Track: orm-factories
- [ ] **WSP-002** Creating fills `workspace_id`; changing it on an existing record is refused. — Track: orm-factories
- [ ] **WSP-003** The current workspace comes from `CurrentWorkspace` and the unit of work. — Track: orm-factories
- [ ] **WSP-004** Explicit opt-outs for system work. — API: `without_workspace_scope()`, `for_workspace(id)` — Track: orm-factories
- [ ] **WSP-005** Relations, eager loads, existence subqueries and relation aggregates stay inside the workspace. — Track: orm-factories
- [ ] **WSP-006** Route model binding answers 404 for another workspace's record. — Track: orm-factories
- [ ] **WSP-007** PostgreSQL row-level security through `navree.workspace`, set for every workspace transaction, also covers ORM queries. — Track: orm-factories
- [ ] **WSP-008** MySQL isolates workspaces by query scoping only (no row-level security), documented and tested. — Track: mysql
- [ ] **WSP-009** Schema-per-workspace (`search_path`) and database-per-workspace (`WorkspaceUow`) work as ORM executors. — Track: orm-factories
- [ ] **WSP-010** Schema helper for the workspace column with its foreign key and index. — API: `t.workspace()` — Track: schema
- [ ] **WSP-011** Unique constraints and validation rules per workspace. — API: `t.unique(["workspace_id", "slug"])`, `unique::<Deal>(..)` scoped to the workspace — Track: orm-factories
- [ ] **WSP-012** Factories used inside a workspace fill it. — Track: orm-factories
- [ ] **WSP-013** Leak tests: every ORM read and write path is tried across two workspaces. — Track: orm-factories

### K5. Access-control filters (`ACL`)

- [ ] **ACL-001** Access filters on model queries. — API: `Deal::query().accessible(&ability, Read)`, the `Accessible<Deal, Read>` extractor (applies `Ability::sql_filter_with`) — Track: orm-factories
- [ ] **ACL-002** Model columns mapped for access conditions from model metadata. — API: `ModelColumns<M>` — Track: orm-factories
- [ ] **ACL-003** Records loaded and authorized by route key inside the unit of work. — API: `Authorized<Deal, Update>` — Track: orm-factories
- [ ] **ACL-004** Record checks with ORM records as subjects. — API: `#[model(subject = "Deal")]`, `ability.authorize(Update, &record)` — Track: orm-factories
- [ ] **ACL-005** Only accessible related records in eager loads. — API: `with_accessible("activities", &ability)` — Track: orm-relations
- [ ] **ACL-006** Counts and pagination respect access filters. — Track: orm-factories
- [x] **ACL-007** Field-level masking in serialized records and resources. — API: `mask_fields` with ORM records — Track: orm-attributes — Tests: crates/navree-orm/tests/orm/attributes/resources.rs::masks_keep_the_permitted_columns_the_key_and_the_type
- [ ] **ACL-008** Policies attached to models. — API: `#[model(policy = DealPolicy)]` — Track: orm-factories
- [ ] **ACL-009** A record check and a list filter never disagree for ORM models (property tests over generated rules). — Track: orm-factories

### K6. Unit of work and outbox (`UOW`)

- [ ] **UOW-001** Queries run on `&Db` (joining the task's current unit of work) or on `&Uow` (its transaction). — Track: orm-core
- [ ] **UOW-002** Plain connections and transactions also work as executors, without change events. — Track: orm-core
- [ ] **UOW-003** Every model write records a change event (table, key, old and new values) in its unit of work, so the audit log gets actor and request id. — Track: orm-core
- [ ] **UOW-004** Mass updates and deletes record no change events by default; an opt-in records one summary event. — Track: orm-core
- [ ] **UOW-005** PostgreSQL: `navree.actor`, `navree.request_id` and `navree.workspace` are set for each transaction. — Track: orm-core
- [ ] **UOW-006** MySQL: the same values as session variables, cleared after each unit of work. — Track: mysql
- [ ] **UOW-007** SQLite: units of work start with `BEGIN IMMEDIATE`, so concurrent writers queue instead of failing. — Track: orm-core
- [ ] **UOW-008** After-commit work for models: "-ed" events, broadcasts, cache invalidation. — Track: orm-lifecycle
- [ ] **UOW-009** Jobs and events dispatched from observers join the unit of work: written in its transaction (database queue) or queued after commit. — Track: orm-lifecycle
- [ ] **UOW-010** The outbox: entries written in the same transaction as the data; a relay claims them with `FOR UPDATE SKIP LOCKED` and publishes after commit, with the entry id as the idempotency key. — Track: orm-core
- [ ] **UOW-011** Outbox crash recovery: entries committed but not yet published go out after a restart (at least once). — Track: orm-core
- [ ] **UOW-012** Read-your-writes: reads in a unit of work see its writes (its transaction, or the writer when replicas exist). — Track: orm-streams
- [ ] **UOW-013** Jobs, scheduled tasks, commands, gRPC calls and WebSocket messages each run in their own unit of work, so ORM code behaves the same everywhere. — Track: orm-core
- [ ] **UOW-014** Per-unit-of-work query statistics (count and total time) in logs and telemetry, flagging repeated queries. — Track: orm-streams
- [ ] **UOW-015** HTTP: the unit of work commits below status 400 and rolls back otherwise, and handlers marked transactional finish even when the client disconnects. — Track: orm-core
- [ ] **UOW-016** The existing `Repo` API and ORM models share one unit of work. — Track: orm-core

## Final audit

The audit reads this file from top to bottom once every track has merged:

1. Every line is ticked and carries ` — Tests: …` pointing at the test that proves it, or it is an n/a line whose replacement is ticked with its own test reference.
2. No unticked lines remain (`grep -c '^- \[ \]'` on this file prints 0).
3. Names on the lines match the merged code and the two DESIGN.md files; any difference was resolved by updating the line.
4. The counts above are recomputed and still add up (no line was deleted to make the list shorter; a capability dropped on purpose stays as an n/a line with its reason).
5. The per-dialect snapshot suites pass, and the gated real-server suites pass against PostgreSQL with TimescaleDB, MySQL 8, MariaDB 11 and MongoDB.
