//! `/api/v1/companies`.

use crm_core::Company;
use navree::orm::list::List;
use navree::prelude::*;

use super::dto::{CompanyList, CreateCompany, UpdateCompany};
use super::resource::{CompanyDetail, CompanyPage, CompanyResource};
use super::service::CompanyService;

/// `/api/v1/companies`: everyone signed in with a CRM role reads and
/// creates companies; owners (and managers, admins) edit and delete them.
/// `from = "id"` routes load the company first (404 when there is none,
/// 403 when the rules say no).
#[injectable]
pub struct CompaniesController {
    companies: Arc<CompanyService>,
}

#[controller("/companies", version = "1")]
impl CompaniesController {
    /// A page of companies by name; `search` matches the name or domain.
    #[get("/")]
    #[can(Read, Company)]
    #[api(
        summary = "List companies",
        description = "Query: `search` (name or domain, case-insensitive), `filter[owner_id]`, `filter[industry]`, `sort` (`name`, `-name`, `created_at`, `-created_at`; default `name`), `page`, `per_page` (default 15, at most 100).",
        tag = "Companies",
        response = CompanyPage,
        responses(400, 401, 403)
    )]
    async fn index(
        &self,
        uow: Uow,
        scope: Accessible<Company, Read>,
        list: List<CompanyList>,
    ) -> Result<Json<CompanyPage>> {
        Ok(Json(self.companies.list(&uow, scope, &list).await?))
    }

    /// Creates a company, owned by the caller unless a manager names the
    /// owner.
    #[post("/")]
    #[can(Create, Company)]
    #[api(
        summary = "Create a company",
        tag = "Companies",
        request = CreateCompany,
        response = CompanyResource,
        status = 201,
        responses(400, 401, 403)
    )]
    async fn store(
        &self,
        uow: Uow,
        CurrentUser(me): CurrentUser,
        Abilities(ability): Abilities,
        ValidAsync(Json(input)): ValidAsync<Json<CreateCompany>>,
    ) -> Result<Created<CompanyResource>> {
        let company = self.companies.create(&uow, &ability, me.id, input).await?;
        Ok(Created(
            format!("/api/v1/companies/{}", company.id),
            company,
        ))
    }

    /// One company with its contact and open deal counts.
    #[get("/{id}")]
    #[can(Read, Company, from = "id")]
    #[api(
        summary = "Show a company",
        tag = "Companies",
        response = CompanyDetail,
        responses(401, 403, 404)
    )]
    async fn show(
        &self,
        uow: Uow,
        company: Authorized<Company, Read>,
    ) -> Result<Json<CompanyDetail>> {
        Ok(Json(self.companies.show(&uow, company.into_inner()).await?))
    }

    /// Changes the fields sent; `null` clears an optional field.
    #[patch("/{id}")]
    #[can(Update, Company, from = "id")]
    #[api(
        summary = "Update a company",
        tag = "Companies",
        request = UpdateCompany,
        response = CompanyResource,
        responses(400, 401, 403, 404)
    )]
    async fn update(
        &self,
        uow: Uow,
        CurrentUser(me): CurrentUser,
        company: Authorized<Company, Update>,
        ValidAsync(Json(input)): ValidAsync<Json<UpdateCompany>>,
    ) -> Result<Json<CompanyResource>> {
        let ability = company.ability().clone();
        let company = self
            .companies
            .update(&uow, &ability, me.id, company.into_inner(), input)
            .await?;
        Ok(Json(company))
    }

    /// Soft-deletes a company: it disappears from the API, its row stays.
    #[delete("/{id}")]
    #[can(Delete, Company, from = "id")]
    #[api(
        summary = "Delete a company",
        tag = "Companies",
        status = 204,
        responses(401, 403, 404)
    )]
    async fn destroy(&self, uow: Uow, company: Authorized<Company, Delete>) -> Result<NoContent> {
        self.companies.delete(&uow, company.into_inner()).await?;
        Ok(NoContent)
    }
}
