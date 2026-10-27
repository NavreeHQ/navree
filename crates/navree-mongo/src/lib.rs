//! # navree-mongo
//!
//! MongoDB for Navree.
#![forbid(unsafe_code)]

mod command;
pub mod compile;
mod convert;
mod error;
pub mod extended;
mod object_id;

pub use command::{Aggregate, Count, Delete, Find, Insert, MongoCommand, Update, UpdateDocument};
pub use convert::{document_row, from_bson, to_bson};
pub use error::{MongoError, MongoResult};
pub use object_id::{OBJECT_ID_TYPE, ObjectId, ObjectIdError};

/// The official MongoDB driver, for what the module does not wrap.
pub use mongodb;

/// BSON: documents (`doc!`), values and ObjectIds.
pub use bson;
