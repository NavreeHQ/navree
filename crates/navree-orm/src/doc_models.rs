//! The models the documentation examples use (`Deal`, `Company`,
//! `Activity`, `Tag`, `Note`, `Setting`, `Event`) and their relations, with
//! every name the examples need in scope, so that each example compiles.
//! Not API: hidden from the docs, and it may change at any time.

pub use chrono::{DateTime, Duration, Utc};
pub use indexmap::IndexMap;
pub use navree_db::{Db, PageQuery, Uow};
pub use uuid::Uuid;

pub use crate::ir::FullText;
pub use crate::prelude::*;
pub use crate::{Page, Result};

use serde::{Deserialize, Serialize};

/// A deal's stage, stored as its name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Being worked on.
    #[default]
    Open,
    /// Closed and won.
    Won,
    /// Closed and lost.
    Lost,
}

/// A sales deal.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(timestamps)]
pub struct Deal {
    /// The key (a UUID v7, generated on insert).
    pub id: Uuid,
    /// The company the deal is with.
    pub company_id: Option<i64>,
    /// The user who owns the deal.
    pub owner_id: Option<i64>,
    /// The title.
    pub title: String,
    /// Free-text notes.
    pub notes: Option<String>,
    /// The stage.
    #[model(cast = "string_enum")]
    pub stage: Stage,
    /// The value, in cents.
    pub amount_cents: i64,
    /// How often the deal was viewed.
    pub views: i64,
    /// When the deal closed.
    pub closed_at: Option<DateTime<Utc>>,
    /// When the deal was created.
    pub created_at: DateTime<Utc>,
    /// When the deal last changed.
    pub updated_at: DateTime<Utc>,
}

#[relations]
impl Deal {
    /// The company the deal is with.
    pub fn company(&self) -> BelongsTo<Deal, Company> {
        self.belongs_to(Deal::COMPANY_ID)
    }

    /// The deal's activities.
    pub fn activities(&self) -> HasMany<Deal, Activity> {
        self.has_many(Activity::DEAL_ID)
    }

    /// The deal's latest activity.
    pub fn latest_activity(&self) -> HasOne<Deal, Activity> {
        self.has_many(Activity::DEAL_ID).latest_of_many()
    }

    /// The deal's tags, through `deal_tag`.
    pub fn tags(&self) -> BelongsToMany<Deal, Tag> {
        self.belongs_to_many("deal_tag")
            .with_pivot(["role"])
            .with_timestamps()
    }

    /// The notes on the deal.
    pub fn notes(&self) -> MorphMany<Deal, Note> {
        self.morph_many("notable")
    }
}

/// A customer company.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "companies")]
pub struct Company {
    /// The key (assigned by the database).
    pub id: i64,
    /// The name.
    pub name: String,
    /// Whether the company is a current customer.
    pub active: bool,
}

#[relations]
impl Company {
    /// The company's deals.
    pub fn deals(&self) -> HasMany<Company, Deal> {
        self.has_many(Deal::COMPANY_ID)
    }
}

/// A call, meeting or task on a deal.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(timestamps)]
pub struct Activity {
    /// The key (assigned by the database).
    pub id: i64,
    /// The deal the activity belongs to.
    pub deal_id: Uuid,
    /// What it is about.
    pub subject: String,
    /// When it is due.
    pub due_at: DateTime<Utc>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

#[relations]
impl Activity {
    /// The deal the activity is on.
    pub fn deal(&self) -> BelongsTo<Activity, Deal> {
        self.belongs_to(Activity::DEAL_ID)
    }
}

/// A label on deals.
#[derive(Model, Clone, Debug, Default, Serialize)]
pub struct Tag {
    /// The key (assigned by the database).
    pub id: i64,
    /// The name.
    pub name: String,
    /// The display color.
    pub color: String,
}

#[relations]
impl Tag {
    /// The deals with the tag.
    pub fn deals(&self) -> BelongsToMany<Tag, Deal> {
        self.belongs_to_many("deal_tag")
            .with_pivot(["role"])
            .with_timestamps()
    }
}

/// A note on a deal or an event.
#[derive(Model, Clone, Debug, Default, Serialize)]
pub struct Note {
    /// The key (assigned by the database).
    pub id: i64,
    /// The morph name of what the note is on.
    pub notable_type: Option<String>,
    /// The key of what the note is on.
    pub notable_id: Option<Uuid>,
    /// The text.
    pub body: String,
}

#[relations]
impl Note {
    /// What the note is on: a deal or an event.
    pub fn notable(&self) -> MorphTo<Note> {
        self.morph_to("notable").targets::<(Deal, Event)>()
    }
}

/// An application setting.
#[derive(Model, Clone, Debug, Default, Serialize)]
pub struct Setting {
    /// The key (assigned by the database).
    pub id: i64,
    /// The setting's name.
    pub key: String,
    /// Its value.
    pub value: String,
}

/// Something that happened, for a feed.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(timestamps)]
pub struct Event {
    /// The key (a UUID v7, generated on insert).
    pub id: Uuid,
    /// What happened.
    pub name: String,
    /// When it was recorded.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}
