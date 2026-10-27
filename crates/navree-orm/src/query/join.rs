//! [`JoinClause`]: the `ON` conditions of a join, built in a closure.

use crate::column::AsExpr;
use crate::ir::{Boolean, Join, JoinKind, Operator, TableRef, Where, WhereClause};
use crate::query::filters::{HasWheres, filters};

/// The conditions of a join: column comparisons ([`on`](JoinClause::on),
/// [`or_on`](JoinClause::or_on)) and every where method, all combined into
/// the join's `ON`.
///
/// ```
/// use navree_orm::{Dialect, col, table};
///
/// let sql = table("deals")
///     .join_on("companies", |j| {
///         j.on("deals.company_id", "=", "companies.id")
///             .where_(col("companies.active").eq(true))
///     })
///     .to_sql(Dialect::Postgres)
///     .unwrap();
/// assert_eq!(
///     sql.sql,
///     r#"SELECT "deals".* FROM "deals" INNER JOIN "companies" ON "deals"."company_id" = "companies"."id" AND "companies"."active" = $1"#
/// );
/// ```
#[derive(Clone, Debug)]
pub struct JoinClause {
    kind: JoinKind,
    table: TableRef,
    on: Vec<WhereClause>,
}

impl JoinClause {
    /// A join of `kind` on `table` without conditions yet.
    pub(crate) fn new(kind: JoinKind, table: TableRef) -> Self {
        JoinClause {
            kind,
            table,
            on: Vec::new(),
        }
    }

    /// The join.
    pub(crate) fn into_join(self) -> Join {
        Join {
            kind: self.kind,
            table: self.table,
            on: self.on,
        }
    }

    fn compare(mut self, boolean: Boolean, first: impl AsExpr, op: impl Into<Operator>, second: impl AsExpr) -> Self {
        self.on.push(WhereClause {
            boolean,
            negated: false,
            filter: Where::Compare {
                left: first.as_expr(),
                op: op.into(),
                right: second.as_expr(),
            },
        });
        self
    }

    /// `AND first op second`, comparing two columns.
    pub fn on(self, first: impl AsExpr, op: impl Into<Operator>, second: impl AsExpr) -> Self {
        self.compare(Boolean::And, first, op, second)
    }

    /// `OR first op second`, comparing two columns.
    pub fn or_on(self, first: impl AsExpr, op: impl Into<Operator>, second: impl AsExpr) -> Self {
        self.compare(Boolean::Or, first, op, second)
    }

    filters!();
}

impl HasWheres for JoinClause {
    fn wheres_mut(&mut self) -> &mut Vec<WhereClause> {
        &mut self.on
    }
}
