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
  The ORM runs on navree-db's engine (its values, rows, pools,
  transactions and executor); SeaORM entities keep working beside ORM
  models while crates move off them (same pool, same unit of work).
- **One driver abstraction.** A query is first a driver-neutral value (the
  IR, `navree_orm::ir`), then SQL for PostgreSQL/TimescaleDB, MySQL or
  SQLite written by the ORM's own renderer (`navree_orm::render`, section
  6), or a document command (a `DocumentDriver` a separate crate
  implements).

## 1. Crates

| Crate | Layer | Role |
|---|---|---|
| `navree-orm` | L4 | Models, records, collections, the builders, the IR, SQL per dialect, execution on `Db`/`Uow`, casts, scopes, hooks, pagination, the relation and document-driver seams |
| `navree-orm-macros` | L0 | `#[derive(Model)]`, `#[scopes]`, `#[relations]`, `#[accessors]`/`#[accessor]` and `#[test]` (proc macros; no runtime navree dependencies) |
| `navree-mongo` | L5 | MongoDB: `MongoModule`, `MongoDriver: DocumentDriver`, sessions and units of work, the schema builder and migrations on collections (mongodb track; its own `DESIGN.md`) |
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

Features of navree-orm: `postgres` (navree-db's PostgreSQL driver, which
binds pgvector's `vector` for `cast = "vector"`), `mysql` (navree-db's
MySQL and MariaDB driver), and three that pull in a same-layer crate (all
optional, so the core needs none of them; the facade turns each on when
both its features are on): `access` (navree-access: models as subjects,
policies, accessible queries), `testing` (navree-testing: `TestAppExt`,
test databases, `#[test]`) and `validation` (navree-validation: `unique`
and `exists` rules on models). SQL for every dialect is generated without
any feature.

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
  value.rs          Value and Kind (navree-db's), Field, FieldError, ValueFor, Numeric, to_json, same_value
  ulid.rs           Ulid (time-ordered 128-bit ids, Crockford base32)
  row.rs            Row: one result row (SQL or document); typed and dynamic reads; Serialize
  model.rs          Model trait (+ the static API), ModelMeta, ColumnMeta, KeyKind, PrimaryKey, Timestamp, derive_support
  column.rs         Column<M, T>, col, col_as, Untyped, Filter, Assignment, ColumnName, ColumnOf, IntoExpr, IntoQuery
  record.rs         Record<M>: the instance; dirty tracking; extras; relations; the instance API
  persist.rs        insert / update / delete / refresh / touch / increment flows
  hooks.rs          ModelEvent (+ Custom), Flow, HookKind, ModelHook<M>, Hooks<M> (resolution, unit-of-work scoping, after-commit dispatch)
  events/           without_events; the after-commit runner; Observer + ObserverHook; closure listeners; Dispatch (typed application events); Record::fire and replicate_on; ModelEventFake
  quiet.rs          the quiet operations (save_quietly, delete_quietly, increment_each_quietly, ...)
  soft_deletes.rs   SoftDeleting; restore, force_delete; with_trashed_if / without_trashed / only_trashed; mass restore and force_delete; restore-or-create
  lifecycle.rs      ModelLifecycle: create_quietly, force_destroy, restore_or_create, create_or_restore, global-scope inspection
  global_scopes.rs  the runtime scope registry, ClosureScope, the scope list queries apply, removal by name
  strict.rs         Strictness, Violation, handlers, strict::scope / current, ExecContext::strictness
  options.rs        OrmOptions (the `[orm]` section)
  module.rs         OrmModule, OrmRuntime, ModuleBuilderOrmExt, RegisteredModel, registered_models
  prune.rs          Prunable, MassPrunable, Pruning (in ModelMeta), prune(), `model:prune`, the pruning events
  broadcast.rs      Broadcasts, BroadcastHook, BroadcastModelEvent (job `orm.broadcast`), without_broadcasting
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
  relations/        relationships (section 11, orm-relations): definitions, relation queries,
                    eager and lazy loading, existence, aggregates, pivots, morphs, touches
  driver.rs         DocumentDriver, InsertOutcome
  doc_models.rs     `__doc_models` (hidden): the models the doc examples use, so every example compiles
  doc_factories.rs  `__doc_factories` (hidden): models with factories for the factory, testing, binding and workspace examples
  doc_access.rs     `__doc_access` (hidden, feature `access`): a subject with a policy
  factory/          ModelFactory, HasFactory, Factory<F>; context.rs, faker.rs, relationships.rs, sequence.rs, sources.rs, bulk.rs, quiet.rs
  testing/          database assertions (mod.rs), query counting (queries.rs), test databases (database.rs), TestAppExt (app.rs)
  binding.rs        route model binding: Bound, RouteBinding, RouteKey, WithTrashed, ChildOf, BoundChild
  workspace.rs      workspace scoping: the current workspace, WorkspaceScope, WorkspaceHook, opt-outs
  access.rs         (feature `access`) ModelColumns, accessible queries, AbilityExt, record loading
  validation.rs     (feature `validation`) unique / exists rules on models
  show.rs           model:show: ModelDescription and the command
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
    mod.rs          Dialect, Statement, the render_* statement functions (re-exported from render/), a filter alone
  render/           the ORM's own SQL renderer (section 6, "The renderer"): every statement the ORM runs
    mod.rs          render, render_all, render_query, render_expr; the render_* lowerings re-exported
    ast.rs          the syntax tree: Statement, Query (WITH, compounds, order, limit, lock), Select, Source, Join, Expr, Insert, Update, Delete, Truncate
    writer.rs       Writer: the SQL buffer, binds and placeholders, identifier quoting, string literals, raw fragments
    grammar/        Grammar (shared SQL as default methods), precedence; postgres.rs, mysql.rs, sqlite.rs (the overrides)
    lower/          the IR as trees, with the dialect rules: select.rs, filter.rs, write.rs, text.rs, timescale.rs
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
...)` (expressions, each an `impl GlobalScope<Self>`), and the lifecycle's
(parsed in `navree-orm-macros/src/model/lifecycle.rs`): `observers(expr,
..)` (stateless observers declared on the model), `dispatches(event = Type,
..)` (typed application events), `broadcasts`, `prunable` or
`mass_prunable`.

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
  `static`; `.pruned(..)` for prunable models), `key`/`set_key`,
  `from_row` (each field read with its default when its type has one, see
  section 12, strict modes), `values` (every column as `(name, Value)`,
  field order), `value(column)`, `set_value(column, Value)`,
  `touch_timestamp`, `timestamp_is_empty`, `clear_timestamp`,
  `timestamp_value`, `global_scopes` when declared, and `hooks()` when the
  model declares hooks (`lifecycle::hook_entries`: observers, dispatches,
  broadcasts; built once, in a `static`).
- `impl SoftDeleting` for a soft-deleting model.
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

`Value` is navree-db's value (`navree_orm::Value` re-exports it, with its
`Kind`): what a statement binds and a row holds, a NULL keeping its kind
(`Value::Null(Kind::I64)`; PostgreSQL types every parameter), plus
`Enum { type_name, value }` (a value of a named type: bound as its text,
cast to the type by the PostgreSQL grammar, a MongoDB `ObjectId` in
documents) and `Vector` (pgvector's `vector` on PostgreSQL, little-endian
`f32` bytes elsewhere). `Field` is the trait of Rust types a column holds:
`to_value`, `null`, `from_value(Value)` and `from_sql(&navree_db::Row,
column)`, which by default reads the column as navree-db reads it by its
type (`Row::value`) and converts it with `from_value`. Implemented for
`bool`, the 8- to 64-bit integers, `f32`, `f64`, `Decimal`, `String`,
`Vec<u8>`, `Uuid`, `Ulid`, `NaiveDate`, `NaiveTime`, `NaiveDateTime`,
`DateTime<Utc>`, `DateTime<FixedOffset>`, `serde_json::Value`, `Value` and
`Option<T>`. Conversion is tolerant where databases differ: any integer
width, exact `NUMERIC`/`DECIMAL` sums read as integers, floats from
decimals, booleans from MySQL's `TINYINT(1)`, UUIDs from text or bytes,
date-times and JSON from text (SQLite, MySQL expressions).

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
deleted-at (and updated-at) column instead, unless the record is being
force deleted → `deleted` change event (the whole old record) → `trashed`
(a soft delete only) → `deleted`. Deleting a record that is not stored
returns `false`.

Force delete (`soft_deletes.rs`): `force_deleting` (cancellable) → the
delete flow with `Record::force_deleting` set (a real `DELETE`; listeners
of `deleting`/`deleted` see `is_force_deleting()`) → `force_deleted`. On a
model without soft deletes it is the delete flow.

Restore: `restoring` (cancellable) → the deleted-at column cleared → the
save flow (`saving`, `updating`, `UPDATE`, `updated`, `saved`) →
`restored`.

Increment/decrement (one column or several, `increment_each`): in memory
and as `SET column = column ± amount, ..` (plus extras and `updated_at`),
`updating` → `UPDATE` → `updated`; overflow and a column listed twice are
errors; a record that is not stored is `NotPersisted` (never a whole-table
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
  `between`, `like`, `eq_column`, ...), expressions, raw SQL, render-tree
  conditions (`From<render::ast::Expr>`), portable statement fragments
  (`From<navree_db::Statement>`: `Statement::portable` SQL with `?`, as
  navree-access's `SqlFilter::to_statement` writes it) and
  `and`/`or`/`not`/`all`/`any`.
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
- Text that ignores letter case beyond ASCII: the column helpers
  `contains_ignore_case` (search boxes) and `eq_ignore_case`
  (`Where::IgnoreCase`), with the same results on every database: case
  ignored for every letter with case forms, accents kept, the text taken
  literally (no wildcards), NULLs never matching. They compile through
  navree-db's `contains_ignore_case` / `eq_ignore_case`: a `GLOB` of each
  letter's case variants on SQLite (whose `LOWER` and `LIKE` fold ASCII
  only), `LOWER(..)` for ASCII text or a regular expression of case
  variants for other letters on PostgreSQL (whose `LOWER` folds ASCII only
  in a database with the `C` locale), lower-cased bytes on MySQL (so the
  column's collation never ignores accents or keeps case). navree-mongo
  matches the escaped text as a `$regex` with the `i` option, whose
  folding is Unicode's.
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
`GlobalScope<M>` (`apply`, `scope_id`, `name`: the type's name by
default) and come, in this order, from `M::global_scopes()` (declared),
the runtime registry (`m.global_scope::<M>(name, f)` /
`m.global_scope_of::<M>(scope)`, installed process-wide by `OrmModule`
at boot, since scopes apply where no executor is at hand), then
`SoftDeletes` for soft-deleting models (`global_scopes::scopes_of`). They
apply once, when the query runs (`Builder::apply_global_scopes`), unless
the query's `ScopeChoices` remove them (`without_global_scope::<S>()`,
`without_global_scope_named`, `without_global_scopes`,
`without_global_scopes_except`); `removed_scopes()` names them. The
query's trashed choice (`with_trashed`, `without_trashed`, `only_trashed`)
replaces the `SoftDeletes` scope and applies last, grouped, even after
`without_global_scopes()`. Any scope's conditions are grouped when they or
the conditions before them contain an `OR`, so a scope never changes the
meaning of the caller's conditions.

### The IR (`ir.rs`)

`QueryIr { table: TableRef, distinct, columns: Vec<SelectItem>, joins,
wheres: Vec<WhereClause>, groups, havings, orders, limit, offset, lock,
unions, group_limit, connection, use_writer, timeout, errors }`.
`TableRef` is a table, an aliased table, a subquery or a table function.
`connection`, `use_writer` and `timeout` say where and how long the query
runs (`Route`, timed reads; section 6), not what it selects.
`Where` covers `Compare`, `Null`, `In` (list or subquery), `Between`,
`Like`, `IgnoreCase`, `DatePart`, `Exists`, `Group`, `JsonContains`,
`JsonLength`, `FullText`, `Raw` (also a portable statement fragment), `Tree`
(a render-tree condition: the access filters of `accessible`) and
`Invalid` (a problem found while building, reported when the query
runs). `Expr` covers columns (with JSON paths), values, raw SQL,
functions, aggregates, arithmetic, subqueries, `TimeBucket`, `First`,
`Last` and `All`. The enums
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
extension). The renderer (`render/`, below) lowers the IR into its syntax
tree and writes it with the dialect's grammar; values are always bound.
Dialect rules live next to the clause: date parts (`CAST`/`EXTRACT`, `DATE()`..., `strftime`),
case-insensitive LIKE (`ILIKE`, `LOWER() LIKE LOWER()` / MySQL's
collation, SQLite's LIKE) and case-sensitive LIKE (`LIKE`, `LIKE BINARY`,
`GLOB`), text ignoring case beyond ASCII (navree-db's text helpers, above),
null-safe equality, regular expressions, XOR, JSON paths
(`->`/`->>`; `json_extract`), JSON containment and length, full text
(`to_tsvector @@ plainto/phraseto/websearch_to_tsquery` with a validated
language; `MATCH ... AGAINST` in natural language, expansion or boolean
mode; unsupported on SQLite), `RETURNING`, `LIMIT` before `OFFSET` on
MySQL and SQLite, row locks (none on SQLite, whose write transactions are
serialised), and the TimescaleDB helpers (native on PostgreSQL; fixed-width
buckets emulated on MySQL and SQLite with TimescaleDB's origin, Monday
2000-01-03; `first`/`last` need TimescaleDB). On PostgreSQL a JSON text
path (`->>`) is compared with the JSON text of a non-string value.
Statements run through navree-db's `Executor`: a pool of navree-db's own
(the writer, or a replica for reads outside a unit of work), the unit of
work (its transaction, navree-db's or, by default while crates move off
SeaORM, SeaORM's), or a transaction or savepoint the ORM began
(`OwnTransaction`: a transaction on the pool, `Transaction::savepoint`,
`Uow::savepoint`, or a SeaORM savepoint of a unit of work on SeaORM's
transaction). navree-db times and reports every statement (query log,
listeners, before-executing hooks, telemetry, slow-query warnings); the
ORM runs the hooks itself only for the statements that go through
SeaORM. A read on a pool that loses its connection (`DbError` of kind
`ConnectionLost`, or a driver message saying so) runs once more; a
cursor holds its transaction's connection (`ensure_free`). SeaORM's
`DatabaseConnection` and `DatabaseTransaction` stay executors while
crates still hand them over (`exec/sea.rs`, through navree-db's bridge;
their cursors read through SeaORM's own).

### The renderer (`render/`)

`navree_orm::render` is the ORM's own SQL renderer: every statement the
ORM runs is written by it. It holds no sea-query or SeaORM code (a unit
test reads its sources to keep it so); its values are
`crate::value::Value` and its output `crate::sql::Statement`.

- **The tree** (`ast.rs`): statements are queries (`WITH [RECURSIVE]`, a
  body of `SELECT`s combined by `UNION`/`INTERSECT`/`EXCEPT [ALL]`, and the
  order, limit, offset, row lock and MySQL execution-time hint of the
  whole), inserts (rows, a query or default values; skipping or updating
  conflicts, with a condition on the update; `RETURNING`), updates and
  deletes over other tables (with `ORDER BY`/`LIMIT` and `RETURNING` where
  the dialect has them) and emptying a table. Expressions cover columns,
  bound values, literals, raw fragments with portable `?` placeholders,
  `{}` templates, calls (a validated name, `DISTINCT`), `AND`/`OR`/`NOT`
  lists, comparisons and arithmetic, null-safe equality, `IS NULL`, `IN`
  (a list or a query; an empty list is `1 = 0`), `BETWEEN`, `LIKE` with
  `ESCAPE` and three letter-case modes, `EXISTS`, `CAST` to a small set of
  portable types or a checked custom name, subqueries, row values, `CASE`,
  JSON paths read as text or as JSON (negative indexes from the end), the
  upsert's proposed row and window functions. Schema-qualified names are
  `ObjectName`s, each part quoted on its own.
- **The grammars** (`grammar/`): `Grammar` writes the SQL the databases
  share as default methods; `Postgres`, `MySql` and `Sqlite` override what
  differs (`DISTINCT ON`; `LIMIT` before `OFFSET`; NULLs sorted by a
  leading `IS NULL` key on MySQL; locks, `LOCK IN SHARE MODE` and none on
  SQLite; `INSERT IGNORE` and `ON DUPLICATE KEY UPDATE`; compound operands
  wrapped as derived tables on SQLite, whose compounds cannot be
  parenthesized; a `WHERE` before an `INSERT .. SELECT`'s conflict clause on
  SQLite; `json_extract` and `->`; `<=>`, `IS`; `CONCAT`; `LIKE BINARY` and
  a `GLOB` of the converted pattern; date functions for date casts; two
  statements to empty a table on SQLite). Operands are parenthesized by
  precedence, and raw fragments and templates whenever they are an
  operand, so a hand-written `OR` never regroups its neighbours. The
  `Writer` numbers placeholders in writing order (`$n` on PostgreSQL, `?`
  elsewhere), doubles quote characters in names and escapes string
  literals per dialect (`E'..'` for a backslash on PostgreSQL, doubled
  backslashes on MySQL). Rendering never panics: what a dialect lacks is
  `OrmError::Unsupported`, a malformed tree `OrmError::InvalidQuery`.
- **The lowering** (`lower/`): `render_select`, `render_aggregate`,
  `render_exists`, `render_insert`, `render_update`, `render_update_from`,
  `render_delete` and `render_truncate` (also at `sql::render_*`) lower the
  IR with the dialect rules (date parts, LIKE variants, text ignoring
  case, JSON containment, overlaps, keys and lengths, full text, group
  limits, time buckets, vector distances, JSON path assignments, the MySQL
  timeout hint). `Where::Tree` is embedded as it is; `Where::Raw` (raw
  SQL, or a portable statement fragment) is embedded parenthesized, its
  `?` placeholders numbered with the statement's.
- **Access filters** (`access.rs`, feature `access`): `accessible` turns
  an ability's rules into one render-tree `CASE` (`ModelColumns::filter`:
  navree-access compiles the rules with `ModelColumns` as its
  `SqlColumns`, last match wins, columns qualified with the model's
  table, operands typed by the column, numbers as the column's integer or
  float; the compiled `SqlFilter` is lowered to the tree), so `$ne` is
  null-safe on every dialect (`IS DISTINCT FROM`, `NOT (a <=> b)` on
  MySQL, `IS NOT` on SQLite).
- **The tests**: `tests/render` pins each grammar's SQL per dialect and the
  `sql` tests' IR rendered through the lowering; `tests/sql` pins the
  ORM's statements per dialect. `tests/differential` runs on real
  databases (SQLite always; PostgreSQL with TimescaleDB and MySQL with the
  `postgres` and `mysql` features and their URLs): a corpus of IR
  statements covering every lowered feature, each rendered, run and
  compared by result rows, rows changed and returned, and every fixture
  table after a write with what the sea-query compiler the renderer
  replaced did with the same case on the same server (recorded before it
  was deleted: `tests/differential/compiler/<database>.jsonl`, its SQL and
  outcome per case); and statements built as trees (CTEs, `CASE`,
  windows, upserts with a condition, `INTERSECT`, lateral joins, an access
  filter) checked against the rows they must return on every database.
  Its executor (`tests/differential/exec.rs`) is the one place that
  touches a driver: statements run on navree-db's own engine (its pools,
  transactions and statements on SQLx, rows read as navree-db values),
  the layer the ORM's statements run on once the ORM moves to it.
  Where the renderer deliberately differs (the compiler's SQL was wrong
  there: a JSON key holding `"` on MySQL, `main.table` on SQLite, a union
  inside a union or a part limited per group on SQLite, a raw window order
  with `NULLS LAST` on MySQL), the case says so and the run requires the
  difference.

No sea-query is left in the ORM: `Where::Sql` and
`From<sea_query::Condition>` went with the data-layer move's step 5
(navree-access builds its filters as `SqlFilter`s). SeaORM's connections
and transactions still serve as executors (`exec/sea.rs`) until units of
work run on navree-db's transactions only (step 8).

### Document drivers (`driver.rs`)

```rust
pub trait DocumentDriver: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn time_precision(&self) -> u32 { 6 } // fractional-second digits kept; MongoDB keeps 3
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

The seams a document store's executors use:

- `DocumentTarget::with_uow(uow)` joins navree-db's unit of work (its
  change events, observers and after-commit hooks);
  `DocumentTarget::recording_changes(recorder)` sends the change events of
  model writes to the store's own unit of work (`ChangeRecorder`).
  `Target::record_change` routes an event to whichever applies.
- `ExecContext::time_precision()` (from `DocumentDriver::time_precision`)
  truncates the timestamps the ORM sets, so a saved record equals its
  stored document.
- `OrmError::unique_violation(driver, message)` makes a store's duplicate
  key a unique violation (409 through navree-db's mapping).
- `Target::records::<M>(rows)` turns rows a driver fetched natively (an
  aggregation pipeline after a query) into records, with the model's
  `retrieved` hooks.
- Cursor pagination keeps typed values a driver carries as `Value::Enum`
  (ObjectIds) in its cursors.
- `NativeFilter` carries a store's own filter (a MongoDB filter document)
  through the IR; SQL dialects refuse it.

## 7. Row decoding

`Row` wraps navree-db's `Row`: the driver's row of an SQL query, or the
`(column, value)` pairs a document driver returns (`Row::from_values`).
`M::from_row` reads each column with its field's `from_sql`. A missing column reads as the
default of a `#[model(default)]` field; for any other field it is
`OrmError::MissingAttribute` (naming the model, the column and the selected
columns) in strict mode (`prevent_missing_attributes`, the default) or when
the field's type has no `Default`, and the type's default otherwise (the
derive passes each field's default through an autoref probe,
`derive_support::DefaultOf`). `Builder::fetch` decodes with the strictness
of the executor's app (`ExecContext::strictness`). Columns that are not
model columns become the record's extras, read by their own type
(`navree_db::Row::value`). Aggregates and selected values decode leniently
into the requested type.

## 8. Errors

`OrmError` (`#[non_exhaustive]`):

| Variant | When | Over HTTP |
|---|---|---|
| `ModelNotFound { model, keys }` | `first_or_fail`, `find_or_fail`, `sole`, `refresh` | 404 (its source is a `DbError` of kind `NotFound`, which navree-db maps) |
| `MultipleRecordsFound { model }` | `sole` | 500 |
| `Db(DbError)` | the database refused (navree-db's classified error: `kind`, `code`, `constraint`) | 409 for unique and foreign-key violations (navree-db's mapping), else 500 |
| `Decode`, `Cast` | a column cannot be read as its field / a cast failed | 500 |
| `InvalidQuery(String)` | an unknown operator, wrong raw placeholders, a malformed cursor, an overflow, ... | 500 |
| `Unsupported { feature, driver }` | a feature the dialect or driver lacks | 500 |
| `NotPersisted`, `MissingKey` | an operation needing a stored record; an empty manual key | 500 |
| `Hook { model, message }`, `Driver { driver, message }` | a hook or a document driver failed | 500 |
| `MissingAttribute { model, column, expected, selected }` | a row lacks a model column (strict mode, or a field without a default) | 500 |
| `MassAssignment { model, columns, totally_guarded }` | `fill` on a model that declares neither `fillable` nor `guarded`, or keys it may not set when discarding them is refused (`DiscardPolicy::Fail`, or the strict mode `prevent_discarding_attributes`) | 500 |
| `LazyLoading { model, relation }` | a relation loaded one record at a time (strict mode) | 500 |
| `Serialize { model, column, message }` | a record did not serialize (the column, accessor or relation is named when known) | 500 |
| `Connection { connection, message }` | an unknown connection name, or a unit of work of another connection | 500 |
| `Timeout { timeout, source }` | the database stopped a read that outran the query's `timeout` | 500 |

`is_not_found()`, `is_unique_violation()`, `is_foreign_key_violation()`,
`is_timeout()` and `is_lost_connection()` classify; `db_error()` gives the
`DbError` of `Db`, `ModelNotFound` and `Timeout`. `From<DbErr>` (SeaORM
statements run inside ORM code while crates move off SeaORM) converts
through navree-db's `DbError::from`. Messages name the model, the column
and the operation; no SQL or values reach clients (navree-http logs the
source of 500s).

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
  shapes, extractors, request transactions), `TableBuilder`, the
  document-driver reference, streams (`streams.rs`), raw statements
  (`raw.rs`), bulk writes (`bulk.rs`), locks (`locks.rs`), compound
  queries (`compound.rs`), JSON and operators (`json.rs`), and connections
  (`connections.rs`: named connections, scopes, replicas, sticky reads,
  statement hooks). `tests/orm/attributes/` covers the attribute
  layer: mass assignment (`fill.rs`), casts (`casts.rs`), encryption and
  hashing (`secrets.rs`), serialization (`serialize.rs`), API resources
  and OpenAPI schemas (`resources.rs`) and model references (`refs.rs`).
  `tests/orm/lifecycle/` covers the lifecycle
  (dispatch timing, observers and listeners, typed events, custom events,
  quiet writes, soft deletes, runtime scopes, strict modes with
  `strict::scope`, pruning and `model:prune`, broadcasting, the event fake,
  the unit of work of listeners).
- `tests/strict_apps.rs`: strict modes chosen by apps through `[orm]` (a
  process of its own: an app with `OrmModule` installs its strictness for
  code without an executor).
- `tests/sql/`: SQL snapshots per dialect (insta), each showing the
  PostgreSQL, MySQL and SQLite statement and values or the error
  (`tests/sql/lifecycle.rs` captures mass restores and force deletes
  through a recording document driver; `attributes.rs`: the stored form
  of cast fields in conditions, vectors per database, with and without
  the `postgres` feature).
- `tests/integration/` (SQLite, apps booted with navree-testing):
  factories (making and creating), seeding, the assertions, route model
  binding, workspaces (every read and write path across two workspaces),
  access (with property tests: a record check and a list filter never
  disagree), validation rules, `model:show`, and the test databases.
- `tests/servers/`: PostgreSQL with TimescaleDB and MySQL, gated by
  `NAVREE_TEST_POSTGRES_URL` / `NAVREE_TEST_MYSQL_URL`, or
  `target/test-databases.env` (`cargo test -p navree-orm --features
  postgres,mysql --test servers`; a test skips with a notice when its
  server is not configured); each test runs in a schema or database of
  its own, dropped afterwards. `attributes.rs` runs every cast against the
  servers' native column types, and vectors where the server has a vector
  type (pgvector, MySQL 9; skipped with a notice otherwise). Template
  copies and truncation run here on both servers.
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
  autoloading` for one collection or record). Rules come from a
  `LazyLoading::scope`, else `LazyLoading::install`, else the strict modes
  of the operation (`ExecContext::strictness`: `prevent_lazy_loading` with
  `on_lazy_loading`, `auto_eager_load`), so `[orm]` configures them.
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
pivot-model identity, `refresh` reloading loaded relations); the
`*_quietly` relation writes run inside `without_events`; `exec` (`begin_transaction`,
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
- **orm-lifecycle (`OrmModule`).** Boot step: register the morph map and
  `MorphMap::require` from `[orm] require_morph_map`. (Lazy loading already
  follows the module's strict modes.)
- **Access.** navree-access's `Ability` implements `AccessFilter` where both
  crates meet (the facade, or an adapter crate): navree-orm cannot depend
  on navree-access.
- **Document drivers.** Belongs-to, has-one/many and morph eager loads are
  plain batched `IN` queries any driver answers; pivot and through
  relations join, and per-parent limits use a window, so they need SQL.

### orm-lifecycle (shipped; section 12)

- Owns `src/events/`, `src/quiet.rs`, `src/soft_deletes.rs`,
  `src/lifecycle.rs`, `src/global_scopes.rs`, `src/strict.rs`,
  `src/options.rs`, `src/module.rs`, `src/prune.rs`, `src/broadcast.rs`,
  and `navree-orm-macros/src/model/lifecycle.rs`.
- Seams it offers the other tracks:
  - `ModelHook::kind()`: a hook implemented directly is a
    `HookKind::Behaviour` (runs at once, also in quiet operations), so the
    relations track's `touches` hooks keep writing inside the transaction.
    Observers, closure listeners, `Dispatch` and broadcasts are listeners.
  - `fn hooks()` is written by `lifecycle::hook_entries` +
    `lifecycle::hooks_fn` (one `static` per model): a track that declares
    hooks on the model (`touches`) adds its entries to that list.
  - Strict modes: `ExecContext::strictness()` (per app) where there is an
    executor, `strict::current()` where there is none;
    `Strictness::check(Violation::..)` answers a violation (error, handler,
    or tolerated). The attributes track's `fill` calls it with
    `Violation::DiscardedAttributes` (`OrmError::MassAssignment`); the
    relations track's lazy-loading check with `Violation::LazyLoading`
    (`OrmError::LazyLoading`) and reads `auto_eager_loads()`.
  - `OrmModule` registers boot steps in `module.rs::register`; tracks add
    theirs as separate lines (the model registry, `RegisteredModel`, serves
    `model:show` and the morph map too).
  - `ModelEventFake` (in `events/`) is the testing stand-in for listeners;
    `ModelMeta::pruning()` tells tooling whether a model is prunable.

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
are dropped, reported or refused, first match wins: the task's
`DiscardPolicy` (`with_discard_policy`), the task's strict modes
(`strict::scope`), the process's `DiscardPolicy` (`set_discard_policy`),
the strict modes of the app that booted last with `OrmModule`
(`prevent_discarding_attributes`, `on_discarded_attributes`), else they
are dropped. Also `force_fill`, `fill_only`, `set_attribute`, `set_json_path`,
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

- orm-lifecycle: `fill` answers discarded keys with
  `Strictness::check(Violation::DiscardedAttributes)` when no
  `DiscardPolicy` is set for the task or the process (a `strict::scope`
  outranks the process's policy); a quiet `force_create` is
  `without_events` around `Model::force_create`.
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
  `connect_timeout_secs`, `test_before_acquire`, `idle_timeout_secs`,
  `max_lifetime_secs`),
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
- **Not moved, by design.** Table prefixes per connection (PostgreSQL
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

Built. What each part is and where it plugs in:

- **Factories** (`src/factory/`): `ModelFactory` (a `definition` from a
  `FactoryContext`: index, count, a seeded `Faker`, the clock,
  `belongs_to` parents; `configure` for default hooks; `relationships`
  naming the model's relationships), `HasFactory` with
  `#[model(factory = F)]` (`Deal::factory()`), and the `Factory<F>`
  builder: `count`, states (closures and functions, appended or
  prepended), `set`, sequences (`Sequence::new`, `column`, `from_fn`,
  `for_each_sequence`, `cross_join_sequence`), `trashed`/`trashed_at`,
  `when`, `seed`, `connection`, `for_`/`for_record`/`has`/`has_attached`
  through named relationships (`BelongsTo`, `HasMany`, `Pivot` links in a
  `Relationships<M>` registry), `recycle`, `without_parents`, `lazy`,
  after-making and after-creating hooks (removable), `make`/`raw`,
  `create` (one `create_on` pass: parents, the records through
  `Record::save`, children, pivot rows), `create_quietly` and `insert`
  (one bulk INSERT, `bulk.rs`). `FactoryOptions` (an app binding) sets the
  seed and whether definition parents are created. Quiet operations are a
  task-local (`factory/quiet.rs`) that `Hooks::resolve` checks; the model's
  own hooks (workspace scoping) still run.
- **Seeders** (navree-db): roots, `ctx.call::<S>()` / `call_once` /
  `call_silent` / `call_seeder`, `Seeder::without_model_events` (the
  task-local `seeding_without_model_events`, read by the quiet check), and
  `db:seed [names] [--database D] [--force]` (production needs `--force`).
  `TestAppExt::seed::<S>()` / `seed_all()` (feature `testing`).
- **Assertions** (`src/testing/`): `assert_database_has`/`missing` (closest
  rows on failure), `assert_database_count`/`empty`,
  `assert_model_exists`/`missing`, `assert_soft_deleted`/`not_soft_deleted`
  (by record or by table), `json_value` for structural JSON, and
  `expect_query_count`/`count_queries` (statements counted at the
  executors).
- **Test databases** (`src/testing/database.rs`, feature `testing`):
  `Template` (built once per test binary, copied per test: SQLite file
  copy, PostgreSQL `CREATE DATABASE … TEMPLATE`, MySQL tables and rows),
  `TestDb` (`sqlite`, `memory`, `postgres`, `mysql`, `truncate`,
  `transaction` — a unit of work ended by navree-db's
  `Uow::rollback_as_committed`, so after-commit hooks run), `LazyTestDb`,
  `TestServer` and `test_setting` (the `NAVREE_TEST_*_URL` variables or
  `target/test-databases.env`), and `#[test]` (`#[navree::test]`,
  navree-orm-macros: an async test on a runtime of its own, given a
  `TestDb` from a template, memory or a server; skipped with a notice
  without a server). `TestAppBuilder::memory_database()` (navree-testing)
  boots apps on in-memory SQLite.
- **Route model binding** (`src/binding.rs`): `Bound<M, B = RouteKey>`
  (the model's route key, `#[model(route_key)]`, read as the column's type:
  a malformed UUID or ULID is a 404 without a query), `RouteBinding` for
  custom bindings and parameters, `WithTrashed<B>`, `BoundChild<P, C>`
  through `ChildOf<P>`; loads run in the request's unit of work, so global
  scopes (workspace included) apply.
- **Workspaces** (`src/workspace.rs`): `#[model(workspace)]` adds the
  built-in `WorkspaceScope` (`apply_global_scopes`) and `WorkspaceHook`
  (first in `Hooks::resolve`: fills the column on create, refuses
  changing it, treats another workspace's record as missing). The current
  workspace is the explicit `workspace::scope`, else the executor's unit of
  work (`Uow::workspace`, which navree-workspace sets on every isolation
  strategy), else `Uow::current()`; without one, scoped queries fail
  closed. Opt-outs: `without_workspace_scope()`, `for_workspace(id)`.
  PostgreSQL row-level security and schema isolation cover ORM queries,
  and a `WorkspaceUow` (database isolation) is an executor
  (navree-workspace's tests).
- **Access** (`src/access.rs`, feature `access`): `#[model(subject,
  permission_prefix, policy)]` implements navree-access's `Subject` and
  `SubjectRecords` (records load by route key in the unit of work, for
  `Authorized<Model, A>`); `ModelColumns<M>` maps rule conditions to
  columns, `Builder::accessible(&ability, A)` and `Accessible<M, A>`
  (`AccessibleQuery`) filter queries (counts and pages included), and
  `AbilityExt` checks records; policies are registered by
  `AccessModule::for_feature::<Model>()`.
- **Validation** (`src/validation.rs`, feature `validation`): `unique`
  and `exists` build navree-validation `AsyncRule`s over a `UowCheck` that
  queries the model in the request's unit of work (scopes and workspace
  included; `ignore`, `ignore_path_param` by route key, `where_`,
  `with_trashed`/`without_trashed`, `query`).
- **`model:show`** (`src/show.rs`, `src/module.rs`): `OrmModule`
  registers the command; `ModuleBuilderOrmExt::model::<M>()` registers a
  model (`RegisteredModel`, `registered_models`) and imports the module.
  The description comes from `ModelMeta` (which records the factory, its
  relationships, the subject and the policy) and the app's hooks.
- **Generators** (navree-cli): `make:model <module> <Name> [--migration
  --factory --seeder --controller --policy --pivot | --all]` (the files
  refer to each other: the model links its factory and policy, the
  controller binds records by route key or checks abilities when there is
  a policy, the seeder uses the factory; the module registers the model,
  its access feature, seeder and controller), `make:factory <module> <name>
  [--model M]`, `make:seeder <module> <name>`. `--resource` is not offered:
  admin-panel resources are built from SeaORM entities.

### mongodb

Built as `crates/navree-mongo` (L5; facade features `mongo` and `mongodb`,
opt-in); its `DESIGN.md` has the details.

- `MongoDriver: DocumentDriver` compiles the IR into filter documents,
  `find`s and aggregation pipelines (`compile`), and runs them on the
  official driver, in a session when it has one. Executors: `MongoDb` (the
  connection; it joins the task's `MongoUow` on the same connection),
  `MongoSession` and `MongoUow`, all returning
  `Target::Document(DocumentTarget::new(driver, app))` with the seams of
  section 6.
- ObjectId keys: `navree_mongo::ObjectId` implements `Field` and
  `PrimaryKey`; `KeyKind::ObjectId` generates it before the insert when it
  is all zeros (like UUID keys, so the record knows its key without a round
  trip), stored as `_id`. Integer keys the database would assign are
  counted in a `navree_sequences` collection, one atomic counter per
  collection and field.
- Embedded documents and arrays are JSON casts; joins, subqueries
  (`EXISTS`, `IN`, scalar) and unions are `$lookup` and `$unionWith`, an
  equality with the enclosing document matched by `localField` and
  `foreignField` so an index serves it. Relations (orm-relations) load
  through the IR's `IN` lists, which become `$in`.
- Units of work: `MongoUow` (a session with a transaction, the actor and
  request id, change events for `MongoChangeObserver`s inside the
  transaction, after-commit hooks), `MongoDb::transaction` retrying on
  write conflicts, and `MongoDb::begin_in(&Uow)` joining navree-db's unit
  of work (its change events go there; the MongoDB transaction commits
  right after it, not atomically with it).
- The schema builder (`MongoSchema`) takes navree-schema's blueprints:
  collections with JSON-schema validators, indexes, TTL and text indexes,
  hypertables as time-series collections, continuous aggregates as views;
  `MongoMigrator` runs `MongoMigration`s recorded in a collection.
- Bulk writes: an `InsertIr` with `on_conflict` (the builders'
  `insert_or_ignore` and `upsert`) is an unordered `insertMany` skipping
  duplicate keys, or one `update` command of upserts by the unique fields.
  Cursor streams: `DocumentDriver::stream` reads a MongoDB cursor batch by
  batch (in the session when there is one), for `cursor` and `lazy` as for
  `MongoStream::stream`.
- Tests: the generated commands as snapshots without a server, the rest
  against `NAVREE_TEST_MONGODB_URL` (a replica set), locally through
  `scripts/dev-databases.ps1` and in CI (`.github/workflows/mongodb.yml`).

### orm-adopt-crates (shipped): the framework's tables as models

The framework crates' tables are navree-orm models, and the crates read
and write them through the ORM where that keeps behaviour and reads
better.

- **One struct, two derives.** A table whose SeaORM entity is public API
  (apps, Helm's panels and tests query `Entity`, `Column` and
  `ActiveModel`) keeps it: its `Model` derives `DeriveEntityModel` and
  `navree_orm::Model` together, with `#[model(table = "..")]` (and
  `primary_key`, `column = ".."` where the entity says so). The struct is
  both; nothing public changes, and apps gain `AuditRecord::query()`,
  `User::query()`, typed columns and records. `tests/orm/entities.rs` pins
  the pattern: rows written by one API read by the other, dirty tracking,
  serialization through the struct's own serde (secrets marked
  `#[serde(skip_serializing)]` stay out of records too). The derive must
  name the trait by path (`navree_orm::Model`): the struct is called
  `Model`.
- **The tables.** navree-audit (`audit_logs`, `AuditRecord`), navree-notify
  (`notifications`), navree-queue (`failed_jobs`: `FailedJob` itself, with
  `JsonText` payload and options and a `unix_timestamp_ms` failure time),
  navree-auth (users, refresh tokens, password resets, sign-in tokens,
  passkeys), navree-workspace (workspaces, members, invitations,
  organizations and their members and grants, access requests, partitions),
  navree-social, navree-webhooks, navree-oauth, navree-flags
  (`feature_flags`), navree-billing and navree-lighthouse. Tables keyed by
  several columns (flag overrides, saved locales) stay entities: a model
  has one key column. Crates below the ORM's layer, and navree-access and
  navree-validation (the ORM's optional dependencies), keep SeaORM.
- **Executors keep the semantics.** Code that wrote on the pool outside the
  request (the audit log's own connection) passes `db.conn()`; code that
  wrote in a transaction it was handed passes `&*tx` (no change events,
  exactly as the SeaORM statement recorded none); reads that went through
  `Db::reader` pass `&Db`, which reads the same way.
- **Change events.** An ORM write through a `Uow` records the stored column
  values; `Repo` records the model's serde output. Writes of rows with
  skipped fields (users, webhook endpoints, social accounts, workspace
  rows) stay on `Repo`, so audit trails keep their exact content; the
  plain inserts and updates that recorded no change events moved to the
  ORM: auth's sessions, sign-in tokens, resets and passkeys, the webhook
  delivery log, the OAuth server's clients, grants and tokens, failed
  jobs, stored notifications and audit rows. Reads moved with them
  (auth's and social sign-in's user lookups too).
- **Errors.** `From<OrmError> for DbErr` (database errors as SeaORM would
  have returned them, not-found as `RecordNotFound`, the rest as
  `DbErr::Custom`) lets services
  whose API speaks `DbErr` use `?`; the crates' own errors convert through
  it (`QueueError`, `AuthError`, `WebhooksError`, `OAuthError`,
  `SocialError`, each into its database variant).
- **`JsonText`** (`cast.rs`): serde values stored as JSON text in `TEXT`
  columns, for tables created before JSON columns were used.
- **Migrations stay SeaORM.** The schema builder's DDL is not the SeaORM
  migrations' (PostgreSQL `VARCHAR(255)` for `string` where they made
  unbounded `varchar`; MySQL `CHAR(36)` for `uuid` where they made
  `binary(16)`, which is how both ORMs bind UUIDs there; MySQL
  `TIMESTAMP(6)` for `timestamp(0)`), so rewriting the table-creating
  migrations would give new databases other columns than migrated ones.
  New migrations of these crates use navree-schema (the hypertable
  conversions do), and the facade's `framework_models` suite checks the
  models on SQLite, PostgreSQL and MySQL.

## 12. The lifecycle

### Events and when listeners hear them

`ModelEvent` lists the fifteen built-in events (`ModelEvent::ALL`) and
`Custom(name)`. Every flow announces its steps to `Hooks<M>` (`hooks.rs`),
resolved once per operation: the model's own hooks (`M::hooks()`), then
the app's `dyn ModelHook<M>` multi-bindings. Each hook has a `HookKind`:

| Kind | `-ing` events | "-ed" write events in a unit of work | quiet operations |
|---|---|---|---|
| `Behaviour` (default) | at once | at once, in the transaction | still run |
| `TransactionalListener` | at once | at once, in the transaction | silenced |
| `Listener` | at once | after the commit, on a snapshot of the record | silenced |

The write events are `created`, `updated`, `saved`, `deleted`, `trashed`,
`force_deleted` and `restored` (`ModelEvent::runs_after_commit`). A
listener's after-commit call is queued with `Uow::after_commit` together
with a clone of the record as the event saw it (so `get_changes`,
`was_changed` and `get_original` describe that write); a rollback drops it.
Outside a unit of work every hook runs at once. Every hook runs with the
operation's unit of work as the task's `Uow::current()`, so jobs and events
it dispatches join that unit of work (written in its transaction, or queued
after its commit). An error from an `-ing` hook stops the operation; an
after-commit listener's error or panic is logged (target
`navree_orm::events`), reported to the app's telemetry
(`Telemetry::report_exception`) and never undoes the commit.

The listener kinds an app writes:

- `Observer<M>` (`events/observer.rs`): one async method per event (`-ing`
  ones return `Flow`), `IN_TRANSACTION` for a transactional listener;
  declared on the model (`#[model(observers(..))]`, also without an app) or
  registered by a module (`m.observe::<M>(o)`, `m.observer::<M, O>()` built
  by the container).
- Closure listeners: `m.on_model::<M, _>(event, |record, ctx| ..)`.
- `Dispatch<M, E>`: `#[model(dispatches(created = DealCreated))]` emits the
  navree-events `Event` `E: From<&Record<M>>` (`-ing` events at once, a
  failing listener refusing the write).
- `BroadcastHook<M>`: `#[model(broadcasts)]` with `impl Broadcasts`
  (`broadcast.rs`): the record's private channel, `deal.created`-style
  names, `{"deal": <record>}` data, or a queue (`orm.broadcast` jobs through
  `dyn Dispatcher`); `without_broadcasting(fut)`.
- `Record::fire(exec, "archived")`: custom events, heard at once through
  `Observer::custom` and closures of `ModelEvent::Custom(name)`; listeners
  may cancel; built-in names are refused. `Record::replicate_on(exec)` runs
  `replicating` on the copy.

`without_events(fut)` (a task-local) keeps only behaviours; the quiet
operations wrap it (`save_quietly`, `update_quietly`, `delete_quietly`,
`touch_quietly`, `increment[_each]_quietly`, `decrement[_each]_quietly`,
`create_quietly`, `restore_quietly`, `force_delete_quietly`,
`replicate_quietly`). Change events for the audit trail are still recorded.
With a `ModelEventFake` in the app, behaviours run and a recorder stands in
for every listener. Mass writes read no records and fire no events.

### Soft deletes

`#[model(soft_deletes)]` makes the derive implement `SoftDeleting`, which
unlocks `Record::restore`, `Builder::only_trashed`, `Builder::restore` and
the restore-or-create family; `force_delete` (record and builder),
`with_trashed`, `with_trashed_if` and `without_trashed` work on every model
(force delete is a plain delete without soft deletes; the filters do
nothing). A query's trashed choice lives in its `ScopeChoices` and applies
last, grouped (section 5). `Builder::restore` restores only the trashed
rows the query matches; `Builder::force_delete` includes trashed rows
unless the query chose otherwise; neither fires events.
`force_destroy(keys)` loads and force-deletes each record.
`restore_or_create` / `create_or_restore` find with trashed rows included
and restore a trashed match.

### Strict modes and `OrmModule`

`OrmModule` (global; `for_root(OrmOptions)`, `from_config()` or plain,
reading `[orm]`) provides `OrmRuntime`: the options, the app's environment
(`env` / `app.env`) and the resolved `Strictness`. At boot it installs the
runtime global scopes and makes its strictness the process's (for code
without an executor), and it registers `model:prune` and the job
`orm.broadcast`. `strict` unset means strict unless the environment is
`production`. `ConfiguredOrmModule::on_missing_attribute`,
`on_discarded_attributes` and `on_lazy_loading` install handlers that
replace the errors (`Ok(())` tolerates a violation). The strictness of an
operation: `strict::scope`, else the executor's app
(`ExecContext::strictness`), else the process's, else
`Strictness::default()` (missing and discarded attributes are errors, lazy
loading is allowed). `ModuleBuilderOrmExt::model::<M>()` and
`global_scope*` import `OrmModule`.

### Pruning

`Prunable` (record by record: `prunable(query)`, the `pruning` hook, a
force delete for soft-deleting models, chunks by key, a failing record
logged and reported) and `MassPrunable` (`force_delete` of a limited query
until nothing is left) each provide `prune_all(exec, chunk)`.
`#[model(prunable)]` / `#[model(mass_prunable)]` put a `Pruning` (erased
`run` and `count` on a `Db`) in the model's `ModelMeta`; `prune(app, run)`
prunes the registered models that have one (`--model`, `--except`,
`--chunk`: the model's `CHUNK_SIZE`, else the option, else
`[orm] prune_chunk`; `--pretend` counts). `ModelsPruned` (a running total
per chunk), `ModelPruningStarting` and `ModelPruningFinished` go through
the app's `EventBus`. Apps made by `navree new` add `OrmModule` and
`ScheduleModule` and schedule `model:prune` daily at 03:00 (the
`Housekeeping` module of the generated `src/app.rs`).
