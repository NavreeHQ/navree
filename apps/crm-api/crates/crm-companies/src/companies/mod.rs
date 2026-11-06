//! [`CompaniesModule`]: `/api/v1/companies`.

mod controller;
mod dto;
mod resource;
mod service;

use crm_core::CrmCoreModule;
use navree::api::{ModuleBuilderApiExt, RouteDoc};
use navree::prelude::*;

pub use controller::CompaniesController;
pub use dto::{CompanyList, CreateCompany, UpdateCompany};
pub use resource::{CompanyDetail, CompanyPage, CompanyResource, CompanyType};
pub use service::CompanyService;

/// Companies; see the [crate docs](crate).
#[module(
    imports = [CrmCoreModule],
    providers = [CompanyService],
    controllers = [CompaniesController],
    exports = [CompanyService],
    register = Self::wire,
)]
pub struct CompaniesModule;

impl CompaniesModule {
    /// The list query of `GET /api/v1/companies`, in the document.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.api_route(RouteDoc::get("/api/v1/companies").list::<CompanyList>());
    }
}
