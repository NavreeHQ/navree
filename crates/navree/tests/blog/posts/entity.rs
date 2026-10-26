//! The `posts` table.

use navree::access::Subject;
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// A blog post; a draft until published.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "posts")]
pub struct Model {
    /// A UUID v7.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The user who wrote it (abilities test `author_id`).
    pub author_id: Uuid,
    /// The title.
    pub title: String,
    /// The text.
    #[sea_orm(column_type = "Text")]
    pub body: String,
    /// Whether guests can read it.
    pub published: bool,
    /// When it was published.
    pub published_at: Option<DateTimeUtc>,
    /// When it was written.
    pub created_at: DateTimeUtc,
    /// When it last changed.
    pub updated_at: DateTimeUtc,
}

/// No relations are navigated.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// The access subject: rules name it `Post`, permissions `posts.<action>`.
impl Subject for Entity {
    const SUBJECT: &'static str = "Post";
    const PERMISSION_PREFIX: &'static str = "posts";
}

/// The subject in route attributes and extractors.
pub type Post = Entity;
