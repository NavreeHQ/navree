//! Connections: read replicas with sticky writes, the writer on demand,
//! named connections, pool and SQLite settings, pool statistics and
//! server facts.

use std::collections::BTreeMap;
use std::sync::Arc;

use navree_db::{
    ChangeEvent, ChangeOp, DatabaseModule, Db, DbConnections, DbOptions, SqliteOptions,
};
use navree_kernel::Navree;
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::support::TempDb;

/// A table `place(name)` holding one row, `name`.
async fn place(db: &Db, conn: &impl ConnectionTrait, name: &str) {
    for sql in [
        "CREATE TABLE IF NOT EXISTS place (name TEXT NOT NULL)".to_owned(),
        format!("INSERT INTO place (name) VALUES ('{name}')"),
    ] {
        conn.execute_raw(Statement::from_string(db.backend(), sql))
            .await
            .unwrap();
    }
}

async fn read_place(conn: &impl ConnectionTrait) -> String {
    conn.query_one_raw(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT name FROM place",
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get::<String>("", "name")
    .unwrap()
}

/// A writer file and a replica file whose `place` rows tell them apart.
async fn replicated(sticky: bool) -> (Db, TempDb, TempDb) {
    let writer = TempDb::new();
    let replica = TempDb::new();
    let db = Db::connect(&DbOptions {
        read_urls: vec![replica.url.clone()],
        sticky,
        ..writer.options()
    })
    .await
    .unwrap();
    place(&db, db.conn(), "writer").await;
    let replica_db = Db::connect(&replica.options()).await.unwrap();
    place(&replica_db, replica_db.conn(), "replica").await;
    (db, writer, replica)
}

#[tokio::test]
async fn replicas_serve_reads_outside_transactions() {
    let (db, _writer, _replica) = replicated(true).await;
    assert_eq!(db.replicas(), 1);
    assert_eq!(read_place(&db.reader().await).await, "replica");
    assert_eq!(read_place(db.conn()).await, "writer", "writes go to the writer");
    // The writer on demand.
    let forced = Db::using_writer(async { read_place(&db.reader().await).await }).await;
    assert_eq!(forced, "writer");
    assert!(!Db::reads_from_writer());
    // Inside an open transaction reads go through it.
    let uow = db.uow(None, None).await;
    let tx = uow.tx().await.unwrap();
    drop(tx);
    let in_tx = uow
        .scope(async { read_place(&db.reader().await).await })
        .await;
    assert_eq!(in_tx, "writer");
    uow.rollback().await.unwrap();
}

#[tokio::test]
async fn reads_stick_to_the_writer_after_a_unit_of_work_writes() {
    let (db, _writer, _replica) = replicated(true).await;
    let uow = db.uow(None, None).await;
    assert!(!uow.has_modified_records());
    let before = uow
        .scope(async { read_place(&db.reader().await).await })
        .await;
    assert_eq!(before, "replica");
    uow.record_change(ChangeEvent {
        entity: "place".into(),
        id: "1".into(),
        op: ChangeOp::Created,
        old: None,
        new: None,
    });
    assert!(uow.has_modified_records());
    uow.commit().await.unwrap();
    // Committed: no transaction is open, yet the task reads its writes.
    let after = uow
        .scope(async { read_place(&db.reader().await).await })
        .await;
    assert_eq!(after, "writer");

    let (lagging, _w, _r) = replicated(false).await;
    let uow = lagging.uow(None, None).await;
    uow.mark_modified();
    let read = uow
        .scope(async { read_place(&lagging.reader().await).await })
        .await;
    assert_eq!(read, "replica", "sticky is off");
}

#[tokio::test]
async fn named_connections_connect_on_first_use() {
    let main = TempDb::new();
    let analytics = TempDb::new();
    let mut connections = BTreeMap::new();
    connections.insert("analytics".to_owned(), analytics.options());
    let app = Navree::app()
        .module(DatabaseModule::for_root(DbOptions {
            connections,
            ..main.options()
        }))
        .boot()
        .await
        .unwrap();
    let registry = app.ctx().get::<DbConnections>().await.unwrap();
    assert_eq!(registry.names(), ["analytics", "default"]);
    let default = registry.get("default").await.unwrap();
    assert_eq!(default.name(), "default");
    assert!(std::ptr::eq(
        default.conn(),
        app.ctx().get::<Db>().await.unwrap().conn()
    ));
    let named = registry.get("analytics").await.unwrap();
    assert_eq!(named.name(), "analytics");
    assert!(Arc::ptr_eq(&named, &registry.get("analytics").await.unwrap()), "one pool");
    place(&named, named.conn(), "analytics").await;
    assert_eq!(read_place(named.conn()).await, "analytics");
    assert!(named.app().is_some(), "named connections belong to the app");
    let error = registry.get("reports").await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "Custom Error: no database connection named `reports` (configured: analytics, default)"
    );
}

#[tokio::test]
async fn any_connection_reaches_the_others_by_name() {
    let main = TempDb::new();
    let analytics = TempDb::new();
    let mut connections = BTreeMap::new();
    connections.insert("analytics".to_owned(), analytics.options());
    let app = Navree::app()
        .module(DatabaseModule::for_root(DbOptions {
            connections,
            ..main.options()
        }))
        .boot()
        .await
        .unwrap();
    let db = app.ctx().get::<Db>().await.unwrap();
    assert!(db.is_default());
    // Named providers.
    let named = app.ctx().get_named::<Db>("analytics").await.unwrap();
    assert_eq!(named.name(), "analytics");
    assert!(!named.is_default());
    let default = app.ctx().get_named::<Db>("default").await.unwrap();
    assert!(std::ptr::eq(default.conn(), db.conn()));
    // From one connection to another.
    let reached = db.connection("analytics").await.unwrap();
    assert!(std::ptr::eq(reached.conn(), named.conn()), "the same pool");
    let back = named.connection("default").await.unwrap();
    assert!(std::ptr::eq(back.conn(), db.conn()));
    assert!(std::ptr::eq(
        db.connection("default").await.unwrap().conn(),
        db.conn()
    ));
    let unknown = db.connection("reports").await.unwrap_err();
    assert!(
        unknown
            .to_string()
            .contains("no database connection named `reports`"),
        "{unknown}"
    );
    // A `Db` connected by hand belongs to no app: it knows only itself.
    let alone = Db::connect(&main.options()).await.unwrap();
    assert!(alone.connection("default").await.is_ok());
    let error = alone.connection("analytics").await.unwrap_err();
    assert!(error.to_string().contains("belongs to no app"), "{error}");
}

/// Records the events published through `dyn EventSink`.
#[derive(Default)]
pub struct Recorder(std::sync::Mutex<Vec<(String, serde_json::Value)>>);

impl Recorder {
    pub fn events(&self) -> Vec<(String, serde_json::Value)> {
        self.0.lock().unwrap().clone()
    }
}

impl navree_contracts::EventSink for Recorder {
    fn publish(
        &self,
        name: &str,
        payload: serde_json::Value,
    ) -> navree_kernel::BoxFuture<'_, Result<(), navree_kernel::BoxError>> {
        self.0.lock().unwrap().push((name.to_owned(), payload));
        Box::pin(async { Ok(()) })
    }
}

/// Provides `recorder` as the app's `dyn EventSink`.
pub struct Events(pub Arc<Recorder>);

impl navree_kernel::Module for Events {
    fn register(&self, m: &mut navree_kernel::ModuleBuilder) {
        m.global();
        m.value_arc::<dyn navree_contracts::EventSink>(self.0.clone());
        m.exports::<dyn navree_contracts::EventSink>();
    }
}

#[tokio::test]
async fn connections_announce_themselves_and_the_monitor_reports_them() {
    use navree_db::{ConnectionEstablished, DatabaseStatus};
    use serde_json::json;

    let main = TempDb::new();
    let analytics = TempDb::new();
    let mut connections = BTreeMap::new();
    connections.insert("analytics".to_owned(), analytics.options());
    let recorder = Arc::new(Recorder::default());
    let app = Navree::app()
        .module(Events(recorder.clone()))
        .module(DatabaseModule::for_root(DbOptions {
            connections,
            ..main.options()
        }))
        .boot()
        .await
        .unwrap();
    assert_eq!(
        recorder.events(),
        [(
            ConnectionEstablished::NAME.to_owned(),
            json!({"connection": "default"})
        )]
    );
    let registry = app.ctx().get::<DbConnections>().await.unwrap();
    let statuses = registry.monitor(&[], Some(0)).await.unwrap();
    assert_eq!(
        statuses,
        [
            DatabaseStatus {
                connection: "analytics".into(),
                open: None,
                busy: false
            },
            DatabaseStatus {
                connection: "default".into(),
                open: None,
                busy: false
            },
        ],
        "SQLite has no server to count connections on"
    );
    assert_eq!(
        recorder.events()[1],
        (
            ConnectionEstablished::NAME.to_owned(),
            json!({"connection": "analytics"})
        ),
        "a named connection announces itself when it opens"
    );
    assert_eq!(recorder.events().len(), 2, "no db.busy for SQLite");
    let unknown = registry.monitor(&["reports".to_owned()], None).await;
    assert!(unknown.is_err());

    // The commands.
    app.run_command("db:monitor", vec!["--databases".into(), "default".into()])
        .await
        .unwrap();
    let bad = app
        .run_command("db:monitor", vec!["--max".into(), "many".into()])
        .await
        .unwrap_err();
    assert!(bad.to_string().contains("--max takes a number"), "{bad}");
    let missing = app
        .run_command("db", vec!["reports".into()])
        .await
        .unwrap_err();
    assert!(
        missing
            .to_string()
            .contains("no database connection named `reports`"),
        "{missing}"
    );
    let flag = app
        .run_command("db", vec!["--both".into()])
        .await
        .unwrap_err();
    assert!(flag.to_string().contains("unknown argument `--both`"), "{flag}");
}

/// Owns the `users` table.
struct Users;

impl navree_kernel::Module for Users {
    fn register(&self, m: &mut navree_kernel::ModuleBuilder) {
        use navree_db::{MigrationSet, ModuleBuilderDbExt};

        m.migrations(MigrationSet {
            module: "users",
            after: vec![],
            migrations: vec![crate::fixtures::CreateTable::boxed(
                "m0001_create_users",
                "users",
            )],
        });
    }
}

#[tokio::test]
async fn migrations_and_session_work_use_the_direct_connection() {
    use navree_db::Migrator;

    // Behind a pooler, `url` and `direct_url` reach one database; two
    // files show which one is used.
    let pooled = TempDb::new();
    let direct = TempDb::new();
    let app = Navree::app()
        .module(DatabaseModule::for_root(DbOptions {
            direct_url: Some(direct.url.clone()),
            migrate_on_boot: true,
            ..pooled.options()
        }))
        .module(Users)
        .boot()
        .await
        .unwrap();
    let db = app.ctx().get::<Db>().await.unwrap();
    assert!(db.has_direct());
    let session = db.direct().await.unwrap();
    assert!(!std::ptr::eq(session.conn(), db.conn()));
    assert!(
        std::ptr::eq(db.direct().await.unwrap().conn(), session.conn()),
        "opened once"
    );
    assert_eq!(session.name(), "default");
    let direct_tables = crate::fixtures::tables(&session).await;
    assert!(direct_tables.contains(&"users".to_owned()), "{direct_tables:?}");
    assert!(
        !crate::fixtures::tables(&db).await.contains(&"users".to_owned()),
        "migrations bypass the pooled URL"
    );
    assert!(
        Migrator::status(app.ctx())
            .await
            .unwrap()
            .iter()
            .all(|status| status.applied)
    );
    // The direct connection's statements reach the connection's listeners.
    let heard = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = heard.clone();
    db.on_query(move |_| {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });
    read_count(&session).await;
    assert!(heard.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    db.close().await.unwrap();
    assert!(session.conn().ping().await.is_err(), "closed with the connection");

    // Without `direct_url`, the direct connection is the connection.
    let plain = Db::connect(&pooled.options()).await.unwrap();
    assert!(!plain.has_direct());
    assert!(std::ptr::eq(plain.direct().await.unwrap().conn(), plain.conn()));
    // It must reach the same kind of database.
    let mismatched = Db::connect(&DbOptions {
        direct_url: Some("postgres://app@db/app".into()),
        ..pooled.options()
    })
    .await
    .unwrap_err();
    assert!(
        mismatched
            .to_string()
            .contains("direct_url must name the same kind of database as url"),
        "{mismatched}"
    );
}

async fn read_count(db: &Db) {
    db.conn()
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT COUNT(*) FROM users",
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn pool_settings_statistics_and_closing() {
    let temp = TempDb::new();
    let db = Db::connect(&DbOptions {
        max_connections: 3,
        idle_timeout_secs: Some(30),
        max_lifetime_secs: Some(600),
        ..temp.options()
    })
    .await
    .unwrap();
    db.conn()
        .execute_unprepared("CREATE TABLE t (id INTEGER)")
        .await
        .unwrap();
    let stats = db.pool_stats();
    assert_eq!(stats.max, 3);
    assert!(stats.size >= 1 && stats.size <= 3, "{stats:?}");
    assert!(stats.idle <= stats.size as usize, "{stats:?}");
    assert_eq!(db.open_connections().await.unwrap(), None, "SQLite has no server");
    db.close().await.unwrap();
    assert!(db.conn().execute_unprepared("SELECT 1").await.is_err());
}

#[tokio::test]
async fn sqlite_settings_apply_to_every_connection() {
    let temp = TempDb::new();
    let mut pragmas = BTreeMap::new();
    pragmas.insert("cache_size".to_owned(), "-4000".to_owned());
    let db = Db::connect(&DbOptions {
        sqlite: SqliteOptions {
            journal_mode: "delete".into(),
            busy_timeout_ms: 1234,
            foreign_keys: false,
            synchronous: Some("full".into()),
            pragmas,
        },
        ..temp.options()
    })
    .await
    .unwrap();
    let pragma = |name: &'static str| {
        let db = db.clone();
        async move {
            let row = db
                .conn()
                .query_one_raw(Statement::from_string(
                    DbBackend::Sqlite,
                    format!("PRAGMA {name}"),
                ))
                .await
                .unwrap()
                .unwrap();
            row.try_get_by_index::<String>(0)
                .or_else(|_| row.try_get_by_index::<i64>(0).map(|v| v.to_string()))
                .unwrap()
        }
    };
    assert_eq!(pragma("journal_mode").await, "delete");
    assert_eq!(pragma("busy_timeout").await, "1234");
    assert_eq!(pragma("foreign_keys").await, "0");
    assert_eq!(pragma("synchronous").await, "2", "FULL");
    assert_eq!(pragma("cache_size").await, "-4000");
}

#[tokio::test]
async fn server_facts() {
    let temp = TempDb::new();
    let db = Db::connect(&temp.options()).await.unwrap();
    assert_eq!(db.name(), "default");
    assert!(db.server_version().await.unwrap().starts_with('3'));
    assert!(!db.is_mariadb().await.unwrap());
    assert!(!db.has_timescaledb().await.unwrap());
    assert_eq!(db.database_name().await.unwrap(), "main");
}

#[test]
fn options_validate_replicas_connections_and_sqlite_settings() {
    use navree_config::ConfigSection;

    let mut nested = BTreeMap::new();
    nested.insert("inner".to_owned(), DbOptions::default());
    let mut connections = BTreeMap::new();
    connections.insert(
        "analytics".to_owned(),
        DbOptions {
            connections: nested,
            ..DbOptions::default()
        },
    );
    let options = DbOptions {
        read_urls: vec![" ".into()],
        connections,
        sqlite: SqliteOptions {
            synchronous: Some("sometimes".into()),
            journal_mode: "diary".into(),
            pragmas: BTreeMap::from([("cache size; DROP".to_owned(), "1".to_owned())]),
            ..SqliteOptions::default()
        },
        ..DbOptions::default()
    };
    let problems = options.validate().unwrap_err();
    for expected in [
        "read_urls must not contain an empty URL",
        "connections.analytics: connections cannot be nested",
        "sqlite.synchronous: `sometimes` is not one of off, normal, full, extra",
        "sqlite.journal_mode: `diary` is not one of delete, truncate, persist, memory, wal, off",
        "sqlite.pragmas: `cache size; DROP` is not a pragma name",
    ] {
        assert!(problems.contains(expected), "{problems}\n  missing: {expected}");
    }
}

#[test]
fn read_urls_come_as_a_list_or_a_comma_separated_string() {
    let listed: DbOptions = serde_json::from_value(serde_json::json!({
        "read_urls": ["postgres://a/db", "postgres://b/db"]
    }))
    .unwrap();
    let from_env: DbOptions = serde_json::from_value(serde_json::json!({
        "read_urls": "postgres://a/db, postgres://b/db"
    }))
    .unwrap();
    assert_eq!(listed.read_urls, from_env.read_urls);
    assert!(listed.sticky, "sticky by default");
    let options = DbOptions {
        url: "postgres://u:secret@main/db".into(),
        read_urls: vec!["postgres://u:secret@replica/db".into()],
        ..DbOptions::default()
    };
    assert!(!format!("{options:?}").contains("secret"));
    let tenant = options.for_database("postgres://u:secret@main/tenant_1");
    assert_eq!(tenant.url, "postgres://u:secret@main/tenant_1");
    assert!(tenant.read_urls.is_empty(), "replicas belong to the main database");
}
