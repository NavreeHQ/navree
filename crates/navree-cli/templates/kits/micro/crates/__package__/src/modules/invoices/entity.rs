//! The `invoices` table as an ORM model.

use chrono::{DateTime, Utc};
use navree::orm::Model;
use serde::Serialize;
use uuid::Uuid;

/// An invoice, as stored. Other services see it as the contracts crate's
/// `Invoice` (`view` in `service.rs`). Query it with
/// `StoredInvoice::query()`; the ORM sets `id` (a UUID v7, so ids sort by
/// creation time) and the timestamps when it creates a record.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "invoices", timestamps)]
pub struct StoredInvoice {
    /// A UUID v7, generated when the invoice is created.
    pub id: Uuid,
    /// The deal it bills; one invoice per deal.
    pub deal_id: Uuid,
    /// The amount in the currency's minor unit (cents).
    pub amount_cents: i64,
    /// The ISO 4217 code, upper-case.
    pub currency: String,
    /// `draft`, `sent`, `paid` or `void`.
    pub status: String,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}
