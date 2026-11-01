//! [`Invoice`]: a provider's invoice, as `Billing::invoices` lists it.

use std::fmt;

use chrono::{DateTime, Utc};
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};

/// Where an invoice stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceStatus {
    /// Not finalized.
    Draft,
    /// Finalized, waiting for payment.
    Open,
    /// Paid.
    Paid,
    /// Canceled.
    Void,
    /// Written off.
    Uncollectible,
}

impl InvoiceStatus {
    /// `draft`, `open`, `paid`, `void` or `uncollectible`.
    pub fn as_str(self) -> &'static str {
        match self {
            InvoiceStatus::Draft => "draft",
            InvoiceStatus::Open => "open",
            InvoiceStatus::Paid => "paid",
            InvoiceStatus::Void => "void",
            InvoiceStatus::Uncollectible => "uncollectible",
        }
    }
}

impl fmt::Display for InvoiceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An invoice (on Paddle, a billed transaction).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Invoice {
    /// The provider's id (`in_...`, `txn_...`).
    pub id: String,
    /// The invoice number (empty until the provider assigns one).
    pub number: String,
    /// Where it stands.
    pub status: InvoiceStatus,
    /// The total, in the currency's smallest unit.
    pub total_cents: i64,
    /// The currency (lower-case ISO code).
    pub currency: String,
    /// The provider's PDF, when it has a link for it.
    pub pdf_url: Option<String>,
    /// The provider's hosted invoice page.
    pub hosted_url: Option<String>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
}
