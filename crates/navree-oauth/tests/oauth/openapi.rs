//! The routes describe themselves in the OpenAPI document.

use navree_api::ApiModule;

use crate::support::{World, builder};

#[tokio::test]
async fn the_routes_are_in_the_openapi_document() {
    let world = World::with(
        builder()
            .config("oauth.dynamic_registration", true)
            .module(ApiModule::from_config()),
    )
    .await;
    let document = world
        .client()
        .get("/api/openapi.json")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let paths = document["paths"].as_object().unwrap();
    for path in [
        "/oauth/authorize",
        "/oauth/token",
        "/oauth/device_authorization",
        "/oauth/revoke",
        "/oauth/introspect",
        "/oauth/userinfo",
        "/oauth/jwks",
        "/oauth/register",
        "/.well-known/openid-configuration",
        "/api/oauth/requests/{id}",
        "/api/oauth/requests/{id}/approve",
        "/api/oauth/device/{user_code}",
        "/api/oauth/personal-access-tokens",
        "/api/oauth/authorizations/{client_id}",
    ] {
        assert!(paths.contains_key(path), "{path} missing");
    }
    let schemas = document["components"]["schemas"].as_object().unwrap();
    for schema in [
        "TokenResponse",
        "AuthorizationRequestView",
        "PersonalAccessTokenResource",
        "CreatePersonalAccessTokenInput",
    ] {
        assert!(schemas.contains_key(schema), "{schema} missing");
    }
}
