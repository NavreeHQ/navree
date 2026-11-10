//! # navree-schema
//!
//! The schema builder and the migration language of Navree: tables,
//! columns, indexes and foreign keys described in Rust and compiled to the
//! right DDL for PostgreSQL (with TimescaleDB), MySQL/MariaDB and SQLite.
//!
//! - [`Schema`] creates, alters, renames and drops tables, reads the
//!   database's structure (`has_table`, `has_column`, `columns`,
//!   `indexes`, `foreign_keys`, ...), and manages TimescaleDB hypertables,
//!   policies and continuous aggregates. It runs on any
//!   [`SchemaConnection`] (navree-db's `Db`, pools, transactions and units
//!   of work implement it), [offline](Schema::offline) (recording the SQL for a given database),
//!   or [pretending](Schema::pretending) (reading the database, recording
//!   what it would change).
//! - [`Blueprint`] describes a table: every column type, the helpers
//!   (`id`, `uuid_primary`, `timestamps`, `soft_deletes`, `morphs`, ...),
//!   indexes and foreign keys (`foreign_id("company_id").constrained()`).
//! - [`Migration`] is what a migration file implements; [`migration!`]
//!   adds one to a module's `MigrationSet`, which `navree-db`'s migrator
//!   runs (`migrate`, `migrate:rollback`, `--pretend`, ...).
//! - [`Hypertable`], [`Compression`], [`ContinuousAggregate`] and
//!   [`RefreshPolicy`] put TimescaleDB in migrations; databases without it
//!   get regular tables (with time indexes) and plain views, and a notice.
//! - SQLite's `ALTER TABLE` limits are handled by rebuilding the table,
//!   keeping its data, indexes, triggers and constraints.
//! - [`Schema::dump`] writes the database's schema as an SQL script read
//!   from its catalogs (`schema:dump`), and [`Schema::load`] runs one,
//!   cut into statements by [`split_statements`].
//!
//! ```
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), navree_schema::SchemaError> {
//! use navree_schema::{Schema, Target};
//!
//! let schema = Schema::offline(Target::postgres(17).with_timescaledb((2, 17)));
//! schema
//!     .create("deals", |t| {
//!         t.id();
//!         t.foreign_id("company_id").constrained().cascade_on_delete();
//!         t.string("title");
//!         t.decimal("amount", 12, 2);
//!         t.timestamps();
//!         t.soft_deletes();
//!     })
//!     .await?;
//! schema
//!     .create("events", |t| {
//!         t.uuid_primary();
//!         t.timestamp_tz("occurred_at");
//!         t.jsonb("payload");
//!         let events = t.hypertable("occurred_at");
//!         events.chunk_interval("1 day").retain_for("90 days");
//!         events.compress().order_by("occurred_at DESC").after("7 days");
//!     })
//!     .await?;
//! assert!(schema.statements().iter().any(|s| s.starts_with("SELECT create_hypertable")));
//! # Ok(())
//! # }
//! ```
//!
//! See `DESIGN.md` for the architecture: the grammars, SQLite rebuilds,
//! TimescaleDB keys and fallbacks, and how migrations run.
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod blueprint;
mod column;
mod connection;
mod dump;
mod error;
mod grammar;
mod index;
mod inspect;
mod interval;
mod migration;
mod naming;
mod options;
mod rebuild;
mod schema;
mod script;
mod target;
mod timescale;
mod value;

pub use blueprint::{Blueprint, CheckDefinition, Command};
pub use column::{ColumnBuilder, ColumnDefinition, ColumnType, FluentIndex, Identity};
pub use connection::{BoxFuture, SchemaConnection, SchemaRow, SchemaValue};
pub use dump::DumpOptions;
pub use error::{DatabaseError, SchemaError, SchemaResult};
pub use index::{
    Columns, ForeignKeyBuilder, ForeignKeyDefinition, IndexBuilder, IndexDefinition, IndexKind,
    IndexRef, ReferentialAction,
};
pub use inspect::{
    ColumnInfo, ForeignKeyInfo, Generation, IndexInfo, TableInfo, TimeDimension, ViewInfo,
};
pub use interval::{Interval, IntervalUnit};
pub use migration::{Migration, MigrationContext, NamedMigration, SchemaMigration};
pub use options::{KeyStrategy, MorphKey, SchemaOptions, TimescaleMode};
pub use schema::Schema;
pub use script::split_statements;
pub use target::{Dialect, Target, Version};
pub use timescale::{AggregateSpec, Compression, ContinuousAggregate, Hypertable, RefreshPolicy};
pub use value::{DefaultValue, Expression, raw};

/// The names a migration file uses: `use navree_schema::prelude::*;`.
pub mod prelude {
    pub use crate::{
        Blueprint, Compression, ContinuousAggregate, Hypertable, KeyStrategy, ReferentialAction,
        RefreshPolicy, Schema, SchemaResult, raw,
    };
}
