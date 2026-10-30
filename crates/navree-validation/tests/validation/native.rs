//! The extractors implement navree-http's traits: handlers taking them run
//! on Navree's router, read its path parameters and honour the route's body
//! limit.

use std::sync::Arc;

use http::Method;
use navree_http::handler::Endpoint;
use navree_http::{Controller, Form, Json, ModuleBuilderHttpExt, Query, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_validation::{Valid, ValidAsync};
use serde_json::json;

use crate::support::{CreateUser, NewDeal, Search, Subscribe, UserEmail, app_with, errors, ok};

#[test]
fn handlers_on_the_validation_extractors_are_native() {
    async fn json(_: Valid<Json<CreateUser>>) {}
    async fn query(_: Valid<Query<Search>>) {}
    async fn form(_: Valid<Form<Subscribe>>) {}
    async fn async_json(_: ValidAsync<Json<UserEmail>>) {}
    async fn async_query(_: ValidAsync<Query<NewDeal>>) {}
    async fn async_form(_: ValidAsync<Form<NewDeal>>) {}
    async fn head_then_body(_: Valid<Query<Search>>, _: Valid<Json<CreateUser>>) {}
    assert!(Endpoint::new(json).is_native());
    assert!(Endpoint::new(query).is_native());
    assert!(Endpoint::new(form).is_native());
    assert!(Endpoint::new(async_json).is_native());
    assert!(Endpoint::new(async_query).is_native());
    assert!(Endpoint::new(async_form).is_native());
    assert!(Endpoint::new(head_then_body).is_native());
}

struct LimitedController;

impl Injectable for LimitedController {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(LimitedController) })
    }
}

impl Controller for LimitedController {
    fn prefix(&self) -> &'static str {
        "!/limited"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::POST,
                "/users",
                |Valid(Json(input)): Valid<Json<CreateUser>>| async move { ok(&input) },
            )
            .public()
            .body_limit(80),
            RouteDef::on(
                Method::POST,
                "/subscribe",
                |Valid(Form(input)): Valid<Form<Subscribe>>| async move { ok(&input) },
            )
            .public()
            .body_limit(40),
        ]
    }
}

struct LimitedModule;

impl Module for LimitedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<LimitedController>();
    }
}

#[tokio::test]
async fn a_route_limits_the_bodies_valid_reads() {
    let app = app_with(LimitedModule).await;

    app.client()
        .post("/limited/users")
        .json(&json!({ "name": "Ada", "email": "ada@example.com", "age": 36, "tags": ["x"] }))
        .send()
        .await
        .assert_status(200);
    app.client()
        .post("/limited/users")
        .json(&json!({ "name": "A".repeat(100), "email": "ada@example.com", "age": 36, "tags": ["x"] }))
        .send()
        .await
        .assert_error(413, None);
    app.client()
        .post("/limited/subscribe")
        .form(&[("email", "ada@example.com")])
        .send()
        .await
        .assert_status(200);
    app.client()
        .post("/limited/subscribe")
        .form(&[("email", "a".repeat(100))])
        .send()
        .await
        .assert_error(413, None);
}

#[tokio::test]
async fn failed_rules_keep_their_field_errors_under_a_route_limit() {
    let app = app_with(LimitedModule).await;

    let response = app
        .client()
        .post("/limited/users")
        .json(&json!({ "name": "A", "email": "nope", "age": 36, "tags": ["x"] }))
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
}
