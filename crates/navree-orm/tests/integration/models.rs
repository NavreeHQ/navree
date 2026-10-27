//! The models and factories the integration tests use, and their tables.

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use navree_orm::factory::{FactoryContext, ModelFactory, Pivot, Relationships};
use navree_orm::{Model, Ulid};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The tables, as one script.
pub const SCHEMA: &str = r#"
CREATE TABLE companies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    country TEXT NULL,
    employees INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT NULL
);
CREATE TABLE deals (
    id BLOB PRIMARY KEY,
    company_id INTEGER NULL REFERENCES companies (id),
    slug TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    stage TEXT NOT NULL,
    amount_cents INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT NULL
);
CREATE TABLE activities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    deal_id BLOB NULL,
    subject_type TEXT NULL,
    subject_id BLOB NULL,
    kind TEXT NOT NULL,
    summary TEXT NOT NULL
);
CREATE TABLE tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
);
CREATE TABLE deal_tag (
    deal_id BLOB NOT NULL,
    tag_id INTEGER NOT NULL,
    source TEXT NULL,
    created_at TEXT NULL,
    updated_at TEXT NULL,
    PRIMARY KEY (deal_id, tag_id)
);
CREATE TABLE notes (
    id TEXT PRIMARY KEY,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL
);
"#;

/// A company: integer key, timestamps, soft deletes.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[model(timestamps, soft_deletes, factory = CompanyFactory)]
pub struct Company {
    pub id: i64,
    pub name: String,
    pub country: Option<String>,
    pub employees: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Companies with fake names and sizes.
#[derive(Default)]
pub struct CompanyFactory;

impl ModelFactory for CompanyFactory {
    type Model = Company;

    fn definition(&self, ctx: &mut FactoryContext<'_, Company>) -> Company {
        Company {
            name: ctx.fake().company(),
            employees: ctx.fake().number(1..=500),
            ..Company::default()
        }
    }
}

impl CompanyFactory {
    /// A large company.
    pub fn large(company: &mut Company) {
        company.employees = 10_000;
    }

    /// A company in Germany.
    pub fn german(company: &mut Company) {
        company.country = Some("DE".to_owned());
    }
}

/// A deal's stage, stored as its name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    #[default]
    Open,
    Won,
    Lost,
}

/// A deal: UUID key, a company, timestamps, soft deletes.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(timestamps, soft_deletes, factory = DealFactory, route_key = "slug")]
pub struct Deal {
    pub id: Uuid,
    pub company_id: Option<i64>,
    pub slug: String,
    pub title: String,
    #[model(cast = "string_enum")]
    pub stage: Stage,
    pub amount_cents: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Deals of a new company each.
#[derive(Default)]
pub struct DealFactory;

impl ModelFactory for DealFactory {
    type Model = Deal;

    fn definition(&self, ctx: &mut FactoryContext<'_, Deal>) -> Deal {
        Deal {
            company_id: ctx.belongs_to(Deal::COMPANY_ID, Company::factory()),
            slug: format!("{}-{}", ctx.fake().slug(), ctx.fake().alphanumeric(8).to_lowercase()),
            title: ctx.fake().title(),
            amount_cents: ctx.fake().number(1_000..=500_000),
            ..Deal::default()
        }
    }

    fn relationships(relationships: &mut Relationships<Deal>) {
        relationships
            .belongs_to::<Company>("company", Deal::COMPANY_ID)
            .has_many::<Activity>("activities", Activity::DEAL_ID)
            .belongs_to_many::<Tag>(
                "tags",
                Pivot::new("deal_tag", "deal_id", "tag_id").with_timestamps(),
            );
    }
}

impl DealFactory {
    /// A won deal.
    pub fn won(deal: &mut Deal) {
        deal.stage = Stage::Won;
    }

    /// A lost deal.
    pub fn lost(deal: &mut Deal) {
        deal.stage = Stage::Lost;
    }
}

/// An activity: on a deal, or on anything (polymorphic subject).
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "activities", factory = ActivityFactory)]
pub struct Activity {
    pub id: i64,
    pub deal_id: Option<Uuid>,
    pub subject_type: Option<String>,
    pub subject_id: Option<Uuid>,
    pub kind: String,
    pub summary: String,
}

/// Activities with fake summaries.
#[derive(Default)]
pub struct ActivityFactory;

impl ModelFactory for ActivityFactory {
    type Model = Activity;

    fn definition(&self, ctx: &mut FactoryContext<'_, Activity>) -> Activity {
        Activity {
            kind: ctx.fake().pick(&["call", "email", "meeting"]).to_owned(),
            summary: ctx.fake().sentence(),
            ..Activity::default()
        }
    }
}

/// A tag, attached to deals through `deal_tag`.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(factory = TagFactory)]
pub struct Tag {
    pub id: i64,
    pub name: String,
}

/// Tags with unique names.
#[derive(Default)]
pub struct TagFactory;

impl ModelFactory for TagFactory {
    type Model = Tag;

    fn definition(&self, ctx: &mut FactoryContext<'_, Tag>) -> Tag {
        Tag {
            name: format!("{}-{}", ctx.fake().word(), ctx.fake().alphanumeric(6)),
            ..Tag::default()
        }
    }
}

/// A note: ULID key, a creation timestamp only.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(timestamps(updated_at = false), factory = NoteFactory)]
pub struct Note {
    pub id: Ulid,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

/// Notes with a paragraph each.
#[derive(Default)]
pub struct NoteFactory;

impl ModelFactory for NoteFactory {
    type Model = Note;

    fn definition(&self, ctx: &mut FactoryContext<'_, Note>) -> Note {
        Note {
            body: ctx.fake().paragraph(),
            ..Note::default()
        }
    }
}
