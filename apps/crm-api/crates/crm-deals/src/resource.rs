//! The API's deal shapes: [`DealResource`] and its embeds.

use chrono::{DateTime, NaiveDate, Utc};
use crm_core::models::Deal;
use crm_core::subjects::DEAL;
use crm_core::{CompanyRef, ContactRef, OwnerRef, Stage};
use navree::api::ApiResource;
use navree::openapi::ToSchema;
use navree::orm::Record;
use navree::orm::attributes::Resource;
use serde::Serialize;
use uuid::Uuid;

/// The schema of `__type`.
fn deal_type() -> navree::openapi::Schema {
    crm_core::subjects::type_schema(DEAL)
}

/// A deal as the API returns it: the row plus
/// `probability` (from the stage) and the `company`, `contact` and `owner`
/// embeds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = Deal)]
pub struct DealResource {
    /// Always `"Deal"` (the CASL subject type).
    #[serde(rename = "__type")]
    #[schema(schema_with = deal_type)]
    pub resource_type: &'static str,
    /// The id.
    pub id: Uuid,
    /// The title.
    pub title: String,
    /// The value in the currency's minor unit (cents).
    pub amount_cents: i64,
    /// The ISO 4217 currency code.
    pub currency: String,
    /// The pipeline stage.
    pub stage: Stage,
    /// The chance of winning, in percent (from the stage).
    pub probability: u8,
    /// The 0-based place within the stage's column on the board.
    pub position: i32,
    /// When the deal is expected to close (`YYYY-MM-DD`).
    pub expected_close_date: Option<NaiveDate>,
    /// When it was won or lost; `null` while open.
    pub closed_at: Option<DateTime<Utc>>,
    /// Why it was lost.
    pub lost_reason: Option<String>,
    /// The company's id.
    pub company_id: Uuid,
    /// The company.
    pub company: CompanyRef,
    /// The main contact's id.
    pub contact_id: Option<Uuid>,
    /// The main contact.
    pub contact: Option<ContactRef>,
    /// The owner's id.
    pub owner_id: Uuid,
    /// The owner.
    pub owner: OwnerRef,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl DealResource {
    /// The relations a deal's resource embeds: load them with the records
    /// (`Deal::query().with_all(DealResource::EMBEDS)`), one query each for
    /// a whole page.
    pub const EMBEDS: [&'static str; 3] = ["company", "contact", "owner"];
}

impl ApiResource for DealResource {
    const TYPE: &'static str = DEAL;
}

impl Resource<Deal> for DealResource {
    /// `deal` with its loaded company and contact (deleted or not) and
    /// owner.
    fn from_record(deal: &Record<Deal>) -> Self {
        DealResource {
            resource_type: DEAL,
            id: deal.id,
            title: deal.title.clone(),
            amount_cents: deal.amount_cents,
            currency: deal.currency.clone(),
            stage: deal.stage,
            probability: deal.probability(),
            position: deal.position,
            expected_close_date: deal.expected_close_date,
            closed_at: deal.closed_at,
            lost_reason: deal.lost_reason.clone(),
            company_id: deal.company_id,
            company: CompanyRef::loaded(deal.loaded(Deal::company), deal.company_id),
            contact_id: deal.contact_id,
            contact: deal
                .loaded(Deal::contact)
                .and_then(Option::as_ref)
                .map(ContactRef::from),
            owner_id: deal.owner_id,
            owner: OwnerRef::loaded(deal.loaded(Deal::owner), deal.owner_id),
            created_at: deal.created_at,
            updated_at: deal.updated_at,
        }
    }
}

/// One column of the board: a stage with its deals in board order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct BoardStage {
    /// The stage.
    pub stage: Stage,
    /// Its column title.
    pub label: &'static str,
    /// Its chance of winning, in percent.
    pub probability: u8,
    /// How many deals it holds.
    pub count: u64,
    /// The sum of their `amount_cents` (across currencies).
    pub amount_cents: i64,
    /// The deals, by `position`.
    pub deals: Vec<DealResource>,
}

/// `GET /api/v1/deals/board`: every stage, in board order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct DealBoard {
    /// The columns, `lead` to `lost`.
    pub stages: Vec<BoardStage>,
}

impl DealBoard {
    /// The board of `deals` (already in board order): one column per stage,
    /// empty ones included.
    pub fn new(deals: Vec<DealResource>) -> Self {
        let mut stages: Vec<BoardStage> = Stage::ALL
            .iter()
            .map(|&stage| BoardStage {
                stage,
                label: stage.label(),
                probability: stage.probability(),
                count: 0,
                amount_cents: 0,
                deals: Vec::new(),
            })
            .collect();
        for deal in deals {
            if let Some(column) = stages.iter_mut().find(|column| column.stage == deal.stage) {
                column.count += 1;
                column.amount_cents = column.amount_cents.saturating_add(deal.amount_cents);
                column.deals.push(deal);
            }
        }
        DealBoard { stages }
    }
}

/// A page of deals (the OpenAPI shape of `GET /api/v1/deals`).
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct DealPage {
    /// The deals.
    pub data: Vec<DealResource>,
    /// Where the page is.
    #[schema(inline)]
    pub meta: PageMetaSchema,
}

/// The pagination metadata (`navree::db::PageMeta`), for the documentation.
#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub struct PageMetaSchema {
    /// The page number.
    pub current_page: u64,
    /// The page size.
    pub per_page: u64,
    /// The number of matching deals.
    pub total: u64,
    /// The last page number (at least 1).
    pub last_page: u64,
}
