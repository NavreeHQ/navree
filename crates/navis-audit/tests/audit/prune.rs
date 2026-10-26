//! `audit:prune [--days N]` and `AuditLog::prune`.

use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use navis_contracts::{AuditEntry, AuditKind, Clock, FixedClock};
use navis_db::{Db, Repo};
use navis_testing::TestApp;

use crate::support::{self, audit_log, audit_rows, new_note, note};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// An app on a fixed clock with one entry 400 days old, one 200 days old
/// and one from today.
async fn aged(config: &[(&str, serde_json::Value)]) -> (TestApp, Arc<FixedClock>) {
    let clock = Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
    ));
    let mut builder = support::builder()
        .0
        .override_value::<dyn Clock>(clock.clone());
    for (key, value) in config {
        builder = builder.config(key, value.clone());
    }
    let app = builder.build().await;
    let log = audit_log(&app).await;

    log.record(AuditEntry::new(AuditKind::Auth, "auth.login.old"))
        .await
        .unwrap();
    log.flush().await;
    clock.advance(200 * DAY);
    // A data change through a unit of work is stamped by the same clock.
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    Repo::<note::Entity>::insert(&uow, new_note("middle"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    clock.advance(200 * DAY);
    log.record(AuditEntry::new(AuditKind::Auth, "auth.login.new"))
        .await
        .unwrap();
    log.flush().await;
    (app, clock)
}

async fn actions(app: &TestApp) -> Vec<String> {
    audit_rows(app)
        .await
        .into_iter()
        .map(|r| r.action)
        .collect()
}

#[tokio::test]
async fn prune_deletes_older_than_retention() {
    let (app, _clock) = aged(&[]).await;

    app.run_command("audit:prune", &[]).await.unwrap();
    assert_eq!(
        actions(&app).await,
        ["note.created", "auth.login.new"],
        "the default retention is 365 days"
    );

    app.run_command("audit:prune", &["--days", "100"])
        .await
        .unwrap();
    assert_eq!(actions(&app).await, ["auth.login.new"]);
}

#[tokio::test]
async fn retention_comes_from_configuration() {
    let (app, _clock) = aged(&[("audit.retention_days", serde_json::json!("150"))]).await;
    app.run_command("audit:prune", &[]).await.unwrap();
    assert_eq!(actions(&app).await, ["auth.login.new"]);
}

#[tokio::test]
async fn zero_retention_keeps_everything() {
    let (app, _clock) = aged(&[("audit.retention_days", serde_json::json!(0))]).await;
    app.run_command("audit:prune", &[]).await.unwrap();
    assert_eq!(actions(&app).await.len(), 3);
}

#[tokio::test]
async fn prune_returns_the_number_deleted() {
    let (app, clock) = aged(&[]).await;
    let log = audit_log(&app).await;
    assert_eq!(log.prune(365).await.unwrap(), 1);
    assert_eq!(log.prune_before(clock.now()).await.unwrap(), 1);
    assert_eq!(actions(&app).await, ["auth.login.new"]);
}

#[tokio::test]
async fn bad_days_argument_fails() {
    let (app, _clock) = aged(&[]).await;
    let error = app
        .run_command("audit:prune", &["--days", "soon"])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("--days"), "{error}");
    assert_eq!(actions(&app).await.len(), 3);
}
