//! # navree-db
//!
//! `DatabaseModule`: SeaORM integration, the request unit of work,
//! module-owned migrations, seeders, factories and pagination.
//!
//! - [`DatabaseModule`] is global: it connects the pool at boot and exports
//!   [`Db`] and [`DbOptions`] (section `database`). SQLite connections use
//!   WAL, `busy_timeout = 5000` and `foreign_keys = ON`; PostgreSQL is
//!   behind the `postgres` feature. It registers the `database` health
//!   check and the commands `migrate`, `migrate:rollback`,
//!   `migrate:fresh [--seed]`, `migrate:status` and `db:seed`.
//! - [`Uow`] is the unit of work: one per HTTP request (opened by the
//!   module's [`LayerSlot::Uow`](navree_http::LayerSlot::Uow) layer, taken as
//!   an extractor), job run or command ([`Db::uow`]). Its transaction opens
//!   lazily; the layer commits when the status is below 400 and rolls back
//!   otherwise, and the request runs to completion even if the client
//!   disconnects. [`ChangeObserver`]s see every recorded [`ChangeEvent`]
//!   inside the transaction, right before commit; after-commit hooks run
//!   once it is durable.
//! - [`Repo`] writes entities through a unit of work and records the
//!   change events (old and new values) automatically.
//! - [`MigrationSet`]s are contributed per module and run by the
//!   [`Migrator`] in dependency order (`after`), recorded in
//!   `navree_migrations`; each migration runs in a transaction unless it opts
//!   out.
//! - [`Seeder`]s fill the database (`db:seed`); [`Factory`] and
//!   [`create_many`] build test records.
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

mod db;
mod http;
mod migrate;
mod module;
mod mysql;
mod options;
mod paginate;
mod reader;
mod repo;
mod seed;
mod text;
mod uow;

pub use db::Db;
pub use http::map_db_error;
pub use migrate::{MIGRATIONS_TABLE, MigrationError, MigrationSet, MigrationStatus, Migrator};
pub use module::{DatabaseModule, ModuleBuilderDbExt};
pub use options::{DbOptions, MySqlOptions};
pub use paginate::{
    CursorPage, CursorQuery, DEFAULT_PER_PAGE, MAX_PER_PAGE, Page, PageMeta, PageQuery,
    cursor_paginate, cursor_paginate_desc, paginate,
};
pub use reader::Reader;
pub use repo::Repo;
pub use seed::{Factory, Seeder, create, create_many, run_seeders};
pub use text::{contains_ignore_case, eq_ignore_case};
pub use uow::{ChangeEvent, ChangeObserver, ChangeOp, Uow};

pub use sea_orm;
pub use sea_orm_migration;
