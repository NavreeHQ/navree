# navree-orm: architecture

This document is the map of the Navree ORM: how the crates fit together,
what `#[derive(Model)]` generates, how a query becomes SQL (or a document
command) and runs on a `Db` or inside a `Uow`, and where every later
feature plugs in. The core (this crate as first merged) implements
sections 1 to 9; section 10 assigns the rest to the feature tracks, naming
the traits, hooks, files and macro attributes each one owns so they can
work in parallel with few conflicts.

Principles:

- **Rust-shaped.** Models are plain structs. Columns are typed constants
  (`Deal::STAGE: Column<Deal, Stage>`), conditions are typed values
  (`Deal::AMOUNT_CENTS.gte(10_000)`), keys are typed, everything that can
  fail returns `Result`, everything that touches the database is `async`
  and takes the executor explicitly. No runtime reflection: the derive
  writes the metadata.
- **String-flexible where it pays.** Untyped columns (`col("deals.title")`,
  JSON paths `col("meta->status")`), raw expressions and bindings
  (`raw("lower(name) = ?", ["acme"])`) and, later, relation paths
  (`"company.owner"`) are always available.
- **Built on navree-db.** Connections, the pool, the unit of work (`Uow`:
  transaction, actor, request id, change events, after-commit hooks,
  `set_config` for audit and workspace row-level security), migrations and
  pagination shapes stay in navree-db. The ORM runs every statement through
  them, so audit trails, workspace isolation and after-commit work keep
  working. Existing SeaORM entities keep working beside ORM models: both
  bind values the same way (the ORM uses sea-query values and SeaORM's
  decoders).
- **One driver abstraction.** A query is first a driver-neutral value (the
  IR, `navree_orm::ir`), then SQL for PostgreSQL/TimescaleDB, MySQL or
  SQLite (through sea-query's builders), or a document command for MongoDB
  (through the `DocumentDriver` trait a separate crate implements).

## 1. Crates

| Crate | Layer | Role |
|---|---|---|
| `navree-orm` | L4 | Models, records, collections, the query builder, the IR, SQL compilation per dialect, execution on `Db`/`Uow`, casts, pagination, scopes, the extension seams |
| `navree-orm-macros` | L0 | `#[derive(Model)]` and `#[scopes]` (proc macros; no runtime navree dependencies) |
| `navree-orm-mongodb` | L5 | MongoDB: implements `DocumentDriver` and its executors (mongodb track) |
| `navree-schema` | L3/L4 | Migrations and the schema builder (its own design document; the ORM does not depend on it) |

`navree-orm` may depend on navree-kernel, navree-contracts, navree-config,
navree-http, navree-db, navree-events, navree-crypto and navree-orm-macros
(its HIERARCHY row in `crates/navree/tests/dependency_graph.rs` already
allows the last four tracks' needs). It re-exports the macros, so
`use navree_orm::Model;` imports the trait and the derive at once, like
serde's `Serialize`.

The facade turns it on with the `orm` feature (part of `full`):
`navree::orm` is the whole crate; `navree::prelude` brings `Model`,
`Record`, `Collection` and `Builder`. `navree_orm::prelude` brings every
name an app uses (`col`, `raw`, `Filter`, `scopes`, ...).

### Paths in generated code

The derive names the runtime crate as `::navree_orm` when the crate being
compiled depends on `navree-orm` (inside navree-orm itself, through
`extern crate self as navree_orm;`), else as `::navree::orm` through the
facade (honouring renames). Depending on neither is a compile error that
says which dependency to add.

## 2. Module layout of `navree-orm`

```text
src/
  lib.rs            crate docs, re-exports, `extern crate self as navree_orm`
  prelude.rs        the app-facing names
  error.rs          OrmError, Result
  value.rs          Value (sea-query's), Field (Rust type <-> column value), lenient conversions
  ulid.rs           Ulid (time-ordered 128-bit ids, Crockford base32)
  row.rs            Row: one result row (SQL or document), typed and dynamic reads
  model.rs          Model trait (+ the static API: query, find, create, ...), ModelMeta, ColumnMeta, KeyKind
  column.rs         Column<M, T>, col(), Untyped, Filter, OrderBy, Assignment
  record.rs         Record<M>: the persisted instance, dirty tracking, instance API
  persist.rs        insert / update / delete flows (keys, timestamps, change events, hook calls)
  hooks.rs          the lifecycle seams persist.rs and retrieval call (lifecycle and relations fill them)
  collection.rs     Collection<M>
  pagination.rs     paginate / simple_paginate / cursor_paginate, page types, request extractors
  cast.rs           Cast trait, the built-in casts (json, string enums, integer enums)
  scope.rs          Scoped, GlobalScope, scope grouping, soft-delete scope
  expr.rs           Expr: raw SQL, functions, aggregates, subqueries, TimescaleDB helpers
  query/
    mod.rs          Builder<M>: the model query builder and its retrieval methods
    filters.rs      the where-family methods (stamped onto every builder) and WhereGroup
    clauses.rs      select / join / group / having / order / limit methods (stamped onto builders)
    table.rs        TableBuilder: untyped queries on any table (pivot tables, reports)
    write.rs        mass update / delete / increment / insert of rows
  ir.rs             the driver-neutral IR: QueryIr, Where, Expr, Join, Order, InsertIr, UpdateIr, DeleteIr
  sql/
    mod.rs          Dialect, Statement, compile entry points
    select.rs       SELECT (+ aggregates, exists)
    filter.rs       WHERE / HAVING trees, per-dialect date, LIKE and JSON rules
    write.rs        INSERT / UPDATE / DELETE (+ RETURNING where supported)
    timescale.rs    time_bucket / first / last (native on PostgreSQL, emulated buckets elsewhere)
  exec.rs           Executor, Target, Access, ExecContext; SQL execution on Db, Uow, connections
  driver.rs         DocumentDriver: the interface a document store implements
  relations.rs      relation skeleton (Relation trait, loaded-relation storage on records)
```

## 3. Models and `#[derive(Model)]`

```rust
use navree_orm::{Model, Ulid};

#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "deals", timestamps, soft_deletes, per_page = 25)]
pub struct Deal {
    pub id: Uuid,                       // key: `id`, kind inferred: UUID v7 generated when nil
    pub title: String,
    pub amount_cents: i64,
    #[model(cast = "string_enum")]
    pub stage: Stage,                   // stored as its serde name
    #[model(cast = "json")]
    pub meta: DealMeta,                 // stored as JSON
    pub company_id: Uuid,
    pub closed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}
```

Container attributes (core): `table` (default: the snake_case plural of the
type, `Deal` → `deals`), `primary_key` (default `id`), `key_type` (`int`,
`uuid` / `uuid_v7`, `uuid_v4`, `ulid`, `manual`; inferred from the key's
Rust type when absent: integers → `int`, `Uuid` → `uuid_v7`, `Ulid` →
`ulid`, anything else → `manual`), `timestamps` (or
`timestamps(created_at = "created_on", updated_at = false)`),
`soft_deletes` (or `soft_deletes = "removed_at"`), `connection = "name"`,
`per_page = 25`, `global_scopes(ActiveOnly, ...)`.

Field attributes (core): `column = "name"`, `cast = "json" | "string_enum"
| "int_enum" | path::To::Cast`, `default` (a column missing from a row
decodes as `Default::default()`), `skip` (not a column; always
`Default::default()` when read).

The derive generates:

- `impl Model for Deal`: `type Key`, `meta()` (a `&'static ModelMeta`:
  name, table, key column and kind, columns with their Rust types and
  nullability, timestamp and soft-delete columns, connection, per-page,
  global scopes), `key` / `set_key`, `from_row(&Row)`, `values()` (every
  column as `(name, Value)`), `value(column)`, `set_value(column, Value)`
  (used by timestamps, increments, assignments, refresh), `touch_timestamp`
  and the timestamp emptiness checks.
- One typed column constant per column: `Deal::ID: Column<Deal, Uuid>`,
  `Deal::CLOSED_AT: Column<Deal, DateTime<Utc>>` (an `Option<T>` field gives
  `Column<M, T>`; nullability is a property of the column, `is_null()` is
  always available). A constant carries its table, name, and the encoder
  and decoder of its field (the cast's, when there is one), so typed
  conditions encode values exactly as the field is stored.

`Model` requires `Clone + Send + Sync + Serialize + 'static`. Its provided
static methods are the model API: `query()`, `all`, `find`, `find_many`,
`find_or_fail`, `create`, `destroy`, `first_or_create`, `first_or_new`,
`update_or_create`, `where_` (a shortcut for `query().where_`).

### Keys

`KeyKind::Increments`: the database assigns the key; a record whose key is
`0` inserts without it and reads the generated value back (`RETURNING` on
PostgreSQL and SQLite, `last_insert_id` on MySQL). `UuidV7`, `UuidV4`,
`Ulid`: generated in Rust when the key is nil. `Manual`: never generated.
One key column per model.

### Values and fields

`Value` is sea-query's value enum (`navree_orm::Value`), so the ORM binds
exactly as SeaORM does (typed NULLs included, which PostgreSQL needs). Its
variant set depends on cargo features: match it with a `_` arm.

`Field` is the trait of Rust types a column can hold: `to_value`, `null`
(the typed NULL), `from_row(&Row, column)` (exact decoding through
SeaORM's `TryGetable` on SQL rows) and `from_value(Value)` (document rows
and dynamic values; lenient between numeric representations and from
strings for dates). Implemented for `bool`, all integers, `f32`, `f64`,
`Decimal`, `String`, `Vec<u8>`, `Uuid`, `Ulid`, chrono's `NaiveDate`,
`NaiveTime`, `NaiveDateTime`, `DateTime<Utc>`, `DateTime<FixedOffset>`,
`serde_json::Value` and `Option<T>`.

## 4. Records

`Record<M>` is a persisted (or to-be-persisted) instance: the model (`Deref`
/ `DerefMut` to `M`) plus what the ORM tracks:

| Field | Meaning |
|---|---|
| `original: Option<M>` | the model as last synced with the database (`None` while new) |
| `changes`, `previous` | what the last save changed: new and old column values |
| `exists`, `recently_created` | persistence state |
| `extras` | non-column values of the row (aggregates, `with_count` results, pivot columns, selected expressions) |
| `relations` | loaded relations (`relations.rs`; filled by the relations track) |

Instance API (core): `new`, `save`, `update(exec, |m| ..)`, `delete`,
`refresh`, `fresh`, `replicate`, `touch`, `increment` / `decrement`,
`is_dirty` / `is_dirty_on`, `is_clean` / `is_clean_on`, `was_changed` /
`was_changed_on`, `get_original`, `get_dirty`, `get_changes`,
`get_previous`, `sync_original`, `discard_changes`, `exists`,
`was_recently_created`, `trashed`, `extra::<T>(name)`, `into_inner`.

Dirty tracking compares the encoded column values of the model and its
original: floats within a small epsilon, decimals numerically, JSON
structurally, date-times by instant. A new record is dirty in every column.

### Persistence flows (`persist.rs`)

Insert: `saving` → `creating` (both cancellable) → generate the key when
empty → set `created_at` / `updated_at` when empty → INSERT → read a
generated key back → `exists`, `recently_created` → record a `created`
change event → `created` → `saved` → sync the original.

Update: `saving` → (when dirty) `updating` → set `updated_at` unless the
caller changed it → UPDATE the dirty columns `WHERE key = original key` →
sync changes → `updated` change event (changed columns, old and new) →
`updated` → `saved` → sync the original. A clean record still fires
`saving` / `saved`.

Delete: `deleting` (cancellable) → DELETE, or for soft-deleting models an
UPDATE of the deleted-at (and updated-at) column → `deleted` change event
(the whole old record) → `deleted`.

Change events go to the unit of work the statement ran in (`&Uow`, or the
task's current unit of work that `&Db` joined), exactly like
`navree_db::Repo`: `entity` is the table, `id` the key, values are the
stored column values. So navree-audit records ORM writes with the actor
and request id of the unit of work. Mass updates and deletes
(`Deal::query().where_(..).update(..)`) record no events.

Timestamps come from the app's `dyn Clock` when the executor's pool belongs
to an app that registers one (tests freeze time with a `FixedClock`), else
from `Utc::now()`, truncated to microseconds so the stored value and the
in-memory value agree on every database.

## 5. Queries

`Deal::query()` returns `Builder<Deal>`. Every builder method takes `self`
and returns `Self`; terminal methods take the executor first:

```rust
let won = Deal::query()
    .where_(Deal::STAGE.eq(Stage::Won))
    .where_(Deal::AMOUNT_CENTS.gte(100_000))
    .where_group(|g| g.where_(Deal::OWNER_ID.eq(me)).or_where_null(Deal::OWNER_ID))
    .latest()
    .get(&db)
    .await?;
```

- Conditions are `Filter`s, built from typed columns (`eq`, `ne`, `gt`,
  `gte`, `lt`, `lte`, `op(op, v)`, `is_in`, `not_in`, `is_null`,
  `is_not_null`, `between`, `not_between`, `like`, `not_like`,
  `eq_column` and friends), untyped columns (`col("x")`), raw SQL
  (`Filter::raw`), sea-query conditions (`From<sea_query::Condition>`, so
  navree-access SQL filters plug in) and composition (`and`, `or`, `not`).
- The where family (`filters.rs`): `where_`, `or_where`, `where_not`,
  `or_where_not`, `where_group`, `or_where_group`, `where_in`,
  `where_not_in`, `where_in_sub`, `where_null`, `where_not_null`,
  `where_between`, `where_not_between`, `where_column`, `where_date`,
  `where_time`, `where_day`, `where_month`, `where_year`, `where_like`,
  `where_not_like` (case-insensitive by default, `_case_sensitive`
  variants), `where_json_contains`, `where_json_doesnt_contain`,
  `where_json_length`, `where_exists`, `where_not_exists`, `where_sub`,
  `where_raw`, `when`, `when_some`, each with its `or_` variant. These
  methods are stamped onto every builder by the `filters!` macro, so
  builders need no trait imports.
- Clauses (`clauses.rs`): `select`, `add_select`, `select_raw`,
  `select_expr`, `select_sub`, `distinct`, `distinct_on` (PostgreSQL),
  `join`, `left_join`, `right_join`, `cross_join`, `join_on` (join clauses
  with `on`, `or_on` and wheres), `group_by`, `group_by_raw`, `having`,
  `or_having`, `having_raw`, `order_by`, `order_by_desc`,
  `order_by_raw`, `latest`, `oldest`, `in_random_order`, `reorder`,
  `limit` / `take`, `offset` / `skip`, `for_page`.
- `Builder<M>` adds the model-aware parts: `where_key`, `where_key_not`,
  scopes, soft-delete visibility, and retrieval: `get`, `first`,
  `first_or_fail`, `find`, `find_many`, `find_or_fail`, `sole`, `value`,
  `pluck`, `pluck_keyed`, `exists`, `doesnt_exist`, `count`, `sum`, `avg`,
  `min`, `max`, `paginate`, `simple_paginate`, `cursor_paginate`, mass
  `update`, `delete`, `increment`, `decrement`, `touch`, and
  `first_or_create`, `first_or_new`, `update_or_create`.
- `TableBuilder` (`navree_orm::table("deal_tags")`) is the same builder
  without a model: rows come back as `Row`s, writes take column/value
  lists. The relations track uses it for pivot tables.
- `to_sql(Dialect)` returns the statement (SQL text and values) a builder
  compiles to, without running it.

### Scopes

Local scopes are plain functions `fn(Builder<M>) -> Builder<M>`, applied
with `query.scope(f)`. `#[scopes] impl Deal { #[scope] fn open(q:
Builder<Self>) -> Builder<Self> { .. } }` generates the trait `DealScopes`,
implemented for every `Scoped<Deal>` builder, so `Deal::query().open()`
works wherever `DealScopes` is imported. Global scopes implement
`GlobalScope<M>` and are declared with `#[model(global_scopes(..))]`;
they apply when the query runs (so `without_global_scope::<S>()` and
`without_global_scopes()` can remove them first). Like any scope, a global
scope's wheres are grouped when they contain an `or`, so a scope never
changes the meaning of the caller's conditions. Soft-deleting models get
the built-in `SoftDeletes` scope (`deleted_at IS NULL`), which
`with_trashed()` removes.

### The IR (`ir.rs`)

```rust
pub struct QueryIr {
    pub table: TableRef,              // name + optional alias
    pub distinct: Distinct,           // No, Yes, On(exprs)
    pub columns: Vec<SelectItem>,     // empty = all columns
    pub joins: Vec<Join>,
    pub wheres: Vec<WhereClause>,     // (And | Or, negated, Where)
    pub groups: Vec<Expr>,
    pub havings: Vec<WhereClause>,
    pub orders: Vec<Order>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub lock: Option<Lock>,
    pub unions: Vec<Union>,
    pub errors: Vec<String>,          // problems found while building, reported at execution
}
```

`Where` covers `Compare` (expr op expr), `Null`, `In` (list or subquery),
`Between`, `Like`, `DatePart`, `Exists`, `Group`, `JsonContains`,
`JsonLength`, `Raw` and `Sql` (a sea-query condition, SQL drivers only);
`Expr` covers columns (with JSON paths), values, raw SQL, functions,
aggregates, subqueries and the TimescaleDB helpers. Enums are
`#[non_exhaustive]`: a driver answers variants it cannot express with
`OrmError::Unsupported`. Writes are `InsertIr`, `UpdateIr` and `DeleteIr`.

## 6. Drivers and execution

### Executors

Terminal methods take `&E where E: Executor`:

| Executor | Reads | Writes |
|---|---|---|
| `Db` | the task's current unit of work when it already has an open transaction on this pool (`Db::reader`), else the pool | the task's current unfinished unit of work on this pool (changes recorded there), else the pool (autocommit) |
| `Uow` | its transaction | its transaction, changes recorded |
| `DatabaseConnection` / `DatabaseTransaction` | as given | as given, no change events |
| `Arc<E>` | as `E` | as `E` |

`Executor::target(Access)` resolves a `Target`: `Target::Sql` (a dialect, a
connection or transaction, the unit of work if any, the app if known) or
`Target::Document` (a `DocumentDriver`). Joining the current unit of work
from `&Db` follows the framework's rule for code that takes no `&Uow`:
on SQLite a write through a second pool connection would wait for the
request's write lock until `busy_timeout`.

Through a `Uow`, every statement runs in its transaction, which carries
`navree.actor`, `navree.request_id` and (with navree-workspace)
`navree.workspace` on PostgreSQL; change events reach the
`ChangeObserver`s (audit) inside the transaction; after-commit hooks run
once it is durable (the lifecycle track dispatches model events there).

`ExecContext` (built per operation from the target) gives later tracks the
app (`AppContext`, for observers, the encrypter, the morph map), the unit
of work, and `now()`.

### SQL dialects

`Dialect::{Postgres, MySql, Sqlite}` (TimescaleDB is PostgreSQL with the
extension). `sql/` compiles the IR to sea-query statements and builds them
with `PostgresQueryBuilder`, `MysqlQueryBuilder` or `SqliteQueryBuilder`;
dialect differences live in `sql/filter.rs` (date parts, case-sensitive
LIKE, JSON paths and containment), `sql/write.rs` (RETURNING, generated
keys) and `sql/timescale.rs`. Statements run through SeaORM's
`ConnectionTrait` (`query_all_raw`, `execute_raw`), so query logging and
slow-query warnings of navree-db apply.

### Document drivers (`driver.rs`)

```rust
pub trait DocumentDriver: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn find<'a>(&'a self, query: &'a QueryIr) -> BoxFuture<'a, Result<Vec<Row>, OrmError>>;
    fn aggregate<'a>(&'a self, query: &'a QueryIr, aggregate: &'a Aggregate) -> BoxFuture<'a, Result<Value, OrmError>>;
    fn insert<'a>(&'a self, insert: &'a InsertIr) -> BoxFuture<'a, Result<InsertOutcome, OrmError>>;
    fn update<'a>(&'a self, update: &'a UpdateIr) -> BoxFuture<'a, Result<u64, OrmError>>;
    fn delete<'a>(&'a self, delete: &'a DeleteIr) -> BoxFuture<'a, Result<u64, OrmError>>;
}
```

The table is the collection; wheres become filter documents (`Compare` →
`$eq`/`$gt`..., `In` → `$in`, `Like` → `$regex`, `DatePart` → `$expr` with
`$year`..., `Group` → `$and`/`$or`); orders → `sort`; columns →
`projection`; limit/offset → `limit`/`skip`; aggregates → pipelines. Rows
come back as `Row::from_values`. Relations load with batched `$in` queries
(the same `where_in` the SQL drivers use), so eager loading works
unchanged; `$lookup` is an optimisation the driver may add. Embedded
documents are `json` casts. Transactions are sessions: the driver crate
provides a session executor.

## 7. Row decoding

`Row` wraps a SeaORM `QueryResult` (SQL) or a list of `(column, Value)`
(documents). `M::from_row` reads each column with its field's exact type
(`Field::from_row`), so a `i32` field reads an `INT4` column on PostgreSQL
without conversions. Missing columns are an error naming the model, the
column and the selected columns (or `Default` for `#[model(default)]`
fields). Columns that are not model columns are decoded dynamically into
the record's `extras`. Aggregates decode the single column exactly when the
requested type matches and leniently otherwise (`SUM` of an integer column
is `NUMERIC` on PostgreSQL and `INTEGER` on SQLite; `sum::<i64>` reads
both).

## 8. Errors

`OrmError` (`#[non_exhaustive]`):

| Variant | When | Over HTTP |
|---|---|---|
| `ModelNotFound { model, keys }` | `first_or_fail`, `find_or_fail`, `sole`, `refresh` | 404 (its source is `DbErr::RecordNotFound`, which navree-db maps) |
| `MultipleRecordsFound { model, count }` | `sole` | 500 |
| `Db(DbErr)` | the database refused | 409 for unique and foreign-key violations (navree-db's mapping), else 500 |
| `Decode { model, column, message }` | a column cannot be read as its field | 500 |
| `Cast { model, column, message }` | a cast failed | 500 |
| `InvalidQuery(String)` | an invalid operator, a JSON path, `between` without two values... | 500 |
| `Unsupported { feature, driver }` | a feature the dialect or driver lacks | 500 |
| `MissingKey { model }`, `NotPersisted { model, action }` | operations that need a stored record | 500 |

Messages name the model, the column and the operation; no SQL or values
reach HTTP clients (navree-http logs the source of 500s).

## 9. Casts, collections, pagination

`Cast<T>` (`cast.rs`): `type Stored: Field`, `get(Stored) -> Result<T>`,
`set(&T) -> Result<Stored>`. Built-in: `"json"` (`AsJson`, any serde
type), `"string_enum"` (`StringEnum`, unit enums by serde name),
`"int_enum"` (`IntEnum`, `Into<i64> + TryFrom<i64>`). A cast on an
`Option<T>` field casts the inner value. Native field types cover bool,
integers, decimals, floats, strings, bytes, UUIDs, ULIDs, dates, times and
date-times (UTC and offset).

`Collection<M>` holds `Record<M>`s: `iter`, `map`, `filter`, `find`
(by key), `contains`, `first`, `last`, `key_by`, `group_by`, `pluck`,
`model_keys`, `diff`, `intersect`, `unique`, `only`, `except`,
`sort_by`, `to_query`, `fresh`, `into_vec`, serialization as an array.

Pagination: `paginate(exec, page)` returns navree-db's `Page<Record<M>>`
(`{data, meta: {current_page, per_page, total, last_page}}`; the page is a
number, which uses the model's `per_page`, or a `PageQuery`);
`simple_paginate` returns `SimplePage` (`{data, meta: {current_page,
per_page, has_more}}`); `cursor_paginate` returns `CursorPage`
(`{data, next_cursor, prev_cursor}`) over the builder's orders (the key
is added as a tie-breaker), with `CursorRequest` as the extractor.

## 10. The feature tracks

Each track owns new files and extends core types with inherent `impl`
blocks in those files. Shared files get one-line additions only
(`lib.rs` module lines and re-exports, the facade prelude). The seams
below exist in the core so tracks fill them instead of editing core flows.

### orm-relations

- Owns `src/relations.rs` (split into `src/relations/*.rs`: `belongs_to.rs`,
  `has_one.rs`, `has_many.rs`, `belongs_to_many.rs`, `through.rs`,
  `morph.rs`, `eager.rs`, `existence.rs`, `aggregates.rs`, `pivot.rs`,
  `morph_map.rs`).
- Core provides: `Relation<P>` (associated `Related`, `name`), the
  per-record store `Relations` (`Record::relation::<T>(name)`,
  `set_relation`, `relation_loaded`, `unset_relation`; values are
  `Option<Record<R>>`, `Record<R>` or `Collection<R>`, serialized with the
  record), `Builder::into_ir`, `TableBuilder` for pivots, `where_in` for
  batched loads, `Filter::exists`/`where_exists` for existence queries,
  and the `hooks::saved` / `hooks::deleted` seams for touching parents.
- Adds: relation definitions (methods returning typed relation builders,
  registered by name for string paths), `with`, `with_count`, `with_sum`
  / `avg` / `min` / `max` / `with_exists`, `load`, `load_missing`,
  `load_count` (records and collections), `has`, `where_has`,
  `doesnt_have`, `where_relation`, `where_belongs_to`, writing relations,
  pivots, the morph map, strict lazy loading.
- Macro attributes: `#[model(touches = [..])]`, `#[model(morph_name =
  "..")]`, and any relation-declaration attribute, in
  `navree-orm-macros/src/model/relations.rs`, wired into the container and
  field parsers (`model/container.rs`, `model/field.rs`) with one line
  each.

### orm-lifecycle

- Owns `src/events.rs` (model events, observers, `without_events`),
  `src/soft_deletes.rs` (`restore`, `force_delete`, `only_trashed`,
  `trashed` events), `src/prune.rs` (`Prunable`, `MassPrunable`,
  `model:prune`), `src/strict.rs`, and `src/module.rs` (`OrmModule`: global,
  registers observers and the `model:prune` command).
- Fills the seams in `src/hooks.rs`: `before(event)` (cancellable
  `saving`, `creating`, `updating`, `deleting`, `restoring`,
  `force_deleting`), `after(event)` (`retrieved`, `created`, `updated`,
  `saved`, `deleted`, `restored`, `force_deleted`, `replicating`), each
  given the `ExecContext` (app, unit of work) and the record. Events
  dispatch through navree-events, after commit when a unit of work is
  present (`uow.after_commit`).
- Completes global scopes: runtime registration through `OrmModule`,
  `without_global_scopes_except`, scope removal by name.
- Macro attributes: `#[model(observers(..))]`, `#[model(prunable)]` in
  `navree-orm-macros/src/model/lifecycle.rs`.

### orm-attributes

- Owns `src/attributes/` (`fill.rs`: `fill`, `force_fill`, `fillable` /
  `guarded`; `casts.rs`: encrypted, encrypted JSON, hashed (Argon2 via
  navree-crypto), timestamps as integers, arrays and maps, custom
  inbound-only casts; `accessors.rs`; `serialize.rs`: `hidden`, `visible`,
  `appends`, `to_json`, API resources and OpenAPI schemas).
- Extends `Cast` with a context (`ExecContext::app` gives the
  `Encrypter` and `Hasher`) without changing the built-in casts' behaviour.
- Takes over `Record`'s `Serialize` (`record.rs` keeps the impl; the
  attributes track routes it through `serialize.rs`).
- Macro attributes: `fillable`, `guarded`, `hidden`, `visible`,
  `appends`, field `cast = "encrypted" | "hashed" | ...`, accessor and
  mutator declarations, in `navree-orm-macros/src/model/attributes.rs`.

### orm-streams

- Owns `src/streams.rs` (`chunk`, `chunk_by_id`, `each`, `lazy`,
  `lazy_by_id`, `cursor` as async streams), `src/query/bulk.rs` (`insert`,
  `insert_or_ignore`, `upsert` of many rows, `delete_where`), locking
  (`lock_for_update`, `shared_lock` in `clauses.rs`, the IR already has
  `lock`), unions, sub-joins and lateral joins, PostgreSQL JSON operators
  and full-text search (`sql/filter.rs` additions under clearly separated
  functions), and multiple connections (`#[model(connection)]` resolution
  through `ExecContext::app`).

### orm-factories

- Owns `src/factory/` (`Model::factory()`, states, sequences, `count`,
  `has` / `for`, `recycle`, `make` / `create`, after-making and
  after-creating hooks), `src/testing.rs` (`assert_database_has`,
  `assert_database_missing`, `assert_model_exists`, `assert_soft_deleted`,
  `assert_database_count`), `src/binding.rs` (route model binding:
  `Bound<M>` extractor, soft-delete aware, workspace scoped, 404 in the
  error shape), the workspace-scoping trait, seeders on models, and
  `navree make:model` in navree-cli.

### mongodb

- Creates `crates/navree-orm-mongodb` (L5): implements `DocumentDriver`
  (section 6), a client executor and a session executor (`Executor`
  returning `Target::Document`), BSON conversion for `Value`, ObjectId
  keys (`KeyKind::Manual` with a generated hex id, or a `ObjectId` field
  type implementing `Field`), and time-series collections for the schema
  builder's engine helpers. Integration tests run against
  `NAVREE_TEST_MONGODB_URL`.

## 11. Testing

- SQLite runs every behaviour test (`tests/orm/`, a fresh temporary file
  per test).
- SQL generation is pinned per dialect by snapshot tests (`tests/sql/`):
  each snapshot shows the PostgreSQL, MySQL and SQLite statement and its
  values.
- Real-server tests are gated: `NAVREE_TEST_POSTGRES_URL` (with the
  `postgres` feature), `NAVREE_TEST_MYSQL_URL` (with the `mysql`
  feature), `NAVREE_TEST_MONGODB_URL` (mongodb crate); they skip with a
  notice when the variable is unset.
