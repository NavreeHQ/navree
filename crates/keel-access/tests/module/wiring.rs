//! Module wiring: plain `AccessModule`, `for_subject`, subject conflicts
//! and the `Abilities` extractor.

use std::sync::Arc;

use http::Method;
use keel_access::{Abilities, AbilityFactory, AccessModule, READ, Subject, UPDATE};
use keel_db::DatabaseModule;
use keel_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
use keel_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use keel_testing::{TestApp, principal};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{PostsModule, app, give_role, insert_post};

struct Report;

impl Subject for Report {
    const SUBJECT: &'static str = "Report";
    const PERMISSION_PREFIX: &'static str = "reports";
}

/// Claims the `Post` subject with another type.
struct FakePost;

impl Subject for FakePost {
    const SUBJECT: &'static str = "Post";
    const PERMISSION_PREFIX: &'static str = "fake_posts";
}

struct ReportsController;

impl Injectable for ReportsController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ReportsController) })
    }
}

impl Controller for ReportsController {
    fn prefix(&self) -> &'static str {
        "/reports"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "/export", || async { "csv" })
                .permissions(&["reports.export"]),
            RouteDef::on(
                Method::GET,
                "!/can",
                |Abilities(ability): Abilities| async move {
                    Json(json!({
                        "read": ability.can(READ, "Post", None),
                        "update": ability.can(UPDATE, "Post", None),
                    }))
                },
            )
            .public(),
        ]
    }
}

struct ReportsModule;

impl Module for ReportsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_subject::<Report>())
            .controller::<ReportsController>();
    }
}

struct ConflictingModule;

impl Module for ConflictingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AccessModule::for_subject::<FakePost>());
    }
}

#[tokio::test]
async fn plain_access_module_grants_permissions_only() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        .module(AccessModule)
        .module(PostsModule)
        .build()
        .await;
    let (reader, nobody) = (Uuid::now_v7(), Uuid::now_v7());
    give_role(&app, reader, "reader", &["posts.read"]).await;
    insert_post(&app, "Draft", None, "draft", false).await;

    app.acting_as(principal(reader))
        .get("/api/posts")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("titles", json!(["Draft"]));
    app.acting_as(principal(nobody))
        .get("/api/posts")
        .send()
        .await
        .assert_error(403, None);
}

#[tokio::test]
async fn for_subject_maps_its_permission_prefix() {
    let (app, _audit) = {
        let audit = crate::support::RecordingAudit::default();
        let app = crate::support::builder(&audit)
            .module(ReportsModule)
            .build()
            .await;
        (app, audit)
    };
    let (analyst, other) = (Uuid::now_v7(), Uuid::now_v7());
    give_role(&app, analyst, "analyst", &["reports.export"]).await;

    let factory = app.get::<AbilityFactory>().await;
    assert_eq!(
        factory
            .permission_subjects()
            .get("reports")
            .map(String::as_str),
        Some("Report")
    );
    let ability = factory
        .for_principal(&Some(principal(analyst)))
        .await
        .unwrap();
    assert!(ability.has_unconditional("export", "Report"));

    app.acting_as(principal(analyst))
        .get("/api/reports/export")
        .send()
        .await
        .assert_status(200);
    app.acting_as(principal(other))
        .get("/api/reports/export")
        .send()
        .await
        .assert_error(403, None);
}

#[tokio::test]
async fn abilities_extractor_gives_guests_and_users_their_rules() {
    let audit = crate::support::RecordingAudit::default();
    let app = crate::support::builder(&audit)
        .module(ReportsModule)
        .build()
        .await;

    let guest: Value = app
        .client()
        .get("/can")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(guest, json!({ "read": true, "update": false }));
    app.acting_as(principal(Uuid::now_v7()))
        .get("/can")
        .send()
        .await
        .assert_json_path("update", json!(true));
}

#[tokio::test]
async fn two_types_claiming_one_subject_fail_the_boot() {
    let audit = crate::support::RecordingAudit::default();
    let error = crate::support::builder(&audit)
        .module(ConflictingModule)
        .try_build()
        .await
        .unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("subject `Post` is registered by both"),
        "{message}"
    );
}

#[tokio::test]
async fn the_app_has_one_authorizer_and_the_tables() {
    let (app, _audit) = app().await;
    // Booting proves the authorizer is registered (routes declare roles,
    // permissions and `can`); the version row exists from the migration.
    let service = app.get::<keel_access::AccessService>().await;
    assert_eq!(service.version().await.unwrap(), 1);
}
