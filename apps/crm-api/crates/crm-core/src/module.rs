//! [`CrmCoreModule`]: the CRM tables, their access subjects and the roles
//! seeder.

use navree::prelude::*;

use crate::entities::{Activity, Company, Contact, Deal, User};
use crate::migrations::migrations;
use crate::seed::RolesSeeder;

/// What every CRM feature module builds on: the `crm` migration set (every
/// CRM table), the access subjects `Company`, `Contact`, `Deal`, `Activity`
/// and `User` (so `#[can(.., from = "id")]` routes load their records and
/// `companies.*`, ..., `users.*` permissions map to them) and the
/// `crm-roles` seeder.
///
/// Feature modules import it, so it runs (and seeds) before them:
/// `#[module(imports = [CrmCoreModule], ..)]`.
#[module(
    imports = [
        AccessModule::for_feature::<Company>(),
        AccessModule::for_feature::<Contact>(),
        AccessModule::for_feature::<Deal>(),
        AccessModule::for_feature::<Activity>(),
        AccessModule::for_feature::<User>(),
    ],
    register = Self::wire,
)]
pub struct CrmCoreModule;

impl CrmCoreModule {
    /// Migrations and seeders (no `#[module]` keys for them).
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(migrations());
        m.seeder::<RolesSeeder>();
    }
}
