//! Pluggable strategies: a custom API-key strategy registered by
//! another module as a `dyn Strategy` multi-binding.

use std::sync::Arc;

use http::request::Parts;
use navree_auth::{ModuleBuilderAuthExt, Strategy};
use navree_contracts::{AuthVia, BoxFuture, Principal, Secret};
use navree_http::AppError;
use navree_kernel::{Factory, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_observe::{ObserveModule, ObserveOptions};
use serde_json::json;
use uuid::Uuid;

use crate::support::{self, signed_in};

/// The user every valid API key belongs to.
const KEY_OWNER: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0001);

/// `X-Api-Key: <key>`: one known key, restricted to `deals.read`.
struct ApiKeyStrategy;

impl Strategy for ApiKeyStrategy {
    fn name(&self) -> &'static str {
        "api-key"
    }

    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        Box::pin(async move {
            let Some(key) = parts.headers.get("x-api-key") else {
                return Ok(None);
            };
            if key != "secret-key" {
                return Err(AppError::unauthenticated().with_detail("Invalid API key"));
            }
            Ok(Some(Principal {
                user_id: KEY_OWNER,
                via: AuthVia::Token,
                token_abilities: Some(vec!["deals.read".to_owned()]),
                session_id: None,
            }))
        })
    }
}

/// Registers the strategy with a plain multi-binding.
struct ApiKeyModule;

impl Module for ApiKeyModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.multi(Factory::<dyn Strategy>::new(|_| async {
            Ok(Arc::new(ApiKeyStrategy) as Arc<dyn Strategy>)
        }));
    }
}

#[tokio::test]
async fn custom_strategy_via_multi_binding() {
    let app = support::builder().module(ApiKeyModule).build().await;
    let client = app.client();
    client
        .get("/api/probe/whoami")
        .header("x-api-key", "secret-key")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(KEY_OWNER))
        .assert_json_path("token_abilities", json!(["deals.read"]));
    client
        .get("/api/probe/whoami")
        .header("x-api-key", "stolen-key")
        .send()
        .await
        .assert_error(401, "Invalid API key");
    // The built-in JWT strategy still runs first.
    let (user, access, _) = signed_in(&app, "ada@example.com").await;
    client
        .get("/api/probe/whoami")
        .bearer(&access)
        .header("x-api-key", "secret-key")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(user.id));
    // No credentials at all: a guest.
    client
        .get("/api/probe/whoami")
        .send()
        .await
        .assert_error(401, "Unauthorized");
}

/// An injectable strategy registered with `ModuleBuilderAuthExt::strategy`.
struct HeaderStrategy;

impl Injectable for HeaderStrategy {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(HeaderStrategy) })
    }
}

impl Strategy for HeaderStrategy {
    fn name(&self) -> &'static str {
        "header"
    }

    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        let user = parts
            .headers
            .get("x-test-user")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<Uuid>().ok());
        Box::pin(async move {
            Ok(user.map(|user_id| Principal {
                user_id,
                via: AuthVia::Token,
                token_abilities: None,
                session_id: None,
            }))
        })
    }
}

struct HeaderModule;

impl Module for HeaderModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.strategy::<HeaderStrategy>();
    }
}

#[tokio::test]
async fn opaque_bearer_tokens_are_left_to_other_strategies_and_guards() {
    // navree-observe guards `/metrics` with `Authorization: Bearer <metrics
    // token>`: the JWT strategy must not reject a bearer token that is not
    // a JWT before the route's guard sees it.
    let app = support::builder()
        .module(ObserveModule::for_root(ObserveOptions {
            metrics_token: Some(Secret::new("prometheus-scrape-token".to_owned())),
            ..ObserveOptions::default()
        }))
        .build()
        .await;
    let client = app.client();
    client
        .get("/metrics")
        .bearer("prometheus-scrape-token")
        .send()
        .await
        .assert_status(200);
    client
        .get("/metrics")
        .bearer("wrong-token")
        .send()
        .await
        .assert_error(401, None);
    // An opaque bearer token authenticates nobody: protected routes stay
    // 401, public routes see a guest.
    client
        .get("/api/auth/me")
        .bearer("prometheus-scrape-token")
        .send()
        .await
        .assert_error(401, "Unauthorized");
    client
        .get("/api/probe/ping")
        .bearer("opaque-api-token")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("guest", json!(true));
    // A JWT that does not verify is still a 401, even on a public route.
    client
        .get("/api/probe/ping")
        .bearer("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.c2ln")
        .send()
        .await
        .assert_error(401, "Invalid token");
}

#[tokio::test]
async fn strategies_register_with_the_builder_extension() {
    let app = support::builder().module(HeaderModule).build().await;
    let user = Uuid::now_v7();
    app.client()
        .get("/api/probe/whoami")
        .header("x-test-user", user.to_string())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(user));
}
