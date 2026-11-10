# navree-db: design notes

This file holds the design of the parts of navree-db that other crates
build on. The engine itself (pools, statements, values, rows, errors,
transactions, units of work) is described in the crate documentation
(`src/lib.rs` and the modules under `src/engine/`).

## Records: typed rows below the ORM

Crates at layer 3 and below cannot depend on navree-orm (navree-mail is
below it; navree-access and navree-validation are optional dependencies of
it). They still keep typed rows, written and read through navree-db's
statements alone. A **record** is that row: a struct whose table and
columns are declared once, and every statement is built from that list.

```rust
use chrono::{DateTime, Utc};
use navree_db::record::{self, Filter};
use navree_db::{Record, Uow};
use serde::Serialize;
use uuid::Uuid;

navree_db::record! {
    /// A suppressed address.
    #[derive(Clone, Debug, PartialEq, Serialize)]
    pub struct SuppressionRow in "mail_suppressions" {
        pub id: Uuid,
        pub workspace_id: Option<Uuid>,
        pub email: String,
        #[serde(skip_serializing)]          // kept out of change events
        pub note: String,
        pub created_at: DateTime<Utc>,
    }
}

async fn demo(uow: &Uow, row: SuppressionRow) -> Result<(), navree_db::DbError> {
    // Reads: filters, `None` as IS NULL, groups, IN, order, pages, FOR UPDATE.
    let existing = SuppressionRow::select()
        .eq("email", row.email.as_str())
        .filter(Filter::any().is_null("workspace_id").eq("workspace_id", row.workspace_id))
        .order_by_desc("created_at")
        .one(uow)
        .await?;
    if existing.is_none() {
        // A write through the unit of work that records a ChangeEvent.
        let stored = record::insert_recorded(uow, &row).await?;
        let _ = stored;
    }
    // A set-based claim: only one of two racing writers changes the row.
    let claimed = SuppressionRow::update_where(Filter::all().eq("id", row.id).eq("note", ""))
        .set("note", "claimed")
        .execute(uow)
        .await?;
    let _ = claimed;
    Ok(())
}
```

### What it is

- `trait Record`: `TABLE`, `COLUMNS` (in the order of `values()`), `KEY`
  (`id` by default), `from_row(&Row)`, `values() -> Vec<Value>`. Provided:
  `key()`, `select()`, `update_where(Filter)`, `delete_where(Filter)`.
- `record!`: a `macro_rules!` that writes the struct and the impl from one
  field list. Each field's column is its name (`kind: String as "type"`
  renames it); `key "code"` names another key; attributes pass through.
  Field types read with `FromValue` and bind with `Into<Value>`, so every
  type navree-db's values know works, `Option<T>` included.
- `Filter`: `all()` / `any()` with `eq` (a NULL value is `IS NULL`),
  `compare`, `is_null`, `not_null`, `is_in` (empty matches nothing), `raw`
  (a portable `?` fragment) and `group` (a nested group).
- `Select<R>`: `filter`, `eq`, `order_by(_desc)`, `limit`, `offset`,
  `for_update` (nothing on SQLite, whose units of work begin `IMMEDIATE`);
  `all`, `one`, `count`, `page(PageQuery) -> Page<R>`; `statement(Backend)`.
- `Update<R>` (`set`, `execute -> rows`) and `Delete<R>`.
- Free functions in `navree_db::record`: `find`, `insert`, `update` (every
  column by key), `insert_returning` / `update_returning` (the row as the
  database stored it: timestamps rounded to its precision), `upsert` (`ON
  CONFLICT .. DO UPDATE` / `ON DUPLICATE KEY UPDATE`), `delete`, and the
  recorded writes on a `Uow`: `insert_recorded`, `update_recorded(old,
  new)`, `delete_recorded`, which record the same `ChangeEvent`s as
  navree-orm's model writes (the record's `Serialize` form, so a `skip_serializing` secret stays
  out of audit trails; an update records only the changed fields).
- `Db::read_executor()`: where a read goes: the task's open unit of work, the writer after a sticky write or
  inside `Db::using_writer`, else a read replica. `Db::same_as` tells
  whether two handles share pools (whether to join the task's unit of
  work).

Column names in filters, ordering and `SET` are checked against `COLUMNS`
(`ErrorKind::InvalidStatement` for a typo, the same on every backend);
identifiers are quoted per backend; values are always bound; SQL is built
with `?` placeholders (`Statement::portable`), so a record never names a
backend.

### Why a `macro_rules!` declaration and not a derive

- The users are a dozen tables (navree-mail 5, navree-access 6), all plain
  columns and a single key. A declarative macro gives them the one-place
  column list a derive would, in about forty lines that anyone can read.
- A derive would live in navree-macros, next to navree-orm's
  `#[derive(Model)]`, as a second, smaller model derive: every user would
  gain a dependency edge on the proc-macro crate, and the trait and its
  code generator would sit in two crates. The `macro_rules!` lives beside
  the trait it implements and expands to plain code.
- Anything richer (relations, scopes, casts, soft deletes, events per
  model, eager loading) is the ORM's job. Records stop at what a store
  needs, so they cannot grow into a second ORM.
- The trait is still implementable by hand where the macro does not fit
  (a computed column, a composite key).

### How a crate uses it (navree-mail, navree-access)

1. Declare one record per table (`entity.rs`), field names and types as the
   table has them. The public types of the crate stay its own (`Delivery`,
   `Suppression`, ...) with `From<&Row>` conversions; records are the
   storage shape.
2. Reads outside a unit of work use `db.read_executor().await`; reads and
   writes inside one take the `&Uow` (an `Executor`). Join the task's
   unit of work when it is unfinished and `current.db().same_as(&db)`.
3. Writes that admins or tenants make (settings, lists) use the recorded
   functions so audit trails see them; high-volume logs (deliveries,
   events) use plain `insert` and `update_where`.
4. Concurrency: claim rows with `update_where(.. status = 'held')` and
   check the affected count; read a row you are about to change with
   `for_update()`.
5. Errors: `DbError` and its `ErrorKind` (`UniqueViolation`, `NotFound`,
   `is_retryable()`), wrapped in the crate's own error.
6. Units of work: test on SQLite, PostgreSQL and MySQL.

For navree-access the same applies to its 6 tables: its ability
filters become `Filter::raw` fragments (portable SQL with `?`), the shape
`navree_orm::Filter::raw` accepts.

## Prepared statements and parameter types

### The problem it solves

Statements run through SQLx, which prepares each statement once per
connection and caches it by its SQL text. On PostgreSQL the prepare
(`Parse`) carries the type of every parameter, taken from the values of
that first run, and later runs bind their values in binary form against
those types. When the same SQL ran again with a value of another type in
a placeholder, the connection reused the first statement and the server
read the new bytes as the old type:

| first run | then | on PostgreSQL |
|---|---|---|
| `amount > ?` with `I64(1)` | `F64(3.5)` | no rows (the float's bits read as a huge integer) |
| `amount >= ?` with `F64(2.5)` | `I64(3)` | every row (the integer's bits read as a tiny float) |
| `code = '' \|\| ?` with `I64(7)` | `"12345678"` | no rows (eight bytes of text read as an integer) |
| `amount > COALESCE(?, 0)` with `Null(I64)` | `F64(2.0)` | no rows |
| `amount > COALESCE(?, 0)` with `Null(F64)` | `I64(2)` | every row |
| `... ? ...` with `I64` | `"a"`, `F32` | `08P01` insufficient data left in message |
| `... ? ...` with `I32`, `U64(2)` | `Decimal`, `U64::MAX` (`NUMERIC`) | `22P03` incorrect binary data format |
| `... ? ...` with text or a text `NULL` | `I64(7)` | `22021` invalid byte sequence (`0x00`) |

The same happened on every pooled connection that had run the statement,
inside transactions and savepoints, in streamed rows, through navree-orm's
builder over an untyped column (`col("amount").gt(..)`), and in units of
work. Nothing failed where the sizes matched: the rows were silently
wrong.

MySQL sends the parameters' types with every run, so parameters are read
correctly, but a prepared statement keeps the result types it derived
from earlier runs: `SELECT ? AS v` returned a decimal as a `DOUBLE`, text
as a number, a date as a date-time and a boolean as `NULL` once the
statement had run with a value of another type (`CAST(? AS CHAR)` and
comparisons were right). SQLite types values, not parameters or columns,
and answered every run as a first run.

### How: the statement's text names its types

`driver::Keyed` builds the text a statement is prepared with: the SQL as
written, then a comment naming the type each value binds, as the driver
sends it, before any trailing `;` (after a `;`, MySQL would read the
comment as another statement):

```sql
SELECT id FROM measures WHERE amount > $1 ORDER BY id /* params: INT8 */
SELECT id FROM measures WHERE amount > $1 ORDER BY id /* params: FLOAT8 */
```

SQLx's cache key is the text, so the key is the SQL plus the
parameter types: a run with other types is another statement, prepared
for its own types; runs with the same types share one prepared statement
per connection, as before. The names come from the same `Encode`/`Type`
the value is bound with, so they cannot drift from what is sent (a `u64`
beyond `i64` binds `NUMERIC` and is named so; an enum binds text, its
`CAST(? AS mood)` being in the SQL; a vector is pgvector's `vector`). A
`NULL` is named by its kind's type. Statements without values keep their
text unchanged. PostgreSQL and MySQL key their statements; SQLite runs the
SQL as written.

Considered:

- **Placeholders typed in the SQL** (`$1::int8`): needs a parser that
  finds placeholders outside strings, comments, quoted identifiers and
  dollar quotes, and changes the SQL users wrote; the text still changes
  per type, so it is the same cache behaviour with more risk.
- **A cache keyed by SQL and parameter types**, in navree-db: SQLx's cache
  is private and keyed by text; a second cache would mean preparing and
  closing statements by hand beside SQLx's. Putting the types in the text
  is that key, with SQLx's own cache, eviction and `Close`.
- **Re-preparing when the type signature changes**: needs to know, per
  physical connection, which types a cached statement was prepared with.
  Pooled connections carry no navree-db state, and asking SQLx's cache
  (`prepare_with`) costs a lookup, and catalog queries on a miss. A
  statement whose types alternate would re-prepare on every change
  instead of keeping both.

Whether a value is `NULL` is not part of the key: PostgreSQL types a
`NULL` by its kind, and MySQL reads values correctly after a `NULL` of
their type ran first; only the kind a `NULL` read back from `SELECT ?`
reports on MySQL follows the statement (`NULL` either way). Keying by
nullness would multiply the statements of every insert with nullable
columns.

### What users see

- Navree's query log (`QueryLog`, `Db::on_query`, `before_executing`),
  the slow-query log, telemetry, errors and `Statement`'s `Display` show
  the SQL as written, without the comment, and so does `migrate
  --pretend`.
- The database sees the comment: PostgreSQL's `pg_stat_activity.query`,
  `pg_prepared_statements`, server logs (`log_statement`,
  `log_min_duration_statement`) and `pg_stat_statements` show
  `... /* params: INT8 */`; MySQL's process list and logs show it after
  the statement with its values in place. PostgreSQL error positions are
  unchanged (the comment is at the end), and `pg_stat_statements` already
  counted statements whose parameter types differ separately.
- A connection holds one prepared statement per SQL and set of types,
  bounded by SQLx's statement cache as before.

### Cost

Building the text is one allocation (the SQL was copied before too) and
one type name per value. A timed loop of 2,000 runs per statement on one
connection (`stable_types_timing`; microseconds per statement, the median
of three runs, each the median of three rounds, on a development machine
running other builds):

| statement | PostgreSQL before | after | MySQL before | after |
|---|---|---|---|---|
| select, 2 values | 169.4 | 107.0 | 255.6 | 244.3 |
| select, 8 values | 160.4 | 133.9 | 307.0 | 270.9 |
| insert, 3 values | 131.8 | 106.4 | 279.4 | 212.8 |

The differences are the machine's noise; what matters is that stable
types are not slower: they still run one prepared statement per
connection (the PostgreSQL test counts them in `pg_prepared_statements`).

### Tests

`tests/engine/placeholder_types.rs`, on every server: integers then floats
and back, text then integers, typed `NULL`s then other kinds, every
pooled connection, transactions, savepoints, streams, units of work,
every kind of value after every other through
`SELECT ?` and a cast to text, the prepared statements on PostgreSQL, and
what logs and the server see. navree-orm's `tests/servers/value_types.rs`
runs the builder over an untyped column.
