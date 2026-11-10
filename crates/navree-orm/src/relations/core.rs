//! What every relation type shares: [`RelationCore`] (the name, the parent
//! a relation was made from, the refinements of its query), how the
//! relation's own conditions combine with the caller's, and the macros
//! that give relation types every builder and retrieval method.

use std::borrow::Cow;

use crate::error::{OrmError, Result};
use crate::ir::{Boolean, ColumnRef, InValues, Where, WhereClause};
use crate::model::Model;
use crate::query::Builder;
use crate::record::Record;
use crate::value::Value;

/// The part every relation has: its name, the parent record it was made
/// from (`None` for a relation used by name: eager loads, existence
/// checks), and the refinements of the related query (conditions, orders,
/// limits, eager loads, scopes) made in the relation's definition and by
/// the caller.
pub(crate) struct RelationCore<P: Model, R: Model> {
    pub(crate) name: Cow<'static, str>,
    pub(crate) parent: Option<P>,
    pub(crate) query: Builder<R>,
}

impl<P: Model, R: Model> RelationCore<P, R> {
    /// A relation of `parent` named `name`.
    pub(crate) fn new(parent: Option<P>, name: Cow<'static, str>) -> Self {
        RelationCore {
            name,
            parent,
            query: Builder::new().with_relation_defaults(),
        }
    }

    /// The parent, or the error for using a relation that was not made
    /// from a record.
    pub(crate) fn parent(&self) -> Result<&P> {
        self.parent.as_ref().ok_or_else(|| {
            OrmError::invalid(format!(
                "the relation `{}` of {} was not made from a record: call the relation method on one (`record.{}()`)",
                self.name,
                P::meta().name(),
                self.name
            ))
        })
    }

    /// The same relation without its parent (what relation names resolve
    /// to).
    pub(crate) fn unbound(&self) -> Self {
        RelationCore {
            name: self.name.clone(),
            parent: None,
            query: self.query.clone(),
        }
    }
}

impl<P: Model, R: Model> Clone for RelationCore<P, R> {
    fn clone(&self) -> Self {
        RelationCore {
            name: self.name.clone(),
            parent: self.parent.clone(),
            query: self.query.clone(),
        }
    }
}

/// The stored value of `column` of `model`, or the error naming the
/// relation that needs it.
pub(crate) fn column_value<M: Model>(model: &M, column: &str, relation: &str) -> Result<Value> {
    model.value(column)?.ok_or_else(|| {
        OrmError::invalid(format!(
            "the relation `{relation}` uses column `{column}`, which {} does not have",
            M::meta().name()
        ))
    })
}

/// The value of `column` of each record.
pub(crate) fn column_values<M: Model>(
    records: &[Record<M>],
    column: &str,
    relation: &str,
) -> Result<Vec<Value>> {
    records
        .iter()
        .map(|record| column_value(&record.model, column, relation))
        .collect()
}

/// `table.column`.
pub(crate) fn qualified(table: &str, column: &str) -> ColumnRef {
    ColumnRef::new(Some(table.to_owned()), column.to_owned())
}

/// A condition no row of `R` meets: its key in an empty list.
pub(crate) fn nothing<R: Model>() -> Where {
    Where::In {
        expr: crate::query::key_expr::<R>(),
        values: InValues::List(Vec::new()),
        not: false,
    }
}

/// Puts the relation's own conditions before the caller's. The caller's
/// are grouped when they contain an `OR`, so `deal.activities().where_(a)
/// .or_where(b)` is `deal_id = ? AND (a OR b)`: an `OR` never reaches the
/// records of other parents.
pub(crate) fn constrain<R: Model>(mut query: Builder<R>, constraints: Vec<Where>) -> Builder<R> {
    let caller = std::mem::take(&mut query.ir.wheres);
    query.ir.wheres = constraints.into_iter().map(WhereClause::and).collect();
    if caller.iter().any(|clause| clause.boolean == Boolean::Or) {
        query.ir.wheres.push(WhereClause::and(Where::Group(caller)));
    } else {
        query.ir.wheres.extend(caller);
    }
    query
}

/// A new record of `R` with `attributes` set, then `fill`.
pub(crate) fn new_with<R: Model + Default>(
    attributes: &[crate::column::Assignment],
    fill: impl FnOnce(&mut R),
) -> Result<Record<R>> {
    let mut model = R::default();
    for attribute in attributes {
        match attribute.assigned_value() {
            Some(value) => model.set_value(attribute.column(), value.clone())?,
            None => {
                return Err(OrmError::invalid(format!(
                    "the attribute `{}` of a {} to find or create must be a value",
                    attribute.column(),
                    R::meta().name()
                )));
            }
        }
    }
    fill(&mut model);
    Ok(Record::new(model))
}

/// `attributes` as conditions on `R`'s columns.
pub(crate) fn matching<R: Model>(
    query: Builder<R>,
    attributes: &[crate::column::Assignment],
) -> Builder<R> {
    let table = R::meta().table();
    attributes.iter().fold(query, |query, attribute| {
        query.where_(attribute.to_filter(Some(table)))
    })
}

/// Saves `record` on `target`, inside a savepoint when `target` is a
/// transaction (so a failed insert does not abort it).
pub(crate) async fn save_atomically<R: Model>(
    target: &crate::exec::Target<'_>,
    record: &mut Record<R>,
) -> Result<bool> {
    match target.begin_savepoint().await? {
        Some(savepoint) => {
            let nested = target.on_transaction(&savepoint);
            let result = crate::persist::save(&nested, record).await;
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
        None => crate::persist::save(target, record).await,
    }
}

/// Restores a soft-deleted record: `Restoring` (cancellable), the
/// deleted-at column cleared and saved, `Restored`. `false` when the model
/// does not soft-delete, the record is not trashed, or a hook cancelled.
pub(crate) async fn restore_record<R: Model>(
    target: &crate::exec::Target<'_>,
    record: &mut Record<R>,
) -> Result<bool> {
    use crate::hooks::{Flow, Hooks, ModelEvent};
    let Some(deleted_at) = R::meta().deleted_at() else {
        return Ok(false);
    };
    if !record.trashed() {
        return Ok(false);
    }
    let ctx = target.context();
    let hooks = Hooks::<R>::resolve(&ctx).await?;
    if hooks.before(ModelEvent::Restoring, &ctx, record).await? == Flow::Cancel {
        return Ok(false);
    }
    record.model.clear_timestamp(deleted_at);
    if !crate::persist::save(target, record).await? {
        return Ok(false);
    }
    hooks.after(ModelEvent::Restored, &ctx, record).await?;
    Ok(true)
}

/// Gives each parent the value of its key: `values` by key, and each
/// parent's key (`None`: nothing related). A value used by several parents
/// is cloned for all but the last.
pub(crate) fn distribute<T: Clone>(
    mut values: std::collections::HashMap<crate::relations::keys::RelationKey, T>,
    parents: &[Option<crate::relations::keys::RelationKey>],
) -> Vec<Option<T>> {
    let mut remaining: std::collections::HashMap<&crate::relations::keys::RelationKey, usize> =
        std::collections::HashMap::new();
    for key in parents.iter().flatten() {
        *remaining.entry(key).or_default() += 1;
    }
    parents
        .iter()
        .map(|key| {
            let key = key.as_ref()?;
            let left = remaining.get_mut(key)?;
            *left -= 1;
            if *left == 0 {
                values.remove(key)
            } else {
                values.get(key).cloned()
            }
        })
        .collect()
}

/// A relation's query, ready to run for its parent.
pub(crate) struct Bound<R: Model> {
    /// The query: the relation's conditions, then the caller's.
    pub(crate) query: Builder<R>,
    /// The parent's key is NULL: nothing is related, nothing is queried.
    pub(crate) empty: bool,
    /// What happens to the records once they are read (pivots are taken
    /// out of the selected columns, through keys removed, inverse
    /// relations set).
    pub(crate) finish: Option<Finisher<R>>,
}

/// Work on the records a relation query read.
pub(crate) type Finisher<R> = Box<dyn Fn(&mut [Record<R>]) -> Result<()> + Send + Sync>;

impl<R: Model> Bound<R> {
    /// `query`, to run as it is.
    pub(crate) fn new(query: Builder<R>) -> Self {
        Bound {
            query,
            empty: false,
            finish: None,
        }
    }

    /// Nothing is related (the parent's key is NULL): `query` gets a
    /// condition no row meets (an empty `IN`, which every driver
    /// understands), and the methods that can answer without a query do.
    pub(crate) fn nothing(query: Builder<R>) -> Self {
        let query = constrain(query, vec![nothing::<R>()]);
        Bound {
            query,
            empty: true,
            finish: None,
        }
    }

    /// With `finish` run on the records read.
    pub(crate) fn finishing(mut self, finish: Option<Finisher<R>>) -> Self {
        self.finish = finish;
        self
    }

    /// Runs the finisher on `records`.
    pub(crate) fn finish(&self, records: &mut [Record<R>]) -> Result<()> {
        match &self.finish {
            Some(finish) => finish(records),
            None => Ok(()),
        }
    }
}

/// The query methods of a relation type: the where family and the clauses
/// (on the relation's query, after its own conditions), the model query
/// methods, eager loads, relation counts and existence checks of the
/// related records. Expects `self.core: RelationCore<P, R>`.
macro_rules! relation_query_methods {
    () => {
        $crate::query::filters::filters!();
        $crate::query::clauses::clauses!();

        /// Only the related record with key `key`.
        pub fn where_key(mut self, key: impl Into<R::Key>) -> Self {
            self.core.query = self.core.query.where_key(key);
            self
        }

        /// Every related record but the one with key `key`.
        pub fn where_key_not(mut self, key: impl Into<R::Key>) -> Self {
            self.core.query = self.core.query.where_key_not(key);
            self
        }

        /// Only the related records with these keys.
        pub fn where_key_in(mut self, keys: impl IntoIterator<Item = impl Into<R::Key>>) -> Self {
            self.core.query = self.core.query.where_key_in(keys);
            self
        }

        /// Every related record but the ones with these keys.
        pub fn where_key_not_in(
            mut self,
            keys: impl IntoIterator<Item = impl Into<R::Key>>,
        ) -> Self {
            self.core.query = self.core.query.where_key_not_in(keys);
            self
        }

        /// Applies `scope` to the related query: a local scope, or any
        /// [`Builder`]($crate::Builder) method
        /// (`.scope(|q| q.or_has("notes"))`). Its conditions are grouped
        /// when they contain an `OR`.
        pub fn scope(
            mut self,
            scope: impl FnOnce($crate::Builder<R>) -> $crate::Builder<R>,
        ) -> Self {
            self.core.query = self.core.query.scope(scope);
            self
        }

        /// Leaves out global scope `S` of the related model.
        pub fn without_global_scope<S: $crate::GlobalScope<R>>(mut self) -> Self {
            self.core.query = self.core.query.without_global_scope::<S>();
            self
        }

        /// Leaves out every global scope of the related model.
        pub fn without_global_scopes(mut self) -> Self {
            self.core.query = self.core.query.without_global_scopes();
            self
        }

        /// Includes soft-deleted related records.
        pub fn with_trashed(mut self) -> Self {
            self.core.query = self.core.query.with_trashed();
            self
        }

        /// Newest related records first (by their creation timestamp).
        pub fn latest(mut self) -> Self {
            self.core.query = self.core.query.latest();
            self
        }

        /// Oldest related records first (by their creation timestamp).
        pub fn oldest(mut self) -> Self {
            self.core.query = self.core.query.oldest();
            self
        }

        /// Eager loads relation `path` of the related records.
        pub fn with(mut self, path: impl $crate::relations::RelationPath<R>) -> Self {
            self.core.query = self.core.query.with(path);
            self
        }

        /// Eager loads every relation path of `paths` of the related
        /// records.
        pub fn with_all(
            mut self,
            paths: impl IntoIterator<Item = impl $crate::relations::RelationPath<R>>,
        ) -> Self {
            self.core.query = self.core.query.with_all(paths);
            self
        }

        /// Eager loads relation `path` of the related records, constrained
        /// by `constraint`.
        pub fn with_constrained<Q: 'static>(
            mut self,
            path: impl $crate::relations::ConstrainPath<R, Q>,
            constraint: impl Fn(Q) -> Q + Send + Sync + 'static,
        ) -> Self {
            self.core.query = self.core.query.with_constrained(path, constraint);
            self
        }

        /// Stops eager loading relation `path` of the related records (a
        /// default eager load or count of their model too).
        pub fn without(mut self, path: impl $crate::relations::RelationPath<R>) -> Self {
            self.core.query = self.core.query.without(path);
            self
        }

        /// Eager loads exactly `paths` of the related records, replacing
        /// their model's default eager loads and counts.
        pub fn with_only(
            mut self,
            paths: impl IntoIterator<Item = impl $crate::relations::RelationPath<R>>,
        ) -> Self {
            self.core.query = self.core.query.with_only(paths);
            self
        }

        /// Eager loads nothing into the related records: no default eager
        /// load or count of their model either.
        pub fn without_eager_loads(mut self) -> Self {
            self.core.query = self.core.query.without_eager_loads();
            self
        }

        /// Adds `{relation}_count` (the number of related records of each
        /// related record) to the related records' extras.
        pub fn with_count(mut self, path: impl $crate::relations::RelationPath<R>) -> Self {
            self.core.query = self.core.query.with_count(path);
            self
        }

        /// Only related records that have records through relation `path`.
        pub fn has(mut self, path: impl $crate::relations::RelationPath<R>) -> Self {
            self.core.query = self.core.query.has(path);
            self
        }

        /// Only related records without records through relation `path`.
        pub fn doesnt_have(mut self, path: impl $crate::relations::RelationPath<R>) -> Self {
            self.core.query = self.core.query.doesnt_have(path);
            self
        }

        /// Only related records with records through relation `path` that
        /// match `constraint`.
        pub fn where_has<T: $crate::Model>(
            mut self,
            path: impl $crate::relations::RelationPathTo<R, T>,
            constraint: impl FnOnce($crate::Builder<T>) -> $crate::Builder<T>,
        ) -> Self {
            self.core.query = self.core.query.where_has(path, constraint);
            self
        }

        /// Only related records without records through relation `path`
        /// that match `constraint`.
        pub fn where_doesnt_have<T: $crate::Model>(
            mut self,
            path: impl $crate::relations::RelationPathTo<R, T>,
            constraint: impl FnOnce($crate::Builder<T>) -> $crate::Builder<T>,
        ) -> Self {
            self.core.query = self.core.query.where_doesnt_have(path, constraint);
            self
        }

        /// Only related records with a record through relation `path`
        /// matching `filter`.
        pub fn where_relation(
            mut self,
            path: impl $crate::relations::RelationPath<R>,
            filter: impl Into<$crate::Filter>,
        ) -> Self {
            self.core.query = self.core.query.where_relation(path, filter);
            self
        }

        /// The relation's name.
        pub fn relation_name(&self) -> &str {
            &self.core.name
        }

        /// The related model's table.
        pub fn related_table(&self) -> &'static str {
            <R as $crate::Model>::meta().table()
        }

        /// Names the relation (what it is loaded under on records).
        /// `#[relations]` names relations after their methods.
        pub fn named(mut self, name: impl Into<std::borrow::Cow<'static, str>>) -> Self {
            self.core.name = name.into();
            self
        }
    };
}

pub(crate) use relation_query_methods;

/// The retrieval and mass-write methods of a relation type, running its
/// query for its parent. Expects `fn bound(self) -> Result<Bound<R>>`.
macro_rules! relation_retrieval_methods {
    () => {
        /// The related records.
        pub async fn get<E: $crate::Executor + ?Sized>(
            self,
            exec: &E,
        ) -> $crate::Result<$crate::Collection<R>> {
            let bound = self.bound()?;
            if bound.empty {
                return Ok($crate::Collection::new());
            }
            let target = exec.target($crate::Access::Read).await?;
            let query = bound.query.clone();
            let mut records = query.fetch(&target).await?;
            bound.finish(&mut records)?;
            Ok(records)
        }

        /// The first related record, if any.
        pub async fn first<E: $crate::Executor + ?Sized>(
            self,
            exec: &E,
        ) -> $crate::Result<Option<$crate::Record<R>>> {
            Ok(self.limit(1).get(exec).await?.into_vec().into_iter().next())
        }

        /// The first related record, or
        /// [`OrmError::ModelNotFound`]($crate::OrmError::ModelNotFound).
        pub async fn first_or_fail<E: $crate::Executor + ?Sized>(
            self,
            exec: &E,
        ) -> $crate::Result<$crate::Record<R>> {
            self.first(exec).await?.ok_or_else(|| {
                $crate::OrmError::not_found(<R as $crate::Model>::meta().name(), Vec::new())
            })
        }

        /// The first related record matching `filter`, if any.
        pub async fn first_where<E: $crate::Executor + ?Sized>(
            self,
            exec: &E,
            filter: impl Into<$crate::Filter>,
        ) -> $crate::Result<Option<$crate::Record<R>>> {
            self.where_(filter).first(exec).await
        }

        /// The only related record: `ModelNotFound` when there is none,
        /// `MultipleRecordsFound` when there are more.
        pub async fn sole<E: $crate::Executor + ?Sized>(
            self,
            exec: &E,
        ) -> $crate::Result<$crate::Record<R>> {
            self.limit(2).get(exec).await?.sole()
        }

        /// The related record with key `key`, if it is related.
        pub async fn find<E: $crate::Executor + ?Sized>(
            self,
            exec: &E,
            key: impl Into<R::Key>,
        ) -> $crate::Result<Option<$crate::Record<R>>> {
            self.where_key(key).first(exec).await
        }

        /// The related records with these keys (others are left out).
        pub async fn find_many<E: $crate::Executor + ?Sized>(
            self,
            exec: &E,
            keys: impl IntoIterator<Item = impl Into<R::Key>>,
        ) -> $crate::Result<$crate::Collection<R>> {
            let keys: Vec<R::Key> = keys.into_iter().map(Into::into).collect();
            if keys.is_empty() {
                return Ok($crate::Collection::new());
            }
            self.where_key_in(keys).get(exec).await
        }

        /// The related record with key `key`, or `ModelNotFound` naming the
        /// key.
        pub async fn find_or_fail<E: $crate::Executor + ?Sized>(
            self,
            exec: &E,
            key: impl Into<R::Key>,
        ) -> $crate::Result<$crate::Record<R>> {
            let key: R::Key = key.into();
            let text = $crate::PrimaryKey::to_key_string(&key);
            self.find(exec, key).await?.ok_or_else(|| {
                $crate::OrmError::not_found(<R as $crate::Model>::meta().name(), vec![text])
            })
        }

        /// The only related record with key `key` (an error when there is
        /// none or more).
        pub async fn find_sole<E: $crate::Executor + ?Sized>(
            self,
            exec: &E,
            key: impl Into<R::Key>,
        ) -> $crate::Result<$crate::Record<R>> {
            self.where_key(key).sole(exec).await
        }

        /// Whether any record is related.
        pub async fn exists<E: $crate::Executor + ?Sized>(self, exec: &E) -> $crate::Result<bool> {
            let bound = self.bound()?;
            if bound.empty {
                return Ok(false);
            }
            bound.query.exists(exec).await
        }

        /// Whether no record is related.
        pub async fn doesnt_exist<E: $crate::Executor + ?Sized>(
            self,
            exec: &E,
        ) -> $crate::Result<bool> {
            Ok(!self.exists(exec).await?)
        }

        /// How many records are related.
        pub async fn count<E: $crate::Executor + ?Sized>(self, exec: &E) -> $crate::Result<u64> {
            let bound = self.bound()?;
            if bound.empty {
                return Ok(0);
            }
            bound.query.count(exec).await
        }

        /// The sum of `column` over the related records (zero when there
        /// are none).
        pub async fn sum<T: $crate::Field + Default>(
            self,
            exec: &(impl $crate::Executor + ?Sized),
            column: impl $crate::IntoExpr,
        ) -> $crate::Result<T> {
            let bound = self.bound()?;
            if bound.empty {
                return Ok(T::default());
            }
            bound.query.sum(exec, column).await
        }

        /// The average of `column` over the related records.
        pub async fn avg<T: $crate::Field>(
            self,
            exec: &(impl $crate::Executor + ?Sized),
            column: impl $crate::IntoExpr,
        ) -> $crate::Result<Option<T>> {
            let bound = self.bound()?;
            if bound.empty {
                return Ok(None);
            }
            bound.query.avg(exec, column).await
        }

        /// The smallest value of `column` over the related records.
        pub async fn min<T: $crate::Field>(
            self,
            exec: &(impl $crate::Executor + ?Sized),
            column: impl $crate::IntoExpr,
        ) -> $crate::Result<Option<T>> {
            let bound = self.bound()?;
            if bound.empty {
                return Ok(None);
            }
            bound.query.min(exec, column).await
        }

        /// The largest value of `column` over the related records.
        pub async fn max<T: $crate::Field>(
            self,
            exec: &(impl $crate::Executor + ?Sized),
            column: impl $crate::IntoExpr,
        ) -> $crate::Result<Option<T>> {
            let bound = self.bound()?;
            if bound.empty {
                return Ok(None);
            }
            bound.query.max(exec, column).await
        }

        /// The value of `column` in the first related record.
        pub async fn value<E, C>(self, exec: &E, column: C) -> $crate::Result<Option<C::Output>>
        where
            E: $crate::Executor + ?Sized,
            C: $crate::Selectable,
        {
            let bound = self.bound()?;
            if bound.empty {
                return Ok(None);
            }
            bound.query.value(exec, column).await
        }

        /// The values of `column` in every related record, in order.
        pub async fn pluck<E, C>(self, exec: &E, column: C) -> $crate::Result<Vec<C::Output>>
        where
            E: $crate::Executor + ?Sized,
            C: $crate::Selectable,
        {
            let bound = self.bound()?;
            if bound.empty {
                return Ok(Vec::new());
            }
            bound.query.pluck(exec, column).await
        }

        /// Page `page` of the related records and their total.
        pub async fn paginate<E: $crate::Executor + ?Sized>(
            self,
            exec: &E,
            page: impl Into<$crate::PageRequest>,
        ) -> $crate::Result<$crate::Page<$crate::Record<R>>> {
            let bound = self.bound()?;
            let mut page = bound.query.clone().paginate(exec, page).await?;
            bound.finish(&mut page.data)?;
            Ok(page)
        }

        /// Page `page` of the related records, without a total.
        pub async fn simple_paginate<E: $crate::Executor + ?Sized>(
            self,
            exec: &E,
            page: impl Into<$crate::PageRequest>,
        ) -> $crate::Result<$crate::SimplePage<$crate::Record<R>>> {
            let bound = self.bound()?;
            let mut page = bound.query.clone().simple_paginate(exec, page).await?;
            bound.finish(&mut page.data)?;
            Ok(page)
        }

        /// The page of related records after (or before) a cursor.
        pub async fn cursor_paginate<E: $crate::Executor + ?Sized>(
            self,
            exec: &E,
            request: impl Into<$crate::CursorRequest>,
        ) -> $crate::Result<$crate::CursorPage<$crate::Record<R>>> {
            let bound = self.bound()?;
            let mut page = bound.query.clone().cursor_paginate(exec, request).await?;
            bound.finish(&mut page.data)?;
            Ok(page)
        }

        /// Sets columns on every related record in one statement (and their
        /// update timestamp); returns how many changed. No hooks run.
        pub async fn update<E: $crate::Executor + ?Sized>(
            self,
            exec: &E,
            assignments: impl IntoIterator<Item = $crate::Assignment>,
        ) -> $crate::Result<u64> {
            let bound = self.bound()?;
            if bound.empty {
                return Ok(0);
            }
            bound.query.update(exec, assignments).await
        }

        /// Deletes every related record in one statement (soft-deleting
        /// models set their deleted-at column); returns how many. No hooks
        /// run.
        pub async fn delete<E: $crate::Executor + ?Sized>(self, exec: &E) -> $crate::Result<u64> {
            let bound = self.bound()?;
            if bound.empty {
                return Ok(0);
            }
            bound.query.delete(exec).await
        }

        /// Adds `amount` to `column` on every related record in one
        /// statement.
        pub async fn increment<E, T>(
            self,
            exec: &E,
            column: $crate::Column<R, T>,
            amount: T,
        ) -> $crate::Result<u64>
        where
            E: $crate::Executor + ?Sized,
            T: $crate::Numeric,
        {
            let bound = self.bound()?;
            if bound.empty {
                return Ok(0);
            }
            bound.query.increment(exec, column, amount).await
        }

        /// Subtracts `amount` from `column` on every related record in one
        /// statement.
        pub async fn decrement<E, T>(
            self,
            exec: &E,
            column: $crate::Column<R, T>,
            amount: T,
        ) -> $crate::Result<u64>
        where
            E: $crate::Executor + ?Sized,
            T: $crate::Numeric,
        {
            let bound = self.bound()?;
            if bound.empty {
                return Ok(0);
            }
            bound.query.decrement(exec, column, amount).await
        }

        /// The relation's query for its parent, as a plain
        /// [`Builder`]($crate::Builder) (for methods relations do not
        /// forward). Records it reads are not finished: a many-to-many
        /// relation's pivot columns stay in their extras.
        pub fn into_query(self) -> $crate::Result<$crate::Builder<R>> {
            Ok(self.bound()?.query)
        }

        /// The statement the relation's query compiles to for `dialect`.
        pub fn to_sql(&self, dialect: $crate::Dialect) -> $crate::Result<$crate::Statement> {
            self.clone().bound()?.query.to_sql(dialect)
        }
    };
}

pub(crate) use relation_retrieval_methods;

/// Implements `HasWheres` and `HasQuery` (the traits behind the stamped
/// where and clause methods) for a relation type.
macro_rules! relation_builder_traits {
    ($relation:ident) => {
        impl<P: $crate::Model, R: $crate::Model> $crate::query::filters::HasWheres
            for $relation<P, R>
        {
            fn wheres_mut(&mut self) -> &mut Vec<$crate::ir::WhereClause> {
                &mut self.core.query.ir.wheres
            }
        }

        impl<P: $crate::Model, R: $crate::Model> $crate::query::clauses::HasQuery
            for $relation<P, R>
        {
            fn query_mut(&mut self) -> &mut $crate::ir::QueryIr {
                &mut self.core.query.ir
            }

            fn default_select(&self) -> Vec<$crate::ir::SelectItem> {
                $crate::query::model_columns::<R>(self.core.query.ir.table.qualifier())
            }
        }

        impl<P: $crate::Model, R: $crate::Model> $crate::Scoped<R> for $relation<P, R> {
            fn apply_scope(
                mut self,
                scope: impl FnOnce($crate::Builder<R>) -> $crate::Builder<R>,
            ) -> Self {
                self.core.query = self.core.query.scope(scope);
                self
            }
        }
    };
}

pub(crate) use relation_builder_traits;
