async fn bad(conn: Conn, name: String, id: i64, req: Req) {
    // ruleid: navree-sql-built-from-strings
    conn.execute_unprepared(&format!("DELETE FROM users WHERE name = '{name}'")).await;

    let sql = format!("SELECT * FROM users WHERE id = {}", id);
    // ruleid: navree-sql-built-from-strings
    conn.query_all(Statement::from_string(Backend::Postgres, sql)).await;

    // ruleid: navree-sql-built-from-strings
    sqlx::query(&format!("update t set a = {}", name)).execute(&pool).await;

    // An argument that is a call (a method, a hand-written escaper) is still interpolated text.
    // ruleid: navree-sql-built-from-strings
    conn.fetch_all(format!("SELECT * FROM users WHERE name = {}", req.name())).await;

    // ruleid: navree-sql-built-from-strings
    conn.fetch_all(AssertSqlSafe(format!("SELECT * FROM t WHERE n = {}", literal(&name)))).await;

    let sql = format!("SELECT * FROM t WHERE a = {} AND b = {}", id, req.id());
    // ruleid: navree-sql-built-from-strings
    conn.fetch_optional(&sql).await;
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

    // A constant table name beside a call that builds no SQL text.
    // ok: navree-sql-built-from-strings
    conn.fetch_all(Statement::portable(&format!("SELECT id FROM {JOBS_TABLE}"), vec![name.into()])).await;

    // A call argument in a format! that is not SQL.
    // ok: navree-sql-built-from-strings
    conn.execute_unprepared(&format!("{}", quote(table))).await;
}
