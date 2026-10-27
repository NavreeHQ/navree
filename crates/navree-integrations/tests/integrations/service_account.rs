//! Google service accounts with domain-wide delegation, with the fake.

use navree_integrations::{
    CredentialKind, GoogleWorkspace, IntegrationsError, NewServiceAccount, Owner, ServiceAccountKey,
};
use serde_json::json;

use crate::support::{PEM, harness, service_account, with_workspaces};

#[tokio::test]
async fn a_service_account_connects_and_acts_as_the_admin() {
    let h = harness(with_workspaces).await;
    let acme = h.workspace(crate::support::OTHER, "acme").await;
    let owner = Owner::Workspace(acme.id);
    let integrations = h.integrations().await;

    let connection = integrations
        .connect_service_account(
            owner,
            "google",
            NewServiceAccount::new(service_account(), Some("admin@acme.test".into()))
                .scope_set("directory")
                .connected_by(crate::support::ADMIN),
        )
        .await
        .unwrap();
    assert_eq!(connection.kind, CredentialKind::ServiceAccount);
    assert_eq!(connection.provider_name, "Google Workspace");
    assert_eq!(connection.external_id.as_deref(), Some("acme.test"));
    assert_eq!(
        connection.display_name.as_deref(),
        Some("navree@acme-prod.iam.gserviceaccount.com")
    );
    assert_eq!(
        connection.scopes,
        vec![
            "https://www.googleapis.com/auth/admin.directory.user.readonly",
            "https://www.googleapis.com/auth/admin.directory.group.readonly",
        ]
    );
    // The key was tried before it was stored, acting as the admin.
    let minted = h.fake.minted();
    assert_eq!(minted.len(), 1);
    assert_eq!(minted[0].grant_type, "jwt_bearer");
    assert_eq!(minted[0].subject.as_deref(), Some("admin@acme.test"));
    assert_eq!(minted[0].client_id, None);
    // That first token is cached.
    integrations.token(connection.id).await.unwrap();
    assert_eq!(h.fake.minted().len(), 1);

    // Acting as another user mints (and keeps) a token for that user.
    let google = integrations
        .client_in::<GoogleWorkspace, crate::scopes::DirectoryRead>(owner)
        .await
        .unwrap();
    assert_eq!(google.domain(), Some("acme.test"));
    let bob = google.as_user("bob@acme.test");
    let token = bob.token().await.unwrap();
    assert_eq!(bob.token().await.unwrap().expose(), token.expose());
    let minted = h.fake.minted();
    assert_eq!(minted.len(), 2);
    assert_eq!(minted[1].subject.as_deref(), Some("bob@acme.test"));
}

#[tokio::test]
async fn a_key_the_provider_refuses_is_not_stored() {
    let h = harness(|b| b).await;
    h.fake.refuse_tokens("google", "unauthorized_client");
    let integrations = h.integrations().await;
    let error = integrations
        .connect_service_account(
            Owner::App,
            "google",
            NewServiceAccount::new(service_account(), Some("admin@acme.test".into())),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Google Workspace refused the connection: unauthorized_client"
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
async fn bad_keys_subjects_and_providers_are_refused() {
    let h = harness(|b| b).await;
    let integrations = h.integrations().await;
    // Not an email address.
    assert!(matches!(
        integrations
            .connect_service_account(
                Owner::App,
                "google",
                NewServiceAccount::new(service_account(), Some("admin".into())),
            )
            .await,
        Err(IntegrationsError::InvalidCredentials(_))
    ));
    // Microsoft doesn't take service-account keys.
    assert!(matches!(
        integrations
            .connect_service_account(
                Owner::App,
                "microsoft",
                NewServiceAccount::new(service_account(), None),
            )
            .await,
        Err(IntegrationsError::Unsupported { .. })
    ));
    // A scope set Google has no scopes for.
    assert!(matches!(
        integrations
            .connect_service_account(
                Owner::App,
                "google",
                NewServiceAccount::new(service_account(), None).scope_set("billing"),
            )
            .await,
        Err(IntegrationsError::UnknownScopeSet { .. })
    ));
    // A user's OAuth client file is not a service-account key.
    assert!(matches!(
        ServiceAccountKey::from_value(&json!({
            "type": "authorized_user",
            "client_email": "a@b.test",
            "private_key": PEM
        })),
        Err(IntegrationsError::InvalidCredentials(_))
    ));
    assert!(h.fake.minted().is_empty());
}
