//! The models and factories the documentation examples of factories,
//! testing assertions, route binding and workspaces use (`Company`, `Deal`,
//! `Activity`, `Tag`), with every name the examples need in scope. Not API:
//! hidden from the docs, and it may change at any time.

pub use chrono::{DateTime, Duration, Utc};
pub use navree_db::{Db, Uow};
pub use uuid::Uuid;

pub use crate::factory::{
    Factory, FactoryContext, HasFactory, ModelFactory, Pivot, Relationships, Sequence,
};
pub use crate::prelude::*;
pub use crate::{Result, Value};

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

/// A customer company.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "companies", factory = CompanyFactory)]
pub struct Company {
    /// The key (assigned by the database).
    pub id: i64,
    /// The name.
    pub name: String,
    /// Whether the company is a current customer.
    pub active: bool,
}

/// Companies with fake names.
#[derive(Default)]
pub struct CompanyFactory;

impl ModelFactory for CompanyFactory {
    type Model = Company;

    fn definition(&self, ctx: &mut FactoryContext<'_, Company>) -> Company {
        Company {
            name: ctx.fake().company(),
            active: true,
            ..Company::default()
        }
    }
}

/// A sales deal, belonging to a company.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(timestamps, soft_deletes, factory = DealFactory, route_key = "slug")]
pub struct Deal {
    /// The key (a UUID v7, generated on insert).
    pub id: Uuid,
    /// The company the deal is with.
    pub company_id: Option<i64>,
    /// The URL name.
    pub slug: String,
    /// The title.
    pub title: String,
    /// The stage.
    #[model(cast = "string_enum")]
    pub stage: Stage,
    /// The value, in cents.
    pub amount_cents: i64,
    /// When the deal was created.
    pub created_at: DateTime<Utc>,
    /// When the deal last changed.
    pub updated_at: DateTime<Utc>,
    /// When the deal was deleted.
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Deals of a new company each, with fake titles.
#[derive(Default)]
pub struct DealFactory;

impl ModelFactory for DealFactory {
    type Model = Deal;

    fn definition(&self, ctx: &mut FactoryContext<'_, Deal>) -> Deal {
        Deal {
            company_id: ctx.belongs_to(Deal::COMPANY_ID, Company::factory()),
            slug: ctx.fake().unique(|f| f.slug()),
            title: ctx.fake().title(),
            amount_cents: ctx.fake().number(1_000..=500_000),
            ..Deal::default()
        }
    }

    fn relationships(relationships: &mut Relationships<Deal>) {
        relationships
            .belongs_to::<Company>("company", Deal::COMPANY_ID)
            .has_many::<Activity>("activities", Activity::DEAL_ID)
            .belongs_to_many::<Tag>("tags", Pivot::new("deal_tag", "deal_id", "tag_id"));
    }
}

/// Deals are found under their company in nested routes.
impl crate::binding::ChildOf<Company> for Deal {
    fn children_of(company: &Record<Company>, query: Builder<Deal>) -> Builder<Deal> {
        query.where_(Deal::COMPANY_ID.eq(company.id))
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

/// A call, meeting or task on a deal (or, polymorphically, on anything).
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "activities", factory = ActivityFactory)]
pub struct Activity {
    /// The key (assigned by the database).
    pub id: i64,
    /// The deal the activity belongs to.
    pub deal_id: Uuid,
    /// What the activity is about: its type.
    pub subject_type: String,
    /// What the activity is about: its key.
    pub subject_id: Uuid,
    /// The summary.
    pub subject: String,
}

/// Activities with fake summaries.
#[derive(Default)]
pub struct ActivityFactory;

impl ModelFactory for ActivityFactory {
    type Model = Activity;

    fn definition(&self, ctx: &mut FactoryContext<'_, Activity>) -> Activity {
        Activity {
            subject: ctx.fake().sentence(),
            ..Activity::default()
        }
    }
}

/// A label on deals.
#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "tags", factory = TagFactory)]
pub struct Tag {
    /// The key (assigned by the database).
    pub id: i64,
    /// The name.
    pub name: String,
}

/// Tags with unique fake names.
#[derive(Default)]
pub struct TagFactory;

impl ModelFactory for TagFactory {
    type Model = Tag;

    fn definition(&self, ctx: &mut FactoryContext<'_, Tag>) -> Tag {
        Tag {
            name: ctx.fake().unique(|f| f.word()),
            ..Tag::default()
        }
    }
}
