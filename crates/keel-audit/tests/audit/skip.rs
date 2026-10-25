//! Skipped fields and entities never reach `audit_logs`.

use keel_audit::AuditLog;
use keel_contracts::{AuditEntry, AuditKind};
use keel_db::{Db, Repo};
use sea_orm::{ActiveValue::Set, ConnectionTrait, IntoActiveModel, Statement};
use serde_json::json;
use uuid::Uuid;

use crate::support::{self, audit_rows, new_note, note, user};

type Users = Repo<user::Entity>;
type Notes = Repo<note::Entity>;

fn new_user() -> user::ActiveModel {
    user::ActiveModel {
        id: Set(Uuid::now_v7()),
        email: Set("ada@example.com".into()),
        password_hash: Set("$argon2id$hash-value".into()),
        remember_token: Set(Some("remember-me-value".into())),
        ssn: Set(Some("123-45-6789".into())),
        settings: Set(json!({"theme": "dark", "token": "nested-token-value"})),
    }
}

/// Every stored audit column as text, to search for leaked values.
async fn raw_audit_text(db: &Db) -> String {
    let rows = db
        .conn()
        .query_all_raw(Statement::from_string(
            db.backend(),
            "SELECT action, coalesce(old_values, '') AS o, coalesce(new_values, '') AS n, metadata AS m FROM audit_logs",
        ))
        .await
        .unwrap();
    rows.iter()
        .map(|row| {
            ["action", "o", "n", "m"]
                .iter()
                .map(|c| row.try_get::<String>("", c).unwrap())
                .collect::<Vec<_>>()
                .join(" ")
        })
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
    let created = Users::insert(&uow, new_user()).await.unwrap();
    let mut changed = created.clone().into_active_model();
    changed.password_hash = Set("$argon2id$new-hash-value".into());
    changed.remember_token = Set(None);
    Users::update(&uow, changed).await.unwrap();
    let mut changed = Users::find_or_fail(&uow, created.id)
        .await
        .unwrap()
        .into_active_model();
    changed.email = Set("ada@lovelace.dev".into());
    Users::update(&uow, changed).await.unwrap();
    Users::delete(&uow, Users::find_or_fail(&uow, created.id).await.unwrap())
        .await
        .unwrap();
    Notes::insert(&uow, new_note("a")).await.unwrap();
    uow.commit().await.unwrap();

    let log = app.get::<AuditLog>().await;
    log.record(
        AuditEntry::new(AuditKind::Security, "auth.failed")
            .metadata(json!({"email": "ada@example.com", "password": "typed-password-value"})),
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
        Some(json!({"id": created.id, "email": "ada@example.com", "settings": {"theme": "dark"}}))
    );
    // Only secret fields changed: the update is recorded without values.
    assert_eq!(rows[1].old_values, Some(json!({})));
    assert_eq!(rows[1].new_values, Some(json!({})));
    assert_eq!(
        rows[2].old_values,
        Some(json!({"email": "ada@example.com"}))
    );
    assert_eq!(
        rows[2].new_values,
        Some(json!({"email": "ada@lovelace.dev"}))
    );
    assert_eq!(
        rows[4].new_values.as_ref().and_then(|v| v.get("title")),
        Some(&json!("a")),
        "`notes.body` drops only that field"
    );
    assert_eq!(rows[5].metadata, json!({"email": "ada@example.com"}));
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
    Notes::insert(&uow, new_note("a")).await.unwrap();
    Users::insert(&uow, new_user()).await.unwrap();
    uow.commit().await.unwrap();

    let actions: Vec<_> = audit_rows(&app)
        .await
        .into_iter()
        .map(|r| r.action)
        .collect();
    assert_eq!(actions, ["user.created"]);
}
