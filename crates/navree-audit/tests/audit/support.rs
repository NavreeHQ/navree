//! Test fixtures: a `notes` and a `users` table, a fake route authorizer and
//! app builders.

#![allow(dead_code)]

use std::collections::HashSet;
use std::sync::Arc;

use axum::http::request::Parts;
use navree_audit::{AuditLog, AuditModule, AuditRecord};
use navree_contracts::AuthState;
use navree_db::navree_schema::{
    Blueprint, ColumnBuilder, Dialect, Migration, Schema, SchemaMigration, SchemaResult,
};
use navree_db::{DatabaseModule, Db, ExecutorExt, MigrationSet, ModuleBuilderDbExt, Statement};
use navree_http::{AppError, RouteAuthorizer, RouteMeta};
use navree_kernel::{BoxFuture, Module, ModuleBuilder};
use navree_orm::Model as _;
use navree_testing::{TestApp, TestAppBuilder};
use uuid::Uuid;

/// The `notes` table: uuid id, title, body.
pub mod note {
    use serde::Serialize;
    use uuid::Uuid;

    #[derive(Clone, Debug, Default, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "notes")]
    pub struct Model {
        pub id: Uuid,
        pub title: String,
        pub body: String,
    }
}

/// A note.
pub type Note = note::Model;

/// The `users` table, with fields that must never reach the audit trail.
pub mod user {
    use serde::Serialize;
    use serde_json::Value;
    use uuid::Uuid;

    #[derive(Clone, Debug, PartialEq, navree_orm::Model, Serialize)]
    #[model(table = "users")]
    pub struct Model {
        pub id: Uuid,
        pub email: String,
        pub password_hash: String,
        pub remember_token: Option<String>,
        pub ssn: Option<String>,
        pub settings: Value,
    }
}

/// A user of the fixtures.
pub type User = user::Model;

/// A UUID column: `BINARY(16)` on MySQL (navree-db writes UUIDs as 16
/// bytes there), `uuid` elsewhere.
fn uuid<'a>(t: &'a mut Blueprint, name: &str, dialect: Dialect) -> ColumnBuilder<'a> {
    if dialect == Dialect::MySql {
        t.binary(name).length(16).fixed()
    } else {
        t.uuid(name)
    }
}

/// Creates `notes` and `users`.
pub struct CreateFixtures;

impl Migration for CreateFixtures {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let dialect = schema.target().await?.dialect();
        schema
            .create("notes", |t| {
                uuid(t, "id", dialect).primary();
                t.string("title");
                t.string("body");
            })
            .await?;
        schema
            .create("users", |t| {
                uuid(t, "id", dialect).primary();
                t.string("email");
                t.string("password_hash");
                t.string("remember_token").nullable();
                t.string("ssn").nullable();
                t.json("settings");
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("users").await?;
        schema.drop_if_exists("notes").await
    }
}

/// Registers the fixture migrations and a [`GrantAuthorizer`].
#[derive(Clone, Default)]
pub struct FixturesModule {
    pub authorizer: Arc<GrantAuthorizer>,
}

impl Module for FixturesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("fixtures").migration(SchemaMigration::new(
            "m0001_create_fixtures",
            CreateFixtures,
        )))
            .value_arc::<dyn RouteAuthorizer>(self.authorizer.clone())
            .exports::<dyn RouteAuthorizer>();
    }
}

/// Lets a request through when its principal was granted every permission
/// the route requires; 403 otherwise.
#[derive(Default)]
pub struct GrantAuthorizer {
    granted: std::sync::Mutex<HashSet<(Uuid, String)>>,
}

impl GrantAuthorizer {
    pub fn grant(&self, user: Uuid, permission: &str) {
        self.granted
            .lock()
            .unwrap()
            .insert((user, permission.to_owned()));
    }
}

impl RouteAuthorizer for GrantAuthorizer {
    fn authorize<'a>(
        &'a self,
        parts: &'a mut Parts,
        meta: &'a RouteMeta,
    ) -> BoxFuture<'a, Result<(), AppError>> {
        let user = parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|principal| principal.user_id);
        let allowed = user.is_some_and(|user| {
            let granted = self.granted.lock().unwrap();
            meta.access
                .permissions
                .iter()
                .all(|p| granted.contains(&(user, p.clone())))
        });
        Box::pin(async move {
            if allowed {
                Ok(())
            } else {
                Err(AppError::forbidden())
            }
        })
    }
}

/// A test app with a database of its own (on the server
/// `NAVREE_TEST_DATABASE` names; see [`servers`](crate::servers)),
/// `DatabaseModule`, `AuditModule` (configured from the `audit` section)
/// and the fixtures.
pub fn builder() -> (TestAppBuilder, FixturesModule) {
    with_modules(crate::servers::database(TestApp::builder()))
}

/// `builder` with the modules of [`builder`]: for an app whose database
/// the test chooses.
pub fn with_modules(builder: TestAppBuilder) -> (TestAppBuilder, FixturesModule) {
    let fixtures = FixturesModule::default();
    let builder = builder
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(DatabaseModule)
        .module(AuditModule)
        .module(fixtures.clone());
    (builder, fixtures)
}

/// [`builder`], booted.
pub async fn app() -> TestApp {
    builder().0.build().await
}

/// Every audit row, oldest first.
pub async fn audit_rows(app: &TestApp) -> Vec<AuditRecord> {
    let db = app.get::<Db>().await;
    AuditRecord::query()
        .order_by(AuditRecord::CREATED_AT)
        .order_by(AuditRecord::ID)
        .get(&*db)
        .await
        .expect("read audit rows")
        .into_models()
}

/// `SELECT count(*) FROM table`.
pub async fn count(app: &TestApp, table: &str) -> i64 {
    let db = app.get::<Db>().await;
    db.fetch_scalar::<i64>(&Statement::text(format!("SELECT count(*) FROM {table}")))
        .await
        .expect("count query")
}

/// The app's audit log.
pub async fn audit_log(app: &TestApp) -> Arc<AuditLog> {
    app.get::<AuditLog>().await
}

/// A new note.
pub fn new_note(title: &str) -> Note {
    Note {
        id: Uuid::now_v7(),
        title: title.to_owned(),
        body: format!("body of {title}"),
    }
}
