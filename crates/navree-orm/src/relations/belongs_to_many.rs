//! [`BelongsToMany`]: records related through a pivot table (deals and
//! their tags through `deal_tag`), and its polymorphic forms
//! (`morph_to_many`, `morphed_by_many`).

use std::any::TypeId;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

use navree_kernel::BoxFuture;

use crate::collection::Collection;
use crate::column::{Assignment, Column, ColumnOf, Filter};
use crate::error::Result;
use crate::exec::{Access, Executor, Target};
use crate::ir::{
    Boolean, ColumnRef, Direction, Expr, Operator, Order, QueryIr, SelectItem, Where, WhereClause,
};
use crate::model::{Model, snake_case};
use crate::query::filters::HasWheres;
use crate::query::{Builder, TableBuilder, table};
use crate::record::Record;
use crate::relations::any::AnyRecord;
use crate::relations::core::{
    Bound, Finisher, RelationCore, column_value, column_values, constrain, distribute, matching,
    new_with, qualified, relation_query_methods, relation_retrieval_methods, restore_record,
    save_atomically,
};
use crate::relations::define::KeyColumn;
use crate::relations::eager::{EagerNode, EagerTree, RelationParts, constrained, prepare_query};
use crate::relations::existence::{ExistsRequest, build_subquery, columns_equal};
use crate::relations::has::{drop_row_numbers, limit_per_parent};
use crate::relations::keys::{RelationKey, distinct_keys, in_keys, in_keys_on, key_of};
use crate::relations::pivot::{
    PivotKind, PivotSpec, RelatedId, SyncChanges, assigned, canonical_id,
};
use crate::relations::registry::{ErasedRelation, RelationKind, RelationMeta};
use crate::relations::{RelatedTo, Relation, private};
use crate::value::{Numeric, Value, ValueFor, is_null};

pub use crate::relations::pivot::Pivot;

/// The prefix of the pivot columns a many-to-many query selects.
const PIVOT_PREFIX: &str = "pivot_";

/// Runs `$body` (an expression using `$t: &Target`) atomically: in a
/// transaction of its own when `$target` is a pool connection, else in the
/// transaction it already is.
macro_rules! atomically {
    ($target:expr, |$t:ident| $body:expr) => {{
        let target = $target;
        match target.begin_transaction().await? {
            Some(tx) => {
                let nested = target.on_transaction(&tx);
                let result = {
                    let $t = &nested;
                    $body.await
                };
                drop(nested);
                match result {
                    Ok(value) => {
                        tx.commit().await?;
                        Ok(value)
                    }
                    Err(error) => {
                        tx.rollback().await?;
                        Err(error)
                    }
                }
            }
            None => {
                let $t = &target;
                $body.await
            }
        }
    }};
}

/// The polymorphic part of a many-to-many relation: the pivot's type
/// column, and the name it holds.
#[derive(Clone, Debug)]
pub(crate) struct MorphPivot {
    pub(crate) type_column: Cow<'static, str>,
    /// The stored name: the parent's for `morph_to_many`, the related
    /// model's for `morphed_by_many`.
    pub(crate) name: Cow<'static, str>,
    pub(crate) inverse: bool,
}

/// The records of `R` related to the parent through rows of a pivot table:
/// a deal's tags through `deal_tag (deal_id, tag_id)`. Loaded, it holds a
/// [`Collection<R>`], each record carrying its pivot row under `pivot` (or
/// the name given with [`as_`](BelongsToMany::as_)): a [`Pivot`], or a
/// `Record<X>` of a typed pivot model (`.using::<X>()`).
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db, uow: Uow, deal: Record<Deal>) -> Result<()> {
/// // #[relations] impl Deal {
/// //     fn tags(&self) -> BelongsToMany<Deal, Tag> {
/// //         self.belongs_to_many("deal_tag").with_pivot(["role"]).with_timestamps()
/// //     }
/// // }
/// deal.tags().attach_with(&uow, [(1_i64, [col("role").set("owner")])]).await?;
/// let changes = deal.tags().sync(&uow, [1_i64, 2, 3]).await?;
/// let owners = deal.tags().where_pivot("role", "=", "owner").get(&db).await?;
/// # Ok(()) }
/// ```
pub struct BelongsToMany<P: Model, R: Model, X: PivotKind = Pivot> {
    pub(crate) core: RelationCore<P, R>,
    pub(crate) table: Cow<'static, str>,
    pub(crate) foreign_pivot_key: Cow<'static, str>,
    pub(crate) related_pivot_key: Cow<'static, str>,
    pub(crate) parent_key: Cow<'static, str>,
    pub(crate) related_key: Cow<'static, str>,
    pub(crate) morph: Option<MorphPivot>,
    pub(crate) pivot_columns: Vec<Cow<'static, str>>,
    pub(crate) pivot_wheres: Vec<WhereClause>,
    pub(crate) pivot_values: Vec<(Cow<'static, str>, Value)>,
    pub(crate) created_at: Option<Cow<'static, str>>,
    pub(crate) updated_at: Option<Cow<'static, str>>,
    pub(crate) accessor: Cow<'static, str>,
    pub(crate) chaperone: bool,
    pub(crate) _pivot: PhantomData<fn() -> X>,
}

/// A polymorphic [`BelongsToMany`] (`morph_to_many`, `morphed_by_many`):
/// its pivot rows also hold a morph type.
pub type MorphToMany<P, R, X = Pivot> = BelongsToMany<P, R, X>;

impl<P: Model, R: Model, X: PivotKind> Clone for BelongsToMany<P, R, X> {
    fn clone(&self) -> Self {
        BelongsToMany {
            core: self.core.clone(),
            table: self.table.clone(),
            foreign_pivot_key: self.foreign_pivot_key.clone(),
            related_pivot_key: self.related_pivot_key.clone(),
            parent_key: self.parent_key.clone(),
            related_key: self.related_key.clone(),
            morph: self.morph.clone(),
            pivot_columns: self.pivot_columns.clone(),
            pivot_wheres: self.pivot_wheres.clone(),
            pivot_values: self.pivot_values.clone(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            accessor: self.accessor.clone(),
            chaperone: self.chaperone,
            _pivot: PhantomData,
        }
    }
}

impl<P: Model, R: Model, X: PivotKind> fmt::Debug for BelongsToMany<P, R, X> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BelongsToMany")
            .field("parent", &P::meta().name())
            .field("related", &R::meta().name())
            .field("name", &self.core.name)
            .field("table", &self.table)
            .field("foreign_pivot_key", &self.foreign_pivot_key)
            .field("related_pivot_key", &self.related_pivot_key)
            .finish_non_exhaustive()
    }
}

impl<P: Model, R: Model, X: PivotKind> HasWheres for BelongsToMany<P, R, X> {
    fn wheres_mut(&mut self) -> &mut Vec<WhereClause> {
        &mut self.core.query.ir.wheres
    }
}

impl<P: Model, R: Model, X: PivotKind> crate::query::clauses::HasQuery for BelongsToMany<P, R, X> {
    fn query_mut(&mut self) -> &mut QueryIr {
        &mut self.core.query.ir
    }

    fn default_select(&self) -> Vec<SelectItem> {
        crate::query::model_columns::<R>(self.core.query.ir.table.qualifier())
    }
}

impl<P: Model, R: Model, X: PivotKind> crate::Scoped<R> for BelongsToMany<P, R, X> {
    fn apply_scope(mut self, scope: impl FnOnce(Builder<R>) -> Builder<R>) -> Self {
        self.core.query = self.core.query.scope(scope);
        self
    }
}

impl<P: Model, R: Model, X: PivotKind> RelationParts<P, R> for BelongsToMany<P, R, X> {
    fn core_mut(&mut self) -> &mut RelationCore<P, R> {
        &mut self.core
    }
}

/// The default name of a pivot table: both models' snake_case names in
/// alphabetical order, joined by `_` (`Deal` and `Tag` → `deal_tag`).
///
/// ```
/// # use navree_orm::__doc_models::*;
/// assert_eq!(navree_orm::relations::joining_table::<Tag, Deal>(), "deal_tag");
/// ```
pub fn joining_table<A: Model, B: Model>() -> String {
    let mut names = [snake_case(A::meta().name()), snake_case(B::meta().name())];
    names.sort();
    names.join("_")
}

impl<P: Model, R: Model> BelongsToMany<P, R, Pivot> {
    /// The relation of `parent` through pivot table `table`, keyed by both
    /// models' foreign key names, named after `R`'s table.
    pub(crate) fn new(parent: Option<P>, table: Cow<'static, str>) -> Self {
        BelongsToMany {
            core: RelationCore::new(parent, Cow::Borrowed(R::meta().table())),
            table,
            foreign_pivot_key: P::foreign_key_name(),
            related_pivot_key: R::foreign_key_name(),
            parent_key: Cow::Borrowed(P::meta().key_column()),
            related_key: Cow::Borrowed(R::meta().key_column()),
            morph: None,
            pivot_columns: Vec::new(),
            pivot_wheres: Vec::new(),
            pivot_values: Vec::new(),
            created_at: None,
            updated_at: None,
            accessor: Cow::Borrowed("pivot"),
            chaperone: false,
            _pivot: PhantomData,
        }
    }

    /// The polymorphic relation named `name` through `table`: the parent's
    /// side (`morph_to_many`), or the related model's (`morphed_by_many`,
    /// `inverse`).
    pub(crate) fn new_morph(
        parent: Option<P>,
        name: &str,
        table: Cow<'static, str>,
        inverse: bool,
    ) -> Self {
        let mut relation = BelongsToMany::new(parent, table);
        let id_column = Cow::Owned(format!("{name}_id"));
        if inverse {
            relation.related_pivot_key = id_column;
        } else {
            relation.foreign_pivot_key = id_column;
        }
        relation.morph = Some(MorphPivot {
            type_column: Cow::Owned(format!("{name}_type")),
            name: if inverse {
                R::morph_name()
            } else {
                P::morph_name()
            },
            inverse,
        });
        relation
    }
}

impl<P: Model, R: Model, X: PivotKind> BelongsToMany<P, R, X> {
    relation_query_methods!();
    relation_retrieval_methods!();

    /// The pivot column holding the parent's key, when it is not
    /// `{parent}_id`.
    pub fn foreign_pivot_key(mut self, column: impl Into<Cow<'static, str>>) -> Self {
        self.foreign_pivot_key = column.into();
        self
    }

    /// The pivot column holding the related record's key, when it is not
    /// `{related}_id`.
    pub fn related_pivot_key(mut self, column: impl Into<Cow<'static, str>>) -> Self {
        self.related_pivot_key = column.into();
        self
    }

    /// The parent's column the pivot points at, when it is not its key.
    pub fn parent_key(mut self, column: impl KeyColumn<P>) -> Self {
        self.parent_key = column.key_column();
        self
    }

    /// The related model's column the pivot points at, when it is not its
    /// key.
    pub fn related_key(mut self, column: impl KeyColumn<R>) -> Self {
        self.related_key = column.key_column();
        self
    }

    /// Loads these pivot columns with the related records.
    pub fn with_pivot<C: Into<Cow<'static, str>>>(
        mut self,
        columns: impl IntoIterator<Item = C>,
    ) -> Self {
        for column in columns {
            let column = column.into();
            if !self.pivot_columns.contains(&column) {
                self.pivot_columns.push(column);
            }
        }
        self
    }

    /// The pivot table keeps creation and update timestamps (the parent's
    /// timestamp column names, `created_at` and `updated_at` by default):
    /// attaching sets both, updating a pivot row sets the update one, and
    /// both are loaded.
    pub fn with_timestamps(self) -> Self {
        let created = P::meta().created_at().unwrap_or("created_at");
        let updated = P::meta().updated_at().unwrap_or("updated_at");
        self.with_timestamps_named(Some(created), Some(updated))
    }

    /// The pivot table keeps the timestamps `created` and `updated` (either
    /// may be `None`).
    pub fn with_timestamps_named(
        mut self,
        created: Option<impl Into<Cow<'static, str>>>,
        updated: Option<impl Into<Cow<'static, str>>>,
    ) -> Self {
        self.created_at = created.map(Into::into);
        self.updated_at = updated.map(Into::into);
        let columns: Vec<Cow<'static, str>> = [self.created_at.clone(), self.updated_at.clone()]
            .into_iter()
            .flatten()
            .collect();
        self.with_pivot(columns)
    }

    /// The name related records carry their pivot under (`pivot` by
    /// default; it is also the name they serialize it under).
    pub fn as_(mut self, accessor: impl Into<Cow<'static, str>>) -> Self {
        self.accessor = accessor.into();
        self
    }

    /// Pivot rows are records of the typed pivot model `M` (its table is the
    /// pivot table): its casts apply to loaded pivots, and attaching,
    /// updating and detaching go through its saves and deletes, so its
    /// hooks and events run.
    pub fn using<M: Model + Default>(self) -> BelongsToMany<P, R, M> {
        BelongsToMany {
            core: self.core,
            table: self.table,
            foreign_pivot_key: self.foreign_pivot_key,
            related_pivot_key: self.related_pivot_key,
            parent_key: self.parent_key,
            related_key: self.related_key,
            morph: self.morph,
            pivot_columns: self.pivot_columns,
            pivot_wheres: self.pivot_wheres,
            pivot_values: self.pivot_values,
            created_at: self.created_at,
            updated_at: self.updated_at,
            accessor: self.accessor,
            chaperone: self.chaperone,
            _pivot: PhantomData,
        }
    }

    /// Loaded pivots keep snapshots of the parent and the related record
    /// (without their relations): `pivot.parent::<Deal>()`,
    /// `pivot.related::<Tag>()`; a typed pivot record gets them as
    /// relations named after the two models (`deal`, `tag`).
    pub fn chaperone_pivot(mut self) -> Self {
        self.chaperone = true;
        self
    }

    /// Pivots keep no snapshots.
    pub fn without_chaperone_pivot(mut self) -> Self {
        self.chaperone = false;
        self
    }

    /// `column` of the pivot table: a name is qualified by it, a typed
    /// column keeps its table.
    pub fn qualify_pivot_column(&self, column: &str) -> String {
        if column.contains('.') {
            column.to_owned()
        } else {
            format!("{}.{column}", self.table)
        }
    }

    fn pivot_column_ref(&self, column: ColumnRef) -> ColumnRef {
        if column.table.is_some() {
            column
        } else {
            ColumnRef {
                table: Some(self.table.clone()),
                ..column
            }
        }
    }

    /// Adds a condition on the pivot table (to the related query, and to
    /// the pivot queries of `detach`, `sync` and `toggle`).
    fn push_pivot_where(mut self, boolean: Boolean, negated: bool, filter: Where) -> Self {
        self.pivot_wheres.push(WhereClause {
            boolean,
            negated,
            filter: filter.clone(),
        });
        self.push_where(boolean, negated, filter);
        self
    }

    fn pivot_compare<T>(
        &self,
        column: impl ColumnOf<T>,
        op: impl Into<Operator>,
        value: impl ValueFor<T>,
    ) -> Where {
        let column_ref = self.pivot_column_ref(column.column_ref());
        match column.encode_value(&value.into_value_for()) {
            Ok(value) => Filter::compare(Expr::Column(column_ref), op.into(), Expr::Value(value))
                .into_where(),
            Err(error) => Where::Invalid(format!(
                "cannot use a value for pivot column `{column_ref}`: {error}"
            )),
        }
    }

    /// Only related records whose pivot `column` compares to `value` with
    /// `op`: `where_pivot("role", "=", "owner")`, or a typed pivot model's
    /// column.
    pub fn where_pivot<T>(
        self,
        column: impl ColumnOf<T>,
        op: impl Into<Operator>,
        value: impl ValueFor<T>,
    ) -> Self {
        let filter = self.pivot_compare(column, op, value);
        self.push_pivot_where(Boolean::And, false, filter)
    }

    /// `OR` related records whose pivot `column` compares to `value`.
    pub fn or_where_pivot<T>(
        self,
        column: impl ColumnOf<T>,
        op: impl Into<Operator>,
        value: impl ValueFor<T>,
    ) -> Self {
        let filter = self.pivot_compare(column, op, value);
        self.push_pivot_where(Boolean::Or, false, filter)
    }

    fn pivot_in<T>(
        &self,
        column: impl ColumnOf<T>,
        values: impl IntoIterator<Item = impl ValueFor<T>>,
        not: bool,
    ) -> Where {
        let qualified = PivotColumn {
            column,
            table: &self.table,
        };
        crate::query::filters::in_list(&qualified, values, not).into_where()
    }

    /// Only related records whose pivot `column` is one of `values`.
    pub fn where_pivot_in<T>(
        self,
        column: impl ColumnOf<T>,
        values: impl IntoIterator<Item = impl ValueFor<T>>,
    ) -> Self {
        let filter = self.pivot_in(column, values, false);
        self.push_pivot_where(Boolean::And, false, filter)
    }

    /// `OR` related records whose pivot `column` is one of `values`.
    pub fn or_where_pivot_in<T>(
        self,
        column: impl ColumnOf<T>,
        values: impl IntoIterator<Item = impl ValueFor<T>>,
    ) -> Self {
        let filter = self.pivot_in(column, values, false);
        self.push_pivot_where(Boolean::Or, false, filter)
    }

    /// Only related records whose pivot `column` is none of `values`.
    pub fn where_pivot_not_in<T>(
        self,
        column: impl ColumnOf<T>,
        values: impl IntoIterator<Item = impl ValueFor<T>>,
    ) -> Self {
        let filter = self.pivot_in(column, values, true);
        self.push_pivot_where(Boolean::And, false, filter)
    }

    /// `OR` related records whose pivot `column` is none of `values`.
    pub fn or_where_pivot_not_in<T>(
        self,
        column: impl ColumnOf<T>,
        values: impl IntoIterator<Item = impl ValueFor<T>>,
    ) -> Self {
        let filter = self.pivot_in(column, values, true);
        self.push_pivot_where(Boolean::Or, false, filter)
    }

    fn pivot_between<T>(
        &self,
        column: impl ColumnOf<T>,
        low: impl ValueFor<T>,
        high: impl ValueFor<T>,
        not: bool,
    ) -> Where {
        let qualified = PivotColumn {
            column,
            table: &self.table,
        };
        crate::query::filters::between(&qualified, low.into_value_for(), high.into_value_for(), not)
            .into_where()
    }

    /// Only related records whose pivot `column` is between `low` and
    /// `high`.
    pub fn where_pivot_between<T>(
        self,
        column: impl ColumnOf<T>,
        low: impl ValueFor<T>,
        high: impl ValueFor<T>,
    ) -> Self {
        let filter = self.pivot_between(column, low, high, false);
        self.push_pivot_where(Boolean::And, false, filter)
    }

    /// `OR` related records whose pivot `column` is between `low` and
    /// `high`.
    pub fn or_where_pivot_between<T>(
        self,
        column: impl ColumnOf<T>,
        low: impl ValueFor<T>,
        high: impl ValueFor<T>,
    ) -> Self {
        let filter = self.pivot_between(column, low, high, false);
        self.push_pivot_where(Boolean::Or, false, filter)
    }

    /// Only related records whose pivot `column` is not between `low` and
    /// `high`.
    pub fn where_pivot_not_between<T>(
        self,
        column: impl ColumnOf<T>,
        low: impl ValueFor<T>,
        high: impl ValueFor<T>,
    ) -> Self {
        let filter = self.pivot_between(column, low, high, true);
        self.push_pivot_where(Boolean::And, false, filter)
    }

    /// `OR` related records whose pivot `column` is not between `low` and
    /// `high`.
    pub fn or_where_pivot_not_between<T>(
        self,
        column: impl ColumnOf<T>,
        low: impl ValueFor<T>,
        high: impl ValueFor<T>,
    ) -> Self {
        let filter = self.pivot_between(column, low, high, true);
        self.push_pivot_where(Boolean::Or, false, filter)
    }

    fn pivot_null(&self, column: &str, not: bool) -> Where {
        Where::Null {
            expr: Expr::Column(self.pivot_column_ref(ColumnRef::parse(column.to_owned()))),
            not,
        }
    }

    /// Only related records whose pivot `column` is NULL.
    pub fn where_pivot_null(self, column: &str) -> Self {
        let filter = self.pivot_null(column, false);
        self.push_pivot_where(Boolean::And, false, filter)
    }

    /// `OR` related records whose pivot `column` is NULL.
    pub fn or_where_pivot_null(self, column: &str) -> Self {
        let filter = self.pivot_null(column, false);
        self.push_pivot_where(Boolean::Or, false, filter)
    }

    /// Only related records whose pivot `column` is not NULL.
    pub fn where_pivot_not_null(self, column: &str) -> Self {
        let filter = self.pivot_null(column, true);
        self.push_pivot_where(Boolean::And, false, filter)
    }

    /// `OR` related records whose pivot `column` is not NULL.
    pub fn or_where_pivot_not_null(self, column: &str) -> Self {
        let filter = self.pivot_null(column, true);
        self.push_pivot_where(Boolean::Or, false, filter)
    }

    /// Only related records whose pivot `column` holds `value`, and new
    /// pivot rows (attached, created) get it.
    pub fn with_pivot_value(
        mut self,
        column: impl Into<Cow<'static, str>>,
        value: impl Into<Value>,
    ) -> Self {
        let column = column.into();
        let value = value.into();
        if is_null(&value) {
            return self.with_error(format!(
                "with_pivot_value(`{column}`) needs a value, not NULL (use where_pivot_null)"
            ));
        }
        self.pivot_values.push((column.clone(), value.clone()));
        self.where_pivot(column.into_owned(), "=", value)
    }

    /// Orders the related records by pivot `column`, ascending.
    pub fn order_by_pivot(self, column: &str) -> Self {
        let column = self.pivot_column_ref(ColumnRef::parse(column.to_owned()));
        self.order(Order::By {
            expr: Expr::Column(column),
            direction: Direction::Asc,
            nulls: None,
        })
    }

    /// Orders the related records by pivot `column`, descending.
    pub fn order_by_pivot_desc(self, column: &str) -> Self {
        let column = self.pivot_column_ref(ColumnRef::parse(column.to_owned()));
        self.order(Order::By {
            expr: Expr::Column(column),
            direction: Direction::Desc,
            nulls: None,
        })
    }

    fn with_error(mut self, message: String) -> Self {
        self.core.query = self.core.query.with_error(message);
        self
    }

    /// The pivot table.
    pub fn pivot_table(&self) -> &str {
        &self.table
    }

    /// The pivot column holding the parent's key.
    pub fn foreign_pivot_key_name(&self) -> &str {
        &self.foreign_pivot_key
    }

    /// The pivot column holding the parent's key, qualified.
    pub fn qualified_foreign_pivot_key_name(&self) -> String {
        format!("{}.{}", self.table, self.foreign_pivot_key)
    }

    /// The pivot column holding the related record's key.
    pub fn related_pivot_key_name(&self) -> &str {
        &self.related_pivot_key
    }

    /// The pivot column holding the related record's key, qualified.
    pub fn qualified_related_pivot_key_name(&self) -> String {
        format!("{}.{}", self.table, self.related_pivot_key)
    }

    /// The parent's column the pivot points at.
    pub fn parent_key_name(&self) -> &str {
        &self.parent_key
    }

    /// The related model's column the pivot points at.
    pub fn related_key_name(&self) -> &str {
        &self.related_key
    }

    /// The pivot columns loaded besides the two keys.
    pub fn pivot_columns(&self) -> Vec<&str> {
        self.pivot_columns.iter().map(AsRef::as_ref).collect()
    }

    /// Whether pivot column `column` is loaded.
    pub fn has_pivot_column(&self, column: &str) -> bool {
        self.pivot_columns.iter().any(|existing| existing == column)
    }

    /// The name related records carry their pivot under.
    pub fn pivot_accessor(&self) -> &str {
        &self.accessor
    }

    /// The pivot's morph type column (polymorphic relations).
    pub fn morph_type(&self) -> Option<&str> {
        self.morph.as_ref().map(|morph| morph.type_column.as_ref())
    }

    /// The name the pivot's morph type column holds (polymorphic
    /// relations).
    pub fn morph_name(&self) -> Option<&str> {
        self.morph.as_ref().map(|morph| morph.name.as_ref())
    }

    /// What the pivot rows know of the relation.
    pub(crate) fn spec(&self) -> PivotSpec {
        PivotSpec {
            table: self.table.clone(),
            foreign_key: self.foreign_pivot_key.clone(),
            related_key: self.related_pivot_key.clone(),
            morph: self
                .morph
                .as_ref()
                .map(|morph| (morph.type_column.clone(), morph.name.to_string())),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
        }
    }

    /// The pivot columns selected with the related records, as
    /// `pivot_{column}`.
    fn pivot_select(&self) -> Vec<SelectItem> {
        let mut columns: Vec<Cow<'static, str>> = vec![
            self.foreign_pivot_key.clone(),
            self.related_pivot_key.clone(),
        ];
        if let Some(morph) = &self.morph {
            columns.push(morph.type_column.clone());
        }
        columns.extend(self.pivot_columns.iter().cloned());
        columns.extend(X::columns().into_iter().map(Cow::Borrowed));
        let mut seen: Vec<Cow<'static, str>> = Vec::new();
        columns
            .into_iter()
            .filter(|column| {
                if seen.contains(column) {
                    false
                } else {
                    seen.push(column.clone());
                    true
                }
            })
            .map(|column| SelectItem {
                expr: Expr::Column(qualified(&self.table, &column)),
                alias: Some(Cow::Owned(format!("{PIVOT_PREFIX}{column}"))),
            })
            .collect()
    }

    /// `query` joined with the pivot table, selecting the pivot columns.
    fn joined(&self, query: Builder<R>) -> Builder<R> {
        let related = qualified(R::meta().table(), &self.related_key);
        let pivot = qualified(&self.table, &self.related_pivot_key);
        let pivot_select = self.pivot_select();
        query
            .join(&self.table, related, "=", pivot)
            .add_select(pivot_select)
    }

    /// The pivot's morph condition.
    fn morph_condition(&self) -> Option<Where> {
        self.morph.as_ref().map(|morph| Where::Compare {
            left: Expr::Column(qualified(&self.table, &morph.type_column)),
            op: Operator::Eq,
            right: Expr::Value(Value::from(morph.name.to_string())),
        })
    }

    /// Takes the pivot columns out of the records' extras into their pivot
    /// rows; returns each record's parent key.
    fn hydrate(&self, records: &mut [Record<R>]) -> Result<Vec<Option<RelationKey>>> {
        let spec = self.spec();
        let foreign = format!("{PIVOT_PREFIX}{}", self.foreign_pivot_key);
        let mut parents = Vec::with_capacity(records.len());
        for record in records.iter_mut() {
            let mut values = Vec::new();
            let mut parent = None;
            record
                .extras
                .retain(|(column, value)| match column.strip_prefix(PIVOT_PREFIX) {
                    Some(name) => {
                        if *column == foreign {
                            parent = key_of(value);
                        }
                        values.push((name.to_owned(), value.clone()));
                        false
                    }
                    None => true,
                });
            let row = X::row(&spec, values, true)?;
            record.relations.set_relation(self.accessor.clone(), row);
            parents.push(parent);
        }
        Ok(parents)
    }

    /// Gives `record`'s pivot snapshots of `parent` and of the record.
    fn chaperone_one(&self, parent: &AnyRecord, record: &mut Record<R>) {
        let related = AnyRecord::new(record.without_relations());
        if let Some(row) = record.relations.get_mut::<X::Row>(&self.accessor) {
            X::chaperone(row, parent.clone(), related);
        }
    }

    /// The relation's query for its parent.
    pub(crate) fn bound(self) -> Result<Bound<R>> {
        let parent = self.core.parent()?.clone();
        let value = column_value(&parent, &self.parent_key, &self.core.name)?;
        let query = self.joined(self.core.query.clone());
        if is_null(&value) {
            return Ok(Bound::nothing(query));
        }
        let mut conditions = vec![Where::Compare {
            left: Expr::Column(qualified(&self.table, &self.foreign_pivot_key)),
            op: Operator::Eq,
            right: Expr::Value(value),
        }];
        conditions.extend(self.morph_condition());
        let relation = self.clone();
        let snapshot = AnyRecord::new(Record::stored(parent));
        let finish: Finisher<R> = Box::new(move |records: &mut [Record<R>]| {
            relation.hydrate(records)?;
            if relation.chaperone {
                for record in records.iter_mut() {
                    relation.chaperone_one(&snapshot, record);
                }
            }
            Ok(())
        });
        Ok(Bound::new(constrain(query, conditions)).finishing(Some(finish)))
    }

    /// The pivot rows of the parent: the parent's key, the morph type and
    /// the pivot conditions.
    pub fn pivot_query(&self) -> Result<TableBuilder> {
        let parent = self.core.parent()?;
        let value = column_value(parent, &self.parent_key, &self.core.name)?;
        let mut rows = table(&self.table).where_(Filter::from(Where::Compare {
            left: Expr::Column(qualified(&self.table, &self.foreign_pivot_key)),
            op: Operator::Eq,
            right: Expr::Value(value),
        }));
        if let Some(morph) = self.morph_condition() {
            rows = rows.where_(Filter::from(morph));
        }
        if !self.pivot_wheres.is_empty() {
            let clauses = self.pivot_wheres.clone();
            rows = rows.where_(Filter::from(Where::Group(clauses)));
        }
        Ok(rows)
    }

    /// The pivot rows of the parent and the related records `ids`.
    pub fn pivot_query_for<I: RelatedId<R>>(
        &self,
        ids: impl IntoIterator<Item = I>,
    ) -> Result<TableBuilder> {
        let ids = self.related_ids(ids)?;
        let keys = distinct_keys(ids);
        Ok(self.pivot_query()?.where_(Filter::from(in_keys(
            qualified(&self.table, &self.related_pivot_key),
            &keys,
        ))))
    }

    /// A new, unsaved pivot row for the parent with `values` (and the
    /// relation's pivot values and morph type).
    pub fn new_pivot(&self, values: impl IntoIterator<Item = Assignment>) -> Result<X::Row> {
        let row = self.pivot_row_values(None, assigned(values)?)?;
        X::row(&self.spec(), row, false)
    }

    /// A pivot row for the parent with `values`, marked stored (to update
    /// or delete a row whose keys are known).
    pub fn new_existing_pivot(
        &self,
        values: impl IntoIterator<Item = Assignment>,
    ) -> Result<X::Row> {
        let row = self.pivot_row_values(None, assigned(values)?)?;
        X::row(&self.spec(), row, true)
    }

    /// The columns of a pivot row for related id `id`: the two keys, the
    /// morph type, the relation's pivot values, then `values`.
    fn pivot_row_values(
        &self,
        id: Option<Value>,
        values: Vec<(String, Value)>,
    ) -> Result<Vec<(String, Value)>> {
        let parent = self.core.parent()?;
        let mut row: Vec<(String, Value)> = Vec::new();
        if let Some(id) = id {
            row.push((self.related_pivot_key.to_string(), id));
        }
        row.push((
            self.foreign_pivot_key.to_string(),
            column_value(parent, &self.parent_key, &self.core.name)?,
        ));
        if let Some(morph) = &self.morph {
            let name = if morph.inverse {
                crate::relations::morph_map::type_name_of::<R>()?
            } else {
                crate::relations::morph_map::type_name_of::<P>()?
            };
            row.push((
                morph.type_column.to_string(),
                Value::from(name.into_owned()),
            ));
        }
        for (column, value) in &self.pivot_values {
            row.push((column.to_string(), value.clone()));
        }
        for (column, value) in values {
            match row.iter_mut().find(|(existing, _)| *existing == column) {
                Some(existing) => existing.1 = value,
                None => row.push((column, value)),
            }
        }
        Ok(row)
    }

    /// The related key values of `ids`.
    fn related_ids<I: RelatedId<R>>(&self, ids: impl IntoIterator<Item = I>) -> Result<Vec<Value>> {
        ids.into_iter()
            .map(|id| {
                id.related_value(&self.related_key)
                    .map(|value| canonical_id::<R>(&self.related_key, value))
            })
            .collect()
    }

    /// The related keys of the parent's pivot rows.
    async fn current_ids(&self, target: &Target<'_>) -> Result<Vec<Value>> {
        let rows = self.pivot_query()?.select([format!(
            "{}.{} as value",
            self.table, self.related_pivot_key
        )]);
        let rows = target.fetch(&rows.into_ir()).await?;
        rows.iter()
            .map(|row| {
                row.value("value")
                    .map(|value| canonical_id::<R>(&self.related_key, value))
            })
            .collect()
    }

    /// The keys of every related record (from the pivot table).
    pub async fn all_related_ids<E: Executor + ?Sized>(&self, exec: &E) -> Result<Vec<Value>> {
        let target = exec.target(Access::Read).await?;
        self.current_ids(&target).await
    }

    /// Inserts the pivot rows of `rows` (related id and values).
    async fn attach_on(
        &self,
        target: &Target<'_>,
        rows: Vec<(Value, Vec<(String, Value)>)>,
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let now = Value::ChronoDateTimeUtc(Some(target.context().now().await));
        let spec = self.spec();
        let mut records = Vec::with_capacity(rows.len());
        for (id, values) in rows {
            let mut row = self.pivot_row_values(Some(id), Vec::new())?;
            for column in [&self.created_at, &self.updated_at].into_iter().flatten() {
                if !row.iter().any(|(existing, _)| existing == column.as_ref()) {
                    row.push((column.to_string(), now.clone()));
                }
            }
            for (column, value) in values {
                match row.iter_mut().find(|(existing, _)| *existing == column) {
                    Some(existing) => existing.1 = value,
                    None => row.push((column, value)),
                }
            }
            records.push(row);
        }
        X::insert(target, &spec, records).await
    }

    /// Deletes the parent's pivot rows of `ids` (all of them for `None`).
    async fn detach_on(&self, target: &Target<'_>, ids: Option<Vec<Value>>) -> Result<u64> {
        let rows = match ids {
            Some(ids) if ids.is_empty() => return Ok(0),
            Some(ids) => {
                let keys = distinct_keys(ids);
                self.pivot_query()?.where_(Filter::from(in_keys(
                    qualified(&self.table, &self.related_pivot_key),
                    &keys,
                )))
            }
            None => self.pivot_query()?,
        };
        X::delete(target, rows).await
    }

    /// Sets `values` (and the update timestamp) on the pivot row of `id`.
    async fn update_existing_on(
        &self,
        target: &Target<'_>,
        id: Value,
        mut values: Vec<(String, Value)>,
    ) -> Result<u64> {
        if let Some(updated_at) = &self.updated_at
            && !values
                .iter()
                .any(|(column, _)| column == updated_at.as_ref())
        {
            let now = Value::ChronoDateTimeUtc(Some(target.context().now().await));
            values.push((updated_at.to_string(), now));
        }
        let keys = distinct_keys([id]);
        let rows = self.pivot_query()?.where_(Filter::from(in_keys(
            qualified(&self.table, &self.related_pivot_key),
            &keys,
        )));
        X::update(target, &self.spec(), rows, values).await
    }

    /// Touches the parent when the related model touches its inverse
    /// relation (named after the parent's table), and the related records
    /// when the parent touches this relation.
    async fn touch_if_touching(&self, target: &Target<'_>) -> Result<()> {
        if R::touched_relations().contains(&P::meta().table()) {
            let mut parent = Record::stored(self.core.parent()?.clone());
            crate::persist::touch(target, &mut parent, None).await?;
        }
        if P::touched_relations().contains(&self.core.name.as_ref()) {
            self.touch_on(target).await?;
        }
        Ok(())
    }

    /// Sets the update timestamp of every related record to now.
    async fn touch_on(&self, target: &Target<'_>) -> Result<u64> {
        if R::meta().updated_at().is_none() || crate::relations::touch::is_ignoring_touch_of::<R>()
        {
            return Ok(0);
        }
        let ids = self.current_ids(target).await?;
        if ids.is_empty() {
            return Ok(0);
        }
        let keys = distinct_keys(ids);
        let query = Builder::<R>::new()
            .without_global_scopes()
            .where_(Filter::from(in_keys(
                qualified(R::meta().table(), &self.related_key),
                &keys,
            )));
        crate::relations::touch::touch_related(Some(query), target).await
    }

    /// Sets the update timestamp of every related record to now (no hooks
    /// run).
    pub async fn touch<E: Executor + ?Sized>(&self, exec: &E) -> Result<u64> {
        let target = exec.target(Access::Write).await?;
        self.touch_on(&target).await
    }

    /// Attaches the related records `ids` (keys, or records): inserts their
    /// pivot rows, with the relation's pivot values and timestamps. A typed
    /// pivot model's rows are saved one by one, so its hooks run.
    pub async fn attach<E, I>(&self, exec: &E, ids: impl IntoIterator<Item = I>) -> Result<()>
    where
        E: Executor + ?Sized,
        I: RelatedId<R>,
    {
        let rows: Vec<(Value, Vec<(String, Value)>)> = self
            .related_ids(ids)?
            .into_iter()
            .map(|id| (id, Vec::new()))
            .collect();
        let target = exec.target(Access::Write).await?;
        atomically!(target, |t| async {
            self.attach_on(t, rows).await?;
            self.touch_if_touching(t).await
        })
    }

    /// Attaches related records with pivot values of their own:
    /// `attach_with(&uow, [(tag_id, [col("role").set("owner")])])`.
    pub async fn attach_with<E, I, V>(
        &self,
        exec: &E,
        rows: impl IntoIterator<Item = (I, V)>,
    ) -> Result<()>
    where
        E: Executor + ?Sized,
        I: RelatedId<R>,
        V: IntoIterator<Item = Assignment>,
    {
        let mut prepared = Vec::new();
        for (id, values) in rows {
            let id = canonical_id::<R>(&self.related_key, id.related_value(&self.related_key)?);
            prepared.push((id, assigned(values)?));
        }
        let target = exec.target(Access::Write).await?;
        atomically!(target, |t| async {
            self.attach_on(t, prepared).await?;
            self.touch_if_touching(t).await
        })
    }

    /// Detaches the related records `ids`: deletes their pivot rows of the
    /// parent; returns how many.
    pub async fn detach<E, I>(&self, exec: &E, ids: impl IntoIterator<Item = I>) -> Result<u64>
    where
        E: Executor + ?Sized,
        I: RelatedId<R>,
    {
        let ids = self.related_ids(ids)?;
        let target = exec.target(Access::Write).await?;
        atomically!(target, |t| async {
            let detached = self.detach_on(t, Some(ids)).await?;
            self.touch_if_touching(t).await?;
            Ok(detached)
        })
    }

    /// Detaches every related record (the pivot conditions apply); returns
    /// how many rows went.
    pub async fn detach_all<E: Executor + ?Sized>(&self, exec: &E) -> Result<u64> {
        let target = exec.target(Access::Write).await?;
        atomically!(target, |t| async {
            let detached = self.detach_on(t, None).await?;
            self.touch_if_touching(t).await?;
            Ok(detached)
        })
    }

    async fn sync_on(
        &self,
        target: &Target<'_>,
        records: Vec<(Value, Vec<(String, Value)>)>,
        detaching: bool,
    ) -> Result<SyncChanges> {
        let mut changes = SyncChanges::default();
        if records.is_empty() && !detaching {
            return Ok(changes);
        }
        let current = self.current_ids(target).await?;
        let current_keys: Vec<Option<RelationKey>> = current.iter().map(key_of).collect();
        let wanted: Vec<Option<RelationKey>> = records.iter().map(|(id, _)| key_of(id)).collect();
        if detaching {
            let detach: Vec<Value> = current
                .iter()
                .zip(&current_keys)
                .filter(|(_, key)| !wanted.contains(key))
                .map(|(id, _)| id.clone())
                .collect();
            if !detach.is_empty() {
                self.detach_on(target, Some(detach.clone())).await?;
                changes.detached = detach;
            }
        }
        let mut attach = Vec::new();
        for ((id, values), key) in records.into_iter().zip(wanted) {
            if !current_keys.contains(&key) {
                if !attach
                    .iter()
                    .any(|(existing, _): &(Value, _)| key_of(existing) == key)
                {
                    changes.attached.push(id.clone());
                    attach.push((id, values));
                }
            } else if !values.is_empty()
                && self.update_existing_on(target, id.clone(), values).await? > 0
            {
                changes.updated.push(id);
            }
        }
        self.attach_on(target, attach).await?;
        if !changes.is_empty() {
            self.touch_if_touching(target).await?;
        }
        Ok(changes)
    }

    /// Makes the related records exactly `ids`: attaches the missing ones,
    /// detaches the others. Returns what changed.
    pub async fn sync<E, I>(
        &self,
        exec: &E,
        ids: impl IntoIterator<Item = I>,
    ) -> Result<SyncChanges>
    where
        E: Executor + ?Sized,
        I: RelatedId<R>,
    {
        let records: Vec<(Value, Vec<(String, Value)>)> = self
            .related_ids(ids)?
            .into_iter()
            .map(|id| (id, Vec::new()))
            .collect();
        let target = exec.target(Access::Write).await?;
        atomically!(target, |t| self.sync_on(t, records, true))
    }

    /// Makes the related records exactly the ids of `rows`, with their
    /// pivot values: attached ones get them, already attached ones whose
    /// values differ are updated.
    pub async fn sync_with<E, I, V>(
        &self,
        exec: &E,
        rows: impl IntoIterator<Item = (I, V)>,
    ) -> Result<SyncChanges>
    where
        E: Executor + ?Sized,
        I: RelatedId<R>,
        V: IntoIterator<Item = Assignment>,
    {
        let mut records = Vec::new();
        for (id, values) in rows {
            let id = canonical_id::<R>(&self.related_key, id.related_value(&self.related_key)?);
            records.push((id, assigned(values)?));
        }
        let target = exec.target(Access::Write).await?;
        atomically!(target, |t| self.sync_on(t, records, true))
    }

    /// Attaches the missing ones of `ids`, detaching nothing.
    pub async fn sync_without_detaching<E, I>(
        &self,
        exec: &E,
        ids: impl IntoIterator<Item = I>,
    ) -> Result<SyncChanges>
    where
        E: Executor + ?Sized,
        I: RelatedId<R>,
    {
        let records: Vec<(Value, Vec<(String, Value)>)> = self
            .related_ids(ids)?
            .into_iter()
            .map(|id| (id, Vec::new()))
            .collect();
        let target = exec.target(Access::Write).await?;
        atomically!(target, |t| self.sync_on(t, records, false))
    }

    /// [`sync`](Self::sync), with the same pivot `values` for every id.
    pub async fn sync_with_pivot_values<E, I>(
        &self,
        exec: &E,
        ids: impl IntoIterator<Item = I>,
        values: impl IntoIterator<Item = Assignment>,
    ) -> Result<SyncChanges>
    where
        E: Executor + ?Sized,
        I: RelatedId<R>,
    {
        let values = assigned(values)?;
        let records: Vec<(Value, Vec<(String, Value)>)> = self
            .related_ids(ids)?
            .into_iter()
            .map(|id| (id, values.clone()))
            .collect();
        let target = exec.target(Access::Write).await?;
        atomically!(target, |t| self.sync_on(t, records, true))
    }

    /// Detaches the attached ones of `ids` and attaches the others. Returns
    /// what changed (`updated` stays empty).
    pub async fn toggle<E, I>(
        &self,
        exec: &E,
        ids: impl IntoIterator<Item = I>,
    ) -> Result<SyncChanges>
    where
        E: Executor + ?Sized,
        I: RelatedId<R>,
    {
        let ids = self.related_ids(ids)?;
        let target = exec.target(Access::Write).await?;
        atomically!(target, |t| async {
            let current: Vec<Option<RelationKey>> =
                self.current_ids(t).await?.iter().map(key_of).collect();
            let mut changes = SyncChanges::default();
            let (detach, attach): (Vec<Value>, Vec<Value>) = ids
                .into_iter()
                .partition(|id| current.contains(&key_of(id)));
            if !detach.is_empty() {
                self.detach_on(t, Some(detach.clone())).await?;
                changes.detached = detach;
            }
            let mut rows: Vec<(Value, Vec<(String, Value)>)> = Vec::new();
            for id in attach {
                if !rows
                    .iter()
                    .any(|(existing, _)| key_of(existing) == key_of(&id))
                {
                    rows.push((id, Vec::new()));
                }
            }
            changes.attached = rows.iter().map(|(id, _)| id.clone()).collect();
            self.attach_on(t, rows).await?;
            if !changes.is_empty() {
                self.touch_if_touching(t).await?;
            }
            Ok(changes)
        })
    }

    /// Sets `values` (and the pivot's update timestamp) on the pivot row of
    /// related record `id`; returns how many rows changed. A typed pivot
    /// model's row is saved (its hooks run) when it changed.
    pub async fn update_existing_pivot<E, I>(
        &self,
        exec: &E,
        id: I,
        values: impl IntoIterator<Item = Assignment>,
    ) -> Result<u64>
    where
        E: Executor + ?Sized,
        I: RelatedId<R>,
    {
        let id = canonical_id::<R>(&self.related_key, id.related_value(&self.related_key)?);
        let values = assigned(values)?;
        let target = exec.target(Access::Write).await?;
        atomically!(target, |t| async {
            let updated = self.update_existing_on(t, id, values).await?;
            self.touch_if_touching(t).await?;
            Ok(updated)
        })
    }

    /// Saves `record` and attaches it with `pivot` values.
    pub async fn save<E: Executor + ?Sized>(
        &self,
        exec: &E,
        record: &mut Record<R>,
        pivot: impl IntoIterator<Item = Assignment>,
    ) -> Result<bool> {
        let pivot = assigned(pivot)?;
        let target = exec.target(Access::Write).await?;
        atomically!(target, |t| async {
            if !crate::persist::save(t, record).await? {
                return Ok(false);
            }
            let id = column_value(&record.model, &self.related_key, &self.core.name)?;
            self.attach_on(t, vec![(id, pivot)]).await?;
            self.touch_if_touching(t).await?;
            Ok(true)
        })
    }

    /// [`save`](Self::save) without model events.
    pub async fn save_quietly<E: Executor + ?Sized>(
        &self,
        exec: &E,
        record: &mut Record<R>,
        pivot: impl IntoIterator<Item = Assignment>,
    ) -> Result<bool> {
        let pivot: Vec<Assignment> = pivot.into_iter().collect();
        crate::events::without_events(self.save(exec, record, pivot)).await
    }

    /// Saves each of `records` and attaches it.
    pub async fn save_many<E: Executor + ?Sized>(
        &self,
        exec: &E,
        records: &mut [Record<R>],
    ) -> Result<bool> {
        let mut all = true;
        for record in records {
            all &= self.save(exec, record, []).await?;
        }
        Ok(all)
    }

    /// [`save_many`](Self::save_many) without model events.
    pub async fn save_many_quietly<E: Executor + ?Sized>(
        &self,
        exec: &E,
        records: &mut [Record<R>],
    ) -> Result<bool> {
        crate::events::without_events(self.save_many(exec, records)).await
    }

    /// Inserts `model` and attaches it with `pivot` values.
    pub async fn create<E: Executor + ?Sized>(
        &self,
        exec: &E,
        model: R,
        pivot: impl IntoIterator<Item = Assignment>,
    ) -> Result<Record<R>> {
        let mut record = Record::new(model);
        self.save(exec, &mut record, pivot).await?;
        Ok(record)
    }

    /// [`create`](Self::create) without model events.
    pub async fn create_quietly<E: Executor + ?Sized>(
        &self,
        exec: &E,
        model: R,
        pivot: impl IntoIterator<Item = Assignment>,
    ) -> Result<Record<R>> {
        let pivot: Vec<Assignment> = pivot.into_iter().collect();
        crate::events::without_events(self.create(exec, model, pivot)).await
    }

    /// Inserts each of `models` and attaches it.
    pub async fn create_many<E: Executor + ?Sized>(
        &self,
        exec: &E,
        models: impl IntoIterator<Item = R>,
    ) -> Result<Collection<R>> {
        let mut records = Collection::new();
        for model in models {
            records.push(self.create(exec, model, []).await?);
        }
        Ok(records)
    }

    /// [`create_many`](Self::create_many) without model events.
    pub async fn create_many_quietly<E: Executor + ?Sized>(
        &self,
        exec: &E,
        models: impl IntoIterator<Item = R>,
    ) -> Result<Collection<R>> {
        let models: Vec<R> = models.into_iter().collect();
        crate::events::without_events(self.create_many(exec, models)).await
    }

    /// Inserts a related record from untyped `attributes` and attaches it.
    pub async fn force_create<E: Executor + ?Sized>(
        &self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        pivot: impl IntoIterator<Item = Assignment>,
    ) -> Result<Record<R>>
    where
        R: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        let mut record = new_with::<R>(&attributes, |_| {})?;
        self.save(exec, &mut record, pivot).await?;
        Ok(record)
    }

    /// [`force_create`](Self::force_create) without model events.
    pub async fn force_create_quietly<E: Executor + ?Sized>(
        &self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        pivot: impl IntoIterator<Item = Assignment>,
    ) -> Result<Record<R>>
    where
        R: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        let pivot: Vec<Assignment> = pivot.into_iter().collect();
        crate::events::without_events(self.force_create(exec, attributes, pivot)).await
    }

    /// The related record with key `key`, or a new, unsaved `R` (not
    /// attached).
    pub async fn find_or_new<E: Executor + ?Sized>(
        self,
        exec: &E,
        key: impl Into<R::Key>,
    ) -> Result<Record<R>>
    where
        R: Default,
    {
        Ok(self
            .find(exec, key)
            .await?
            .unwrap_or_else(|| Record::new(R::default())))
    }

    /// The first record of `R` (attached or not) matching `attributes`, or
    /// a new, unsaved one with them and whatever `fill` sets.
    pub async fn first_or_new<E: Executor + ?Sized>(
        self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut R) + Send,
    ) -> Result<Record<R>>
    where
        R: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        let mut global = Builder::<R>::new();
        global.scopes = self.core.query.scopes.clone();
        match matching(global, &attributes).first(exec).await? {
            Some(found) => Ok(found),
            None => new_with::<R>(&attributes, fill),
        }
    }

    /// The related record matching `attributes`; else an existing record of
    /// `R` matching them, attached with `pivot`; else a new one with them
    /// (and whatever `fill` sets), inserted and attached.
    pub async fn first_or_create<E: Executor + ?Sized>(
        self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut R) + Send,
        pivot: impl IntoIterator<Item = Assignment>,
    ) -> Result<Record<R>>
    where
        R: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        let pivot = assigned(pivot)?;
        let target = exec.target(Access::Write).await?;
        if let Some(found) = self.clone().related_matching(&target, &attributes).await? {
            return Ok(found);
        }
        if let Some(existing) = self.global_matching(&target, &attributes).await? {
            let id = column_value(&existing.model, &self.related_key, &self.core.name)?;
            self.attach_atomically(&target, id, pivot).await?;
            return Ok(existing);
        }
        self.create_or_first_on(&target, &attributes, fill, pivot)
            .await
    }

    /// Inserts a record with `attributes` (and whatever `fill` sets) and
    /// attaches it; on a unique violation, attaches the existing record
    /// instead (or returns it when it is attached already).
    pub async fn create_or_first<E: Executor + ?Sized>(
        self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut R) + Send,
        pivot: impl IntoIterator<Item = Assignment>,
    ) -> Result<Record<R>>
    where
        R: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        let pivot = assigned(pivot)?;
        let target = exec.target(Access::Write).await?;
        self.create_or_first_on(&target, &attributes, fill, pivot)
            .await
    }

    /// Updates the related record matching `attributes` with `fill` (and
    /// saves it), or finds or creates one with both and attaches it.
    pub async fn update_or_create<E: Executor + ?Sized>(
        self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl Fn(&mut R) + Send + Sync,
        pivot: impl IntoIterator<Item = Assignment>,
    ) -> Result<Record<R>>
    where
        R: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        let pivot: Vec<Assignment> = pivot.into_iter().collect();
        let mut record = self
            .clone()
            .first_or_create(exec, attributes, &fill, pivot)
            .await?;
        if !record.was_recently_created() && record.exists() {
            fill(&mut record.model);
            record.save(exec).await?;
        }
        Ok(record)
    }

    /// Adds `step` to `column` of the related record matching
    /// `attributes`, or finds or creates it (attached) with `column` set to
    /// `default`.
    pub async fn increment_or_create<E, T>(
        self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        column: Column<R, T>,
        default: T,
        step: T,
    ) -> Result<Record<R>>
    where
        E: Executor + ?Sized,
        T: Numeric,
        R: Default,
    {
        let attributes: Vec<Assignment> = attributes.into_iter().collect();
        let initial = column.set(default);
        let column_name = initial.column().to_owned();
        let mut record = self
            .clone()
            .first_or_create(
                exec,
                attributes,
                move |model: &mut R| {
                    if let Some(value) = initial.assigned_value() {
                        let _ = model.set_value(&column_name, value.clone());
                    }
                },
                [],
            )
            .await?;
        if !record.was_recently_created() && record.exists() {
            record.increment(exec, column, step).await?;
        }
        Ok(record)
    }

    /// Like [`first_or_create`](Self::first_or_create), soft-deleted
    /// records included: a trashed match is restored.
    pub async fn restore_or_create<E: Executor + ?Sized>(
        self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut R) + Send,
        pivot: impl IntoIterator<Item = Assignment>,
    ) -> Result<Record<R>>
    where
        R: Default,
    {
        let mut record = self
            .with_trashed()
            .first_or_create(exec, attributes, fill, pivot)
            .await?;
        let target = exec.target(Access::Write).await?;
        restore_record(&target, &mut record).await?;
        Ok(record)
    }

    /// Like [`create_or_first`](Self::create_or_first), soft-deleted
    /// records included: a trashed match is restored.
    pub async fn create_or_restore<E: Executor + ?Sized>(
        self,
        exec: &E,
        attributes: impl IntoIterator<Item = Assignment>,
        fill: impl FnOnce(&mut R) + Send,
        pivot: impl IntoIterator<Item = Assignment>,
    ) -> Result<Record<R>>
    where
        R: Default,
    {
        let mut record = self
            .with_trashed()
            .create_or_first(exec, attributes, fill, pivot)
            .await?;
        let target = exec.target(Access::Write).await?;
        restore_record(&target, &mut record).await?;
        Ok(record)
    }

    /// The related record matching `attributes`, on `target`.
    async fn related_matching(
        self,
        target: &Target<'_>,
        attributes: &[Assignment],
    ) -> Result<Option<Record<R>>> {
        let bound = self
            .scope(|query| matching(query, attributes))
            .limit(1)
            .bound()?;
        if bound.empty {
            return Ok(None);
        }
        let mut records = bound.query.clone().fetch(target).await?.into_vec();
        bound.finish(&mut records)?;
        Ok(records.into_iter().next())
    }

    /// A record of `R` matching `attributes`, attached or not.
    async fn global_matching(
        &self,
        target: &Target<'_>,
        attributes: &[Assignment],
    ) -> Result<Option<Record<R>>> {
        let mut global = Builder::<R>::new();
        global.scopes = self.core.query.scopes.clone();
        Ok(matching(global, attributes)
            .limit(1)
            .fetch(target)
            .await?
            .into_vec()
            .into_iter()
            .next())
    }

    /// Attaches `id` inside a savepoint, ignoring a unique violation (the
    /// row is there already).
    async fn attach_atomically(
        &self,
        target: &Target<'_>,
        id: Value,
        pivot: Vec<(String, Value)>,
    ) -> Result<()> {
        let result = match target.begin_savepoint().await? {
            Some(savepoint) => {
                let nested = target.on_transaction(&savepoint);
                let result = self.attach_on(&nested, vec![(id, pivot)]).await;
                drop(nested);
                match result {
                    Ok(()) => {
                        savepoint.commit().await?;
                        Ok(())
                    }
                    Err(error) => {
                        savepoint.rollback().await?;
                        Err(error)
                    }
                }
            }
            None => self.attach_on(target, vec![(id, pivot)]).await,
        };
        match result {
            Ok(()) => self.touch_if_touching(target).await,
            Err(error) if error.is_unique_violation() => Ok(()),
            Err(error) => Err(error),
        }
    }

    async fn create_or_first_on(
        self,
        target: &Target<'_>,
        attributes: &[Assignment],
        fill: impl FnOnce(&mut R) + Send,
        pivot: Vec<(String, Value)>,
    ) -> Result<Record<R>>
    where
        R: Default,
    {
        let mut record = new_with::<R>(attributes, fill)?;
        match save_atomically(target, &mut record).await {
            Ok(_) => {
                let id = column_value(&record.model, &self.related_key, &self.core.name)?;
                self.attach_atomically(target, id, pivot).await?;
                Ok(record)
            }
            Err(error) if error.is_unique_violation() => {
                match self.global_matching(target, attributes).await? {
                    Some(existing) => {
                        let id = column_value(&existing.model, &self.related_key, &self.core.name)?;
                        self.attach_atomically(target, id, pivot).await?;
                        Ok(existing)
                    }
                    None => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }

    /// What the relation holds once loaded: the related records.
    pub async fn results<E: Executor + ?Sized>(self, exec: &E) -> Result<Collection<R>> {
        self.get(exec).await
    }
}

/// A pivot column for the where helpers: a name is qualified by the pivot
/// table.
struct PivotColumn<'a, C> {
    column: C,
    table: &'a str,
}

impl<T, C: ColumnOf<T>> ColumnOf<T> for PivotColumn<'_, C> {
    fn column_ref(&self) -> ColumnRef {
        let column = self.column.column_ref();
        if column.table.is_some() {
            column
        } else {
            ColumnRef {
                table: Some(Cow::Owned(self.table.to_owned())),
                ..column
            }
        }
    }

    fn encode_value(&self, value: &T) -> std::result::Result<Value, String> {
        self.column.encode_value(value)
    }
}

impl<P: Model, R: Model, X: PivotKind> private::Erase<P> for BelongsToMany<P, R, X> {
    fn erase(self) -> Arc<dyn ErasedRelation<P>> {
        let mut unbound = self;
        unbound.core = unbound.core.unbound();
        Arc::new(unbound)
    }

    fn rename(&mut self, name: Cow<'static, str>) {
        self.core.name = name;
    }
}

impl<P: Model, R: Model, X: PivotKind> Relation<P> for BelongsToMany<P, R, X> {
    type Value = Collection<R>;

    fn name(&self) -> &str {
        &self.core.name
    }

    fn kind(&self) -> RelationKind {
        match &self.morph {
            None => RelationKind::BelongsToMany,
            Some(morph) if morph.inverse => RelationKind::MorphedByMany,
            Some(_) => RelationKind::MorphToMany,
        }
    }

    fn meta(&self) -> RelationMeta {
        let mut meta = RelationMeta::new::<P>(&self.core.name, self.kind())
            .related::<R>()
            .keys(&self.foreign_pivot_key, &self.parent_key);
        meta.pivot_table = Some(self.table.to_string());
        meta.related_pivot_key = Some(self.related_pivot_key.to_string());
        if let Some(morph) = &self.morph {
            meta.morph_type = Some(morph.type_column.to_string());
            meta.morph_name = Some(morph.name.to_string());
        }
        meta
    }
}

impl<P: Model, R: Model, X: PivotKind> RelatedTo<P> for BelongsToMany<P, R, X> {
    type Related = R;
}

impl<P: Model, R: Model, X: PivotKind> ErasedRelation<P> for BelongsToMany<P, R, X> {
    fn name(&self) -> &str {
        &self.core.name
    }

    fn meta(&self) -> RelationMeta {
        Relation::meta(self)
    }

    fn related_type(&self) -> Option<TypeId> {
        Some(TypeId::of::<R>())
    }

    fn eager_load<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        parents: &'a mut [Record<P>],
        node: &'a EagerNode,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let values = column_values(parents, &self.parent_key, &self.core.name)?;
            let parent_keys: Vec<Option<RelationKey>> = values.iter().map(key_of).collect();
            let keys = distinct_keys(values);
            let mut groups: HashMap<RelationKey, Vec<Record<R>>> = HashMap::new();
            let relation = constrained(self.clone(), node)?;
            if !keys.is_empty() {
                let query =
                    prepare_query(relation.core.query.clone(), node, R::meta().table(), &[])?;
                let query = relation.joined(query);
                let mut conditions = vec![in_keys_on(
                    target,
                    qualified(&relation.table, &relation.foreign_pivot_key),
                    &keys,
                )];
                conditions.extend(relation.morph_condition());
                let query = constrain(query, conditions);
                let query = limit_per_parent(
                    query,
                    Expr::Column(qualified(&relation.table, &relation.foreign_pivot_key)),
                );
                let mut records = query.fetch(target).await?.into_vec();
                drop_row_numbers(&mut records);
                let owners = relation.hydrate(&mut records)?;
                if node.autoload {
                    crate::relations::lazy::group_records(records.iter_mut());
                }
                for (record, owner) in records.into_iter().zip(owners) {
                    if let Some(owner) = owner {
                        groups.entry(owner).or_default().push(record);
                    }
                }
            }
            let values = distribute(groups, &parent_keys);
            for (parent, value) in parents.iter_mut().zip(values) {
                let mut children = value.unwrap_or_default();
                if relation.chaperone {
                    let snapshot = AnyRecord::new(parent.without_relations());
                    for child in &mut children {
                        relation.chaperone_one(&snapshot, child);
                    }
                }
                parent
                    .relations
                    .set_relation(node.name.clone(), Collection::from(children));
            }
            Ok(())
        })
    }

    fn exists_query(&self, request: ExistsRequest) -> Result<QueryIr> {
        let parent_table = P::meta().table();
        let related_table = R::meta().table();
        let aliased = (related_table == parent_table).then_some(related_table);
        let pivot = self.table.clone();
        let related_key = self.related_key.clone();
        let related_pivot_key = self.related_pivot_key.clone();
        let morph = self.morph_condition();
        let parent = qualified(parent_table, &self.parent_key);
        let foreign = qualified(&self.table, &self.foreign_pivot_key);
        build_subquery::<R>(
            P::meta(),
            &self.core.query,
            request,
            &self.core.name,
            aliased,
            |query, alias| {
                let query = query.join(
                    &pivot,
                    qualified(alias.unwrap_or(related_table), &related_key),
                    "=",
                    qualified(&pivot, &related_pivot_key),
                );
                match morph {
                    Some(morph) => query.where_(Filter::from(morph)),
                    None => query,
                }
            },
            |_| vec![columns_equal(parent, foreign)],
        )
    }

    fn touch<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        parent: &'a Record<P>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut relation = self.clone();
            relation.core.parent = Some(parent.model.clone());
            relation.touch_on(target).await?;
            Ok(())
        })
    }

    fn load_nested_missing<'a, 'b>(
        &'a self,
        target: &'a Target<'b>,
        parents: &'a mut [Record<P>],
        rest: &'a EagerTree,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(crate::relations::lazy::load_nested_many::<P, R>(
            target,
            parents,
            &self.core.name,
            rest,
        ))
    }
}
