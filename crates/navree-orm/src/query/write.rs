//! Writing through a model query: mass updates, deletes, increments and
//! touches, and the find-or-create family.
//!
//! Mass writes run one statement over every matching row: no record is
//! read, so no lifecycle hook runs and no change event is recorded (load
//! the records and save them one by one when those matter). The update
//! timestamp is set when the model keeps one, and a soft-deleting model's
//! mass delete sets its deleted-at column instead of removing rows.

use std::borrow::Cow;

use chrono::{DateTime, Utc};

use crate::column::{Assignment, Column};
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor, Target};
use crate::ir::{
    ArithmeticOp, ColumnRef, DeleteIr, Distinct, Expr, InValues, QueryIr, SelectItem, TableRef,
    UpdateIr, Where, WhereClause,
};
use crate::model::Model;
use crate::persist;
use crate::query::{Builder, key_expr};
use crate::record::Record;
use crate::value::Numeric;

/// The alias of the derived table that keeps the keys of a write with
/// joins or a limit (MySQL refuses a subquery on the table it writes
/// unless it is wrapped).
const KEYS_ALIAS: &str = "navree_keys";

impl<M: Model> Builder<M> {
    /// The rows a mass write touches: the query's conditions, or, for a
    /// query with joins, groups, a limit, an offset, a per-group limit or a
    /// derived table, the keys of the rows it selects.
    pub(crate) fn write_target(self, target: &Target<'_>) -> Result<(TableRef, Vec<WhereClause>)> {
        let ir = crate::workspace::with_target(target, || self.apply_global_scopes()).ir;
        ir.check()?;
        let meta = M::meta();
        let table = TableRef::new(meta.table());
        let simple = ir.joins.is_empty()
            && ir.groups.is_empty()
            && ir.havings.is_empty()
            && ir.unions.is_empty()
            && ir.limit.is_none()
            && ir.offset.is_none()
            && ir.group_limit.is_none()
            && ir.table.subquery.is_none()
            && ir.table.function.is_none()
            && matches!(ir.distinct, Distinct::No);
        if simple {
            return Ok((table, ir.wheres));
        }
        let mut inner = ir;
        inner.columns = vec![SelectItem {
            expr: Expr::Column(ColumnRef::new(
                Some(inner.table.qualifier().to_owned()),
                meta.key_column(),
            )),
            alias: None,
        }];
        inner.lock = None;
        // MySQL refuses a write whose subquery reads the written table,
        // unless the derived table is materialized: a LIMIT does that,
        // otherwise DISTINCT (the order is moot without a limit, and DISTINCT
        // would need the ordered columns selected).
        if inner.limit.is_none() && inner.offset.is_none() && inner.group_limit.is_none() {
            inner.orders.clear();
            inner.distinct = Distinct::Yes;
        }
        let mut keys = QueryIr::new(KEYS_ALIAS);
        keys.table = TableRef::subquery(inner, KEYS_ALIAS);
        keys.columns = vec![SelectItem {
            expr: Expr::Column(ColumnRef::new(Some(KEYS_ALIAS), meta.key_column())),
            alias: None,
        }];
        Ok((
            table,
            vec![WhereClause::and(Where::In {
                expr: key_expr::<M>(),
                values: InValues::Subquery(Box::new(keys)),
                not: false,
            })],
        ))
    }

    /// `SET` items from assignments, plus the update timestamp (at `now`)
    /// unless an assignment sets it. An assignment to a JSON path
    /// (`col("meta->status").set("won")`) changes that value inside the
    /// column (see [`render_update`](crate::sql::render_update)).
    pub(crate) fn sets(
        now: DateTime<Utc>,
        assignments: impl IntoIterator<Item = Assignment>,
    ) -> Result<Vec<(Cow<'static, str>, Expr)>> {
        let mut sets = Vec::new();
        for assignment in assignments {
            let (column, expr) = assignment.into_set().map_err(OrmError::invalid)?;
            sets.push((column, expr));
        }
        crate::workspace::check_assignments::<M>(&sets)?;
        let meta = M::meta();
        if let Some(updated_at) = meta.updated_at()
            && !sets.iter().any(|(column, _)| column == updated_at)
            && let Some(value) = M::timestamp_value(updated_at, now)
        {
            sets.push((Cow::Borrowed(updated_at), Expr::Value(value)));
        }
        Ok(sets)
    }

    /// Sets columns on every matching record in one statement (and the
    /// update timestamp); returns how many rows changed. No hooks run and
    /// no change events are recorded.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow) -> Result<()> {
    /// let cutoff = Utc::now() - Duration::days(90);
    /// let lost = Deal::query()
    ///     .where_(Deal::STAGE.eq(Stage::Open))
    ///     .where_(Deal::CREATED_AT.lt(cutoff))
    ///     .update(&uow, [Deal::STAGE.set(Stage::Lost)])
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn update<E: Executor + ?Sized>(
        self,
        exec: &E,
        assignments: impl IntoIterator<Item = Assignment>,
    ) -> Result<u64> {
        let target = exec.target_routed(Access::Write, self.route()).await?;
        let ctx = target.context();
        let now = ctx.now().await;
        let sets = Self::sets(now, assignments)?;
        let (table, wheres) = self.write_target(&target)?;
        target
            .update(&UpdateIr {
                table,
                sets,
                wheres,
            })
            .await
    }

    /// Deletes every matching record in one statement (a soft-deleting
    /// model sets the deleted-at column instead); returns how many rows
    /// were affected. No hooks run and no change events are recorded.
    pub async fn delete<E: Executor + ?Sized>(self, exec: &E) -> Result<u64> {
        let meta = M::meta();
        match meta.deleted_at() {
            Some(deleted_at) => {
                let target = exec.target_routed(Access::Write, self.route()).await?;
                let ctx = target.context();
                let now = ctx.now().await;
                let value = M::timestamp_value(deleted_at, now).ok_or_else(|| {
                    crate::model::derive_support::not_a_timestamp::<M>(deleted_at)
                })?;
                let sets = Self::sets(now, [Assignment::value(deleted_at, value)])?;
                let (table, wheres) = self.write_target(&target)?;
                target
                    .update(&UpdateIr {
                        table,
                        sets,
                        wheres,
                    })
                    .await
            }
            None => self.delete_rows(exec).await,
        }
    }

    /// Removes every matching row in one statement, also for a
    /// soft-deleting model (no hooks, no change events).
    pub(crate) async fn delete_rows<E: Executor + ?Sized>(self, exec: &E) -> Result<u64> {
        let target = exec.target_routed(Access::Write, self.route()).await?;
        let (table, wheres) = self.write_target(&target)?;
        target.delete(&DeleteIr { table, wheres }).await
    }

    /// Adds `amount` to `column` on every matching record in one statement
    /// (and sets the update timestamp); returns how many rows changed.
    pub async fn increment<E, T>(self, exec: &E, column: Column<M, T>, amount: T) -> Result<u64>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        self.step(exec, column, amount, ArithmeticOp::Add, Vec::new())
            .await
    }

    /// Like [`increment`](Builder::increment), also setting `extra`
    /// columns.
    pub async fn increment_with<E, T>(
        self,
        exec: &E,
        column: Column<M, T>,
        amount: T,
        extra: impl IntoIterator<Item = Assignment>,
    ) -> Result<u64>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        self.step(
            exec,
            column,
            amount,
            ArithmeticOp::Add,
            extra.into_iter().collect(),
        )
        .await
    }

    /// Subtracts `amount` from `column` on every matching record.
    pub async fn decrement<E, T>(self, exec: &E, column: Column<M, T>, amount: T) -> Result<u64>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        self.step(exec, column, amount, ArithmeticOp::Sub, Vec::new())
            .await
    }

    /// Like [`decrement`](Builder::decrement), also setting `extra`
    /// columns.
    pub async fn decrement_with<E, T>(
        self,
        exec: &E,
        column: Column<M, T>,
        amount: T,
        extra: impl IntoIterator<Item = Assignment>,
    ) -> Result<u64>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        self.step(
            exec,
            column,
            amount,
            ArithmeticOp::Sub,
            extra.into_iter().collect(),
        )
        .await
    }

    async fn step<E, T>(
        self,
        exec: &E,
        column: Column<M, T>,
        amount: T,
        op: ArithmeticOp,
        extra: Vec<Assignment>,
    ) -> Result<u64>
    where
        E: Executor + ?Sized,
        T: Numeric,
    {
        let name = column.column_ref().name.into_owned();
        let step = column.set_expr(Expr::Arithmetic {
            left: Box::new(Expr::Column(ColumnRef::new(None::<&str>, name))),
            op,
            right: Box::new(Expr::Value(amount.to_value())),
        });
        let target = exec.target_routed(Access::Write, self.route()).await?;
        let ctx = target.context();
        let now = ctx.now().await;
        let sets = Self::sets(now, std::iter::once(step).chain(extra))?;
        let (table, wheres) = self.write_target(&target)?;
        target
            .update(&UpdateIr {
                table,
                sets,
                wheres,
            })
            .await
    }

    /// Sets the update timestamp of every matching record to now; returns
    /// how many rows changed (0 when the model keeps no update timestamp).
    pub async fn touch<E: Executor + ?Sized>(self, exec: &E) -> Result<u64> {
        if M::meta().updated_at().is_none() {
            return Ok(0);
        }
        self.update(exec, std::iter::empty()).await
    }

    /// The first record matching `attributes`, or a new, unsaved one with
    /// them (and whatever `fill` sets on it).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let tag = Tag::query().first_or_new(&db, [Tag::NAME.set("vip")], |t| t.color = "gold".into()).await?;
    /// assert!(tag.exists() || tag.color == "gold"); // a new tag got the fill
    /// # Ok(()) }
    /// ```
    pub async fn first_or_new<E: Executor + ?Sized>(
        self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut M) + Send,
    ) -> Result<Record<M>>
    where
        M: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        let target = exec.target_routed(Access::Read, self.route()).await?;
        if let Some(found) = self.matching(&attributes).first_on(&target).await? {
            return Ok(found);
        }
        new_record(&attributes, fill)
    }

    /// The first record matching `attributes`, or a new one with them (and
    /// whatever `fill` sets) inserted. When a concurrent request inserts
    /// the same record first (a unique violation), the record that won is
    /// returned.
    pub async fn first_or_create<E: Executor + ?Sized>(
        self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut M) + Send,
    ) -> Result<Record<M>>
    where
        M: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        let target = exec.target_routed(Access::Write, self.route()).await?;
        if let Some(found) = self.clone().matching(&attributes).first_on(&target).await? {
            return Ok(found);
        }
        self.create_or_first_on(&target, &attributes, fill).await
    }

    /// Inserts a record with `attributes` (and whatever `fill` sets); when
    /// that violates a unique constraint, returns the record matching
    /// `attributes` instead. Inside a transaction the insert runs in a
    /// savepoint, so the failed attempt does not abort the transaction.
    pub async fn create_or_first<E: Executor + ?Sized>(
        self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut M) + Send,
    ) -> Result<Record<M>>
    where
        M: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        let target = exec.target_routed(Access::Write, self.route()).await?;
        self.create_or_first_on(&target, &attributes, fill).await
    }

    /// Updates the first record matching `attributes` with `fill` (and
    /// saves it), or creates one with both.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(uow: Uow) -> Result<()> {
    /// Setting::query()
    ///     .update_or_create(&uow, [Setting::KEY.set("theme")], |s| s.value = "dark".into())
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub async fn update_or_create<E: Executor + ?Sized>(
        self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl Fn(&mut M) + Send + Sync,
    ) -> Result<Record<M>>
    where
        M: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        let target = exec.target_routed(Access::Write, self.route()).await?;
        let mut record = match self.clone().matching(&attributes).first_on(&target).await? {
            Some(found) => found,
            None => self.create_or_first_on(&target, &attributes, &fill).await?,
        };
        if !record.was_recently_created() && record.exists() {
            fill(&mut record.model);
            persist::save(&target, &mut record).await?;
        }
        Ok(record)
    }

    /// The query narrowed to `attributes`.
    fn matching(self, attributes: &[Assignment]) -> Self {
        let table = M::meta().table();
        attributes.iter().fold(self, |query, attribute| {
            query.where_(attribute.to_filter(Some(table)))
        })
    }

    /// The first record on `target`.
    async fn first_on(self, target: &Target<'_>) -> Result<Option<Record<M>>> {
        Ok(self
            .limit(1)
            .fetch(target)
            .await?
            .into_vec()
            .into_iter()
            .next())
    }

    async fn create_or_first_on(
        self,
        target: &Target<'_>,
        attributes: &[Assignment],
        fill: impl FnOnce(&mut M) + Send,
    ) -> Result<Record<M>>
    where
        M: Default,
    {
        let mut record = new_record(attributes, fill)?;
        let inserted = match target.begin_savepoint().await? {
            Some(savepoint) => {
                let nested = target.on_transaction(&savepoint);
                let result = persist::save(&nested, &mut record).await;
                drop(nested);
                match result {
                    Ok(saved) => {
                        savepoint.commit().await?;
                        Ok(saved)
                    }
                    Err(error) => {
                        savepoint.rollback().await?;
                        Err(error)
                    }
                }
            }
            None => persist::save(target, &mut record).await,
        };
        match inserted {
            Ok(_) => Ok(record),
            Err(error) if error.is_unique_violation() => {
                match self.matching(attributes).first_on(target).await? {
                    Some(found) => Ok(found),
                    None => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }
}

/// A new record with `attributes` set, then `fill`.
fn new_record<M: Model + Default>(
    attributes: &[Assignment],
    fill: impl FnOnce(&mut M),
) -> Result<Record<M>> {
    let mut model = M::default();
    for attribute in attributes {
        match attribute.assigned_value() {
            Some(value) => model.set_value(attribute.column(), value.clone())?,
            None => {
                return Err(OrmError::invalid(format!(
                    "the attribute `{}` of a {} to find or create must be a value",
                    attribute.column(),
                    M::meta().name()
                )));
            }
        }
    }
    fill(&mut model);
    Ok(Record::new(model))
}
