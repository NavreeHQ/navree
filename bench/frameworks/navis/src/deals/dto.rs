//! Request query and body, with their validation rules (navis-validation on
//! garde: every failure is a 400 in Navis's error format).

use garde::Validate;
use navis::db::PageQuery;
use serde::Deserialize;

use super::entity::Stage;

/// The largest `amount_cents`: 10^12 cents.
pub const MAX_AMOUNT_CENTS: i64 = 1_000_000_000_000;

/// The query of `GET /api/deals`: `page` ≥ 1 (default 1), `per_page`
/// 1..=100 (default 20). Anything else, a non-integer included, is a 400.
#[derive(Debug, Deserialize, Validate)]
pub struct DealsQuery {
    /// The 1-based page number.
    #[serde(default = "first_page")]
    #[garde(range(min = 1))]
    pub page: u64,
    /// The page size.
    #[serde(default = "default_per_page")]
    #[garde(range(min = 1, max = 100))]
    pub per_page: u64,
}

impl DealsQuery {
    /// The page to load.
    pub fn page(&self) -> PageQuery {
        PageQuery::new(self.page, self.per_page)
    }
}

fn first_page() -> u64 {
    1
}

fn default_per_page() -> u64 {
    20
}

/// The body of `POST /api/deals`.
#[derive(Debug, Deserialize, Validate)]
pub struct CreateDeal {
    /// 1..=200 characters.
    #[garde(length(chars, min = 1, max = 200))]
    pub title: String,
    /// 0..=10^12, an integer.
    #[garde(range(min = 0, max = MAX_AMOUNT_CENTS))]
    pub amount_cents: i64,
    /// One of `lead`, `qualified`, `proposal`, `negotiation`, `won`, `lost`.
    #[garde(skip)]
    pub stage: Stage,
}
