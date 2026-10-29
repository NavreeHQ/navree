//! Audit entries: connecting and disconnecting are recorded with the
//! provider, scope set, kind and owner, never a credential.

use std::sync::{Arc, Mutex, PoisonError};

use navree_contracts::{AuditEntry, AuditKind, AuditSink};
use navree_integrations::{ConnectCallback, ConnectOptions, FakeGrant, Owner};
use navree_kernel::{BoxError, BoxFuture};
use serde_json::json;

use crate::support::{ADMIN, harness};

#[derive(Default)]
struct Recorded(Mutex<Vec<AuditEntry>>);

impl Recorded {
    fn entries(&self) -> Vec<AuditEntry> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl AuditSink for Recorded {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(entry);
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn connecting_and_disconnecting_are_audited_without_secrets() {
    let audit = Arc::new(Recorded::default());
    let sink = audit.clone();
    let h = harness(move |b| b.override_value::<dyn AuditSink>(sink)).await;
    h.fake.on_code(
        "hubspot",
        "code",
        FakeGrant::new()
            .access_token("access-secret")
            .refresh_token("refresh-secret")
            .expires_in(600),
    );
    let integrations = h.integrations().await;
    let state = integrations
        .connect_url(Owner::App, "hubspot", ConnectOptions::default().user(ADMIN))
        .await
        .unwrap()
        .state;
    let connection = integrations
        .complete(
            Owner::App,
            "hubspot",
            &ConnectCallback::code(state, "code"),
            Some(ADMIN),
        )
        .await
        .unwrap()
        .connection;
    integrations.disconnect(connection.id).await.unwrap();

    let entries = audit.entries();
    let actions: Vec<&str> = entries.iter().map(|e| e.action.as_str()).collect();
    assert_eq!(
        actions,
        ["integration.connected", "integration.disconnected"]
    );
    let connected = &entries[0];
    assert_eq!(connected.kind, AuditKind::Custom);
    assert_eq!(connected.actor_id, Some(ADMIN));
    assert_eq!(
        connected.subject_type.as_deref(),
        Some("IntegrationConnection")
    );
    assert_eq!(
        connected.subject_id.as_deref(),
        Some(connection.id.to_string().as_str())
    );
    assert_eq!(
        connected.metadata,
        json!({
            "provider": "hubspot",
            "scope_set": "default",
            "kind": "oauth",
            "owner": { "type": "app" },
        })
    );
    let written = serde_json::to_string(&entries).unwrap();
    assert!(!written.contains("secret"), "{written}");
}

/// A connection row holds encrypted credentials and a token, so writing it
/// (a connect, a reconnect, a token refresh) records no change event for
/// the unit of work's observers: the audit entry names the connection
/// instead.
#[tokio::test]
async fn connection_writes_record_no_change_events() {
    let h = harness(|b| b).await;
    let integrations = h.integrations().await;
    let db = h.app.get::<navree_db::Db>().await;
    let uow = db.uow(Some(ADMIN), None).await;
    let connection = uow
        .scope(async {
            let mut id = None;
            for code in ["first", "second"] {
                h.fake.on_code(
                    "hubspot",
                    code,
                    FakeGrant::new()
                        .access_token("access-secret")
                        .refresh_token("refresh-secret")
                        .expires_in(600),
                );
                let state = integrations
                    .connect_url(Owner::App, "hubspot", ConnectOptions::default().user(ADMIN))
                    .await
                    .unwrap()
                    .state;
                id = Some(
                    integrations
                        .complete(
                            Owner::App,
                            "hubspot",
                            &ConnectCallback::code(state, code),
                            Some(ADMIN),
                        )
                        .await
                        .unwrap()
                        .connection
                        .id,
                );
            }
            id.unwrap()
        })
        .await;
    let changes = uow.changes();
    assert!(
        changes
            .iter()
            .all(|change| change.entity != "integration_connections"),
        "{changes:?}"
    );
    uow.commit().await.unwrap();
    assert!(integrations.find(connection).await.is_ok());
}
