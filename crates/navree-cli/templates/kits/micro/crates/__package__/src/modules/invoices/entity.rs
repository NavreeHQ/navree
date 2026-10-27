//! The `invoices` table.

use sea_orm::entity::prelude::*;
use serde::Serialize;

/// An invoice, as stored. Other services see it as the contracts crate's
/// `Invoice` (`view` in `service.rs`).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "invoices")]
pub struct Model {
    /// A UUID v7, so ids sort by creation time.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// The deal it bills; one invoice per deal.
    #[sea_orm(unique)]
    pub deal_id: Uuid,
    pub amount_cents: i64,
    pub currency: String,
    /// `draft`, `sent`, `paid` or `void`.
    pub status: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
