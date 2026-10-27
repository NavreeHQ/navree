//! [`MediaRecord`]: the `media` table as a navree-orm model.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use navree_orm::Model;
use serde::Serialize;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{GeneratedConversion, MediaItem};

/// A row of `media`: one file attached to a model.
///
/// Read it freely (`MediaRecord::query()`, typed columns such as
/// `MediaRecord::COLLECTION`); change media through the
/// [`Media`](crate::Media) service, which keeps the files on the disks in
/// step with the rows. [`MediaItem`] is the same row with unsigned sizes,
/// as the service returns it (`MediaItem::from(record)`).
///
/// The UUID columns are native `UUID`s, except on MySQL and MariaDB, where
/// they are `BINARY(16)`; the JSON columns are `JSONB` on PostgreSQL and
/// `JSON` on MySQL.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "media")]
pub struct MediaRecord {
    /// The id (a UUID v7); also the directory of the item's files.
    pub id: Uuid,
    /// The model type (`HasMedia::MEDIA_TYPE`).
    pub model_type: String,
    /// The model's key (`HasMedia::media_key`).
    pub model_id: String,
    /// The collection.
    pub collection: String,
    /// The display name.
    pub name: String,
    /// The stored file name.
    pub file_name: String,
    /// The type sniffed from the content.
    pub mime_type: String,
    /// The disk the original is on.
    pub disk: String,
    /// The disk the conversions are on.
    pub conversions_disk: String,
    /// The original's path on its disk.
    pub path: String,
    /// The original's size in bytes.
    pub size: i64,
    /// The width of images and videos, in pixels.
    pub width: Option<i32>,
    /// The height of images and videos, in pixels.
    pub height: Option<i32>,
    /// The generated conversions, by name.
    #[model(cast = "json")]
    pub conversions: BTreeMap<String, GeneratedConversion>,
    /// What the module read from the file.
    #[model(cast = "json")]
    pub metadata: Map<String, Value>,
    /// The app's own data (`alt`, `caption`, ...).
    #[model(cast = "json")]
    pub custom_properties: Map<String, Value>,
    /// The position in the collection, from 1.
    pub order_column: i32,
    /// When it was added.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<MediaRecord> for MediaItem {
    fn from(row: MediaRecord) -> Self {
        MediaItem {
            id: row.id,
            model_type: row.model_type,
            model_id: row.model_id,
            collection: row.collection,
            name: row.name,
            file_name: row.file_name,
            mime_type: row.mime_type,
            disk: row.disk,
            conversions_disk: row.conversions_disk,
            path: row.path,
            size: u64::try_from(row.size).unwrap_or_default(),
            width: row.width.and_then(|w| u32::try_from(w).ok()),
            height: row.height.and_then(|h| u32::try_from(h).ok()),
            conversions: row.conversions,
            metadata: row.metadata,
            custom_properties: row.custom_properties,
            order: row.order_column,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

impl From<&MediaItem> for MediaRecord {
    fn from(item: &MediaItem) -> Self {
        MediaRecord {
            id: item.id,
            model_type: item.model_type.clone(),
            model_id: item.model_id.clone(),
            collection: item.collection.clone(),
            name: item.name.clone(),
            file_name: item.file_name.clone(),
            mime_type: item.mime_type.clone(),
            disk: item.disk.clone(),
            conversions_disk: item.conversions_disk.clone(),
            path: item.path.clone(),
            size: i64::try_from(item.size).unwrap_or(i64::MAX),
            width: item.width.and_then(|w| i32::try_from(w).ok()),
            height: item.height.and_then(|h| i32::try_from(h).ok()),
            conversions: item.conversions.clone(),
            metadata: item.metadata.clone(),
            custom_properties: item.custom_properties.clone(),
            order_column: item.order,
            created_at: item.created_at,
            updated_at: item.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_and_items_round_trip() {
        let mut item = MediaItem::from(MediaRecord {
            id: Uuid::now_v7(),
            model_type: "Course".into(),
            model_id: "7".into(),
            size: 42,
            width: Some(640),
            height: Some(-1),
            order_column: 3,
            ..MediaRecord::default()
        });
        assert_eq!(item.size, 42);
        assert_eq!(item.width, Some(640));
        assert_eq!(item.height, None);
        assert_eq!(item.order, 3);
        item.height = Some(480);
        let row = MediaRecord::from(&item);
        assert_eq!(row.height, Some(480));
        assert_eq!(MediaRecord::meta().table(), "media");
    }
}
