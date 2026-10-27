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
| `navree-orm-macros` | L0 | `#[derive(Model)]`, `#[scopes]` and `#[accessors]`/`#[accessor]` (proc macros; no runtime navree dependencies) |
| `navree-orm-mongodb` | L5 | MongoDB: implements `DocumentDriver` (mongodb track) |
| `navree-schema` | L3/L4 | Migrations and the schema builder (own design; the ORM does not depend on it) |

`navree-orm` depends on navree-kernel, navree-contracts, navree-http,
navree-db and navree-orm-macros. Its HIERARCHY row
(`crates/navree/tests/dependency_graph.rs`) also allows navree-config,
navree-telemetry, navree-events, navree-crypto and navree-api, for the
lifecycle and attributes tracks. The attribute layer uses navree-crypto
(encrypted and hashed casts), navree-config (the configuration casts can
read) and navree-api (records as API resources), plus utoipa (OpenAPI
schemas of models), argon2 (recognising Argon2 hashes without hashing),
url (`Url` fields) and tokio (task-local scopes). It re-exports the
macros, so `use navree_orm::Model;` imports the trait and the derive at
once.

The facade turns it on with the `orm` feature (in `full`; it turns on
`db`, `http`, `crypto` and `api` too): `navree::orm` is the whole crate;
`navree::prelude` brings `Model` (trait and derive), `Record`,
`Collection`, `Builder`, `#[accessors]`, `attrs!` and `Attributes`;
`navree_orm::prelude` has every name an app writes. The facade test
treats navree-orm-macros as a companion proc-macro crate of navree-orm
(`COMPANION_MACROS` in `crates/navree/tests/facade.rs`).

Features of navree-orm: `postgres` (navree-db's PostgreSQL driver, and
SeaORM's pgvector values for `cast = "vector"`), `mysql` (SeaORM's MySQL
driver; once navree-db has its own `mysql` feature, this one forwards to
it). SQL for every dialect is generated without any feature.

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
  cast.rs           Cast, CastError, AsJson, StringEnum, IntEnum, encode/decode helpers; re-exports the built-in casts
  attributes/       the attribute layer (section 11, orm-attributes)
    mod.rs          re-exports
    context.rs      CastContext: the services casts use (encrypter, hasher, clock, config, dialect), resolved per operation; scopes
    fill.rs         Attributes, attrs!, fill / force_fill / fill_only / set_attribute / set_json_path, guards, unguarded, DiscardPolicy
    casts/          decimal, datetime (formats, unix timestamps), encrypted, hashed, binary ids, inbound, castable, multi-column, url, vector
    accessors.rs    Accessor (what #[accessors] generates), NoAccessors
    arith.rs        Record::increment_cast / decrement_cast
    extras.rs       ExtraCast, Builder::with_casts (query-time casts of extra columns)
    state.rs        RecordState: per-record hidden / visible / appends overrides, stale columns
    serialize.rs    the record serializer; Record::to_json / to_value / only / except
    visibility.rs   Condition; make_visible / make_hidden / append / set_* / merge_* on records and collections
    resource.rs     Resource, HasResource, API resources and field masks of records and collections
    schema.rs       ModelSchema, ToSchema for records and pages, page schemas
    model_ref.rs    ModelRef, ModelRefs (records in queued jobs)
  scope.rs          GlobalScope, Scoped, SoftDeletes, scope grouping
  expr.rs           Expr constructors: col, val, raw, func, aggregates, arithmetic, subquery, time_bucket, first, last
  relations.rs      Relation<P>, Relations (loaded relations on a record)
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

The attribute layer adds container keys (`fillable(..)`, `guarded(..)`,
`unguarded`, `hidden(..)`, `visible(..)`, `appends(..)`, `encrypter =
"name"`, `serialize_dates_with = path`, `resource = Type`), field keys
(`hidden`, `fillable`, `guarded`, `set_with = path`, `schema = Type`,
`inbound = path`, `columns(a: A, b: B)`) and built-in casts (`"decimal(N)"`,
`"datetime_format(\"..\")"`, `"unix_timestamp"`, `"unix_timestamp_ms"`,
`"encrypted"`, `"encrypted_json"`, `"hashed"`, `"binary_uuid"`,
`"binary_ulid"`, `"vector"`, `cast` alone, `cast(path, args..)`); section
11 (orm-attributes) describes them.

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
  A field stored across columns (`columns(..)`) has one constant per
  column.
- For the attribute layer: the `Model` methods `set_attribute` and
  `set_attribute_parts` (typed assignment from JSON through the cast and
  the mutator), `changed_columns` (dirty columns, compared through each
  cast), `json_attribute`, `accessors`, `prepare_for_write`,
  `serialized_overrides`, `stale_columns` and `increment_cast`; the
  attribute metadata on `ModelMeta`/`ColumnMeta`; a `set_<field>` setter
  for each field with a mutator; `HasResource` for `resource = Type`; and
  a compile-time check that each `appends(..)` name is an accessor.

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
| `attributes` | per-record `hidden`/`visible`/`appends` overrides and the columns whose stored form is stale (section 11, attributes) |

Instance API: `new` (also `From<M>`), `model`, `model_mut`, `key`,
`save`, `update(exec, |m| ..)`, `delete`, `refresh`,
`fresh`, `replicate`, `touch`, `touch_column`, `increment[_with]`,
`decrement[_with]`, `is_dirty[_on]`, `is_clean[_on]`, `was_changed[_on]`,
`get_dirty`, `get_changes`, `get_previous`, `get_original`,
`get_original_value`, `sync_original[_on]`, `discard_changes`, `exists`,
`was_recently_created`, `trashed`, `is`/`is_not`, `extras`, `extra::<T>`,
`set_extra`, `relations[_mut]`, `into_inner`. The attribute layer adds
mass assignment (`fill`, `force_fill`, `fill_only`, `set_attribute`,
`set_json_path`, `update_attributes`), `increment_cast[_with]` /
`decrement_cast[_with]`, serialization (`to_json`, `to_json_pretty`,
`to_value`, `only`, `except`), visibility (`make_visible[_if]`,
`make_hidden[_if]`, `set_hidden`, `merge_hidden`, `set_visible`,
`merge_visible`, `append`, `set_appends`, `merge_appends`,
`without_appends`, `has_appended`, `hidden`, `visible`, `appends`), API
resources (`to_resource`, `resource`, `to_resource_json[_as]`,
`to_value_masked`, `field_names`) and `to_ref`.

Dirty tracking compares stored values (`same_value`): NULLs regardless of
type, floats within a relative tolerance, decimals numerically, date-times
by instant, JSON structurally. A cast can compare in its own terms
(`Cast::eq_stored` on stored values, `Cast::same` on field values:
encrypted fields compare their plain values, since every encryption
differs), and a column whose stored form is stale (`Cast::is_stale`: a
payload under an old key) is dirty until saved. A new record is dirty in
every column.

Records serialize through `attributes::serialize` (`Serialize for
Record<M>`): the model's own serde output (keys, renames, skips), cast
outputs, one date-time format, the extras, the appended accessors, then
the loaded relations, filtered by `visible` and `hidden` (section 11,
attributes).

### Persistence flows (`persist.rs`)

Insert: `saving` → `creating` (both cancellable) → generate an empty key →
fill empty `created_at`/`updated_at` → prepare the casts (`Cast::prepare`:
hash plain passwords, round and normalise values, so the record holds what
is stored) → `INSERT` → read a database-assigned key back → `exists`,
`recently_created` → `created` change event → `created` → `saved` → sync
the original.

Update (dirty records only): `saving` → `updating` → `updated_at` unless the
caller changed it → prepare the casts → `UPDATE` of the dirty columns by
the original key → sync the changes → `updated` change event (old and new
values of the changed columns) → `updated` → `saved` → sync. A clean
record runs `saving` and `saved` and writes nothing.

Every flow, and every query's decoding, runs inside the model's cast
context (`attributes::in_model_scope`) when its casts need services.

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
| `MassAssignment { model, columns, totally_guarded }` | `fill` on a model that declares neither `fillable` nor `guarded`, or keys it may not set under `DiscardPolicy::Fail` | 500 |
| `Serialize { model, column, message }` | a record did not serialize (the column, accessor or relation is named when known) | 500 |

`is_not_found()` and `is_unique_violation()` classify. Messages name the
model, the column and the operation; no SQL or values reach clients
(navree-http logs the source of 500s).

## 9. Casts, collections, pagination

`Cast<T>`: `type Stored: Field`, `get(Stored) -> Result<T, CastError>`,
`set(&T) -> Result<Stored, CastError>`; static and synchronous. Provided
items refine how the ORM treats the field: `CONTEXT` with
`get_with`/`set_with`/`null_with` (casts that read services from the
`CastContext`), `KIND` (what serialized values are, for schemas and date
formats), `eq_stored` and `same` (dirty tracking), `from_json` (mass
assignment), `serialize` (serialized records), `increment`/`decrement`,
`prepare` (asynchronous work before a write) and `is_stale`. Built-in:
`"json"` (`AsJson`, any serde type), `"string_enum"` (`StringEnum`),
`"int_enum"` (`IntEnum`: `Copy + Into<i64> + TryFrom<i64>`), and the
attribute layer's casts (section 11). A cast on an `Option<T>` field casts
the inner value. `Field` also says what a type is in JSON (`Field::KIND`, a
`FieldKind`) and reads untyped input (`Field::from_json`).

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
  document-driver reference. `tests/orm/attributes/` covers the attribute
  layer: mass assignment (`fill.rs`), casts (`casts.rs`), encryption and
  hashing (`secrets.rs`), serialization (`serialize.rs`), API resources
  and OpenAPI schemas (`resources.rs`) and model references (`refs.rs`).
- `tests/sql/`: SQL snapshots per dialect (insta), each showing the
  PostgreSQL, MySQL and SQLite statement and values or the error
  (`attributes.rs`: the stored form of cast fields in conditions, vectors
  per database, with and without the `postgres` feature).
- `tests/servers/`: PostgreSQL with TimescaleDB and MySQL, gated by
  `NAVREE_TEST_POSTGRES_URL` / `NAVREE_TEST_MYSQL_URL` (`cargo test -p
  navree-orm --features postgres,mysql --test servers`; a test skips with
  a notice when its variable is unset); each test runs in a schema or
  database of its own, dropped afterwards. `attributes.rs` runs every cast
  against the servers' native column types, and vectors where the server
  has a vector type (pgvector, MySQL 9; skipped with a notice otherwise).
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

Shipped. Owns `src/attributes/` (layout in section 2),
`navree-orm-macros/src/model/attributes.rs` (the derive's attribute keys,
listed in its `CONTAINER_KEYS` and `FIELD_KEYS`) and
`navree-orm-macros/src/accessors.rs`. The metadata goes on `ModelMeta` and
`ColumnMeta` as const builder methods (`model.rs`), written by the derive.

**Mass assignment** (`fill.rs`). `Attributes` are JSON values keyed by
column name or JSON path (`"meta->status"`), in order: `attrs!`, a JSON
object, a request body (it deserializes) or any serializable struct.
`fill` sets what the model allows: `fillable(..)` lists the fillable
columns; without it `guarded(..)` lists the guarded ones and every other
column is fillable; a model that declares neither is totally guarded
(`fill` fails with `MassAssignment`, `force_fill` works). Field flags
(`#[model(fillable)]`, `#[model(guarded)]`) add to the lists;
`#[model(unguarded)]` turns guarding off for a model, `unguarded(fut)` for
a task. Only real columns are set: unknown keys and JSON paths into
columns that are not JSON never reach the model. Values convert like
untyped input (`Field::from_json`), through the cast (`Cast::from_json`:
an encrypted field takes the plain value) and the mutator; a field stored
across columns is assigned once from all its parts (`set_attribute_parts`).
`fill` checks every key before it changes anything. Keys it may not set
are dropped, reported or refused (`DiscardPolicy`, for the process or a
task). Also `force_fill`, `fill_only`, `set_attribute`, `set_json_path`,
`update_attributes`, and `create_from` / `force_create` on models and
builders.

**Casts** (`casts/`, re-exported from `cast`):

| Attribute | Field type | Stored as | Notes |
|---|---|---|---|
| `"decimal(N)"` | `Decimal` | a decimal | rounded half away from zero to N places (N ≤ 28) when written |
| `"datetime_format(\"..\")"` | `DateTime<Utc>`/`<FixedOffset>`, `NaiveDateTime`, `NaiveDate`, `NaiveTime` | text | the format is checked at compile time; serialized in the same format |
| `"unix_timestamp"`, `"unix_timestamp_ms"` | `DateTime<Utc>`, `NaiveDateTime` | an integer | the record holds the stored precision after a save |
| `"encrypted"`, `"encrypted_json"` | `String`/`Vec<u8>`, any serde type | navree-crypto payload `v1.<kid>.<nonce>.<ct>` | compared by plain value; a payload under a previous key is stale and rewritten at the next save; `#[model(encrypter = "name")]` picks a named encrypter |
| `"hashed"` | `String` | an Argon2id PHC string | hashed in `prepare` (navree-crypto's `Hasher`, off the async threads); an Argon2 hash is kept; other algorithms' hashes and malformed Argon2 strings are refused |
| `"binary_uuid"`, `"binary_ulid"` | `Uuid`, `Ulid` | 16 bytes | ULIDs big-endian, so bytes sort by time |
| `"vector"` | `Vec<f32>` | pgvector (with the `postgres` feature; its text form without), little-endian `f32` bytes on MySQL 9, MariaDB and SQLite | non-finite values refused |
| (none) | `url::Url` | text | a `Field` |
| `cast` | a `Castable` type | its cast's | the type names its cast |
| `cast(path, args..)` | the cast's | the cast's | arguments become const generics: `cast(Scaled, 100)` is `Scaled<100>` |
| `inbound = path` | a `Field` | itself | `InboundCast`: transformed on write only; the record holds the transformed value after a save |
| `cast = path, columns(a: A, b: B)` | any | each column | `MultiColumnCast` over `StoredColumns` tuples of 2 to 8 columns, one typed constant per column |

**The cast context** (`context.rs`). Casts run inside `from_row` and
`values`, which are synchronous and take no executor. A cast that needs a
service sets `Cast::CONTEXT`; for a model with such casts, every query,
save, delete, refresh and increment first resolves a `CastContext` from the
executor's app (`CastContext::for_model`: the `Encrypter`, the named one
for `encrypter = "name"`, the `Hasher`, the `dyn Clock`, the `Config`, and
the dialect) and runs decoding and encoding inside its task-local scope
(`in_model_scope`). `CastContext::scope` / `sync_scope` set one explicitly
(winning over the app's services, so tests pin keys); `set_default`
installs a process-wide fallback. A missing service is a `Cast` error
saying what to register. Asynchronous work (hashing) is `Cast::prepare`,
which the persistence flows run before they take the values (section 4).
A record keeps the context it was read or saved under (`RecordState`), so
its dirty checks, fills and serialization outside any scope use the same
services; `create_from` / `force_create` resolve it before they fill.
Conditions are encoded where they are built, before any operation runs,
so a condition on a cast that reads services is built inside an explicit
scope (or under the process default).

**Accessors and mutators.** `#[accessors] impl Deal { #[accessor] fn
label(&self) -> String { .. } }` generates an inherent
`Deal::__NAVREE_ACCESSORS` table (name, a getter returning JSON, the Rust
type, an optional `#[accessor(schema = Type)]`); the derive reads it by
inherent-constant precedence over the `NoAccessors` fallback, so a model
without accessors needs nothing, and a const assertion makes each
`appends(..)` name that is not an accessor a compile error at the name.
`#[model(set_with = path)]` names a mutator `fn(&mut M, T)`, which `fill`,
`set_attribute` and the generated `set_<field>` setter call.

**Serialization** (`serialize.rs`, `visibility.rs`). A record serializes as
the model's own serde output (keys, renames, skips, `serialize_with`), each
cast's `serialize` output where it has one, date-times in one format (RFC
3339 UTC with microseconds, or `serialize_dates_with = path`), the extras,
the appended accessors, then the loaded relations, each applying its own
visibility; `visible(..)` keeps only the names listed, `hidden(..)` leaves
names out. Names are fields or columns (matched to serde's keys) or output
keys. The model's lists come from the derive; a record overrides them
(`RecordState`: `make_visible`, `make_hidden`, `append`, `set_*`,
`merge_*`, `without_appends`, with conditional forms), a collection for
all its records, and `only` / `except` per call. Failures are
`OrmError::Serialize` naming the field. A record with nothing to rewrite
(no casts, date-times, extras, relations, visibility or appends)
serializes straight through its model's `Serialize`.

**API resources and OpenAPI** (`resource.rs`, `schema.rs`). `Resource<M>`
(a navree-api `ApiResource` built `from_record`) is the shape a front end
receives; `#[model(resource = Type)]` makes one the model's own
(`HasResource`, `Record::resource`); a record is also a resource of its own
(`to_resource_json`: its JSON plus `"__type"`), and `to_value_masked`
applies navree-api field masks. `Record<M>` implements utoipa's `ToSchema`
(named after the model) from `ModelSchema<M>`: one property per serialized
field under its serde key, typed from `FieldKind` (or the cast's `KIND`, or
`#[model(schema = Type)]`), `Option`s nullable, `skip_serializing_if`
fields not required, hidden fields left out, accessors included.
`SimplePage` and `CursorPage` of any `ToSchema` type have schemas, and
`page_schema` / `simple_page_schema` / `cursor_page_schema` give
navree-http `SchemaFn`s for the page shapes.

**Query-time casts** (`extras.rs`): `Builder::with_casts([("total",
ExtraCast::Decimal(Some(2)))])` converts extra columns (`select_raw`,
aggregates) as they are read; extras serialize in the model's date format.

**Model references** (`model_ref.rs`): `ModelRef<M>` / `ModelRefs<M>`
serialize records' identities (model name, key, loaded relation names,
connection) into queued jobs; `restore` reads the records again through
the writer without global scopes, failing with `ModelNotFound` when one is
gone (`restore_or_skip` gives `None`; `ModelRefs::restore` keeps the order
and leaves out records deleted since).

Seams for the other tracks:

- orm-lifecycle: strict handling of discarded attributes calls
  `attributes::set_discard_policy(DiscardPolicy::Fail | Report(..))` at
  boot; a quiet `force_create` is `without_events` around
  `Model::force_create`.
- orm-relations: `ModelRef::relations()` names the relations to load again
  after `restore` (which returns the record without them); serialized
  relations already apply each related record's visibility and appends.
- orm-factories: states and overrides can be `Attributes` applied with
  `force_fill`.

Compile errors (pinned by `tests/ui/model_attributes_*.rs`): unknown
names in the lists (with suggestions), `unguarded` with lists, a column
both fillable and guarded, missing or too large decimal scales, invalid
date-time formats, unknown casts, `columns(..)` without a cast or with
fewer than two columns, `cast` with `inbound`, accessor signatures,
setters that would shadow record methods, and `appends(..)` names that are
not accessors.

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
