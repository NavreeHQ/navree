//! Module wiring: configuration, `for_root`, boot errors and navree-http's
//! route `audit` actions.

use std::sync::Arc;

use axum::http::Method;
use navree_audit::{AuditModule, AuditOptions, DEFAULT_SKIP_FIELDS};
use navree_config::{Config, ConfigModule, ConfigSources};
use navree_db::DatabaseModule;
use navree_http::{Controller, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, Resolver};
use navree_testing::{TestApp, principal};
use uuid::Uuid;

use crate::support::{self, audit_log, audit_rows};

#[test]
fn defaults() {
    let options = AuditOptions::default();
    assert!(options.enabled);
    assert_eq!(options.retention_days, 365);
    assert!(options.skip_entities.is_empty());
    assert_eq!(options.skip_fields, DEFAULT_SKIP_FIELDS);
    assert!(options.routes);
}

#[test]
fn lists_come_from_environment_strings() {
    let config = Config::load(&ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_AUDIT__SKIP_FIELDS", "ssn, users.phone"),
            ("APP_AUDIT__SKIP_ENTITIES", "sessions,jobs"),
            ("APP_AUDIT__RETENTION_DAYS", "30"),
            ("APP_AUDIT__ENABLED", "false"),
        ],
    ))
    .unwrap();
    let options = config.typed::<AuditOptions>().unwrap();
    assert_eq!(options.skip_fields, ["ssn", "users.phone"]);
    assert_eq!(options.skip_entities, ["sessions", "jobs"]);
    assert_eq!(options.retention_days, 30);
    assert!(!options.enabled);
}

#[test]
fn malformed_skip_field_fails_validation() {
    let config = Config::load(&ConfigSources::new().set("audit.skip_fields", "users.")).unwrap();
    let error = config.typed::<AuditOptions>().unwrap_err();
    assert!(error.to_string().contains("skip_fields"), "{error}");
}

#[tokio::test]
async fn for_root_needs_no_audit_configuration() {
    let (url, _guard) = crate::servers::scratch();
    let app = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new()
                .set("database.url", url)
                .set("database.migrate_on_boot", true)
                .set(
                    "database.native_units_of_work",
                    crate::servers::native_units_of_work(),
                )
                // Ignored: for_root options win.
                .set("audit.retention_days", 1),
        ))
        .module(DatabaseModule)
        .module(AuditModule::for_root(AuditOptions {
            retention_days: 90,
            routes: false,
            ..AuditOptions::default()
        }))
        .boot()
        .await
        .unwrap();
    let options = app.ctx().get::<AuditOptions>().await.unwrap();
    assert_eq!(options.retention_days, 90);
    let log = app.ctx().get::<navree_audit::AuditLog>().await.unwrap();
    assert_eq!(log.options().retention_days, 90);
}

#[tokio::test]
async fn boot_fails_without_database_module() {
    let error = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(AuditModule)
        .boot()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("Db"), "{error}");
    assert!(error.contains("AuditModule"), "{error}");
}

/// A controller with a route that writes a custom audit action.
struct ExportController;

impl Injectable for ExportController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(ExportController) })
    }
}

impl Controller for ExportController {
    fn prefix(&self) -> &'static str {
        "/exports"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::POST, "/", || async { "exported" })
                .authenticated()
                .audit("contacts.exported"),
        ]
    }
}

struct ExportModule;

impl Module for ExportModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<ExportController>();
    }
}

#[tokio::test]
async fn route_audit_actions_are_recorded() {
    let app = support::builder().0.module(ExportModule).build().await;
    let user = Uuid::now_v7();

    app.acting_as(principal(user))
        .post("/api/exports")
        .header("user-agent", "audit-tests")
        .send()
        .await
        .assert_status(200);
    audit_log(&app).await.flush().await;

    let rows = audit_rows(&app).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert_eq!(row.kind, "custom");
    assert_eq!(row.action, "contacts.exported");
    assert_eq!(row.actor_id, Some(user));
    assert_eq!(row.ip.as_deref(), Some("127.0.0.1"));
    assert_eq!(row.user_agent.as_deref(), Some("audit-tests"));
    assert!(row.request_id.is_some());
}

#[tokio::test]
async fn plain_and_from_config_read_the_audit_section() {
    let app = support::builder()
        .0
        .config("audit.retention_days", 7)
        .build()
        .await;
    assert_eq!(audit_log(&app).await.options().retention_days, 7);

    let app = crate::servers::database(TestApp::builder())
        .config("audit.retention_days", 8)
        .config("audit.routes", false)
        .module(DatabaseModule)
        .module(AuditModule::from_config())
        .build()
        .await;
    assert_eq!(audit_log(&app).await.options().retention_days, 8);
}
