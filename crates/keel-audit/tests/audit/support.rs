//! Test fixtures: a `notes` and a `users` table, a fake route authorizer and
//! app builders.

#![allow(dead_code)]

use std::collections::HashSet;
use std::sync::Arc;

use axum::http::request::Parts;
use keel_audit::{AuditLog, AuditModule, AuditRecord};
use keel_contracts::AuthState;
use keel_db::{DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt};
use keel_http::{AppError, RouteAuthorizer, RouteMeta};
use keel_kernel::{BoxFuture, Module, ModuleBuilder};
use keel_testing::{TestApp, TestAppBuilder};
use sea_orm::sea_query::{ColumnDef, Table};
use sea_orm::{ConnectionTrait, DbErr, EntityTrait, QueryOrder, Statement};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use uuid::Uuid;

/// The `notes` entity: uuid id, title, body.
pub mod note {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "notes")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub title: String,
        pub body: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// The `users` entity, with fields that must never reach the audit trail.
pub mod user {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "users")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub email: String,
        pub password_hash: String,
        pub remember_token: Option<String>,
        pub ssn: Option<String>,
        pub settings: Json,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// Creates `notes`, `users` and `sessions`.
pub struct CreateFixtures;

impl MigrationName for CreateFixtures {
    fn name(&self) -> &str {
        "m0001_create_fixtures"
    }
}

#[async_trait]
impl MigrationTrait for CreateFixtures {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("notes")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("title").string().not_null())
                    .col(ColumnDef::new("body").string().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table("users")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("email").string().not_null())
                    .col(ColumnDef::new("password_hash").string().not_null())
                    .col(ColumnDef::new("remember_token").string().null())
                    .col(ColumnDef::new("ssn").string().null())
                    .col(ColumnDef::new("settings").json().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("users").to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table("notes").to_owned())
            .await
    }
}

/// Registers the fixture migrations and a [`GrantAuthorizer`].
#[derive(Clone, Default)]
pub struct FixturesModule {
    pub authorizer: Arc<GrantAuthorizer>,
}

impl Module for FixturesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("fixtures").migration(CreateFixtures))
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

/// A test app with a temporary database, `DatabaseModule`, `AuditModule`
/// (configured from the `audit` section) and the fixtures.
pub fn builder() -> (TestAppBuilder, FixturesModule) {
    let fixtures = FixturesModule::default();
    let builder = TestApp::builder()
        .temp_database()
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
    keel_audit::audit_log::Entity::find()
        .order_by_asc(keel_audit::audit_log::Column::CreatedAt)
        .order_by_asc(keel_audit::audit_log::Column::Id)
        .all(db.conn())
        .await
        .expect("read audit rows")
}

/// `SELECT count(*) FROM table`.
pub async fn count(app: &TestApp, table: &str) -> i64 {
    let db = app.get::<Db>().await;
    let row = db
        .conn()
        .query_one_raw(Statement::from_string(
            db.backend(),
            format!("SELECT count(*) AS n FROM \"{table}\""),
        ))
        .await
        .expect("count query")
        .expect("count row");
    row.try_get::<i64>("", "n").expect("count")
}

/// The app's audit log.
pub async fn audit_log(app: &TestApp) -> Arc<AuditLog> {
    app.get::<AuditLog>().await
}

/// A new note.
pub fn new_note(title: &str) -> note::ActiveModel {
    use sea_orm::ActiveValue::Set;
    note::ActiveModel {
        id: Set(Uuid::now_v7()),
        title: Set(title.to_owned()),
        body: Set(format!("body of {title}")),
    }
}
