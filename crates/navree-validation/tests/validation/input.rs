//! `ValidInput`: extractors from other crates (navree-micro's `Payload<T>`)
//! wrapped by `Valid` and `ValidAsync`, with the same field errors as
//! `Valid<Json<T>>`.

use std::sync::Arc;

use http::Method;
use navree_http::extract::Request;
use navree_http::{AppError, Controller, Json, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_validation::Validate;
use navree_validation::{Valid, ValidAsync, ValidInput};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::databases::Scratch;
use crate::on_every_server;
use crate::support::{CreateUser, UserEmail, app_with, create_tables, errors, insert};

/// Input read from a body of `key=value` lines; `!` is unreadable input and
/// anything else that is not a line pair is malformed.
struct Lines<T>(T);

impl<T> ValidInput for Lines<T>
where
    T: DeserializeOwned + Validate + Send + 'static,
{
    type Target = T;

    async fn read(req: Request) -> Result<Option<Value>, AppError> {
        let bytes = navree_http::body::to_bytes(req.into_body(), 1 << 16)
            .await
            .map_err(AppError::internal)?;
        let text = String::from_utf8_lossy(&bytes);
        if text == "!" {
            return Err(AppError::bad_request("unreadable input"));
        }
        let mut object = serde_json::Map::new();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                return Ok(None);
            };
            let value = serde_json::from_str(value).unwrap_or_else(|_| json!(value));
            object.insert(key.to_owned(), value);
        }
        Ok(Some(Value::Object(object)))
    }

    fn wrap(value: T) -> Self {
        Lines(value)
    }
}

struct LinesController;

impl Injectable for LinesController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(LinesController) })
    }
}

impl Controller for LinesController {
    fn prefix(&self) -> &'static str {
        "!/lines"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::POST,
                "/users",
                |Valid(Lines(input)): Valid<Lines<CreateUser>>| async move {
                    Json(json!({ "ok": input }))
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "/emails",
                |ValidAsync(Lines(input)): ValidAsync<Lines<UserEmail>>| async move {
                    Json(json!({ "ok": input }))
                },
            )
            .public(),
        ]
    }
}

struct LinesModule;

impl Module for LinesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<LinesController>();
    }
}

#[tokio::test]
async fn valid_wraps_a_valid_input_with_the_json_field_errors() {
    let app = app_with(LinesModule).await;

    app.client()
        .post("/lines/users")
        .body("name=Ada\nemail=ada@example.com\nage=36\ntags=[\"x\"]")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("ok.name", json!("Ada"));

    let response = app
        .client()
        .post("/lines/users")
        .body("name=A\nemail=nope\nage=36\ntags=[\"x\"]")
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({
            "email": ["email must be a valid email address"],
            "name": ["name must be longer than or equal to 2 characters"],
        })
    );

    // serde errors at their path, as for JSON bodies.
    let response = app
        .client()
        .post("/lines/users")
        .body("name=Ada\nemail=ada@example.com\ntags=[\"x\"]")
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "age": ["age should not be empty"] })
    );

    // Malformed input is the malformed-JSON field error; unreadable input
    // is the input's own error.
    let response = app
        .client()
        .post("/lines/users")
        .body("no pairs here")
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "body": ["body must be a valid JSON string"] })
    );
    app.client()
        .post("/lines/users")
        .body("!")
        .send()
        .await
        .assert_error(400, "unreadable input");
}

async fn valid_async_runs_the_database_rules_after_the_validation_rules(scratch: Scratch) {
    let app = navree_testing::TestApp::builder()
        .config("http.per_ip_rate_limit", Value::Null)
        .config("database.url", scratch.url.as_str())
        .config("database.migrate_on_boot", true)
        .module(navree_db::DatabaseModule)
        .module(LinesModule)
        .build()
        .await;
    let db = app.get::<navree_db::Db>().await;
    create_tables(&db).await;
    insert(&app, "users", Uuid::now_v7(), "taken@example.com").await;

    app.client()
        .post("/lines/emails")
        .body("email=free@example.com")
        .send()
        .await
        .assert_status(200);
    let response = app
        .client()
        .post("/lines/emails")
        .body("email=taken@example.com")
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "email": ["email has already been taken"] })
    );
    let response = app
        .client()
        .post("/lines/emails")
        .body("email=nope")
        .send()
        .await
        .assert_error(400, None);
    assert_eq!(
        errors(&response.json_value()),
        &json!({ "email": ["email must be a valid email address"] })
    );
}

on_every_server!(
    "navree_validation";
    valid_async_runs_the_database_rules_after_the_validation_rules,
);
