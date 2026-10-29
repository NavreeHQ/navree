//! navree-db's own data layer on SQLx: [`Backend`]s, [`Value`]s,
//! [`Statement`]s and [`Row`]s, classified [`DbError`]s, [`Pool`]s,
//! [`Transaction`]s with savepoints, and the [`Executor`] they share.

// Built without any driver (none of `sqlite`, `postgres`, `mysql`), the
// engine compiles but every connection fails: the drivers' code is unused.
#![cfg_attr(
    not(any(feature = "postgres", feature = "mysql", feature = "sqlite")),
    allow(unused, unreachable_code)
)]

mod backend;
pub(crate) mod driver;
mod error;
mod executor;
mod pool;
pub(crate) mod report;
mod row;
mod statement;
pub(crate) mod transaction;
mod value;

pub use backend::Backend;
pub use driver::{Executed, RowStream};
pub use error::{DbError, ErrorKind};
pub use executor::{Executor, ExecutorExt};
pub use pool::Pool;
pub use row::{ColumnIndex, Row};
pub use statement::Statement;
pub(crate) use statement::inline_values as statement_text;
pub use transaction::{Transaction, TxOptions};
pub use value::{FromValue, HasKind, Kind, Value, ValueError};
