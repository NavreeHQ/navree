//! The DTOs and the hand-written controller shared by the tests.
//!
//! Every route is absolute (`!/...`), public, and echoes the validated input
//! as `{"ok": <input>}`.

#![allow(dead_code)]

use std::sync::Arc;

use axum::extract::{Form, Query};
use garde::Validate;
use http::Method;
use navree_db::{DatabaseModule, Db};
use navree_http::{Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::TestApp;
use navree_validation::rules::{self, confirmed};
use navree_validation::{AsyncRule, AsyncRules, Valid, ValidAsync};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Laravel's `min`/`max`/`email` family.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CreateUser {
    #[garde(length(min = 2, max = 50))]
    pub name: String,
    #[garde(email)]
    pub email: String,
    #[garde(range(min = 18))]
    pub age: u32,
    #[garde(length(min = 1))]
    pub tags: Vec<String>,
}

/// Nested structs and lists (`dive`).
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CreateOrder {
    #[garde(dive)]
    pub customer: Customer,
    #[garde(dive, length(min = 1))]
    pub items: Vec<Item>,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Customer {
    #[garde(dive)]
    pub address: Address,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Address {
    #[garde(length(min = 1))]
    pub city: String,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Item {
    #[garde(length(min = 1))]
    pub sku: String,
    #[garde(range(min = 1))]
    pub quantity: u32,
}

/// Field types serde checks before garde runs.
#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
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
    #[garde(length(min = 2))]
    pub q: Option<String>,
    #[garde(range(min = 1, max = 100))]
    pub per_page: Option<u32>,
}

/// A form post.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Subscribe {
    #[garde(email)]
    pub email: String,
    #[garde(skip)]
    pub newsletter: Option<bool>,
}

/// `confirmed`: `password` must equal `password_confirmation`.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Register {
    #[garde(length(min = 8), custom(confirmed(&self.password_confirmation)))]
    pub password: String,
    #[garde(skip)]
    pub password_confirmation: String,
}

/// navree's extra rules.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Schedule {
    #[garde(custom(rules::uuid))]
    pub owner_id: String,
    #[garde(custom(rules::date_ymd))]
    pub due_on: Option<String>,
    #[garde(custom(rules::one_of(&["lead", "won"])))]
    pub stage: String,
}

/// App-defined rules: a keyed one (`even`) and a plain-text one.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Parity {
    #[garde(custom(is_even))]
    pub even: u32,
    #[garde(custom(is_odd))]
    pub odd: u32,
}

fn is_even(value: &u32, _: &()) -> garde::Result {
    if value.is_multiple_of(2) {
        Ok(())
    } else {
        Err(navree_validation::rule_error("even", &[("value", value)]))
    }
}

fn is_odd(value: &u32, _: &()) -> garde::Result {
    if !value.is_multiple_of(2) {
        Ok(())
    } else {
        Err(garde::Error::new(":attribute must be an odd number"))
    }
}

/// `unique` on `users.email`, ignoring the `{id}` of the route, if any.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct UserEmail {
    #[garde(email)]
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
    #[garde(length(min = 1))]
    pub title: String,
    #[garde(skip)]
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

/// A rule with an identifier that is not a plain SQL name.
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Sneaky {
    #[garde(length(min = 1))]
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

/// An app with a fresh SQLite database holding `users(id, email)` and
/// `companies(id, name)`.
pub async fn database_app() -> TestApp {
    let app = TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .temp_database()
        .module(DatabaseModule)
        .module(DatabaseTestModule)
        .build()
        .await;
    let db = app.get::<Db>().await;
    for sql in [
        "CREATE TABLE users (id BLOB PRIMARY KEY NOT NULL, email TEXT NOT NULL)",
        "CREATE TABLE companies (id BLOB PRIMARY KEY NOT NULL, name TEXT NOT NULL)",
    ] {
        db.conn().execute_unprepared(sql).await.expect(sql);
    }
    app
}

/// Inserts a row `(id, value)` into `table` (`users` or `companies`).
pub async fn insert(app: &TestApp, table: &str, id: uuid::Uuid, value: &str) {
    let db = app.get::<Db>().await;
    let column = if table == "users" { "email" } else { "name" };
    db.conn()
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("INSERT INTO {table} (id, {column}) VALUES (?, ?)"),
            [id.into(), value.into()],
        ))
        .await
        .expect("insert");
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
