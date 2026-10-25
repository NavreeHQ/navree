//! The API's deal shapes (spec §7): [`DealResource`] and its embeds.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use crm_core::entities::deal;
use crm_core::subjects::DEAL;
use crm_core::{CompanyRef, ContactRef, OwnerRef, Stage};
use sea_orm::{ConnectionTrait, DbErr};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

/// A deal as the API returns it (spec §7 `Deal`): the row plus
/// `probability` (from the stage) and the `company`, `contact` and `owner`
/// embeds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[schema(as = Deal)]
pub struct DealResource {
    /// Always `"Deal"` (the CASL subject type).
    #[serde(rename = "__type")]
    #[schema(example = "Deal")]
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
    /// The resources of `deals`, in order, with their embeds read through
    /// `conn` (one query per kind of embed).
    pub async fn load_many<C: ConnectionTrait>(
        conn: &C,
        deals: Vec<deal::Model>,
    ) -> Result<Vec<DealResource>, DbErr> {
        let embeds = Embeds::load(conn, &deals).await?;
        Ok(deals
            .into_iter()
            .map(|deal| embeds.resource(deal))
            .collect())
    }

    /// The resource of one deal.
    pub async fn load<C: ConnectionTrait>(
        conn: &C,
        deal: deal::Model,
    ) -> Result<DealResource, DbErr> {
        let mut resources = Self::load_many(conn, vec![deal]).await?;
        Ok(resources.remove(0))
    }
}

/// The embeds of a set of deals, by id.
struct Embeds {
    companies: HashMap<Uuid, CompanyRef>,
    contacts: HashMap<Uuid, ContactRef>,
    owners: HashMap<Uuid, OwnerRef>,
}

impl Embeds {
    async fn load<C: ConnectionTrait>(conn: &C, deals: &[deal::Model]) -> Result<Self, DbErr> {
        Ok(Embeds {
            companies: CompanyRef::load(conn, deals.iter().map(|deal| deal.company_id)).await?,
            contacts: ContactRef::load(conn, deals.iter().filter_map(|deal| deal.contact_id))
                .await?,
            owners: OwnerRef::load(conn, deals.iter().map(|deal| deal.owner_id)).await?,
        })
    }

    fn resource(&self, deal: deal::Model) -> DealResource {
        // Foreign keys keep the company and the owner; a missing row (never
        // expected) shows with an empty name rather than failing the page.
        let company = self
            .companies
            .get(&deal.company_id)
            .cloned()
            .unwrap_or_else(|| CompanyRef {
                id: deal.company_id,
                name: String::new(),
            });
        let owner = self
            .owners
            .get(&deal.owner_id)
            .cloned()
            .unwrap_or_else(|| OwnerRef {
                id: deal.owner_id,
                name: String::new(),
            });
        let contact = deal
            .contact_id
            .and_then(|id| self.contacts.get(&id).cloned());
        DealResource {
            resource_type: DEAL,
            id: deal.id,
            probability: deal.probability(),
            title: deal.title,
            amount_cents: deal.amount_cents,
            currency: deal.currency,
            stage: deal.stage,
            position: deal.position,
            expected_close_date: deal.expected_close_date,
            closed_at: deal.closed_at,
            lost_reason: deal.lost_reason,
            company_id: deal.company_id,
            company,
            contact_id: deal.contact_id,
            contact,
            owner_id: deal.owner_id,
            owner,
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

/// The pagination metadata (`keel::db::PageMeta`), for the documentation.
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
