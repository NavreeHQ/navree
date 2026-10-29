//! [`Deal`] (`deals`) and its [`Stage`].

use chrono::{DateTime, Utc};
use navree::orm::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A deal. `owner_id` points at navree-auth's `users`.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "deals")]
pub struct Deal {
    /// A UUID (v7 for new deals).
    pub id: Uuid,
    /// 1..=200 characters.
    pub title: String,
    /// The value in cents, 0..=10^12.
    pub amount_cents: i64,
    /// The pipeline stage.
    #[model(cast = "string_enum")]
    pub stage: Stage,
    /// The user who owns the deal.
    pub owner_id: Uuid,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// A deal's stage, stored and sent as the same lower-case string.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// `lead`.
    #[default]
    Lead,
    /// `qualified`.
    Qualified,
    /// `proposal`.
    Proposal,
    /// `negotiation`.
    Negotiation,
    /// `won`.
    Won,
    /// `lost`.
    Lost,
}
