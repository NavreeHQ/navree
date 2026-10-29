//! # navree-db
//!
//! `DatabaseModule`: Navree's own data layer on SQLx, the request unit of
//! work, module-owned migrations, seeders, factories and pagination.
//!
//! - The engine: [`Pool`]s of navree-db's own connections, [`Statement`]s
//!   (SQL and bound [`Value`]s; portable `?` placeholders), [`Row`]s read
//!   as any [`FromValue`] type, classified [`DbError`]s, [`Transaction`]s
//!   with savepoints, and the [`Executor`] they share with [`Db`] and
//!   [`Uow`]. Statements are timed and reported like everything else below.
//!   SeaORM still runs on the same connections ([`Db::conn`] is a view over
//!   the writer's pool) while crates move off it; [`compat`] bridges the
//!   two (see `docs/superpowers/specs/2026-10-28-navree-data-layer-on-sqlx.md`).
//! - [`DatabaseModule`] is global: it connects the pool at boot and exports
//!   [`Db`] and [`DbOptions`] (section `database`). TimescaleDB, Navree's
//!   default database, and plain PostgreSQL are behind the `postgres`
//!   feature; SQLite is behind the `sqlite` feature, on by default (its
//!   connections use WAL, `busy_timeout = 5000` and `foreign_keys = ON`);
//!   MySQL and MariaDB are behind the `mysql` feature (`mysql://` or
//!   `mariadb://` URLs; connections in UTC, `utf8mb4` and strict SQL mode
//!   over TLS when the server offers it, see [`MySqlOptions`]). It
//!   registers the `database` health check
//!   and the commands `migrate`, `migrate:install`, `migrate:rollback`,
//!   `migrate:reset`, `migrate:refresh`, `migrate:fresh`, `migrate:status`,
//!   `schema:dump`, `db:seed`, `db:monitor` and `db`.
//! - Connections: read replicas (`read_urls`, with `sticky` reads after a
//!   unit of work writes and [`Db::using_writer`]), named connections
//!   (`[database.connections.<name>]`, opened on first use through
//!   [`DbConnections`] and [`Db::connection`]), a direct connection past a
//!   transaction-mode pooler (`direct_url`, [`Db::direct`]; migrations run
//!   on it), PostgreSQL session settings ([`PostgresOptions`]) and SQLite
//!   pragmas ([`SqliteOptions`]). [`Db::pool_stats`],
//!   [`Db::open_connections`] and the server facts report; the
//!   [`ConnectionEstablished`] and [`DatabaseBusy`] events go through the
//!   app's `dyn EventSink`.
//! - Watching statements: [`QueryLog::capture`] returns what a future ran,
//!   [`Db::on_query`] listens to every statement,
//!   [`Db::before_executing`] and [`Db::before_starting_transaction`] run
//!   hooks, [`Db::when_querying_for_longer_than`] reacts to slow units of
//!   work, and every unit of work counts its statements
//!   ([`Uow::query_stats`]).
//! - [`Uow`] is the unit of work: one per HTTP request (opened by the
//!   module's [`LayerSlot::Uow`](navree_http::LayerSlot::Uow) layer, taken as
//!   an extractor), job run or command ([`Db::uow`]). Its transaction opens
//!   lazily; the layer commits when the status is below 400 and rolls back
//!   otherwise, and the request runs to completion even if the client
//!   disconnects. The actor and request id reach SQL as `navree.actor` and
//!   `navree.request_id` on PostgreSQL (`set_config`) and as
//!   `@navree_actor` and `@navree_request_id` on MySQL (session variables,
//!   cleared before the connection goes back to the pool), unless
//!   `database.transaction_settings` is off.
//!   [`ChangeObserver`]s see every recorded [`ChangeEvent`] inside the
//!   transaction, right before commit; after-commit hooks run once it is
//!   durable.
//! - [`Repo`] writes entities through a unit of work and records the
//!   change events (old and new values) automatically.
//! - [`Record`]s are typed rows on navree-db's statements alone, for the
//!   crates below navree-orm: one declaration ([`record!`]) lists the
//!   columns, and [`record`](mod@record)'s selects ([`Filter`], pages, `FOR UPDATE`),
//!   inserts, updates, upserts and deletes are built from it (with change
//!   events through a [`Uow`]). [`Db::read_executor`] reads where
//!   [`Db::reader`] would.
//! - [`MigrationSet`]s are contributed per module and run by the
//!   [`Migrator`] in dependency order (`after`), recorded in
//!   `navree_migrations` by batch; each migration runs in a transaction
//!   unless it opts out. Migrations are written with the schema builder
//!   ([`navree_schema`], re-exported) or with SeaORM; the migrator steps,
//!   pretends, rolls back, resets, refreshes, and loads and writes schema
//!   dumps.
//! - [`Seeder`]s fill the database (`db:seed`); [`Factory`] and
//!   [`create_many`] build test records.
//! - [`prune_time_series`] deletes the rows of a time-series table older
//!   than a cutoff; on a TimescaleDB hypertable it drops whole chunks
//!   instead of deleting row by row ([`prune_time_series_on`] runs on
//!   navree-db's executors).
//! - [`PageQuery`] / [`Page`] / [`paginate`] give offset
//!   pages (`{"data", "meta": {"current_page", "per_page", "total",
//!   "last_page"}}`); [`CursorQuery`] / [`CursorPage`] / [`cursor_paginate`]
//!   page by a UUID v7 key.
//! - Database errors returned from handlers with `?` map to error
//!   responses: record not found → 404, unique or foreign-key violation →
//!   409, anything else → 500 ([`map_db_error`]).
//!
//! ```no_run
//! use navree_db::{DatabaseModule, MigrationSet, ModuleBuilderDbExt};
//! use navree_kernel::{Navree, Module, ModuleBuilder};
//!
//! struct CrmModule;
//!
//! impl Module for CrmModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         // Runs after the `users` module's migrations (foreign keys).
//!         m.migrations(MigrationSet::new("crm").after(["users"]));
//!     }
//! }
//!
//! fn main() -> Result<(), navree_kernel::BoxError> {
//!     Navree::app()
//!         .module(navree_config::ConfigModule::for_root(
//!             navree_config::ConfigSources::defaults(),
//!         ))
//!         .module(DatabaseModule::from_config())
//!         .module(CrmModule)
//!         .run()
//! }
//! ```
//!
//! A handler using the unit of work:
//!
//! ```ignore
//! async fn store(uow: Uow, Json(input): Json<NewDeal>) -> navree_http::Result<Created<deal::Model>> {
//!     let deal = Repo::<deal::Entity>::insert(&uow, input.into_active_model()).await?;
//!     Ok(Created(format!("/api/v1/deals/{}", deal.id), deal))
//! }
//!
//! async fn index(uow: Uow, page: PageQuery) -> navree_http::Result<Page<deal::Model>> {
//!     Ok(paginate(deal::Entity::find(), &*uow.tx().await?, page).await?)
//! }
//! ```
#![forbid(unsafe_code)]

mod client;
mod connections;
mod db;
mod engine;
mod events;
mod http;
mod migrate;
mod module;
mod mysql;
mod options;
mod paginate;
mod queries;
mod reader;
pub mod record;
mod repo;
mod schema;
mod seed;
mod text;
mod time_series;
mod uow;

pub use client::ClientCommand;
pub use connections::DbConnections;
pub use db::{DEFAULT_CONNECTION, Db, PoolStats, ReadExecutor};
pub use engine::compat;
pub use engine::{
    Backend, ColumnIndex, DbError, ErrorKind, Executed, Executor, ExecutorExt, FromValue, HasKind,
    Kind, Pool, Row, RowStream, Statement, Transaction, TxOptions, Value, ValueError,
};
pub use events::{ConnectionEstablished, DatabaseBusy, DatabaseStatus};
pub use http::{MappedDbError, map_db_error};
pub use migrate::{
    FreshOptions, IntoMigration, LoadedDump, MIGRATIONS_TABLE, MigrateOptions, MigrationError,
    MigrationReport, MigrationRun, MigrationSet, MigrationStatus, Migrator, Refreshed,
    RollbackOptions, SetMigration, migration_kind,
};
pub use module::{DatabaseModule, ModuleBuilderDbExt};
pub use options::{DbOptions, IsolationLevel, MySqlOptions, PostgresOptions, SqliteOptions};
pub use paginate::{
    CursorPage, CursorQuery, DEFAULT_PER_PAGE, MAX_PER_PAGE, Page, PageMeta, PageQuery,
    cursor_paginate, cursor_paginate_desc, paginate,
};
pub use queries::{LoggedQuery, QueryLog, QueryStats, RepeatedQuery};
pub use reader::Reader;
pub use record::{Compare, Delete, Filter, Record, Select, Update};
pub use repo::Repo;
pub use seed::{
    Factory, SeedOptions, Seeder, SeederCalls, create, create_many, run_seeders, run_seeders_with,
    seeding_without_model_events,
};
pub use text::{contains_ignore_case, eq_ignore_case};
pub use time_series::{TimePoint, prune_time_series, prune_time_series_on};
pub use uow::{ChangeEvent, ChangeObserver, ChangeOp, Uow};

pub use navree_schema;
pub use sea_orm;
pub use sea_orm_migration;
