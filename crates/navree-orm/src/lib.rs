//! # navree-orm
//!
//! Navree's ORM: active-record models on PostgreSQL / TimescaleDB, MySQL /
//! MariaDB and SQLite (and document stores through [`DocumentDriver`]),
//! running on navree-db's pool and unit of work.
//!
//! - **Models** are plain structs with `#[derive(Model)]`: a typed key
//!   (integers the database assigns, UUID v7 / v4 and [`Ulid`]s generated
//!   on insert, or keys the application sets), timestamps, soft deletes,
//!   casts, and one typed [`Column`] constant per column.
//! - **Queries** chain on [`Builder`]: every where variant, nested groups,
//!   selects and subqueries, joins, groups, havings, orders, limits,
//!   unions, locks, scopes, and TimescaleDB's `time_bucket`, `first` and
//!   `last`. [`TableBuilder`] queries tables without a model.
//! - **Records** ([`Record`]) carry dirty tracking, extras and loaded
//!   relations; saving inserts or updates, with lifecycle [`hooks`].
//! - **Relationships** ([`relations`](mod@relations)) are methods of a
//!   `#[relations]` impl block (belongs to, has one/many, many to many
//!   with pivots, through, polymorphic): query them like builders, eager
//!   load them in batches (`with("company.owner")`), check their existence
//!   (`where_has`), aggregate them (`with_count`), and write through them
//!   (`attach`, `sync`, `associate`).
//! - **The lifecycle**: model [`events`] heard by observers, closures,
//!   typed application events and [`broadcast`]s (after the commit inside a
//!   unit of work), quiet operations, soft deletes (`restore`,
//!   `force_delete`, `only_trashed`), global scopes registered at boot,
//!   [`strict`] modes, and [`prune`] (`model:prune`), configured by
//!   [`OrmModule`].
//! - **Streams and bulk writes**: chunks, lazy streams and cursors
//!   ([`streams`]), raw statements ([`raw()`]), multi-row inserts,
//!   insert-or-ignore, upserts and the other writes without events
//!   ([`Builder::insert`], [`TableBuilder::upsert`], ...).
//! - **Executors**: every terminal method takes the executor first — a
//!   [`Db`](navree_db::Db) or a [`Uow`](navree_db::Uow) — so statements run
//!   in the request's transaction, with its actor, workspace settings and
//!   change events (the audit trail), exactly like navree-db's repositories.
//!   Models and queries can live on named connections
//!   (`#[model(connection = "..")]`, `on_connection`, [`using_connection`])
//!   and read from the writer when replicas exist (`use_write`,
//!   [`using_writer`]); see [`Route`].
//! - **In apps**: model [`factory`] builders for tests and seeders, the
//!   database assertions and test databases of [`testing`], route model
//!   [`binding`] (`Bound<Deal>`), [`workspace`] scoping
//!   (`#[model(workspace)]`), access control (`#[model(subject, policy)]`,
//!   feature `access`), validation rules on models (feature `validation`)
//!   and `model:show` ([`OrmModule`]).
//!
//! ```
//! use chrono::{DateTime, Utc};
//! use navree_db::{Db, DbOptions, Executor};
//! use navree_orm::prelude::*;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
//! #[serde(rename_all = "snake_case")]
//! enum Stage {
//!     #[default]
//!     Open,
//!     Won,
//! }
//!
//! #[derive(Model, Clone, Debug, Default, Serialize)]
//! #[model(table = "deals", timestamps)]
//! struct Deal {
//!     id: i64,
//!     title: String,
//!     amount_cents: i64,
//!     #[model(cast = "string_enum")]
//!     stage: Stage,
//!     created_at: DateTime<Utc>,
//!     updated_at: DateTime<Utc>,
//! }
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! # let dir = tempfile::tempdir().unwrap();
//! # let url = format!("sqlite://{}?mode=rwc", dir.path().join("doc.db").display());
//! let db = Db::connect(&DbOptions { url, ..DbOptions::default() }).await?;
//! db.execute_script(
//!     "CREATE TABLE deals (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL,
//!      amount_cents INTEGER NOT NULL, stage TEXT NOT NULL,
//!      created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
//! )
//! .await?;
//!
//! let mut deal = Deal::create(&db, Deal {
//!     title: "Renewal".into(),
//!     amount_cents: 120_000,
//!     ..Default::default()
//! })
//! .await?;
//! assert_eq!(deal.id, 1); // the database assigned the key
//!
//! deal.stage = Stage::Won;
//! assert!(deal.is_dirty_on([Deal::STAGE]));
//! deal.save(&db).await?;
//!
//! let won = Deal::query()
//!     .where_(Deal::STAGE.eq(Stage::Won))
//!     .where_(Deal::AMOUNT_CENTS.gte(100_000))
//!     .latest()
//!     .get(&db)
//!     .await?;
//! assert_eq!(won[0].title, "Renewal");
//! assert_eq!(Deal::query().sum::<i64>(&db, Deal::AMOUNT_CENTS).await?, 120_000);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! # }).unwrap();
//! ```
//!
//! The architecture, and where each later feature plugs in, is in the
//! crate's `DESIGN.md`.
#![forbid(unsafe_code)]

extern crate self as navree_orm;

#[cfg(feature = "access")]
pub mod access;
pub mod attributes;
pub mod binding;
pub mod broadcast;
pub mod cast;
mod collection;
mod column;
mod driver;
mod error;
pub mod events;
mod exec;
mod expr;
pub mod factory;
mod global_scopes;
pub mod hooks;
pub mod ir;
mod lifecycle;
mod model;
mod module;
mod operators;
mod options;
pub mod pagination;
mod persist;
pub mod prelude;
pub mod prune;
mod query;
mod quiet;
mod raw;
mod record;
pub mod relations;
pub mod render;
mod row;
mod scope;
pub mod show;
mod soft_deletes;
pub mod sql;
pub mod streams;
pub mod strict;
pub mod testing;
mod ulid;
#[cfg(feature = "validation")]
pub mod validation;
mod value;
pub mod workspace;

pub use collection::Collection;
pub use column::{
    Assignment, Column, ColumnName, ColumnOf, Decoder, Encoder, Filter, IntoExpr, IntoQuery,
    Untyped, col, col_as,
};
pub use driver::{DocumentDriver, InsertOutcome};
pub use error::{OrmError, Result};
pub use events::{Observer, ObserverHook, without_events};
pub use exec::{
    Access, ChangeRecorder, DocumentTarget, ExecContext, Executor, Route, RowsStream, SqlTarget,
    Target, scoped_connection, using_connection, using_writer,
};
pub use global_scopes::ClosureScope;
pub use ir::{Expr, LockMode, VectorMetric};
pub use lifecycle::ModelLifecycle;
pub use model::{
    ColumnMeta, DateSerializer, KeyKind, Model, ModelMeta, PartialSchemaFn, PrimaryKey, Timestamp,
};
pub use module::{
    ConfiguredOrmModule, ModuleBuilderOrmExt, OrmModule, OrmRuntime, RegisteredModel,
    registered_models,
};
pub use options::{DEFAULT_PRUNE_CHUNK, OrmOptions};
pub use pagination::{
    CursorPage, CursorRequest, Page, PageMeta, PageQuery, PageRequest, SimplePage, SimplePageMeta,
};
pub use query::{
    Builder, ConflictUpdate, HasQuery, HasWheres, IntoSelect, JoinClause, Selectable, TableBuilder,
    WhereGroup, table,
};
pub use raw::{Raw, affecting_statement, raw, scalar, select, select_one, statement, unprepared};
pub use record::Record;
pub use row::Row;
pub use scope::{GlobalScope, Scoped, SoftDeletes};
pub use soft_deletes::SoftDeleting;
pub use sql::{Dialect, Statement};
pub use streams::{RowStream, cursor};
pub use ulid::{Ulid, UlidError};
pub use value::{
    Decimal, Field, FieldError, FieldKind, Kind, Numeric, Value, ValueFor, is_null, same_value,
    to_json,
};

/// `#[derive(Model)]`, `#[scopes]`, `#[relations]`, `#[accessors]` and
/// `#[test]` (see their documentation in navree-orm-macros; `#[test]` needs
/// the `testing` feature for its databases).
pub use navree_orm_macros::{Model, accessor, accessors, relations, scopes, test};

/// What the macros expand to. Not part of the API.
#[doc(hidden)]
pub mod __private {
    pub use crate::attributes::{CastContext, NoAccessors, has_accessor};
    pub use crate::cast::{decode, decode_option, encode, encode_option};
    pub use crate::model::derive_support::*;
    pub use crate::relations::derive_support as relations;
    pub use chrono::{DateTime, FixedOffset, Utc};
    pub use navree_kernel::BoxFuture;
    pub use serde;
    pub use serde_json;
    pub use std::sync::{Arc, OnceLock};
    pub use tokio;
    pub use utoipa;
}

/// The models the documentation examples use, so every example compiles.
/// Not part of the API.
#[doc(hidden)]
#[path = "doc_models.rs"]
pub mod __doc_models;

/// The models and factories the factory, testing, binding and workspace
/// examples use. Not part of the API.
#[doc(hidden)]
#[path = "doc_factories.rs"]
pub mod __doc_factories;

/// The model the access-control examples use. Not part of the API.
#[cfg(feature = "access")]
#[doc(hidden)]
#[path = "doc_access.rs"]
pub mod __doc_access;
