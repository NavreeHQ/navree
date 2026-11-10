//! Configuration: validation at boot and the environment.

use navree_config::ConfigSources;
use serde_json::json;

use crate::support::{World, builder};

async fn boot_error(builder: navree_testing::TestAppBuilder) -> String {
    match builder.try_build().await {
        Ok(_) => panic!("the app booted"),
        Err(error) => error.to_string(),
    }
}

#[tokio::test]
async fn invalid_options_fail_the_boot() {
    // (`config` objects merge into the test app's scopes, so these are new
    // names.)
    let error = boot_error(builder().config(
        "oauth.scopes",
        json!({ "invoices:read": { "permissions": ["invoices.read"] } }),
    ))
    .await;
    assert!(
        error.contains("oauth.scopes.invoices:read.description is required"),
        "{error}"
    );
    let error = boot_error(builder().config(
        "oauth.scopes",
        json!({ "invoices:write": { "description": "Write", "permissions": ["invoices"] } }),
    ))
    .await;
    assert!(error.contains("`invoices`"), "{error}");
    let error = boot_error(builder().config("oauth.consent_url", "/consent")).await;
    assert!(error.contains("oauth.consent_url"), "{error}");
    let error = boot_error(builder().config("oauth.signing_key", "ed25519:nope")).await;
    assert!(error.contains("oauth.signing_key"), "{error}");
}

#[tokio::test]
async fn keys_come_from_the_environment() {
    let world = World::with(builder().sources(ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_OAUTH__ACCESS_TOKEN_TTL_SECS", "1800"),
            ("APP_OAUTH__ISSUER", "https://env.acme.test"),
        ],
    )))
    .await;
    world
        .client()
        .get("/.well-known/oauth-authorization-server")
        .send()
        .await
        .assert_json_path("issuer", json!("https://env.acme.test"));
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    let tokens = navree_oauth::testing::authorize(
        &world.client(),
        client.client.id,
        &token,
        &["deals:read"],
    )
    .await;
    assert_eq!(tokens.expires_in, 1800);
}

#[tokio::test]
async fn scopes_load_from_a_toml_file() {
    let dir = navree_testing::TempDirGuard::new();
    let path = dir.path().join("app.toml");
    std::fs::write(
        &path,
        r#"
[oauth]
consent_url = "https://app.acme.test/oauth/consent"

[oauth.scopes."deals:read"]
description = "Read your deals"
permissions = ["deals.read"]

[oauth.scopes."invoices:read"]
description = "Read your invoices"
permissions = ["invoices.read", "payments.read"]
"#,
    )
    .unwrap();
    let world = World::with(builder().sources(ConfigSources::new().file(&path))).await;
    let metadata = world
        .client()
        .get("/.well-known/openid-configuration")
        .send()
        .await
        .json_value();
    let scopes = metadata["scopes_supported"].as_array().unwrap();
    assert!(scopes.contains(&json!("deals:read")), "{scopes:?}");
    assert!(scopes.contains(&json!("invoices:read")), "{scopes:?}");
}

#[tokio::test]
async fn without_a_consent_page_the_code_flow_reports_unavailable() {
    let world = World::with(builder().config("oauth.consent_url", serde_json::Value::Null)).await;
    let client = world.public_client(&["deals:read"]).await;
    let (_, challenge) = navree_oauth::testing::pkce_pair();
    let response = world
        .authorize(&[
            ("response_type", "code"),
            ("client_id", &client.client.id.to_string()),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ])
        .await
        .assert_status(302);
    let location = response.header("location").unwrap();
    assert_eq!(
        crate::support::param(location, "error").as_deref(),
        Some("temporarily_unavailable")
    );
}
