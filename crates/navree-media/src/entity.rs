//! The `media` table as a SeaORM entity.
//!
//! `media(id PRIMARY KEY, model_type, model_id, collection, name,
//! file_name, mime_type, disk, conversions_disk, path, size, width NULL,
//! height NULL, conversions JSON, metadata JSON, custom_properties JSON,
//! order_column, created_at, updated_at)`, indexed by `(model_type,
//! model_id, collection)`. Change it through the [`Media`](crate::Media)
//! service, which keeps the files in step; read it freely.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// One row of `media`.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "media")]
pub struct Model {
    /// The id.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The model type.
    pub model_type: String,
    /// The model's key.
    pub model_id: String,
    /// The collection.
    pub collection: String,
    /// The display name.
    pub name: String,
    /// The stored file name.
    pub file_name: String,
    /// The type.
    pub mime_type: String,
    /// The original's disk.
    pub disk: String,
    /// The conversions' disk.
    pub conversions_disk: String,
    /// The original's path.
    pub path: String,
    /// The original's size in bytes.
    pub size: i64,
    /// The width in pixels.
    pub width: Option<i32>,
    /// The height in pixels.
    pub height: Option<i32>,
    /// The generated conversions (`{name: {path, mime_type, size, ...}}`).
    #[sea_orm(column_type = "JsonBinary")]
    pub conversions: Json,
    /// What the module read from the file.
    #[sea_orm(column_type = "JsonBinary")]
    pub metadata: Json,
    /// The app's own data.
    #[sea_orm(column_type = "JsonBinary")]
    pub custom_properties: Json,
    /// The position in the collection.
    pub order_column: i32,
    /// When it was added.
    pub created_at: DateTimeUtc,
    /// When it last changed.
    pub updated_at: DateTimeUtc,
}

/// `media` has no relations: models are referenced by type and key.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
