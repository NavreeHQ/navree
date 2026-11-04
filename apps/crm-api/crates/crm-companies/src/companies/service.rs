//! [`CompanyService`]: listing, showing, creating, editing and soft-deleting
//! companies.

use std::sync::Arc;

use crm_core::models::{Company, CompanyScopes, Deal};
use crm_core::owner_for;
use crm_core::subjects::COMPANY;
use navree::access::{Ability, Accessible, Read};
use navree::contracts::Clock;
use navree::orm::Record;
use navree::orm::access::AccessibleQuery;
use navree::orm::list::{ListQuery, ListQueryExt, ListResource};
use navree::prelude::*;
use uuid::Uuid;

use super::dto::{CompanyList, CreateCompany, UpdateCompany};
use super::resource::{CompanyDetail, CompanyPage, CompanyResource};
use crate::common::search_text;

/// Companies, as the API sees them: soft-deleted ones do not exist (the
/// model's soft deletes leave them out of every query, the route's record
/// included, so a deleted company is a 404). Writes run in the caller's
/// unit of work, so the audit trail records them with the request.
#[injectable]
pub struct CompanyService {
    clock: Arc<dyn Clock>,
}

impl CompanyService {
    /// One page of the companies `scope` allows that match `list` (read
    /// against [`CompanyList`]), by name unless it sorts, with their owners.
    pub async fn list(
        &self,
        uow: &Uow,
        scope: Accessible<Company, Read>,
        list: &ListQuery,
    ) -> Result<CompanyPage> {
        let page = scope
            .apply(Company::query())
            .list(list, &CompanyList::SPEC)?
            // The name or domain search and the industry, which ignores
            // case, are the model's own scopes.
            .when_some(search_text(list.search.as_deref()), |query, term| {
                query.search(term)
            })
            .when_some(search_text(list.eq("industry")), |query, industry| {
                query.in_industry(industry)
            })
            .scope(|query| CompanyList::order(query, list))
            .with_all(CompanyResource::EMBEDS)
            .paginate(uow, list.page_request(&CompanyList::SPEC))
            .await?;
        Ok(page
            .map(|company| company.to_resource::<CompanyResource>())
            .into())
    }

    /// `company` with its owner, its live contacts counted and its open
    /// deals counted and summed.
    pub async fn show(&self, uow: &Uow, mut company: Record<Company>) -> Result<CompanyDetail> {
        company.load_all(uow, CompanyResource::EMBEDS).await?;
        company.load_count(uow, ["contacts", "open_deals"]).await?;
        company
            .load_sum(uow, "open_deals", Deal::AMOUNT_CENTS)
            .await?;
        Ok(CompanyDetail::from_record(&company)?)
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
        let mut company = Company::create(uow, input.into_company(owner_id)).await?;
        company.load_all(uow, CompanyResource::EMBEDS).await?;
        Ok(company.to_resource())
    }

    /// Changes the fields `input` sends; a new owner needs `manage` on
    /// companies (else a 403). A request that changes nothing writes
    /// nothing.
    pub async fn update(
        &self,
        uow: &Uow,
        ability: &Ability,
        me: Uuid,
        mut company: Record<Company>,
        input: UpdateCompany,
    ) -> Result<CompanyResource> {
        let owner_id = owner_for(ability, COMPANY, me, Some(company.owner_id), input.owner_id)?;
        input.apply_to(&mut company);
        company.owner_id = owner_id;
        company.save(uow).await?;
        company.load_all(uow, CompanyResource::EMBEDS).await?;
        Ok(company.to_resource())
    }

    /// Soft-deletes `company`: sets `deleted_at`, so the row stays and the
    /// model leaves it out of every query from now on. It is written as an
    /// update, so the audit trail records it as `company.updated` setting
    /// `deleted_at`, which the web apps show as a deletion.
    pub async fn delete(&self, uow: &Uow, mut company: Record<Company>) -> Result<()> {
        let now = self.clock.now();
        company
            .update(uow, |company| company.deleted_at = Some(now))
            .await?;
        Ok(())
    }
}
