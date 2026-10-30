//! # navree-db
//!
//! `DatabaseModule`: Navree's own data layer on SQLx, the request unit of
//! work, module-owned migrations, seeders, factories and pagination.
//!
//! - The engine: [`Pool`]s of navree-db's own connections, [`Statement`]s
//!   (SQL and bound [`Value`]s; portable `?` placeholders), [`Row`]s read
//!   as any [`FromValue`] type, classified [`DbError`]s, [`Transaction`]s
//!   with savepoints, and the [`Executor`] they share with [`Db`] and
//!   [`Uow`]. Statements are timed and reported like everything else below
//!   (see `docs/design`).
//! - [`DatabaseModule`] is global: it connects the pool at boot and exports
//!   [`Db`] and [`DbOptions`] (section `database`). TimescaleDB, Navree's
//!   default database, and plain PostgreSQL are behind the `postgres`
//!   feature; SQLite is behind the `sqlite` feature, off by default (its
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
//!   durable. navree-orm's models write through it and record the change
//!   events (old and new values) automatically.
//! - [`Record`]s are typed rows on navree-db's statements alone, for the
//!   crates below navree-orm: one declaration ([`record!`]) lists the
//!   columns, and [`record`](mod@record)'s selects ([`Filter`], pages, `FOR UPDATE`),
//!   inserts, updates, upserts and deletes are built from it (with change
//!   events through a [`Uow`]). [`Db::read_executor`] reads from the
//!   task's unit of work, the writer or a replica.
//! - [`MigrationSet`]s are contributed per module and run by the
//!   [`Migrator`] in dependency order (`after`), recorded in
//!   `navree_migrations` by batch; each migration runs in a transaction
//!   unless it opts out. Migrations are written with the schema builder
//!   ([`navree_schema`], re-exported); the migrator steps, pretends, rolls
//!   back, resets, refreshes, and loads and writes schema dumps.
//! - [`Seeder`]s fill the database (`db:seed`); navree-orm's factories
//!   build test records.
//! - [`prune_time_series`] deletes the rows of a time-series table older
//!   than a cutoff; on a TimescaleDB hypertable it drops whole chunks
//!   instead of deleting row by row.
//! - [`PageQuery`] / [`Page`] are offset pages (`{"data", "meta":
//!   {"current_page", "per_page", "total", "last_page"}}`); [`CursorQuery`]
//!   / [`CursorPage`] page by a UUID v7 key. navree-orm's builders fill
//!   them (`paginate`, `cursor_paginate`).
//! - [`contains_ignore_case`] and [`eq_ignore_case`] match text ignoring
//!   case beyond ASCII, the same on every database, as statement fragments.
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
//! A handler using the unit of work (a navree-orm model, and a statement):
//!
//! ```ignore
//! async fn store(uow: Uow, Json(input): Json<NewDeal>) -> navree_http::Result<Created<Record<Deal>>> {
//!     let deal = Deal::create(&uow, input.into()).await?;
//!     Ok(Created(format!("/api/v1/deals/{}", deal.id), deal))
//! }
//!
//! async fn stale(uow: Uow) -> navree_http::Result<Json<i64>> {
//!     let count: i64 = uow
//!         .fetch_scalar(&Statement::text("SELECT COUNT(*) FROM deals WHERE stage = 'lead'"))
//!         .await?;
//!     Ok(Json(count))
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
pub mod record;
mod schema;
mod seed;
mod text;
mod time_series;
mod uow;

pub use client::ClientCommand;
pub use connections::DbConnections;
pub use db::{DEFAULT_CONNECTION, Db, PoolStats, ReadExecutor};
pub use engine::{
    Backend, ColumnIndex, DbError, ErrorKind, Executed, Executor, ExecutorExt, FromValue, HasKind,
    Kind, Pool, Row, RowStream, Statement, Transaction, TxOptions, Value, ValueError,
};
pub use events::{ConnectionEstablished, DatabaseBusy, DatabaseStatus};
pub use http::map_db_error;
pub use migrate::{
    FreshOptions, LoadedDump, MIGRATIONS_TABLE, MigrateOptions, MigrationError, MigrationReport,
    MigrationRun, MigrationSet, MigrationStatus, Migrator, Refreshed, RollbackOptions,
    SetMigration, record_migration, recorded_migrations,
};
pub use module::{DatabaseModule, ModuleBuilderDbExt};
pub use options::{DbOptions, IsolationLevel, MySqlOptions, PostgresOptions, SqliteOptions};
pub use paginate::{
    CursorPage, CursorQuery, DEFAULT_PER_PAGE, MAX_PER_PAGE, Page, PageMeta, PageQuery,
};
pub use queries::{LoggedQuery, QueryLog, QueryStats, RepeatedQuery};
pub use record::{Compare, Delete, Filter, Record, Select, Update};
pub use seed::{
    SeedOptions, Seeder, SeederCalls, run_seeders, run_seeders_with, seeding_without_model_events,
};
pub use text::{contains_ignore_case, eq_ignore_case};
pub use time_series::{TimePoint, prune_time_series};
pub use uow::{ChangeEvent, ChangeObserver, ChangeOp, Uow};

pub use navree_schema;

#[cfg(test)]
mod tests {
    /// navree-db is its own data layer: no source file names another
    /// one's crates.
    #[test]
    fn the_sources_name_no_other_data_layer() {
        let banned = [concat!("sea", "_orm"), concat!("sea", "_query")];
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        let mut stack = vec![dir];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("reads src") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("reads a source file");
                for (number, line) in text.lines().enumerate() {
                    if banned.iter().any(|name| line.contains(name)) {
                        offenders.push(format!("{}:{}: {line}", path.display(), number + 1));
                    }
                }
            }
        }
        assert!(offenders.is_empty(), "{}", offenders.join("\n"));
    }
}
