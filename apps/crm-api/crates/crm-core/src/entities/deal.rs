//! `deals`: opportunities moving through the pipeline. The probability is
//! derived from the stage ([`Stage::probability`]); `position` orders deals
//! within a stage on the board (0-based).

use sea_orm::entity::prelude::*;
use serde::Serialize;

use crate::enums::Stage;

/// A deal.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "deals")]
pub struct Model {
    /// A UUID v7.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The title (required).
    pub title: String,
    /// The value in the currency's minor unit (cents), never negative.
    pub amount_cents: i64,
    /// The ISO 4217 code, e.g. `USD` (the default).
    pub currency: String,
    /// The pipeline stage.
    pub stage: Stage,
    /// The 0-based place within the stage's column on the board.
    pub position: i32,
    /// When the deal is expected to close.
    pub expected_close_date: Option<Date>,
    /// When it was won or lost; `None` while open.
    pub closed_at: Option<DateTimeUtc>,
    /// Why it was lost (required when moved to `lost`).
    pub lost_reason: Option<String>,
    /// The company (required).
    pub company_id: Uuid,
    /// The main contact.
    pub contact_id: Option<Uuid>,
    /// The user who owns the deal.
    pub owner_id: Uuid,
    /// When it was created.
    pub created_at: DateTimeUtc,
    /// When it last changed.
    pub updated_at: DateTimeUtc,
}

impl Model {
    /// The chance of winning, in percent, from the stage.
    pub fn probability(&self) -> u8 {
        self.stage.probability()
    }

    /// `amount_cents` weighted by the probability (rounded down).
    pub fn weighted_amount_cents(&self) -> i64 {
        self.amount_cents * i64::from(self.probability()) / 100
    }
}

/// A deal belongs to a company and optionally a contact; the owner is a user.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// The company (`company_id` → `companies.id`).
    #[sea_orm(
        belongs_to = "super::company::Entity",
        from = "Column::CompanyId",
        to = "super::company::Column::Id"
    )]
    Company,
    /// The contact (`contact_id` → `contacts.id`).
    #[sea_orm(
        belongs_to = "super::contact::Entity",
        from = "Column::ContactId",
        to = "super::contact::Column::Id"
    )]
    Contact,
    /// The owner (`owner_id` → `users.id`).
    #[sea_orm(
        belongs_to = "navis::auth::user::Entity",
        from = "Column::OwnerId",
        to = "navis::auth::user::Column::Id"
    )]
    Owner,
}

impl Related<super::company::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Company.def()
    }
}

impl Related<super::contact::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Contact.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
