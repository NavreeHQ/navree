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
| `navree-orm-macros` | L0 | `#[derive(Model)]` and `#[scopes]` (proc macros; no runtime navree dependencies) |
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
`mysql` (navree-db's MySQL and MariaDB driver). SQL for every dialect is
generated without any feature.

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
  relations.rs      Relation<P>, Relations (loaded relations on a record)
  driver.rs         DocumentDriver, InsertOutcome
  doc_models.rs     `__doc_models` (hidden): the models the doc examples use, so every example compiles
  exec.rs           Executor, Access, Route, Target, SqlTarget, DocumentTarget, ExecContext; using_connection, using_writer; timed reads
  ir.rs             QueryIr, Where, Expr, Order, Join, TableRef, FullText, Lock, GroupLimit, InsertIr (OnConflict), UpdateIr, DeleteIr, ...
  operators.rs      Column pattern, JSON and null-safe helpers (ilike, regex, similar_to, contains, has_key, ...), Filter::op_raw, Expr::binary, vector distances
  raw.rs            Raw, raw(): statements written by hand (select, select_one, scalar, statement, affecting_statement, unprepared)
  streams.rs        RowStream; chunk/each/lazy/cursor on both builders (offset pages or keysets); cursor(exec, raw(..))
  query/
    mod.rs          Builder<M> (state, keys, scopes, route, into_ir, to_sql)
    filters.rs      the where family (`filters!`), WhereGroup, HasWheres
    clauses.rs      select/join/group/having/order/limit/union/lock/timeout/connection (`clauses!`), HasQuery, IntoSelect
    join.rs         JoinClause
    fetch.rs        retrieval, values, plucks, existence, aggregates; Selectable
    write.rs        mass update/delete/increment/touch; first_or_new/create, create_or_first, update_or_create
    bulk.rs         inserts of many rows or models, insert-or-ignore, upserts, insert-using, update-or-insert, update-from, truncate; ConflictUpdate
    table.rs        TableBuilder, table(): rows of any table, inserts/updates/deletes of rows
  sql/
    mod.rs          Dialect, Statement, Compiler, expressions, JSON paths, raw placeholders, vector distances
    select.rs       SELECT (derived tables, table functions, lateral joins, limits per group, union orders), aggregates, EXISTS, timeout hints
    filter.rs       WHERE/HAVING/ON trees; dialect rules for dates, LIKE, JSON, full text, operators
    write.rs        INSERT (RETURNING, ON CONFLICT / IGNORE / ON DUPLICATE KEY), UPDATE (JSON paths, row ids, FROM), DELETE, TRUNCATE
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
  `union`, `union_all` (orders and limits on a union apply to the whole;
  counts, `exists`, values and pages read a union as a derived table),
  `right_join_sub`, `cross_join_sub`, `join_lateral`, `left_join_lateral`
  (PostgreSQL and MySQL 8.0.14+), `group_limit(n, column)` (top N per
  group through `ROW_NUMBER()`), row locks (`lock_for_update`,
  `shared_lock`, `lock(LockMode)` with `NoKeyUpdate`/`KeyShare`,
  `skip_locked`, `no_wait`; a locking read through `&Db` joins the task's
  unit of work), `timeout(Duration)`, `on_connection(name)`, `use_write()`,
  and the vector helpers (`where_vector_similar_to`,
  `where_vector_distance_less_than`, `select_vector_distance`,
  `order_by_vector_distance`; pgvector, MariaDB vectors).
- Sources: `from_as(alias)` and `TableBuilder::from_sub(query, alias)`,
  `Builder::from_sub(query)` (a model read from a derived table named
  like its table), `from_function(Expr::func("generate_series", ..), "n")`
  and `from_raw("generate_series(?, ?) AS n", values)` (a table function
  written by hand).
- JSON and pattern conditions: `where_json_overlaps` /
  `where_json_doesnt_overlap`, `where_json_contains_key` /
  `where_json_doesnt_contain_key` (with `or_` forms), the column helpers
  `contains`, `contained_by`, `overlaps`, `has_key`, `ilike`, `regex`,
  `regex_case_insensitive`, `not_regex`, `similar_to`, `is_distinct_from`,
  and `Filter::op_raw(column, op, value)` checked against each dialect's
  operator allow-list. Full text takes `FullText::english()` (a validated
  language), `.websearch()`/`.phrase()`/`.raw()` modes and `tsvector`
  columns (`FullText::vectors()`); SQLite answers `Unsupported` (query an
  FTS5 table with `where_raw`).
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
  An assignment to a JSON path (`col("meta->status").set("won")`) updates
  the value inside the column: `jsonb_set` on PostgreSQL, `JSON_SET` on
  MySQL, `json_set` on SQLite, several paths of one column folded into
  one expression.
- Bulk writes (`query/bulk.rs`) run without hooks or change events:
  `insert` of many rows (one statement, split under the dialect's bind
  limit and run as one transaction or savepoint), `insert_get_id`,
  `insert_or_ignore` (`ON CONFLICT DO NOTHING`, `INSERT IGNORE`, `INSERT OR
  IGNORE`), `insert_or_ignore_returning`, `insert_using` /
  `insert_or_ignore_using` (`INSERT .. SELECT`), `upsert(rows, unique_by,
  update)` (`ON CONFLICT .. DO UPDATE SET c = excluded.c`, MySQL `ON
  DUPLICATE KEY UPDATE c = VALUES(c)`; `ConflictUpdate` names columns,
  expressions or `All`), `update_or_insert`, `update_from` (`UPDATE ..
  FROM` / MySQL's joined update), `truncate` (`RESTART IDENTITY`, opt-in
  `CASCADE`; SQLite deletes and resets its sequence). Model builders add
  `fill_and_insert`, `fill_and_insert_or_ignore`, `fill_and_insert_get_id`
  and `fill_for_insert` (generated keys, timestamps and casts applied);
  `Model::insert`, `insert_or_ignore`, `fill_and_insert` and `upsert` are
  the static shortcuts; `Record::save_or_ignore(_on)` inserts one record
  through its lifecycle unless it conflicts.
- Table writes with joins, orders or a limit narrow the rows by row id:
  `ctid IN (SELECT ..)` on PostgreSQL, `rowid IN (SELECT ..)` on SQLite;
  MySQL (no row id) refuses them on tables and uses keys on models.
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
unions, group_limit, connection, use_writer, timeout, errors }`.
`TableRef` is a table, an aliased table, a subquery or a table function.
`connection`, `use_writer` and `timeout` say where and how long the query
runs (`Route`, timed reads; section 6), not what it selects.
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
| `Db` | the task's current unit of work when its transaction is open on this pool (`Db::reader`), else a read replica (the writer without replicas, after the unit of work wrote, or with `use_write`/`using_writer`) | the task's current unfinished unit of work on this pool (events recorded there), else the pool |
| `Uow` | its transaction | its transaction, events recorded |
| `DatabaseConnection`, `DatabaseTransaction` | as given | as given, no events |
| `Arc<E>` | as `E` | as `E` |
| a document store's executor | `Target::Document` | `Target::Document` |

`Executor::target(Access)` resolves a `Target`: `Target::Sql(SqlTarget)`
(dialect, connection or transaction, unit of work, app, the connection's
hooks) or `Target::Document(DocumentTarget)` (driver, app). Terminal
methods call `Executor::target_routed(Access, Route)`: a `Route` carries
the connection the query names (`on_connection`), the model's connection
(`#[model(connection = "..")]`, `M::connection_name()`) and whether reads
go to the writer. Given the default `Db`, an operation runs on the
connection its query names, else its model's, else (inside
`using_connection(name, fut)`) the scoped one, reached through the app's
`DbConnections`; a named connection's `Db` runs what it is given unless
the query names another; a `Uow` refuses an operation on another
connection (`OrmError::Connection`: a unit of work spans one
connection); SeaORM connections, transactions and document stores run
what they are given. A write through `Db` or `Uow` marks the task's unit
of work as having written (`Uow::has_modified_records`), so with replicas
and `sticky` its later reads stay on the writer. `Target` runs IR:
`fetch`, `stream`, `aggregate`, `exists`, `insert`, `update`, `delete`,
and nests a savepoint (`begin_savepoint`, `on_transaction`) or a
transaction of its own (`begin_atomic`). `ExecContext` (from
`Target::context()`) gives hooks and later tracks the app, the unit of
work, the dialect and `now()`.

`SqlTarget` announces each statement and each transaction it begins to the
connection's hooks (`Db::before_executing`,
`Db::before_starting_transaction`), reads again once on another pooled
connection when a read outside a transaction loses its connection
(`OrmError::is_lost_connection`; never inside a transaction, never for
writes), and runs reads under a query's `timeout`: MySQL's
`MAX_EXECUTION_TIME` hint, PostgreSQL's `SET LOCAL statement_timeout` in a
transaction of its own on the pool or a savepoint inside a transaction
(restoring the transaction's own timeout; a stopped read leaves the
transaction usable), `Unsupported` on SQLite and on PostgreSQL cursors;
expiry is `OrmError::Timeout`.

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
| `Connection { connection, message }` | an unknown connection name, or a unit of work of another connection | 500 |
| `Timeout { timeout, source }` | the database stopped a read that outran the query's `timeout` | 500 |

`is_not_found()`, `is_unique_violation()`, `is_timeout()` and
`is_lost_connection()` classify. Messages name the model, the column and
the operation; no SQL or values reach clients (navree-http logs the
source of 500s).

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
  shapes, extractors, request transactions), `TableBuilder`, the
  document-driver reference, streams (`streams.rs`), raw statements
  (`raw.rs`), bulk writes (`bulk.rs`), locks (`locks.rs`), compound
  queries (`compound.rs`), JSON and operators (`json.rs`), and connections
  (`connections.rs`: named connections, scopes, replicas, sticky reads,
  statement hooks).
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

### orm-relations

- Owns `src/relations.rs`, which it may turn into `src/relations/`
  (`mod.rs` keeping `Relation` and `Relations`, plus `belongs_to.rs`,
  `has_one.rs`, `has_many.rs`, `belongs_to_many.rs`, `through.rs`,
  `morph.rs`, `eager.rs`, `existence.rs`, `aggregates.rs`, `pivot.rs`,
  `morph_map.rs`).
- Core provides: `Relation<P>` (`type Related`, `name`, `query_for(parent)
  -> Builder<Related>`), the per-record `Relations` store (typed
  `get`/`get_mut`/`set`/`take`/`is_loaded`/`unset`, serialized with the
  record), `Record::relations[_mut]`, `Builder::into_ir`, `IntoQuery` for
  builders (correlated `where_exists`, `select_sub` for `with_count` and
  friends), `where_in`/`where_key_in` for batched loads, `TableBuilder`
  insert/update/delete for pivots, and `ModelHook<M>` after `Saved` and
  `Deleted` for touching parents.
- Adds: relation definitions (methods returning typed relations: belongs
  to, has one, has many, many to many with pivots, has one/many through,
  polymorphic), registered by name for string paths (`"company.owner"`);
  `with`, `with_count`, `with_sum`/`with_avg`/`with_min`/`with_max`,
  `with_exists`, constrained eager loads; `load`, `load_missing`,
  `load_count` on records and collections; `has`, `where_has`,
  `doesnt_have`, `where_relation`, `where_belongs_to`; writing through
  relations (`save`, `create`, `associate`, `dissociate`, `attach`,
  `detach`, `sync`, `toggle`, `update_existing_pivot`); the morph map;
  strict lazy-loading checks.
- Eager loading needs one edit in core: a field on `Builder<M>`
  (`query/mod.rs`) holding the requested loads, and a call at the end of
  `Builder::fetch` (`query/fetch.rs`, after the `Retrieved` hooks) that
  loads them into the records.
- Macro attributes: `touches = [..]`, `morph_name = ".."` and relation
  declarations in `navree-orm-macros/src/model/relations.rs`, one line each
  in the key lists and parsers of `model/container.rs` / `model/field.rs`;
  `touches` generates `fn hooks()` entries.

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
  `src/strict.rs` (strict modes: lazy loading, silently discarded
  attributes, missing attributes) and `src/module.rs` (`OrmModule`, which
  other tracks extend with their boot steps as separate lines).
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

### orm-streams (shipped)

- **Streams** (`src/streams.rs`). Both builders have `chunk(exec, n, f)`
  (the callback returns `ControlFlow` to stop), `chunk_map`, `each`,
  `chunk_by_id` / `chunk_by_id_desc` / `chunk_by_column(col, alias)`
  (keysets, safe while the callback updates the rows), `each_by_id`,
  `lazy`, `lazy_by_id`, `lazy_by_id_desc`, `lazy_by_column` (async streams
  of records or rows fetched a page at a time, through `Builder::fetch`, so
  `Retrieved` hooks fire and the eager loads of the relations track will
  apply per page), and `cursor` (one statement whose rows stream as the
  database sends them; it holds its connection, and inside a transaction
  other statements on that transaction fail fast instead of waiting for
  it). `navree_orm::cursor(exec, raw(..))` streams a raw statement. Chunks
  without an order are ordered by key. Lazy-collection helpers are the
  `futures` adapters (`StreamExt`, `TryStreamExt`, in the prelude without
  their names): `map_ok`, `try_filter`, `take`, `chunks`, ...
- **Bulk writes** (`src/query/bulk.rs`) and **JSON path updates**: see
  section 5.
- **Raw statements** (`src/raw.rs`): `raw(sql, values)` with portable `?`
  placeholders as an expression, a condition or a select item, and
  `select`, `select_one`, `scalar`, `statement`, `affecting_statement`,
  `unprepared` running one.
- **Compound queries**: derived tables, table functions, sub-joins,
  lateral joins, limits per group, unions with orders and limits (section
  5). **Row locks** with modes, `SKIP LOCKED`, `NOWAIT`;
  `Record::refresh_for_update` reloads under a lock. **JSON, operators,
  full text and vectors** (section 5; `src/operators.rs`).
- **Connections** live in navree-db and the executor (section 6):
  `[database.connections.<name>]` opens named connections on first use
  (`DbConnections::get`, `Db::connection(name)`, named `Db` providers
  `ctx.get_named::<Db>(name)`; `DbConnections::names()` lists every
  configured one); `#[model(connection = "..")]`, `on_connection`,
  `M::on_connection(name)` and `using_connection(name, fut)` route ORM
  operations; `read_urls` adds read replicas with `sticky` reads after a
  write, `use_write()`, `M::on_write_connection()` and `using_writer(fut)`
  read from the writer; `direct_url` is a direct connection past a
  transaction-mode pooler (`Db::direct`) for migrations and session work.
  Pool settings (`max_connections`, `min_connections`,
  `connect_timeout_secs`, `idle_timeout_secs`, `max_lifetime_secs`),
  `[database.postgres]` session settings and `[database.sqlite]` pragmas
  shape every connection; `Db::pool_stats`, `Db::open_connections`,
  `Db::server_version`, `database_name`, `is_mariadb`, `has_timescaledb`
  report.
- **Watching statements** (navree-db's `queries.rs`): `QueryLog::capture(fut)`
  returns the statements a future ran (`LoggedQuery { sql, values,
  duration, failed, connection }`, `raw_sql()` with values written in),
  `Db::on_query` listens to every statement, `Db::before_executing` and
  `Db::before_starting_transaction` run before the ORM's statements and
  transactions, `Db::when_querying_for_longer_than` runs once per unit of
  work over a total, and every unit of work counts its statements
  (`Uow::query_stats`, `total_query_duration`,
  `reset_total_query_duration`, `allow_duration_handlers_again`), logging
  a summary when it finishes and warning about a statement that repeated
  ten times or more. `[database] log_queries` and `slow_query_ms` log
  through `tracing`.
- **Events and commands**: `db.connection_established`
  (`ConnectionEstablished`) when a connection's pool opens and `db.busy`
  (`DatabaseBusy`) from `db:monitor [--databases a,b] [--max N]`, both
  through the app's `dyn EventSink`; `navree db [connection] [--read |
  --write]` opens `psql`, `mysql`, `mariadb`, `sqlite3` or `mongosh` with
  the configured credentials (passwords through the environment).
- **Not ported, by design.** Table prefixes per connection (PostgreSQL
  schemas or a separate database instead: prefixes would leak into raw
  SQL, row-level security policies and hypertable names). Low-level
  handles (raw driver connections, separate read and write handles,
  reconnector callbacks, swapping grammars, processors or the resolver
  models use): the pool owns its connections and replaces lost ones,
  executors are explicit, and tests boot their own app. Moving one record
  to another connection at runtime: pass that connection's executor.
- **Waiting on the relations track**: eager loads per chunk of `chunk`,
  `each` and `lazy` (every page already runs through `Builder::fetch`,
  where eager loading plugs in).

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
