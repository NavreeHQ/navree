//! Running model queries: records, single values, plucks, existence and
//! aggregates, and [`Selectable`].

use std::borrow::Cow;
use std::hash::Hash;

use indexmap::IndexMap;

use crate::collection::Collection;
use crate::column::{Column, IntoExpr};
use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor, Target};
use crate::hooks::{Hooks, ModelEvent};
use crate::ir::{Aggregate, AggregateFunction, Expr, SelectItem};
use crate::model::{Model, PrimaryKey};
use crate::query::Builder;
use crate::record::Record;
use crate::row::{Row, decode_error};
use crate::value::{Field, FieldError, Value};

/// A column or expression whose values a query can return one by one
/// ([`value`](Builder::value), [`pluck`](Builder::pluck)), with the Rust
/// type they come back as: a typed [`Column<M, T>`] gives `T` (decoded as
/// the field is, casts included); a column name or an [`Expr`] gives a
/// [`Value`].
pub trait Selectable {
    /// The Rust type of the values.
    type Output;

    /// The selected expression.
    fn selected(&self) -> Expr;

    /// Reads the value of result column `column` of `row`.
    fn read(&self, row: &Row, column: &str) -> Result<Self::Output, FieldError>;
}

impl<M, T> Selectable for Column<M, T> {
    type Output = T;

    fn selected(&self) -> Expr {
        self.expr()
    }

    fn read(&self, row: &Row, column: &str) -> Result<T, FieldError> {
        self.decode_as(row, column)
    }
}

impl Selectable for &str {
    type Output = Value;

    fn selected(&self) -> Expr {
        self.into_expr()
    }

    fn read(&self, row: &Row, column: &str) -> Result<Value, FieldError> {
        row.field(column)
    }
}

impl Selectable for String {
    type Output = Value;

    fn selected(&self) -> Expr {
        self.as_str().into_expr()
    }

    fn read(&self, row: &Row, column: &str) -> Result<Value, FieldError> {
        row.field(column)
    }
}

impl Selectable for Expr {
    type Output = Value;

    fn selected(&self) -> Expr {
        self.clone()
    }

    fn read(&self, row: &Row, column: &str) -> Result<Value, FieldError> {
        row.field(column)
    }
}

/// The result column names of selected values (stable, whatever the
/// expression).
const VALUE: &str = "value";
const KEY: &str = "key";

impl Target<'_> {
    /// Records of model `M` read from `rows`, the way a query reads its
    /// own: stored records with the model's columns, the other columns as
    /// [extras](Record::extras), and the `Retrieved` hooks run. For rows a
    /// driver got outside a builder (a native MongoDB pipeline, say).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # use navree_orm::{Access, Executor, Row};
    /// # async fn demo(db: Db, rows: Vec<Row>) -> Result<()> {
    /// let target = db.target(Access::Read).await?;
    /// let deals = target.records::<Deal>(&rows).await?;
    /// # Ok(()) }
    /// ```
    pub async fn records<M: Model>(&self, rows: &[Row]) -> Result<Collection<M>> {
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            records.push(Record::<M>::from_row(row)?);
        }
        if !records.is_empty() {
            let ctx = self.context();
            let hooks = Hooks::<M>::resolve(&ctx).await?;
            if !hooks.is_empty() {
                for record in &mut records {
                    hooks.after(ModelEvent::Retrieved, &ctx, record).await?;
                }
            }
        }
        Ok(Collection::from(records))
    }
}

impl<M: Model> Builder<M> {
    /// Runs the query on `target` and reads its records (running the
    /// `Retrieved` hooks).
    pub(crate) async fn fetch(self, target: &Target<'_>) -> Result<Collection<M>> {
        let ir = self.into_ir();
        let rows = target.fetch(&ir).await?;
        target.records(&rows).await
    }

    /// The rows of the query with `items` as its select list (global scopes
    /// applied).
    async fn select_rows(self, target: &Target<'_>, items: Vec<SelectItem>) -> Result<Vec<Row>> {
        let mut builder = self.apply_global_scopes();
        builder.ir.columns = items;
        target.fetch(&builder.ir).await
    }

    /// The query's records.
    pub async fn get<E: Executor + ?Sized>(self, exec: &E) -> Result<Collection<M>> {
        let target = exec.target(Access::Read).await?;
        self.fetch(&target).await
    }

    /// The first record, if any.
    pub async fn first<E: Executor + ?Sized>(self, exec: &E) -> Result<Option<Record<M>>> {
        Ok(self.limit(1).get(exec).await?.into_vec().into_iter().next())
    }

    /// The first record, or [`OrmError::ModelNotFound`] (a 404 in Navree's
    /// error shape when a handler returns it).
    pub async fn first_or_fail<E: Executor + ?Sized>(self, exec: &E) -> Result<Record<M>> {
        self.first(exec)
            .await?
            .ok_or_else(|| OrmError::not_found(M::meta().name(), Vec::new()))
    }

    /// The only record the query matches: [`OrmError::ModelNotFound`] when
    /// there is none, [`OrmError::MultipleRecordsFound`] when there are
    /// more.
    pub async fn sole<E: Executor + ?Sized>(self, exec: &E) -> Result<Record<M>> {
        self.limit(2).get(exec).await?.sole()
    }

    /// The record with key `key`, if the query matches it.
    pub async fn find<E: Executor + ?Sized>(
        self,
        exec: &E,
        key: impl Into<M::Key>,
    ) -> Result<Option<Record<M>>> {
        self.where_key(key).first(exec).await
    }

    /// The record with key `key`, or [`OrmError::ModelNotFound`] naming the
    /// key (a 404 over HTTP).
    pub async fn find_or_fail<E: Executor + ?Sized>(
        self,
        exec: &E,
        key: impl Into<M::Key>,
    ) -> Result<Record<M>> {
        let key = key.into();
        let text = key.to_key_string();
        self.find(exec, key)
            .await?
            .ok_or_else(|| OrmError::not_found(M::meta().name(), vec![text]))
    }

    /// The records with these keys that the query matches, in the query's
    /// order (missing keys are left out).
    pub async fn find_many<E: Executor + ?Sized>(
        self,
        exec: &E,
        keys: impl IntoIterator<Item = impl Into<M::Key>>,
    ) -> Result<Collection<M>> {
        let keys: Vec<M::Key> = keys.into_iter().map(Into::into).collect();
        if keys.is_empty() {
            return Ok(Collection::new());
        }
        self.where_key_in(keys).get(exec).await
    }

    /// The value of `column` in the first matching row; `None` when no row
    /// matches.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db, id: Uuid) -> Result<()> {
    /// let title: Option<String> = Deal::query().where_key(id).value(&db, Deal::TITLE).await?;
    /// # Ok(()) }
    /// ```
    pub async fn value<E, C>(self, exec: &E, column: C) -> Result<Option<C::Output>>
    where
        E: Executor + ?Sized,
        C: Selectable,
    {
        let target = exec.target(Access::Read).await?;
        let item = SelectItem {
            expr: column.selected(),
            alias: Some(Cow::Borrowed(VALUE)),
        };
        let rows = self.limit(1).select_rows(&target, vec![item]).await?;
        match rows.first() {
            Some(row) => column.read(row, VALUE).map(Some).map_err(|error| {
                decode_error(M::meta().name(), VALUE, "the selected value".into(), &error)
            }),
            None => Ok(None),
        }
    }

    /// The values of `column` in every matching row, in order.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let titles: Vec<String> = Deal::query().latest().pluck(&db, Deal::TITLE).await?;
    /// # Ok(()) }
    /// ```
    pub async fn pluck<E, C>(self, exec: &E, column: C) -> Result<Vec<C::Output>>
    where
        E: Executor + ?Sized,
        C: Selectable,
    {
        let target = exec.target(Access::Read).await?;
        let item = SelectItem {
            expr: column.selected(),
            alias: Some(Cow::Borrowed(VALUE)),
        };
        let rows = self.select_rows(&target, vec![item]).await?;
        rows.iter()
            .map(|row| {
                column.read(row, VALUE).map_err(|error| {
                    decode_error(M::meta().name(), VALUE, "the selected value".into(), &error)
                })
            })
            .collect()
    }

    /// The values of `column` keyed by the values of `key`, in order (a
    /// later row with the same key replaces an earlier one, keeping its
    /// position).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let titles: IndexMap<Uuid, String> = Deal::query().pluck_keyed(&db, Deal::TITLE, Deal::ID).await?;
    /// # Ok(()) }
    /// ```
    pub async fn pluck_keyed<E, V, K>(
        self,
        exec: &E,
        column: V,
        key: K,
    ) -> Result<IndexMap<K::Output, V::Output>>
    where
        E: Executor + ?Sized,
        V: Selectable,
        K: Selectable,
        K::Output: Hash + Eq,
    {
        let target = exec.target(Access::Read).await?;
        let items = vec![
            SelectItem {
                expr: column.selected(),
                alias: Some(Cow::Borrowed(VALUE)),
            },
            SelectItem {
                expr: key.selected(),
                alias: Some(Cow::Borrowed(KEY)),
            },
        ];
        let rows = self.select_rows(&target, items).await?;
        let mut map = IndexMap::with_capacity(rows.len());
        for row in &rows {
            let value = column.read(row, VALUE).map_err(|error| {
                decode_error(M::meta().name(), VALUE, "the selected value".into(), &error)
            })?;
            let key = key.read(row, KEY).map_err(|error| {
                decode_error(M::meta().name(), KEY, "the selected key".into(), &error)
            })?;
            map.insert(key, value);
        }
        Ok(map)
    }

    /// Whether the query matches any record.
    pub async fn exists<E: Executor + ?Sized>(self, exec: &E) -> Result<bool> {
        let target = exec.target(Access::Read).await?;
        target.exists(&self.into_ir()).await
    }

    /// Whether the query matches no record.
    pub async fn doesnt_exist<E: Executor + ?Sized>(self, exec: &E) -> Result<bool> {
        Ok(!self.exists(exec).await?)
    }

    /// The number of matching records (of matching groups for a grouped
    /// query).
    pub async fn count<E: Executor + ?Sized>(self, exec: &E) -> Result<u64> {
        let target = exec.target(Access::Read).await?;
        self.count_on(&target).await
    }

    /// [`count`](Self::count) on `target`.
    pub(crate) async fn count_on(self, target: &Target<'_>) -> Result<u64> {
        self.aggregate_on::<u64>(target, AggregateFunction::Count, None, false)
            .await
            .map(Option::unwrap_or_default)
    }

    /// The number of matching records where `column` is not NULL.
    pub async fn count_column(
        self,
        exec: &(impl Executor + ?Sized),
        column: impl IntoExpr,
    ) -> Result<u64> {
        self.aggregate::<u64>(
            exec,
            AggregateFunction::Count,
            Some(column.into_expr()),
            false,
        )
        .await
        .map(Option::unwrap_or_default)
    }

    /// The number of distinct non-NULL values of `column`.
    pub async fn count_distinct(
        self,
        exec: &(impl Executor + ?Sized),
        column: impl IntoExpr,
    ) -> Result<u64> {
        self.aggregate::<u64>(
            exec,
            AggregateFunction::Count,
            Some(column.into_expr()),
            true,
        )
        .await
        .map(Option::unwrap_or_default)
    }

    /// The sum of `column` over the matching records (zero when none match).
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// let pipeline: i64 = Deal::query().where_(Deal::STAGE.eq(Stage::Open)).sum(&db, Deal::AMOUNT_CENTS).await?;
    /// # Ok(()) }
    /// ```
    pub async fn sum<T: Field + Default>(
        self,
        exec: &(impl Executor + ?Sized),
        column: impl IntoExpr,
    ) -> Result<T> {
        self.aggregate::<T>(
            exec,
            AggregateFunction::Sum,
            Some(column.into_expr()),
            false,
        )
        .await
        .map(Option::unwrap_or_default)
    }

    /// The average of `column`; `None` when no record matches. Read it as
    /// an `f64` or, exactly, as a [`Decimal`](crate::Decimal).
    pub async fn avg<T: Field>(
        self,
        exec: &(impl Executor + ?Sized),
        column: impl IntoExpr,
    ) -> Result<Option<T>> {
        self.aggregate::<T>(
            exec,
            AggregateFunction::Avg,
            Some(column.into_expr()),
            false,
        )
        .await
    }

    /// The smallest value of `column`; `None` when no record matches.
    pub async fn min<T: Field>(
        self,
        exec: &(impl Executor + ?Sized),
        column: impl IntoExpr,
    ) -> Result<Option<T>> {
        self.aggregate::<T>(
            exec,
            AggregateFunction::Min,
            Some(column.into_expr()),
            false,
        )
        .await
    }

    /// The largest value of `column`; `None` when no record matches.
    pub async fn max<T: Field>(
        self,
        exec: &(impl Executor + ?Sized),
        column: impl IntoExpr,
    ) -> Result<Option<T>> {
        self.aggregate::<T>(
            exec,
            AggregateFunction::Max,
            Some(column.into_expr()),
            false,
        )
        .await
    }

    /// An aggregate of the matching records read as `T`; `None` when the
    /// database returns NULL (no row matched, for everything but counts).
    pub async fn aggregate<T: Field>(
        self,
        exec: &(impl Executor + ?Sized),
        function: AggregateFunction,
        argument: Option<Expr>,
        distinct: bool,
    ) -> Result<Option<T>> {
        let target = exec.target(Access::Read).await?;
        self.aggregate_on(&target, function, argument, distinct)
            .await
    }

    async fn aggregate_on<T: Field>(
        self,
        target: &Target<'_>,
        function: AggregateFunction,
        argument: Option<Expr>,
        distinct: bool,
    ) -> Result<Option<T>> {
        let aggregate = Aggregate {
            function,
            arg: argument,
            distinct,
        };
        // Without the default columns: a grouped count selects only the
        // groups (PostgreSQL refuses ungrouped columns).
        let ir = self.apply_global_scopes().ir;
        let value = target.aggregate(&ir, &aggregate).await?;
        <Option<T>>::from_value(value).map_err(|error| {
            decode_error(
                M::meta().name(),
                function.sql_name(),
                Cow::Owned(T::type_name().into_owned()),
                &error,
            )
        })
    }
}
