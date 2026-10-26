//! [`DealService`]: the deal queries behind `/api/deals`.

use chrono::Utc;
use navis::auth::User;
use navis::db::paginate;
use navis::prelude::*;
use sea_orm::ActiveValue::Set;
use sea_orm::{EntityTrait, QueryOrder};
use uuid::Uuid;

use super::dto::{CreateDeal, DealsQuery};
use super::entity::{self as deal, Entity as Deal};
use super::resource::{DealResource, OwnerRef};

/// Lists and creates deals in the caller's unit of work.
#[injectable]
pub struct DealService;

impl DealService {
    /// One page of every deal, newest first (`created_at`, then `id`), with
    /// its owner: a count, the page and one query for the page's owners,
    /// all in the request's transaction (so the total and the page agree).
    pub async fn list(&self, uow: &Uow, query: &DealsQuery) -> Result<Page<DealResource>> {
        let select = Deal::find()
            .order_by_desc(deal::Column::CreatedAt)
            .order_by_desc(deal::Column::Id);
        let tx = uow.tx().await?;
        let page = paginate(select, &*tx, query.page()).await?;
        Ok(Page {
            data: DealResource::load_many(&*tx, page.data).await?,
            meta: page.meta,
        })
    }

    /// Creates a deal owned by `owner`, through the unit of work (committed
    /// by the request pipeline once the response is a success).
    pub async fn create(&self, uow: &Uow, owner: &User, input: CreateDeal) -> Result<DealResource> {
        let now = Utc::now();
        let deal = Repo::<Deal>::insert(
            uow,
            deal::ActiveModel {
                id: Set(Uuid::now_v7()),
                title: Set(input.title),
                amount_cents: Set(input.amount_cents),
                stage: Set(input.stage),
                owner_id: Set(owner.id),
                created_at: Set(now),
                updated_at: Set(now),
            },
        )
        .await?;
        Ok(DealResource::new(deal, OwnerRef::from(owner)))
    }
}
