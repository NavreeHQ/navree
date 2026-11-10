//! The models the tests use, and their SQLite tables.

#![allow(dead_code)]

use chrono::{DateTime, NaiveDate, Utc};
use navree_orm::cast::{Cast, CastError};
use navree_orm::{Builder, Decimal, GlobalScope, Model, Ulid, col, scopes};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The tables, as one script.
pub const SCHEMA: &str = r#"
CREATE TABLE companies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    country TEXT NULL,
    active BOOLEAN NOT NULL DEFAULT 1,
    employees INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE deals (
    id BLOB PRIMARY KEY,
    company_id INTEGER NULL REFERENCES companies (id),
    title TEXT NOT NULL,
    amount_cents INTEGER NOT NULL,
    stage TEXT NOT NULL,
    priority INTEGER NOT NULL,
    meta TEXT NOT NULL,
    tags TEXT NOT NULL,
    probability REAL NOT NULL,
    value NUMERIC NOT NULL,
    closes_on TEXT NULL,
    closed_at TEXT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT NULL
);
CREATE TABLE events (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    payload TEXT NULL
);
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    color TEXT NOT NULL
);
CREATE TABLE user_accounts (
    account_id INTEGER PRIMARY KEY AUTOINCREMENT,
    display_name TEXT NOT NULL,
    score INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE articles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    published BOOLEAN NOT NULL,
    author TEXT NOT NULL
);
CREATE TABLE invoices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    total TEXT NOT NULL,
    lines TEXT NULL,
    previous_stage TEXT NULL,
    discount TEXT NULL
);
"#;

/// An integer-keyed model with timestamps.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[model(timestamps)]
pub struct Company {
    pub id: i64,
    pub name: String,
    pub country: Option<String>,
    pub active: bool,
    pub employees: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A deal's stage, stored as its name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    #[default]
    Lead,
    Qualified,
    ClosedWon,
    ClosedLost,
}

/// A deal's priority, stored as a number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    #[default]
    Low = 1,
    Normal = 2,
    High = 3,
}

impl From<Priority> for i64 {
    fn from(priority: Priority) -> i64 {
        priority as i64
    }
}

impl TryFrom<i64> for Priority {
    type Error = String;

    fn try_from(value: i64) -> Result<Self, String> {
        match value {
            1 => Ok(Priority::Low),
            2 => Ok(Priority::Normal),
            3 => Ok(Priority::High),
            other => Err(format!("{other} is not a priority")),
        }
    }
}

/// Free-form deal details, stored as JSON.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DealMeta {
    pub source: String,
    pub score: u32,
}

/// A UUID-keyed, soft-deleting model with casts of every kind.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "deals", timestamps, soft_deletes, per_page = 2)]
pub struct Deal {
    pub id: Uuid,
    pub company_id: Option<i64>,
    pub title: String,
    pub amount_cents: i64,
    #[model(cast = "string_enum")]
    pub stage: Stage,
    #[model(cast = "int_enum")]
    pub priority: Priority,
    #[model(cast = "json")]
    pub meta: DealMeta,
    #[model(cast = "json")]
    pub tags: Vec<String>,
    pub probability: f64,
    pub value: Decimal,
    pub closes_on: Option<NaiveDate>,
    pub closed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    /// Not a column.
    #[model(skip)]
    #[serde(skip)]
    pub scratch: String,
}

impl Deal {
    /// A deal with the given title and amount.
    pub fn new(title: &str, amount_cents: i64) -> Deal {
        Deal {
            title: title.to_owned(),
            amount_cents,
            ..Deal::default()
        }
    }
}

#[scopes]
impl Deal {
    /// Deals still in play.
    #[scope]
    fn open(query: Builder<Self>) -> Builder<Self> {
        query.where_not_in(Deal::STAGE, [Stage::ClosedWon, Stage::ClosedLost])
    }

    /// Deals of at least `cents`.
    #[scope]
    fn worth(query: Builder<Self>, cents: i64) -> Builder<Self> {
        query.where_(Deal::AMOUNT_CENTS.gte(cents))
    }

    /// Deals that are lost or tiny: a scope with an `OR`.
    #[scope]
    fn doubtful(query: Builder<Self>) -> Builder<Self> {
        query
            .where_(Deal::STAGE.eq(Stage::ClosedLost))
            .or_where(Deal::AMOUNT_CENTS.lt(100))
    }
}

/// A ULID-keyed model without timestamps.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "events")]
pub struct Event {
    pub id: Ulid,
    pub name: String,
    pub occurred_at: DateTime<Utc>,
    pub payload: Option<serde_json::Value>,
}

/// A model keyed by an application-set string, with an update timestamp
/// only.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(primary_key = "key", timestamps(created_at = false))]
pub struct Setting {
    pub key: String,
    pub value: String,
    pub updated_at: DateTime<Utc>,
}

/// A model with a unique column, for the find-or-create family.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub color: String,
}

/// A model with renamed columns, a custom key and a defaulted field.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "user_accounts", primary_key = "account_id")]
pub struct Account {
    pub account_id: i32,
    #[model(column = "display_name")]
    pub name: String,
    #[model(default)]
    pub score: i32,
}

/// Only published articles.
#[derive(Clone, Copy, Debug, Default)]
pub struct Published;

impl GlobalScope<Article> for Published {
    fn apply(&self, query: Builder<Article>) -> Builder<Article> {
        query.where_(Article::PUBLISHED.eq(true))
    }
}

/// Only one author's articles, or the house's.
#[derive(Clone, Debug, Default)]
pub struct ByAuthor(pub String);

impl GlobalScope<Article> for ByAuthor {
    fn apply(&self, query: Builder<Article>) -> Builder<Article> {
        query
            .where_(Article::AUTHOR.eq(self.0.clone()))
            .or_where(col("author").eq("house"))
    }
}

/// A model with global scopes.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(global_scopes(Published, ByAuthor("anu".to_owned())))]
pub struct Article {
    pub id: i64,
    pub title: String,
    pub published: bool,
    pub author: String,
}

/// An invoice line, stored inside a JSON column.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub item: String,
    pub cents: i64,
}

/// A model with a custom cast and nullable casts.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
pub struct Invoice {
    pub id: i64,
    #[model(cast = crate::models::CentsText)]
    pub total: i64,
    #[model(cast = "json")]
    pub lines: Option<Vec<Line>>,
    #[model(cast = "string_enum")]
    pub previous_stage: Option<Stage>,
    #[model(cast = "crate::models::CentsText")]
    pub discount: Option<i64>,
}

/// Cents stored as text: a custom cast.
pub struct CentsText;

impl Cast<i64> for CentsText {
    type Stored = String;

    fn get(stored: String) -> Result<i64, CastError> {
        stored
            .parse()
            .map_err(|_| CastError::new(format!("`{stored}` is not a number of cents")))
    }

    fn set(value: &i64) -> Result<String, CastError> {
        Ok(value.to_string())
    }
}
