//! Scopes: reusable query constraints.
//!
//! - **Local scopes** are functions `fn(Builder<M>) -> Builder<M>`, applied
//!   with [`Builder::scope`](crate::Builder::scope). `#[scopes]` on an
//!   `impl` block turns its `#[scope]` functions into builder methods
//!   (`Deal::query().open().owned_by(me)`) through a generated trait.
//! - **Global scopes** implement [`GlobalScope`] and are declared on the
//!   model (`#[model(global_scopes(ActiveOnly))]`). They apply when a query
//!   runs, unless removed with `without_global_scope::<S>()` or
//!   `without_global_scopes()`.
//! - Soft-deleting models get [`SoftDeletes`], which hides deleted rows
//!   until `with_trashed()` removes it.
//!
//! A scope never changes the meaning of the conditions around it: when the
//! conditions it adds (or those before it) contain an `OR`, they are
//! wrapped in parentheses, so `where a OR b` plus a scope's `where c` is
//! `(a OR b) AND c`.

use std::any::TypeId;

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
pub trait Scoped<M: Model>: Sized {
    /// Applies `scope`, grouping the conditions it adds.
    fn apply_scope(self, scope: impl FnOnce(Builder<M>) -> Builder<M>) -> Self;
}

impl<M: Model> Scoped<M> for Builder<M> {
    fn apply_scope(self, scope: impl FnOnce(Builder<M>) -> Builder<M>) -> Self {
        self.scope(scope)
    }
}

/// Which global scopes a builder has removed.
#[derive(Clone, Debug, Default)]
pub(crate) struct RemovedScopes {
    pub(crate) all: bool,
    pub(crate) ids: Vec<TypeId>,
}

impl RemovedScopes {
    pub(crate) fn contains(&self, id: TypeId) -> bool {
        self.all || self.ids.contains(&id)
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
    if !slice.iter().skip(1).any(|clause| clause.boolean == Boolean::Or) {
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
                Where::Compare { left: Expr::Column(column), .. } => {
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
    fn nothing_added_changes_nothing() {
        let mut ir = QueryIr::new("deals");
        ir.wheres = vec![clause(Boolean::And, "a"), clause(Boolean::Or, "b")];
        group_scope_wheres(&mut ir, 2);
        assert_eq!(shape(&ir.wheres), "And:a Or:b");
    }
}
