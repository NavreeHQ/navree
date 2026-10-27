//! `audit_logs` is a time-series table: a TimescaleDB hypertable on
//! `created_at` (see the facade's `time_series` suite), a regular table
//! with its `created_at` index here.

use navree_db::navree_schema::{IndexKind, Schema};
use navree_db::{Db, MIGRATIONS_TABLE};
use sea_orm::{ConnectionTrait, Statement};

use crate::support;

#[tokio::test]
async fn the_audit_log_is_declared_a_hypertable() {
    let app = support::builder().0.build().await;
    let db = app.get::<Db>().await;
    let applied: Vec<String> = db
        .conn()
        .query_all_raw(Statement::from_string(
            db.backend(),
            format!("SELECT name FROM {MIGRATIONS_TABLE} WHERE module = 'audit' ORDER BY name"),
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .collect();
    assert_eq!(
        applied,
        [
            "m20261025_000001_create_audit_logs",
            "m20261027_000001_add_impersonator_id_to_audit_logs",
            "m20261027_000002_convert_audit_logs_to_hypertable"
        ]
    );
    let schema = Schema::new(db.conn());
    assert!(!schema.is_hypertable("audit_logs").await.unwrap());
    assert!(
        schema
            .has_index("audit_logs", ["created_at"], Some(IndexKind::Index))
            .await
            .unwrap(),
        "the time index range queries and pruning use"
    );
    assert!(
        schema
            .has_index("audit_logs", ["id"], Some(IndexKind::Primary))
            .await
            .unwrap(),
        "only TimescaleDB needs the time column in the key"
    );
}
