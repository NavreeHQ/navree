//! # navree-mongo
//!
//! MongoDB for Navree: the ORM's models on collections, the connection as
//! a module, units of work on sessions, and the schema builder and
//! migrations on collections, with time-series collections as the
//! time-series engine.
//!
//! - **Models** name their collection and the driver
//!   (`#[model(collection = "deals", driver = "mongodb")]`); every builder
//!   method compiles to a filter document or an aggregation pipeline
//!   ([`compile`]), and records keep their dirty tracking, timestamps, soft
//!   deletes, scopes and hooks. Keys are [`ObjectId`]s made before the
//!   insert (stored as `_id`), counted integers, UUIDs, ULIDs or keys the
//!   application sets.
//! - **Executors**: [`MongoDb`] (the connection), [`MongoSession`] and
//!   [`MongoUow`] (a transaction with change events and after-commit
//!   hooks) run models like navree-db's `Db` and `Uow` do.
//! - **[`MongoModule`]** reads `[database.mongodb]` ([`MongoOptions`]),
//!   provides [`MongoDb`], checks readiness (`mongodb`) and registers the
//!   `mongo:migrate` commands.
//! - **Schema**: [`MongoSchema`] turns navree-schema's blueprints into
//!   collections with JSON-schema validators and indexes, and hypertables
//!   into time-series collections; [`MongoMigrator`] runs migrations
//!   ([`MongoMigration`]) and records them in a collection.
//! - **MongoDB's own language** where it says more: filter documents
//!   ([`MongoFilter`]), pipeline stages after a query ([`MongoQuery`]),
//!   results streamed from cursors ([`MongoStream`]), commands shown
//!   without running them ([`ToMongo`]). The builders' bulk writes
//!   (`insert_or_ignore`, `upsert`) skip or update conflicting documents
//!   with MongoDB's own commands.
//!
//! ## Models
//!
//! ```
//! use chrono::{DateTime, Utc};
//! use navree_mongo::ObjectId;
//! use navree_orm::Model;
//! use serde::{Deserialize, Serialize};
//!
//! /// Embedded in each deal.
//! #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
//! pub struct Terms {
//!     pub net_days: u32,
//!     pub currency: String,
//! }
//!
//! #[derive(Model, Clone, Debug, Default, Serialize, Deserialize)]
//! #[model(collection = "deals", driver = "mongodb", timestamps, soft_deletes)]
//! pub struct Deal {
//!     pub id: ObjectId,
//!     pub company_id: Option<ObjectId>,
//!     pub title: String,
//!     pub amount: i64,
//!     #[model(cast = "json")]
//!     pub terms: Option<Terms>, // an embedded document
//!     #[model(cast = "json")]
//!     pub tags: Vec<String>, // an array
//!     pub created_at: DateTime<Utc>,
//!     pub updated_at: DateTime<Utc>,
//!     pub deleted_at: Option<DateTime<Utc>>,
//! }
//!
//! let query = Deal::query()
//!     .where_(Deal::AMOUNT.gte(10_000))
//!     .where_json_contains("tags", "vip")
//!     .order_by_desc(Deal::CREATED_AT)
//!     .limit(25);
//! # use navree_mongo::ToMongo;
//! assert_eq!(
//!     query.to_mongo().unwrap().to_string(),
//!     concat!(
//!         r#"db.deals.find({"amount": {"$gte": 10000}, "tags": "vip", "deleted_at": null}, "#,
//!         r#"{"_id": 1, "company_id": 1, "title": 1, "amount": 1, "terms": 1, "tags": 1, "created_at": 1, "updated_at": 1, "deleted_at": 1})"#,
//!         r#".sort({"created_at": -1}).limit(25)"#,
//!     )
//! );
//! ```
//!
//! ## In an app
//!
//! ```no_run
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_kernel::Navree;
//! use navree_mongo::MongoModule;
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! // config/app.toml:
//! // [database.mongodb]
//! // url = "mongodb://127.0.0.1:27017/crm?replicaSet=rs0"
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(MongoModule::from_config())
//!     .run()
//! # }
//! ```
//!
//! Handlers and services take [`MongoDb`] from the app and run models on
//! it, or open a unit of work for writes that commit together:
//!
//! ```no_run
//! # use navree_mongo::{MongoDb, MongoResult};
//! # use navree_orm::Model;
//! # #[derive(Model, Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
//! # #[model(collection = "deals", driver = "mongodb")]
//! # struct Deal { id: navree_mongo::ObjectId, title: String, amount: i64 }
//! async fn close(mongo: &MongoDb, title: &str) -> MongoResult {
//!     let uow = mongo.begin().await?;
//!     let mut deal = Deal::query().where_(Deal::TITLE.eq(title)).first_or_fail(&uow).await?;
//!     deal.amount = 0;
//!     deal.save(&uow).await?;
//!     uow.after_commit(|| Box::pin(async { /* notify */ }));
//!     uow.commit().await
//! }
//! ```
//!
//! ## What MongoDB does differently
//!
//! - Transactions need a replica set or a sharded cluster (a single-node
//!   replica set does); schema changes run outside them.
//! - Joins, subqueries and unions run as `$lookup` and `$unionWith`; row
//!   locks, raw SQL and `RIGHT JOIN` are refused with
//!   [`OrmError::Unsupported`](navree_orm::OrmError::Unsupported), naming
//!   what to do instead.
//! - Comparisons follow SQL where it matters: no ordering comparison holds
//!   for NULL or a missing field, and NULL keys join nothing. Negations
//!   (`<>`, `NOT IN`, `NOT LIKE`, `where_not`) keep MongoDB's rule and
//!   match documents whose field is null or missing.
//! - Date-times are kept to the millisecond.
//!
//! ## Tests
//!
//! The suite checks the generated commands without a server and runs the
//! rest against `NAVREE_TEST_MONGODB_URL` (a replica set, e.g.
//! `mongodb://127.0.0.1:45017/navree_test?replicaSet=rs0` from
//! `scripts/dev-databases.ps1`), each test in a database of its own.
#![forbid(unsafe_code)]

mod bulk;
mod command;
pub mod compile;
mod connection;
mod convert;
mod driver;
mod error;
pub mod extended;
mod migrate;
mod module;
mod native;
mod object_id;
mod options;
pub mod schema;
mod stream;
mod uow;

pub use command::{
    Aggregate, Count, Delete, Find, Insert, MongoCommand, Update, UpdateDocument, Upsert,
};
pub use connection::{MongoDb, MongoExecutor, MongoSession};
pub use convert::{document_row, from_bson, to_bson};
pub use driver::MongoDriver;
pub use error::{MongoError, MongoResult};
pub use migrate::{
    MongoMigrateOptions, MongoMigration, MongoMigrationReport, MongoMigrationRun,
    MongoMigrationSet, MongoMigrationStatus, MongoMigrator, MongoRollbackOptions,
    NamedMongoMigration,
};
pub use module::{ModuleBuilderMongoExt, MongoModule};
pub use native::{ModelPipeline, MongoFilter, MongoQuery, RowPipeline, ToMongo};
pub use object_id::{OBJECT_ID_TYPE, ObjectId, ObjectIdError};
pub use options::{DEFAULT_CONNECTION, MongoConnectionOptions, MongoOptions};
pub use schema::{MongoBlueprint, MongoSchema};
pub use stream::MongoStream;
pub use uow::{MongoChangeObserver, MongoUow};

/// The official MongoDB driver, for what the module does not wrap.
pub use mongodb;

/// BSON: documents (`doc!`), values and ObjectIds.
pub use bson;
