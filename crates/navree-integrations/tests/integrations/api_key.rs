//! API-key connections, with the fake.

use http::Method;
use navree_http_client::FakeResponse;
use navree_integrations::{CredentialKind, IntegrationsError, NewApiKey, Owner};
use serde_json::json;

use crate::support::harness;

#[tokio::test]
async fn an_api_key_is_tried_on_the_health_url_then_stored_encrypted() {
    let h = harness(|b| b).await;
    h.fake.api().on(
        Method::GET,
        "https://acme.edr.example/api/v1/health",
        FakeResponse::new(200).json(&json!({ "status": "ok" })),
    );
    let integrations = h.integrations().await;
    let connection = integrations
        .connect_api_key(
            Owner::App,
            "edr",
            NewApiKey::new("edr-key-4f2a9c")
                .api_url("https://acme.edr.example/api")
                .field("site_id", "17"),
        )
        .await
        .unwrap();
    assert_eq!(connection.kind, CredentialKind::ApiKey);
    assert_eq!(connection.provider_name, "Sentinel EDR");
    h.fake.api().assert_sent(|request| {
        request.url().as_str() == "https://acme.edr.example/api/v1/health"
            && request.header("x-api-key") == Some("edr-key-4f2a9c")
    });

    let credentials = integrations.api_key(connection.id).await.unwrap();
    assert_eq!(credentials.key.expose(), "edr-key-4f2a9c");
    assert_eq!(credentials.api_url.as_deref(), Some("https://acme.edr.example/api"));
    assert_eq!(credentials.fields["site_id"], "17");
    assert_eq!(
        integrations.token(connection.id).await.unwrap().expose(),
        "edr-key-4f2a9c"
    );
    // Other kinds have no API key.
    let microsoft = h
        .connect_microsoft(Owner::App, "9188040d-6c67-4c5b-b112-36a304b66dad")
        .await;
    assert!(matches!(
        integrations.api_key(microsoft.id).await,
        Err(IntegrationsError::Unsupported { .. })
    ));
}

#[tokio::test]
async fn a_refused_or_incomplete_key_is_not_stored() {
    let h = harness(|b| b).await;
    h.fake.api().on(
        Method::GET,
        "https://acme.edr.example/api/v1/health",
        FakeResponse::new(401).json(&json!({ "error": "unauthorized" })),
    );
    let integrations = h.integrations().await;
    let error = integrations
        .connect_api_key(
            Owner::App,
            "edr",
            NewApiKey::new("wrong").api_url("https://acme.edr.example/api"),
        )
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "Sentinel EDR refused the connection: http_401");
    // The provider has no api_url of its own, so the connection needs one.
    assert!(matches!(
        integrations
            .connect_api_key(Owner::App, "edr", NewApiKey::new("k"))
            .await,
        Err(IntegrationsError::InvalidCredentials(_))
    ));
    assert!(matches!(
        integrations
            .connect_api_key(
                Owner::App,
                "edr",
                NewApiKey::new("  ").api_url("https://acme.edr.example/api")
            )
            .await,
        Err(IntegrationsError::InvalidCredentials(_))
    ));
    assert!(matches!(
        integrations
            .connect_api_key(Owner::App, "slack", NewApiKey::new("xoxb"))
            .await,
        Err(IntegrationsError::Unsupported { .. })
    ));
    assert!(integrations.connections(Owner::App).await.unwrap().is_empty());
}
