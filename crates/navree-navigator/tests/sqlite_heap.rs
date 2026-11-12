//! The SQLite heap cap `navree mcp` sets as it starts. It is process-wide, as
//! SQLite has it, so it runs in a test binary of its own, where no other test
//! shares the cap.

use std::time::{Duration, Instant};

use navree_navigator::database::{
    SQLITE_HEAP_LIMIT, STATEMENT_TIMEOUT, Target, limit_sqlite_heap, query_database,
};

/// An aggregate that grows past the heap cap fails with SQLite's "out of
/// memory" instead of holding the memory: the value limit applies only to the
/// finished value, so without the cap it would hold about 900 MB first.
#[tokio::test]
async fn aggregates_stop_at_the_heap_cap() {
    let dir = tempfile::tempdir().unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("app.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT); \
         INSERT INTO users (name) VALUES ('Harpreet Kaur');",
    )
    .unwrap();
    drop(conn);
    limit_sqlite_heap(SQLITE_HEAP_LIMIT).unwrap();
    let target = Target::new("sqlite://app.db", dir.path(), false).unwrap();

    for aggregate in [
        "json_group_array(printf('%.*c', 900000, 'x'))",
        "json_group_object(i, printf('%.*c', 900000, 'x'))",
    ] {
        let started = Instant::now();
        let sql = format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1000) \
             SELECT length({aggregate}) FROM n"
        );
        let failed = query_database(&target, &sql).await.unwrap_err().to_string();
        assert!(failed.contains("out of memory"), "{aggregate}: {failed}");
        assert!(started.elapsed() < STATEMENT_TIMEOUT + Duration::from_secs(5));
    }

    // The database is still read once the cap has been reached.
    let read = query_database(&target, "SELECT name FROM users")
        .await
        .unwrap();
    assert_eq!(read.rows, vec![vec![serde_json::json!("Harpreet Kaur")]]);
}
