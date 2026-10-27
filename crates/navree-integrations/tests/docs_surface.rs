//! Every public item `docs/packages/integrations` names (types, fields,
//! methods, configuration keys, events, routes, commands and fake helpers),
//! so a rename fails the build or this test. The behavior is exercised in
//! `tests/integrations`; the page's examples run in the facade's
//! `tests/integrations_docs.rs`.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use navree_config::{Config, ConfigSources};
use navree_contracts::Secret;
use navree_events::Event;
use navree_http_client::FakeServer;
use navree_integrations::{
    ApiKeyCredentials, ApiKeyInput, ApiRequest, AppCredentials, CheckSummary, ClientAuth,
    CompletedConnection, ConnectCallback, ConnectOptions, ConnectQuery, ConnectRedirect,
    Connection, ConnectionInfo, ConnectionRestored, ConnectionStatus, ConnectionUnhealthy,
    CredentialKind, DEFAULT_SCOPE_SET, DefaultScopes, FakeGrant, GoogleClient, GoogleWorkspace,
    GraphClient, Health, INTEGRATIONS_MIGRATIONS, IntegrationClient, IntegrationConnected,
    IntegrationConnection, IntegrationDisconnected, IntegrationOwner, Integrations,
    IntegrationsController, IntegrationsError, IntegrationsFake, IntegrationsModule,
    IntegrationsOptions, Level, MANAGE_PERMISSION, MicrosoftGraph, MicrosoftMode, Minted,
    ModuleBuilderIntegrationsExt, NewApiKey, NewServiceAccount, Owner, OwnerType, Provider,
    ProviderConfig, ProviderInfo, ProviderType, ScopeSet, ScopeSetInfo, ServiceAccountInput,
    ServiceAccountKey, Slack, SlackClient,
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn the_configuration_keys_have_their_documented_defaults() {
    let IntegrationsOptions {
        redirect_base,
        routes,
        level,
        state_ttl_secs,
        check_interval_mins,
        unhealthy_after,
        lock_timeout_secs,
        providers,
        mailboxes,
    } = IntegrationsOptions::default();
    // Feature `mail` (on in the tests): `integrations.mailboxes`.
    let navree_integrations::MailboxesOptions {
        graph_notification_url,
        gmail_topic,
        gmail_token,
        renew_before_hours,
    } = mailboxes;
    assert_eq!((graph_notification_url, gmail_topic), (None, None));
    assert!(gmail_token.is_none());
    assert_eq!(renew_before_hours, 24);
    assert_eq!(redirect_base, None);
    assert!(routes);
    assert_eq!(level, Level::Workspace);
    assert_eq!(state_ttl_secs, 600);
    assert_eq!(check_interval_mins, 60);
    assert_eq!(unhealthy_after, 3);
    assert_eq!(lock_timeout_secs, 15);
    assert!(providers.is_empty());

    let ProviderConfig {
        kind,
        name,
        client_id,
        client_secret,
        tenant,
        mode,
        scopes,
        params,
        pkce,
        client_auth,
        authorize_url,
        token_url,
        api_url,
        revoke_url,
        health_url,
        authority_url,
        header,
        scheme,
        apps,
    } = ProviderConfig::default();
    let _: (Option<ProviderType>, Option<bool>) = (kind, pkce);
    let _: [Option<String>; 12] = [
        name,
        client_id,
        client_secret,
        tenant,
        authorize_url,
        token_url,
        api_url,
        revoke_url,
        health_url,
        authority_url,
        header,
        scheme,
    ];
    assert_eq!(mode, MicrosoftMode::App);
    assert_eq!(client_auth, ClientAuth::Post);
    let _: (Vec<String>, BTreeMap<String, String>) = (scopes, params);
    let _: BTreeMap<String, AppCredentials> = apps;
    let AppCredentials {
        client_id,
        client_secret,
    } = AppCredentials::default();
    let _: (String, Option<String>) = (client_id, client_secret);

    // The documented TOML, every key at once.
    let config = Config::load(&ConfigSources::new().set(
        "integrations",
        json!({
            "redirect_base": "https://app.acme.test/settings/integrations/callback",
            "routes": true,
            "level": "organization",
            "state_ttl_secs": 600,
            "check_interval_mins": 0,
            "unhealthy_after": 3,
            "lock_timeout_secs": 15,
            "providers": {
                "microsoft": {
                    "client_id": "0f6c1d2e-8a3b-4c5d-9e7f-1a2b3c4d5e6f",
                    "client_secret": "s",
                    "tenant": "organizations",
                    "mode": "delegated",
                    "authority_url": "https://login.microsoftonline.com",
                    "apps": { "mail_send": { "client_id": "5d4c", "client_secret": "s2" } }
                },
                "google": { "scopes": "https://www.googleapis.com/auth/admin.directory.user.readonly" },
                "slack": { "client_id": "4129031.6713", "scopes": ["chat:write"], "pkce": false },
                "hubspot": {
                    "type": "oauth2", "name": "HubSpot", "client_id": "c", "client_auth": "basic",
                    "authorize_url": "https://app.hubspot.com/oauth/authorize",
                    "token_url": "https://api.hubapi.com/oauth/v1/token",
                    "revoke_url": "https://api.hubapi.com/oauth/v1/revoke",
                    "api_url": "https://api.hubapi.com",
                    "params": { "optional_scope": "crm.lists.read" }
                },
                "edr": { "type": "api_key", "header": "X-Api-Key", "scheme": "", "health_url": "/v1/health" }
            }
        }),
    ))
    .unwrap();
    let options = config.typed::<IntegrationsOptions>().unwrap();
    assert_eq!(options.level, Level::Organization);
    assert_eq!(
        options.provider("microsoft").unwrap().mode,
        MicrosoftMode::Delegated
    );
    assert_eq!(
        options.provider("hubspot").unwrap().client_auth,
        ClientAuth::Basic
    );
    assert_eq!(
        options.provider("edr").unwrap().resolved_type("edr"),
        Some(ProviderType::ApiKey)
    );
    for kind in [
        ProviderType::Microsoft,
        ProviderType::Google,
        ProviderType::Slack,
        ProviderType::OAuth2,
        ProviderType::ApiKey,
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
    }
}

#[test]
fn the_documented_values_have_their_fields() {
    let ConnectOptions {
        scope_set,
        return_to,
        user_id,
        params,
    } = ConnectOptions::default()
        .scope_set("directory")
        .return_to("/settings")
        .user(Uuid::nil());
    let _: (Option<String>, Option<String>, Option<Uuid>) = (scope_set, return_to, user_id);
    let _: BTreeMap<String, String> = params;

    let ConnectCallback {
        state,
        code,
        tenant,
        admin_consent,
        error,
        error_description,
    } = ConnectCallback::code("nvi1.s", "c");
    let _: String = state;
    let _: [Option<String>; 5] = [code, tenant, admin_consent, error, error_description];
    let _ = ConnectCallback::admin_consent("nvi1.s", "tenant");

    let ConnectRedirect { url, state } = ConnectRedirect {
        url: String::new(),
        state: String::new(),
    };
    let _: (String, String) = (url, state);

    let NewServiceAccount {
        scope_set,
        key,
        subject,
        connected_by,
    } = NewServiceAccount::new(sample_key(), None)
        .scope_set("directory")
        .connected_by(Uuid::nil());
    let _: (String, ServiceAccountKey, Option<String>, Option<Uuid>) =
        (scope_set, key, subject, connected_by);

    let NewApiKey {
        scope_set,
        key,
        api_url,
        fields,
        connected_by,
    } = NewApiKey::new("k")
        .scope_set("default")
        .api_url("https://edr.example/api")
        .field("site_id", "17")
        .connected_by(Uuid::nil());
    let _: (String, String, Option<String>, Option<Uuid>) = (scope_set, key, api_url, connected_by);
    let _: BTreeMap<String, String> = fields;

    let ServiceAccountKey {
        client_email,
        private_key,
        private_key_id,
        client_id,
        project_id,
    } = sample_key();
    let _: (String, String) = (client_email, private_key);
    let _: [Option<String>; 3] = [private_key_id, client_id, project_id];

    let ApiKeyCredentials {
        key,
        api_url,
        fields,
    } = ApiKeyCredentials {
        key: Secret::new(String::new()),
        api_url: None,
        fields: BTreeMap::new(),
    };
    let _: (Secret<String>, Option<String>, BTreeMap<String, String>) = (key, api_url, fields);

    let CheckSummary {
        checked,
        healthy,
        unhealthy,
    } = CheckSummary::default();
    assert_eq!((checked, healthy, unhealthy), (0, 0, 0));

    let Minted {
        provider,
        grant_type,
        scope_set,
        subject,
        client_id,
    } = Minted {
        provider: String::new(),
        grant_type: String::new(),
        scope_set: String::new(),
        subject: None,
        client_id: None,
    };
    let _: (String, String, String, Option<String>, Option<String>) =
        (provider, grant_type, scope_set, subject, client_id);

    let _: FakeGrant = FakeGrant::new()
        .access_token("a")
        .refresh_token("r")
        .expires_in(60)
        .never_expires()
        .scopes(["s"])
        .external_id("T1")
        .display_name("Acme");

    let _ = ApiKeyInput {
        key: String::new(),
        api_url: None,
        fields: BTreeMap::new(),
        scope_set: None,
    };
    let _ = ServiceAccountInput {
        key: json!({}),
        subject: None,
        scope_set: None,
    };
    let _ = ConnectQuery {
        scope_set: None,
        return_to: None,
    };
}

fn sample_key() -> ServiceAccountKey {
    ServiceAccountKey {
        client_email: "navree@acme-prod.iam.gserviceaccount.com".into(),
        private_key: String::new(),
        private_key_id: None,
        client_id: None,
        project_id: None,
    }
}

#[test]
fn connection_info_has_the_documented_members() {
    let now: DateTime<Utc> = Utc::now();
    let info = ConnectionInfo {
        id: Uuid::nil(),
        owner_type: OwnerType::Workspace,
        owner_id: Some(Uuid::from_u128(5)),
        provider: "microsoft".into(),
        provider_name: "Microsoft 365".into(),
        scope_set: DEFAULT_SCOPE_SET.into(),
        kind: CredentialKind::AppOnly,
        external_id: None,
        display_name: None,
        scopes: Vec::new(),
        status: ConnectionStatus::Healthy,
        failures: 0,
        last_error: None,
        last_checked_at: None,
        connected_by: None,
        created_at: now,
        updated_at: now,
    };
    assert_eq!(info.owner(), Owner::Workspace(Uuid::from_u128(5)));
    assert!(info.is_healthy());
    let body = serde_json::to_value(&info).unwrap();
    for member in [
        "id",
        "owner_type",
        "owner_id",
        "provider",
        "provider_name",
        "scope_set",
        "kind",
        "external_id",
        "display_name",
        "scopes",
        "status",
        "failures",
        "last_error",
        "last_checked_at",
        "connected_by",
        "created_at",
        "updated_at",
    ] {
        assert!(body.get(member).is_some(), "{member}");
    }
    let completed = serde_json::to_value(CompletedConnection {
        connection: info,
        return_to: Some("/settings".into()),
    })
    .unwrap();
    assert_eq!(completed["return_to"], "/settings");
    assert_eq!(completed["kind"], "app_only");

    let Health {
        connection_id,
        status,
        failures,
        error,
        checked_at,
    } = Health {
        connection_id: Uuid::nil(),
        status: ConnectionStatus::Unhealthy,
        failures: 1,
        error: None,
        checked_at: now,
    };
    let _: (Uuid, ConnectionStatus, u32, Option<String>, DateTime<Utc>) =
        (connection_id, status, failures, error, checked_at);

    let ProviderInfo {
        key,
        name,
        kind,
        kinds,
        scope_sets,
    } = ProviderInfo {
        key: String::new(),
        name: String::new(),
        kind: ProviderType::Slack,
        kinds: vec![CredentialKind::OAuth],
        scope_sets: vec![ScopeSetInfo {
            name: "default".into(),
            description: String::new(),
            scopes: Vec::new(),
        }],
    };
    let _: (
        String,
        String,
        ProviderType,
        Vec<CredentialKind>,
        Vec<ScopeSetInfo>,
    ) = (key, name, kind, kinds, scope_sets);
    for (kind, name) in [
        (CredentialKind::OAuth, "oauth"),
        (CredentialKind::AppOnly, "app_only"),
        (CredentialKind::ServiceAccount, "service_account"),
        (CredentialKind::ApiKey, "api_key"),
    ] {
        assert_eq!(kind.as_str(), name);
    }
    assert_eq!(Owner::App.kind(), OwnerType::App);
    assert_eq!(Owner::Organization(Uuid::nil()).id(), Some(Uuid::nil()));
}

#[test]
fn events_constants_and_types_have_their_documented_names() {
    assert_eq!(IntegrationConnected::NAME, "integrations.connected");
    assert_eq!(IntegrationDisconnected::NAME, "integrations.disconnected");
    assert_eq!(ConnectionUnhealthy::NAME, "integrations.unhealthy");
    assert_eq!(ConnectionRestored::NAME, "integrations.restored");
    assert_eq!(INTEGRATIONS_MIGRATIONS, "integrations");
    assert_eq!(MANAGE_PERMISSION, "integrations.manage");
    assert_eq!(DEFAULT_SCOPE_SET, "default");
    assert_eq!(DefaultScopes::NAME, "default");
    assert_eq!(MicrosoftGraph::KEY, "microsoft");
    assert_eq!(GoogleWorkspace::KEY, "google");
    assert_eq!(Slack::KEY, "slack");

    let _ = ConnectionUnhealthy {
        connection_id: Uuid::nil(),
        owner: Owner::App,
        provider: String::new(),
        scope_set: String::new(),
        error: String::new(),
        failures: 0,
    };
    let _ = IntegrationConnected {
        connection_id: Uuid::nil(),
        owner: Owner::App,
        provider: String::new(),
        scope_set: String::new(),
        kind: CredentialKind::OAuth,
        connected_by: None,
    };

    // The typed clients and extractors exist under their documented names.
    type _Clients<'a> = (
        Option<IntegrationClient>,
        Option<GraphClient>,
        Option<GoogleClient>,
        Option<SlackClient>,
        Option<ApiRequest>,
        Option<Connection<MicrosoftGraph>>,
        Option<IntegrationOwner>,
        Option<IntegrationsController>,
        Option<IntegrationsError>,
        Option<&'a FakeServer>,
        Option<IntegrationConnection>,
    );
    let _ = IntegrationsModule::from_config;
    let _ = IntegrationsModule::for_root;
    let _ = IntegrationsModule::for_feature::<DefaultScopes>;
    fn _ext(m: &mut navree_kernel::ModuleBuilder) {
        m.integration_scope_set::<DefaultScopes>();
    }

    // The service's documented methods.
    let _ = Integrations::fake;
    let _ = Integrations::providers;
    let _ = Integrations::level;
    let _ = Integrations::is_fake;
    let _ = IntegrationsFake::api;
    let _ = IntegrationsFake::minted;
    let _ = IntegrationsFake::exchanged;
    let _ = IntegrationsFake::revoked;
    let _ = IntegrationsFake::on_code;
    let _ = IntegrationsFake::on_code_error;
    let _ = IntegrationsFake::on_token;
    let _ = IntegrationsFake::refuse_tokens;
    let _ = IntegrationsFake::fail_tokens;
    let _ = IntegrationsFake::restore_tokens;
    let _ = GraphClient::tenant_id;
    let _ = GoogleClient::domain;
    let _ = |google: &GoogleClient| google.as_user("admin@acme.test");
    let _ = SlackClient::team_id;
    let _ = IntegrationClient::connection;
    let _ = IntegrationClient::base_url;
    let _ = ServiceAccountKey::from_json;
    let _ = ServiceAccountKey::from_value;
    let _: Level = serde_json::from_value(json!("app")).unwrap();
}

/// The async service methods the page names, as callable items.
#[allow(dead_code)]
async fn service_methods(integrations: &Integrations, owner: Owner, id: Uuid) {
    let _ = integrations
        .connect_url(owner, "microsoft", ConnectOptions::default())
        .await;
    let _ = integrations
        .complete(owner, "microsoft", &ConnectCallback::default(), None)
        .await;
    let _ = integrations
        .connect_service_account(owner, "google", NewServiceAccount::new(sample_key(), None))
        .await;
    let _ = integrations
        .connect_api_key(owner, "edr", NewApiKey::new("k"))
        .await;
    let _ = integrations.connections(owner).await;
    let _ = integrations.connection(owner, "microsoft", "default").await;
    let _ = integrations.find(id).await;
    let _ = integrations.find_for(owner, id).await;
    let _ = integrations.all().await;
    let _ = integrations.disconnect(id).await;
    let _ = integrations.disconnect_for(owner, id).await;
    let _ = integrations.forget(owner).await;
    let _ = integrations.token(id).await;
    let _ = integrations.api_key(id).await;
    let _ = integrations.client(owner, "microsoft", "default").await;
    let _ = integrations.client_for::<MicrosoftGraph>(owner).await;
    let _ = integrations
        .client_in::<MicrosoftGraph, DefaultScopes>(owner)
        .await;
    let _ = integrations.check(id).await;
    let _ = integrations.check_due().await;
    let _ = integrations.check_all().await;
    if let Ok(client) = integrations.client(owner, "slack", "default").await {
        let _ = client.token().await;
        let request: ApiRequest = client
            .request(http::Method::GET, "/x")
            .header("x", "y")
            .query(&[("a", "b")])
            .json(&json!({}))
            .form(&[("a", "b")])
            .body("raw")
            .timeout(std::time::Duration::from_secs(1));
        let _ = request.send().await;
        let _ = client.get("/a").send_json::<serde_json::Value>().await;
        let _ = (
            client.post("/"),
            client.put("/"),
            client.patch("/"),
            client.delete("/"),
        );
        let slack = SlackClient::from(client);
        let _ = slack.call("auth.test", &json!({})).await;
        let graph = GraphClient::from(IntegrationClient::clone(&slack));
        let _ = graph.paginate::<serde_json::Value>("/users").await;
        let google = GoogleClient::from(IntegrationClient::clone(&slack));
        let _ = google
            .paginate::<serde_json::Value>("/users", "users")
            .await;
    }
}
