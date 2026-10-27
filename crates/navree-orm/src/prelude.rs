//! The names an app uses with models: `use navree_orm::prelude::*;`.
//!
//! The model trait and its derive (`Model`), `#[scopes]`, records and
//! collections, the builders and their condition types, executors, errors,
//! values and the pagination types.

pub use crate::hooks::{Flow, ModelEvent, ModelHook};
// The stream adapters (`try_next`, `map_ok`, `try_collect`, ...) for
// `lazy`, `cursor` and friends, without their names.
pub use crate::streams::{StreamExt as _, TryStreamExt as _};
pub use crate::{
    Assignment, Builder, Collection, Column, CursorPage, CursorRequest, Decimal, Dialect, Executor,
    Expr, Field, Filter, GlobalScope, IntoExpr, JoinClause, Model, OrmError, PageRequest, Raw,
    Record, RowStream, Scoped, SimplePage, TableBuilder, Ulid, Value, ValueFor, WhereGroup, col,
    col_as, raw, scopes, table,
};
