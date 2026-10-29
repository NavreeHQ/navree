//! The `posts` table as an ORM model.

use chrono::{DateTime, Utc};
use navree::orm::Model;
use serde::Serialize;
use uuid::Uuid;

/// A blog post; a draft until published. The model is the access subject
/// `Post`, with permissions `posts.<action>`.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "posts", timestamps, subject)]
pub struct Post {
    /// A UUID v7.
    pub id: Uuid,
    /// The user who wrote it (abilities test `author_id`).
    pub author_id: Uuid,
    /// The title.
    pub title: String,
    /// The text.
    pub body: String,
    /// Whether guests can read it.
    pub published: bool,
    /// When it was published.
    pub published_at: Option<DateTime<Utc>>,
    /// When it was written.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}
