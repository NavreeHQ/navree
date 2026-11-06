//! Two-factor policy: `auth.two_factor.required_for` and `TwoFactorPolicy`
//! limit the sessions of users without two-factor authentication to the
//! setup routes.

use std::sync::Arc;

use navree_auth::{
    AuthService, JwtService, ModuleBuilderAuthExt, TokenType, TwoFactorPolicy, User,
};
use navree_contracts::{BoxError, BoxFuture};
use navree_kernel::{Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::TestApp;
use serde_json::json;

use crate::support::{
    self, PASSWORD, claims_of, enable_two_factor, grant_role, login, refresh, signed_in, tokens,
    totp_code,
};

const SETUP_REQUIRED: &str = "Two-factor authentication required";

/// Requires two-factor authentication of `@staff.example.com` addresses.
struct StaffPolicy;

impl Injectable for StaffPolicy {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(StaffPolicy) })
    }
}

impl TwoFactorPolicy for StaffPolicy {
    fn requires_two_factor<'a>(&'a self, user: &'a User) -> BoxFuture<'a, Result<bool, BoxError>> {
        Box::pin(async move { Ok(user.email.ends_with("@staff.example.com")) })
    }
}

struct StaffPolicyModule;

impl Module for StaffPolicyModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.two_factor_policy::<StaffPolicy>();
    }
}

#[tokio::test]
async fn without_rules_nobody_is_limited() {
    let app = support::app().await;
    let (_, access, _) = signed_in(&app, "anu@example.com").await;
    let jwt = app.get::<JwtService>().await;
    assert_eq!(claims_of(&jwt, &access).typ, TokenType::Access);
    let service = app.get::<AuthService>().await;
    let user = service
        .find_by_email("anu@example.com")
        .await
        .unwrap()
        .unwrap();
    assert!(!service.two_factor_required(&user).await.unwrap());
}

#[tokio::test]
async fn a_policy_limits_sessions_to_the_setup_until_two_factor_is_on() {
    let clock = support::clock();
    let app = support::clocked(&clock)
        .module(StaffPolicyModule)
        .build()
        .await;
    let user = support::create_user(&app, "anu@staff.example.com").await;
    let client = app.client();

    let response = login(&client, "anu@staff.example.com", PASSWORD)
        .await
        .assert_status(200)
        .assert_json_path("two_factor_setup_required", json!(true));
    let (access, refresh_token) = tokens(&response);
    let jwt = app.get::<JwtService>().await;
    assert_eq!(claims_of(&jwt, &access).typ, TokenType::TwoFactorSetup);

    // Everything but the setup is refused.
    let refused = client
        .get("/api/probe/whoami")
        .bearer(&access)
        .send()
        .await
        .assert_error(403, SETUP_REQUIRED);
    assert!(refused.json_value().get("errors").is_none());
    client
        .get("/api/auth/sessions")
        .bearer(&access)
        .send()
        .await
        .assert_error(403, SETUP_REQUIRED);
    client
        .get("/api/auth/me")
        .bearer(&access)
        .send()
        .await
        .assert_status(200);

    // Set it up with the limited token.
    let setup = client
        .post("/api/auth/2fa/enable")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .json_value();
    let secret = setup["secret"].as_str().unwrap().to_owned();
    client
        .post("/api/auth/2fa/confirm")
        .bearer(&access)
        .json(&json!({ "code": totp_code(&secret, &clock) }))
        .send()
        .await
        .assert_status(200);

    // A refresh now gives a full token.
    let refreshed = refresh(&client, &refresh_token).await.assert_status(200);
    assert!(
        refreshed
            .json_value()
            .get("two_factor_setup_required")
            .is_none()
    );
    let (full, _) = tokens(&refreshed);
    assert_eq!(claims_of(&jwt, &full).typ, TokenType::Access);
    client
        .get("/api/probe/whoami")
        .bearer(&full)
        .send()
        .await
        .assert_status(200);

    // Other users are not affected.
    let (_, other, _) = signed_in(&app, "gauri@example.com").await;
    assert_eq!(claims_of(&jwt, &other).typ, TokenType::Access);
    let _ = user;
}

#[tokio::test]
async fn required_roles_need_two_factor_and_a_new_requirement_applies_at_refresh() {
    let clock = support::clock();
    let app: TestApp = support::with_access(&clock)
        .config("auth.two_factor.required_for", json!(["admin"]))
        .build()
        .await;
    let (user, access, refresh_token) = signed_in(&app, "anu@example.com").await;
    let jwt = app.get::<JwtService>().await;
    assert_eq!(claims_of(&jwt, &access).typ, TokenType::Access);

    // Made an admin: the next refresh limits the session.
    grant_role(&app, user.id, "admin", &[]).await;
    let refreshed = refresh(&app.client(), &refresh_token)
        .await
        .assert_status(200)
        .assert_json_path("two_factor_setup_required", json!(true));
    let (limited, refresh_token) = tokens(&refreshed);
    assert_eq!(claims_of(&jwt, &limited).typ, TokenType::TwoFactorSetup);
    let service = app.get::<AuthService>().await;
    assert!(service.two_factor_required(&user).await.unwrap());

    // With two-factor authentication on, sign-ins go through the challenge.
    enable_two_factor(&app, &clock, &user).await;
    let (full, _) = tokens(
        &refresh(&app.client(), &refresh_token)
            .await
            .assert_status(200),
    );
    assert_eq!(claims_of(&jwt, &full).typ, TokenType::Access);
    login(&app.client(), "anu@example.com", PASSWORD)
        .await
        .assert_status(200)
        .assert_json_path("two_factor_required", json!(true));
}

#[tokio::test]
async fn setup_tokens_still_sign_out() {
    let app = support::builder().module(StaffPolicyModule).build().await;
    support::create_user(&app, "anu@staff.example.com").await;
    let (access, refresh_token) = tokens(
        &login(&app.client(), "anu@staff.example.com", PASSWORD)
            .await
            .assert_status(200),
    );
    app.client()
        .post("/api/auth/logout")
        .bearer(&access)
        .json(&json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .assert_status(204);
    refresh(&app.client(), &refresh_token)
        .await
        .assert_error(401, "Invalid refresh token");
}

#[tokio::test]
async fn registering_policies_through_the_builder_resolves_them() {
    let app = support::builder().module(StaffPolicyModule).build().await;
    let policies = app.ctx().get_all::<dyn TwoFactorPolicy>().await.unwrap();
    assert_eq!(policies.len(), 1);
    let _: Vec<Arc<dyn TwoFactorPolicy>> = policies;
}
