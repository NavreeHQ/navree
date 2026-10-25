//! The whole NestJS-style flow through `TestClient` with bearer headers:
//! login → authenticated request → refresh → logout, and the OpenAPI
//! document of the auth routes.

use keel_api::{ApiModule, ApiOptions};
use serde_json::{Value, json};

use crate::support::{self, PASSWORD, login, refresh, tokens};

#[tokio::test]
async fn login_then_request_then_refresh_then_logout() {
    let app = support::app().await;
    let user = support::create_user(&app, "ada@example.com").await;
    let client = app.client();

    // 1. Login.
    let (access, refresh_token) = tokens(
        &login(&client, "ada@example.com", PASSWORD)
            .await
            .assert_status(200),
    );

    // 2. An authenticated request with the access token.
    client
        .get("/api/probe/whoami")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(user.id));

    // 3. Refresh: a new pair; the new access token works too.
    let (access, refresh_token) =
        tokens(&refresh(&client, &refresh_token).await.assert_status(200));
    client
        .get("/api/auth/me")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("id", json!(user.id));

    // 4. Logout revokes the session's refresh token.
    client
        .post("/api/auth/logout")
        .bearer(&access)
        .json(&json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .assert_status(204);
    refresh(&client, &refresh_token)
        .await
        .assert_error(401, "Invalid refresh token");
}

#[tokio::test]
async fn openapi_documents_the_auth_routes() {
    let app = support::builder()
        .module(ApiModule::for_root(ApiOptions::default()))
        .build()
        .await;
    let doc = app
        .client()
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let paths = &doc["paths"];
    let login = &paths["/api/auth/login"]["post"];
    assert_eq!(login["tags"], json!(["Auth"]));
    assert!(
        login.get("security").is_none() || login["security"] == json!([]),
        "{login}"
    );
    let body = login["requestBody"].to_string();
    assert!(body.contains("LoginInput"), "{body}");
    for status in ["200", "400", "401", "429"] {
        assert!(
            login["responses"].get(status).is_some(),
            "{status}: {login}"
        );
    }
    assert!(
        login_200_schema(&doc).to_string().contains("LoginResponse"),
        "{login}"
    );
    for path in [
        "/api/auth/register",
        "/api/auth/refresh",
        "/api/auth/logout",
    ] {
        assert!(paths[path]["post"].is_object(), "{path}");
    }
    let me = &paths["/api/auth/me"]["get"];
    assert!(
        me["responses"]["200"].to_string().contains("UserResource"),
        "{me}"
    );
    assert_ne!(me["security"], Value::Null, "{me}");
    let schemas = &doc["components"]["schemas"];
    for schema in [
        "LoginInput",
        "LoginResponse",
        "RegisterInput",
        "RefreshInput",
        "LogoutInput",
        "TokenPair",
        "UserResource",
    ] {
        assert!(schemas[schema].is_object(), "{schema} missing: {schemas}");
    }
    assert!(
        schemas["UserResource"]["properties"]["__type"].is_object(),
        "{}",
        schemas["UserResource"]
    );
    let text = doc.to_string();
    assert!(!text.contains("password_hash"), "{text}");
}

#[tokio::test]
async fn openapi_documents_the_account_routes() {
    let app = support::builder()
        .module(ApiModule::for_root(ApiOptions::default()))
        .build()
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
        ("/api/auth/forgot-password", "post", "202"),
        ("/api/auth/reset-password", "post", "204"),
        ("/api/auth/email/verification", "post", "202"),
        ("/api/auth/email/verify/{id}/{hash}", "get", "200"),
        ("/api/auth/me/password", "patch", "200"),
        ("/api/auth/me", "patch", "200"),
        ("/api/auth/2fa/enable", "post", "200"),
        ("/api/auth/2fa/confirm", "post", "200"),
        ("/api/auth/2fa/disable", "post", "204"),
        ("/api/auth/2fa/recovery-codes", "post", "200"),
        ("/api/auth/2fa/verify", "post", "200"),
    ] {
        let operation = &paths[path][method];
        assert_eq!(operation["tags"], json!(["Auth"]), "{path}: {operation}");
        assert!(
            operation["responses"].get(status).is_some(),
            "{path} {status}: {operation}"
        );
    }
    let schemas = &doc["components"]["schemas"];
    for schema in [
        "ForgotPasswordInput",
        "ResetPasswordInput",
        "ChangePasswordInput",
        "UpdateProfileInput",
        "ConfirmTwoFactorInput",
        "DisableTwoFactorInput",
        "TwoFactorVerifyInput",
        "TwoFactorSetup",
        "RecoveryCodesResponse",
        "AuthMessage",
        "TwoFactorChallenge",
        "LoginResponse",
    ] {
        assert!(schemas[schema].is_object(), "{schema} missing: {schemas}");
    }
    assert!(
        schemas["TwoFactorChallenge"]["properties"]["two_factor_required"].is_object(),
        "{}",
        schemas["TwoFactorChallenge"]
    );
    // The login answers with tokens or, with two-factor on, a challenge:
    // generated clients must see both shapes.
    let login = login_200_schema(&doc);
    let shapes: Vec<String> = login["oneOf"]
        .as_array()
        .unwrap_or_else(|| panic!("a oneOf: {login}"))
        .iter()
        .map(Value::to_string)
        .collect();
    assert_eq!(shapes.len(), 2, "{login}");
    assert!(shapes[0].contains("LoginResponse"), "{login}");
    assert!(shapes[1].contains("TwoFactorChallenge"), "{login}");
}

/// The schema of the login's 200 response, its `$ref` resolved.
fn login_200_schema(doc: &Value) -> &Value {
    let schema = &doc["paths"]["/api/auth/login"]["post"]["responses"]["200"]["content"]["application/json"]
        ["schema"];
    match schema["$ref"].as_str() {
        Some(reference) => {
            let name = reference
                .strip_prefix("#/components/schemas/")
                .expect("a component reference");
            &doc["components"]["schemas"][name]
        }
        None => schema,
    }
}
