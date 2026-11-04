//! [`CompaniesModule`]: `/api/v1/companies`.

mod controller;
mod dto;
mod resource;
mod service;

use crm_core::CrmCoreModule;
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
)]
pub struct CompaniesModule;
