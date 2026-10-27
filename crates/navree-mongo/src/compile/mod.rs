//! The ORM's queries and writes compiled into MongoDB commands, without a
//! server: what the driver runs and what `to_mongo()` shows.
//!
//! | Query | MongoDB |
//! |---|---|
//! | the table | the collection |
//! | `where` conditions | a filter document: the query language for a field and a value, `$expr` for computed values; `LIKE` as an escaped `$regex` (`s`, and `i` without case); date parts with `$dateToString`, `$month`, ... in UTC; JSON paths as dot paths, containment as `$all` and dotted equalities, length as `$size`, key existence as `$exists`; full text as `$text` |
//! | `select` | a projection (`{"value": "$title"}` for aliases), computed columns in `$project` |
//! | `order_by`, `latest`, `in_random_order` | `$sort` (NULL placement and computed keys through `$set`, random order through `$rand`) |
//! | `limit`, `offset` | `$limit`, `$skip` |
//! | `distinct`, `distinct_on` | `$group` on the selected fields; the first document per key |
//! | `group_by`, aggregates, `having` | `$group` with accumulators, `$match` after it |
//! | `time_bucket`, `first`, `last` | TimescaleDB's alignment in milliseconds, `$dateTrunc` for months and years; `$top` and `$bottom` |
//! | `join`, `left_join` | `$lookup` (by `localField`/`foreignField` for an equality, a pipeline otherwise) and `$unwind` |
//! | `where_exists`, `where_in_sub`, `where_sub`, `select_sub` | a `$lookup` per document, correlated through its `let` variables |
//! | `union`, `union_all` | `$unionWith`, de-duplicated for `union` |
//! | derived tables (`from_sub`) | the inner pipeline, then the outer stages |
//! | raw SQL, row locks, `RIGHT JOIN` | refused with [`OrmError::Unsupported`], naming what to do instead |
//!
//! A pipeline whose stages are only a filter, a sort, a skip, a limit and a
//! projection is a `find`.

mod expr;
mod filter;
mod group;
mod like;
mod read;
mod scope;
mod subquery;
mod write;

use bson::Document;
use navree_orm::OrmError;
use navree_orm::ir::{Aggregate, QueryIr};

pub(crate) use expr::time_bucket;
pub(crate) use read::read_with_columns;
pub(crate) use write::{Target, insert, plain_field, target, update_document};

use crate::command::MongoCommand;

/// Stages a condition or a value needs before the stage that uses it
/// (`$lookup`s for subqueries), and the temporary fields they add.
#[derive(Debug, Default)]
pub(crate) struct Pending {
    pub(crate) stages: Vec<Document>,
    pub(crate) temps: Vec<String>,
}

impl Pending {
    /// Moves the stages into `stages`; returns the temporary fields to
    /// remove once they are used.
    pub(crate) fn flush(&mut self, stages: &mut Vec<Document>) -> Vec<String> {
        stages.append(&mut self.stages);
        std::mem::take(&mut self.temps)
    }
}

/// The command reading `query`'s documents: a `find`, or an aggregation
/// pipeline when the query needs one.
///
/// ```
/// use navree_mongo::compile;
/// use navree_orm::ir::QueryIr;
///
/// let query = QueryIr::new("deals");
/// assert_eq!(compile::read(&query).unwrap().to_string(), "db.deals.find({})");
/// ```
pub fn read(query: &QueryIr) -> Result<MongoCommand, OrmError> {
    read::read(query)
}

/// The command computing `aggregate` over `query`, in a field named
/// `aggregate` (a `countDocuments` for a plain count).
pub fn aggregate(query: &QueryIr, aggregate: &Aggregate) -> Result<MongoCommand, OrmError> {
    read::aggregate(query, aggregate)
}

/// The command telling whether `query` has a document.
pub fn exists(query: &QueryIr) -> Result<MongoCommand, OrmError> {
    read::exists(query)
}

/// Whether `query` names columns of a query around it: a correlated
/// subquery, which runs once per document of that query.
pub(crate) fn is_correlated(query: &QueryIr) -> bool {
    names_outside(query, &[])
}

/// Whether `query` names a table that is neither its own, nor joined, nor
/// in `known` (the tables of the queries around it that are allowed).
fn names_outside(query: &QueryIr, known: &[String]) -> bool {
    use navree_orm::ir::InValues;

    let mut own: Vec<String> = known.to_vec();
    own.push(query.table.name.to_string());
    own.push(query.table.qualifier().to_owned());
    for join in &query.joins {
        own.push(join.table.name.to_string());
        own.push(join.table.qualifier().to_owned());
    }
    fn expr_outside(expr: &navree_orm::ir::Expr, own: &[String]) -> bool {
        use navree_orm::ir::Expr;
        match expr {
            Expr::Column(column) => column
                .table
                .as_deref()
                .is_some_and(|table| !own.iter().any(|name| name == table)),
            Expr::Function { args, .. } => args.iter().any(|arg| expr_outside(arg, own)),
            Expr::Aggregate { arg, .. } => arg.as_deref().is_some_and(|arg| expr_outside(arg, own)),
            Expr::Arithmetic { left, right, .. } => {
                expr_outside(left, own) || expr_outside(right, own)
            }
            Expr::Subquery(query) => names_outside(query, own),
            Expr::TimeBucket { expr, origin, .. } => {
                expr_outside(expr, own) || origin.as_deref().is_some_and(|o| expr_outside(o, own))
            }
            Expr::First { value, time } | Expr::Last { value, time } => {
                expr_outside(value, own) || expr_outside(time, own)
            }
            _ => false,
        }
    }
    fn where_outside(filter: &navree_orm::ir::Where, own: &[String]) -> bool {
        use navree_orm::ir::Where;
        let column_outside = |column: &navree_orm::ir::ColumnRef| {
            column
                .table
                .as_deref()
                .is_some_and(|table| !own.iter().any(|name| name == table))
        };
        match filter {
            Where::Compare { left, right, .. } => expr_outside(left, own) || expr_outside(right, own),
            Where::Null { expr, .. } => expr_outside(expr, own),
            Where::In { expr, values, .. } => {
                expr_outside(expr, own)
                    || match values {
                        InValues::List(list) => list.iter().any(|item| expr_outside(item, own)),
                        InValues::Subquery(query) => names_outside(query, own),
                        _ => false,
                    }
            }
            Where::Between {
                expr, low, high, ..
            } => expr_outside(expr, own) || expr_outside(low, own) || expr_outside(high, own),
            Where::Like { expr, pattern, .. } => expr_outside(expr, own) || expr_outside(pattern, own),
            Where::DatePart { expr, value, .. } => expr_outside(expr, own) || expr_outside(value, own),
            Where::Exists { query, .. } => names_outside(query, own),
            Where::Group(clauses) => clauses.iter().any(|clause| where_outside(&clause.filter, own)),
            Where::JsonContains { column, .. } | Where::JsonLength { column, .. } => {
                column_outside(column)
            }
            Where::FullText { columns, .. } => columns.iter().any(|column| expr_outside(column, own)),
            _ => false,
        }
    }
    if let Some(inner) = &query.table.subquery
        && names_outside(inner, known)
    {
        return true;
    }
    query.columns.iter().any(|item| expr_outside(&item.expr, &own))
        || query.wheres.iter().any(|clause| where_outside(&clause.filter, &own))
        || query.groups.iter().any(|group| expr_outside(group, &own))
        || query.havings.iter().any(|clause| where_outside(&clause.filter, &own))
        || query.orders.iter().any(|order| match order {
            navree_orm::ir::Order::By { expr, .. } => expr_outside(expr, &own),
            _ => false,
        })
        || query
            .joins
            .iter()
            .any(|join| join.on.iter().any(|clause| where_outside(&clause.filter, &own)))
        || query.unions.iter().any(|union| names_outside(&union.query, known))
}
