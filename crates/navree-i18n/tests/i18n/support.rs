//! Shared fixtures: the docs' Fluent files, a `users` table, a controller
//! that translates, and the test app builders.

#![allow(dead_code)]

use chrono::NaiveDate;
use garde::Validate;
use navree_access::{AccessModule, AccessOptions};
use navree_db::navree_schema::{Dialect, Migration, Schema, SchemaMigration, SchemaResult};
use navree_db::{DatabaseModule, Db, Executor, MigrationSet, ModuleBuilderDbExt, Statement, Uow};
use navree_http::Json;
use navree_i18n::{I18nModule, Locale, t};
use navree_kernel::{Module, ModuleBuilder};
use navree_macros::{controller, injectable, module};
use navree_testing::{TestApp, TestAppBuilder, TestClient, principal};
use navree_validation::{Country, Valid, ValidationModule, rules};
use navree_workspace::{NewWorkspace, Workspace, WorkspaceModule, Workspaces};
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
    /// An international type: its message key is `country`.
    #[garde(skip)]
    #[serde(default)]
    pub country: Option<Country>,
    /// An international rule: its message key is `unicode_name`.
    #[garde(inner(custom(rules::unicode_name)))]
    #[serde(default)]
    pub company: Option<String>,
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

impl Migration for CreateUsers {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        let mysql = schema.target().await?.dialect() == Dialect::MySql;
        schema
            .create("users", |t| {
                if mysql {
                    t.binary("id").length(16).fixed().primary();
                } else {
                    t.uuid("id").primary();
                }
                t.string("name");
                t.string("email").unique();
            })
            .await
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        schema.drop_if_exists("users").await
    }
}

/// The `users` table, as migration set `auth`.
pub struct UsersModule;

impl Module for UsersModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("auth").migration(SchemaMigration::new(
            "m20261026_000001_create_users",
            CreateUsers,
        )));
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

/// [`builder`] with a temporary database (on the server
/// `NAVREE_TEST_DATABASE` names), users and workspaces.
pub fn with_database() -> TestAppBuilder {
    crate::servers::database(builder())
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
            .execute(&Statement::portable(
                "INSERT INTO users (id, name, email) VALUES (?, ?, ?)",
                [id.into(), name.into(), email.into()],
            ))
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
