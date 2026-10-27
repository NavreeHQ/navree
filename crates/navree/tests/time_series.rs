//! Navree's own time-series tables, as an app gets them: the audit log, the
//! webhook delivery log, Fleet's job history and metrics, the scheduler's
//! runs and Lighthouse's events are TimescaleDB hypertables, and regular
//! tables with time indexes on every other database.
//!
//! The SQLite test always runs. The TimescaleDB and MySQL ones run against
//! real servers: build with `--features postgres,mysql` and set
//! `NAVREE_TEST_POSTGRES_URL` and `NAVREE_TEST_MYSQL_URL` to URLs whose users
//! may create databases (each test works in a database of its own):
//!
//! ```text
//! NAVREE_TEST_POSTGRES_URL=postgres://postgres:secret@127.0.0.1:55432/postgres \
//! NAVREE_TEST_MYSQL_URL=mysql://root:secret@127.0.0.1:53306/test \
//! cargo test -p navree --features postgres,mysql --test time_series
//! ```

use navree::db::navree_schema::Schema;
use navree::db::{Db, MIGRATIONS_TABLE};
use navree::prelude::*;
use navree::testing::{TestApp, TestAppBuilder};
use sea_orm::{ConnectionTrait, Statement};

/// Every time-series table of the framework, with its time column.
const TIME_SERIES: [(&str, &str); 9] = [
    ("audit_logs", "created_at"),
    ("webhook_deliveries", "created_at"),
    ("webhook_attempts", "created_at"),
    ("fleet_jobs", "finished_at"),
    ("fleet_metrics", "at"),
    ("schedule_runs", "tick"),
    ("lighthouse_samples", "at"),
    ("lighthouse_occurrences", "at"),
    ("lighthouse_aggregates", "bucket"),
];

/// The conversion migrations, one per module.
const CONVERSIONS: [&str; 5] = [
    "m20261027_000001_convert_audit_logs_to_hypertable",
    "m20261027_000001_convert_schedule_runs_to_hypertable",
    "m20261027_000001_convert_webhook_deliveries_to_hypertables",
    "m20261027_000002_convert_fleet_history_to_hypertables",
    "m20261027_000002_convert_lighthouse_events_to_hypertables",
];

/// An app with every module that keeps time series, migrated at boot (the
/// database comes from the caller).
fn app() -> TestAppBuilder {
    TestApp::builder()
        .config("database.migrate_on_boot", true)
        .config("audit.routes", false)
        .config("webhooks.routes", false)
        .config("fleet.dashboard", false)
        .config("lighthouse.dashboard", false)
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(QueueModule::from_config())
        .module(HttpClientModule::from_config())
        .module(AuditModule::from_config())
        .module(WebhooksModule::from_config())
        .module(FleetModule::from_config())
        .module(ScheduleModule::for_root())
        .module(LighthouseModule::from_config())
}

/// The names of the applied migrations that convert tables.
async fn conversions_applied(db: &Db) -> Vec<String> {
    let mut names: Vec<String> = db
        .conn()
        .query_all_raw(Statement::from_string(
            db.backend(),
            format!("SELECT name FROM {MIGRATIONS_TABLE}"),
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .filter(|name| name.contains("_to_hypertable"))
        .collect();
    names.sort();
    names
}

/// Every table is a regular table, indexed by its time column.
async fn assert_regular_tables(db: &Db) {
    let schema = Schema::new(db.conn());
    for (table, time) in TIME_SERIES {
        assert!(!schema.is_hypertable(table).await.unwrap(), "{table}");
        assert!(
            schema.has_index(table, [time], None).await.unwrap(),
            "{table} is indexed by {time}"
        );
    }
}

/// Attempts reference their endpoint (with `ON DELETE CASCADE`), not their
/// delivery.
async fn assert_attempts_reference_endpoints(db: &Db) {
    let keys = Schema::new(db.conn())
        .foreign_keys("webhook_attempts")
        .await
        .unwrap();
    assert_eq!(keys.len(), 1, "{keys:?}");
    assert_eq!(keys[0].columns, ["endpoint_id"]);
    assert_eq!(keys[0].foreign_table, "webhook_endpoints");
}

#[tokio::test]
async fn sqlite_keeps_regular_tables_with_time_indexes() {
    let app = app().temp_database().build().await;
    let db = app.get::<Db>().await;
    assert_eq!(conversions_applied(&db).await, CONVERSIONS);
    assert_regular_tables(&db).await;
    assert_attempts_reference_endpoints(&db).await;
    app.shutdown().await.unwrap();
}

/// Databases of their own on the test servers.
#[cfg(any(feature = "postgres", feature = "mysql"))]
mod scratch {
    use std::sync::atomic::{AtomicU32, Ordering};

    use sea_orm::{ConnectionTrait, Database};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A database created for one test; [`drop`](Scratch::drop) removes it.
    pub struct Scratch {
        admin: String,
        name: String,
        pub url: String,
        mysql: bool,
    }

    impl Scratch {
        /// A fresh database on the server of `variable`, or `None` (with a
        /// notice) when it is unset.
        pub async fn new(variable: &str, mysql: bool) -> Option<Scratch> {
            let Ok(admin) = std::env::var(variable) else {
                eprintln!("{variable} is not set: skipping this real-server test");
                return None;
            };
            let name = format!(
                "navree_time_series_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::SeqCst)
            );
            let conn = Database::connect(&admin).await.expect("connects");
            conn.execute_unprepared(&format!("CREATE DATABASE {name}"))
                .await
                .expect("the test user may create databases");
            conn.close().await.expect("closes");
            let (base, query) = match admin.split_once('?') {
                Some((base, query)) => (base.to_owned(), format!("?{query}")),
                None => (admin.clone(), String::new()),
            };
            let (server, _) = base.rsplit_once('/').expect("a URL with a database");
            let url = format!("{server}/{name}{query}");
            Some(Scratch {
                admin,
                name,
                url,
                mysql,
            })
        }

        /// Drops the database.
        pub async fn drop(self) {
            let conn = Database::connect(&self.admin).await.expect("connects");
            let sql = if self.mysql {
                format!("DROP DATABASE IF EXISTS {}", self.name)
            } else {
                format!("DROP DATABASE IF EXISTS {} WITH (FORCE)", self.name)
            };
            conn.execute_unprepared(&sql).await.expect("drops");
            conn.close().await.expect("closes");
        }
    }
}

#[cfg(feature = "postgres")]
mod timescaledb {
    use chrono::{TimeDelta, Utc};
    use navree::db::navree_schema::{IndexKind, Schema};
    use navree::db::{Db, MIGRATIONS_TABLE};
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    use super::scratch::Scratch;
    use super::{CONVERSIONS, app, assert_attempts_reference_endpoints, conversions_applied};

    async fn number(db: &Db, sql: &str) -> i64 {
        db.conn()
            .query_one_raw(Statement::from_string(DbBackend::Postgres, sql))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
            .unwrap()
            .try_get_by_index::<i64>(0)
            .unwrap()
    }

    #[tokio::test]
    async fn every_time_series_table_is_a_hypertable() {
        let Some(scratch) = Scratch::new("NAVREE_TEST_POSTGRES_URL", false).await else {
            return;
        };
        let app = app()
            .config("database.url", scratch.url.clone())
            .build()
            .await;
        let db = app.get::<Db>().await;
        let schema = Schema::new(db.conn());
        let target = schema.target().await.unwrap();
        if target.timescaledb().is_none() {
            eprintln!("TimescaleDB is not available on this server: skipping");
            app.shutdown().await.unwrap();
            scratch.drop().await;
            return;
        }
        assert_eq!(conversions_applied(&db).await, CONVERSIONS);
        let timestamp = "timestamp with time zone";
        for (table, column, kind, interval) in [
            ("audit_logs", "created_at", timestamp, "7 days"),
            ("webhook_deliveries", "created_at", timestamp, "1 day"),
            ("webhook_attempts", "created_at", timestamp, "1 day"),
            ("fleet_jobs", "finished_at", "bigint", "3600000"),
            ("fleet_metrics", "at", "bigint", "86400000"),
            ("schedule_runs", "tick", timestamp, "1 day"),
            ("lighthouse_samples", "at", timestamp, "1 day"),
            ("lighthouse_occurrences", "at", timestamp, "1 day"),
            ("lighthouse_aggregates", "bucket", timestamp, "1 day"),
        ] {
            let dimension = schema
                .time_dimension(table)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("{table} is a hypertable"));
            assert_eq!(
                (
                    dimension.column.as_str(),
                    dimension.column_type.as_str(),
                    dimension.chunk_interval.as_str()
                ),
                (column, kind, interval),
                "{table}"
            );
        }
        // Unique keys gained the time column; the runs' key already had it.
        for (table, key) in [
            ("audit_logs", &["id", "created_at"][..]),
            ("webhook_deliveries", &["id", "created_at"][..]),
            ("webhook_attempts", &["id", "created_at"][..]),
            ("fleet_jobs", &["id", "finished_at"][..]),
            ("schedule_runs", &["task", "tick"][..]),
            ("lighthouse_aggregates", &["id", "bucket"][..]),
        ] {
            assert!(
                schema
                    .has_index(table, key, Some(IndexKind::Primary))
                    .await
                    .unwrap(),
                "{table}: PRIMARY KEY ({})",
                key.join(", ")
            );
        }
        assert_attempts_reference_endpoints(&db).await;
        for table in [
            "webhook_endpoints",
            "jobs",
            "lighthouse_issues",
            "fleet_masters",
        ] {
            assert!(!schema.is_hypertable(table).await.unwrap(), "{table}");
        }

        // The conversions run again harmlessly (after a rollback, say).
        let names = CONVERSIONS
            .iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(", ");
        db.conn()
            .execute_unprepared(&format!(
                "DELETE FROM {MIGRATIONS_TABLE} WHERE name IN ({names})"
            ))
            .await
            .unwrap();
        app.run_command("migrate", &[]).await.unwrap();
        assert_eq!(conversions_applied(&db).await, CONVERSIONS);
        assert_eq!(
            number(&db, "SELECT count(*) FROM timescaledb_information.dimensions WHERE hypertable_name = 'audit_logs'").await,
            1
        );

        // `audit:prune` drops the weeks before the cutoff whole.
        let now = Utc::now();
        for weeks in [10, 9, 8, 1] {
            db.conn()
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "INSERT INTO audit_logs (id, kind, action, metadata, created_at) \
                     VALUES (gen_random_uuid(), 'auth', 'auth.login', '{}', $1)",
                    [(now - TimeDelta::weeks(weeks)).into()],
                ))
                .await
                .unwrap();
        }
        let chunks = || number(&db, "SELECT count(*) FROM show_chunks('audit_logs')");
        assert_eq!(chunks().await, 4);
        app.run_command("audit:prune", &["--days", "30"])
            .await
            .unwrap();
        assert_eq!(number(&db, "SELECT count(*) FROM audit_logs").await, 1);
        assert_eq!(chunks().await, 1, "three old weeks dropped whole");
        // The other prunes run on hypertables too.
        app.run_command("webhooks:prune", &[]).await.unwrap();
        app.run_command("lighthouse:prune", &[]).await.unwrap();

        app.shutdown().await.unwrap();
        scratch.drop().await;
    }
}

#[cfg(feature = "mysql")]
mod mysql {
    use navree::db::Db;

    use super::scratch::Scratch;
    use super::{
        CONVERSIONS, app, assert_attempts_reference_endpoints, assert_regular_tables,
        conversions_applied,
    };

    #[tokio::test]
    async fn mysql_keeps_regular_tables_with_time_indexes() {
        let Some(scratch) = Scratch::new("NAVREE_TEST_MYSQL_URL", true).await else {
            return;
        };
        let app = app()
            .config("database.url", scratch.url.clone())
            .build()
            .await;
        let db = app.get::<Db>().await;
        assert_eq!(conversions_applied(&db).await, CONVERSIONS);
        assert_regular_tables(&db).await;
        assert_attempts_reference_endpoints(&db).await;
        app.run_command("audit:prune", &[]).await.unwrap();
        app.run_command("webhooks:prune", &[]).await.unwrap();
        app.shutdown().await.unwrap();
        scratch.drop().await;
    }
}
