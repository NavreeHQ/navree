//! Subqueries as `$lookup`s: `EXISTS`, `IN (subquery)` and subqueries
//! giving one value, correlated through `$lookup` variables when they name
//! the enclosing query's columns (`where_has`, `with_count`, `select_sub`).
//! An equality between a field of the subquery's collection and one of the
//! enclosing documents (`deals.company_id = companies._id`) is matched by
//! `localField` and `foreignField`, so an index serves it.

use std::borrow::Cow;

use bson::{Bson, Document, doc};
use navree_orm::OrmError;
use navree_orm::ir::{
    AggregateFunction, Boolean, ColumnRef, Expr, Operator, QueryIr, SelectItem, Where, WhereClause,
};

use super::Pending;
use super::expr;
use super::read;
use super::scope::{Cx, Scope};
use crate::error::invalid;

/// What a subquery gives back.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Whether it has a document.
    Exists,
    /// Every value of its one column.
    Values,
    /// The value of its one column in its first document.
    Scalar,
}

/// The output field values of a subquery land in (`{"_v": ..}`).
const VALUE: &str = "_v";

/// Adds the `$lookup` running `query` for each document of `scope` to
/// `pending`; returns the field its results land in.
fn lookup(
    cx: &mut Cx,
    scope: &Scope<'_>,
    pending: &mut Pending,
    query: &QueryIr,
    kind: Kind,
) -> Result<String, OrmError> {
    let mut query = query.clone();
    query.lock = None;
    match kind {
        Kind::Exists => {
            query.orders.clear();
            if query.unions.is_empty() && query.groups.is_empty() && query.havings.is_empty() {
                query.columns.clear();
            }
        }
        Kind::Values | Kind::Scalar => {
            if query.columns.len() != 1 {
                return Err(invalid(format!(
                    "a subquery used as {} selects one column, not {}",
                    if kind == Kind::Values {
                        "a list of values"
                    } else {
                        "a value"
                    },
                    query.columns.len()
                )));
            }
            if query.columns[0].alias.is_none() && !matches!(query.columns[0].expr, Expr::Column(_))
            {
                query.columns[0] = SelectItem {
                    expr: query.columns[0].expr.clone(),
                    alias: Some(Cow::Borrowed(VALUE)),
                };
            }
        }
    }
    let keys = correlation(scope, &query);
    if let Some(keys) = &keys {
        query.wheres.remove(keys.clause);
    }
    let mut child = scope.child(&query.table);
    let mut plan = read::plan(cx, &mut child, &query)?;
    match kind {
        Kind::Exists => {
            plan.stages.push(doc! { "$limit": 1 });
            plan.stages.push(doc! { "$project": { "_id": 1 } });
        }
        Kind::Values | Kind::Scalar => {
            let column = plan
                .columns
                .as_ref()
                .and_then(|columns| columns.first())
                .cloned()
                .ok_or_else(|| invalid("a subquery used as a value selects one column"))?;
            if kind == Kind::Scalar {
                plan.stages.push(doc! { "$limit": 1 });
            }
            plan.stages
                .push(doc! { "$project": { VALUE: format!("${column}"), "_id": 0 } });
        }
    }
    let field = cx.field(match kind {
        Kind::Exists => "exists",
        Kind::Values => "in",
        Kind::Scalar => "value",
    });
    let mut stage = doc! { "from": plan.collection };
    let mut pipeline = Vec::with_capacity(plan.stages.len() + 1);
    if let Some(keys) = keys {
        pipeline.extend(keys.matched(&mut stage));
    }
    pipeline.extend(plan.stages);
    let lets = child.lets();
    if !lets.is_empty() {
        stage.insert("let", lets.into_iter().collect::<Document>());
    }
    stage.insert("pipeline", pipeline);
    stage.insert("as", field.clone());
    pending.stages.push(doc! { "$lookup": stage });
    pending.temps.push(field.clone());
    Ok(field)
}

/// The equality a `$lookup` matches by `localField` and `foreignField`: a
/// field of the enclosing documents and one of the looked-up collection.
/// MongoDB matches it with an index on the foreign field, where an `$expr`
/// on a variable would read the whole collection for every document.
#[derive(Debug)]
pub(crate) struct Keys {
    /// The condition it came from, among the query's top-level ones.
    pub(crate) clause: usize,
    /// The field of the enclosing documents.
    pub(crate) local: String,
    /// The field of the looked-up collection.
    pub(crate) foreign: String,
}

impl Keys {
    /// Puts `localField` and `foreignField` on `lookup`; returns the stage
    /// the pipeline starts with, if it needs one. MongoDB matches a null
    /// or missing local field with documents whose foreign field is null
    /// or missing; SQL never matches NULL, so unless one side is `_id`
    /// (never null) those documents are left out first.
    pub(crate) fn matched(self, lookup: &mut Document) -> Option<Document> {
        let guard = (self.local != "_id" && self.foreign != "_id")
            .then(|| doc! { "$match": { self.foreign.clone(): { "$ne": Bson::Null } } });
        lookup.insert("localField", self.local);
        lookup.insert("foreignField", self.foreign);
        guard
    }
}

/// The equality of `query`'s top-level conditions (when they are all
/// joined with AND) between a field of its own collection and a field of
/// the documents of `scope`, the query it is a subquery of. A derived
/// table has no fields of its own collection to match.
fn correlation(scope: &Scope<'_>, query: &QueryIr) -> Option<Keys> {
    if query.table.subquery.is_some() || has_or(&query.wheres) {
        return None;
    }
    let inner = scope.child(&query.table);
    let joined = |column: &ColumnRef| {
        column.table.as_deref().is_some_and(|qualifier| {
            query
                .joins
                .iter()
                .any(|join| join.table.qualifier() == qualifier)
        })
    };
    let own = |column: &ColumnRef| {
        column
            .table
            .as_deref()
            .is_none_or(|qualifier| inner.is_own(qualifier))
    };
    query
        .wheres
        .iter()
        .enumerate()
        .find_map(|(clause, condition)| {
            let Where::Compare {
                left: Expr::Column(left),
                op: Operator::Eq,
                right: Expr::Column(right),
            } = &condition.filter
            else {
                return None;
            };
            if condition.negated {
                return None;
            }
            let (foreign, local) = match (own(left), own(right)) {
                (true, false) if !joined(right) => (left, right),
                (false, true) if !joined(left) => (right, left),
                _ => return None,
            };
            Some(Keys {
                clause,
                local: scope.field_of(local)?,
                foreign: inner.field_of(foreign)?,
            })
        })
}

/// Whether a list of conditions has an OR at its top level.
pub(crate) fn has_or(clauses: &[WhereClause]) -> bool {
    clauses
        .iter()
        .skip(1)
        .any(|clause| clause.boolean == Boolean::Or)
}

/// `[NOT] EXISTS (query)`.
pub(crate) fn exists(
    cx: &mut Cx,
    scope: &Scope<'_>,
    pending: &mut Pending,
    query: &QueryIr,
    not: bool,
) -> Result<Document, OrmError> {
    let field = lookup(cx, scope, pending, query, Kind::Exists)?;
    Ok(doc! { format!("{field}.0"): { "$exists": !not } })
}

/// `expr [NOT] IN (query)`.
pub(crate) fn in_(
    cx: &mut Cx,
    scope: &Scope<'_>,
    pending: &mut Pending,
    expr: &Expr,
    query: &QueryIr,
    not: bool,
) -> Result<Document, OrmError> {
    let value = expr::value(cx, scope, pending, expr)?;
    let field = lookup(cx, scope, pending, query, Kind::Values)?;
    let found = Bson::Document(doc! { "$in": [value, format!("${field}.{VALUE}")] });
    Ok(doc! {
        "$expr": if not { Bson::Document(doc! { "$not": [found] }) } else { found }
    })
}

/// The value of a subquery giving one value (`NULL` when it has no
/// document; a count of nothing is 0).
pub(crate) fn scalar(
    cx: &mut Cx,
    scope: &Scope<'_>,
    pending: &mut Pending,
    query: &QueryIr,
) -> Result<Bson, OrmError> {
    let counts = matches!(
        query.columns.as_slice(),
        [SelectItem {
            expr: Expr::Aggregate {
                function: AggregateFunction::Count,
                ..
            },
            ..
        }]
    );
    let field = lookup(cx, scope, pending, query, Kind::Scalar)?;
    let first = Bson::Document(doc! { "$first": format!("${field}.{VALUE}") });
    Ok(if counts {
        Bson::Document(doc! { "$ifNull": [first, 0] })
    } else {
        first
    })
}
