//! Shared fixtures: the docs' Fluent files, a `users` table, a controller
//! that translates, and the test app builders.

#![allow(dead_code)]

use chrono::NaiveDate;
use garde::Validate;
use navree_access::{AccessModule, AccessOptions};
use navree_db::{DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt, Uow};
use navree_http::Json;
use navree_i18n::{I18nModule, Locale, t};
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{controller, injectable, module};
use navree_testing::{TestApp, TestAppBuilder, TestClient, principal};
use navree_validation::{Valid, ValidationModule};
use navree_workspace::{NewWorkspace, Workspace, WorkspaceModule, Workspaces};
use sea_orm::sea_query::{ColumnDef, Query, Table};
use sea_orm::{ConnectionTrait, DbErr};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

/// The Fluent files of the docs (en, fr, de, pt-BR).
pub const FIXTURES: &str = "tests/fixtures/locales";

/// A contact, as in the docs' validation example.
#[derive(Debug, Deserialize, Validate)]
pub struct NewContact {
    #[garde(length(chars, min = 1))]
    pub first_name: String,
    #[garde(length(chars, min = 1))]
    pub last_name: String,
    #[garde(email)]
    #[serde(default)]
    pub email: Option<String>,
}

/// Routes that translate.
#[injectable]
pub struct DealsController;

#[controller("/deals")]
impl DealsController {
    /// The docs' `summary` handler.
    #[get("/summary")]
    #[public]
    async fn summary(&self, locale: Locale) -> Json<Value> {
        let updated = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
        Json(json!({
            "locale": locale.tag(),
            "headline": t!("deal-created", count = 3),
            "total": locale.format_money(4_200_000, "EUR"),
            "updated": locale.format_date(updated),
        }))
    }

    /// The signed-in user's locale.
    #[get("/mine")]
    #[authenticated]
    async fn mine(&self, locale: Locale) -> String {
        locale.tag().to_owned()
    }

    #[post("/contacts")]
    #[public]
    async fn create(&self, Valid(Json(contact)): Valid<Json<NewContact>>) -> Json<Value> {
        Json(json!({ "last_name": contact.last_name }))
    }
}

#[module(controllers = [DealsController])]
pub struct DealsModule;

/// `users(id, name, email)`, as navree-auth creates it.
struct CreateUsers;

impl MigrationName for CreateUsers {
    fn name(&self) -> &str {
        "m20261026_000001_create_users"
    }
}

#[async_trait]
impl MigrationTrait for CreateUsers {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table("users")
                    .col(ColumnDef::new("id").uuid().not_null().primary_key())
                    .col(ColumnDef::new("name").string().not_null())
                    .col(ColumnDef::new("email").string().not_null().unique_key())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table("users").to_owned())
            .await
    }
}

/// The `users` table, as migration set `auth`.
pub struct UsersModule;

impl Module for UsersModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("auth").migration(CreateUsers));
    }
}

/// An app with the fixtures in en, fr, de and pt-BR (`i18n.missing =
/// "error"`), the deals routes and validation; no database.
pub fn builder() -> TestAppBuilder {
    TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("i18n.locales", json!(["en", "fr", "de", "pt-BR"]))
        .config("i18n.dir", FIXTURES)
        .config("i18n.missing", "error")
        .module(I18nModule::from_config())
        .module(ValidationModule)
        .module(DealsModule)
}

/// [`builder`] with a temporary database, users and workspaces.
pub fn with_database() -> TestAppBuilder {
    builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(UsersModule)
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
}

/// A booted app with a database.
pub struct World {
    pub app: TestApp,
}

impl World {
    pub async fn new() -> World {
        World::with(with_database()).await
    }

    pub async fn with(builder: TestAppBuilder) -> World {
        World {
            app: builder.build().await,
        }
    }

    pub async fn db(&self) -> std::sync::Arc<Db> {
        self.app.get::<Db>().await
    }

    /// Inserts a user into `users`.
    pub async fn user(&self, name: &str) -> Uuid {
        let id = Uuid::now_v7();
        let email = format!("{}-{id}@navree.test", name.to_lowercase());
        self.db()
            .await
            .conn()
            .execute(
                Query::insert()
                    .into_table("users")
                    .columns(["id", "name", "email"])
                    .values_panic([id.into(), name.into(), email.into()]),
            )
            .await
            .unwrap();
        id
    }

    /// Runs `f` in a new unit of work, committed when it returns `Ok`.
    pub async fn work<T, E, F, Fut>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        let uow = self.db().await.uow(None, None).await;
        match uow.scope(f(uow.clone())).await {
            Ok(value) => {
                uow.commit().await.unwrap();
                Ok(value)
            }
            Err(error) => {
                uow.rollback().await.unwrap();
                Err(error)
            }
        }
    }

    /// Creates workspace `name` owned by `owner`.
    pub async fn workspace(&self, owner: Uuid, name: &str) -> Workspace {
        let workspaces = self.app.get::<Workspaces>().await;
        let name = name.to_owned();
        self.work(|uow| async move {
            workspaces
                .create(&uow, owner, NewWorkspace::named(name))
                .await
        })
        .await
        .unwrap()
    }

    pub fn as_user(&self, user: Uuid) -> TestClient {
        self.app.acting_as(principal(user))
    }
}
