//! # navree-mongo
//!
//! MongoDB for Navree.
#![forbid(unsafe_code)]

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
mod uow;

pub use command::{Aggregate, Count, Delete, Find, Insert, MongoCommand, Update, UpdateDocument};
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
pub use uow::{MongoChangeObserver, MongoUow};

/// The official MongoDB driver, for what the module does not wrap.
pub use mongodb;

/// BSON: documents (`doc!`), values and ObjectIds.
pub use bson;
