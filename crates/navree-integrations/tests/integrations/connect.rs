//! Consent flows with the fake providers: Microsoft's admin consent,
//! Slack installs, OAuth 2.0 with PKCE, the sealed state and scope sets.

use std::time::Duration;

use navree_integrations::{
    ConnectCallback, ConnectOptions, ConnectionStatus, CredentialKind, FakeGrant,
    IntegrationConnected, IntegrationsError, Owner, OwnerType,
};
use uuid::Uuid;

use crate::support::{ADMIN, OTHER, REDIRECT_BASE, harness, query_param, with_workspaces};

const TENANT: &str = "9188040d-6c67-4c5b-b112-36a304b66dad";

#[tokio::test]
async fn microsoft_admin_consent_connects_a_workspace_to_its_tenant() {
    let h = harness(with_workspaces).await;
    let acme = h.workspace(OTHER, "acme").await;
    let owner = Owner::Workspace(acme.id);
    let integrations = h.integrations().await;

    let redirect = integrations
        .connect_url(
            owner,
            "microsoft",
            ConnectOptions::default()
                .return_to("/settings/integrations")
                .user(ADMIN),
        )
        .await
        .unwrap();
    assert!(
        redirect
            .url
            .starts_with("https://login.microsoftonline.com/organizations/v2.0/adminconsent?"),
        "{}",
        redirect.url
    );
    assert!(redirect.state.starts_with("nvi1."));
    assert_eq!(
        query_param(&redirect.url, "client_id").as_deref(),
        Some("0f6c1d2e-8a3b-4c5d-9e7f-1a2b3c4d5e6f")
    );
    assert_eq!(
        query_param(&redirect.url, "redirect_uri"),
        Some(format!("{REDIRECT_BASE}/microsoft"))
    );
    assert_eq!(
        query_param(&redirect.url, "scope").as_deref(),
        Some("https://graph.microsoft.com/.default")
    );
    assert_eq!(
        query_param(&redirect.url, "state"),
        Some(redirect.state.clone())
    );
    assert_eq!(query_param(&redirect.url, "code_challenge"), None);

    let completed = integrations
        .complete(
            owner,
            "microsoft",
            &ConnectCallback::admin_consent(&redirect.state, TENANT),
            Some(ADMIN),
        )
        .await
        .unwrap();
    assert_eq!(
        completed.return_to.as_deref(),
        Some("/settings/integrations")
    );
    let connection = completed.connection;
    assert_eq!(connection.owner_type, OwnerType::Workspace);
    assert_eq!(connection.owner_id, Some(acme.id));
    assert_eq!(connection.owner(), owner);
    assert_eq!(connection.provider, "microsoft");
    assert_eq!(connection.provider_name, "Microsoft 365");
    assert_eq!(connection.scope_set, "default");
    assert_eq!(connection.kind, CredentialKind::AppOnly);
    assert_eq!(connection.external_id.as_deref(), Some(TENANT));
    assert_eq!(connection.status, ConnectionStatus::Healthy);
    assert_eq!(connection.connected_by, Some(ADMIN));

    let connected = h.inbox.of::<IntegrationConnected>();
    assert_eq!(connected.len(), 1);
    assert_eq!(connected[0].connection_id, connection.id);
    assert_eq!(connected[0].owner, owner);
    assert_eq!(connected[0].kind, CredentialKind::AppOnly);

    // App-only tokens are minted for the tenant with the app's client.
    let token = integrations.token(connection.id).await.unwrap();
    assert!(token.expose().starts_with("fake-microsoft-"));
    let minted = h.fake.minted();
    assert_eq!(minted.len(), 1);
    assert_eq!(minted[0].grant_type, "client_credentials");
    assert_eq!(minted[0].subject.as_deref(), Some(TENANT));
    assert_eq!(
        minted[0].client_id.as_deref(),
        Some("0f6c1d2e-8a3b-4c5d-9e7f-1a2b3c4d5e6f")
    );
    // ...and cached until they expire.
    assert_eq!(
        integrations.token(connection.id).await.unwrap().expose(),
        token.expose()
    );
    assert_eq!(h.fake.minted().len(), 1);

    // The state works once.
    let again = integrations
        .complete(
            owner,
            "microsoft",
            &ConnectCallback::admin_consent(&redirect.state, TENANT),
            Some(ADMIN),
        )
        .await;
    assert!(
        matches!(again, Err(IntegrationsError::InvalidState)),
        "{again:?}"
    );
}

#[tokio::test]
async fn the_state_is_bound_to_its_provider_owner_admin_and_lifetime() {
    let h = harness(|b| b).await;
    let integrations = h.integrations().await;
    let start = |owner| {
        let integrations = integrations.clone();
        async move {
            integrations
                .connect_url(owner, "microsoft", ConnectOptions::default().user(ADMIN))
                .await
                .unwrap()
                .state
        }
    };
    let refused = |result: Result<_, IntegrationsError>| {
        assert!(
            matches!(result, Err(IntegrationsError::InvalidState)),
            "{result:?}"
        );
    };

    // Another owner.
    let state = start(Owner::App).await;
    refused(
        integrations
            .complete(
                Owner::Workspace(Uuid::from_u128(9)),
                "microsoft",
                &ConnectCallback::admin_consent(&state, TENANT),
                Some(ADMIN),
            )
            .await,
    );
    // Another admin.
    let state = start(Owner::App).await;
    refused(
        integrations
            .complete(
                Owner::App,
                "microsoft",
                &ConnectCallback::admin_consent(&state, TENANT),
                Some(OTHER),
            )
            .await,
    );
    // Another provider.
    let state = start(Owner::App).await;
    refused(
        integrations
            .complete(
                Owner::App,
                "slack",
                &ConnectCallback::code(&state, "code"),
                Some(ADMIN),
            )
            .await,
    );
    // Tampered.
    let mut state = start(Owner::App).await;
    let last = state.pop().unwrap();
    state.push(if last == 'A' { 'B' } else { 'A' });
    refused(
        integrations
            .complete(
                Owner::App,
                "microsoft",
                &ConnectCallback::admin_consent(&state, TENANT),
                Some(ADMIN),
            )
            .await,
    );
    // Expired (10 minutes by default).
    let state = start(Owner::App).await;
    h.advance(Duration::from_secs(601));
    refused(
        integrations
            .complete(
                Owner::App,
                "microsoft",
                &ConnectCallback::admin_consent(&state, TENANT),
                Some(ADMIN),
            )
            .await,
    );
    assert!(
        integrations
            .connections(Owner::App)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_declined_consent_is_refused() {
    let h = harness(|b| b).await;
    let integrations = h.integrations().await;
    let state = integrations
        .connect_url(Owner::App, "microsoft", ConnectOptions::default())
        .await
        .unwrap()
        .state;
    let declined = ConnectCallback {
        state,
        error: Some("access_denied".into()),
        error_description: Some("AADSTS65004: User declined to consent".into()),
        ..ConnectCallback::default()
    };
    let error = integrations
        .complete(Owner::App, "microsoft", &declined, None)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Microsoft 365 consent was not granted: access_denied"
    );

    let state = integrations
        .connect_url(Owner::App, "microsoft", ConnectOptions::default())
        .await
        .unwrap()
        .state;
    let not_granted = ConnectCallback {
        state,
        tenant: Some(TENANT.into()),
        admin_consent: Some("False".into()),
        ..ConnectCallback::default()
    };
    assert!(matches!(
        integrations
            .complete(Owner::App, "microsoft", &not_granted, None)
            .await,
        Err(IntegrationsError::ConsentDenied { .. })
    ));
}

#[tokio::test]
async fn a_slack_install_stores_the_team_and_its_bot_token() {
    let h = harness(|b| b).await;
    h.fake.on_code(
        "slack",
        "slack-code",
        FakeGrant::new()
            .access_token("xoxb-17653672481-19874698323")
            .never_expires()
            .scopes(["chat:write", "users:read"])
            .external_id("T0123ABCD")
            .display_name("Acme"),
    );
    let integrations = h.integrations().await;
    let redirect = integrations
        .connect_url(Owner::App, "slack", ConnectOptions::default())
        .await
        .unwrap();
    assert!(
        redirect
            .url
            .starts_with("https://slack.com/oauth/v2/authorize?")
    );
    assert_eq!(
        query_param(&redirect.url, "scope").as_deref(),
        Some("chat:write,users:read")
    );
    // Slack's v2 installs take no PKCE challenge.
    assert_eq!(query_param(&redirect.url, "code_challenge"), None);

    let connection = integrations
        .complete(
            Owner::App,
            "slack",
            &ConnectCallback::code(&redirect.state, "slack-code"),
            None,
        )
        .await
        .unwrap()
        .connection;
    assert_eq!(connection.kind, CredentialKind::OAuth);
    assert_eq!(connection.external_id.as_deref(), Some("T0123ABCD"));
    assert_eq!(connection.display_name.as_deref(), Some("Acme"));
    assert_eq!(connection.scopes, vec!["chat:write", "users:read"]);
    assert_eq!(
        h.fake.exchanged(),
        vec![("slack".to_owned(), "slack-code".to_owned())]
    );
    // A token without an expiry is used as it is, for as long as it works.
    h.advance(Duration::from_secs(30 * 24 * 3600));
    assert_eq!(
        integrations.token(connection.id).await.unwrap().expose(),
        "xoxb-17653672481-19874698323"
    );
    assert!(h.fake.minted().is_empty());

    // A code the provider doesn't know is refused.
    let state = integrations
        .connect_url(Owner::App, "slack", ConnectOptions::default())
        .await
        .unwrap()
        .state;
    let error = integrations
        .complete(
            Owner::App,
            "slack",
            &ConnectCallback::code(state, "nope"),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Slack refused the connection: invalid_grant"
    );
}

#[tokio::test]
async fn oauth2_providers_use_pkce_and_keep_rotated_refresh_tokens() {
    let h = harness(|b| b).await;
    h.fake.on_code(
        "hubspot",
        "hub-code",
        FakeGrant::new()
            .access_token("hub-access-1")
            .refresh_token("hub-refresh-1")
            .expires_in(1800),
    );
    let integrations = h.integrations().await;
    let redirect = integrations
        .connect_url(Owner::App, "hubspot", ConnectOptions::default())
        .await
        .unwrap();
    assert!(
        redirect
            .url
            .starts_with("https://app.hubspot.com/oauth/authorize?")
    );
    assert_eq!(
        query_param(&redirect.url, "response_type").as_deref(),
        Some("code")
    );
    assert_eq!(
        query_param(&redirect.url, "code_challenge_method").as_deref(),
        Some("S256")
    );
    assert_eq!(
        query_param(&redirect.url, "scope").as_deref(),
        Some("crm.objects.contacts.read")
    );
    let connection = integrations
        .complete(
            Owner::App,
            "hubspot",
            &ConnectCallback::code(&redirect.state, "hub-code"),
            None,
        )
        .await
        .unwrap()
        .connection;
    assert_eq!(connection.provider_name, "HubSpot");
    assert_eq!(
        integrations.token(connection.id).await.unwrap().expose(),
        "hub-access-1"
    );

    // Half an hour later the token has expired: one refresh, which rotates
    // the refresh token.
    h.fake.on_token(
        "hubspot",
        FakeGrant::new()
            .access_token("hub-access-2")
            .refresh_token("hub-refresh-2")
            .expires_in(1800),
    );
    h.advance(Duration::from_secs(1800));
    assert_eq!(
        integrations.token(connection.id).await.unwrap().expose(),
        "hub-access-2"
    );
    let minted = h.fake.minted();
    assert_eq!(minted.len(), 1);
    assert_eq!(minted[0].grant_type, "refresh_token");

    // Disconnecting revokes the newest refresh token.
    integrations.disconnect(connection.id).await.unwrap();
    assert_eq!(
        h.fake.revoked(),
        vec![("hubspot".to_owned(), "hub-refresh-2".to_owned())]
    );
    assert!(matches!(
        integrations.find(connection.id).await,
        Err(IntegrationsError::ConnectionNotFound)
    ));
    assert_eq!(
        h.inbox.names(),
        vec!["integrations.connected", "integrations.disconnected"]
    );
}

#[tokio::test]
async fn connecting_again_replaces_the_credentials() {
    let h = harness(|b| b).await;
    let first = h.connect_microsoft(Owner::App, TENANT).await;
    let second = h
        .connect_microsoft(Owner::App, "contoso.onmicrosoft.com")
        .await;
    assert_eq!(first.id, second.id);
    assert_eq!(
        second.external_id.as_deref(),
        Some("contoso.onmicrosoft.com")
    );
    let all = h
        .integrations()
        .await
        .connections(Owner::App)
        .await
        .unwrap();
    assert_eq!(all.len(), 1);
}

#[tokio::test]
async fn scope_sets_are_connected_apart_with_their_own_app_registration() {
    let h = harness(|b| b).await;
    let integrations = h.integrations().await;

    let providers = integrations.providers().unwrap();
    let microsoft = providers.iter().find(|p| p.key == "microsoft").unwrap();
    let names: Vec<&str> = microsoft
        .scope_sets
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, vec!["default", "directory", "mail_send"]);
    assert_eq!(microsoft.kinds, vec![CredentialKind::AppOnly]);
    let slack = providers.iter().find(|p| p.key == "slack").unwrap();
    assert_eq!(slack.scope_sets.len(), 1);

    let directory = integrations
        .connect_url(
            Owner::App,
            "microsoft",
            ConnectOptions::default().scope_set("directory"),
        )
        .await
        .unwrap();
    assert_eq!(
        query_param(&directory.url, "scope").as_deref(),
        Some(
            "https://graph.microsoft.com/User.Read.All https://graph.microsoft.com/Group.Read.All"
        )
    );
    let mail = integrations
        .connect_url(
            Owner::App,
            "microsoft",
            ConnectOptions::default().scope_set("mail_send"),
        )
        .await
        .unwrap();
    assert_eq!(
        query_param(&mail.url, "client_id").as_deref(),
        Some("ms-mail-app")
    );

    for state in [directory.state, mail.state] {
        integrations
            .complete(
                Owner::App,
                "microsoft",
                &ConnectCallback::admin_consent(state, TENANT),
                None,
            )
            .await
            .unwrap();
    }
    let connections = integrations.connections(Owner::App).await.unwrap();
    let sets: Vec<&str> = connections.iter().map(|c| c.scope_set.as_str()).collect();
    assert_eq!(sets, vec!["directory", "mail_send"]);

    // Mail send tokens come from its own registration.
    let mail_connection = integrations
        .connection(Owner::App, "microsoft", "mail_send")
        .await
        .unwrap();
    integrations.token(mail_connection.id).await.unwrap();
    assert_eq!(h.fake.minted()[0].client_id.as_deref(), Some("ms-mail-app"));
    // The default scope set was never connected.
    assert!(matches!(
        integrations
            .connection(Owner::App, "microsoft", "default")
            .await,
        Err(IntegrationsError::NotConnected { .. })
    ));

    // Unknown scope sets, and sets a provider doesn't support.
    for (provider, set) in [("microsoft", "billing"), ("slack", "directory")] {
        let error = integrations
            .connect_url(
                Owner::App,
                provider,
                ConnectOptions::default().scope_set(set),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(error, IntegrationsError::UnknownScopeSet { .. }),
            "{error:?}"
        );
    }
}

#[tokio::test]
async fn unknown_providers_kinds_and_return_paths_are_refused() {
    let h = harness(|b| b).await;
    let integrations = h.integrations().await;
    assert!(matches!(
        integrations
            .connect_url(Owner::App, "zoom", ConnectOptions::default())
            .await,
        Err(IntegrationsError::UnknownProvider(_))
    ));
    // Google connects with service accounts unless a client is configured;
    // API-key providers never have a consent screen.
    for provider in ["google", "edr"] {
        let error = integrations
            .connect_url(Owner::App, provider, ConnectOptions::default())
            .await
            .unwrap_err();
        assert!(
            matches!(error, IntegrationsError::Unsupported { .. }),
            "{error:?}"
        );
    }
    assert!(matches!(
        integrations
            .connect_url(
                Owner::App,
                "slack",
                ConnectOptions::default().return_to("https://evil.test")
            )
            .await,
        Err(IntegrationsError::InvalidReturnTo)
    ));
}
