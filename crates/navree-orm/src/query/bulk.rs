//! Bulk writes: multi-row inserts, inserts that skip conflicting rows or
//! read a generated key back, `INSERT … SELECT`, upserts,
//! update-or-insert, `UPDATE … FROM` and truncation, on table queries
//! ([`TableBuilder`]) and on models ([`Builder`]).
//!
//! | Method | Writes | Returns |
//! |---|---|---|
//! | `insert` | `INSERT INTO t (..) VALUES (..), (..)` | rows inserted |
//! | `insert_get_id` | one row, reading its key back | the key |
//! | `insert_or_ignore` | the rows that conflict with no unique index | rows inserted |
//! | `insert_or_ignore_returning` | the same, reading columns of the inserted rows back | rows |
//! | `insert_using`, `insert_or_ignore_using` | `INSERT INTO t (..) SELECT ..` | rows inserted |
//! | `upsert` | new rows, and an update of the rows they conflict with | rows the database reports |
//! | `update_or_insert` | an update of the matching rows, or a new row | whether a row was written |
//! | `update_from` | `UPDATE t SET .. FROM joined ..` | rows updated |
//! | `truncate` | every row, and the identity sequence | — |
//! | models: `fill_and_insert` and friends | models with generated keys and timestamps, like `save` | as above |
//!
//! None of them runs lifecycle hooks or records change events: they write
//! rows, not records (save records one by one when hooks or the audit trail
//! matter). A multi-row insert is one statement, split into several when
//! its values exceed the database's bind limit (65,535 values on
//! PostgreSQL and MySQL, 32,766 on SQLite); the parts then run in one
//! transaction (a savepoint inside a unit of work), so the insert stays
//! all or nothing.
//!
//! Skipping conflicts is `ON CONFLICT DO NOTHING` on PostgreSQL and SQLite
//! (only rows that conflict on a unique index are skipped; any other error
//! fails the statement) and `INSERT IGNORE` on MySQL, which also turns
//! other errors, such as a value too long for its column, into warnings.
//! An upsert is `ON CONFLICT (unique columns) DO UPDATE` on PostgreSQL and
//! SQLite and `ON DUPLICATE KEY UPDATE` on MySQL (which checks every unique
//! index, whatever columns are named).
//!
//! ```no_run
//! # use navree_orm::__doc_models::*;
//! use navree_orm::ConflictUpdate;
//!
//! # async fn demo(db: Db, uow: Uow) -> Result<()> {
//! // Tags by name: new ones are inserted, existing ones get the new color.
//! let tags = vec![
//!     Tag { name: "vip".into(), color: "gold".into(), ..Default::default() },
//!     Tag { name: "churn-risk".into(), color: "red".into(), ..Default::default() },
//! ];
//! Tag::upsert(&uow, tags, [Tag::NAME], [Tag::COLOR]).await?;
//!
//! // Pivot rows, skipping the pairs that exist.
//! table("deal_tag")
//!     .insert_or_ignore(&uow, [[("deal_id", 1), ("tag_id", 2)], [("deal_id", 1), ("tag_id", 3)]])
//!     .await?;
//!
//! // Every column but the key and created_at on conflict.
//! let settings = vec![Setting { key: "theme".into(), value: "dark".into(), ..Default::default() }];
//! Setting::upsert(&db, settings, [Setting::KEY], [ConflictUpdate::All]).await?;
//! # Ok(()) }
//! ```

use std::borrow::Cow;

use chrono::{DateTime, Utc};

use crate::column::{Assignment, Column, ColumnName, IntoQuery};
use crate::driver::InsertOutcome;
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor, SqlTarget, Target};
use crate::ir::{DeleteIr, Expr, InsertIr, OnConflict, TableRef};
use crate::model::{KeyKind, Model, PrimaryKey};
use crate::query::{Builder, TableBuilder};
use crate::row::{Row, decode_error};
use crate::sql::{Dialect, Statement, render_truncate, render_update_from};
use crate::value::{Field, Value};

/// What an [`upsert`](TableBuilder::upsert) sets on a row that already
/// exists: a column (set to the value the row would have been inserted
/// with), an assignment, or every inserted column.
///
/// ```
/// use navree_orm::{ConflictUpdate, Expr, col};
///
/// let updates: Vec<ConflictUpdate> = vec![
///     "color".into(),
///     // Add to the stored count instead of replacing it.
///     col("hits").set_expr(Expr::col("tags.hits").add(Expr::excluded("hits"))).into(),
/// ];
/// # let _ = updates;
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ConflictUpdate {
    /// Every inserted column but the unique ones (and, for a model, its key
    /// and creation timestamp).
    All,
    /// The column, set to the value the row would have been inserted with.
    Column(Cow<'static, str>),
    /// An assignment; [`Expr::excluded`] is the value the row would have
    /// been inserted with.
    Set(Assignment),
}

impl From<&str> for ConflictUpdate {
    fn from(column: &str) -> Self {
        ConflictUpdate::Column(Cow::Owned(column.to_owned()))
    }
}

impl From<String> for ConflictUpdate {
    fn from(column: String) -> Self {
        ConflictUpdate::Column(Cow::Owned(column))
    }
}

impl<M, T> From<Column<M, T>> for ConflictUpdate {
    fn from(column: Column<M, T>) -> Self {
        ConflictUpdate::Column(Cow::Owned(column.column_name().into_owned()))
    }
}

impl<M, T> From<&Column<M, T>> for ConflictUpdate {
    fn from(column: &Column<M, T>) -> Self {
        ConflictUpdate::Column(Cow::Owned(column.column_name().into_owned()))
    }
}

impl From<Assignment> for ConflictUpdate {
    fn from(assignment: Assignment) -> Self {
        ConflictUpdate::Set(assignment)
    }
}

// ---------------------------------------------------------------------------
// Rows and statements
// ---------------------------------------------------------------------------

/// Rows to insert: the first row's columns, and every row's values in that
/// order.
struct Rows {
    columns: Vec<Cow<'static, str>>,
    values: Vec<Vec<Expr>>,
}

impl Rows {
    /// `rows` as `(column, value)` pairs, every row with the same columns
    /// (in any order).
    fn collect<R, C, V>(table: &str, rows: impl IntoIterator<Item = R>) -> Result<Rows>
    where
        R: IntoIterator<Item = (C, V)>,
        C: Into<Cow<'static, str>>,
        V: Into<Value>,
    {
        let mut columns: Vec<Cow<'static, str>> = Vec::new();
        let mut values: Vec<Vec<Expr>> = Vec::new();
        for (index, row) in rows.into_iter().enumerate() {
            let pairs: Vec<(Cow<'static, str>, Value)> = row
                .into_iter()
                .map(|(column, value)| (column.into(), value.into()))
                .collect();
            if index == 0 {
                columns = pairs.iter().map(|(column, _)| column.clone()).collect();
            }
            if pairs.len() != columns.len() {
                return Err(OrmError::invalid(format!(
                    "row {} of an insert into `{table}` has {} columns, the first has {}",
                    index + 1,
                    pairs.len(),
                    columns.len()
                )));
            }
            let mut ordered = Vec::with_capacity(columns.len());
            for column in &columns {
                let value = pairs
                    .iter()
                    .find(|(name, _)| name == column)
                    .map(|(_, value)| value.clone())
                    .ok_or_else(|| {
                        OrmError::invalid(format!(
                            "row {} of an insert into `{table}` has no `{column}` column",
                            index + 1
                        ))
                    })?;
                ordered.push(Expr::Value(value));
            }
            values.push(ordered);
        }
        Ok(Rows { columns, values })
    }
}

/// How many values `expr` binds.
fn bound_values(expr: &Expr) -> usize {
    match expr {
        Expr::Value(_) => 1,
        Expr::Raw { values, .. } => values.len(),
        Expr::Function { args, .. } => args.iter().map(bound_values).sum(),
        Expr::Arithmetic { left, right, .. } => bound_values(left) + bound_values(right),
        Expr::Aggregate { arg, .. } => arg.as_deref().map_or(0, bound_values),
        Expr::Column(_) | Expr::All { .. } | Expr::Excluded { .. } => 0,
        _ => 1,
    }
}

/// `insert` as statements that each bind fewer values than `dialect`
/// allows: its rows in consecutive parts.
fn split(mut insert: InsertIr, dialect: Option<Dialect>) -> Vec<InsertIr> {
    if insert.select.is_some() || insert.rows.len() <= 1 {
        return vec![insert];
    }
    if insert.columns.is_empty() {
        // `DEFAULT VALUES` inserts one row per statement.
        let count = insert.rows.len();
        insert.rows = vec![Vec::new()];
        return vec![insert; count];
    }
    let Some(dialect) = dialect else {
        return vec![insert];
    };
    let reserved: usize = match &insert.on_conflict {
        Some(OnConflict::Update { sets, .. }) => {
            sets.iter().map(|(_, expr)| bound_values(expr)).sum()
        }
        _ => 0,
    };
    let per_row = insert
        .rows
        .iter()
        .map(|row| row.iter().map(bound_values).sum::<usize>())
        .max()
        .unwrap_or(0)
        .max(1);
    let budget = dialect.max_bind_values().saturating_sub(reserved);
    let rows_per_statement = (budget / per_row).max(1);
    if insert.rows.len() <= rows_per_statement {
        return vec![insert];
    }
    let rows = std::mem::take(&mut insert.rows);
    rows.chunks(rows_per_statement)
        .map(|part| InsertIr {
            rows: part.to_vec(),
            ..insert.clone()
        })
        .collect()
}

/// Runs `inserts` on `target` as one whole: alone when there is one, else
/// in a transaction (a savepoint inside one).
async fn run_inserts(target: &Target<'_>, inserts: Vec<InsertIr>) -> Result<InsertOutcome> {
    if inserts.len() <= 1 {
        return match inserts.first() {
            Some(insert) => target.insert(insert).await,
            None => Ok(InsertOutcome::default()),
        };
    }
    match target.begin_atomic().await? {
        Some(tx) => {
            let nested = target.on_transaction(&tx);
            let result = run_each(&nested, &inserts).await;
            drop(nested);
            finish(tx, result).await
        }
        None => run_each(target, &inserts).await,
    }
}

async fn run_each(target: &Target<'_>, inserts: &[InsertIr]) -> Result<InsertOutcome> {
    let mut total = InsertOutcome::default();
    for insert in inserts {
        let outcome = target.insert(insert).await?;
        total.rows_affected += outcome.rows_affected;
        total.returned.extend(outcome.returned);
        if total.last_insert_id.is_none() {
            total.last_insert_id = outcome.last_insert_id;
        }
    }
    Ok(total)
}

/// Commits `tx` after `result`, or rolls it back.
async fn finish<T>(tx: crate::exec::OwnTransaction, result: Result<T>) -> Result<T> {
    match result {
        Ok(value) => {
            tx.commit().await?;
            Ok(value)
        }
        Err(error) => {
            if let Err(rollback) = tx.rollback().await {
                tracing::warn!(target: "navree_orm", error = %rollback, "rolling back a bulk write failed");
            }
            Err(error)
        }
    }
}

/// The `SET` list of an upsert of rows with `columns`, conflicting on
/// `unique_by`: `update` resolved, later items replacing earlier ones for
/// the same column. [`ConflictUpdate::All`] leaves out the unique columns
/// and `skipped` ones.
fn conflict_sets(
    columns: &[Cow<'static, str>],
    unique_by: &[Cow<'static, str>],
    skipped: &[&str],
    update: Vec<ConflictUpdate>,
) -> Result<Vec<(Cow<'static, str>, Expr)>> {
    let mut sets: Vec<(Cow<'static, str>, Expr)> = Vec::new();
    let mut put = |column: Cow<'static, str>, expr: Expr| match sets
        .iter_mut()
        .find(|(existing, _)| *existing == column)
    {
        Some(existing) => existing.1 = expr,
        None => sets.push((column, expr)),
    };
    for item in update {
        match item {
            ConflictUpdate::All => {
                for column in columns {
                    if !unique_by.contains(column) && !skipped.contains(&column.as_ref()) {
                        put(column.clone(), Expr::excluded(column.clone()));
                    }
                }
            }
            ConflictUpdate::Column(column) => put(column.clone(), Expr::excluded(column)),
            ConflictUpdate::Set(assignment) => {
                let (column, expr) = assignment.into_set().map_err(OrmError::invalid)?;
                put(column, expr);
            }
        }
    }
    Ok(sets)
}

/// The insert handling conflicts on `unique_by` with `sets` (skipping the
/// conflicting rows when there is nothing to set).
fn upsert_conflict(
    unique_by: Vec<Cow<'static, str>>,
    sets: Vec<(Cow<'static, str>, Expr)>,
) -> OnConflict {
    if sets.is_empty() {
        OnConflict::Ignore { target: unique_by }
    } else {
        OnConflict::Update {
            target: unique_by,
            sets,
        }
    }
}

/// The SQL target of `target`, or the error a document store answers
/// `feature` with.
fn sql_only<'t, 'a>(target: &'t Target<'a>, feature: &str) -> Result<&'t SqlTarget<'a>> {
    match target {
        Target::Sql(sql) => Ok(sql),
        Target::Document(_) => Err(OrmError::unsupported(
            feature.to_owned(),
            target.context().database(),
        )),
    }
}

/// Reads key column `column` of an insert's outcome as `K`.
fn returned_key<K: Field>(outcome: &InsertOutcome, table: &str, column: &str) -> Result<K> {
    if let Some(row) = outcome.returned.first() {
        return row
            .field::<K>(column)
            .map_err(|error| decode_error(table.to_owned(), column, K::type_name(), &error));
    }
    match outcome.last_insert_id {
        Some(id) if outcome.rows_affected > 0 => K::from_value(Value::U64(id))
            .map_err(|error| decode_error(table.to_owned(), column, K::type_name(), &error)),
        _ => Err(OrmError::invalid(format!(
            "the database did not return the `{column}` of the row inserted into `{table}`"
        ))),
    }
}

/// Empties `table` on `target` (see [`render_truncate`]).
async fn truncate_table(target: &Target<'_>, table: &str, cascade: bool) -> Result<()> {
    let sql = match target {
        Target::Sql(sql) => sql,
        Target::Document(_) => {
            target
                .delete(&DeleteIr {
                    table: TableRef::new(table.to_owned()),
                    wheres: Vec::new(),
                })
                .await?;
            return Ok(());
        }
    };
    let dialect = sql.dialect();
    let statements = render_truncate(table, cascade, dialect);
    if dialect != Dialect::Sqlite {
        for statement in &statements {
            sql.execute(statement).await?;
        }
        return Ok(());
    }
    // SQLite keeps AUTOINCREMENT counters in `sqlite_sequence`, which exists
    // once a table declares one.
    let has_sequences = sql
        .query_one(&Statement {
            sql: "SELECT 1 AS present FROM sqlite_master WHERE type = 'table' AND name = 'sqlite_sequence'"
                .to_owned(),
            values: Vec::new(),
            dialect,
        })
        .await?
        .is_some();
    let statements: Vec<&Statement> = statements
        .iter()
        .filter(|statement| has_sequences || !statement.sql.contains("sqlite_sequence"))
        .collect();
    match target.begin_atomic().await? {
        Some(tx) => {
            let nested = target.on_transaction(&tx);
            let result = async {
                let nested_sql = sql_only(&nested, "truncate")?;
                for statement in &statements {
                    nested_sql.execute(statement).await?;
                }
                Ok(())
            }
            .await;
            drop(nested);
            finish(tx, result).await
        }
        None => {
            for statement in statements {
                sql.execute(statement).await?;
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Table queries
// ---------------------------------------------------------------------------

impl TableBuilder {
    /// The table an insert writes to (a subquery cannot be written).
    fn insert_table(&self, what: &str) -> Result<Cow<'static, str>> {
        let ir = self.ir();
        ir.check()?;
        if ir.table.subquery.is_some() {
            return Err(OrmError::invalid(format!(
                "cannot {what} into the subquery `{}`; name a table",
                ir.table.qualifier()
            )));
        }
        Ok(ir.table.name.clone())
    }

    /// Runs `insert` (split under the bind limit) where `exec` writes.
    async fn run_insert<E: Executor + ?Sized>(
        exec: &E,
        route: crate::Route<'_>,
        insert: InsertIr,
    ) -> Result<InsertOutcome> {
        let target = exec.target_routed(Access::Write, route).await?;
        let parts = split(insert, target.dialect());
        run_inserts(&target, parts).await
    }

    /// Inserts `rows` (each a list of `(column, value)` pairs, every row
    /// with the same columns) in one statement, split under the database's
    /// bind limit; returns how many were inserted. No statement runs for an
    /// empty list.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow, deal: Uuid, tag: i64) -> Result<()> {
    /// table("deal_tag")
    ///     .insert(&uow, [[("deal_id", Value::from(deal)), ("tag_id", Value::from(tag))]])
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn insert<E, R, C, V>(
        self,
        exec: &E,
        rows: impl IntoIterator<Item = R>,
    ) -> Result<u64>
    where
        E: Executor + ?Sized,
        R: IntoIterator<Item = (C, V)>,
        C: Into<Cow<'static, str>>,
        V: Into<Value>,
    {
        let table = self.insert_table("insert")?;
        let rows = Rows::collect(&table, rows)?;
        if rows.values.is_empty() {
            return Ok(0);
        }
        let insert = InsertIr::new(table, rows.columns, rows.values);
        Ok(Self::run_insert(exec, self.route(), insert)
            .await?
            .rows_affected)
    }

    /// Inserts one row and returns its `id` column, read back with
    /// `RETURNING` on PostgreSQL and SQLite and from `LAST_INSERT_ID()` on
    /// MySQL.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let id: i64 = table("tags").insert_get_id(&db, [("name", "vip"), ("color", "gold")]).await?;
    /// # Ok(()) }
    /// ```
    pub async fn insert_get_id<K: Field>(
        self,
        exec: &(impl Executor + ?Sized),
        row: impl IntoIterator<Item = (impl Into<Cow<'static, str>>, impl Into<Value>)>,
    ) -> Result<K> {
        self.insert_get_key(exec, row, "id").await
    }

    /// [`insert_get_id`](Self::insert_get_id) for a key column other than
    /// `id`.
    pub async fn insert_get_key<K: Field>(
        self,
        exec: &(impl Executor + ?Sized),
        row: impl IntoIterator<Item = (impl Into<Cow<'static, str>>, impl Into<Value>)>,
        column: &str,
    ) -> Result<K> {
        let table = self.insert_table("insert")?;
        let rows = Rows::collect(&table, [row])?;
        let insert = InsertIr::new(table.clone(), rows.columns, rows.values)
            .returning(vec![Cow::Owned(column.to_owned())]);
        let outcome = Self::run_insert(exec, self.route(), insert).await?;
        returned_key(&outcome, &table, column)
    }

    /// Inserts `rows`, skipping the ones that conflict with a unique index;
    /// returns how many were inserted. PostgreSQL and SQLite skip only the
    /// rows that conflict on a unique index (`ON CONFLICT DO NOTHING`, `OR
    /// IGNORE`); MySQL's `INSERT IGNORE` also turns other errors, such as a
    /// value too long for its column, into warnings.
    pub async fn insert_or_ignore<E, R, C, V>(
        self,
        exec: &E,
        rows: impl IntoIterator<Item = R>,
    ) -> Result<u64>
    where
        E: Executor + ?Sized,
        R: IntoIterator<Item = (C, V)>,
        C: Into<Cow<'static, str>>,
        V: Into<Value>,
    {
        let table = self.insert_table("insert")?;
        let rows = Rows::collect(&table, rows)?;
        if rows.values.is_empty() {
            return Ok(0);
        }
        let insert = InsertIr::new(table, rows.columns, rows.values)
            .on_conflict(OnConflict::Ignore { target: Vec::new() });
        Ok(Self::run_insert(exec, self.route(), insert)
            .await?
            .rows_affected)
    }

    /// Inserts `rows`, skipping the ones that conflict on the unique index
    /// of `unique_by` (any unique index when empty), and returns the
    /// `returning` columns of the rows inserted. PostgreSQL and SQLite
    /// only: MySQL cannot return inserted rows (use
    /// [`insert_or_ignore`](Self::insert_or_ignore), then select them).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let created = table("tags")
    ///     .insert_or_ignore_returning(&db, [[("name", "vip"), ("color", "gold")]], ["id", "name"], ["name"])
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn insert_or_ignore_returning<E, R, C, V>(
        self,
        exec: &E,
        rows: impl IntoIterator<Item = R>,
        returning: impl IntoIterator<Item = impl Into<Cow<'static, str>>>,
        unique_by: impl IntoIterator<Item = impl Into<Cow<'static, str>>>,
    ) -> Result<Vec<Row>>
    where
        E: Executor + ?Sized,
        R: IntoIterator<Item = (C, V)>,
        C: Into<Cow<'static, str>>,
        V: Into<Value>,
    {
        let table = self.insert_table("insert")?;
        let returning: Vec<Cow<'static, str>> = returning.into_iter().map(Into::into).collect();
        if returning.is_empty() {
            return Err(OrmError::invalid(format!(
                "insert_or_ignore_returning into `{table}` names no column to return"
            )));
        }
        let rows = Rows::collect(&table, rows)?;
        if rows.values.is_empty() {
            return Ok(Vec::new());
        }
        let target = exec.target_routed(Access::Write, self.route()).await?;
        if target.dialect() == Some(Dialect::MySql) {
            return Err(OrmError::unsupported(
                "returning the rows of an insert (MySQL has no RETURNING; use insert_or_ignore, then select the rows)",
                "MySQL",
            ));
        }
        let insert = InsertIr::new(table, rows.columns, rows.values)
            .returning(returning)
            .on_conflict(OnConflict::Ignore {
                target: unique_by.into_iter().map(Into::into).collect(),
            });
        let parts = split(insert, target.dialect());
        Ok(run_inserts(&target, parts).await?.returned)
    }

    /// Inserts the rows `query` selects (`INSERT INTO table (columns)
    /// SELECT ..`), one value per column; returns how many were inserted.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let archived = table("archived_deals")
    ///     .insert_using(&db, ["id", "title"], table("deals").select(["id", "title"]).where_(col("stage").eq("lost")))
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn insert_using<E: Executor + ?Sized>(
        self,
        exec: &E,
        columns: impl IntoIterator<Item = impl Into<Cow<'static, str>>>,
        query: impl IntoQuery,
    ) -> Result<u64> {
        let table = self.insert_table("insert")?;
        let mut insert = InsertIr::new(
            table,
            columns.into_iter().map(Into::into).collect(),
            Vec::new(),
        );
        insert.select = Some(Box::new(query.into_query()));
        Ok(Self::run_insert(exec, self.route(), insert)
            .await?
            .rows_affected)
    }

    /// [`insert_using`](Self::insert_using), skipping the rows that
    /// conflict with a unique index.
    pub async fn insert_or_ignore_using<E: Executor + ?Sized>(
        self,
        exec: &E,
        columns: impl IntoIterator<Item = impl Into<Cow<'static, str>>>,
        query: impl IntoQuery,
    ) -> Result<u64> {
        let table = self.insert_table("insert")?;
        let mut insert = InsertIr::new(
            table,
            columns.into_iter().map(Into::into).collect(),
            Vec::new(),
        )
        .on_conflict(OnConflict::Ignore { target: Vec::new() });
        insert.select = Some(Box::new(query.into_query()));
        Ok(Self::run_insert(exec, self.route(), insert)
            .await?
            .rows_affected)
    }

    /// Inserts `rows`; a row that conflicts on the unique index of
    /// `unique_by` updates the existing row with `update` instead (an empty
    /// `update` skips it). Returns the number of rows the database reports
    /// (MySQL counts an updated row twice and an unchanged one not at all).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// use navree_orm::{ConflictUpdate, Expr, col};
    ///
    /// # async fn demo(db: Db) -> Result<()> {
    /// table("page_views")
    ///     .upsert(
    ///         &db,
    ///         [[("path", Value::from("/pricing")), ("views", Value::from(1))]],
    ///         ["path"],
    ///         [col("views").set_expr(Expr::col("page_views.views").add(Expr::excluded("views")))],
    ///     )
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn upsert<E, R, C, V, U>(
        self,
        exec: &E,
        rows: impl IntoIterator<Item = R>,
        unique_by: impl IntoIterator<Item = impl Into<Cow<'static, str>>>,
        update: impl IntoIterator<Item = U>,
    ) -> Result<u64>
    where
        E: Executor + ?Sized,
        R: IntoIterator<Item = (C, V)>,
        C: Into<Cow<'static, str>>,
        V: Into<Value>,
        U: Into<ConflictUpdate>,
    {
        let table = self.insert_table("upsert")?;
        let unique_by: Vec<Cow<'static, str>> = unique_by.into_iter().map(Into::into).collect();
        if unique_by.is_empty() {
            return Err(OrmError::invalid(format!(
                "an upsert into `{table}` needs the unique columns its rows conflict on"
            )));
        }
        let rows = Rows::collect(&table, rows)?;
        if rows.values.is_empty() {
            return Ok(0);
        }
        let update: Vec<ConflictUpdate> = update.into_iter().map(Into::into).collect();
        let sets = conflict_sets(&rows.columns, &unique_by, &[], update)?;
        let insert = InsertIr::new(table, rows.columns, rows.values)
            .on_conflict(upsert_conflict(unique_by, sets));
        Ok(Self::run_insert(exec, self.route(), insert)
            .await?
            .rows_affected)
    }

    /// Updates the rows matching `attributes` with the assignments
    /// `values` returns, or, when no row matches, inserts one with the
    /// attributes and the values. `values` learns whether a row existed.
    /// Returns whether a row was inserted or changed.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// table("settings")
    ///     .update_or_insert(&db, [col("key").set("theme")], |exists| {
    ///         let mut values = vec![col("value").set("dark")];
    ///         if !exists {
    ///             values.push(col("created_by").set("system"));
    ///         }
    ///         values
    ///     })
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn update_or_insert<E, F, I>(
        self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        values: F,
    ) -> Result<bool>
    where
        E: Executor + ?Sized,
        F: FnOnce(bool) -> I,
        I: IntoIterator<Item = Assignment>,
    {
        let table = self.insert_table("insert")?;
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        let query = attributes.iter().fold(self, |query, attribute| {
            query.where_(attribute.to_filter(None))
        });
        // Read and write through the same connection (the unit of work's).
        let target = exec.target_routed(Access::Write, query.route()).await?;
        let exists = target.exists(query.ir()).await?;
        let values: Vec<Assignment> = values(exists).into_iter().collect();
        if exists {
            if values.is_empty() {
                return Ok(true);
            }
            let sets = values
                .into_iter()
                .map(|assignment| assignment.into_set().map_err(OrmError::invalid))
                .collect::<Result<Vec<_>>>()?;
            let (table, wheres) = query.write_target("update", target.dialect())?;
            let changed = target
                .update(&crate::ir::UpdateIr {
                    table,
                    sets,
                    wheres,
                })
                .await?;
            return Ok(changed > 0);
        }
        let mut row: Vec<(Cow<'static, str>, Value)> = Vec::new();
        for assignment in attributes.into_iter().chain(values) {
            let column = Cow::Owned(assignment.column().to_owned());
            let value = assignment.assigned_value().cloned().ok_or_else(|| {
                OrmError::invalid(format!(
                    "update_or_insert into `{table}` needs a value for `{column}`, not an expression"
                ))
            })?;
            match row.iter_mut().find(|(existing, _)| *existing == column) {
                Some(existing) => existing.1 = value,
                None => row.push((column, value)),
            }
        }
        let rows = Rows::collect(&table, [row])?;
        let insert = InsertIr::new(table, rows.columns, rows.values);
        let outcome = target.insert(&insert).await?;
        Ok(outcome.rows_affected > 0)
    }

    /// Updates the rows of the table joined to the query's joined tables:
    /// PostgreSQL's (and SQLite's) `UPDATE table SET .. FROM joined ..
    /// WHERE <join conditions> AND <conditions>`, MySQL's `UPDATE table
    /// JOIN joined ON .. SET ..` (one joined table). Inner and cross joins
    /// only; returns how many rows changed.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// table("deals")
    ///     .join("companies", "companies.id", "=", "deals.company_id")
    ///     .where_(col("companies.active").eq(false))
    ///     .update_from(&db, [col("stage").set("lost")])
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn update_from<E: Executor + ?Sized>(
        self,
        exec: &E,
        assignments: impl IntoIterator<Item = Assignment>,
    ) -> Result<u64> {
        let sets = assignments
            .into_iter()
            .map(|assignment| assignment.into_set().map_err(OrmError::invalid))
            .collect::<Result<Vec<_>>>()?;
        let target = exec.target_routed(Access::Write, self.route()).await?;
        update_from_on(&target, self.into_ir(), sets).await
    }

    /// Empties the table and restarts its identity (see [`render_truncate`]
    /// per database); on MySQL this commits the open transaction.
    pub async fn truncate<E: Executor + ?Sized>(self, exec: &E) -> Result<()> {
        let table = self.insert_table("truncate")?;
        let target = exec.target_routed(Access::Write, self.route()).await?;
        truncate_table(&target, &table, false).await
    }

    /// [`truncate`](Self::truncate) that also empties the tables whose
    /// foreign keys reference this one (`CASCADE`, PostgreSQL only).
    pub async fn truncate_cascade<E: Executor + ?Sized>(self, exec: &E) -> Result<()> {
        let table = self.insert_table("truncate")?;
        let target = exec.target_routed(Access::Write, self.route()).await?;
        if target.dialect() != Some(Dialect::Postgres) {
            return Err(OrmError::unsupported(
                "TRUNCATE … CASCADE",
                target.context().database(),
            ));
        }
        truncate_table(&target, &table, true).await
    }
}

/// `UPDATE … FROM` of `query`'s table, joins and conditions.
async fn update_from_on(
    target: &Target<'_>,
    query: crate::ir::QueryIr,
    sets: Vec<(Cow<'static, str>, Expr)>,
) -> Result<u64> {
    query.check()?;
    let sql = sql_only(target, "update_from")?;
    if query.limit.is_some()
        || query.offset.is_some()
        || !query.groups.is_empty()
        || !query.unions.is_empty()
    {
        return Err(OrmError::invalid(format!(
            "update_from on `{}` takes joins and conditions only (no limit, offset, groups or unions)",
            query.table.qualifier()
        )));
    }
    let statement = render_update_from(
        &query.table,
        &sets,
        &query.joins,
        &query.wheres,
        sql.dialect(),
    )?;
    Ok(sql.execute(&statement).await?.rows_affected)
}

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

/// `models` with empty keys generated and empty timestamps set to `now`,
/// as [`Record::save`](crate::Record::save) would before inserting.
fn fill<M: Model>(models: impl IntoIterator<Item = M>, now: DateTime<Utc>) -> Result<Vec<M>> {
    let meta = M::meta();
    let mut filled = Vec::new();
    for mut model in models {
        if model.key().is_empty() {
            match meta.key_kind() {
                KeyKind::Increments => {}
                KeyKind::Manual => {
                    return Err(OrmError::MissingKey {
                        model: Cow::Borrowed(meta.name()),
                        column: meta.key_column(),
                    });
                }
                kind => {
                    let key = M::Key::generate(kind).ok_or_else(|| {
                        OrmError::invalid(format!(
                            "{}'s key type {kind:?} cannot be generated for its key field of type {}",
                            meta.name(),
                            M::Key::type_name()
                        ))
                    })?;
                    model.set_key(key);
                }
            }
        }
        for column in [meta.created_at(), meta.updated_at()].into_iter().flatten() {
            if model.timestamp_is_empty(column) {
                model.touch_timestamp(column, now)?;
            }
        }
        filled.push(model);
    }
    Ok(filled)
}

/// The columns of an insert and its rows.
type InsertGroup = (Vec<Cow<'static, str>>, Vec<Vec<Expr>>);

/// The inserts of `models`' stored values (casts applied): one group per
/// column list, as a key the database assigns is left out when it is
/// empty.
fn model_inserts<M: Model>(models: &[M]) -> Result<Vec<InsertIr>> {
    let meta = M::meta();
    let mut without_key: Option<InsertGroup> = None;
    let mut with_key: Option<InsertGroup> = None;
    for model in models {
        let skip_key = meta.key_kind() == KeyKind::Increments && model.key().is_empty();
        let (columns, row): (Vec<Cow<'static, str>>, Vec<Expr>) = model
            .values()?
            .into_iter()
            .filter(|(column, _)| !(skip_key && *column == meta.key_column()))
            .map(|(column, value)| (Cow::Borrowed(column), Expr::Value(value)))
            .unzip();
        let group = if skip_key {
            &mut without_key
        } else {
            &mut with_key
        };
        group
            .get_or_insert_with(|| (columns, Vec::new()))
            .1
            .push(row);
    }
    Ok([without_key, with_key]
        .into_iter()
        .flatten()
        .map(|(columns, rows)| InsertIr::new(meta.table(), columns, rows))
        .collect())
}

impl<M: Model> Builder<M> {
    /// Runs the inserts of `models` where `route` asks, each handling
    /// conflicts with `on_conflict` (given the insert's columns).
    async fn insert_models<E: Executor + ?Sized>(
        exec: &E,
        route: crate::Route<'_>,
        models: Option<Vec<M>>,
        fill_models: bool,
        on_conflict: impl Fn(&[Cow<'static, str>]) -> Result<Option<OnConflict>>,
    ) -> Result<u64> {
        let Some(models) = models.filter(|models| !models.is_empty()) else {
            return Ok(0);
        };
        let target = exec.target_routed(Access::Write, route).await?;
        let models = if fill_models {
            fill(models, target.context().now().await)?
        } else {
            models
        };
        let mut parts = Vec::new();
        for mut insert in model_inserts(&models)? {
            insert.on_conflict = on_conflict(&insert.columns)?;
            parts.extend(split(insert, target.dialect()));
        }
        Ok(run_inserts(&target, parts).await?.rows_affected)
    }

    /// Inserts `models` as they are, in one statement (split under the
    /// bind limit): every column's stored value, casts applied, and no key
    /// for a model whose key the database assigns and is empty. No hooks,
    /// no change events, no generated keys or timestamps (see
    /// [`fill_and_insert`](Self::fill_and_insert)). Returns how many rows
    /// were inserted.
    pub async fn insert<E: Executor + ?Sized>(
        self,
        exec: &E,
        models: impl IntoIterator<Item = M>,
    ) -> Result<u64> {
        Self::insert_models(
            exec,
            self.route(),
            Some(models.into_iter().collect()),
            false,
            |_| Ok(None),
        )
        .await
    }

    /// [`insert`](Self::insert), skipping the models that conflict with a
    /// unique index.
    pub async fn insert_or_ignore<E: Executor + ?Sized>(
        self,
        exec: &E,
        models: impl IntoIterator<Item = M>,
    ) -> Result<u64> {
        Self::insert_models(
            exec,
            self.route(),
            Some(models.into_iter().collect()),
            false,
            |_| Ok(Some(OnConflict::Ignore { target: Vec::new() })),
        )
        .await
    }

    /// `models` as [`fill_and_insert`](Self::fill_and_insert) would insert
    /// them: empty keys generated (UUID v7, UUID v4 or ULID by the key
    /// type) and empty timestamps set to now (the app's clock). Writes
    /// nothing.
    pub async fn fill_for_insert<E: Executor + ?Sized>(
        self,
        exec: &E,
        models: impl IntoIterator<Item = M>,
    ) -> Result<Vec<M>> {
        let target = exec.target_routed(Access::Read, self.route()).await?;
        fill(models, target.context().now().await)
    }

    /// Inserts `models` the way [`Record::save`](crate::Record::save) would
    /// fill them (generated keys, timestamps, casts), in one statement and
    /// without hooks or change events. Returns how many rows were
    /// inserted.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let events = (0..1_000).map(|n| Event { name: format!("import-{n}"), ..Default::default() });
    /// Event::query().fill_and_insert(&db, events).await?; // UUID v7 keys and timestamps filled in
    /// # Ok(()) }
    /// ```
    pub async fn fill_and_insert<E: Executor + ?Sized>(
        self,
        exec: &E,
        models: impl IntoIterator<Item = M>,
    ) -> Result<u64> {
        Self::insert_models(
            exec,
            self.route(),
            Some(models.into_iter().collect()),
            true,
            |_| Ok(None),
        )
        .await
    }

    /// [`fill_and_insert`](Self::fill_and_insert), skipping the models that
    /// conflict with a unique index.
    pub async fn fill_and_insert_or_ignore<E: Executor + ?Sized>(
        self,
        exec: &E,
        models: impl IntoIterator<Item = M>,
    ) -> Result<u64> {
        Self::insert_models(
            exec,
            self.route(),
            Some(models.into_iter().collect()),
            true,
            |_| Ok(Some(OnConflict::Ignore { target: Vec::new() })),
        )
        .await
    }

    /// Inserts one model filled like [`fill_and_insert`](Self::fill_and_insert)
    /// and returns its key (read back when the database assigns it).
    pub async fn fill_and_insert_get_id<E: Executor + ?Sized>(
        self,
        exec: &E,
        model: M,
    ) -> Result<M::Key> {
        let meta = M::meta();
        let target = exec.target_routed(Access::Write, self.route()).await?;
        let mut filled = fill([model], target.context().now().await)?;
        let model = filled.remove(0);
        let assigned = meta.key_kind() == KeyKind::Increments && model.key().is_empty();
        let mut inserts = model_inserts(std::slice::from_ref(&model))?;
        let mut insert = inserts.remove(0);
        if assigned {
            insert.returning = vec![Cow::Borrowed(meta.key_column())];
        }
        let outcome = target.insert(&insert).await?;
        if assigned {
            let key: Value = returned_key(&outcome, meta.table(), meta.key_column())?;
            return M::Key::from_value(key).map_err(|error| {
                decode_error(meta.name(), meta.key_column(), M::Key::type_name(), &error)
            });
        }
        Ok(model.key())
    }

    /// Inserts `models` filled like [`fill_and_insert`](Self::fill_and_insert);
    /// a model that conflicts on the unique index of `unique_by` updates
    /// the stored row with `update` instead: the listed columns (or
    /// [`ConflictUpdate::All`]: every column but the key, `unique_by` and
    /// the creation timestamp), plus the update timestamp. An empty
    /// `update` skips conflicting models. Returns the number of rows the
    /// database reports.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db, tags: Vec<Tag>) -> Result<()> {
    /// Tag::query().upsert(&db, tags, [Tag::NAME], [Tag::COLOR]).await?;
    /// # Ok(()) }
    /// ```
    pub async fn upsert<E, U>(
        self,
        exec: &E,
        models: impl IntoIterator<Item = M>,
        unique_by: impl IntoIterator<Item = impl ColumnName>,
        update: impl IntoIterator<Item = U>,
    ) -> Result<u64>
    where
        E: Executor + ?Sized,
        U: Into<ConflictUpdate>,
    {
        let meta = M::meta();
        let unique_by: Vec<Cow<'static, str>> = unique_by
            .into_iter()
            .map(|column| Cow::Owned(column.column_name().into_owned()))
            .collect();
        if unique_by.is_empty() {
            return Err(OrmError::invalid(format!(
                "an upsert of {} needs the unique columns its rows conflict on",
                meta.name()
            )));
        }
        let update: Vec<ConflictUpdate> = update.into_iter().map(Into::into).collect();
        let mut skipped = vec![meta.key_column()];
        skipped.extend(meta.created_at());
        Self::insert_models(
            exec,
            self.route(),
            Some(models.into_iter().collect()),
            true,
            |columns| {
                if update.is_empty() {
                    return Ok(Some(OnConflict::Ignore {
                        target: unique_by.clone(),
                    }));
                }
                let mut sets = conflict_sets(columns, &unique_by, &skipped, update.clone())?;
                if let Some(updated_at) = meta.updated_at()
                    && !sets.iter().any(|(column, _)| column == updated_at)
                    && columns.iter().any(|column| column == updated_at)
                {
                    sets.push((Cow::Borrowed(updated_at), Expr::excluded(updated_at)));
                }
                Ok(Some(upsert_conflict(unique_by.clone(), sets)))
            },
        )
        .await
    }

    /// Updates the model's rows joined to the query's joined tables
    /// (`UPDATE … FROM`, see [`TableBuilder::update_from`]), setting the
    /// update timestamp too; returns how many rows changed. Global scopes
    /// apply; no hooks run.
    pub async fn update_from<E: Executor + ?Sized>(
        self,
        exec: &E,
        assignments: impl IntoIterator<Item = Assignment>,
    ) -> Result<u64> {
        let target = exec.target_routed(Access::Write, self.route()).await?;
        let now = target.context().now().await;
        let sets = Self::sets(now, assignments)?;
        let mut ir = self.apply_global_scopes().ir;
        ir.table = TableRef::new(M::meta().table());
        update_from_on(&target, ir, sets).await
    }

    /// Empties the model's table and restarts its identity (no hooks; see
    /// [`TableBuilder::truncate`]).
    pub async fn truncate<E: Executor + ?Sized>(self, exec: &E) -> Result<()> {
        let target = exec.target_routed(Access::Write, self.route()).await?;
        truncate_table(&target, M::meta().table(), false).await
    }
}

impl<M: Model> crate::record::Record<M> {
    /// Inserts this new record unless it conflicts with a unique index of
    /// its table; returns whether it was inserted. The insert runs like
    /// [`save`](Self::save) (hooks, generated key, timestamps, change
    /// event), and nothing after `creating` runs when the row is skipped.
    /// A record that is stored already is refused: this only inserts.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let mut tag = Record::new(Tag { name: "vip".into(), color: "gold".into(), ..Default::default() });
    /// if !tag.save_or_ignore(&db).await? {
    ///     println!("a tag named vip exists already");
    /// }
    /// # Ok(()) }
    /// ```
    pub async fn save_or_ignore<E: Executor + ?Sized>(&mut self, exec: &E) -> Result<bool> {
        self.save_or_ignore_on(exec, Vec::<&str>::new()).await
    }

    /// [`save_or_ignore`](Self::save_or_ignore), skipping only conflicts on
    /// the unique index of `columns` (another unique index still fails the
    /// insert; MySQL checks every unique index).
    pub async fn save_or_ignore_on<E: Executor + ?Sized, C: ColumnName>(
        &mut self,
        exec: &E,
        columns: impl IntoIterator<Item = C>,
    ) -> Result<bool> {
        if self.exists() {
            return Err(OrmError::invalid(format!(
                "save_or_ignore inserts new records; this {} is stored already (use save)",
                M::meta().name()
            )));
        }
        let target_columns = columns
            .into_iter()
            .map(|column| Cow::Owned(column.column_name().into_owned()))
            .collect();
        let target = exec
            .target_routed(Access::Write, crate::query::model_route::<M>())
            .await?;
        crate::persist::save_new(
            &target,
            self,
            Some(OnConflict::Ignore {
                target: target_columns,
            }),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert_of(rows: usize, columns: usize) -> InsertIr {
        InsertIr::new(
            "t",
            (0..columns).map(|c| Cow::Owned(format!("c{c}"))).collect(),
            (0..rows)
                .map(|_| (0..columns).map(|_| Expr::val(1)).collect())
                .collect(),
        )
    }

    #[test]
    fn inserts_split_under_the_bind_limit() {
        let parts = split(insert_of(10_000, 4), Some(Dialect::Sqlite));
        assert_eq!(parts.len(), 2, "32,766 / 4 = 8,191 rows per statement");
        assert_eq!(parts[0].rows.len(), 8_191);
        assert_eq!(parts[1].rows.len(), 1_809);
        assert_eq!(
            split(insert_of(10_000, 4), Some(Dialect::Postgres)).len(),
            1
        );
        assert_eq!(split(insert_of(10_000, 4), None).len(), 1);
        let defaults = split(insert_of(3, 0), Some(Dialect::Postgres));
        assert_eq!(defaults.len(), 3, "DEFAULT VALUES inserts one row each");
    }

    #[test]
    fn conflict_updates_resolve_against_the_columns() {
        let columns = vec![
            Cow::Borrowed("id"),
            Cow::Borrowed("name"),
            Cow::Borrowed("color"),
        ];
        let sets = conflict_sets(
            &columns,
            &[Cow::Borrowed("name")],
            &["id"],
            vec![ConflictUpdate::All, "name".into()],
        )
        .unwrap();
        let names: Vec<&str> = sets.iter().map(|(c, _)| c.as_ref()).collect();
        assert_eq!(names, ["color", "name"]);
    }
}
