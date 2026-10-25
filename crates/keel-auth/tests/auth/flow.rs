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
        login["responses"]["200"]
            .to_string()
            .contains("LoginResponse"),
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
