//! `TestAuthenticator` and `acting_as`.

use std::sync::Arc;

use http::request::Parts;
use keel_config::{ConfigModule, ConfigSources};
use keel_contracts::{AuthVia, Principal};
use keel_http::{AppError, Authenticator, HttpModule};
use keel_kernel::{BoxFuture, Keel, Module, ModuleBuilder};
use keel_testing::{TestApp, TestAuthenticator, principal};
use serde_json::json;
use uuid::Uuid;

use crate::support::ProfileModule;

const ALICE: Uuid = Uuid::from_u128(0x0192_f0c1_0000_7000_8000_0000_0000_000a);
const BOB: Uuid = Uuid::from_u128(0x0192_f0c1_0000_7000_8000_0000_0000_000b);

#[tokio::test]
async fn acting_as_sets_principal() {
    let app = TestApp::builder().module(ProfileModule).build().await;

    app.acting_as(principal(ALICE))
        .get("/api/profile/me")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(ALICE))
        .assert_json_path("via", json!("Test"));

    app.client()
        .get("/api/profile/me")
        .send()
        .await
        .assert_error(401, "Unauthorized");
}

#[tokio::test]
async fn acting_as_passes_the_whole_principal() {
    let app = TestApp::builder().module(ProfileModule).build().await;
    let token_user = Principal {
        user_id: BOB,
        via: AuthVia::Token,
        token_abilities: Some(vec!["deals:read".to_owned()]),
        session_id: None,
    };
    app.acting_as(token_user)
        .get("/api/profile/me")
        .send()
        .await
        .assert_json_path("user_id", json!(BOB))
        .assert_json_path("via", json!("Token"))
        .assert_json_path("token_abilities", json!(["deals:read"]));
}

#[tokio::test]
async fn a_client_can_switch_identity_and_keep_its_cookies() {
    let app = TestApp::builder().module(ProfileModule).build().await;
    let client = app.client();
    client.set_cookie("theme", "dark");

    let alice = client.clone().acting_as(principal(ALICE));
    let bob = client.clone().acting_as(principal(BOB));
    alice
        .get("/api/profile/me")
        .send()
        .await
        .assert_json_path("user_id", json!(ALICE));
    bob.get("/api/profile/me")
        .send()
        .await
        .assert_json_path("user_id", json!(BOB));
    assert_eq!(bob.cookie("theme").as_deref(), Some("dark"));

    bob.as_guest()
        .get("/api/profile/me")
        .send()
        .await
        .assert_error(401, None);
}

/// Authenticates requests carrying `x-user-id: <uuid>`.
struct HeaderAuthenticator;

impl Authenticator for HeaderAuthenticator {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        Box::pin(async move {
            Ok(parts
                .headers
                .get("x-user-id")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| Uuid::parse_str(v).ok())
                .map(|user_id| Principal {
                    user_id,
                    via: AuthVia::Session,
                    token_abilities: None,
                    session_id: None,
                }))
        })
    }
}

/// A module with its own authenticator, as keel-auth has.
struct HeaderAuthModule;

impl Module for HeaderAuthModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value_arc::<dyn Authenticator>(Arc::new(HeaderAuthenticator))
            .exports::<dyn Authenticator>();
    }
}

#[tokio::test]
async fn an_app_with_its_own_authenticator_keeps_it() {
    let app = TestApp::builder()
        .module(HeaderAuthModule)
        .module(ProfileModule)
        .build()
        .await;
    app.client()
        .get("/api/profile/me")
        .header("x-user-id", ALICE.to_string())
        .send()
        .await
        .assert_status(200)
        .assert_json_path("via", json!("Session"));
}

#[tokio::test]
#[should_panic(expected = "fake_auth")]
async fn acting_as_without_the_test_authenticator_explains_how_to_get_it() {
    let app = TestApp::builder()
        .module(HeaderAuthModule)
        .module(ProfileModule)
        .build()
        .await;
    let _ = app.acting_as(principal(ALICE));
}

#[tokio::test]
async fn fake_auth_replaces_the_apps_authenticator() {
    let app = TestApp::builder()
        .module(HeaderAuthModule)
        .module(ProfileModule)
        .fake_auth()
        .build()
        .await;
    app.acting_as(principal(ALICE))
        .get("/api/profile/me")
        .send()
        .await
        .assert_json_path("user_id", json!(ALICE));
    app.client()
        .get("/api/profile/me")
        .header("x-user-id", ALICE.to_string())
        .send()
        .await
        .assert_error(401, None);
}

#[tokio::test]
async fn from_builder_installs_the_test_authenticator_when_the_app_has_none() {
    let builder = Keel::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(HttpModule)
        .module(ProfileModule);
    let app = TestApp::from_builder(builder).await;
    app.acting_as(principal(ALICE))
        .get("/api/profile/me")
        .send()
        .await
        .assert_json_path("user_id", json!(ALICE));
}

#[tokio::test]
async fn install_puts_the_test_authenticator_into_any_app() {
    let builder = Keel::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(HttpModule)
        .module(HeaderAuthModule)
        .module(ProfileModule);
    let app = TestApp::from_builder(TestAuthenticator::install(builder)).await;
    app.acting_as(principal(BOB))
        .get("/api/profile/me")
        .send()
        .await
        .assert_json_path("user_id", json!(BOB));
}

#[test]
fn principal_is_a_test_user_without_token_or_session() {
    assert_eq!(
        principal(ALICE),
        Principal {
            user_id: ALICE,
            via: AuthVia::Test,
            token_abilities: None,
            session_id: None,
        }
    );
}
