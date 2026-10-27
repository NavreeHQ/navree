//! [`MediaItem`]: one attached file, and what it is presented as.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

/// One file attached to a model: a row of the `media` table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaItem {
    /// The id (UUID v7); also the directory of its files.
    pub id: Uuid,
    /// The model type (`HasMedia::MEDIA_TYPE`).
    pub model_type: String,
    /// The model's key (`HasMedia::media_key`).
    pub model_id: String,
    /// The collection.
    pub collection: String,
    /// A display name (by default the file name without its extension).
    pub name: String,
    /// The stored file name (sanitized).
    pub file_name: String,
    /// The type sniffed from the content.
    pub mime_type: String,
    /// The disk the original is on.
    pub disk: String,
    /// The disk the conversions are on.
    pub conversions_disk: String,
    /// The original's path on its disk.
    pub path: String,
    /// The original's size in bytes (after metadata stripping).
    pub size: u64,
    /// The width of images and videos, in pixels.
    pub width: Option<u32>,
    /// The height of images and videos, in pixels.
    pub height: Option<u32>,
    /// The conversions generated so far, by name.
    pub conversions: BTreeMap<String, GeneratedConversion>,
    /// What the module read from the file (`duration`, `video_codec`,
    /// `audio_codec` for video and audio).
    pub metadata: Map<String, Value>,
    /// The app's own data (`alt`, `caption`, ...).
    pub custom_properties: Map<String, Value>,
    /// The position in its collection, from 1.
    pub order: i32,
    /// When it was added.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// A generated conversion of a [`MediaItem`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedConversion {
    /// The path on the item's conversions disk.
    pub path: String,
    /// The type (`image/webp`, `video/mp4`).
    pub mime_type: String,
    /// The size in bytes.
    pub size: u64,
    /// The width in pixels (images and video).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// The height in pixels (images and video).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// The narrower copies of a `responsive()` conversion, narrowest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<ResponsiveVariant>,
    /// When it was generated.
    pub generated_at: DateTime<Utc>,
}

/// One narrower copy of a responsive conversion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponsiveVariant {
    /// The path on the item's conversions disk.
    pub path: String,
    /// The width in pixels.
    pub width: u32,
    /// The height in pixels.
    pub height: u32,
    /// The size in bytes.
    pub size: u64,
}

impl MediaItem {
    /// Whether the conversion `name` has been generated.
    pub fn has_conversion(&self, name: &str) -> bool {
        self.conversions.contains_key(name)
    }

    /// The custom property `key`.
    pub fn property(&self, key: &str) -> Option<&Value> {
        self.custom_properties.get(key)
    }

    /// Whether the file is an image (SVG included).
    pub fn is_image(&self) -> bool {
        self.mime_type.starts_with("image/")
    }

    /// Whether the file is a video.
    pub fn is_video(&self) -> bool {
        self.mime_type.starts_with("video/")
    }

    /// Whether the file is audio.
    pub fn is_audio(&self) -> bool {
        self.mime_type.starts_with("audio/")
    }

    /// The directory of the item's files on its disks.
    pub(crate) fn directory(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(dir, _)| dir)
    }
}

/// A [`MediaItem`] as an API returns it, with URLs: public URLs on disks
/// that have one, temporary signed URLs otherwise (see
/// [`Media::resource`](crate::Media::resource)).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct MediaResource {
    /// `"Media"`.
    #[serde(rename = "__type")]
    pub kind: String,
    /// The id.
    pub id: Uuid,
    /// The collection.
    pub collection: String,
    /// The display name.
    pub name: String,
    /// The file name.
    pub file_name: String,
    /// The type.
    pub mime_type: String,
    /// The size in bytes.
    pub size: u64,
    /// The width in pixels, for images and videos.
    pub width: Option<u32>,
    /// The height in pixels, for images and videos.
    pub height: Option<u32>,
    /// The original's URL.
    pub url: String,
    /// Each generated conversion's URL.
    pub conversions: BTreeMap<String, String>,
    /// The `srcset` of each responsive conversion.
    pub srcsets: BTreeMap<String, String>,
    /// The app's own data.
    #[schema(value_type = Object)]
    pub custom_properties: Map<String, Value>,
    /// The position in the collection.
    pub order: i32,
    /// When it was added.
    pub created_at: DateTime<Utc>,
}
