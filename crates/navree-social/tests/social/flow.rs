//! The redirect and the callback with the fake provider: the sealed state
//! (once, expiry, tampering, provider binding), PKCE, `return_to`, the
//! intents, provider errors, the throttle and the module's configuration.

use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use navree_contracts::Clock;
use navree_social::SocialUser;
use navree_testing::{FixedClock, TestApp};
use serde_json::json;

use crate::support::{
    REDIRECT_BASE, access_token, builder, callback, create_user, fake_app, query_param, redirect,
    sign_in, state,
};

fn ada() -> SocialUser {
    SocialUser::new("github", "583231")
        .name("Ada Lovelace")
        .email(Some("ada@example.com"), true)
}

#[tokio::test]
async fn the_redirect_is_the_providers_url_with_a_sealed_state_and_pkce() {
    let (app, _fake) = fake_app(|b| b).await;
    let body = redirect(&app, "github", "", None)
        .await
        .assert_status(200)
        .json_value();
    let url = body["url"].as_str().unwrap();
    let state = body["state"].as_str().unwrap();
    assert!(state.starts_with("nvs1."), "{state}");
    assert!(
        url.starts_with(
            "https://github.com/login/oauth/authorize?response_type=code&client_id=Iv1.8a61f9b3a7aba766\
             &redirect_uri=https%3A%2F%2Fapp.acme.test%2Fauth%2Fcallback%2Fgithub\
             &scope=read%3Auser+user%3Aemail&state=nvs1."
        ),
        "{url}"
    );
    assert_eq!(query_param(url, "state").as_deref(), Some(state));
    assert_eq!(
        query_param(url, "code_challenge_method").as_deref(),
        Some("S256")
    );
    let challenge = query_param(url, "code_challenge").unwrap();
    assert_eq!(challenge.len(), 43, "base64url of a SHA-256 digest");
    assert_eq!(query_param(url, "nonce"), None, "GitHub has no ID token");
    // Each flow has its own state and verifier.
    let again = redirect(&app, "github", "", None).await.json_value();
    assert_ne!(again["state"], body["state"]);
    assert_ne!(
        query_param(again["url"].as_str().unwrap(), "code_challenge").unwrap(),
        challenge
    );
}

#[tokio::test]
async fn a_new_github_user_gets_an_account_and_tokens_and_the_state_works_once() {
    let (app, fake) = fake_app(|b| b).await;
    fake.on_code("github", "code-ada", ada());
    let state = state(&app, "github").await;

    let response = callback(&app, "github", "code-ada", &state)
        .await
        .assert_status(200)
        .assert_json_path("user.email", json!("ada@example.com"))
        .assert_json_path("user.name", json!("Ada Lovelace"))
        .assert_json_path("token_type", json!("Bearer"))
        .assert_json_path("expires_in", json!(900));
    let body = response.json_value();
    assert!(body["access_token"].as_str().unwrap().split('.').count() == 3);
    assert!(body["refresh_token"].is_string());
    assert!(body["user"]["email_verified_at"].is_string(), "{body}");
    assert!(body.get("return_to").is_none());

    // The tokens are normal Navree tokens.
    let access = body["access_token"].as_str().unwrap();
    app.client()
        .get("/api/auth/me")
        .bearer(access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("email", json!("ada@example.com"));

    callback(&app, "github", "code-ada", &state)
        .await
        .assert_error(400, "Invalid or expired state")
        .assert_error_code(400, "invalid_state");
    assert_eq!(
        fake.exchanged(),
        [("github".to_owned(), "code-ada".to_owned())],
        "a used state never reaches the provider"
    );
}

#[tokio::test]
async fn forged_expired_and_foreign_states_are_refused() {
    let clock = Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 10, 26, 8, 0, 0).unwrap(),
    ));
    let (app, fake) = fake_app(|b| {
        b.config("social.providers.google.client_id", "google-client")
            .override_value::<dyn Clock>(clock.clone())
    })
    .await;
    fake.on_code("github", "code", ada());

    // Tampered.
    let mut tampered = state(&app, "github").await;
    let last = tampered.pop().unwrap();
    tampered.push(if last == 'A' { 'B' } else { 'A' });
    callback(&app, "github", "code", &tampered)
        .await
        .assert_error(400, "Invalid or expired state");
    callback(&app, "github", "code", "nvs1.made-up")
        .await
        .assert_error(400, "Invalid or expired state");

    // Issued for another provider.
    let google = state(&app, "google").await;
    callback(&app, "github", "code", &google)
        .await
        .assert_error(400, "Invalid or expired state");

    // Expired: 10 minutes by default.
    let old = state(&app, "github").await;
    clock.advance(Duration::from_secs(601));
    callback(&app, "github", "code", &old)
        .await
        .assert_error(400, "Invalid or expired state");

    // Sealed by another app (another key).
    let (other, _) = fake_app(|b| b).await;
    let foreign = state(&other, "github").await;
    callback(&app, "github", "code", &foreign)
        .await
        .assert_error(400, "Invalid or expired state");
    assert!(fake.exchanged().is_empty());
}

#[tokio::test]
async fn the_state_lifetime_is_configurable() {
    let clock = Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 10, 26, 8, 0, 0).unwrap(),
    ));
    let (app, fake) = fake_app(|b| {
        b.config("social.state_ttl_secs", 60)
            .override_value::<dyn Clock>(clock.clone())
    })
    .await;
    fake.on_code("github", "code", ada());
    let state = state(&app, "github").await;
    clock.advance(Duration::from_secs(61));
    callback(&app, "github", "code", &state)
        .await
        .assert_error(400, "Invalid or expired state");
}

#[tokio::test]
async fn return_to_comes_back_and_must_be_a_relative_path() {
    let (app, fake) = fake_app(|b| b).await;
    fake.on_code("github", "code-ada", ada());
    let state = redirect(&app, "github", "?return_to=/deals", None)
        .await
        .assert_status(200)
        .json_value()["state"]
        .as_str()
        .unwrap()
        .to_owned();
    callback(&app, "github", "code-ada", &state)
        .await
        .assert_status(200)
        .assert_json_path("return_to", json!("/deals"));

    for bad in ["https://evil.test/", "//evil.test", "/%5Cevil.test"] {
        redirect(&app, "github", &format!("?return_to={bad}"), None)
            .await
            .assert_error(400, "return_to must be a relative path");
    }
}

#[tokio::test]
async fn unknown_providers_are_404() {
    let (app, _fake) = fake_app(|b| b).await;
    redirect(&app, "myspace", "", None)
        .await
        .assert_error(404, "Unknown sign-in provider `myspace`");
    callback(&app, "myspace", "c", "nvs1.x")
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn provider_errors_are_400_and_unknown_codes_are_refused() {
    let (app, fake) = fake_app(|b| b).await;
    fake.on_code_error("github", "code-denied", "access_denied");
    sign_in(&app, "github", "code-denied")
        .await
        .assert_error(400, "GitHub refused the sign-in: access_denied");
    sign_in(&app, "github", "code-unknown")
        .await
        .assert_error(400, "GitHub refused the sign-in: invalid_grant");
    assert_eq!(fake.exchanged().len(), 2);
}

#[tokio::test]
async fn link_flows_need_a_signed_in_user_and_states_keep_their_intent() {
    let (app, fake) = fake_app(|b| b).await;
    fake.on_code("github", "code-ada", ada());
    redirect(&app, "github", "?intent=link", None)
        .await
        .assert_error(401, "Sign in to connect a provider");
    redirect(&app, "github", "?intent=nonsense", None)
        .await
        .assert_status(400);

    let grace = create_user(&app, "Grace Hopper", "grace@example.com").await;
    let token = access_token(&app, &grace).await;
    let link_state = redirect(&app, "github", "?intent=link", Some(&token))
        .await
        .assert_status(200)
        .json_value()["state"]
        .as_str()
        .unwrap()
        .to_owned();
    // A link state does not sign in.
    callback(&app, "github", "code-ada", &link_state)
        .await
        .assert_error(400, "Invalid or expired state");

    // A sign-in state does not link.
    let sign_in_state = state(&app, "github").await;
    app.client()
        .post("/api/auth/social/github/link")
        .bearer(&token)
        .json(&json!({ "code": "code-ada", "state": sign_in_state }))
        .send()
        .await
        .assert_error(400, "Invalid or expired state");
}

#[tokio::test]
async fn the_callback_is_counted_by_the_login_throttle() {
    let (app, fake) = fake_app(|b| b.config("auth.login_throttle_per_minute", 2)).await;
    fake.on_code("github", "code-ada", ada());
    callback(&app, "github", "code", "nvs1.bad")
        .await
        .assert_status(400);
    callback(&app, "github", "code", "nvs1.bad")
        .await
        .assert_status(400);
    let refused = callback(&app, "github", "code", "nvs1.bad")
        .await
        .assert_status(429);
    assert!(refused.header("retry-after").is_some());
    sign_in(&app, "github", "code-ada").await.assert_status(429);
}

/// Callbacks from the addresses of one IPv6 network share the throttle.
#[tokio::test]
async fn the_callback_throttle_counts_ipv6_clients_by_network() {
    let (app, _fake) = fake_app(|b| {
        b.config("auth.login_throttle_per_minute", 2)
            .config("http.trust_proxy", true)
    })
    .await;
    let from = |ip: &'static str| {
        let app = &app;
        async move {
            app.client()
                .post("/api/auth/social/github/callback")
                .header("x-forwarded-for", ip)
                .json(&json!({ "code": "code", "state": "nvs1.bad" }))
                .send()
                .await
        }
    };
    from("2001:db8:7:7::1").await.assert_status(400);
    from("2001:db8:7:7:1:2:3:4").await.assert_status(400);
    from("2001:db8:7:7:ffff::1").await.assert_status(429);
    // Another /64, and IPv4 clients, have their own budgets.
    from("2001:db8:7:8::1").await.assert_status(400);
    from("192.0.2.1").await.assert_status(400);
    from("192.0.2.2").await.assert_status(400);
}

#[tokio::test]
async fn a_successful_sign_in_resets_the_throttle() {
    let (app, fake) = fake_app(|b| b.config("auth.login_throttle_per_minute", 2)).await;
    fake.on_code("github", "code-ada", ada());
    callback(&app, "github", "code", "nvs1.bad")
        .await
        .assert_status(400);
    sign_in(&app, "github", "code-ada").await.assert_status(200);
    callback(&app, "github", "code", "nvs1.bad")
        .await
        .assert_status(400);
    callback(&app, "github", "code", "nvs1.bad")
        .await
        .assert_status(400);
}

#[tokio::test]
async fn the_routes_can_be_turned_off() {
    let (app, _fake) = fake_app(|b| b.config("social.routes", false)).await;
    redirect(&app, "github", "", None).await.assert_status(404);
}

#[tokio::test]
async fn the_boot_fails_naming_the_key() {
    let error = builder()
        .config("social.providers.github.client_id", "")
        .try_build()
        .await
        .expect_err("the boot fails")
        .to_string();
    assert!(
        error.contains("social.providers.github.client_id is required"),
        "{error}"
    );

    let error = builder()
        .config("social.redirect_base", "not a url")
        .try_build()
        .await
        .expect_err("the boot fails")
        .to_string();
    assert!(error.contains("social.redirect_base"), "{error}");

    let error = builder()
        .config("social.providers.zoho", json!({ "type": "custom", "client_id": "z", "token_url": "https://accounts.zoho.com/oauth/v2/token" }))
        .try_build()
        .await
        .expect_err("the boot fails")
        .to_string();
    assert!(error.contains("SocialModule::for_feature"), "{error}");
}

#[tokio::test]
async fn a_real_social_builds_the_same_redirect_without_calling_anyone() {
    let app: TestApp = builder().build().await;
    let body = redirect(&app, "github", "", None)
        .await
        .assert_status(200)
        .json_value();
    let url = body["url"].as_str().unwrap();
    assert_eq!(
        query_param(url, "redirect_uri").as_deref(),
        Some(format!("{REDIRECT_BASE}/github").as_str())
    );
}

#[tokio::test]
async fn scopes_params_and_pkce_shape_the_url() {
    let (app, _fake) = fake_app(|b| {
        b.config("social.providers.github.scopes", "repo, gist")
            .config(
                "social.providers.github.params",
                json!({ "allow_signup": "false" }),
            )
            .config("social.providers.github.pkce", false)
    })
    .await;
    let body = redirect(&app, "github", "", None)
        .await
        .assert_status(200)
        .json_value();
    let url = body["url"].as_str().unwrap();
    assert_eq!(
        query_param(url, "scope").as_deref(),
        Some("read:user user:email repo gist")
    );
    assert_eq!(query_param(url, "allow_signup").as_deref(), Some("false"));
    assert_eq!(query_param(url, "code_challenge"), None);

    // The service adds per-flow scopes and parameters.
    let social = app.get::<navree_social::Social>().await;
    let redirect = social
        .redirect_with(
            "github",
            navree_social::RedirectOptions {
                scopes: vec!["read:org".to_owned()],
                params: [("prompt".to_owned(), "consent".to_owned())].into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        query_param(&redirect.url, "scope").as_deref(),
        Some("read:user user:email repo gist read:org")
    );
    assert_eq!(
        query_param(&redirect.url, "prompt").as_deref(),
        Some("consent")
    );
}

#[tokio::test]
async fn openapi_documents_the_social_routes() {
    let (app, _fake) = fake_app(|b| {
        b.module(navree_api::ApiModule::for_root(
            navree_api::ApiOptions::default(),
        ))
    })
    .await;
    let doc = app
        .client()
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let paths = &doc["paths"];
    for (path, method, status) in [
        ("/api/auth/social/{provider}/redirect", "get", "200"),
        ("/api/auth/social/{provider}/callback", "post", "200"),
        ("/api/auth/social/accounts", "get", "200"),
        ("/api/auth/social/{provider}/link", "post", "201"),
        ("/api/auth/social/accounts/{provider}", "delete", "204"),
    ] {
        let operation = &paths[path][method];
        assert_eq!(operation["tags"], json!(["Auth"]), "{path}: {operation}");
        assert!(
            operation["responses"].get(status).is_some(),
            "{path} {status}: {operation}"
        );
    }
    let schemas = &doc["components"]["schemas"];
    for schema in ["SocialRedirect", "SocialCallback", "LinkedAccount"] {
        assert!(schemas.get(schema).is_some(), "{schema}");
    }
}
