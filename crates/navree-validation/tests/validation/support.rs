//! The DTOs and the hand-written controller shared by the tests.
//!
//! Every route is absolute (`!/...`), public, and echoes the validated input
//! as `{"ok": <input>}`.

#![allow(dead_code)]

use std::sync::Arc;

use axum::extract::{Form, Query};
use http::Method;
use navree_db::{Backend, DatabaseModule, Db, Executor, Statement, Uow};
use navree_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver,
};
use navree_testing::TestApp;
use navree_validation::Validate;
use navree_validation::rules::{self, confirmed};
use navree_validation::{AsyncRule, AsyncRules, UowCheck, Valid, ValidAsync};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::databases::Scratch;

/// The `min`/`max`/`email` family.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CreateUser {
    #[validate(length(min = 2, max = 50))]
    pub name: String,
    #[validate(email)]
    pub email: String,
    #[validate(range(min = 18))]
    pub age: u32,
    #[validate(length(min = 1))]
    pub tags: Vec<String>,
}

/// Nested structs and lists (`dive`).
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CreateOrder {
    #[validate(nested)]
    pub customer: Customer,
    #[validate(nested, length(min = 1))]
    pub items: Vec<Item>,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Customer {
    #[validate(nested)]
    pub address: Address,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Address {
    #[validate(length(min = 1))]
    pub city: String,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Item {
    #[validate(length(min = 1))]
    pub sku: String,
    #[validate(range(min = 1))]
    pub quantity: u32,
}

/// Field types serde checks before the rules run.
#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
#[validate(skip_unmarked)]
pub struct Typed {
    pub title: String,
    pub count: i64,
    pub ratio: f64,
    pub active: bool,
    pub labels: Vec<String>,
    pub owner_id: Option<uuid::Uuid>,
    pub stage: Stage,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Lead,
    Won,
}

/// Query-string filters.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Search {
    #[validate(length(min = 2))]
    pub q: Option<String>,
    #[validate(range(min = 1, max = 100))]
    pub per_page: Option<u32>,
}

/// A form post.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Subscribe {
    #[validate(email)]
    pub email: String,
    #[validate(skip)]
    pub newsletter: Option<bool>,
}

/// `confirmed`: `password` must equal `password_confirmation`.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Register {
    #[validate(length(min = 8), custom(confirmed(&self.password_confirmation)))]
    pub password: String,
    #[validate(skip)]
    pub password_confirmation: String,
}

/// navree's extra rules.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Schedule {
    #[validate(custom(rules::uuid))]
    pub owner_id: String,
    #[validate(custom(rules::date_ymd))]
    pub due_on: Option<String>,
    #[validate(custom(rules::one_of(&["lead", "won"])))]
    pub stage: String,
}

/// App-defined rules: a keyed one (`even`) and a plain-text one.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Parity {
    #[validate(custom(is_even))]
    pub even: u32,
    #[validate(custom(is_odd))]
    pub odd: u32,
}

fn is_even(value: &u32) -> Result<(), navree_validation::Violation> {
    if value.is_multiple_of(2) {
        Ok(())
    } else {
        Err(navree_validation::rule_error("even", &[("value", value)]))
    }
}

fn is_odd(value: &u32) -> Result<(), &'static str> {
    if !value.is_multiple_of(2) {
        Ok(())
    } else {
        Err(":attribute must be an odd number")
    }
}

/// `unique` on `users.email`, ignoring the `{id}` of the route, if any.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct UserEmail {
    #[validate(email)]
    pub email: String,
}

impl AsyncRules for UserEmail {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![
            AsyncRule::unique("email", "users", "email", self.email.clone(), None)
                .ignore_path_param("id"),
        ]
    }
}

/// `exists` on `companies.id`; `null` skips the rule.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct NewDeal {
    #[validate(length(min = 1))]
    pub title: String,
    #[validate(skip)]
    pub company_id: Option<uuid::Uuid>,
}

impl AsyncRules for NewDeal {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![AsyncRule::exists(
            "company_id",
            "companies",
            "id",
            self.company_id,
        )]
    }
}

/// A [`UowCheck`] rule: `code` may not be `admin` (a reserved handle),
/// unless the route's `{code}` is `admin` (the record being updated).
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Handle {
    #[validate(skip)]
    pub code: Option<String>,
}

/// The check of [`Handle`]: it runs in the unit of work.
pub struct NotReserved(pub String);

impl UowCheck for NotReserved {
    fn passes<'a>(
        &'a self,
        uow: &'a Uow,
        ignore: Option<&'a str>,
    ) -> BoxFuture<'a, Result<bool, BoxError>> {
        Box::pin(async move {
            uow.fetch_all(&Statement::text("SELECT 1")).await?;
            Ok(self.0 != "admin" || ignore == Some("admin"))
        })
    }
}

impl AsyncRules for Handle {
    fn async_rules(&self) -> Vec<AsyncRule> {
        let code = self.code.clone().unwrap_or_default();
        vec![
            AsyncRule::check("code", "unique", NotReserved(code))
                .skipped_when(self.code.is_none())
                .ignore_path_param("code"),
        ]
    }
}

/// A rule with an identifier that is not a plain SQL name.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Sneaky {
    #[validate(length(min = 1))]
    pub name: String,
}

impl AsyncRules for Sneaky {
    fn async_rules(&self) -> Vec<AsyncRule> {
        vec![AsyncRule::exists(
            "name",
            "companies; DROP TABLE companies",
            "name",
            self.name.clone(),
        )]
    }
}

/// The routes of [`DatabaseTestModule`].
pub struct DatabaseController;

impl Injectable for DatabaseController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(DatabaseController) })
    }
}

impl Controller for DatabaseController {
    fn prefix(&self) -> &'static str {
        "!/db"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::POST,
                "/users",
                |ValidAsync(Json(input)): ValidAsync<Json<UserEmail>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::PATCH,
                "/users/{id}",
                |ValidAsync(Json(input)): ValidAsync<Json<UserEmail>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/deals",
                |ValidAsync(Json(input)): ValidAsync<Json<NewDeal>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "/deals",
                |ValidAsync(Query(input)): ValidAsync<Query<NewDeal>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/deals-form",
                |ValidAsync(Form(input)): ValidAsync<Form<NewDeal>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/handles",
                |ValidAsync(Json(input)): ValidAsync<Json<Handle>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::PATCH,
                "/handles/{code}",
                |ValidAsync(Json(input)): ValidAsync<Json<Handle>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/sneaky",
                |ValidAsync(Json(input)): ValidAsync<Json<Sneaky>>| async move { ok(&input) },
            )
            .public(),
        ]
    }
}

/// Registers [`DatabaseController`].
pub struct DatabaseTestModule;

impl Module for DatabaseTestModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<DatabaseController>();
    }
}

/// The type of an `id` column that holds the UUIDs the ORM writes: a 16-byte
/// blob on SQLite and MySQL, `uuid` on PostgreSQL.
fn id_type(backend: Backend) -> &'static str {
    match backend {
        Backend::Postgres => "uuid",
        Backend::MySql => "BINARY(16)",
        Backend::Sqlite => "BLOB",
    }
}

/// Creates `users(id, email)` and `companies(id, name)`.
pub async fn create_tables(db: &Db) {
    let id = id_type(Executor::backend(db));
    for sql in [
        format!("CREATE TABLE users (id {id} PRIMARY KEY NOT NULL, email TEXT NOT NULL)"),
        format!("CREATE TABLE companies (id {id} PRIMARY KEY NOT NULL, name TEXT NOT NULL)"),
    ] {
        db.execute_script(&sql).await.expect(&sql);
    }
}

/// An app on the scratch database (its units of work on navree-db's own
/// transaction) holding `users(id, email)` and `companies(id, name)`.
pub async fn database_app(scratch: &Scratch) -> TestApp {
    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("database.url", scratch.url.as_str())
        .config("database.migrate_on_boot", true)
        .module(DatabaseModule)
        .module(DatabaseTestModule)
        .build()
        .await;
    create_tables(&*app.get::<Db>().await).await;
    app
}

/// Inserts a row `(id, value)` into `table` (`users` or `companies`).
pub async fn insert(app: &TestApp, table: &str, id: uuid::Uuid, value: &str) {
    let db = app.get::<Db>().await;
    let column = if table == "users" { "email" } else { "name" };
    db.execute(&Statement::portable(
        format!("INSERT INTO {table} (id, {column}) VALUES (?, ?)"),
        [id.into(), value.into()],
    ))
    .await
    .expect("insert");
}

/// Records the type of the deserializer that read it (and skips its value).
#[derive(Debug, Serialize)]
pub struct Probe(pub String);

impl<'de> Deserialize<'de> for Probe {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = std::any::type_name::<D>().to_owned();
        serde::de::IgnoredAny::deserialize(deserializer)?;
        Ok(Probe(name))
    }
}

/// A body with a [`Probe`].
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Probed {
    #[validate(skip)]
    pub probe: Probe,
    #[validate(length(min = 1))]
    pub name: String,
}

/// The routes of [`ValidationTestModule`].
pub struct TestController;

impl Injectable for TestController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(TestController) })
    }
}

fn ok<T: Serialize>(input: &T) -> Json<Value> {
    Json(json!({ "ok": input }))
}

impl Controller for TestController {
    fn prefix(&self) -> &'static str {
        "!/v"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::POST,
                "/users",
                |Valid(Json(input)): Valid<Json<CreateUser>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/orders",
                |Valid(Json(input)): Valid<Json<CreateOrder>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "/search",
                |Valid(Query(input)): Valid<Query<Search>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/subscribe",
                |Valid(Form(input)): Valid<Form<Subscribe>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/register",
                |Valid(Json(input)): Valid<Json<Register>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/parity",
                |Valid(Json(input)): Valid<Json<Parity>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/schedule",
                |Valid(Json(input)): Valid<Json<Schedule>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/probe",
                |Valid(Json(input)): Valid<Json<Probed>>| async move { ok(&input) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/typed",
                |Valid(Json(input)): Valid<Json<Typed>>| async move { ok(&input) },
            )
            .public(),
        ]
    }
}

/// Registers [`TestController`].
pub struct ValidationTestModule;

impl Module for ValidationTestModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<TestController>();
    }
}

/// An app with the test routes and no database.
pub async fn app() -> TestApp {
    app_with(ValidationTestModule).await
}

/// An app with `module` and no database.
pub async fn app_with(module: impl Module) -> TestApp {
    TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .module(module)
        .build()
        .await
}

/// The `errors` member of an error body.
pub fn errors(body: &Value) -> &Value {
    &body["errors"]
}
