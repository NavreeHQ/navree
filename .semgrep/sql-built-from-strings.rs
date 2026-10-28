async fn bad(conn: Conn, name: String, id: i64) {
    // ruleid: navree-sql-built-from-strings
    conn.execute_unprepared(&format!("DELETE FROM users WHERE name = '{name}'")).await;

    let sql = format!("SELECT * FROM users WHERE id = {}", id);
    // ruleid: navree-sql-built-from-strings
    conn.query_all(Statement::from_string(Backend::Postgres, sql)).await;

    // ruleid: navree-sql-built-from-strings
    sqlx::query(&format!("update t set a = {}", name)).execute(&pool).await;
}

async fn good(conn: Conn, name: String, table: &str) {
    // ok: navree-sql-built-from-strings
    conn.execute(Statement::from_sql_and_values(Backend::Postgres, "DELETE FROM users WHERE name = $1", [name.into()])).await;

    // DDL is not matched: identifiers cannot be bound.
    // ok: navree-sql-built-from-strings
    conn.execute_unprepared(&format!("DROP TABLE IF EXISTS {table} CASCADE")).await;

    // A constant table name is not input.
    // ok: navree-sql-built-from-strings
    conn.execute_unprepared(&format!("DELETE FROM {JOBS_TABLE} WHERE id = 1")).await;

    // Escaped braces are literal text, not a placeholder.
    // ok: navree-sql-built-from-strings
    conn.execute_unprepared(&format!("INSERT INTO t (doc) VALUES ('{{}}')")).await;

    // A format! that is not SQL.
    // ok: navree-sql-built-from-strings
    conn.execute_unprepared(&format!("{table}")).await;
}
