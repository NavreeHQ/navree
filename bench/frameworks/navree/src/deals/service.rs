//! [`DealService`]: the deal queries behind `/api/deals`.

use chrono::Utc;
use navree::auth::User;
use navree::orm::prelude::*;
use navree::prelude::*;
use uuid::Uuid;

use super::dto::{CreateDeal, DealsQuery};
use super::entity::Deal;
use super::resource::{DealResource, OwnerRef};

/// Lists and creates deals in the caller's unit of work.
#[injectable]
pub struct DealService;

impl DealService {
    /// One page of every deal, newest first (`created_at`, then `id`), with
    /// its owner: a count, the page and one query for the page's owners,
    /// all in the request's transaction (so the total and the page agree).
    pub async fn list(&self, uow: &Uow, query: &DealsQuery) -> Result<Page<DealResource>> {
        let page = Deal::query()
            .order_by_desc(Deal::CREATED_AT)
            .order_by_desc(Deal::ID)
            .paginate(uow, query.page())
            .await?;
        let deals: Vec<Deal> = page.data.into_iter().map(Record::into_inner).collect();
        Ok(Page {
            data: DealResource::load_many(uow, deals).await?,
            meta: page.meta,
        })
    }

    /// Creates a deal owned by `owner`, through the unit of work (committed
    /// by the request pipeline once the response is a success).
    pub async fn create(&self, uow: &Uow, owner: &User, input: CreateDeal) -> Result<DealResource> {
        let now = Utc::now();
        let deal = Deal::create(
            uow,
            Deal {
                id: Uuid::now_v7(),
                title: input.title,
                amount_cents: input.amount_cents,
                stage: input.stage,
                owner_id: owner.id,
                created_at: now,
                updated_at: now,
            },
        )
        .await?;
        Ok(DealResource::new(deal.into_inner(), OwnerRef::from(owner)))
    }
}
