//! Fleet's history is time series: `fleet_jobs` (finished attempts) and
//! `fleet_metrics` (snapshots) are TimescaleDB hypertables on their Unix
//! millisecond columns (see the facade's `time_series` suite), regular
//! tables with their time indexes elsewhere.

use navree_db::navree_schema::{IndexKind, Schema};
use navree_db::{Db, Executor as _, MIGRATIONS_TABLE, Statement};

use crate::support::builder;

#[tokio::test]
async fn the_job_history_and_metrics_are_declared_hypertables() {
    let app = builder().build().await;
    let db = app.get::<Db>().await;
    let applied: Vec<String> = db
        .fetch_all(&Statement::text(format!(
            "SELECT name FROM {MIGRATIONS_TABLE} WHERE module = 'fleet' ORDER BY name"
        )))
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String>("name").unwrap())
        .collect();
    assert_eq!(
        applied,
        [
            "m20261027_000001_create_fleet_tables",
            "m20261027_000002_convert_fleet_history_to_hypertables"
        ]
    );
    let schema = Schema::new(&db);
    let timescale = db.has_timescaledb().await.unwrap();
    for (table, time) in [("fleet_jobs", "finished_at"), ("fleet_metrics", "at")] {
        assert_eq!(schema.is_hypertable(table).await.unwrap(), timescale);
        if !timescale {
            assert!(
                schema
                    .has_index(table, [time], Some(IndexKind::Index))
                    .await
                    .unwrap(),
                "{table} is indexed by {time}"
            );
        }
    }
}
