//! Scopes: reusable query constraints.
//!
//! - **Local scopes** are functions `fn(Builder<M>) -> Builder<M>`, applied
//!   with [`Builder::scope`](crate::Builder::scope). `#[scopes]` on an
//!   `impl` block turns its `#[scope]` functions into builder methods
//!   (`Deal::query().open().owned_by(me)`) through a generated trait.
//! - **Global scopes** implement [`GlobalScope`] and are declared on the
//!   model (`#[model(global_scopes(ActiveOnly))]`) or registered by a module
//!   at boot (`m.global_scope::<Deal>("active", |q| ..)`, see
//!   [`ModuleBuilderOrmExt`](crate::ModuleBuilderOrmExt)). They apply when a
//!   query runs, unless removed with `without_global_scope::<S>()`,
//!   `without_global_scope_named("active")`, `without_global_scopes()` or
//!   `without_global_scopes_except([..])`.
//! - Soft-deleting models get [`SoftDeletes`], which hides deleted rows;
//!   `with_trashed()`, `without_trashed()` and `only_trashed()` choose
//!   otherwise.
//!
//! A scope never changes the meaning of the conditions around it: when the
//! conditions it adds (or those before it) contain an `OR`, they are
//! wrapped in parentheses, so `where a OR b` plus a scope's `where c` is
//! `(a OR b) AND c`.

use std::any::TypeId;
use std::borrow::Cow;

use crate::ir::{Boolean, QueryIr, Where, WhereClause};
use crate::model::Model;
use crate::query::Builder;

/// A constraint every query of model `M` gets, unless removed.
///
/// ```
/// use navree_orm::{Builder, GlobalScope, Model, col};
///
/// /// Only records that are not archived.
/// struct NotArchived;
///
/// impl<M: Model> GlobalScope<M> for NotArchived {
///     fn apply(&self, query: Builder<M>) -> Builder<M> {
///         query.where_(col("archived").eq(false))
///     }
/// }
/// ```
pub trait GlobalScope<M: Model>: Send + Sync + 'static {
    /// Adds the scope's constraints to `query`.
    fn apply(&self, query: Builder<M>) -> Builder<M>;

    /// The scope's identity, for `without_global_scope::<S>()`.
    fn scope_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    /// The scope's name, for `without_global_scope_named(..)`,
    /// `without_global_scopes_except(..)` and inspection: the type's name
    /// without its path (`NotArchived`) unless the scope says otherwise.
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed(short_type_name(std::any::type_name::<Self>()))
    }
}

/// `path::to::Name<Args>` → `Name`.
pub(crate) fn short_type_name(full: &'static str) -> &'static str {
    let without_args = full.split('<').next().unwrap_or(full);
    without_args.rsplit("::").next().unwrap_or(without_args)
}

/// The built-in global scope of soft-deleting models: rows whose
/// deleted-at column is set are left out. `with_trashed()` removes it.
#[derive(Clone, Copy, Debug, Default)]
pub struct SoftDeletes;

impl<M: Model> GlobalScope<M> for SoftDeletes {
    fn apply(&self, query: Builder<M>) -> Builder<M> {
        match M::meta().deleted_at() {
            Some(column) => {
                let column = crate::ir::ColumnRef::new(Some(M::meta().table()), column);
                query.where_(crate::column::Filter::from(Where::Null {
                    expr: crate::ir::Expr::Column(column),
                    not: false,
                }))
            }
            None => query,
        }
    }
}

/// A builder that model `M`'s local scopes apply to: [`Builder<M>`] (and
/// relation builders). `#[scopes]` implements its generated trait for every
/// `Scoped<M>`.
///
/// ```
/// use chrono::{DateTime, Utc};
/// use navree_orm::prelude::*;
///
/// #[derive(Model, Clone, Debug, Default, serde::Serialize)]
/// pub struct Deal {
///     id: i64,
///     owner_id: Option<i64>,
///     closed_at: Option<DateTime<Utc>>,
/// }
///
/// #[scopes]
/// impl Deal {
///     /// Deals not closed yet.
///     #[scope]
///     fn open(query: Builder<Self>) -> Builder<Self> {
///         query.where_null(Deal::CLOSED_AT)
///     }
///
///     /// Deals of `owner`.
///     #[scope]
///     fn owned_by(query: Builder<Self>, owner: i64) -> Builder<Self> {
///         query.where_(Deal::OWNER_ID.eq(owner))
///     }
/// }
///
/// // `DealScopes`, the generated trait, is in scope here.
/// let sql = Deal::query().open().owned_by(7).to_sql(Dialect::Postgres).unwrap();
/// assert_eq!(
///     sql.sql,
///     r#"SELECT "deals"."id", "deals"."owner_id", "deals"."closed_at" FROM "deals" WHERE "deals"."closed_at" IS NULL AND "deals"."owner_id" = $1"#
/// );
/// ```
pub trait Scoped<M: Model>: Sized {
    /// Applies `scope`, grouping the conditions it adds.
    fn apply_scope(self, scope: impl FnOnce(Builder<M>) -> Builder<M>) -> Self;
}

impl<M: Model> Scoped<M> for Builder<M> {
    fn apply_scope(self, scope: impl FnOnce(Builder<M>) -> Builder<M>) -> Self {
        self.scope(scope)
    }
}

/// What a query does with the model's global scopes: the ones it leaves
/// out, and how it treats soft-deleted rows.
#[derive(Clone, Debug, Default)]
pub(crate) struct ScopeChoices {
    /// `without_global_scopes()`.
    pub(crate) all: bool,
    /// `without_global_scope::<S>()`.
    pub(crate) ids: Vec<TypeId>,
    /// `without_global_scope_named(..)`.
    pub(crate) names: Vec<Cow<'static, str>>,
    /// `without_global_scopes_except(..)`: only these stay.
    pub(crate) keep: Option<Vec<Cow<'static, str>>>,
    /// `with_trashed()`, `without_trashed()`, `only_trashed()`.
    pub(crate) trashed: Trashed,
}

/// How a query of a soft-deleting model treats trashed rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Trashed {
    /// The [`SoftDeletes`] scope hides them (unless it is removed).
    #[default]
    Hidden,
    /// `with_trashed()`: they are included.
    Included,
    /// `without_trashed()`: they are left out, whatever else is removed.
    Excluded,
    /// `only_trashed()`: only they are included.
    Only,
}

impl ScopeChoices {
    /// Whether the query leaves `scope` out.
    pub(crate) fn removes<M: Model>(&self, scope: &dyn GlobalScope<M>) -> bool {
        if scope.scope_id() == TypeId::of::<SoftDeletes>() && self.trashed != Trashed::Hidden {
            return true;
        }
        if self.all || self.ids.contains(&scope.scope_id()) {
            return true;
        }
        let name = scope.name();
        self.names.contains(&name) || self.keep.as_ref().is_some_and(|keep| !keep.contains(&name))
    }
}

/// After a scope added clauses to `ir.wheres` (beyond the first
/// `before`), wraps the old and the new clauses in groups when they
/// contain an `OR`.
pub(crate) fn group_scope_wheres(ir: &mut QueryIr, before: usize) {
    if ir.wheres.len() <= before {
        return;
    }
    let mut all = std::mem::take(&mut ir.wheres);
    let added = all.split_off(before);
    ir.wheres = group_slice(all);
    ir.wheres.extend(group_slice(added));
}

fn group_slice(slice: Vec<WhereClause>) -> Vec<WhereClause> {
    if !slice
        .iter()
        .skip(1)
        .any(|clause| clause.boolean == Boolean::Or)
    {
        return slice;
    }
    let boolean = slice.first().map_or(Boolean::And, |clause| clause.boolean);
    vec![WhereClause {
        boolean,
        negated: false,
        filter: Where::Group(slice),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Expr, Operator};

    fn clause(boolean: Boolean, name: &'static str) -> WhereClause {
        WhereClause {
            boolean,
            negated: false,
            filter: Where::Compare {
                left: Expr::col(name),
                op: Operator::Eq,
                right: Expr::val(1),
            },
        }
    }

    fn shape(clauses: &[WhereClause]) -> String {
        clauses
            .iter()
            .map(|c| match &c.filter {
                Where::Group(inner) => format!("{:?}({})", c.boolean, shape(inner)),
                Where::Compare {
                    left: Expr::Column(column),
                    ..
                } => {
                    format!("{:?}:{}", c.boolean, column.name)
                }
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn ors_on_either_side_of_a_scope_are_grouped() {
        let mut ir = QueryIr::new("deals");
        ir.wheres = vec![clause(Boolean::And, "a"), clause(Boolean::Or, "b")];
        ir.wheres.push(clause(Boolean::And, "c"));
        group_scope_wheres(&mut ir, 2);
        assert_eq!(shape(&ir.wheres), "And(And:a Or:b) And:c");
    }

    #[test]
    fn plain_and_chains_are_left_flat() {
        let mut ir = QueryIr::new("deals");
        ir.wheres = vec![clause(Boolean::And, "a"), clause(Boolean::And, "b")];
        ir.wheres.push(clause(Boolean::And, "c"));
        ir.wheres.push(clause(Boolean::Or, "d"));
        group_scope_wheres(&mut ir, 2);
        assert_eq!(shape(&ir.wheres), "And:a And:b And(And:c Or:d)");
    }

    #[test]
    fn scope_names_drop_the_path_and_the_arguments() {
        assert_eq!(short_type_name("navree_orm::scope::SoftDeletes"), "SoftDeletes");
        assert_eq!(short_type_name("app::scopes::Owned<app::Deal>"), "Owned");
        assert_eq!(short_type_name("Plain"), "Plain");
    }

    #[test]
    fn nothing_added_changes_nothing() {
        let mut ir = QueryIr::new("deals");
        ir.wheres = vec![clause(Boolean::And, "a"), clause(Boolean::Or, "b")];
        group_scope_wheres(&mut ir, 2);
        assert_eq!(shape(&ir.wheres), "And:a Or:b");
    }
}
