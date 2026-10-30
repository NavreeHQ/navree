//! [`CompaniesModule`]: `/api/v1/companies`.

mod controller;
mod dto;
mod resource;
mod service;

use crm_core::CrmCoreModule;
use navree::prelude::*;

pub use controller::CompaniesController;
pub use dto::{CompanyFilters, CompanySort, CreateCompany, UpdateCompany};
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
    /// Schemas the routes' documents refer to only through query strings.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.api_schema::<CompanySort>().api_schema::<CompanyFilters>();
    }
}
