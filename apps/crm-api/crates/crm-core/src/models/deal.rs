//! [`Deal`]: opportunities moving through the pipeline (`deals`).

use chrono::{DateTime, NaiveDate, Utc};
use navree::orm::prelude::*;
use serde::Serialize;
use uuid::Uuid;

use super::{Activity, Company, Contact, User};
use crate::enums::Stage;
use crate::factories::DealFactory;

/// A deal. The probability comes from the stage ([`Stage::probability`]);
/// `position` orders deals within a stage on the board (0-based).
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(
    table = "deals",
    timestamps,
    subject,
    morph_name = "deal",
    factory = DealFactory
)]
pub struct Deal {
    /// A UUID v7, generated when the deal is created.
    pub id: Uuid,
    /// The title (required).
    pub title: String,
    /// The value in the currency's minor unit (cents), never negative.
    pub amount_cents: i64,
    /// The ISO 4217 code, e.g. `INR` (the default).
    pub currency: String,
    /// The pipeline stage.
    #[model(cast = "string_enum")]
    pub stage: Stage,
    /// The 0-based place within the stage's column on the board.
    pub position: i32,
    /// When the deal is expected to close.
    pub expected_close_date: Option<NaiveDate>,
    /// When it was won or lost; `None` while open.
    pub closed_at: Option<DateTime<Utc>>,
    /// Why it was lost (required when moved to `lost`).
    pub lost_reason: Option<String>,
    /// The company (required).
    pub company_id: Uuid,
    /// The main contact.
    pub contact_id: Option<Uuid>,
    /// The user who owns the deal.
    pub owner_id: Uuid,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl Deal {
    /// The chance of winning, in percent, from the stage.
    pub fn probability(&self) -> u8 {
        self.stage.probability()
    }

    /// `amount_cents` weighted by the probability (rounded down).
    pub fn weighted_amount_cents(&self) -> i64 {
        self.amount_cents * i64::from(self.probability()) / 100
    }
}

#[relations]
impl Deal {
    /// The company, deleted or not: a deal keeps naming its company after
    /// the company is deleted.
    pub fn company(&self) -> BelongsTo<Deal, Company> {
        self.belongs_to(Deal::COMPANY_ID).with_trashed()
    }

    /// The main contact, deleted or not.
    pub fn contact(&self) -> BelongsTo<Deal, Contact> {
        self.belongs_to(Deal::CONTACT_ID).with_trashed()
    }

    /// The user who owns the deal.
    pub fn owner(&self) -> BelongsTo<Deal, User> {
        self.belongs_to(Deal::OWNER_ID)
    }

    /// The notes, calls, emails, meetings and tasks attached to it.
    pub fn activities(&self) -> HasMany<Deal, Activity> {
        self.morph_many("subject")
    }
}

#[scopes]
impl Deal {
    /// Deals still open: neither won nor lost.
    #[scope]
    pub fn open(query: Builder<Self>) -> Builder<Self> {
        query.where_not_in(Deal::STAGE, [Stage::Won, Stage::Lost])
    }

    /// Deals in `stage`.
    #[scope]
    pub fn in_stage(query: Builder<Self>, stage: Stage) -> Builder<Self> {
        query.where_(Deal::STAGE.eq(stage))
    }

    /// Deals `user` owns.
    #[scope]
    pub fn owned_by(query: Builder<Self>, user: Uuid) -> Builder<Self> {
        query.where_(Deal::OWNER_ID.eq(user))
    }

    /// Deals of `company`.
    #[scope]
    pub fn of_company(query: Builder<Self>, company: Uuid) -> Builder<Self> {
        query.where_(Deal::COMPANY_ID.eq(company))
    }

    /// Deals whose title contains `term`, ignoring case (non-ASCII letters
    /// too, on every database).
    #[scope]
    pub fn search(query: Builder<Self>, term: &str) -> Builder<Self> {
        query.where_(Deal::TITLE.contains_ignore_case(term))
    }

    /// Deals won within `[start, end)`.
    #[scope]
    pub fn won_between(
        query: Builder<Self>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Builder<Self> {
        query
            .where_(Deal::STAGE.eq(Stage::Won))
            .where_(Deal::CLOSED_AT.gte(start))
            .where_(Deal::CLOSED_AT.lt(end))
    }

    /// Board order: by position, then age, then id, so ties resolve the
    /// same way every time.
    #[scope]
    pub fn in_board_order(query: Builder<Self>) -> Builder<Self> {
        query
            .order_by(Deal::POSITION)
            .order_by(Deal::CREATED_AT)
            .order_by(Deal::ID)
    }

    /// Newest first (by id among deals created at the same moment).
    #[scope]
    pub fn newest_first(query: Builder<Self>) -> Builder<Self> {
        query
            .order_by_desc(Deal::CREATED_AT)
            .order_by_desc(Deal::ID)
    }
}
