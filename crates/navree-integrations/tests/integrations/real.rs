//! The real providers against wiremock servers answering recorded
//! responses: the requests Navree sends, and how it reads the answers.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use navree_integrations::{
    ConnectCallback, ConnectOptions, ConnectionStatus, ConnectionUnhealthy, CredentialKind,
    GoogleWorkspace, IntegrationsError, MicrosoftGraph, NewApiKey, NewServiceAccount, Owner,
};
use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RsaKeyPair, UnparsedPublicKey};
use serde_json::Value;
use wiremock::matchers::{
    body_string_contains, header, method, path, query_param, query_param_is_missing,
};
use wiremock::{Match, Mock, MockServer, Request, ResponseTemplate};

use crate::support::{PEM, fixture, real, service_account};

const TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";

fn json(name: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(fixture(name))
}

/// The form fields of a request body.
fn form(request: &Request) -> Vec<(String, String)> {
    form_urlencoded::parse(&request.body)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn field(request: &Request, name: &str) -> Option<String> {
    form(request)
        .into_iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

#[tokio::test]
async fn microsoft_app_only_tokens_and_graph_paging() {
    let mock = MockServer::start().await;
    let h = real(&mock.uri(), |b| b).await;
    Mock::given(method("POST"))
        .and(path(format!("/{TENANT}/oauth2/v2.0/token")))
        .and(body_string_contains("grant_type=client_credentials"))
        .and(body_string_contains(
            "client_id=0f6c1d2e-8a3b-4c5d-9e7f-1a2b3c4d5e6f",
        ))
        .and(body_string_contains("client_secret=ms-secret"))
        .and(body_string_contains(
            "scope=https%3A%2F%2Fgraph.microsoft.com%2F.default",
        ))
        .respond_with(json("microsoft-token.json"))
        .expect(1)
        .mount(&mock)
        .await;
    let token = fixture("microsoft-token.json")["access_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let page1 = fixture("graph-users-page1.json")
        .to_string()
        .replace("{{base}}", &mock.uri());
    Mock::given(method("GET"))
        .and(path("/v1.0/users"))
        .and(query_param("$skiptoken", "RFNwdAIAAQAAAD8"))
        .and(header("authorization", format!("Bearer {token}").as_str()))
        .respond_with(json("graph-users-page2.json"))
        .with_priority(1)
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1.0/users"))
        .and(query_param_is_missing("$skiptoken"))
        .and(header("authorization", format!("Bearer {token}").as_str()))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(page1, "application/json"),
        )
        .mount(&mock)
        .await;

    let integrations = h.integrations().await;
    let state = integrations
        .connect_url(Owner::App, "microsoft", ConnectOptions::default())
        .await
        .unwrap()
        .state;
    let connection = integrations
        .complete(
            Owner::App,
            "microsoft",
            &ConnectCallback::admin_consent(state, TENANT),
            None,
        )
        .await
        .unwrap()
        .connection;
    assert_eq!(connection.kind, CredentialKind::AppOnly);

    let graph = integrations
        .client_for::<MicrosoftGraph>(Owner::App)
        .await
        .unwrap();
    let users: Vec<Value> = graph.paginate("/users?$select=id,mail").await.unwrap();
    let mails: Vec<&str> = users.iter().map(|u| u["mail"].as_str().unwrap()).collect();
    assert_eq!(
        mails,
        vec!["AdeleV@contoso.com", "AlexW@contoso.com", "DiegoS@contoso.com"]
    );
    // The token lasts an hour: one request for both pages and the check
    // below reuses nothing (checks always mint).
    assert_eq!(graph.tenant_id(), Some(TENANT));
}

#[tokio::test]
async fn a_scope_set_with_its_own_registration_uses_its_client() {
    let mock = MockServer::start().await;
    let h = real(&mock.uri(), |b| b).await;
    Mock::given(method("POST"))
        .and(path(format!("/{TENANT}/oauth2/v2.0/token")))
        .and(body_string_contains("client_id=ms-mail-app"))
        .and(body_string_contains("client_secret=ms-mail-secret"))
        .respond_with(json("microsoft-token.json"))
        .expect(1)
        .mount(&mock)
        .await;
    let integrations = h.integrations().await;
    let state = integrations
        .connect_url(
            Owner::App,
            "microsoft",
            ConnectOptions::default().scope_set("mail_send"),
        )
        .await
        .unwrap()
        .state;
    let connection = integrations
        .complete(
            Owner::App,
            "microsoft",
            &ConnectCallback::admin_consent(state, TENANT),
            None,
        )
        .await
        .unwrap()
        .connection;
    integrations.token(connection.id).await.unwrap();
}

#[tokio::test]
async fn a_tenant_that_removed_the_app_is_unhealthy_and_an_outage_is_not() {
    let mock = MockServer::start().await;
    let h = real(&mock.uri(), |b| b).await;
    Mock::given(method("POST"))
        .and(path(format!("/{TENANT}/oauth2/v2.0/token")))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/{TENANT}/oauth2/v2.0/token")))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(fixture("microsoft-error.json")),
        )
        .mount(&mock)
        .await;
    let integrations = h.integrations().await;
    let state = integrations
        .connect_url(Owner::App, "microsoft", ConnectOptions::default())
        .await
        .unwrap()
        .state;
    let connection = integrations
        .complete(
            Owner::App,
            "microsoft",
            &ConnectCallback::admin_consent(state, TENANT),
            None,
        )
        .await
        .unwrap()
        .connection;

    let outage = integrations.token(connection.id).await.unwrap_err();
    assert!(matches!(outage, IntegrationsError::Unavailable { .. }), "{outage:?}");
    assert_eq!(
        integrations.find(connection.id).await.unwrap().status,
        ConnectionStatus::Healthy
    );

    let refused = integrations.token(connection.id).await.unwrap_err();
    assert!(matches!(refused, IntegrationsError::Reauthorize { .. }), "{refused:?}");
    let stored = integrations.find(connection.id).await.unwrap();
    assert_eq!(stored.status, ConnectionStatus::Unhealthy);
    assert!(
        stored
            .last_error
            .as_deref()
            .unwrap()
            .starts_with("unauthorized_client: AADSTS700016"),
        "{:?}",
        stored.last_error
    );
    assert_eq!(h.inbox.of::<ConnectionUnhealthy>().len(), 1);
}

#[tokio::test]
async fn a_slack_install_is_exchanged_checked_and_revoked() {
    let mock = MockServer::start().await;
    let h = real(&mock.uri(), |b| b).await;
    Mock::given(method("POST"))
        .and(path("/api/oauth.v2.access"))
        .and(body_string_contains("code=slack-code"))
        .and(body_string_contains("client_id=4129031.6713"))
        .and(body_string_contains("client_secret=slack-secret"))
        .and(body_string_contains("redirect_uri=https%3A%2F%2Fapp.acme.test"))
        .respond_with(json("slack-oauth-v2-access.json"))
        .expect(1)
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/oauth.v2.access"))
        .and(body_string_contains("code=used-code"))
        .respond_with(json("slack-invalid-code.json"))
        .mount(&mock)
        .await;
    let bot = "Bearer xoxb-17653672481-19874698323-pdFZKVeTuE8sk7oOcBrzbqgy";
    Mock::given(method("POST"))
        .and(path("/api/auth.test"))
        .and(header("authorization", bot))
        .respond_with(json("slack-auth-test.json"))
        .expect(1)
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/auth.revoke"))
        .and(header("authorization", bot))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "ok": true, "revoked": true })))
        .expect(1)
        .mount(&mock)
        .await;

    let integrations = h.integrations().await;
    let state = integrations
        .connect_url(Owner::App, "slack", ConnectOptions::default())
        .await
        .unwrap()
        .state;
    let connection = integrations
        .complete(
            Owner::App,
            "slack",
            &ConnectCallback::code(state, "slack-code"),
            None,
        )
        .await
        .unwrap()
        .connection;
    assert_eq!(connection.external_id.as_deref(), Some("T9TK3CUKW"));
    assert_eq!(connection.display_name.as_deref(), Some("Slack Softball Team"));
    assert_eq!(connection.scopes, vec!["chat:write", "users:read"]);
    assert_eq!(
        integrations.check(connection.id).await.unwrap().status,
        ConnectionStatus::Healthy
    );
    integrations.disconnect(connection.id).await.unwrap();

    let state = integrations
        .connect_url(Owner::App, "slack", ConnectOptions::default())
        .await
        .unwrap()
        .state;
    let error = integrations
        .complete(
            Owner::App,
            "slack",
            &ConnectCallback::code(state, "used-code"),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "Slack refused the connection: invalid_code");
}

/// Matches a service-account token request whose assertion is an RS256
/// JWT signed by the fixture key, verified with `ring` directly.
struct SignedAssertion {
    audience: String,
    subject: &'static str,
}

impl Match for SignedAssertion {
    fn matches(&self, request: &Request) -> bool {
        if field(request, "grant_type").as_deref()
            != Some("urn:ietf:params:oauth:grant-type:jwt-bearer")
        {
            return false;
        }
        let Some(assertion) = field(request, "assertion") else {
            return false;
        };
        let parts: Vec<&str> = assertion.split('.').collect();
        if parts.len() != 3 {
            return false;
        }
        let body: String = PEM
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();
        let der = STANDARD.decode(body).unwrap();
        let pair = RsaKeyPair::from_pkcs8(&der).unwrap();
        let public = UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, pair.public().as_ref());
        let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        let input = format!("{}.{}", parts[0], parts[1]);
        if public.verify(input.as_bytes(), &signature).is_err() {
            return false;
        }
        let header: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        let claims: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        header["alg"] == "RS256"
            && header["kid"] == "4f2a9c"
            && claims["iss"] == "navree@acme-prod.iam.gserviceaccount.com"
            && claims["sub"] == self.subject
            && claims["aud"] == self.audience.as_str()
            && claims["scope"]
                == "https://www.googleapis.com/auth/admin.directory.user.readonly"
            && claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap() == 3600
    }
}

#[tokio::test]
async fn google_service_accounts_sign_their_assertions_with_rs256() {
    let mock = MockServer::start().await;
    let h = real(&mock.uri(), |b| b).await;
    let audience = format!("{}/token", mock.uri());
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(SignedAssertion {
            audience: audience.clone(),
            subject: "admin@acme.test",
        })
        .respond_with(json("google-token.json"))
        .expect(1)
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(SignedAssertion {
            audience,
            subject: "intern@acme.test",
        })
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(fixture("google-unauthorized-client.json")),
        )
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/google/admin/directory/v1/users"))
        .and(query_param("customer", "my_customer"))
        .and(header(
            "authorization",
            "Bearer ya29.c.b0AXv0zTPrW1u8Wh5Iq9Xq7wKZq2m1Y6N3dQ",
        ))
        .respond_with(json("google-users.json"))
        .expect(1)
        .mount(&mock)
        .await;

    let integrations = h.integrations().await;
    let connection = integrations
        .connect_service_account(
            Owner::App,
            "google",
            NewServiceAccount::new(service_account(), Some("admin@acme.test".into())),
        )
        .await
        .unwrap();
    assert_eq!(connection.external_id.as_deref(), Some("acme.test"));
    let google = integrations
        .client_for::<GoogleWorkspace>(Owner::App)
        .await
        .unwrap();
    let users: Vec<Value> = google
        .paginate("/admin/directory/v1/users?customer=my_customer", "users")
        .await
        .unwrap();
    assert_eq!(users.len(), 2);
    assert_eq!(users[0]["primaryEmail"], "liz@acme.test");

    // A user without domain-wide delegation for the scopes is refused.
    let error = integrations
        .connect_service_account(
            Owner::App,
            "google",
            NewServiceAccount::new(service_account(), Some("intern@acme.test".into())),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Google Workspace refused the connection: unauthorized_client"
    );
}

#[tokio::test]
async fn oauth2_codes_are_exchanged_with_pkce_refreshed_and_revoked() {
    let mock = MockServer::start().await;
    let h = real(&mock.uri(), |b| b).await;
    Mock::given(method("POST"))
        .and(path("/oauth/v1/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=hub-code"))
        .and(body_string_contains("code_verifier="))
        .and(body_string_contains("client_id=hub-client"))
        .and(body_string_contains("client_secret=hub-secret"))
        .and(body_string_contains(
            "redirect_uri=https%3A%2F%2Fapp.acme.test%2Fsettings%2Fintegrations%2Fcallback%2Fhubspot",
        ))
        .respond_with(json("hubspot-token.json"))
        .expect(1)
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/v1/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains(
            "refresh_token=6f18f21e-a743-4509-b7fd-1a5e632fffa1",
        ))
        .respond_with(json("hubspot-refreshed.json"))
        .expect(1)
        .mount(&mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/v1/revoke"))
        .and(body_string_contains(
            "token=6f18f21e-a743-4509-b7fd-1a5e632fffa1",
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/hub/crm/v3/objects/contacts"))
        .and(header(
            "authorization",
            "Bearer CJSP5qf1KhICAQEYs-gDIIGOBii1hQIyGQAf3xBKmlwHjX7OIpuIFEavB2-qYAGQsF4",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "results": [] })))
        .expect(1)
        .mount(&mock)
        .await;

    let integrations = h.integrations().await;
    let redirect = integrations
        .connect_url(Owner::App, "hubspot", ConnectOptions::default())
        .await
        .unwrap();
    let connection = integrations
        .complete(
            Owner::App,
            "hubspot",
            &ConnectCallback::code(redirect.state, "hub-code"),
            None,
        )
        .await
        .unwrap()
        .connection;
    h.clock.advance(Duration::from_secs(1800));
    let hubspot = integrations.client(Owner::App, "hubspot", "default").await.unwrap();
    let contacts: Value = hubspot
        .get("/crm/v3/objects/contacts")
        .send_json()
        .await
        .unwrap();
    assert_eq!(contacts["results"], serde_json::json!([]));
    integrations.disconnect(connection.id).await.unwrap();
}

#[tokio::test]
async fn api_keys_are_checked_against_the_health_url() {
    let mock = MockServer::start().await;
    let h = real(&mock.uri(), |b| b).await;
    Mock::given(method("GET"))
        .and(path("/edr/v1/health"))
        .and(header("x-api-key", "edr-live-key"))
        .respond_with(ResponseTemplate::new(200))
        .expect(2)
        .mount(&mock)
        .await;
    let integrations = h.integrations().await;
    let connection = integrations
        .connect_api_key(Owner::App, "edr", NewApiKey::new("edr-live-key"))
        .await
        .unwrap();
    assert_eq!(
        integrations.check(connection.id).await.unwrap().status,
        ConnectionStatus::Healthy
    );
}
