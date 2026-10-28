# navree-db: design notes

This file holds the design of the parts of navree-db that other crates
build on. The engine itself (pools, statements, values, rows, errors,
transactions, units of work) is described in
`docs/superpowers/specs/2026-10-28-navree-data-layer-on-sqlx.md`, §2.1.

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
  (a portable `?` fragment) and `add` (a nested group).
- `Select<R>`: `filter`, `eq`, `order_by(_desc)`, `limit`, `offset`,
  `for_update` (nothing on SQLite, whose units of work begin `IMMEDIATE`);
  `all`, `one`, `count`, `page(PageQuery) -> Page<R>`; `statement(Backend)`.
- `Update<R>` (`set`, `execute -> rows`) and `Delete<R>`.
- Free functions in `navree_db::record`: `find`, `insert`, `update` (every
  column by key), `insert_returning` / `update_returning` (the row as the
  database stored it: timestamps rounded to its precision), `upsert` (`ON
  CONFLICT .. DO UPDATE` / `ON DUPLICATE KEY UPDATE`), `delete`, and the
  recorded writes on a `Uow`: `insert_recorded`, `update_recorded(old,
  new)`, `delete_recorded`, which record the same `ChangeEvent`s `Repo`
  does (the record's `Serialize` form, so a `skip_serializing` secret stays
  out of audit trails; an update records only the changed fields).
- `Db::read_executor()`: where a read goes, as `Db::reader` decides for
  SeaORM: the task's open unit of work, the writer after a sticky write or
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
6. Units of work: test with `database.native_units_of_work = true` as well
   as the default, on SQLite, PostgreSQL and MySQL.

For navree-access (step 5) the same applies to its 6 tables: its ability
filters become `Filter::raw` fragments (portable SQL with `?`), the shape
`navree_orm::Filter::raw` accepts.
