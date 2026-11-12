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

    // Navree's executor running a statement built from a format!.
    // ruleid: navree-sql-built-from-strings
    executor.execute(&Statement::portable(format!("DELETE FROM {} WHERE id = ?", name), vec![id.into()])).await;

    // ruleid: navree-sql-built-from-strings
    let statement = Statement::new(backend, format!("SELECT * FROM t WHERE n = {name}"), vec![]);

    // ruleid: navree-sql-built-from-strings
    executor.fetch_scalar(&Statement::portable(format!("SELECT count(*) FROM {name}"), vec![])).await;

    // The same macro by its path, and with brackets.
    // ruleid: navree-sql-built-from-strings
    conn.fetch_all(std::format!("SELECT * FROM t WHERE n = {}", name)).await;

    // ruleid: navree-sql-built-from-strings
    conn.fetch_all(format!["SELECT * FROM t WHERE n = {}", name]).await;

    // A literal brace, then a placeholder.
    // ruleid: navree-sql-built-from-strings
    conn.fetch_all(format!("SELECT * FROM t WHERE doc = '{{{name}}}'")).await;

    // A raw string with a value in it.
    // ruleid: navree-sql-built-from-strings
    conn.fetch_all(format!(r#"SELECT "a" FROM t WHERE b = {name}"#)).await;

    // An escaped quote does not end the template.
    // ruleid: navree-sql-built-from-strings
    conn.fetch_all(format!("SELECT \"a\" FROM t WHERE b = {name}")).await;

    // A value after constants, on a continued line.
    // ruleid: navree-sql-built-from-strings
    conn.fetch_all(format!("SELECT {COLUMNS} FROM {TABLE} \
        WHERE {filter} LIMIT ?")).await;
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

    // Braces in the arguments (a closure, a block) are not placeholders of the template.
    // ok: navree-sql-built-from-strings
    conn.fetch_all(format!("SELECT {COLS} FROM {TABLE}", COLS = cols.iter().map(|c| { c.name() }).collect::<String>())).await;

    // ok: navree-sql-built-from-strings
    conn.fetch_all(format!("SELECT {COLS} FROM t", COLS = { let c = 1; c })).await;

    // Four braces are two literal ones.
    // ok: navree-sql-built-from-strings
    conn.fetch_all(format!("SELECT * FROM t WHERE doc = '{{{{x}}}}'")).await;

    // A raw string with a constant only.
    // ok: navree-sql-built-from-strings
    conn.fetch_all(format!(r#"SELECT "a" FROM {TABLE} WHERE b = ?"#)).await;

    // A statement from a template with no value in it.
    // ok: navree-sql-built-from-strings
    executor.execute(&Statement::portable(format!("DELETE FROM {JOBS_TABLE} WHERE id = ?"), vec![name.into()])).await;
}

// Inside an async block, where the taint rule does not look.
impl Store {
    fn count<'a>(&'a self, collection: &'a str) -> BoxFuture<'a, u64> {
        Box::pin(async move {
            // ruleid: navree-sql-built-from-strings-in-async
            let row = self.db.fetch_optional(&Statement::text(format!("SELECT COUNT(*) AS n FROM {}", table(collection)))).await;

            let sql = format!("SELECT n FROM t WHERE c = {collection}");
            // ruleid: navree-sql-built-from-strings-in-async
            self.db.fetch_all(&Statement::portable(sql, vec![])).await;

            // ruleid: navree-sql-built-from-strings-in-async
            let statement = Statement::new(Backend::Postgres, format!("SELECT * FROM t WHERE c = {collection}"), vec![]);

            // ok: navree-sql-built-from-strings-in-async
            self.db.fetch_all(&Statement::portable(format!("SELECT n FROM {TABLE} WHERE c = ?"), vec![collection.into()])).await;

            // ok: navree-sql-built-from-strings-in-async
            let label = format!("SELECT {collection}");

            // Words that start like SQL, handed to a constructor that is not a statement's.
            // ok: navree-sql-built-from-strings-in-async
            let text = Text::new(format!("Delete {}?", collection));
            0
        })
    }
}
