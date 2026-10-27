//! Subqueries as `$lookup`s: `EXISTS`, `IN (subquery)` and subqueries
//! giving one value, correlated through `$lookup` variables when they name
//! the enclosing query's columns (`where_has`, `with_count`, `select_sub`).

use std::borrow::Cow;

use bson::{Bson, Document, doc};
use navree_orm::OrmError;
use navree_orm::ir::{AggregateFunction, Expr, QueryIr, SelectItem};

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
                    if kind == Kind::Values { "a list of values" } else { "a value" },
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
    let lets = child.lets();
    if !lets.is_empty() {
        stage.insert("let", lets.into_iter().collect::<Document>());
    }
    stage.insert("pipeline", plan.stages);
    stage.insert("as", field.clone());
    pending.stages.push(doc! { "$lookup": stage });
    pending.temps.push(field.clone());
    Ok(field)
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
