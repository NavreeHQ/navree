//! [`app`]: the CRM API's root, used by `main` and by the tests.

use crm_activities::{ActivitiesModule, DashboardModule};
use crm_companies::{CompaniesModule, ContactsModule};
use crm_core::{CrmCoreModule, platform};
use crm_deals::DealsModule;
use crm_users::UsersModule;
use navree::prelude::*;

use crate::seed::DemoModule;

/// The Navree CRM API: the platform modules ([`crm_core::platform`]), the
/// MCP server at `/mcp` (the deals and activities modules add its tools,
/// resources and prompts) and every CRM module, configured from `sources`.
pub fn app(sources: ConfigSources) -> AppBuilder {
    platform(Navree::app(), sources)
        .module(McpModule::from_config()) // section `mcp`; serves /mcp
        .module(CrmCoreModule)
        .module(UsersModule)
        .module(CompaniesModule)
        .module(ContactsModule)
        .module(DealsModule)
        .module(ActivitiesModule)
        .module(DashboardModule)
        .module(DemoModule)
    // navree:modules
}
