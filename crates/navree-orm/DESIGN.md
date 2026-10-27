# navree-orm: architecture

This is the map of the Navree ORM: how its crates fit together, what
`#[derive(Model)]` generates, how a query becomes SQL (or a document
command) and runs on a `Db` or inside a `Uow`, and where every later feature
plugs in. Sections 1 to 10 describe the core as shipped; section 11 assigns
the remaining features to the feature tracks, naming the files, traits,
seams and macro attributes each one owns, so they can work in parallel with
few conflicts.

Principles:

- **Rust-shaped.** Models are plain structs. Columns are typed constants
  (`Deal::STAGE: Column<Deal, Stage>`), conditions are typed values
  (`Deal::AMOUNT_CENTS.gte(10_000)`), keys are typed, everything that can
  fail returns `Result`, everything that touches the database is `async`
  and takes the executor first. No runtime reflection: the derive writes
  the metadata.
- **String-flexible where it pays.** Untyped columns (`col("deals.title")`,
  JSON paths `col("meta->status")`), raw SQL with portable `?` placeholders
  (`where_raw`, `select_raw`, `order_by_raw`), and later relation paths
  (`"company.owner"`).
- **Built on navree-db.** Connections, the pool, the unit of work (`Uow`:
  transaction, actor, request id, change events, after-commit hooks,
  `set_config` for audit and workspace row-level security), migrations and
  the page shape stay in navree-db. Every statement runs through them, so
  audit trails, workspace isolation and after-commit work keep working.
  SeaORM entities keep working beside ORM models (same pool, same values).
- **One driver abstraction.** A query is first a driver-neutral value (the
  IR, `navree_orm::ir`), then SQL for PostgreSQL/TimescaleDB, MySQL or
  SQLite (sea-query's builders), or a document command (a `DocumentDriver`
  a separate crate implements).

## 1. Crates

| Crate | Layer | Role |
|---|---|---|
| `navree-orm` | L4 | Models, records, collections, the builders, the IR, SQL per dialect, execution on `Db`/`Uow`, casts, scopes, hooks, pagination, the relation and document-driver seams |
| `navree-orm-macros` | L0 | `#[derive(Model)]`, `#[scopes]` and `#[relations]` (proc macros; no runtime navree dependencies) |
| `navree-orm-mongodb` | L5 | MongoDB: implements `DocumentDriver` (mongodb track) |
| `navree-schema` | L3/L4 | Migrations and the schema builder (own design; the ORM does not depend on it) |

`navree-orm` depends on navree-kernel, navree-contracts, navree-http,
navree-db and navree-orm-macros. Its HIERARCHY row
(`crates/navree/tests/dependency_graph.rs`) also allows navree-config,
navree-telemetry, navree-events and navree-crypto, for the lifecycle and
attributes tracks. It re-exports the macros, so `use navree_orm::Model;`
imports the trait and the derive at once.

The facade turns it on with the `orm` feature (in `full`): `navree::orm` is
the whole crate; `navree::prelude` brings `Model` (trait and derive),
`Record`, `Collection` and `Builder`; `navree_orm::prelude` has every name
an app writes. The facade test treats navree-orm-macros as a companion
proc-macro crate of navree-orm (`COMPANION_MACROS` in
`crates/navree/tests/facade.rs`).

Features of navree-orm: `postgres` (navree-db's PostgreSQL driver),
`mysql` (SeaORM's MySQL driver; once navree-db has its own `mysql` feature,
this one forwards to it). SQL for every dialect is generated without any
feature.

### Paths in generated code

The macros name the runtime crate `::navree_orm` when the crate being
compiled depends on navree-orm (navree-orm itself names itself so through
`extern crate self as navree_orm;`, which also serves its tests and
doctests), else `::navree::orm` through the facade, honouring a renamed
dependency (`navree-orm-macros/src/paths.rs`). Generated code calls only
`#krate::...` items and `#krate::__private` (the derive's support
functions; not API).

## 2. Module layout of `navree-orm`

```text
src/
  lib.rs            crate docs (a runnable example), re-exports, `extern crate self`, `__private`
  prelude.rs        the app-facing names
  error.rs          OrmError, Result
  value.rs          Value (sea-query's), Field, FieldError, ValueFor, Numeric, to_json, same_value
  ulid.rs           Ulid (time-ordered 128-bit ids, Crockford base32)
  row.rs            Row: one result row (SQL or document); typed and dynamic reads; Serialize
  model.rs          Model trait (+ the static API), ModelMeta, ColumnMeta, KeyKind, PrimaryKey, Timestamp, derive_support
  column.rs         Column<M, T>, col, col_as, Untyped, Filter, Assignment, ColumnName, ColumnOf, IntoExpr, IntoQuery
  record.rs         Record<M>: the instance; dirty tracking; extras; relations; the instance API
  persist.rs        insert / update / delete / refresh / touch / increment flows
  hooks.rs          ModelEvent, Flow, ModelHook<M>, Hooks<M> (resolution and dispatch)
  collection.rs     Collection<M>
  pagination.rs     PageRequest, SimplePage, CursorPage, CursorRequest (extractor), Builder::{paginate, simple_paginate, cursor_paginate}
  cast.rs           Cast, CastError, AsJson, StringEnum, IntEnum, encode/decode helpers
  scope.rs          GlobalScope, Scoped, SoftDeletes, scope grouping
  expr.rs           Expr constructors: col, val, raw, func, aggregates, arithmetic, subquery, time_bucket, first, last
  relations/        relationships (section 11, orm-relations): definitions, relation queries,
                    eager and lazy loading, existence, aggregates, pivots, morphs, touches
  driver.rs         DocumentDriver, InsertOutcome
  doc_models.rs     `__doc_models` (hidden): the models the doc examples use, so every example compiles
  exec.rs           Executor, Access, Target, SqlTarget, DocumentTarget, ExecContext
  ir.rs             QueryIr, Where, Expr, Order, Join, TableRef, FullText, InsertIr, UpdateIr, DeleteIr, ...
  query/
    mod.rs          Builder<M> (state, keys, scopes, into_ir, to_sql)
    filters.rs      the where family (`filters!`), WhereGroup, HasWheres
    clauses.rs      select/join/group/having/order/limit/union/lock (`clauses!`), HasQuery, IntoSelect
    join.rs         JoinClause
    fetch.rs        retrieval, values, plucks, existence, aggregates; Selectable
    write.rs        mass update/delete/increment/touch; first_or_new/create, create_or_first, update_or_create
    table.rs        TableBuilder, table(): rows of any table, inserts/updates/deletes of rows
  sql/
    mod.rs          Dialect, Statement, Compiler, expressions, JSON paths, raw placeholders
    select.rs       SELECT, aggregates over queries, EXISTS
    filter.rs       WHERE/HAVING/ON trees; dialect rules for dates, LIKE, JSON, full text, operators
    write.rs        INSERT (RETURNING), UPDATE, DELETE
    timescale.rs    time_bucket, first, last (native on PostgreSQL; fixed-width buckets emulated elsewhere)
```

## 3. Models and `#[derive(Model)]`

```rust
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "deals", timestamps, soft_deletes, per_page = 25)]
pub struct Deal {
    pub id: Uuid,                        // the key: a UUID v7 is generated when nil
    pub company_id: Option<i64>,
    pub title: String,
    #[model(cast = "string_enum")]
    pub stage: Stage,                    // stored as its serde name
    #[model(cast = "json")]
    pub meta: Option<DealMeta>,          // JSON; NULL is None
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}
```

Container attributes: `table` (default: the snake_case plural of the
struct's name, with English inflection: `Company` → `companies`, `Person` →
`people`), `primary_key` (default `id`), `key_type = "int" | "uuid" |
"uuid_v7" | "uuid_v4" | "ulid" | "manual"` (`"integer"` and
`"increments"` also mean `"int"`; the default follows the key's type:
`i16`/`i32`/`i64`/`u32`/`u64` → int, `Uuid` → UUID v7, `Ulid` → ULID,
anything else (`String`) → manual),
`timestamps` or `timestamps(created_at = "..." | false, updated_at = "..."
| false)`, `soft_deletes` or `soft_deletes = "column"` (the field must be an
`Option`), `connection = "name"`, `per_page = N`, `global_scopes(expr,
...)` (expressions, each an `impl GlobalScope<Self>`).

Field attributes: `column = "name"`, `cast = "json" | "string_enum" |
"int_enum" | path::To::Cast` (also as a string; a lowercase word that is
not a built-in is an error), `default` (a row without the column reads as
the field's default), `skip` (not a column; `Default` when read).

The derive generates:

- `impl Model`: `type Key`, `meta()` (a `&'static ModelMeta` built in a
  `static`), `key`/`set_key`, `from_row`, `values` (every column as `(name,
  Value)`, field order), `value(column)`, `set_value(column, Value)`,
  `touch_timestamp`, `timestamp_is_empty`, `clear_timestamp`,
  `timestamp_value`, and `global_scopes` when declared.
- One typed constant per column, named after the field in
  SCREAMING_SNAKE_CASE, with the struct's visibility, typed like the field:
  `Deal::TITLE: Column<Deal, String>`, `Deal::COMPANY_ID: Column<Deal,
  Option<i64>>`. A nullable column takes plain values in conditions
  (`Deal::COMPANY_ID.eq(7)`) and `None` (`IS NULL`), and plucks as
  `Option`s. Constants of cast fields encode and decode through the cast.

Errors are compile errors with spans and fixes (unknown keys suggest the
closest one; missing key and timestamp fields, `Option` keys, casts on
keys, duplicate columns, non-nullable soft-delete columns). Malformed
attributes are reported alone (no cascade). Trait bounds carry
`#[diagnostic::on_unimplemented]` notes (`PrimaryKey`, `Timestamp`,
`Numeric`, `ValueFor`). The cases are pinned by `tests/compile_fail.rs`.

`Model` requires `Clone + Send + Sync + Serialize + 'static`. Its provided
methods are the static API: `query`, `where_`, `all`, `find`,
`find_or_fail`, `find_many`, `create`, `destroy`, `first_or_create`,
`first_or_new`, `create_or_first`, `update_or_create`, plus
`global_scopes()` (written by the derive from `#[model(global_scopes(..))]`)
and `hooks()` (empty in core; the derive writes it for model-level hooks
such as the relations track's `touches`, while app-wide hooks come from
the container).

### Keys

`KeyKind::Increments`: the database assigns the key; a record whose key is
empty (`0`) inserts without it and reads it back (`RETURNING` on PostgreSQL
and SQLite, `LAST_INSERT_ID()` on MySQL, returned rows from a document
driver). `UuidV7`, `UuidV4`, `Ulid`: generated in Rust when empty. `Manual`:
never generated; inserting an empty one is `OrmError::MissingKey`. One key
column per model. `Ulid` is stored as its 26-character text.

### Values and fields

`Value` is sea-query's value enum, so the ORM binds exactly as SeaORM does
(typed NULLs included). `Field` is the trait of Rust types a column holds:
`to_value`, `null`, `from_sql(&QueryResult, column)` and
`from_value(Value)`. Implemented for `bool`, the 8- to 64-bit integers,
`f32`, `f64`, `Decimal`, `String`, `Vec<u8>`, `Uuid`, `Ulid`, `NaiveDate`,
`NaiveTime`, `NaiveDateTime`, `DateTime<Utc>`, `DateTime<FixedOffset>`,
`serde_json::Value`, `Value` and `Option<T>`. Decoding is exact first, then
tolerant where databases differ: any integer width (PostgreSQL's decoders
are strict), exact `NUMERIC`/`DECIMAL` sums read as integers, floats from
decimals, UUIDs from text or bytes, date-times and JSON from text (SQLite,
MySQL expressions).

`ValueFor<T>` is what a column of `T` accepts: `T`, `T` for an `Option<T>`
column, text for text columns, lossless integer and float widenings, and
everything a `Value` is built from for untyped columns.

## 4. Records

`Record<M>` is a model instance as the ORM knows it; it dereferences to
`M`, so fields are read and written directly.

| Field | Meaning |
|---|---|
| `model` | the model |
| `original: Option<M>` | the model as last read or saved (`None` while new) |
| `exists`, `recently_created` | persistence state |
| `changes`, `previous` | what the last save changed: new and old values |
| `extras` | selected columns that are not model columns (`select_raw`, `select_sub`, `with_count` results, pivot columns) |
| `relations` | loaded relations (section 11, relations) |

Instance API: `new` (also `From<M>`), `model`, `model_mut`, `key`,
`save`, `update(exec, |m| ..)`, `delete`, `refresh`,
`fresh`, `replicate`, `touch`, `touch_column`, `increment[_with]`,
`decrement[_with]`, `is_dirty[_on]`, `is_clean[_on]`, `was_changed[_on]`,
`get_dirty`, `get_changes`, `get_previous`, `get_original`,
`get_original_value`, `sync_original[_on]`, `discard_changes`, `exists`,
`was_recently_created`, `trashed`, `is`/`is_not`, `extras`, `extra::<T>`,
`set_extra`, `relations[_mut]`, `into_inner`.

Dirty tracking compares stored values (`same_value`): NULLs regardless of
type, floats within a relative tolerance, decimals numerically, date-times
by instant, JSON structurally. A new record is dirty in every column.

Records serialize as their model, then the extras, then the loaded
relations (`Serialize for Record<M>`; the attributes track routes it through
its serializer).

### Persistence flows (`persist.rs`)

Insert: `saving` → `creating` (both cancellable) → generate an empty key →
fill empty `created_at`/`updated_at` → `INSERT` → read a database-assigned
key back → `exists`, `recently_created` → `created` change event →
`created` → `saved` → sync the original.

Update (dirty records only): `saving` → `updating` → `updated_at` unless the
caller changed it → `UPDATE` of the dirty columns by the original key →
sync the changes → `updated` change event (old and new values of the
changed columns) → `updated` → `saved` → sync. A clean record runs `saving`
and `saved` and writes nothing.

Delete: `deleting` (cancellable) → `DELETE`; a soft-deleting model sets the
deleted-at (and updated-at) column instead → `deleted` change event (the
whole old record) → `deleted` (and `trashed` for a soft delete). Deleting a
record that is not stored returns `false`.

Increment/decrement: in memory and as `SET column = column ± amount` (plus
extras and `updated_at`), `updating` → `UPDATE` → `updated`; overflow is an
error; a record that is not stored is `NotPersisted` (never a whole-table
update).

`refresh`/`fresh` read by the original key without global scopes (a
trashed record reloads). `replicate` copies the columns without key and
creation/update timestamps.

Change events go to the unit of work the statement ran in (`&Uow`, or the
task's current unit of work that `&Db` joined), like `navree_db::Repo`:
`entity` is the table, `id` the key text, values are the stored column
values as JSON (decimals as exact strings). Mass writes record no events.

Timestamps come from the app's `dyn Clock` when the executor's pool belongs
to an app (`Db::app()`), else `Utc::now()`, truncated to microseconds so the
stored and in-memory values agree on every database.

## 5. Queries

`Deal::query()` returns `Builder<Deal>`. Builder methods take `self` and
return `Self`; terminal methods take the executor first.

- The where family (`filters!`, on every builder, `WhereGroup` and
  `JoinClause`): `where_`, `or_where`, `where_not`, `or_where_not`,
  `where_group`, `or_where_group`, `where_not_group`, `where_in`,
  `or_where_in`, `where_not_in`, `or_where_not_in`, `where_in_sub`,
  `where_not_in_sub`, `where_null`, `or_where_null`, `where_not_null`,
  `or_where_not_null`, `where_between` (and `or_`/`not_` forms),
  `where_column`, `or_where_column`, `where_sub`, `where_date`,
  `where_time`, `where_day`, `where_month`, `where_year` (and `or_` forms),
  `where_like`, `or_where_like`, `where_not_like`, `or_where_not_like`,
  `where_like_case_sensitive`, `where_json_contains`,
  `or_where_json_contains`, `where_json_doesnt_contain`,
  `where_json_length`, `where_exists`, `or_where_exists`,
  `where_not_exists`, `or_where_not_exists`, `where_full_text`,
  `where_full_text_with`, `or_where_full_text`, `where_raw`,
  `or_where_raw`, `when`, `when_else`, `when_some`, `unless`. Conditions
  are `Filter`s: from typed columns (`eq`, `ne`, `gt`, `gte`, `lt`, `lte`,
  `op`, `null_safe_eq`, `is_in`, `not_in`, `in_subquery`, `is_null`,
  `between`, `like`, `eq_column`, ...), expressions, raw SQL, sea-query
  conditions (`From<sea_query::Condition>`, so navree-access SQL filters
  plug in) and `and`/`or`/`not`/`all`/`any`.
- Clauses (`clauses!`): `select` (replaces), `add_select`, `select_expr`,
  `select_raw`, `select_sub`, `select_time_bucket`, `select_first`,
  `select_last` (these add, keeping a model builder's columns), `distinct`,
  `distinct_on` (PostgreSQL), `join`, `join_on`, `left_join[_on]`,
  `right_join[_on]`, `cross_join`, `join_sub`, `left_join_sub`,
  `join_sub_on`, `group_by`, `group_by_raw`, `having`, `or_having`,
  `having_raw`, `or_having_raw`, `order_by`, `order_by_desc`,
  `order_by_dir`, `order_by_raw` (a trailing `ASC`/`DESC`/`NULLS ...` is
  honoured), `order`, `latest_by`, `oldest_by`, `in_random_order`,
  `reorder`, `reorder_by`, `limit`/`take`, `offset`/`skip`, `for_page`,
  `union`, `union_all`, `lock_for_update`, `shared_lock`.
- `Builder<M>` adds `where_key`, `where_key_not`, `where_key_in`,
  `where_key_not_in`, `scope`, `without_global_scope::<S>`,
  `without_global_scopes`, `with_trashed`, `latest`/`oldest` (by the
  model's creation timestamp; an error when it keeps none), `into_ir`,
  `to_sql(dialect)`, and running: `get`, `first`, `first_or_fail`, `sole`,
  `find`, `find_or_fail`, `find_many`, `value`, `pluck`, `pluck_keyed`,
  `exists`, `doesnt_exist`, `count`, `count_column`, `count_distinct`,
  `sum`, `avg`, `min`, `max`, `aggregate`, `paginate`, `simple_paginate`,
  `cursor_paginate`, mass `update`, `delete`, `increment[_with]`,
  `decrement[_with]`, `touch`, and the find-or-create family.
- A model query selects the model's columns qualified by its table, so
  joins never confuse columns; `select` replaces them (decoding then needs
  the model's columns or `#[model(default)]` fields).
- `value`/`pluck` read through `Selectable`: a typed column gives its type
  (casts included), a column name or `Expr` gives a `Value`. Aggregates take
  the result type: `sum::<i64>`, `avg::<Decimal>`.
- Mass writes (`update`, `delete`, `increment`, `touch`) run one statement,
  without hooks or change events; a soft-deleting model's `delete` sets the
  deleted-at column. With joins, groups, unions, `DISTINCT`, a limit or an
  offset they run as `WHERE key IN (SELECT key FROM (SELECT key ...) AS
  navree_keys)`: the inner query keeps its order with a limit or offset,
  and is `DISTINCT` without orders otherwise (MySQL refuses a merged
  subquery on the written table; the derived table is materialized).
  Setting a JSON path in a mass update is `Unsupported` in core (streams
  track).
- `create_or_first` inserts inside a savepoint when the target is a
  transaction (PostgreSQL aborts a transaction on a failed statement), and
  returns the existing record on a unique violation; `first_or_create` and
  `update_or_create` build on it.
- `TableBuilder` (`navree_orm::table("deal_tags")`, `"t as alias"`,
  `TableBuilder::from_sub`) has the same where and clause methods, returns
  `Row`s, and writes rows (`insert` of many rows, `update`, `delete`,
  `increment`). Writes take plain conditions only.

### Scopes

Local scopes are functions of the builder: `query.scope(f)` applies one;
`#[scopes] impl Deal { #[scope] fn open(q: Builder<Self>) -> Builder<Self>
}` generates the trait `DealScopes` (public unless `vis = "..."`; `name =
"..."` renames it), implemented for every `Scoped<Deal>`, so
`Deal::query().open().worth(10_000)` works wherever the trait is imported.
`Self` in a scope's parameters means the model. Global scopes implement
`GlobalScope<M>` (`apply`, `scope_id`) and come from `M::global_scopes()`
plus `SoftDeletes` for soft-deleting models; they apply once, when the
query runs (`Builder::apply_global_scopes`), unless removed. Any scope's
conditions are grouped when they or the conditions before them contain an
`OR`, so a scope never changes the meaning of the caller's conditions.

### The IR (`ir.rs`)

`QueryIr { table: TableRef, distinct, columns: Vec<SelectItem>, joins,
wheres: Vec<WhereClause>, groups, havings, orders, limit, offset, lock,
unions, errors }`. `TableRef` is a table, an aliased table, or a subquery.
`Where` covers `Compare`, `Null`, `In` (list or subquery), `Between`,
`Like`, `DatePart`, `Exists`, `Group`, `JsonContains`, `JsonLength`,
`FullText`, `Raw`, `Sql` (a sea-query condition) and `Invalid` (a problem
found while building, reported when the query runs). `Expr` covers
columns (with JSON paths), values, raw SQL, functions, aggregates,
arithmetic, subqueries, `TimeBucket`, `First`, `Last` and `All`. The enums
are `#[non_exhaustive]`: a driver answers what it cannot express with
`OrmError::Unsupported`. Writes are `InsertIr` (with `returning`),
`UpdateIr` and `DeleteIr`.

## 6. Drivers and execution

### Executors

Terminal methods take `&E where E: Executor + ?Sized`:

| Executor | Reads | Writes |
|---|---|---|
| `Db` | the task's current unit of work when its transaction is open on this pool (`Db::reader`), else the pool | the task's current unfinished unit of work on this pool (events recorded there), else the pool |
| `Uow` | its transaction | its transaction, events recorded |
| `DatabaseConnection`, `DatabaseTransaction` | as given | as given, no events |
| `Arc<E>` | as `E` | as `E` |
| a document store's executor | `Target::Document` | `Target::Document` |

`Executor::target(Access)` resolves a `Target`: `Target::Sql(SqlTarget)`
(dialect, connection or transaction, unit of work, app) or
`Target::Document(DocumentTarget)` (driver, app). `Target` runs IR:
`fetch`, `aggregate`, `exists`, `insert`, `update`, `delete`, and nests a
savepoint (`begin_savepoint`, `on_transaction`). `ExecContext` (from
`Target::context()`) gives hooks and later tracks the app, the unit of
work, the dialect and `now()`.

Inside a `Uow`, statements run in its transaction, which carries
`navree.actor` and `navree.request_id` (and, with navree-workspace,
`navree.workspace` and the `search_path`) on PostgreSQL; the real-server
tests show a trigger reading `navree.actor` from an ORM insert. Change
observers (audit) see the events at commit; after-commit hooks run once it
is durable.

### SQL dialects

`Dialect::{Postgres, MySql, Sqlite}` (TimescaleDB is PostgreSQL with the
extension). `sql/` compiles the IR into sea-query statements built with
the dialect's query builder; values are always bound. Dialect rules live
next to the clause: date parts (`CAST`/`EXTRACT`, `DATE()`..., `strftime`),
case-insensitive LIKE (`ILIKE`, `LOWER() LIKE LOWER()` / MySQL's
collation, SQLite's LIKE) and case-sensitive LIKE (`LIKE`, `LIKE BINARY`,
`GLOB`), null-safe equality, regular expressions, XOR, JSON paths
(`->`/`->>`; `json_extract`), JSON containment and length, full text
(`to_tsvector @@ plainto/phraseto/websearch_to_tsquery` with a validated
language; `MATCH ... AGAINST` in natural language, expansion or boolean
mode; unsupported on SQLite), `RETURNING`, `LIMIT` before `OFFSET` on
MySQL and SQLite, row locks (none on SQLite, whose write transactions are
serialised), and the TimescaleDB helpers (native on PostgreSQL; fixed-width
buckets emulated on MySQL and SQLite with TimescaleDB's origin, Monday
2000-01-03; `first`/`last` need TimescaleDB). On PostgreSQL a JSON text
path (`->>`) is compared with the JSON text of a non-string value.
Statements run through SeaORM's `ConnectionTrait`, so navree-db's query
log, slow-query warnings and telemetry apply.

### Document drivers (`driver.rs`)

```rust
pub trait DocumentDriver: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn find<'a>(&'a self, query: &'a QueryIr) -> BoxFuture<'a, Result<Vec<Row>, OrmError>>;
    fn aggregate<'a>(&'a self, query: &'a QueryIr, aggregate: &'a Aggregate) -> BoxFuture<'a, Result<Value, OrmError>>;
    fn exists<'a>(&'a self, query: &'a QueryIr) -> BoxFuture<'a, Result<bool, OrmError>>; // default: a count
    fn insert<'a>(&'a self, insert: &'a InsertIr) -> BoxFuture<'a, Result<InsertOutcome, OrmError>>;
    fn update<'a>(&'a self, update: &'a UpdateIr) -> BoxFuture<'a, Result<u64, OrmError>>;
    fn delete<'a>(&'a self, delete: &'a DeleteIr) -> BoxFuture<'a, Result<u64, OrmError>>;
}
```

The table is the collection; wheres become a filter document; orders the
sort; `columns` the projection, **each under its alias when it has one**
(`value`/`pluck` select `<column> AS value`); `limit`/`offset` the limit and
skip; aggregates a pipeline. Keys the insert asks for (`returning`) come
back as returned rows. Rows are `Row::from_values`, decoded leniently by
`Field::from_value`. `tests/orm/documents.rs` is a reference: an in-memory
driver running the model API (create with store-assigned keys, queries,
counts, pagination, saves, deletes, plucks) and refusing what it cannot
express.

## 7. Row decoding

`Row` wraps a SeaORM `QueryResult` or `(column, value)` pairs. `M::from_row`
reads each column with its field's decoder; a missing column is an error
naming the model, the column and the selected columns (or the default for
`#[model(default)]` fields). Columns that are not model columns become the
record's extras, decoded by their own type (`row::sniff`). Aggregates and
selected values decode leniently into the requested type.

## 8. Errors

`OrmError` (`#[non_exhaustive]`):

| Variant | When | Over HTTP |
|---|---|---|
| `ModelNotFound { model, keys }` | `first_or_fail`, `find_or_fail`, `sole`, `refresh` | 404 (its source is `DbErr::RecordNotFound`, which navree-db maps) |
| `MultipleRecordsFound { model }` | `sole` | 500 |
| `Db(DbErr)` | the database refused | 409 for unique and foreign-key violations (navree-db's mapping), else 500 |
| `Decode`, `Cast` | a column cannot be read as its field / a cast failed | 500 |
| `InvalidQuery(String)` | an unknown operator, wrong raw placeholders, a malformed cursor, an overflow, ... | 500 |
| `Unsupported { feature, driver }` | a feature the dialect or driver lacks | 500 |
| `NotPersisted`, `MissingKey` | an operation needing a stored record; an empty manual key | 500 |
| `Hook { model, message }`, `Driver { driver, message }` | a hook or a document driver failed | 500 |

`is_not_found()` and `is_unique_violation()` classify. Messages name the
model, the column and the operation; no SQL or values reach clients
(navree-http logs the source of 500s).

## 9. Casts, collections, pagination

`Cast<T>`: `type Stored: Field`, `get(Stored) -> Result<T, CastError>`,
`set(&T) -> Result<Stored, CastError>`; static and synchronous. Built-in:
`"json"` (`AsJson`, any serde type), `"string_enum"` (`StringEnum`),
`"int_enum"` (`IntEnum`: `Copy + Into<i64> + TryFrom<i64>`). A cast on an
`Option<T>` field casts the inner value.

`Collection<M>` dereferences to `[Record<M>]` (and is built from and
iterates as records: `From<Vec<_>>`, `FromIterator`, `Extend`,
`IntoIterator`) and adds `new`, `push`, `find`, `find_mut`,
`contains`, `model_keys`, `map`, `filter`, `partition`, `pluck`, `key_by`
and `group_by` (insertion-ordered `IndexMap`s), `diff`, `intersect`,
`unique`, `only`, `except`, `sort_by`, `sort_by_key`, `reverse`,
`to_query`, `fresh`, `sole`, `into_models`, `into_vec`; it serializes as an
array.

Pagination: `paginate(exec, page)` returns navree-db's `Page<Record<M>>`
(`{data, meta: {current_page, per_page, total, last_page}}`); a page is a
number (the model's `per_page`), a `(page, per_page)` pair or navree-db's
`PageQuery` extractor. `simple_paginate` returns `SimplePage` (`{data,
meta: {current_page, per_page, has_more}}`). `cursor_paginate` returns
`CursorPage` (`{data, next_cursor, prev_cursor}`) over the query's order
(any mix of directions; the key is appended as a tie-breaker); cursors are
base64url JSON of the order columns and their typed values, rejected when
they do not match the query. `CursorRequest` is the `?cursor=&limit=`
extractor (400 for a malformed cursor or limit).

## 10. Testing

- `tests/orm/` (SQLite, a fresh file per test): derive, CRUD, dirty
  tracking, every where variant, selects, joins, groups, unions,
  aggregates, plucks, pagination, collections, scopes, soft deletes,
  timestamps with a fixed clock, hooks from an app, the unit of work
  (events, rollback, joining, after-commit), casts, HTTP (404 shape, page
  shapes, extractors, request transactions), `TableBuilder`, and the
  document-driver reference.
- `tests/sql/`: SQL snapshots per dialect (insta), each showing the
  PostgreSQL, MySQL and SQLite statement and values or the error.
- `tests/servers/`: PostgreSQL with TimescaleDB and MySQL, gated by
  `NAVREE_TEST_POSTGRES_URL` / `NAVREE_TEST_MYSQL_URL` (`cargo test -p
  navree-orm --features postgres,mysql --test servers`; a test skips with
  a notice when its variable is unset); each test runs in a schema or
  database of its own, dropped afterwards.
- `tests/compile_fail.rs` with `tests/ui/`: the macros' error messages.
- Doctests on the public items: none is `ignore`d. Examples that need a
  database are `no_run` (compiled) over the hidden `__doc_models` models
  (`# use navree_orm::__doc_models::*;`); the crate root has a runnable
  example on SQLite. New examples follow the same pattern.

## 11. The feature tracks

Each track owns new files and extends core types with inherent `impl`
blocks in its own files (Rust allows `impl Builder<M>` in any module of the
crate). Shared files get small, separate additions: `lib.rs` module lines
and re-exports, `prelude.rs`, the facade prelude, the macro parsers' key
lists. The seams below exist so tracks fill them instead of rewriting core
flows.

### orm-relations (shipped)

Relationships live in `src/relations/`:

```text
mod.rs            Relation<P>, RelatedTo<P>, the sealed Erase, derive_support (what #[relations] expands to)
define.rs         Relationships: belongs_to, has_one, has_many, belongs_to_many, has_one_through,
                  has_many_through, morph_one, morph_many, morph_to, morph_to_many, morphed_by_many,
                  through(..).has(..); KeyColumn
core.rs           RelationCore (parent + query of the related model), the builder/retrieval macros,
                  Bound (a relation's query for one parent, with a finisher), find-or-create helpers
belongs_to.rs     BelongsTo (owner key, defaults, associate/dissociate)
has.rs            HasOne, HasMany (and their morph forms), writes, inverses, per-parent limits
one_of_many.rs    latest/oldest/of_many: joins on grouped subqueries
belongs_to_many.rs, pivot.rs   BelongsToMany (and MorphToMany), Pivot, PivotKind (typed pivots),
                  RelatedId, SyncChanges, attach/detach/sync/toggle/update_existing_pivot
through.rs        HasOneThrough, HasManyThrough, composed through relations (Cardinality, ThroughLeg)
morph.rs, morph_map.rs, any.rs   MorphTo, MorphWith, MorphTypes, MorphRef, polymorphic existence,
                  MorphMap, AnyRecord, morph_types!
registry.rs       RelationKind, RelationMeta, ErasedRelation, DynRelation, RelationRegistry, find
path.rs           RelationPath, RelationPathTo, ConstrainPath, ParsedPath ("a.b:c,d as e")
eager.rs          EagerTree/EagerNode, Builder::with*, load_tree, constraint closures
existence.rs, requalify.rs   has/where_has/where_relation/where_belongs_to/where_attached_to,
                  correlated subqueries, self-relation aliases
aggregates.rs     with_count/sum/avg/min/max/exists, load_* aggregates
lazy.rs           Record and Collection load*, LazyLoading, relation_or_load, push, associate,
                  pivots on records, refresh's reload of relations, load_missing_chain
touch.rs          touches, without_touching(_on), is_ignoring_touch
store.rs, keys.rs Relations (loaded values), RelationValue; RelationKey and batched IN lists
access.rs         AccessFilter, with_accessible
```

How the pieces fit:

- **Definitions.** A `#[relations]` impl block's methods return typed
  relations built from `self` (`self.belongs_to(Deal::COMPANY_ID)`). The
  macro (`navree-orm-macros/src/relations.rs`) names each relation after
  its method and adds `__navree_relations(name)` and
  `__navree_relation_names()`; the derive's `Model::relation` and
  `relation_names` call them by inherent-method shadowing (the
  `derive_support::NoRelations` fallback answers for models without a
  block), then the `RelationRegistry` (relations other modules define).
  Definitions are read from `M::default()`: models with relations
  implement `Default` (`ModelWithRelations` reports a missing one once).
- **Typed and string paths.** Relation methods are paths (`Deal::company`)
  and fix the closure types of constrained loads and existence checks
  (`ConstrainPath<M, Q>`, `RelationPathTo<M, T>`); strings (`"company.owner"`,
  `"company:id,name"`, `"activities as open_count"`) resolve through
  `Model::relation`, and an unknown name is `OrmError::RelationNotFound`
  naming the model, the relation and the known ones.
- **Relation queries.** Every builder method works on a relation; its own
  conditions come first and the caller's are grouped (`core::constrain`),
  so an `OR` never reaches other parents' records. A NULL parent key
  relates nothing without a query.
- **Eager loading.** `Builder<M>` holds an `EagerState` (the tree, the
  model's default counts, the existence depth); `fetch` takes the tree and
  calls `eager::after_fetch`, which marks the result for the lazy-loading
  rules and loads each relation with one batched `IN` query per level
  (integer keys written into SQL, bound for document drivers; no query
  when every key is NULL). Limits in a constrained load apply per parent
  through `Expr::RowNumber` in a derived table (`navree_row`,
  `navree_table`; MySQL sorts NULLs with an `IS NULL` key).
  `#[model(with = [..], with_count = [..])]` apply in `Model::query` and
  in every relation query of the model (`Builder::with_relation_defaults`:
  the eager loads at once, the counts when the query reads records, since
  adding them earlier would resolve relations while relations are being
  defined). Cycles among default eager loads never end: avoid them.
- **Existence and aggregates.** Correlated `EXISTS` (at least one) or
  `COUNT` subqueries, nested by path; the related model's global scopes
  apply; a relation to its own table aliases the inner one `navree_{depth}`
  (`requalify` moves the caller's conditions onto the alias). Aggregates
  are correlated subqueries in the select list (`Expr::Exists` for
  `with_exists`); `load_*` runs one query of the parents' keys.
- **Pivots.** Many-to-many queries join the pivot table and select its
  columns as `pivot_*`, hydrated into a `Pivot` or the typed pivot model's
  record under the accessor (`pivot`, or `as_`). Pivot writes are atomic:
  a transaction of their own on a pool, the unit of work's otherwise;
  typed pivots write through their model, so its hooks run. A pivot model
  without an id is identified by both pivot columns (`persist::identity`).
- **Polymorphism.** Type columns hold `Model::morph_name`; a `morph_to`
  loads the models it lists (`target`, `targets`) or the `MorphMap`'s
  (process-wide `register`, `scope` for one future, `require` to refuse
  unmapped models with `MorphNotMapped`); stored ids are normalized to the
  target key type. Renaming a morph name needs a data migration rewriting
  the stored names (documented on `MorphMap`).
- **Lazy loading.** `Record::relation_or_load` / `related` follow
  `LazyLoading` (prevented: `OrmError::LazyLoading` or a handler;
  automatic: the record's result loads together, `with_relationship_
  autoloading` for one collection or record).
- **Touching.** `#[model(touches = [..])]`: `persist::save` (when the record
  changed) and `delete` call `touch::touch_owners`, which touches each
  relation's records and theirs, once per record (cycles end);
  many-to-many writes touch the parent when the related model touches the
  relation named after the parent's table.
- **Access.** `with_accessible(path, &filter)` adds an `AccessFilter`'s
  condition for each related model along the path (`morph_to`: per model).

Core edits it made: the `Builder` field and the `fetch` call; `Model::relation`,
`relation_names`, `relation_meta`, `morph_name`, `foreign_key_name`,
`touched_relations`, and `query()` applying the model's defaults (`where_`
too); `ModelMeta`'s relation metadata (`morph_name`, `foreign_key`,
`touches`, `with`, `with_count`, `pivot_keys`); `persist` (touches,
pivot-model identity, `refresh` reloading loaded relations); `hooks::quietly`
(a task-local for the `*_quietly` writes); `exec` (`begin_transaction`,
`Executor for Target`); the IR's `Expr::Exists` and `Expr::RowNumber`;
`Record::stored`. Macro attributes `touches`, `morph_name`, `with`,
`with_count` and `pivot(foreign_key, related_key)` live in
`navree-orm-macros/src/model/relations.rs`.

Seams left for other tracks:

- **orm-streams.** Relation `upsert` (filling the foreign key and morph
  type) waits for `InsertIr::on_conflict`; chunks and streams on relations
  build on `into_query()` and the relation's finisher (pivots); relations
  to models on other connections need named connections (existence
  subqueries across connections will be refused with `Unsupported`, eager
  loads will run on the related model's connection).
- **orm-lifecycle (`OrmModule`).** Boot steps: register the morph map and
  `MorphMap::require` from `[orm] require_morph_map`; install
  `LazyLoading` from `[orm] prevent_lazy_loading` / `auto_eager_load`, with
  a violation handler that logs.
- **Access.** navree-access's `Ability` implements `AccessFilter` where both
  crates meet (the facade, or an adapter crate): navree-orm cannot depend
  on navree-access.
- **Document drivers.** Belongs-to, has-one/many and morph eager loads are
  plain batched `IN` queries any driver answers; pivot and through
  relations join, and per-parent limits use a window, so they need SQL.

### orm-lifecycle

- Owns `src/events.rs` (observers as `ModelHook<M>` adapters registered by
  `OrmModule` as `dyn ModelHook<M>` multi-bindings; navree-events dispatch,
  after commit inside a unit of work through `uow.after_commit`),
  `src/soft_deletes.rs` (`restore`, `force_delete`, `only_trashed` on
  `Record`/`Builder`, firing the `Restoring`/`Restored`/`ForceDeleting`/
  `ForceDeleted` events that `ModelEvent` already has; the crate-internal
  `Builder::delete_rows` (`query/write.rs`) removes rows for good, and
  `without_global_scope::<SoftDeletes>()` reaches trashed ones),
  `src/prune.rs` (`Prunable`, `MassPrunable`, the `model:prune` command),
  `src/strict.rs` (strict modes: silently discarded attributes, missing
  attributes; lazy loading's rules are `relations::LazyLoading`, which
  `OrmModule` installs) and `src/module.rs` (`OrmModule`, which other
  tracks extend with their boot steps as separate lines).
- Seams: `Hooks::resolve` (`hooks.rs`) is the single place hooks are
  gathered (`M::hooks()`, then the app's); `without_events` adds one check
  there (a task-local set by `without_events(fut)`). Core already fires
  `Retrieved`, `Saving`, `Creating`, `Created`, `Updating`, `Updated`,
  `Saved`, `Deleting`, `Deleted` and `Trashed`; `Replicating` belongs to an
  executor-aware `replicate` variant the track adds.
- Global scopes registered at runtime extend
  `Builder::apply_global_scopes` (`query/mod.rs`), the single place scopes
  apply; scopes have no executor there, so registration is process-wide
  (filled by `OrmModule` at boot). `without_global_scopes_except` and
  removal by name go with it.
- Macro attributes: `observers(..)`, `prunable` in
  `navree-orm-macros/src/model/lifecycle.rs`.

### orm-attributes

- Owns `src/attributes/` (`fill.rs`: `fill`, `force_fill`, `fillable`,
  `guarded` through `Model::set_value`; `casts.rs`: encrypted, encrypted
  JSON, hashed, integer timestamps, arrays and maps, inbound-only casts;
  `accessors.rs`; `serialize.rs`: `hidden`, `visible`, `appends`,
  `to_json`, API resources, OpenAPI schemas).
- Casts are static and synchronous (`Cast<T>`), and run inside `from_row`
  and `values`, which have no executor. Services a cast needs (the
  `Encrypter`, the `Hasher`) are installed once at boot by `OrmModule`
  (from navree-crypto's providers) and read by the casts; a missing
  service is a `Cast` error naming the module to add.
- Serialization: `Serialize for Record<M>` (`record.rs`) routes through
  `attributes::serialize`; the metadata (`hidden`, `visible`, `appends`)
  goes on `ModelMeta` as const builder methods (`model.rs`), written by
  the derive.
- Macro attributes: `fillable`, `guarded`, `hidden`, `visible`, `appends`,
  field casts `"encrypted" | "hashed" | ...`, accessor/mutator
  declarations, in `navree-orm-macros/src/model/attributes.rs`.

### orm-streams

- Owns `src/streams.rs` (`chunk`, `chunk_by_id`, `each`, `lazy`,
  `lazy_by_id`, `cursor` as async streams over `Builder::fetch` with
  pages or keysets), `src/query/bulk.rs` (`insert`, `insert_or_ignore`,
  `upsert` of many models, `delete_where`; `InsertIr` gains an
  `on_conflict`), JSON path assignments in mass updates (the check in
  `query/write.rs::sets`), PostgreSQL-specific JSON clauses (key
  existence, `@>` on paths) and lateral joins (separate functions in
  `sql/filter.rs` / `sql/select.rs`), and multiple connections
  (`#[model(connection)]`, already in `ModelMeta::connection_name`,
  resolved through `ExecContext::app`).
- Core already has unions, row locks, sub-joins and subquery tables, full
  text on PostgreSQL and MySQL, and the TimescaleDB helpers.

### orm-factories

- Owns `src/factory/` (`Model::factory()`, states, sequences, `count`,
  `has`/`for`, `recycle`, `make`/`create`, after-making and
  after-creating hooks), `src/testing.rs` (`assert_database_has`,
  `assert_database_missing`, `assert_model_exists`, `assert_soft_deleted`,
  `assert_database_count`, through `TableBuilder`), `src/binding.rs`
  (route model binding: a `Bound<M>` extractor loading by route key,
  soft-delete aware, workspace scoped, 404 through `ModelNotFound`), the
  workspace-scoping trait (a `GlobalScope` plus a `Saving` hook filling
  `workspace_id`), seeders on models, and `navree make:model` in
  navree-cli.

### mongodb

- Creates `crates/navree-orm-mongodb` (L5): implements `DocumentDriver`
  (section 6; `tests/orm/documents.rs` is the reference), an executor for
  the client and one for a session (`Executor::target` returning
  `Target::Document(DocumentTarget::new(driver, app))`), BSON conversion
  for `Value`, ObjectId keys (an `ObjectId` field type implementing `Field`
  and `PrimaryKey`, empty when all zero; store-assigned like
  `KeyKind::Increments`: an empty key inserts without `_id` and the driver
  returns the generated one in `InsertOutcome::returned`; the derive
  learns `key_type = "object_id"` and infers it for an `ObjectId` key
  field, one line each in `model/container.rs` and `model/mod.rs`), embedded
  documents as JSON casts, `$lookup` or batched `$in` loads for relations,
  and time-series collections for the schema builder's engine helpers.
  Integration tests run against `NAVREE_TEST_MONGODB_URL`.
