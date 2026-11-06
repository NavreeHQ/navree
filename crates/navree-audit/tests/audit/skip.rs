//! Skipped fields and entities never reach `audit_logs`.

use navree_audit::AuditLog;
use navree_contracts::{AuditEntry, AuditKind};
use navree_db::{Db, Executor, Statement};
use navree_orm::Model as _;
use serde_json::json;
use uuid::Uuid;

use crate::support::{self, Note, User, audit_rows, new_note};

fn new_user() -> User {
    User {
        id: Uuid::now_v7(),
        email: "anu@example.com".into(),
        password_hash: "$argon2id$hash-value".into(),
        remember_token: Some("remember-me-value".into()),
        ssn: Some("123-45-6789".into()),
        settings: json!({"theme": "dark", "token": "nested-token-value"}),
    }
}

/// Every stored audit column as text, to search for leaked values.
async fn raw_audit_text(db: &Db) -> String {
    let rows = db
        .fetch_all(&Statement::text(
            "SELECT action, old_values, new_values, metadata FROM audit_logs",
        ))
        .await
        .unwrap();
    rows.iter()
        .map(|row| serde_json::Value::from(row.to_json().unwrap()).to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn skipped_fields_never_stored() {
    let app = support::builder()
        .0
        .config("audit.skip_fields", "ssn, notes.body")
        .build()
        .await;
    let db = app.get::<Db>().await;

    let uow = db.uow(None, None).await;
    let mut created = User::create(&uow, new_user()).await.unwrap();
    created
        .update(&uow, |user| {
            user.password_hash = "$argon2id$new-hash-value".into();
            user.remember_token = None;
        })
        .await
        .unwrap();
    let mut found = User::find_or_fail(&uow, created.id).await.unwrap();
    found
        .update(&uow, |user| user.email = "anu@lakhotia.dev".into())
        .await
        .unwrap();
    User::find_or_fail(&uow, created.id)
        .await
        .unwrap()
        .delete(&uow)
        .await
        .unwrap();
    Note::create(&uow, new_note("a")).await.unwrap();
    uow.commit().await.unwrap();

    let log = app.get::<AuditLog>().await;
    log.record(
        AuditEntry::new(AuditKind::Security, "auth.failed")
            .metadata(json!({"email": "anu@example.com", "password": "typed-password-value"})),
    )
    .await
    .unwrap();
    log.flush().await;

    let text = raw_audit_text(&db).await;
    for secret in [
        "hash-value",
        "remember-me-value",
        "nested-token-value",
        "123-45-6789",
        "typed-password-value",
        "body of a",
    ] {
        assert!(!text.contains(secret), "`{secret}` leaked into:\n{text}");
    }

    let rows = audit_rows(&app).await;
    let actions: Vec<_> = rows.iter().map(|r| r.action.as_str()).collect();
    assert_eq!(
        actions,
        [
            "user.created",
            "user.updated",
            "user.updated",
            "user.deleted",
            "note.created",
            "auth.failed"
        ]
    );
    assert_eq!(
        rows[0].new_values,
        Some(json!({"id": created.id, "email": "anu@example.com", "settings": {"theme": "dark"}}))
    );
    // Only secret fields changed: the update is recorded without values.
    assert_eq!(rows[1].old_values, Some(json!({})));
    assert_eq!(rows[1].new_values, Some(json!({})));
    assert_eq!(
        rows[2].old_values,
        Some(json!({"email": "anu@example.com"}))
    );
    assert_eq!(
        rows[2].new_values,
        Some(json!({"email": "anu@lakhotia.dev"}))
    );
    assert_eq!(
        rows[4].new_values.as_ref().and_then(|v| v.get("title")),
        Some(&json!("a")),
        "`notes.body` drops only that field"
    );
    assert_eq!(rows[5].metadata, json!({"email": "anu@example.com"}));
}

#[tokio::test]
async fn skipped_entities_are_not_audited() {
    let app = support::builder()
        .0
        .config("audit.skip_entities", json!(["Note"]))
        .build()
        .await;
    let db = app.get::<Db>().await;

    let uow = db.uow(None, None).await;
    Note::create(&uow, new_note("a")).await.unwrap();
    User::create(&uow, new_user()).await.unwrap();
    uow.commit().await.unwrap();

    let actions: Vec<_> = audit_rows(&app)
        .await
        .into_iter()
        .map(|r| r.action)
        .collect();
    assert_eq!(actions, ["user.created"]);
}
