//! [`app`]: the CRM API's root, used by `main` and by the tests.

use crm_activities::{ActivitiesModule, DashboardModule};
use crm_companies::{CompaniesModule, ContactsModule};
use crm_core::{CrmCoreModule, platform};
use crm_deals::DealsModule;
use crm_users::UsersModule;
use navis::prelude::*;

use crate::seed::DemoModule;

/// The Navis CRM API: the platform modules ([`crm_core::platform`]) and
/// every CRM module, configured from `sources`.
pub fn app(sources: ConfigSources) -> AppBuilder {
    platform(Navis::app(), sources)
        .module(CrmCoreModule)
        .module(UsersModule)
        .module(CompaniesModule)
        .module(ContactsModule)
        .module(DealsModule)
        .module(ActivitiesModule)
        .module(DashboardModule)
        .module(DemoModule)
    // navis:modules
}
