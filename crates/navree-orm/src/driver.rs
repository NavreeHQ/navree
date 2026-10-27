//! [`DocumentDriver`]: the interface a document store (MongoDB) implements
//! to run models.
//!
//! A document store receives the same IR the SQL compiler does and answers
//! with [`Row`]s of `(field, value)` pairs. The table is the collection;
//! `wheres` become a filter document (`Compare` → `$eq`/`$gt`/..., `In` →
//! `$in`, `Null` → `null`, `Between` → `$gte`/`$lte`, `Like` → `$regex`,
//! `DatePart` → `$expr` with `$year`/`$month`/..., `Group` →
//! `$and`/`$or`); `orders` the sort; `columns` the projection; `limit` and
//! `offset` the limit and skip; aggregates a pipeline. Variants a driver
//! cannot express (raw SQL, joins) are answered with
//! [`OrmError::Unsupported`].
//!
//! Relations load with `where_in` on the related keys, which a driver
//! answers with `$in`, so eager loading needs nothing more; a `$lookup` is
//! an optimisation a driver may add. Embedded documents are JSON casts.
//! Transactions are the driver's sessions: its crate provides an
//! [`Executor`](crate::Executor) for a session as well as for the client,
//! whose [`DocumentTarget`](crate::DocumentTarget) says where the change
//! events of the writes go (navree-db's unit of work, or the store's own
//! [`ChangeRecorder`](crate::ChangeRecorder)). Rows a driver gets from
//! its own queries become records with
//! [`Target::records`](crate::Target::records).
//!
//! ```
//! use std::sync::{Arc, Mutex};
//!
//! use navree_kernel::BoxFuture;
//! use navree_orm::ir::{Aggregate, DeleteIr, InsertIr, QueryIr, UpdateIr};
//! use navree_orm::{DocumentDriver, InsertOutcome, OrmError, Row, Value};
//!
//! /// A driver that answers every find with no documents.
//! struct Empty;
//!
//! impl DocumentDriver for Empty {
//!     fn name(&self) -> &'static str {
//!         "empty"
//!     }
//!     fn find<'a>(&'a self, _query: &'a QueryIr) -> BoxFuture<'a, Result<Vec<Row>, OrmError>> {
//!         Box::pin(async { Ok(Vec::new()) })
//!     }
//!     fn aggregate<'a>(&'a self, _q: &'a QueryIr, _a: &'a Aggregate) -> BoxFuture<'a, Result<Value, OrmError>> {
//!         Box::pin(async { Ok(Value::BigInt(Some(0))) })
//!     }
//!     fn insert<'a>(&'a self, insert: &'a InsertIr) -> BoxFuture<'a, Result<InsertOutcome, OrmError>> {
//!         Box::pin(async move { Ok(InsertOutcome::affected(insert.rows.len() as u64)) })
//!     }
//!     fn update<'a>(&'a self, _update: &'a UpdateIr) -> BoxFuture<'a, Result<u64, OrmError>> {
//!         Box::pin(async { Ok(0) })
//!     }
//!     fn delete<'a>(&'a self, _delete: &'a DeleteIr) -> BoxFuture<'a, Result<u64, OrmError>> {
//!         Box::pin(async { Ok(0) })
//!     }
//! }
//! ```

use navree_kernel::BoxFuture;

use crate::error::OrmError;
use crate::ir::{Aggregate, AggregateFunction, DeleteIr, InsertIr, QueryIr, UpdateIr};
use crate::row::Row;
use crate::value::{Field, Value};

/// A document store under the model API. See the [module docs](self).
pub trait DocumentDriver: Send + Sync + 'static {
    /// The driver's name in messages, e.g. `mongodb`.
    fn name(&self) -> &'static str;

    /// The fractional-second digits the store keeps in date-times: 6
    /// (microseconds, like the SQL databases) unless the driver says
    /// otherwise; MongoDB keeps 3. The timestamps the ORM sets
    /// (`created_at`, `updated_at`, `deleted_at`, touches) are truncated to
    /// it, so a saved record and its stored document agree.
    fn time_precision(&self) -> u32 {
        6
    }

    /// The documents matching `query`.
    fn find<'a>(&'a self, query: &'a QueryIr) -> BoxFuture<'a, Result<Vec<Row>, OrmError>>;

    /// An aggregate over the documents matching `query` (`count` gives an
    /// integer; the others NULL when nothing matches).
    fn aggregate<'a>(
        &'a self,
        query: &'a QueryIr,
        aggregate: &'a Aggregate,
    ) -> BoxFuture<'a, Result<Value, OrmError>>;

    /// Whether any document matches `query` (by default, a count).
    fn exists<'a>(&'a self, query: &'a QueryIr) -> BoxFuture<'a, Result<bool, OrmError>> {
        Box::pin(async move {
            let count = Aggregate {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
            };
            let value = self.aggregate(query, &count).await?;
            let count = i64::from_value(value).map_err(|error| OrmError::Driver {
                driver: self.name().into(),
                message: format!("the count is not a number: {error}"),
            })?;
            Ok(count > 0)
        })
    }

    /// Inserts documents; generated keys come back as returned rows.
    fn insert<'a>(&'a self, insert: &'a InsertIr)
    -> BoxFuture<'a, Result<InsertOutcome, OrmError>>;

    /// Updates the matching documents; returns how many changed.
    fn update<'a>(&'a self, update: &'a UpdateIr) -> BoxFuture<'a, Result<u64, OrmError>>;

    /// Deletes the matching documents; returns how many were deleted.
    fn delete<'a>(&'a self, delete: &'a DeleteIr) -> BoxFuture<'a, Result<u64, OrmError>>;
}

/// The result of an insert.
#[derive(Debug, Default)]
pub struct InsertOutcome {
    /// How many rows (documents) were inserted.
    pub rows_affected: u64,
    /// The `returning` columns of each inserted row (PostgreSQL and SQLite
    /// `RETURNING`; generated ids of a document store).
    pub returned: Vec<Row>,
    /// MySQL's `LAST_INSERT_ID()` (the first generated key of the batch).
    pub last_insert_id: Option<u64>,
}

impl InsertOutcome {
    /// An outcome of `rows` inserted rows with nothing returned.
    pub fn affected(rows: u64) -> Self {
        InsertOutcome {
            rows_affected: rows,
            ..InsertOutcome::default()
        }
    }
}
