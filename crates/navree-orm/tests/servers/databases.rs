//! Test databases on real servers: template copies (`CREATE DATABASE …
//! TEMPLATE` on PostgreSQL, a copy of the tables and rows on MySQL), fresh
//! databases and truncation, each dropped afterwards.

use navree_db::sea_orm::{ConnectionTrait, DbBackend, Statement};
use navree_db::{Db, DbOptions};
use navree_orm::table;
use navree_orm::testing::database::{DatabaseKind, Template, TestDb, TestServer, test_setting};

/// Authors and notes (a foreign key), one author and a migration row.
static SCHEMA: Template = Template::new("servers", |template| {
    Box::pin(async move {
        let conn = template.db().conn();
        let statements = match template.kind() {
            DatabaseKind::Postgres => [
                "CREATE TABLE authors (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL)",
                "CREATE TABLE notes (id BIGSERIAL PRIMARY KEY, author_id BIGINT NOT NULL REFERENCES authors (id), body TEXT NOT NULL)",
                "CREATE TABLE navree_migrations (version TEXT PRIMARY KEY)",
            ],
            _ => [
                "CREATE TABLE authors (id BIGINT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(100) NOT NULL)",
                "CREATE TABLE notes (id BIGINT AUTO_INCREMENT PRIMARY KEY, author_id BIGINT NOT NULL, body TEXT NOT NULL, FOREIGN KEY (author_id) REFERENCES authors (id))",
                "CREATE TABLE navree_migrations (version VARCHAR(50) PRIMARY KEY)",
            ],
        };
        for sql in statements {
            conn.execute_unprepared(sql).await?;
        }
        conn.execute_unprepared("INSERT INTO authors (name) VALUES ('Ada')")
            .await?;
        conn.execute_unprepared("INSERT INTO navree_migrations (version) VALUES ('m0001')")
            .await?;
        Ok(())
    })
});

async fn count(db: &Db, name: &str) -> u64 {
    table(name).count(db).await.unwrap()
}

/// The database name in a test database's URL.
fn database_name(url: &str) -> String {
    let path = url.split('?').next().unwrap();
    path.rsplit('/').next().unwrap().to_owned()
}

/// Whether database `name` exists on `server`.
async fn exists(server: TestServer, name: &str) -> bool {
    let db = Db::connect(&DbOptions {
        url: test_setting(server.variable()).unwrap(),
        max_connections: 1,
        ..DbOptions::default()
    })
    .await
    .unwrap();
    let (sql, backend) = match server {
        TestServer::Postgres => (
            "SELECT datname FROM pg_database WHERE datname = $1",
            DbBackend::Postgres,
        ),
        _ => (
            "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?",
            DbBackend::MySql,
        ),
    };
    let found = db
        .conn()
        .query_one_raw(Statement::from_sql_and_values(backend, sql, [name.into()]))
        .await
        .unwrap()
        .is_some();
    let _ = db.conn().clone().close().await;
    found
}

/// Two copies of the template, independent; truncation; and both ways a
/// test database goes away.
async fn copies_truncation_and_cleanup(server: TestServer, first: TestDb, second: TestDb) {
    assert_ne!(first.url(), second.url());
    let insert = "INSERT INTO notes (author_id, body) VALUES (1, 'hello')";
    first.db().conn().execute_unprepared(insert).await.unwrap();
    assert_eq!(count(first.db(), "notes").await, 1);
    assert_eq!(count(second.db(), "notes").await, 0);
    assert_eq!(count(second.db(), "authors").await, 1, "the template's rows");
    // The copy keeps the foreign key.
    let orphan = "INSERT INTO notes (author_id, body) VALUES (99, 'orphan')";
    assert!(second.db().conn().execute_unprepared(orphan).await.is_err());

    first.truncate(&["navree_migrations"]).await.unwrap();
    assert_eq!(count(first.db(), "notes").await, 0);
    assert_eq!(count(first.db(), "authors").await, 0);
    assert_eq!(count(first.db(), "navree_migrations").await, 1);
    first
        .db()
        .conn()
        .execute_unprepared("INSERT INTO authors (name) VALUES ('Grace')")
        .await
        .unwrap();
    let restarted = table("authors")
        .where_(navree_orm::col("id").eq(navree_orm::Value::from(1_i64)))
        .count(first.db())
        .await
        .unwrap();
    assert_eq!(restarted, 1, "counters start over");

    let closed = database_name(first.url());
    first.close().await.unwrap();
    assert!(!exists(server, &closed).await, "closed: dropped");
    let dropped = database_name(second.url());
    assert!(exists(server, &dropped).await);
    drop(second);
    assert!(!exists(server, &dropped).await, "dropped with its TestDb");
}

#[tokio::test]
async fn postgres_template_copies() {
    let Some(first) = SCHEMA.postgres().await.unwrap() else {
        return;
    };
    assert_eq!(first.kind(), DatabaseKind::Postgres);
    let second = SCHEMA.postgres().await.unwrap().unwrap();
    copies_truncation_and_cleanup(TestServer::Postgres, first, second).await;
}

#[tokio::test]
async fn mysql_template_copies() {
    let Some(first) = SCHEMA.mysql().await.unwrap() else {
        return;
    };
    assert_eq!(first.kind(), DatabaseKind::MySql);
    let second = SCHEMA.mysql().await.unwrap().unwrap();
    copies_truncation_and_cleanup(TestServer::MySql, first, second).await;
}

#[navree_orm::test(template = SCHEMA, database = "postgres")]
async fn the_test_attribute_copies_templates_on_postgres(db: TestDb) {
    assert_eq!(db.kind(), DatabaseKind::Postgres);
    assert_eq!(count(db.db(), "authors").await, 1);
}

#[navree_orm::test(template = SCHEMA, database = "mysql")]
async fn the_test_attribute_copies_templates_on_mysql(db: TestDb) -> Result<(), String> {
    assert_eq!(db.kind(), DatabaseKind::MySql);
    assert_eq!(count(db.db(), "authors").await, 1);
    Ok(())
}

#[tokio::test]
async fn fresh_server_databases_are_empty() {
    for server in [TestServer::Postgres, TestServer::MySql] {
        let test = match server {
            TestServer::Postgres => TestDb::postgres().await.unwrap(),
            _ => TestDb::mysql().await.unwrap(),
        };
        let Some(test) = test else {
            continue;
        };
        let tables = match test.kind() {
            DatabaseKind::Postgres => {
                "SELECT COUNT(*) AS n FROM pg_tables WHERE schemaname = current_schema()"
            }
            _ => {
                "SELECT COUNT(*) AS n FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE()"
            }
        };
        let row = test
            .db()
            .conn()
            .query_one_raw(Statement::from_string(test.db().backend(), tables))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<i64>("", "n").unwrap(), 0, "{server:?}");
        test.close().await.unwrap();
    }
}
