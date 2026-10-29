//! `audit_logs` is a time-series table: a TimescaleDB hypertable on
//! `created_at` (see the facade's `time_series` suite) where the server
//! has TimescaleDB, a regular table with its `created_at` index elsewhere.

use navree_db::navree_schema::{IndexKind, Schema};
use navree_db::{Db, Executor, MIGRATIONS_TABLE, Statement};

use crate::support;

#[tokio::test]
async fn the_audit_log_is_declared_a_hypertable() {
    let app = support::builder().0.build().await;
    let db = app.get::<Db>().await;
    let applied: Vec<String> = db
        .fetch_all(&Statement::text(format!(
            "SELECT name FROM {MIGRATIONS_TABLE} WHERE module = 'audit' ORDER BY name"
        )))
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String>("name").unwrap())
        .collect();
    assert_eq!(
        applied,
        [
            "m20261025_000001_create_audit_logs",
            "m20261027_000001_add_impersonator_id_to_audit_logs",
            "m20261027_000002_convert_audit_logs_to_hypertable"
        ]
    );
    let schema = Schema::new(&*db);
    let timescale = schema.target().await.unwrap().timescaledb().is_some();
    assert_eq!(schema.is_hypertable("audit_logs").await.unwrap(), timescale);
    assert!(
        schema
            .has_index("audit_logs", ["created_at"], Some(IndexKind::Index))
            .await
            .unwrap(),
        "the time index range queries and pruning use"
    );
    let key: &[&str] = if timescale {
        &["id", "created_at"]
    } else {
        &["id"]
    };
    assert!(
        schema
            .has_index("audit_logs", key.to_vec(), Some(IndexKind::Primary))
            .await
            .unwrap(),
        "only TimescaleDB needs the time column in the key"
    );
}
