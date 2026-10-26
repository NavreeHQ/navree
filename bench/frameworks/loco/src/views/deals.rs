use loco_rs::{model::query::PageResponse, prelude::*};
use serde::Serialize;

use crate::models::{deals, users};

#[derive(Debug, Serialize)]
pub struct OwnerResponse {
    pub id: Uuid,
    pub name: String,
}

impl From<users::Model> for OwnerResponse {
    fn from(user: users::Model) -> Self {
        Self {
            id: user.id,
            name: user.name,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DealResponse {
    pub id: Uuid,
    pub title: String,
    pub amount_cents: i64,
    pub stage: String,
    pub owner: OwnerResponse,
    pub created_at: DateTimeWithTimeZone,
}

impl DealResponse {
    #[must_use]
    pub fn new(deal: deals::Model, owner: users::Model) -> Self {
        Self {
            id: deal.id,
            title: deal.title,
            amount_cents: deal.amount_cents,
            stage: deal.stage,
            owner: owner.into(),
            created_at: deal.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PageMeta {
    pub current_page: u64,
    pub per_page: u64,
    pub total: u64,
    pub last_page: u64,
}

/// `GET /api/deals`: one page of deals plus Laravel-style page metadata.
#[derive(Debug, Serialize)]
pub struct DealPageResponse {
    pub data: Vec<DealResponse>,
    pub meta: PageMeta,
}

impl TryFrom<PageResponse<(deals::Model, Option<users::Model>)>> for DealPageResponse {
    type Error = Error;

    fn try_from(res: PageResponse<(deals::Model, Option<users::Model>)>) -> Result<Self> {
        let data = res
            .page
            .into_iter()
            .map(|(deal, owner)| {
                // `owner_id` is a NOT NULL foreign key, so the join always finds a row.
                let owner = owner.ok_or_else(|| Error::string("deal has no owner row"))?;
                Ok(DealResponse::new(deal, owner))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            data,
            meta: PageMeta {
                current_page: res.meta.page,
                per_page: res.meta.page_size,
                total: res.meta.total_items,
                last_page: res.meta.total_pages.max(1),
            },
        })
    }
}
