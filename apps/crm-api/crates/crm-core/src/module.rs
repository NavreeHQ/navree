//! [`CrmCoreModule`]: the CRM tables, their access subjects and the roles
//! seeder.

use navree::api::{ModuleBuilderApiExt, RouteDoc};
use navree::openapi::{Parameter, Schema, Type};
use navree::prelude::*;

use crate::migrations::migrations;
use crate::models::{self, Activity, Company, Contact, Deal, User};
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
    /// Migrations, seeders and the models (no `#[module]` keys for
    /// them). Registered models are what `model:show` describes.
    fn wire(&self, m: &mut ModuleBuilder) {
        // Every client sends a JWT access token (the front ends' backend-for-frontend adds it): the
        // document has no cookie scheme.
        m.api_security(&["bearer"]);
        // The framework's lists, whose handlers read a few plain parameters.
        let paging = |doc: RouteDoc| {
            doc.query(
                Parameter::query(
                    "page",
                    Schema::of_type(Type::Integer).minimum(1.into()),
                )
                .described("The page number (default 1)."),
            )
            .query(
                Parameter::query(
                    "per_page",
                    Schema::of_type(Type::Integer)
                        .minimum(1.into())
                        .maximum(100.into())
                        .default_value(15.into()),
                )
                .described("The page size (default 15, at most 100)."),
            )
        };
        m.api_route(
            paging(RouteDoc::get("/api/notifications").optional()).query(
                Parameter::query("unread", Schema::of_type(Type::Boolean))
                    .described("`true`: only the notifications not read yet."),
            ),
        );
        m.api_route(
            paging(RouteDoc::get("/api/audit").optional())
                .query(
                    Parameter::query("actor_id", Schema::string().format("uuid"))
                        .described("Only what this user did."),
                )
                .query(
                    Parameter::query("subject_type", Schema::string())
                        .described("Only entries about this kind of record (`Deal`)."),
                )
                .query(
                    Parameter::query("subject_id", Schema::string().format("uuid"))
                        .described("Only entries about this record."),
                )
                .query(
                    Parameter::query("action", Schema::string())
                        .described("Only this action (`deals.updated`)."),
                ),
        );
        m.migrations(migrations());
        m.seeder::<RolesSeeder>();
        m.model::<models::Company>()
            .model::<models::Contact>()
            .model::<models::Deal>()
            .model::<models::Activity>()
            .model::<models::User>()
            .model::<models::Role>();
    }
}
