//! Fleet's history is time series: `fleet_jobs` (finished attempts) and
//! `fleet_metrics` (snapshots) are TimescaleDB hypertables on their Unix
//! millisecond columns (see the facade's `time_series` suite), regular
//! tables with their time indexes here.

use navree_db::navree_schema::{IndexKind, Schema};
use navree_db::{Db, MIGRATIONS_TABLE};
use sea_orm::{ConnectionTrait, Statement};

use crate::support::builder;

#[tokio::test]
async fn the_job_history_and_metrics_are_declared_hypertables() {
    let app = builder().build().await;
    let db = app.get::<Db>().await;
    let applied: Vec<String> = db
        .conn()
        .query_all_raw(Statement::from_string(
            db.backend(),
            format!("SELECT name FROM {MIGRATIONS_TABLE} WHERE module = 'fleet' ORDER BY name"),
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .collect();
    assert_eq!(
        applied,
        [
            "m20261027_000001_create_fleet_tables",
            "m20261027_000002_convert_fleet_history_to_hypertables"
        ]
    );
    let schema = Schema::new(db.conn());
    for (table, time) in [("fleet_jobs", "finished_at"), ("fleet_metrics", "at")] {
        assert!(!schema.is_hypertable(table).await.unwrap());
        assert!(
            schema
                .has_index(table, [time], Some(IndexKind::Index))
                .await
                .unwrap(),
            "{table} is indexed by {time}"
        );
    }
}
