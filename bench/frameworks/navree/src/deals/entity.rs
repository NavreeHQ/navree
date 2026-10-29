//! `deals` and its [`Stage`].

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A deal. `owner_id` points at navree-auth's `users`.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "deals")]
pub struct Model {
    /// A UUID (v7 for new deals).
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// 1..=200 characters.
    pub title: String,
    /// The value in cents, 0..=10^12.
    pub amount_cents: i64,
    /// The pipeline stage.
    pub stage: Stage,
    /// The user who owns the deal.
    pub owner_id: Uuid,
    /// When it was created.
    pub created_at: DateTimeUtc,
    /// When it last changed.
    pub updated_at: DateTimeUtc,
}

/// No relations are declared: `owner_id` points at navree-auth's `users`,
/// a navree-orm model, whose names `OwnerRef::load` reads by id.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// A deal's stage, stored and sent as the same lower-case string.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, EnumIter, DeriveActiveEnum, Serialize, Deserialize,
)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::N(20))")]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// `lead`.
    #[sea_orm(string_value = "lead")]
    Lead,
    /// `qualified`.
    #[sea_orm(string_value = "qualified")]
    Qualified,
    /// `proposal`.
    #[sea_orm(string_value = "proposal")]
    Proposal,
    /// `negotiation`.
    #[sea_orm(string_value = "negotiation")]
    Negotiation,
    /// `won`.
    #[sea_orm(string_value = "won")]
    Won,
    /// `lost`.
    #[sea_orm(string_value = "lost")]
    Lost,
}
