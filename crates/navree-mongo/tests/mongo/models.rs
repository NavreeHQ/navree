//! The models of the tests: documents keyed by ObjectIds (stored as
//! `_id`), by counted integers, and by a field of their own.

#![allow(dead_code)]

use chrono::{DateTime, NaiveDate, Utc};
use navree_mongo::ObjectId;
use navree_orm::{Decimal, Model};
use serde::{Deserialize, Serialize};

/// A deal's stage, stored as its name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    #[default]
    Lead,
    Qualified,
    Won,
    Lost,
}

/// An embedded document.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DealMeta {
    pub source: String,
    pub score: i64,
    #[serde(default)]
    pub labels: Vec<String>,
}

/// A deal: an ObjectId key, timestamps, soft deletes, a cast enum, an
/// embedded document and an array.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[model(collection = "deals", driver = "mongodb", timestamps, soft_deletes)]
pub struct Deal {
    pub id: ObjectId,
    pub title: String,
    pub company_id: Option<ObjectId>,
    pub amount: i64,
    pub price: Decimal,
    #[model(cast = "string_enum")]
    pub stage: Stage,
    #[model(cast = "json")]
    pub meta: Option<DealMeta>,
    #[model(cast = "json")]
    pub tags: Vec<String>,
    pub closes_on: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl Deal {
    pub fn new(title: &str, amount: i64) -> Deal {
        Deal {
            title: title.to_owned(),
            amount,
            ..Deal::default()
        }
    }
}

/// A company.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[model(collection = "companies", driver = "mongodb", timestamps)]
pub struct Company {
    pub id: ObjectId,
    pub name: String,
    pub country: Option<String>,
    pub employees: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A ticket: an integer key the database assigns (counted in
/// `navree_sequences`).
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[model(collection = "tickets", driver = "mongodb")]
pub struct Ticket {
    pub id: i64,
    pub subject: String,
}

/// A sensor reading (time series).
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[model(collection = "readings", driver = "mongodb")]
pub struct Reading {
    pub id: ObjectId,
    pub sensor: String,
    pub celsius: f64,
    pub taken_at: DateTime<Utc>,
}

/// An account keyed by its email (the document's `_id` is MongoDB's own).
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[model(
    collection = "accounts",
    driver = "mongodb",
    primary_key = "email",
    key_type = "manual"
)]
pub struct Account {
    pub email: String,
    pub name: String,
    pub external: uuid::Uuid,
}

/// A workspace-scoped note (global scope tests).
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[model(
    collection = "notes",
    driver = "mongodb",
    global_scopes(crate::models::InWorkspace)
)]
pub struct Note {
    pub id: ObjectId,
    pub workspace_id: ObjectId,
    pub body: String,
}

/// The workspace the current task works in (a stand-in for
/// navree-workspace's scoping: a global scope on `workspace_id`).
pub struct InWorkspace;

tokio::task_local! {
    pub static WORKSPACE: ObjectId;
}

impl navree_orm::GlobalScope<Note> for InWorkspace {
    fn apply(&self, query: navree_orm::Builder<Note>) -> navree_orm::Builder<Note> {
        match WORKSPACE.try_with(|workspace| *workspace) {
            Ok(workspace) => query.where_(Note::WORKSPACE_ID.eq(workspace)),
            Err(_) => query,
        }
    }
}

const _: fn() = || {
    fn is_model<M: Model>() {}
    is_model::<Deal>();
};
