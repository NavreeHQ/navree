//! [`CompanyService`]: listing, showing, creating, editing and soft-deleting
//! companies.

use std::sync::Arc;

use crm_core::entities::{company, contact, deal};
use crm_core::subjects::COMPANY;
use crm_core::{OwnerRef, Stage, owner_for};
use navree::access::{Ability, Accessible, Read};
use navree::contracts::Clock;
use navree::db::{eq_ignore_case, paginate};
use navree::prelude::*;
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::{Alias, Expr, ExprTrait, Func};
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, IntoActiveModel, Order, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use uuid::Uuid;

use super::dto::{CompanyFilters, CompanySort, CreateCompany, UpdateCompany};
use super::resource::{CompanyDetail, CompanyPage, CompanyResource};
use crate::common::{any_contains, search_text, set_some};

/// Companies, as the API sees them: soft-deleted ones do not exist. Writes
/// run in the caller's unit of work (`Repo`, so the audit trail records
/// them).
#[injectable]
pub struct CompanyService {
    clock: Arc<dyn Clock>,
}

impl CompanyService {
    /// One page of the companies `scope` allows, matching `filters`.
    pub async fn list(
        &self,
        uow: &Uow,
        scope: Accessible<company::Entity, Read>,
        filters: CompanyFilters,
        page: PageQuery,
    ) -> Result<CompanyPage> {
        let backend = uow.db().backend();
        let mut select = scope
            .apply(company::Entity::find())
            .filter(company::Column::DeletedAt.is_null());
        if let Some(search) = search_text(filters.search.as_deref()) {
            select = select.filter(any_contains(
                backend,
                &[company::Column::Name, company::Column::Domain],
                search,
            ));
        }
        if let Some(owner_id) = filters.owner_id {
            select = select.filter(company::Column::OwnerId.eq(owner_id));
        }
        if let Some(industry) = search_text(filters.industry.as_deref()) {
            select = select.filter(eq_ignore_case(
                backend,
                Expr::col(company::Column::Industry),
                industry,
            ));
        }
        let lower_name = || Expr::expr(Func::lower(Expr::col(company::Column::Name)));
        select = match filters.sort.unwrap_or_default() {
            CompanySort::Name => select.order_by(lower_name(), Order::Asc),
            CompanySort::NameDesc => select.order_by(lower_name(), Order::Desc),
            CompanySort::CreatedAt => select.order_by_asc(company::Column::CreatedAt),
            CompanySort::CreatedAtDesc => select.order_by_desc(company::Column::CreatedAt),
        }
        .order_by_asc(company::Column::Id);

        let tx = uow.tx().await?;
        let page = paginate(select, &*tx, page).await?;
        let owners = OwnerRef::load(&*tx, page.data.iter().map(|c| c.owner_id)).await?;
        Ok(page
            .map(|company| {
                let owner = owners.get(&company.owner_id).cloned();
                CompanyResource::new(company, owner)
            })
            .into())
    }

    /// `company` with its counts; a 404 for a deleted company.
    pub async fn show(&self, uow: &Uow, company: company::Model) -> Result<CompanyDetail> {
        let company = live(company)?;
        let tx = uow.tx().await?;
        let contacts_count = contact::Entity::find()
            .filter(contact::Column::CompanyId.eq(company.id))
            .filter(contact::Column::DeletedAt.is_null())
            .count(&*tx)
            .await?;
        let (open_deals_count, open_deals_amount_cents) = open_deals(&*tx, company.id).await?;
        let company = present(&*tx, company).await?;
        Ok(CompanyDetail {
            company,
            contacts_count,
            open_deals_count,
            open_deals_amount_cents,
        })
    }

    /// Creates a company owned by `me`, or by `input.owner_id` when
    /// `ability` may manage companies (else a 403).
    pub async fn create(
        &self,
        uow: &Uow,
        ability: &Ability,
        me: Uuid,
        input: CreateCompany,
    ) -> Result<CompanyResource> {
        let owner_id = owner_for(ability, COMPANY, me, None, input.owner_id)?;
        let now = self.clock.now();
        let company = Repo::<company::Entity>::insert(
            uow,
            company::ActiveModel {
                id: Set(Uuid::now_v7()),
                name: Set(input.name),
                domain: Set(input.domain),
                industry: Set(input.industry),
                size: Set(input.size),
                phone: Set(input.phone),
                website: Set(input.website),
                city: Set(input.city),
                country: Set(input.country),
                owner_id: Set(owner_id),
                created_at: Set(now),
                updated_at: Set(now),
                deleted_at: Set(None),
            },
        )
        .await?;
        present(&*uow.tx().await?, company).await
    }

    /// Changes the fields `input` sends; a new owner needs `manage` on
    /// companies (else a 403). A 404 for a deleted company.
    pub async fn update(
        &self,
        uow: &Uow,
        ability: &Ability,
        me: Uuid,
        company: company::Model,
        input: UpdateCompany,
    ) -> Result<CompanyResource> {
        let company = live(company)?;
        let owner_id = owner_for(ability, COMPANY, me, Some(company.owner_id), input.owner_id)?;
        let new_owner = (owner_id != company.owner_id).then_some(owner_id);
        let mut active = company.into_active_model();
        set_some(&mut active.name, input.name);
        set_some(&mut active.domain, input.domain);
        set_some(&mut active.industry, input.industry);
        set_some(&mut active.size, input.size);
        set_some(&mut active.phone, input.phone);
        set_some(&mut active.website, input.website);
        set_some(&mut active.city, input.city);
        set_some(&mut active.country, input.country);
        set_some(&mut active.owner_id, new_owner);
        active.updated_at = Set(self.clock.now());
        let company = Repo::<company::Entity>::update(uow, active).await?;
        present(&*uow.tx().await?, company).await
    }

    /// Soft-deletes `company` (sets `deleted_at`); a 404 when it already is.
    pub async fn delete(&self, uow: &Uow, company: company::Model) -> Result<()> {
        let mut active = live(company)?.into_active_model();
        active.deleted_at = Set(Some(self.clock.now()));
        Repo::<company::Entity>::update(uow, active).await?;
        Ok(())
    }
}

/// `company`, or a 404 when it is soft-deleted.
fn live(company: company::Model) -> Result<company::Model> {
    if company.deleted_at.is_some() {
        Err(AppError::not_found())
    } else {
        Ok(company)
    }
}

/// `company` with its owner embed.
async fn present<C: ConnectionTrait>(conn: &C, company: company::Model) -> Result<CompanyResource> {
    let owner = OwnerRef::load(conn, [company.owner_id])
        .await?
        .remove(&company.owner_id);
    Ok(CompanyResource::new(company, owner))
}

/// The number and total amount of the company's open deals.
async fn open_deals<C: ConnectionTrait>(conn: &C, company_id: Uuid) -> Result<(u64, i64)> {
    let (count, amount): (i64, Option<i64>) = deal::Entity::find()
        .select_only()
        .column_as(Expr::col(deal::Column::Id).count(), "count")
        .column_as(
            Expr::expr(Func::cast_as(
                Func::sum(Expr::col(deal::Column::AmountCents)),
                Alias::new("BIGINT"),
            )),
            "amount",
        )
        .filter(deal::Column::CompanyId.eq(company_id))
        .filter(deal::Column::Stage.is_not_in([Stage::Won, Stage::Lost]))
        .into_tuple()
        .one(conn)
        .await?
        .unwrap_or((0, None));
    Ok((
        u64::try_from(count).unwrap_or_default(),
        amount.unwrap_or(0),
    ))
}
