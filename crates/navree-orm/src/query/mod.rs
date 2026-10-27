//! The query builders: [`Builder<M>`] for models, [`TableBuilder`] for any
//! table, with [`WhereGroup`] and [`JoinClause`] for nested conditions.
//!
//! Builder methods take `self` and return `Self`, so queries chain;
//! terminal methods (`get`, `first`, `count`, `update`, ...) take the
//! executor ([`Db`](navree_db::Db), [`Uow`](navree_db::Uow), ...) first and
//! run the query.

use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

pub(crate) mod clauses;
mod fetch;
pub(crate) mod filters;
mod join;
mod table;
mod write;

pub use clauses::{HasQuery, IntoSelect};
pub use fetch::Selectable;
pub use filters::{HasWheres, WhereGroup};
pub use join::JoinClause;
pub use table::{TableBuilder, table};

use crate::attributes::extras::ExtraCasts;
use crate::column::{Filter, IntoQuery};
use crate::error::Result;
use crate::ir::{ColumnRef, Expr, InValues, QueryIr, SelectItem, Where, WhereClause};
use crate::model::Model;
use crate::scope::{GlobalScope, RemovedScopes, SoftDeletes, group_scope_wheres};
use crate::sql::{Dialect, Statement, compile_select};
use crate::value::Field;

use self::clauses::clauses;
use self::filters::filters;

/// A query of model `M`: the where family, selects, joins, groups,
/// orders, limits, scopes, and the retrieval, aggregate, pagination and
/// mass-write methods that run it.
///
/// ```no_run
/// # use navree_orm::__doc_models::*;
/// # async fn demo(db: Db, me: i64, page: PageQuery) -> Result<()> {
/// let won = Deal::query()
///     .where_(Deal::STAGE.eq(Stage::Won))
///     .where_group(|g| g.where_(Deal::OWNER_ID.eq(me)).or_where_null(Deal::OWNER_ID))
///     .latest()
///     .paginate(&db, page)
///     .await?;
/// # Ok(()) }
/// ```
///
/// A query selects the model's columns (qualified by its table, so joins
/// stay unambiguous) unless `select` replaces them; columns added with
/// `add_select`, `select_raw`, `select_sub` and friends come back as the
/// records' [extras](crate::Record::extras). Global scopes (and the
/// soft-delete scope) apply when the query runs.
pub struct Builder<M: Model> {
    pub(crate) ir: QueryIr,
    pub(crate) removed: RemovedScopes,
    pub(crate) scopes_applied: bool,
    /// Query-time casts of extra columns (`with_casts`).
    pub(crate) extra_casts: ExtraCasts,
    _model: PhantomData<fn() -> M>,
}

impl<M: Model> Builder<M> {
    /// A query of every record of `M`.
    pub fn new() -> Self {
        Builder {
            ir: QueryIr::new(M::meta().table()),
            removed: RemovedScopes::default(),
            scopes_applied: false,
            extra_casts: ExtraCasts::default(),
            _model: PhantomData,
        }
    }

    filters!();
    clauses!();

    /// Only the record with key `key`.
    pub fn where_key(self, key: impl Into<M::Key>) -> Self {
        let key = key.into();
        self.where_(Filter::compare(
            key_expr::<M>(),
            crate::ir::Operator::Eq,
            Expr::Value(key.to_value()),
        ))
    }

    /// Every record but the one with key `key`.
    pub fn where_key_not(self, key: impl Into<M::Key>) -> Self {
        let key = key.into();
        self.where_(Filter::compare(
            key_expr::<M>(),
            crate::ir::Operator::Ne,
            Expr::Value(key.to_value()),
        ))
    }

    /// Only the records with these keys (none for an empty list).
    pub fn where_key_in(self, keys: impl IntoIterator<Item = impl Into<M::Key>>) -> Self {
        let values = keys
            .into_iter()
            .map(|key| Expr::Value(key.into().to_value()))
            .collect();
        self.where_(Filter(Where::In {
            expr: key_expr::<M>(),
            values: InValues::List(values),
            not: false,
        }))
    }

    /// Every record but the ones with these keys.
    pub fn where_key_not_in(self, keys: impl IntoIterator<Item = impl Into<M::Key>>) -> Self {
        let values = keys
            .into_iter()
            .map(|key| Expr::Value(key.into().to_value()))
            .collect();
        self.where_(Filter(Where::In {
            expr: key_expr::<M>(),
            values: InValues::List(values),
            not: true,
        }))
    }

    /// Applies local scope `scope` (a function of the builder). The
    /// conditions it adds are grouped when they (or the ones before them)
    /// contain an `OR`, so a scope never changes the meaning of the
    /// conditions around it. `#[scopes]` turns scope functions into builder
    /// methods; this is the plain form.
    ///
    /// ```no_run
    /// # use navree_orm::__doc_models::*;
    /// # async fn demo(db: Db) -> Result<()> {
    /// fn open(query: Builder<Deal>) -> Builder<Deal> {
    ///     query.where_null(Deal::CLOSED_AT)
    /// }
    ///
    /// let open_deals = Deal::query().scope(open).get(&db).await?;
    /// # Ok(()) }
    /// ```
    pub fn scope(mut self, scope: impl FnOnce(Self) -> Self) -> Self {
        let before = self.ir.wheres.len();
        self = scope(self);
        group_scope_wheres(&mut self.ir, before);
        self
    }

    /// Leaves out global scope `S` for this query.
    pub fn without_global_scope<S: GlobalScope<M>>(mut self) -> Self {
        self.removed.ids.push(std::any::TypeId::of::<S>());
        self
    }

    /// Leaves out every global scope (the soft-delete scope included).
    pub fn without_global_scopes(mut self) -> Self {
        self.removed.all = true;
        self
    }

    /// Includes soft-deleted records (removes the soft-delete scope).
    pub fn with_trashed(self) -> Self {
        self.without_global_scope::<SoftDeletes>()
    }

    /// Newest first by the model's creation timestamp. Fails when the query
    /// runs if the model keeps none; use [`latest_by`](Self::latest_by).
    pub fn latest(self) -> Self {
        match M::meta().created_at() {
            Some(column) => self.latest_by(ColumnRef::new(Some(M::meta().table()), column)),
            None => self.with_error(format!(
                "{} keeps no creation timestamp; use latest_by(column)",
                M::meta().name()
            )),
        }
    }

    /// Oldest first by the model's creation timestamp. Fails when the query
    /// runs if the model keeps none; use [`oldest_by`](Self::oldest_by).
    pub fn oldest(self) -> Self {
        match M::meta().created_at() {
            Some(column) => self.oldest_by(ColumnRef::new(Some(M::meta().table()), column)),
            None => self.with_error(format!(
                "{} keeps no creation timestamp; use oldest_by(column)",
                M::meta().name()
            )),
        }
    }

    /// Records a problem found while building; the query fails with it
    /// when it runs.
    pub(crate) fn with_error(mut self, message: String) -> Self {
        self.ir.errors.push(message);
        self
    }

    /// The query as built so far, without global scopes and default
    /// columns (for inspection; [`into_ir`](Self::into_ir) gives what
    /// runs).
    pub fn ir(&self) -> &QueryIr {
        &self.ir
    }

    /// The query that runs: global scopes applied and the model's columns
    /// selected when nothing else is.
    pub fn into_ir(self) -> QueryIr {
        let mut builder = self.apply_global_scopes();
        if builder.ir.columns.is_empty() {
            builder.ir.columns = model_columns::<M>(builder.ir.table.qualifier());
        }
        builder.ir
    }

    /// The statement the query compiles to for `dialect` (global scopes
    /// applied), without running it.
    ///
    /// ```
    /// # use navree_orm::__doc_models::*;
    /// # fn main() -> Result<()> {
    /// let sql = Company::query().where_(Company::ACTIVE.eq(true)).to_sql(Dialect::Postgres)?;
    /// assert_eq!(
    ///     sql.sql,
    ///     r#"SELECT "companies"."id", "companies"."name", "companies"."active" FROM "companies" WHERE "companies"."active" = $1"#
    /// );
    /// assert_eq!(sql.values, [Value::from(true)]);
    /// # Ok(()) }
    /// ```
    pub fn to_sql(&self, dialect: Dialect) -> Result<Statement> {
        compile_select(&self.clone().into_ir(), dialect)
    }

    /// The builder with its global scopes applied (once).
    pub(crate) fn apply_global_scopes(mut self) -> Self {
        if self.scopes_applied {
            return self;
        }
        self.scopes_applied = true;
        if self.removed.all {
            return self;
        }
        let mut scopes: Vec<Arc<dyn GlobalScope<M>>> = M::global_scopes();
        if M::meta().soft_deletes_enabled() {
            scopes.push(Arc::new(SoftDeletes));
        }
        for scope in scopes {
            if self.removed.contains(scope.scope_id()) {
                continue;
            }
            let before = self.ir.wheres.len();
            self = scope.apply(self);
            group_scope_wheres(&mut self.ir, before);
        }
        self
    }
}

/// The model's key column, qualified by its table.
pub(crate) fn key_expr<M: Model>() -> Expr {
    let meta = M::meta();
    Expr::Column(ColumnRef::new(Some(meta.table()), meta.key_column()))
}

/// The model's columns, qualified by `table`.
pub(crate) fn model_columns<M: Model>(table: &str) -> Vec<SelectItem> {
    M::meta()
        .all_columns()
        .iter()
        .map(|column| SelectItem {
            expr: Expr::Column(ColumnRef::new(Some(table.to_owned()), column.name())),
            alias: None,
        })
        .collect()
}

impl<M: Model> Default for Builder<M> {
    fn default() -> Self {
        Builder::new()
    }
}

impl<M: Model> Clone for Builder<M> {
    fn clone(&self) -> Self {
        Builder {
            ir: self.ir.clone(),
            removed: self.removed.clone(),
            scopes_applied: self.scopes_applied,
            extra_casts: self.extra_casts.clone(),
            _model: PhantomData,
        }
    }
}

impl<M: Model> fmt::Debug for Builder<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Builder")
            .field("model", &M::meta().name())
            .field("query", &self.ir)
            .finish_non_exhaustive()
    }
}

impl<M: Model> HasWheres for Builder<M> {
    fn wheres_mut(&mut self) -> &mut Vec<WhereClause> {
        &mut self.ir.wheres
    }
}

impl<M: Model> HasQuery for Builder<M> {
    fn query_mut(&mut self) -> &mut QueryIr {
        &mut self.ir
    }

    fn default_select(&self) -> Vec<SelectItem> {
        model_columns::<M>(self.ir.table.qualifier())
    }
}

/// A model query as a subquery (`where_in_sub`, `where_exists`,
/// `select_sub`, ...), its global scopes applied.
impl<M: Model> IntoQuery for Builder<M> {
    fn into_query(self) -> QueryIr {
        self.into_ir()
    }
}
