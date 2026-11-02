//! Lighthouse's events are time series: samples, occurrences and aggregates
//! are TimescaleDB hypertables (see the facade's `time_series` suite),
//! regular tables with time indexes elsewhere.

use navree_db::navree_schema::{IndexKind, Schema};
use navree_db::{Db, Executor, MIGRATIONS_TABLE, Statement};

use crate::support::builder;

#[tokio::test]
async fn samples_occurrences_and_aggregates_are_declared_hypertables() {
    let app = builder().build().await;
    let db = app.get::<Db>().await;
    let applied: Vec<String> = db
        .fetch_all(&Statement::text(format!(
            "SELECT name FROM {MIGRATIONS_TABLE} WHERE module = 'lighthouse' ORDER BY name"
        )))
        .await
        .unwrap()
        .iter()
        .map(|row| row.get::<String>("name").unwrap())
        .collect();
    assert_eq!(
        applied,
        [
            "m20261027_000001_create_lighthouse_tables",
            "m20261027_000002_convert_lighthouse_events_to_hypertables",
            "m20261102_000003_create_lighthouse_releases_and_settings",
            "m20261102_000004_index_lighthouse_issue_releases"
        ]
    );
    let schema = Schema::new(&db);
    let timescale = schema.target().await.unwrap().timescaledb().is_some();
    for (table, time) in [
        ("lighthouse_samples", "at"),
        ("lighthouse_occurrences", "at"),
        ("lighthouse_aggregates", "bucket"),
    ] {
        assert_eq!(schema.is_hypertable(table).await.unwrap(), timescale);
        assert!(
            schema
                .has_index(table, [time], Some(IndexKind::Index))
                .await
                .unwrap(),
            "{table} is indexed by {time}, which pruning reads"
        );
    }
    app.shutdown().await.unwrap();
}
