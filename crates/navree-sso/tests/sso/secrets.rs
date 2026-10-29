//! Secrets and certificates stay out of change events. An observer
//! registered in the app sees every change event the unit of work records
//! while connections are created, changed and deleted and people sign in;
//! none carries a client secret (or its ciphertext), the identity
//! provider's certificate, or a password hash. The connection events
//! summarize the settings instead.

use std::sync::{Arc, Mutex};

use navree_auth::User;
use navree_db::{ChangeEvent, ChangeObserver, Db, DbError, ModuleBuilderDbExt, Uow};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_orm::Model as _;
use navree_sso::{
    ConnectionSettings, ConnectionUpdate, NewConnection, OidcSettings, Provisioning, SsoFake,
    SsoOwner, sso_connection,
};
use serde_json::{Value, json};

use crate::support;

/// Every change event, in the order they were committed.
#[derive(Default)]
struct Seen(Mutex<Vec<ChangeEvent>>);

struct Recorder {
    seen: Arc<Seen>,
}

impl Injectable for Recorder {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Seen>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Recorder {
                seen: r.get::<Seen>().await?,
            })
        })
    }
}

impl ChangeObserver for Recorder {
    fn observe_with<'a>(
        &'a self,
        _uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            self.seen.0.lock().unwrap().extend(changes.iter().cloned());
            Ok(())
        })
    }
}

struct RecorderModule;

impl Module for RecorderModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value(Seen::default()).change_observer::<Recorder>();
    }
}

/// The certificate in a metadata document.
fn certificate_of(metadata_xml: &str) -> String {
    let start = metadata_xml.find("<ds:X509Certificate>").unwrap() + "<ds:X509Certificate>".len();
    let end = metadata_xml.find("</ds:X509Certificate>").unwrap();
    metadata_xml[start..end].to_owned()
}

fn oidc(client_id: &str, client_secret: &str) -> ConnectionSettings {
    ConnectionSettings::Oidc(OidcSettings {
        issuer: "https://login.acme.test/v2.0".to_owned(),
        client_id: client_id.to_owned(),
        client_secret: Some(client_secret.to_owned()),
        scopes: Vec::new(),
    })
}

#[tokio::test]
async fn no_secret_or_certificate_reaches_a_change_event() {
    let world = support::world_with(support::builder().module(RecorderModule)).await;
    let sso = world.sso().await;
    let db = world.app.get::<Db>().await;
    let fake = SsoFake::new();
    let certificate = certificate_of(&fake.metadata_xml());
    assert!(certificate.len() > 100);
    let mut secrets = vec![certificate];

    // An OpenID Connect connection with a client secret, a change of its
    // client id, a new secret, and its removal.
    let created = {
        let sso = sso.clone();
        world
            .write(|uow| async move {
                sso.create_connection(
                    &uow,
                    NewConnection::oidc(
                        SsoOwner::App,
                        "Acme Entra",
                        "https://login.acme.test/v2.0",
                        "c1",
                        "first-client-secret",
                    ),
                )
                .await
            })
            .await
            .expect("the connection is created")
    };
    let stored = sso_connection::Model::find(&db, created.id)
        .await
        .unwrap()
        .expect("the connection row");
    let stored_settings: Value = serde_json::from_str(&stored.settings).unwrap();
    secrets.push("first-client-secret".to_owned());
    secrets.push(
        stored_settings["client_secret"]
            .as_str()
            .expect("the stored secret")
            .to_owned(),
    );
    {
        let sso = sso.clone();
        let slug = created.slug.clone();
        world
            .write(|uow| async move {
                sso.update_connection(
                    &uow,
                    &slug,
                    ConnectionUpdate {
                        settings: Some(oidc("c2", "second-client-secret")),
                        ..ConnectionUpdate::default()
                    },
                )
                .await
            })
            .await
            .expect("the connection changes");
    }
    secrets.push("second-client-secret".to_owned());
    let renewed = sso_connection::Model::find(&db, created.id)
        .await
        .unwrap()
        .expect("the connection row");
    let renewed_settings: Value = serde_json::from_str(&renewed.settings).unwrap();
    secrets.push(
        renewed_settings["client_secret"]
            .as_str()
            .expect("the stored secret")
            .to_owned(),
    );
    {
        let sso = sso.clone();
        let slug = created.slug.clone();
        world
            .write(|uow| async move { sso.delete_connection(&uow, &slug).await })
            .await
            .expect("the connection is deleted");
    }

    // A SAML connection with the identity provider's certificate, a
    // verified domain, and a sign-in that creates a user.
    let saml = world
        .saml_connection(&fake, SsoOwner::App, "acme", Provisioning::default())
        .await;
    world.verified_domain(&saml, "acme.test").await;
    world
        .saml_sign_in(&fake, "acme", "alice@acme.test", &[])
        .await
        .assert_status(200);
    let users = User::query().get(&db).await.unwrap();
    assert_eq!(users.len(), 1);
    secrets.push(users[0].password_hash.clone());

    let seen = world.app.get::<Seen>().await;
    let events = seen.0.lock().unwrap().clone();
    let entities: Vec<&str> = events.iter().map(|event| event.entity.as_str()).collect();
    for expected in ["sso_connections", "sso_domains", "sso_identities", "users"] {
        assert!(entities.contains(&expected), "{expected} in {entities:?}");
    }
    for event in &events {
        let text = serde_json::to_string(&json!({ "old": event.old, "new": event.new })).unwrap();
        for secret in &secrets {
            assert!(
                !text.contains(secret.as_str()),
                "a secret is in a change event of {}: {text}",
                event.entity
            );
        }
    }

    // The connection events summarize the settings instead.
    let connection_events: Vec<&ChangeEvent> = events
        .iter()
        .filter(|event| event.entity == "sso_connections")
        .collect();
    let first = connection_events[0].new.as_ref().expect("a created row");
    assert_eq!(first["settings"]["client_id"], "c1");
    assert_eq!(first["settings"]["has_client_secret"], true);
    let changed = connection_events
        .iter()
        .find(|event| event.old.is_some() && event.new.is_some())
        .expect("an updated event");
    assert_eq!(changed.old.as_ref().unwrap()["settings"]["client_id"], "c1");
    assert_eq!(changed.new.as_ref().unwrap()["settings"]["client_id"], "c2");
    let saml_event = connection_events
        .iter()
        .find(|event| event.new.as_ref().is_some_and(|new| new["slug"] == "acme"))
        .expect("the SAML connection's event");
    let summary = &saml_event.new.as_ref().unwrap()["settings"];
    assert_eq!(summary["has_metadata_xml"], true);
    assert!(
        summary["metadata_xml_sha256"]
            .as_str()
            .is_some_and(|d| d.len() == 64)
    );
    assert!(summary.get("metadata_xml").is_none());
}
