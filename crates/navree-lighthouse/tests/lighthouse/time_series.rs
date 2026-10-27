//! Lighthouse's events are time series: samples, occurrences and aggregates
//! are TimescaleDB hypertables (see the facade's `time_series` suite),
//! regular tables with time indexes here.

use navree_db::navree_schema::{IndexKind, Schema};
use navree_db::{Db, MIGRATIONS_TABLE};
use sea_orm::{ConnectionTrait, Statement};

use crate::support::builder;

#[tokio::test]
async fn samples_occurrences_and_aggregates_are_declared_hypertables() {
    let app = builder().build().await;
    let db = app.get::<Db>().await;
    let applied: Vec<String> = db
        .conn()
        .query_all_raw(Statement::from_string(
            db.backend(),
            format!(
                "SELECT name FROM {MIGRATIONS_TABLE} WHERE module = 'lighthouse' ORDER BY name"
            ),
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .collect();
    assert_eq!(
        applied,
        [
            "m20261027_000001_create_lighthouse_tables",
            "m20261027_000002_convert_lighthouse_events_to_hypertables"
        ]
    );
    let schema = Schema::new(db.conn());
    for (table, time) in [
        ("lighthouse_samples", "at"),
        ("lighthouse_occurrences", "at"),
        ("lighthouse_aggregates", "bucket"),
    ] {
        assert!(!schema.is_hypertable(table).await.unwrap());
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
