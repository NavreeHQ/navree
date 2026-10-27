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

pub(crate) use read::read_with_columns;
pub(crate) use write::{Target, delete, insert, target, update_document};

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
