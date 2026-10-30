//! [`Record`]: a table's typed row on navree-db's statements alone, for the
//! crates that sit below the ORM (navree-mail, navree-access).
//!
//! A record names its table and columns once ([`record!`](crate::record)
//! writes the struct and the impl from one declaration); every statement
//! is built from that list:
//!
//! - reads: [`find`], [`Record::select`] with [`Filter`]s, ordering,
//!   limits, `FOR UPDATE`, [`Select::count`] and [`Select::page`];
//! - writes on any [`Executor`]: [`insert`], [`update`] (every column by
//!   key), [`insert_returning`] / [`update_returning`] (the row as stored,
//!   timestamps rounded by the database), [`upsert`], [`delete`], and
//!   [`Record::update_where`] /
//!   [`Record::delete_where`] for set-based changes (a claim:
//!   `UPDATE .. SET status = 'released' WHERE id = ? AND status = 'held'`);
//! - writes through a [`Uow`] that also record [`ChangeEvent`](crate::ChangeEvent)s
//!   for audit trails and other observers, as navree-orm's model writes do:
//!   [`insert_recorded`], [`update_recorded`], [`delete_recorded`].
//!
//! Column names in filters, ordering and `SET` are checked against the
//! record's columns (a typo is an [`ErrorKind::InvalidStatement`](crate::ErrorKind::InvalidStatement),
//! not SQL that fails on one backend); identifiers are quoted per backend;
//! values are always bound. [`Filter::raw`] takes a portable fragment for
//! anything else.
//!
//! ```
//! use chrono::{DateTime, Utc};
//! use navree_db::{Backend, Filter, Record};
//! use uuid::Uuid;
//!
//! navree_db::record! {
//!     /// A suppressed address.
//!     #[derive(Clone, Debug, PartialEq)]
//!     pub struct SuppressionRow in "mail_suppressions" {
//!         pub id: Uuid,
//!         pub workspace_id: Option<Uuid>,
//!         pub email: String,
//!         pub created_at: DateTime<Utc>,
//!     }
//! }
//!
//! assert_eq!(SuppressionRow::COLUMNS, ["id", "workspace_id", "email", "created_at"]);
//! let select = SuppressionRow::select()
//!     .filter(Filter::any().is_null("workspace_id").eq("workspace_id", Uuid::nil()))
//!     .eq("email", "ada@example.com")
//!     .order_by_desc("created_at");
//! assert_eq!(
//!     select.statement(Backend::Postgres).unwrap().for_backend(Backend::Postgres).unwrap().sql(),
//!     "SELECT \"id\", \"workspace_id\", \"email\", \"created_at\" FROM \"mail_suppressions\" \
//!      WHERE (\"workspace_id\" IS NULL OR \"workspace_id\" = $1) AND \"email\" = $2 \
//!      ORDER BY \"created_at\" DESC"
//! );
//! ```
//!
//! Why statements and a `macro_rules!` declaration rather than a derive:
//! the crates below navree-orm need a dozen tables between them, all
//! with plain columns and a single key. A declarative macro gives them the
//! one-place column list a derive would, with no proc-macro crate below
//! the ORM and nothing to learn beyond the struct itself; the hand-written
//! [`Record`] impl stays possible (a renamed column, a computed value), and
//! models with relations, scopes and casts belong in navree-orm.

use std::marker::PhantomData;

use serde::Serialize;

use crate::paginate::{Page, PageQuery};
use crate::{
    Backend, ChangeEvent, ChangeOp, DbError, Executor, ExecutorExt, Row, Statement, Uow, Value,
};

/// A table's typed row: its table, its columns (in the order of
/// [`values`](Record::values)), its key, and how a row reads into it.
///
/// Write it with [`record!`](macro@crate::record) (one declaration: the struct
/// and this impl), or by hand.
pub trait Record: Sized + Send + Sync + 'static {
    /// The table (schema-qualified names are quoted part by part).
    const TABLE: &'static str;

    /// The columns, in the order of [`values`](Record::values).
    const COLUMNS: &'static [&'static str];

    /// The primary key column (one of [`COLUMNS`](Record::COLUMNS)).
    const KEY: &'static str = "id";

    /// The record from a row holding (at least) its columns.
    fn from_row(row: &Row) -> Result<Self, DbError>;

    /// The record's values, one per column, in the order of
    /// [`COLUMNS`](Record::COLUMNS).
    fn values(&self) -> Vec<Value>;

    /// The value of the key column.
    fn key(&self) -> Value {
        let at = key_index::<Self>().unwrap_or(0);
        self.values().swap_remove(at)
    }

    /// `SELECT` every column of the table: add filters, ordering and
    /// limits, then run it with [`Select::all`], [`Select::one`],
    /// [`Select::count`] or [`Select::page`].
    fn select() -> Select<Self> {
        Select::new()
    }

    /// `UPDATE` the rows matching `filter`: add columns with
    /// [`Update::set`], then [`Update::execute`].
    fn update_where(filter: Filter) -> Update<Self> {
        Update::new(filter)
    }

    /// `DELETE` the rows matching `filter` ([`Delete::execute`]).
    fn delete_where(filter: Filter) -> Delete<Self> {
        Delete::new(filter)
    }
}

/// Declares a struct and its [`Record`] impl from one list of fields: the
/// column of each field is its name (or the string after `as`), the key is
/// `id` unless `key "..."` names another column. Field types read with
/// [`FromValue`](crate::FromValue) and bind with `Into<Value>`; attributes
/// (derives, serde, docs) pass through.
///
/// ```
/// use chrono::{DateTime, Utc};
/// use navree_db::Record;
///
/// navree_db::record! {
///     /// A role's ability.
///     #[derive(Clone, Debug, serde::Serialize)]
///     pub struct AbilityRow in "access_abilities" key "name" {
///         /// The ability.
///         pub name: String,
///         /// `allow` or `deny`.
///         pub effect: String as "type",
///         pub created_at: DateTime<Utc>,
///     }
/// }
///
/// assert_eq!(AbilityRow::COLUMNS, ["name", "type", "created_at"]);
/// assert_eq!(AbilityRow::KEY, "name");
/// ```
#[macro_export]
macro_rules! record {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident in $table:literal $(key $key:literal)? {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident : $ty:ty $(as $column:literal)?
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $(
                $(#[$field_meta])*
                $field_vis $field: $ty,
            )*
        }

        impl $crate::Record for $name {
            const TABLE: &'static str = $table;
            const COLUMNS: &'static [&'static str] = &[$($crate::record!(@column $field $($column)?)),*];
            $(const KEY: &'static str = $key;)?

            fn from_row(row: &$crate::Row) -> ::core::result::Result<Self, $crate::DbError> {
                ::core::result::Result::Ok(Self {
                    $($field: row.get($crate::record!(@column $field $($column)?))?,)*
                })
            }

            fn values(&self) -> ::std::vec::Vec<$crate::Value> {
                ::std::vec![$($crate::Value::from(::core::clone::Clone::clone(&self.$field))),*]
            }
        }
    };
    (@column $field:ident $column:literal) => {
        $column
    };
    (@column $field:ident) => {
        ::core::stringify!($field)
    };
}

fn key_index<R: Record>() -> Option<usize> {
    R::COLUMNS.iter().position(|column| *column == R::KEY)
}

/// `column` as an error unless it is one of `R`'s.
fn checked<R: Record>(column: &str) -> Result<(), DbError> {
    if R::COLUMNS.contains(&column) {
        Ok(())
    } else {
        Err(DbError::invalid(format!(
            "`{column}` is not a column of `{}` (its columns: {})",
            R::TABLE,
            R::COLUMNS.join(", ")
        )))
    }
}

/// A comparison of [`Filter::compare`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    /// `<>`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

impl Compare {
    fn sql(self) -> &'static str {
        match self {
            Compare::Ne => "<>",
            Compare::Lt => "<",
            Compare::Le => "<=",
            Compare::Gt => ">",
            Compare::Ge => ">=",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Part {
    Eq(&'static str, Value),
    Compare(&'static str, Compare, Value),
    IsNull(&'static str),
    NotNull(&'static str),
    In(&'static str, Vec<Value>),
    Raw(String, Vec<Value>),
    Group(Filter),
}

/// A `WHERE` condition: parts joined with `AND` ([`Filter::all`]) or `OR`
/// ([`Filter::any`]), nested with [`Filter::group`]. An empty `all` matches
/// every row, an empty `any` none.
#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    any: bool,
    parts: Vec<Part>,
}

impl Default for Filter {
    fn default() -> Self {
        Filter::all()
    }
}

impl Filter {
    /// Every part must hold (`AND`).
    pub fn all() -> Filter {
        Filter {
            any: false,
            parts: Vec::new(),
        }
    }

    /// One part must hold (`OR`).
    pub fn any() -> Filter {
        Filter {
            any: true,
            parts: Vec::new(),
        }
    }

    /// `column = value`; a NULL value (`None`) is `column IS NULL`.
    #[must_use]
    pub fn eq(mut self, column: &'static str, value: impl Into<Value>) -> Filter {
        let value = value.into();
        self.parts.push(if value.is_null() {
            Part::IsNull(column)
        } else {
            Part::Eq(column, value)
        });
        self
    }

    /// `column <op> value` (`<>`, `<`, `<=`, `>`, `>=`).
    #[must_use]
    pub fn compare(mut self, column: &'static str, op: Compare, value: impl Into<Value>) -> Filter {
        self.parts.push(Part::Compare(column, op, value.into()));
        self
    }

    /// `column IS NULL`.
    #[must_use]
    pub fn is_null(mut self, column: &'static str) -> Filter {
        self.parts.push(Part::IsNull(column));
        self
    }

    /// `column IS NOT NULL`.
    #[must_use]
    pub fn not_null(mut self, column: &'static str) -> Filter {
        self.parts.push(Part::NotNull(column));
        self
    }

    /// `column IN (values)`; no values matches no row.
    #[must_use]
    pub fn is_in<V: Into<Value>>(
        mut self,
        column: &'static str,
        values: impl IntoIterator<Item = V>,
    ) -> Filter {
        self.parts.push(Part::In(
            column,
            values.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// A portable SQL fragment (`?` placeholders, `??` for a literal `?`)
    /// with its values. Its column names are not checked or quoted.
    #[must_use]
    pub fn raw(
        mut self,
        sql: impl Into<String>,
        values: impl IntoIterator<Item = Value>,
    ) -> Filter {
        self.parts
            .push(Part::Raw(sql.into(), values.into_iter().collect()));
        self
    }

    /// `other`, nested (in parentheses).
    #[must_use]
    pub fn group(mut self, other: Filter) -> Filter {
        self.parts.push(Part::Group(other));
        self
    }

    /// Whether the filter has no parts.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Writes the condition (nothing for an empty `all`).
    fn render<R: Record>(
        &self,
        backend: Backend,
        sql: &mut String,
        values: &mut Vec<Value>,
    ) -> Result<(), DbError> {
        if self.parts.is_empty() {
            if self.any {
                sql.push_str("1 = 0");
            } else {
                sql.push_str("1 = 1");
            }
            return Ok(());
        }
        let joiner = if self.any { " OR " } else { " AND " };
        for (at, part) in self.parts.iter().enumerate() {
            if at > 0 {
                sql.push_str(joiner);
            }
            match part {
                Part::Eq(column, value) => {
                    column_sql::<R>(backend, column, sql)?;
                    sql.push_str(" = ?");
                    values.push(value.clone());
                }
                Part::Compare(column, op, value) => {
                    column_sql::<R>(backend, column, sql)?;
                    sql.push(' ');
                    sql.push_str(op.sql());
                    sql.push_str(" ?");
                    values.push(value.clone());
                }
                Part::IsNull(column) => {
                    column_sql::<R>(backend, column, sql)?;
                    sql.push_str(" IS NULL");
                }
                Part::NotNull(column) => {
                    column_sql::<R>(backend, column, sql)?;
                    sql.push_str(" IS NOT NULL");
                }
                Part::In(column, list) => {
                    checked::<R>(column)?;
                    if list.is_empty() {
                        sql.push_str("1 = 0");
                    } else {
                        sql.push_str(&backend.quote_part(column));
                        sql.push_str(" IN (");
                        sql.push_str(&placeholders(list.len()));
                        sql.push(')');
                        values.extend(list.iter().cloned());
                    }
                }
                Part::Raw(fragment, list) => {
                    sql.push('(');
                    sql.push_str(fragment);
                    sql.push(')');
                    values.extend(list.iter().cloned());
                }
                Part::Group(group) => {
                    sql.push('(');
                    group.render::<R>(backend, sql, values)?;
                    sql.push(')');
                }
            }
        }
        Ok(())
    }

    /// ` WHERE ..`, or nothing for an empty `all`.
    fn render_where<R: Record>(
        &self,
        backend: Backend,
        sql: &mut String,
        values: &mut Vec<Value>,
    ) -> Result<(), DbError> {
        if self.parts.is_empty() && !self.any {
            return Ok(());
        }
        sql.push_str(" WHERE ");
        self.render::<R>(backend, sql, values)
    }
}

fn column_sql<R: Record>(backend: Backend, column: &str, sql: &mut String) -> Result<(), DbError> {
    checked::<R>(column)?;
    sql.push_str(&backend.quote_part(column));
    Ok(())
}

/// `?, ?, ?`
fn placeholders(n: usize) -> String {
    vec!["?"; n].join(", ")
}

fn column_list(backend: Backend, columns: &[&str]) -> String {
    columns
        .iter()
        .map(|column| backend.quote_part(column))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A `SELECT` of a [`Record`]'s columns ([`Record::select`]).
pub struct Select<R> {
    filter: Filter,
    order: Vec<(&'static str, bool)>,
    limit: Option<u64>,
    offset: Option<u64>,
    lock: bool,
    record: PhantomData<fn() -> R>,
}

impl<R> Clone for Select<R> {
    fn clone(&self) -> Self {
        Select {
            filter: self.filter.clone(),
            order: self.order.clone(),
            limit: self.limit,
            offset: self.offset,
            lock: self.lock,
            record: PhantomData,
        }
    }
}

impl<R> std::fmt::Debug for Select<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Select")
            .field("record", &std::any::type_name::<R>())
            .field("filter", &self.filter)
            .field("order", &self.order)
            .field("limit", &self.limit)
            .field("offset", &self.offset)
            .field("lock", &self.lock)
            .finish()
    }
}

impl<R: Record> Default for Select<R> {
    fn default() -> Self {
        Select::new()
    }
}

impl<R: Record> Select<R> {
    /// Every row.
    pub fn new() -> Select<R> {
        Select {
            filter: Filter::all(),
            order: Vec::new(),
            limit: None,
            offset: None,
            lock: false,
            record: PhantomData,
        }
    }

    /// Only the rows matching `filter` too (`AND`).
    #[must_use]
    pub fn filter(mut self, filter: Filter) -> Select<R> {
        self.filter = self.filter.group(filter);
        self
    }

    /// Only the rows whose `column` is `value` (`IS NULL` for `None`).
    #[must_use]
    pub fn eq(mut self, column: &'static str, value: impl Into<Value>) -> Select<R> {
        self.filter = self.filter.eq(column, value);
        self
    }

    /// Ordered by `column`, ascending (after earlier orderings).
    #[must_use]
    pub fn order_by(mut self, column: &'static str) -> Select<R> {
        self.order.push((column, false));
        self
    }

    /// Ordered by `column`, descending (after earlier orderings).
    #[must_use]
    pub fn order_by_desc(mut self, column: &'static str) -> Select<R> {
        self.order.push((column, true));
        self
    }

    /// At most `limit` rows.
    #[must_use]
    pub fn limit(mut self, limit: u64) -> Select<R> {
        self.limit = Some(limit);
        self
    }

    /// Skips the first `offset` rows.
    #[must_use]
    pub fn offset(mut self, offset: u64) -> Select<R> {
        self.offset = Some(offset);
        self
    }

    /// Locks the rows read (`FOR UPDATE`) until the transaction ends, on
    /// PostgreSQL and MySQL; nothing on SQLite, whose units of work begin
    /// `IMMEDIATE` and so serialise writers already.
    #[must_use]
    pub fn for_update(mut self) -> Select<R> {
        self.lock = true;
        self
    }

    /// The statement for `backend`.
    pub fn statement(&self, backend: Backend) -> Result<Statement, DbError> {
        let mut sql = String::from("SELECT ");
        sql.push_str(&column_list(backend, R::COLUMNS));
        sql.push_str(" FROM ");
        sql.push_str(&backend.quote_identifier(R::TABLE));
        let mut values = Vec::new();
        self.filter
            .render_where::<R>(backend, &mut sql, &mut values)?;
        if !self.order.is_empty() {
            sql.push_str(" ORDER BY ");
            for (at, (column, desc)) in self.order.iter().enumerate() {
                if at > 0 {
                    sql.push_str(", ");
                }
                column_sql::<R>(backend, column, &mut sql)?;
                if *desc {
                    sql.push_str(" DESC");
                }
            }
        }
        match (self.limit, self.offset) {
            (Some(limit), _) => sql.push_str(&format!(" LIMIT {limit}")),
            // MySQL and SQLite take an offset only after a limit.
            (None, Some(_)) => match backend {
                Backend::MySql => sql.push_str(" LIMIT 18446744073709551615"),
                Backend::Sqlite => sql.push_str(" LIMIT -1"),
                Backend::Postgres => {}
            },
            (None, None) => {}
        }
        if let Some(offset) = self.offset {
            sql.push_str(&format!(" OFFSET {offset}"));
        }
        if self.lock && backend.supports_row_locks() {
            sql.push_str(" FOR UPDATE");
        }
        Ok(Statement::portable(sql, values))
    }

    /// Every matching row.
    pub async fn all<E: Executor + ?Sized>(&self, exec: &E) -> Result<Vec<R>, DbError> {
        let statement = self.statement(exec.backend())?;
        exec.fetch_all(&statement)
            .await?
            .iter()
            .map(R::from_row)
            .collect()
    }

    /// The first matching row (`LIMIT 1`), if any.
    pub async fn one<E: Executor + ?Sized>(&self, exec: &E) -> Result<Option<R>, DbError> {
        let mut select = self.clone();
        select.limit = Some(1);
        let statement = select.statement(exec.backend())?;
        exec.fetch_optional(&statement)
            .await?
            .as_ref()
            .map(R::from_row)
            .transpose()
    }

    /// How many rows match (ordering, limits and locks are ignored).
    pub async fn count<E: Executor + ?Sized>(&self, exec: &E) -> Result<u64, DbError> {
        let backend = exec.backend();
        let mut sql = String::from("SELECT COUNT(*) FROM ");
        sql.push_str(&backend.quote_identifier(R::TABLE));
        let mut values = Vec::new();
        self.filter
            .render_where::<R>(backend, &mut sql, &mut values)?;
        exec.fetch_scalar::<u64>(&Statement::portable(sql, values))
            .await
    }

    /// Page `query` of the matching rows, with the total (`{"data",
    /// "meta"}`). Order the select first: pages of an unordered select
    /// may overlap.
    pub async fn page<E: Executor + ?Sized>(
        &self,
        exec: &E,
        query: PageQuery,
    ) -> Result<Page<R>, DbError> {
        let query = PageQuery::new(query.page, query.per_page);
        let total = self.count(exec).await?;
        // A page past the end (any page number, however large) is empty and
        // reads no rows: its offset may overflow, or exceed what a bound
        // integer holds.
        let past_the_end = (query.page - 1)
            .checked_mul(query.per_page)
            .is_none_or(|offset| offset >= total);
        let rows = if past_the_end {
            Vec::new()
        } else {
            self.clone()
                .limit(query.per_page)
                .offset(query.offset())
                .all(exec)
                .await?
        };
        Ok(Page::new(rows, query, total))
    }
}

/// An `UPDATE` of the rows matching a filter ([`Record::update_where`]).
pub struct Update<R> {
    filter: Filter,
    sets: Vec<(&'static str, Value)>,
    record: PhantomData<fn() -> R>,
}

impl<R> Clone for Update<R> {
    fn clone(&self) -> Self {
        Update {
            filter: self.filter.clone(),
            sets: self.sets.clone(),
            record: PhantomData,
        }
    }
}

impl<R> std::fmt::Debug for Update<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Update")
            .field("record", &std::any::type_name::<R>())
            .field("filter", &self.filter)
            .field("sets", &self.sets)
            .finish()
    }
}

impl<R: Record> Update<R> {
    fn new(filter: Filter) -> Update<R> {
        Update {
            filter,
            sets: Vec::new(),
            record: PhantomData,
        }
    }

    /// Sets `column` to `value`.
    #[must_use]
    pub fn set(mut self, column: &'static str, value: impl Into<Value>) -> Update<R> {
        self.sets.push((column, value.into()));
        self
    }

    /// The statement for `backend`.
    pub fn statement(&self, backend: Backend) -> Result<Statement, DbError> {
        if self.sets.is_empty() {
            return Err(DbError::invalid(format!(
                "an update of `{}` sets no column",
                R::TABLE
            )));
        }
        let mut sql = String::from("UPDATE ");
        sql.push_str(&backend.quote_identifier(R::TABLE));
        sql.push_str(" SET ");
        let mut values = Vec::new();
        for (at, (column, value)) in self.sets.iter().enumerate() {
            if at > 0 {
                sql.push_str(", ");
            }
            column_sql::<R>(backend, column, &mut sql)?;
            sql.push_str(" = ?");
            values.push(value.clone());
        }
        self.filter
            .render_where::<R>(backend, &mut sql, &mut values)?;
        Ok(Statement::portable(sql, values))
    }

    /// Runs it; how many rows it changed.
    pub async fn execute<E: Executor + ?Sized>(&self, exec: &E) -> Result<u64, DbError> {
        let statement = self.statement(exec.backend())?;
        Ok(exec.execute(&statement).await?.rows_affected)
    }
}

/// A `DELETE` of the rows matching a filter ([`Record::delete_where`]).
pub struct Delete<R> {
    filter: Filter,
    record: PhantomData<fn() -> R>,
}

impl<R> Clone for Delete<R> {
    fn clone(&self) -> Self {
        Delete {
            filter: self.filter.clone(),
            record: PhantomData,
        }
    }
}

impl<R> std::fmt::Debug for Delete<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Delete")
            .field("record", &std::any::type_name::<R>())
            .field("filter", &self.filter)
            .finish()
    }
}

impl<R: Record> Delete<R> {
    fn new(filter: Filter) -> Delete<R> {
        Delete {
            filter,
            record: PhantomData,
        }
    }

    /// The statement for `backend`.
    pub fn statement(&self, backend: Backend) -> Result<Statement, DbError> {
        let mut sql = String::from("DELETE FROM ");
        sql.push_str(&backend.quote_identifier(R::TABLE));
        let mut values = Vec::new();
        self.filter
            .render_where::<R>(backend, &mut sql, &mut values)?;
        Ok(Statement::portable(sql, values))
    }

    /// Runs it; how many rows it deleted.
    pub async fn execute<E: Executor + ?Sized>(&self, exec: &E) -> Result<u64, DbError> {
        let statement = self.statement(exec.backend())?;
        Ok(exec.execute(&statement).await?.rows_affected)
    }
}

/// The record whose key is `key`.
pub async fn find<R: Record, E: Executor + ?Sized>(
    exec: &E,
    key: impl Into<Value>,
) -> Result<Option<R>, DbError> {
    R::select().eq(R::KEY, key).one(exec).await
}

/// `INSERT` of every column of `record`.
pub fn insert_statement<R: Record>(backend: Backend, record: &R) -> Statement {
    let mut sql = String::from("INSERT INTO ");
    sql.push_str(&backend.quote_identifier(R::TABLE));
    sql.push_str(" (");
    sql.push_str(&column_list(backend, R::COLUMNS));
    sql.push_str(") VALUES (");
    sql.push_str(&placeholders(R::COLUMNS.len()));
    sql.push(')');
    Statement::portable(sql, record.values())
}

/// Inserts `record` (every column).
pub async fn insert<R: Record, E: Executor + ?Sized>(exec: &E, record: &R) -> Result<(), DbError> {
    exec.execute(&insert_statement(exec.backend(), record))
        .await?;
    Ok(())
}

/// Writes every column of `record` but its key to the row with its key;
/// how many rows changed (0: there is no such row).
pub async fn update<R: Record, E: Executor + ?Sized>(exec: &E, record: &R) -> Result<u64, DbError> {
    let mut changes = R::update_where(Filter::all().eq(R::KEY, record.key()));
    for (column, value) in R::COLUMNS.iter().zip(record.values()) {
        if *column != R::KEY {
            changes = changes.set(column, value);
        }
    }
    changes.execute(exec).await
}

/// `INSERT` of `record`, or, when a row with the same `conflict` columns
/// exists (a unique key), an update of its `update` columns to `record`'s
/// values (nothing for none).
///
/// PostgreSQL and SQLite: `ON CONFLICT (conflict) DO UPDATE SET c =
/// excluded.c` (`DO NOTHING`). MySQL: `ON DUPLICATE KEY UPDATE c =
/// VALUES(c)`, which reacts to any unique key of the table, not only
/// `conflict`'s.
pub fn upsert_statement<R: Record>(
    backend: Backend,
    record: &R,
    conflict: &[&'static str],
    update: &[&'static str],
) -> Result<Statement, DbError> {
    let Some(first) = conflict.first() else {
        return Err(DbError::invalid(format!(
            "an upsert of `{}` names no conflict column",
            R::TABLE
        )));
    };
    for column in conflict.iter().chain(update) {
        checked::<R>(column)?;
    }
    let mut sql = insert_statement(backend, record).sql().to_owned();
    match backend {
        Backend::MySql => {
            sql.push_str(" ON DUPLICATE KEY UPDATE ");
            if update.is_empty() {
                let column = backend.quote_part(first);
                sql.push_str(&format!("{column} = {column}"));
            } else {
                let sets: Vec<String> = update
                    .iter()
                    .map(|column| {
                        let column = backend.quote_part(column);
                        format!("{column} = VALUES({column})")
                    })
                    .collect();
                sql.push_str(&sets.join(", "));
            }
        }
        Backend::Postgres | Backend::Sqlite => {
            sql.push_str(" ON CONFLICT (");
            sql.push_str(&column_list(backend, conflict));
            sql.push(')');
            if update.is_empty() {
                sql.push_str(" DO NOTHING");
            } else {
                sql.push_str(" DO UPDATE SET ");
                let sets: Vec<String> = update
                    .iter()
                    .map(|column| {
                        let column = backend.quote_part(column);
                        format!("{column} = excluded.{column}")
                    })
                    .collect();
                sql.push_str(&sets.join(", "));
            }
        }
    }
    Ok(Statement::portable(sql, record.values()))
}

/// Runs [`upsert_statement`].
pub async fn upsert<R: Record, E: Executor + ?Sized>(
    exec: &E,
    record: &R,
    conflict: &[&'static str],
    update: &[&'static str],
) -> Result<(), DbError> {
    exec.execute(&upsert_statement(exec.backend(), record, conflict, update)?)
        .await?;
    Ok(())
}

/// Deletes the row with `record`'s key; how many rows were deleted.
pub async fn delete<R: Record, E: Executor + ?Sized>(exec: &E, record: &R) -> Result<u64, DbError> {
    R::delete_where(Filter::all().eq(R::KEY, record.key()))
        .execute(exec)
        .await
}

/// The key as change events name it: text as is, anything else as JSON.
fn key_string(value: &Value) -> String {
    match value.to_json() {
        serde_json::Value::String(text) => text,
        other => other.to_string(),
    }
}

fn json<T: Serialize>(value: &T) -> Result<serde_json::Value, DbError> {
    serde_json::to_value(value)
        .map_err(|error| DbError::invalid(format!("a record does not serialize: {error}")))
}

/// The row with `record`'s key as stored, or `NotFound`.
async fn stored<R: Record, E: Executor + ?Sized>(exec: &E, record: &R) -> Result<R, DbError> {
    let key = record.key();
    find::<R, _>(exec, key.clone())
        .await?
        .ok_or_else(|| DbError::not_found(format!("{} {}", R::TABLE, key_string(&key))))
}

/// [`insert`], then the row as the database stored it: its rounding of
/// timestamps (microseconds on PostgreSQL, the column's precision on
/// MySQL) and decimals. Read back by key on the same executor, so through
/// a unit of work it is read in its transaction.
pub async fn insert_returning<R: Record, E: Executor + ?Sized>(
    exec: &E,
    record: &R,
) -> Result<R, DbError> {
    insert(exec, record).await?;
    stored(exec, record).await
}

/// [`update`], then the row as the database stored it; `NotFound` when no
/// row has `record`'s key.
pub async fn update_returning<R: Record, E: Executor + ?Sized>(
    exec: &E,
    record: &R,
) -> Result<R, DbError> {
    update(exec, record).await?;
    stored(exec, record).await
}

/// [`insert_returning`] through `uow`, recording a `created` change event
/// with the stored record as JSON (its `Serialize` form: fields skipped
/// there, such as secrets, stay out of audit trails).
pub async fn insert_recorded<R: Record + Serialize + Clone>(
    uow: &Uow,
    record: &R,
) -> Result<R, DbError> {
    let created = insert_returning(uow, record).await?;
    let event = created.clone();
    uow.record_lazily(move || {
        Ok(Some(ChangeEvent {
            entity: R::TABLE.to_owned(),
            id: key_string(&event.key()),
            op: ChangeOp::Created,
            old: None,
            new: Some(json(&event)?),
        }))
    })?;
    Ok(created)
}

/// [`update_returning`] of `new` through `uow`, recording an `updated`
/// change event with the fields that differ from `old` (none: no event);
/// `NotFound` when no row has its key.
pub async fn update_recorded<R: Record + Serialize + Clone>(
    uow: &Uow,
    old: &R,
    new: &R,
) -> Result<R, DbError> {
    let updated = update_returning(uow, new).await?;
    let (old, new) = (old.clone(), updated.clone());
    uow.record_lazily(move || {
        let (before, after) = diff(json(&old)?, json(&new)?);
        Ok((!after.is_empty()).then(|| ChangeEvent {
            entity: R::TABLE.to_owned(),
            id: key_string(&new.key()),
            op: ChangeOp::Updated,
            old: Some(before.into()),
            new: Some(after.into()),
        }))
    })?;
    Ok(updated)
}

/// [`delete`] through `uow`, recording a `deleted` change event with the
/// old record; `NotFound` when no row was deleted.
pub async fn delete_recorded<R: Record + Serialize + Clone>(
    uow: &Uow,
    record: &R,
) -> Result<(), DbError> {
    if delete(uow, record).await? == 0 {
        return Err(DbError::not_found(format!(
            "{} {}",
            R::TABLE,
            key_string(&record.key())
        )));
    }
    let deleted = record.clone();
    uow.record_lazily(move || {
        Ok(Some(ChangeEvent {
            entity: R::TABLE.to_owned(),
            id: key_string(&deleted.key()),
            op: ChangeOp::Deleted,
            old: Some(json(&deleted)?),
            new: None,
        }))
    })
}

type Fields = serde_json::Map<String, serde_json::Value>;

/// The fields of `new` whose values differ from `old`: (old values, new
/// values).
fn diff(old: serde_json::Value, new: serde_json::Value) -> (Fields, Fields) {
    let (serde_json::Value::Object(old), serde_json::Value::Object(new)) = (old, new) else {
        return (Fields::new(), Fields::new());
    };
    let mut before = Fields::new();
    let mut after = Fields::new();
    for (key, value) in new {
        let previous = old.get(&key).cloned().unwrap_or(serde_json::Value::Null);
        if previous != value {
            before.insert(key.clone(), previous);
            after.insert(key, value);
        }
    }
    (before, after)
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    use super::*;

    crate::record! {
        #[derive(Clone, Debug, PartialEq, serde::Serialize)]
        struct Deal in "crm.deals" {
            id: Uuid,
            title: String,
            owner_id: Option<Uuid>,
            kind: String as "type",
            #[serde(skip_serializing)]
            secret: String,
            created_at: DateTime<Utc>,
        }
    }

    fn sql(statement: Result<Statement, DbError>, backend: Backend) -> String {
        statement
            .unwrap()
            .for_backend(backend)
            .unwrap()
            .sql()
            .to_owned()
    }

    #[test]
    fn the_declaration_lists_the_columns_once() {
        assert_eq!(
            Deal::COLUMNS,
            ["id", "title", "owner_id", "type", "secret", "created_at"]
        );
        assert_eq!(Deal::KEY, "id");
        let deal = Deal {
            id: Uuid::nil(),
            title: "Acme".into(),
            owner_id: None,
            kind: "new".into(),
            secret: "s".into(),
            created_at: DateTime::<Utc>::UNIX_EPOCH,
        };
        assert_eq!(deal.key(), Value::Uuid(Uuid::nil()));
        assert_eq!(deal.values().len(), 6);
        assert!(deal.values()[2].is_null());
        let row = Row::from_values(Deal::COLUMNS.iter().copied().zip(deal.values()));
        assert_eq!(Deal::from_row(&row).unwrap(), deal);
    }

    #[test]
    fn selects_quote_check_and_bind() {
        let select = Deal::select()
            .filter(
                Filter::any()
                    .is_null("owner_id")
                    .eq("owner_id", Uuid::nil())
                    .group(
                        Filter::all()
                            .eq("type", "new")
                            .compare("title", Compare::Ne, "x"),
                    ),
            )
            .filter(
                Filter::all()
                    .is_in("title", ["a", "b"])
                    .is_in::<&str>("type", []),
            )
            .order_by_desc("created_at")
            .order_by("id")
            .offset(20)
            .for_update();
        assert_eq!(
            sql(select.statement(Backend::MySql), Backend::MySql),
            "SELECT `id`, `title`, `owner_id`, `type`, `secret`, `created_at` FROM `crm`.`deals` \
             WHERE (`owner_id` IS NULL OR `owner_id` = ? OR (`type` = ? AND `title` <> ?)) \
             AND (`title` IN (?, ?) AND 1 = 0) ORDER BY `created_at` DESC, `id` \
             LIMIT 18446744073709551615 OFFSET 20 FOR UPDATE"
        );
        assert_eq!(
            sql(
                Deal::select()
                    .limit(5)
                    .for_update()
                    .statement(Backend::Sqlite),
                Backend::Sqlite
            ),
            "SELECT \"id\", \"title\", \"owner_id\", \"type\", \"secret\", \"created_at\" \
             FROM \"crm\".\"deals\" LIMIT 5"
        );
        let typo = Deal::select().eq("titel", "x").statement(Backend::Postgres);
        assert_eq!(typo.unwrap_err().kind(), crate::ErrorKind::InvalidStatement);
        assert_eq!(
            sql(
                Deal::select()
                    .filter(Filter::any())
                    .statement(Backend::Postgres),
                Backend::Postgres
            ),
            "SELECT \"id\", \"title\", \"owner_id\", \"type\", \"secret\", \"created_at\" \
             FROM \"crm\".\"deals\" WHERE (1 = 0)"
        );
    }

    #[test]
    fn writes_are_built_from_the_columns() {
        let deal = Deal {
            id: Uuid::nil(),
            title: "Acme".into(),
            owner_id: None,
            kind: "new".into(),
            secret: "s".into(),
            created_at: DateTime::<Utc>::UNIX_EPOCH,
        };
        assert_eq!(
            insert_statement(Backend::Postgres, &deal)
                .for_backend(Backend::Postgres)
                .unwrap()
                .sql(),
            "INSERT INTO \"crm\".\"deals\" (\"id\", \"title\", \"owner_id\", \"type\", \"secret\", \"created_at\") \
             VALUES ($1, $2, $3, $4, $5, $6)"
        );
        assert_eq!(
            sql(
                upsert_statement(Backend::Sqlite, &deal, &["id"], &["title", "type"]),
                Backend::Sqlite
            ),
            "INSERT INTO \"crm\".\"deals\" (\"id\", \"title\", \"owner_id\", \"type\", \"secret\", \"created_at\") \
             VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (\"id\") DO UPDATE SET \"title\" = excluded.\"title\", \
             \"type\" = excluded.\"type\""
        );
        assert_eq!(
            sql(
                upsert_statement(Backend::MySql, &deal, &["id"], &[]),
                Backend::MySql
            ),
            "INSERT INTO `crm`.`deals` (`id`, `title`, `owner_id`, `type`, `secret`, `created_at`) \
             VALUES (?, ?, ?, ?, ?, ?) ON DUPLICATE KEY UPDATE `id` = `id`"
        );
        assert!(upsert_statement(Backend::MySql, &deal, &[], &[]).is_err());
        assert_eq!(
            sql(
                Deal::update_where(Filter::all().eq("id", Uuid::nil()).eq("type", "new"))
                    .set("type", "won")
                    .set("owner_id", None::<Uuid>)
                    .statement(Backend::Postgres),
                Backend::Postgres
            ),
            "UPDATE \"crm\".\"deals\" SET \"type\" = $1, \"owner_id\" = $2 WHERE \"id\" = $3 AND \"type\" = $4"
        );
        assert!(
            Deal::update_where(Filter::all())
                .statement(Backend::Postgres)
                .is_err()
        );
        assert_eq!(
            sql(
                Deal::delete_where(Filter::all().raw("created_at < ?", [Value::from(1_i64)]))
                    .statement(Backend::Sqlite),
                Backend::Sqlite
            ),
            "DELETE FROM \"crm\".\"deals\" WHERE (created_at < ?)"
        );
    }

    #[test]
    fn change_events_diff_the_serialized_records() {
        let (old, new) = diff(
            serde_json::json!({"a": 1, "b": 2}),
            serde_json::json!({"a": 1, "b": 3, "c": true}),
        );
        assert_eq!(
            serde_json::Value::from(old),
            serde_json::json!({"b": 2, "c": null})
        );
        assert_eq!(
            serde_json::Value::from(new),
            serde_json::json!({"b": 3, "c": true})
        );
        assert_eq!(key_string(&Value::from(7_i64)), "7");
        assert_eq!(key_string(&Value::from("k")), "k");
    }
}
