//! Renaming a table in a query's column references: the existence and
//! aggregate subqueries of a self-relation (deals and their parent deals)
//! alias the related table, and every reference meant for it (the
//! relation's conditions, the caller's constraints, global scopes) moves
//! to the alias. SQL's scoping is respected: a nested subquery that binds
//! the same name itself keeps its references.

use std::borrow::Cow;

use crate::ir::{
    ColumnRef, Expr, InValues, Order, QueryIr, SelectItem, TableRef, Where, WhereClause,
};

/// Renames references to table `from` as `to` in `query` (not the tables
/// it reads: the caller aliases the one it means).
pub(crate) fn requalify(query: &mut QueryIr, from: &str, to: &str) {
    let renamer = Renamer { from, to };
    renamer.query(query);
}

struct Renamer<'a> {
    from: &'a str,
    to: &'a str,
}

impl Renamer<'_> {
    /// Whether `table` binds the name `from` in its query.
    fn binds(&self, table: &TableRef) -> bool {
        table.qualifier() == self.from
    }

    fn query(&self, query: &mut QueryIr) {
        if let Some(subquery) = &mut query.table.subquery {
            self.nested(subquery);
        }
        self.items(&mut query.columns);
        for join in &mut query.joins {
            if let Some(subquery) = &mut join.table.subquery {
                self.nested(subquery);
            }
            self.clauses(&mut join.on);
        }
        self.clauses(&mut query.wheres);
        for group in &mut query.groups {
            self.expr(group);
        }
        self.clauses(&mut query.havings);
        for order in &mut query.orders {
            if let Order::By { expr, .. } = order {
                self.expr(expr);
            }
        }
        for union in &mut query.unions {
            self.nested(&mut union.query);
        }
    }

    /// A nested query: renamed unless it binds `from` itself.
    fn nested(&self, query: &mut QueryIr) {
        let shadows =
            self.binds(&query.table) || query.joins.iter().any(|join| self.binds(&join.table));
        if !shadows {
            self.query(query);
        }
    }

    fn items(&self, items: &mut [SelectItem]) {
        for item in items {
            self.expr(&mut item.expr);
        }
    }

    fn column(&self, column: &mut ColumnRef) {
        if column.table.as_deref() == Some(self.from) {
            column.table = Some(Cow::Owned(self.to.to_owned()));
        }
    }

    fn expr(&self, expr: &mut Expr) {
        match expr {
            Expr::Column(column) => self.column(column),
            Expr::Value(_) | Expr::Raw { .. } => {}
            Expr::Function { args, .. } => {
                for arg in args {
                    self.expr(arg);
                }
            }
            Expr::Aggregate { arg, .. } => {
                if let Some(arg) = arg {
                    self.expr(arg);
                }
            }
            Expr::Arithmetic { left, right, .. } | Expr::Binary { left, right, .. } => {
                self.expr(left);
                self.expr(right);
            }
            Expr::VectorDistance { expr, .. } => self.expr(expr),
            // The inserted row's value: no table to move.
            Expr::Excluded { .. } => {}
            Expr::Subquery(query) | Expr::Exists(query) => self.nested(query),
            Expr::TimeBucket { expr, origin, .. } => {
                self.expr(expr);
                if let Some(origin) = origin {
                    self.expr(origin);
                }
            }
            Expr::First { value, time } | Expr::Last { value, time } => {
                self.expr(value);
                self.expr(time);
            }
            Expr::All { table } => {
                if table.as_deref() == Some(self.from) {
                    *table = Some(Cow::Owned(self.to.to_owned()));
                }
            }
            Expr::RowNumber { partition, order } => {
                for expr in partition {
                    self.expr(expr);
                }
                for order in order {
                    if let Order::By { expr, .. } = order {
                        self.expr(expr);
                    }
                }
            }
        }
    }

    fn clauses(&self, clauses: &mut [WhereClause]) {
        for clause in clauses {
            self.filter(&mut clause.filter);
        }
    }

    fn filter(&self, filter: &mut Where) {
        match filter {
            Where::Compare { left, right, .. } => {
                self.expr(left);
                self.expr(right);
            }
            Where::Null { expr, .. } => self.expr(expr),
            Where::In { expr, values, .. } => {
                self.expr(expr);
                match values {
                    InValues::List(list) => {
                        for value in list {
                            self.expr(value);
                        }
                    }
                    InValues::Subquery(query) => self.nested(query),
                }
            }
            Where::Between {
                expr, low, high, ..
            } => {
                self.expr(expr);
                self.expr(low);
                self.expr(high);
            }
            Where::Like { expr, pattern, .. } => {
                self.expr(expr);
                self.expr(pattern);
            }
            Where::IgnoreCase { expr, .. } => self.expr(expr),
            Where::DatePart { expr, value, .. } => {
                self.expr(expr);
                self.expr(value);
            }
            Where::Exists { query, .. } => self.nested(query),
            Where::Group(clauses) => self.clauses(clauses),
            Where::JsonContains { column, .. }
            | Where::JsonContainedBy { column, .. }
            | Where::JsonOverlaps { column, .. }
            | Where::JsonContainsKey { column, .. }
            | Where::JsonLength { column, .. } => {
                self.column(column);
            }
            Where::FullText { columns, .. } => {
                for column in columns {
                    self.expr(column);
                }
            }
            // Raw SQL, render-tree conditions and a document store's native
            // filters are written by the caller, who names tables as they
            // mean them.
            Where::Raw { .. } | Where::Tree(_) | Where::Native(_) | Where::Invalid(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Operator, TableRef};

    fn compare(left: &str, right: &str) -> WhereClause {
        WhereClause::and(Where::Compare {
            left: Expr::Column(ColumnRef::parse(left.to_owned())),
            op: Operator::Eq,
            right: Expr::Column(ColumnRef::parse(right.to_owned())),
        })
    }

    #[test]
    fn references_move_to_the_alias_except_where_shadowed() {
        let mut inner = QueryIr::new("companies");
        inner
            .wheres
            .push(compare("deals.company_id", "companies.id"));
        let mut shadowing = QueryIr::new("deals");
        shadowing
            .wheres
            .push(compare("deals.id", "deals.parent_id"));
        let mut query = QueryIr::new("deals");
        query.table = TableRef::aliased("deals", "navree_1");
        query.wheres.push(compare("deals.title", "deals.notes"));
        query.wheres.push(WhereClause::and(Where::Exists {
            query: Box::new(inner),
            not: false,
        }));
        query.wheres.push(WhereClause::and(Where::Exists {
            query: Box::new(shadowing),
            not: false,
        }));
        requalify(&mut query, "deals", "navree_1");
        let text = format!("{:?}", query.wheres);
        assert!(text.contains("navree_1"), "{text}");
        let Where::Compare {
            left: Expr::Column(left),
            ..
        } = &query.wheres[0].filter
        else {
            panic!("a comparison");
        };
        assert_eq!(left.table.as_deref(), Some("navree_1"));
        let Where::Exists { query: inner, .. } = &query.wheres[1].filter else {
            panic!("an exists");
        };
        let Where::Compare {
            left: Expr::Column(outer),
            ..
        } = &inner.wheres[0].filter
        else {
            panic!("a comparison");
        };
        assert_eq!(outer.table.as_deref(), Some("navree_1"), "correlated");
        let Where::Exists {
            query: shadowed, ..
        } = &query.wheres[2].filter
        else {
            panic!("an exists");
        };
        let Where::Compare {
            left: Expr::Column(own),
            ..
        } = &shadowed.wheres[0].filter
        else {
            panic!("a comparison");
        };
        assert_eq!(own.table.as_deref(), Some("deals"), "its own binding");
        assert_eq!(query.table.name, "deals", "tables are not renamed");
    }
}
