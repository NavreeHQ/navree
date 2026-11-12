//! `query_database` and `describe_schema` on PostgreSQL.
//!
//! Opt-in: set `NAVREE_TEST_POSTGRES_URL` to a superuser URL of a throwaway
//! local server, for example
//! `postgres://postgres:postgres@127.0.0.1:55432/postgres`. Each test
//! creates its own database and a read-only role for it, and drops both at
//! the end; Navigator connects as that role, as `query_database` refuses a
//! superuser. Without the variable the tests print `SKIPPED` and pass,
//! unless `NAVREE_TEST_REQUIRE_SERVICES=1`.

use navree_navigator::DatabaseOptions;
use serde_json::json;
use sqlx::{AssertSqlSafe, Connection, Executor, PgConnection};

use crate::support::{Fixture, call, refused};

/// A database of its own on the test server, with a read-only role, both
/// dropped by [`Scratch::drop_db`].
struct Scratch {
    admin: String,
    name: String,
    /// The test server's superuser, on this database.
    url: String,
    /// The read-only role, on this database.
    reader_url: String,
    /// The roles made for this database, the reader first.
    roles: Vec<String>,
}

impl Scratch {
    async fn new(test: &str) -> Option<Scratch> {
        let Ok(admin) = std::env::var("NAVREE_TEST_POSTGRES_URL") else {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test}: NAVREE_TEST_REQUIRE_SERVICES=1 but NAVREE_TEST_POSTGRES_URL is not set"
            );
            eprintln!("SKIPPED {test}: set NAVREE_TEST_POSTGRES_URL");
            return None;
        };
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let name = format!("navree_nav_{}_{nanos}", std::process::id());
        let mut conn = PgConnection::connect(&admin).await.expect("connect");
        conn.execute(AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .await
            .unwrap();
        conn.close().await.unwrap();
        let mut url = url::Url::parse(&admin).expect("a URL");
        url.set_path(&format!("/{name}"));
        let mut scratch = Scratch {
            admin,
            name,
            url: url.to_string(),
            reader_url: String::new(),
            roles: Vec::new(),
        };
        scratch.reader_url = scratch.role("reader", &[]).await;
        // Tables the tests create later, as the superuser, are the reader's to read.
        scratch
            .setup(&[format!(
                "ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT ON TABLES TO {}",
                scratch.roles[0]
            )
            .as_str()])
            .await;
        Some(scratch)
    }

    /// A login role for this database, `<database>_<suffix>`, that may
    /// connect and use the public schema, member of `memberships`; its URL.
    async fn role(&mut self, suffix: &str, memberships: &[&str]) -> String {
        let role = format!("{}_{suffix}", self.name);
        let mut conn = PgConnection::connect(&self.admin).await.unwrap();
        conn.execute(AssertSqlSafe(format!(
            "CREATE ROLE {role} LOGIN PASSWORD 'navree-test'"
        )))
        .await
        .unwrap();
        conn.execute(AssertSqlSafe(format!(
            "GRANT CONNECT ON DATABASE {} TO {role}",
            self.name
        )))
        .await
        .unwrap();
        for membership in memberships {
            conn.execute(AssertSqlSafe(format!("GRANT {membership} TO {role}")))
                .await
                .unwrap();
        }
        conn.close().await.unwrap();
        self.roles.push(role.clone());
        self.setup(&[format!("GRANT USAGE ON SCHEMA public TO {role}").as_str()])
            .await;
        let mut url = url::Url::parse(&self.url).expect("a URL");
        url.set_username(&role).unwrap();
        url.set_password(Some("navree-test")).unwrap();
        url.to_string()
    }

    async fn setup(&self, statements: &[&str]) {
        let mut conn = PgConnection::connect(&self.url).await.unwrap();
        for statement in statements {
            conn.execute(AssertSqlSafe(statement.to_string()))
                .await
                .expect(statement);
        }
        conn.close().await.unwrap();
    }

    /// Sets `standard_conforming_strings = off` on this database, so that a
    /// new connection that does not pin it reads backslash escapes in plain
    /// strings, and checks that a new connection does. On a server that no
    /// longer accepts the setting, prints `SKIPPED` and answers false.
    async fn strings_off(&self, test: &str) -> bool {
        let mut conn = PgConnection::connect(&self.url).await.unwrap();
        let altered = conn
            .execute(AssertSqlSafe(format!(
                "ALTER DATABASE {} SET standard_conforming_strings = off",
                self.name
            )))
            .await;
        conn.close().await.unwrap();
        if let Err(error) = altered {
            eprintln!(
                "SKIPPED {test}: the server refuses standard_conforming_strings = off: {error}"
            );
            return false;
        }
        let mut conn = PgConnection::connect(&self.url).await.unwrap();
        let setting: String = sqlx::query_scalar("SHOW standard_conforming_strings")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        conn.close().await.unwrap();
        if setting != "off" {
            eprintln!(
                "SKIPPED {test}: standard_conforming_strings is {setting} on a new connection"
            );
            return false;
        }
        true
    }

    async fn drop_db(self) {
        let mut conn = PgConnection::connect(&self.admin).await.unwrap();
        conn.execute(AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.name
        )))
        .await
        .unwrap();
        for role in &self.roles {
            conn.execute(AssertSqlSafe(format!("DROP ROLE IF EXISTS {role}")))
                .await
                .unwrap();
        }
        conn.close().await.unwrap();
    }

    /// A role `<database>_<suffix>` with `attributes` (`SUPERUSER NOLOGIN`),
    /// dropped with the database; its name.
    async fn bare_role(&mut self, suffix: &str, attributes: &str) -> String {
        let role = format!("{}_{suffix}", self.name);
        let mut conn = PgConnection::connect(&self.admin).await.unwrap();
        conn.execute(AssertSqlSafe(format!("CREATE ROLE {role} {attributes}")))
            .await
            .unwrap();
        conn.close().await.unwrap();
        self.roles.push(role.clone());
        role
    }

    /// An app whose `database.url` is the read-only role's.
    fn fixture(&self) -> Fixture {
        Fixture::new(&format!("[database]\nurl = \"{}\"\n", self.reader_url))
    }

    /// An app whose `database.url` is the superuser's.
    fn admin_fixture(&self) -> Fixture {
        Fixture::new(&format!("[database]\nurl = \"{}\"\n", self.url))
    }

    /// Whether the test server's URL logs in as a superuser.
    async fn admin_is_superuser(&self) -> bool {
        let mut conn = PgConnection::connect(&self.url).await.unwrap();
        let superuser: bool =
            sqlx::query_scalar("SELECT rolsuper FROM pg_roles WHERE rolname = current_user")
                .fetch_one(&mut conn)
                .await
                .unwrap();
        conn.close().await.unwrap();
        superuser
    }
}

const SCHEMA: &[&str] = &[
    "CREATE TABLE companies (id bigserial PRIMARY KEY, name text NOT NULL)",
    "CREATE TABLE deals (id bigserial PRIMARY KEY, title text NOT NULL, stage text NOT NULL DEFAULT 'lead', \
     amount_cents bigint, company_id bigint REFERENCES companies(id) ON DELETE CASCADE)",
    "CREATE INDEX deals_stage ON deals (stage)",
    "CREATE TABLE users (id bigserial PRIMARY KEY, email text NOT NULL, password_hash text)",
    "CREATE TABLE navree_migrations (module text NOT NULL, name text NOT NULL, batch bigint NOT NULL, \
     applied_at timestamptz NOT NULL, PRIMARY KEY (module, name))",
    "INSERT INTO navree_migrations VALUES ('crm', 'm1_create_deals', 1, now())",
    "INSERT INTO companies (name) VALUES ('Agra')",
    "INSERT INTO deals (title, stage, amount_cents, company_id) \
     SELECT 'Deal ' || i, (ARRAY['lead', 'negotiation', 'won'])[1 + i % 3], i * 1000, 1 FROM generate_series(1, 150) i",
    "INSERT INTO users (email, password_hash) VALUES ('anu@example.com', '$argon2id$x')",
];

#[tokio::test]
async fn postgres_queries_run_read_only_with_typed_values() {
    let Some(db) = Scratch::new("postgres_queries_run_read_only_with_typed_values").await else {
        return;
    };
    db.setup(SCHEMA).await;
    let fixture = db.fixture();
    let navigator = fixture.navigator();

    let sums = call(
        &navigator,
        "query_database",
        json!({"sql": "SELECT stage, count(*) AS deals, sum(amount_cents) AS total FROM deals GROUP BY stage ORDER BY stage"}),
    )
    .await;
    assert_eq!(sums["columns"], json!(["stage", "deals", "total"]));
    assert_eq!(sums["rows"][0][1], 50, "{sums}");
    assert!(
        sums["rows"][0][2].is_i64(),
        "numeric sums are numbers: {sums}"
    );

    let many = call(
        &navigator,
        "query_database",
        json!({"sql": "SELECT id FROM deals ORDER BY id"}),
    )
    .await;
    assert_eq!(many["rows"].as_array().unwrap().len(), 100);
    assert_eq!(many["truncated"], true);

    let users = call(
        &navigator,
        "query_database",
        json!({"sql": "SELECT email, password_hash FROM users"}),
    )
    .await;
    assert_eq!(users["rows"], json!([["anu@example.com", "[redacted]"]]));

    let read_only = call(
        &navigator,
        "query_database",
        json!({"sql": "SHOW transaction_read_only"}),
    )
    .await;
    assert_eq!(read_only["rows"], json!([["on"]]));

    let message = refused(
        &navigator,
        "query_database",
        json!({"sql": "WITH d AS (DELETE FROM deals RETURNING id) SELECT * FROM d"}),
    )
    .await;
    assert!(message.contains("changes data"), "{message}");

    let slow = refused(
        &navigator,
        "query_database",
        json!({"sql": "SELECT count(*) FROM generate_series(1, 100000000000)"}),
    )
    .await;
    assert!(slow.contains("longer than 5 seconds"), "{slow}");

    let count = call(
        &navigator,
        "query_database",
        json!({"sql": "SELECT count(*) AS n FROM deals"}),
    )
    .await;
    assert_eq!(count["rows"], json!([[150]]));
    drop(navigator);
    db.drop_db().await;
}

#[tokio::test]
async fn postgres_schema_has_columns_indexes_and_foreign_keys() {
    let Some(db) = Scratch::new("postgres_schema_has_columns_indexes_and_foreign_keys").await
    else {
        return;
    };
    db.setup(SCHEMA).await;
    let fixture = db.fixture();
    let answer = call(
        &fixture.navigator(),
        "describe_schema",
        json!({"table": "deals"}),
    )
    .await;
    assert_eq!(answer["driver"], "postgres");
    let deals = &answer["tables"][0];
    assert_eq!(deals["name"], "deals");
    assert_eq!(deals["columns"][0]["name"], "id");
    assert_eq!(deals["columns"][0]["primary_key"], true);
    assert_eq!(deals["columns"][2]["default"], "'lead'::text");
    let indexes = deals["indexes"].as_array().unwrap();
    assert!(
        indexes
            .iter()
            .any(|i| i["name"] == "deals_stage" && i["columns"] == json!(["stage"])),
        "{indexes:?}"
    );
    assert!(
        indexes
            .iter()
            .any(|i| i["name"] == "deals_pkey" && i["unique"] == true),
        "{indexes:?}"
    );
    assert_eq!(
        deals["foreign_keys"],
        json!([{
            "columns": ["company_id"],
            "references_table": "companies",
            "references_columns": ["id"],
            "on_delete": "CASCADE",
            "on_update": "NO ACTION",
        }])
    );
    assert_eq!(
        answer["migrations"]["applied"],
        json!(["crm/m1_create_deals"])
    );
    db.drop_db().await;
}

/// The table asked for is picked from the catalog's list and never written
/// into the SQL, and the catalog names that are written into it stay string
/// literals, even where a backslash escapes a quote in a plain string
/// (`standard_conforming_strings = off`).
#[tokio::test]
async fn postgres_schema_table_names_never_become_sql() {
    let Some(db) = Scratch::new("postgres_schema_table_names_never_become_sql").await else {
        return;
    };
    db.setup(SCHEMA).await;
    db.setup(&[r#"CREATE TABLE "it\'s" (id bigint PRIMARY KEY)"#])
        .await;
    if !db
        .strings_off("postgres_schema_table_names_never_become_sql")
        .await
    {
        db.drop_db().await;
        return;
    }
    let fixture = db.fixture();
    let navigator = fixture.navigator();

    // Written into the SQL with quotes doubled only, this would close the
    // string and list every table.
    let hostile = refused(
        &navigator,
        "describe_schema",
        json!({"table": r"\' OR 1=1 --"}),
    )
    .await;
    assert!(hostile.contains("no table named"), "{hostile}");

    let odd = call(&navigator, "describe_schema", json!({"table": r"it\'s"})).await;
    assert_eq!(odd["tables"].as_array().map(Vec::len), Some(1), "{odd}");
    assert_eq!(odd["tables"][0]["name"], r"it\'s");
    assert_eq!(odd["tables"][0]["columns"][0]["name"], "id");
    assert_eq!(odd["tables"][0]["columns"][0]["primary_key"], true);

    let all = call(&navigator, "describe_schema", json!({})).await;
    assert_eq!(all["tables"].as_array().map(Vec::len), Some(5), "{all}");
    drop(navigator);
    db.drop_db().await;
}

/// Navigator's connections start with `standard_conforming_strings = on`,
/// which wins over the database's own setting.
#[tokio::test]
async fn postgres_connections_pin_standard_strings() {
    let test = "postgres_connections_pin_standard_strings";
    let Some(db) = Scratch::new(test).await else {
        return;
    };
    db.setup(SCHEMA).await;
    if !db.strings_off(test).await {
        db.drop_db().await;
        return;
    }
    let fixture = db.fixture();
    let navigator = fixture.navigator();
    let setting = call(
        &navigator,
        "query_database",
        json!({"sql": "SHOW standard_conforming_strings"}),
    )
    .await;
    assert_eq!(setting["rows"], json!([["on"]]), "{setting}");
    drop(navigator);
    db.drop_db().await;
}

/// No statement `query_database` runs can change how strings are read.
#[tokio::test]
async fn postgres_queries_cannot_change_the_string_setting() {
    let Some(db) = Scratch::new("postgres_queries_cannot_change_the_string_setting").await else {
        return;
    };
    db.setup(SCHEMA).await;
    let fixture = db.fixture();
    let navigator = fixture.navigator();
    for sql in [
        "SET standard_conforming_strings = off",
        "SET LOCAL standard_conforming_strings TO off",
        "RESET standard_conforming_strings",
        "SELECT set_config('standard_conforming_strings', 'off', false)",
        "SELECT * FROM set_config('standard_conforming_strings', 'off', false)",
        "SELECT * FROM pg_catalog.set_config('standard_conforming_strings', 'off', true) AS s",
    ] {
        refused(&navigator, "query_database", json!({ "sql": sql })).await;
    }
    let setting = call(
        &navigator,
        "query_database",
        json!({"sql": "SHOW standard_conforming_strings"}),
    )
    .await;
    assert_eq!(setting["rows"], json!([["on"]]), "{setting}");
    drop(navigator);
    db.drop_db().await;
}

/// A backslash before a quote, which a server reading backslash escapes
/// would take as the end of the string, stays inside it: the statement the
/// guard checked is the one that runs, and nothing after it does.
#[tokio::test]
async fn postgres_a_backslash_quote_cannot_write() {
    let test = "postgres_a_backslash_quote_cannot_write";
    let Some(db) = Scratch::new(test).await else {
        return;
    };
    db.setup(SCHEMA).await;
    if !db.strings_off(test).await {
        db.drop_db().await;
        return;
    }
    let fixture = db.fixture();
    let navigator = fixture.navigator();
    let answer = call(
        &navigator,
        "query_database",
        json!({"sql": r"SELECT '\' AS a, ' AS b; COMMIT; BEGIN READ WRITE; DROP TABLE users; COMMIT; --'"}),
    )
    .await;
    assert_eq!(answer["rows"][0][0], r"\", "{answer}");
    assert_eq!(
        answer["rows"][0][1], " AS b; COMMIT; BEGIN READ WRITE; DROP TABLE users; COMMIT; --",
        "{answer}"
    );
    drop(navigator);
    let mut conn = PgConnection::connect(&db.url).await.unwrap();
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&mut conn)
        .await
        .expect("the users table is still there");
    assert_eq!(users, 1);
    conn.close().await.unwrap();
    db.drop_db().await;
}

/// A superuser is refused for queries unless Navigator is started with
/// `NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1`; a read-only role is not. Reading
/// the schema runs no query of the agent's and works either way.
#[tokio::test]
async fn postgres_a_superuser_is_refused_unless_allowed() {
    let test = "postgres_a_superuser_is_refused_unless_allowed";
    let Some(db) = Scratch::new(test).await else {
        return;
    };
    db.setup(SCHEMA).await;
    let reader = db.fixture();
    let answer = call(
        &reader.navigator(),
        "query_database",
        json!({"sql": "SELECT count(*) AS n FROM deals"}),
    )
    .await;
    assert_eq!(answer["rows"], json!([[150]]), "a read-only role queries");
    if !db.admin_is_superuser().await {
        eprintln!("SKIPPED part of {test}: NAVREE_TEST_POSTGRES_URL is not a superuser");
        db.drop_db().await;
        return;
    }
    let admin = db.admin_fixture();
    let message = refused(
        &admin.navigator(),
        "query_database",
        json!({"sql": "SELECT 1 AS one"}),
    )
    .await;
    assert!(message.contains("superuser"), "{message}");
    assert!(message.contains("CREATE ROLE navree_reader"), "{message}");
    assert!(
        message.contains("NAVREE_MCP_ALLOW_PRIVILEGED_DATABASE_ROLE=1"),
        "{message}"
    );
    let schema = call(
        &admin.navigator(),
        "describe_schema",
        json!({"table": "deals"}),
    )
    .await;
    assert_eq!(schema["tables"][0]["name"], "deals");
    let allowed = admin
        .navigator()
        .with_database(DatabaseOptions::default().allow_privileged_role(true));
    let answer = call(
        &allowed,
        "query_database",
        json!({"sql": "SELECT 1 AS one"}),
    )
    .await;
    assert_eq!(answer["rows"], json!([[1]]));
    // `NAVREE_MCP_DATABASE_URL` reads through another role than the app's.
    let through_reader = admin
        .navigator()
        .with_database(DatabaseOptions::default().url(db.reader_url.clone()));
    let answer = call(
        &through_reader,
        "query_database",
        json!({"sql": "SELECT current_user LIKE '%reader' AS reader"}),
    )
    .await;
    assert_eq!(answer["rows"], json!([[true]]));
    db.drop_db().await;
}

/// A role that can end other sessions, or read the server's files, is
/// refused like a superuser.
#[tokio::test]
async fn postgres_a_role_with_server_privileges_is_refused() {
    let Some(mut db) = Scratch::new("postgres_a_role_with_server_privileges_is_refused").await
    else {
        return;
    };
    db.setup(SCHEMA).await;
    for (suffix, membership) in [
        ("signal", "pg_signal_backend"),
        ("files", "pg_read_server_files"),
    ] {
        let url = db.role(suffix, &[membership]).await;
        let fixture = Fixture::new(&format!("[database]\nurl = \"{url}\"\n"));
        let message = refused(
            &fixture.navigator(),
            "query_database",
            json!({"sql": "SELECT 1 AS one"}),
        )
        .await;
        assert!(message.contains(membership), "{membership}: {message}");
    }
    db.drop_db().await;
}

/// A role that can become a superuser is refused like one: a member of a
/// superuser role (it can `SET ROLE` to it), and a superuser session whose
/// `role` setting makes `current_user` a lesser role.
#[tokio::test]
async fn postgres_a_role_that_can_become_a_superuser_is_refused() {
    let test = "postgres_a_role_that_can_become_a_superuser_is_refused";
    let Some(mut db) = Scratch::new(test).await else {
        return;
    };
    db.setup(SCHEMA).await;
    if !db.admin_is_superuser().await {
        eprintln!("SKIPPED {test}: NAVREE_TEST_POSTGRES_URL is not a superuser");
        db.drop_db().await;
        return;
    }
    let boss = db.bare_role("boss", "SUPERUSER NOLOGIN").await;
    let member = db.role("member", &[boss.as_str()]).await;
    let fixture = Fixture::new(&format!("[database]\nurl = \"{member}\"\n"));
    let message = refused(
        &fixture.navigator(),
        "query_database",
        json!({"sql": "SELECT 1 AS one"}),
    )
    .await;
    assert!(message.contains("superuser"), "member: {message}");

    let sessions = db.role("session", &[]).await;
    let session_role = db.roles.last().unwrap().clone();
    let reader = db.roles[0].clone();
    let mut conn = PgConnection::connect(&db.admin).await.unwrap();
    for statement in [
        format!("ALTER ROLE {session_role} SUPERUSER"),
        format!("GRANT {reader} TO {session_role}"),
        format!("ALTER ROLE {session_role} SET role = {reader}"),
    ] {
        conn.execute(AssertSqlSafe(statement)).await.unwrap();
    }
    conn.close().await.unwrap();
    let fixture = Fixture::new(&format!("[database]\nurl = \"{sessions}\"\n"));
    let message = refused(
        &fixture.navigator(),
        "query_database",
        json!({"sql": "SELECT 1 AS one"}),
    )
    .await;
    assert!(message.contains("superuser"), "session: {message}");
    assert!(message.contains("(session "), "session: {message}");
    db.drop_db().await;
}

/// A plain role that may execute a SECURITY DEFINER function (here through
/// PUBLIC, as every new function is) is refused, the function named; with
/// EXECUTE revoked, it queries.
#[tokio::test]
async fn postgres_a_role_with_a_security_definer_function_is_refused() {
    let Some(db) =
        Scratch::new("postgres_a_role_with_a_security_definer_function_is_refused").await
    else {
        return;
    };
    db.setup(SCHEMA).await;
    db.setup(&[
        "CREATE FUNCTION public.whoami() RETURNS text LANGUAGE sql SECURITY DEFINER \
         AS $$ SELECT current_user::text $$",
    ])
    .await;
    let fixture = db.fixture();
    let message = refused(
        &fixture.navigator(),
        "query_database",
        json!({"sql": "SELECT 1 AS one"}),
    )
    .await;
    assert!(message.contains("whoami()"), "{message}");
    assert!(message.contains("SECURITY DEFINER"), "{message}");
    let allowed = fixture
        .navigator()
        .with_database(DatabaseOptions::default().allow_privileged_role(true));
    let answer = call(
        &allowed,
        "query_database",
        json!({"sql": "SELECT 1 AS one"}),
    )
    .await;
    assert_eq!(answer["rows"], json!([[1]]));
    db.setup(&["REVOKE EXECUTE ON FUNCTION public.whoami() FROM PUBLIC"])
        .await;
    let answer = call(
        &fixture.navigator(),
        "query_database",
        json!({"sql": "SELECT 1 AS one"}),
    )
    .await;
    assert_eq!(answer["rows"], json!([[1]]));
    db.drop_db().await;
}

/// Each value is cut on the server before it is sent, so a query that makes
/// huge values costs Navigator a few kilobytes per value; the server's own
/// cost ends with the statement timeout.
#[tokio::test]
async fn postgres_long_values_are_cut_on_the_server() {
    let Some(db) = Scratch::new("postgres_long_values_are_cut_on_the_server").await else {
        return;
    };
    db.setup(SCHEMA).await;
    let fixture = db.fixture();
    let navigator = fixture.navigator();
    let answer = call(
        &navigator,
        "query_database",
        json!({"sql": "SELECT repeat('x', 10000000) AS x, 1 AS n, true AS b FROM generate_series(1, 3)"}),
    )
    .await;
    assert_eq!(answer["columns"], json!(["x", "n", "b"]));
    assert_eq!(answer["rows"].as_array().map(Vec::len), Some(3));
    let x = answer["rows"][0][0].as_str().unwrap();
    assert!(x.ends_with("… [cut]"), "{}", x.len());
    assert!(x.len() < 5 * 1024, "{}", x.len());
    assert_eq!(answer["rows"][0][1], 1, "types are kept");
    assert_eq!(answer["rows"][0][2], true);
    // Six values of 900 MB: cut on the server, or stopped by the timeout.
    let huge = navigator
        .call(
            "query_database",
            json!({"sql": "SELECT repeat('x', 900000000) FROM generate_series(1, 6)"}),
        )
        .await;
    match huge {
        Ok(navree_navigator::ToolOutput::Json(answer)) => {
            for row in answer["rows"].as_array().unwrap() {
                assert!(row[0].as_str().unwrap().len() < 5 * 1024);
            }
        }
        Ok(other) => panic!("{other:?}"),
        Err(error) => assert!(
            error.0.contains("longer than") || error.0.contains("memory"),
            "{}",
            error.0
        ),
    }
    // Duplicate column names are read by position.
    let answer = call(
        &navigator,
        "query_database",
        json!({"sql": "SELECT 1 AS a, 2 AS a"}),
    )
    .await;
    assert_eq!(answer["rows"], json!([[1, 2]]));
    db.drop_db().await;
}

/// A SECURITY DEFINER function counts even when the role can't use its
/// schema (a view calls it), and an aggregate's step function counts even
/// when EXECUTE on it is revoked (PostgreSQL checks it against the
/// aggregate's owner).
#[tokio::test]
async fn postgres_functions_reached_through_views_and_aggregates_are_refused() {
    let Some(db) =
        Scratch::new("postgres_functions_reached_through_views_and_aggregates_are_refused").await
    else {
        return;
    };
    db.setup(SCHEMA).await;
    db.setup(&[
        "CREATE SCHEMA private",
        "CREATE FUNCTION private.sd() RETURNS int LANGUAGE sql SECURITY DEFINER AS $$ SELECT 1 $$",
        "CREATE VIEW public.v AS SELECT private.sd() AS one",
    ])
    .await;
    let fixture = db.fixture();
    let message = refused(
        &fixture.navigator(),
        "query_database",
        json!({"sql": "SELECT 1 AS one"}),
    )
    .await;
    assert!(message.contains("private.sd()"), "{message}");

    db.setup(&[
        "DROP VIEW public.v",
        "DROP FUNCTION private.sd()",
        "CREATE FUNCTION public.step(int, int) RETURNS int LANGUAGE sql SECURITY DEFINER \
         AS $$ SELECT $1 + $2 $$",
        "CREATE AGGREGATE public.total(int) (sfunc = public.step, stype = int, initcond = '0')",
        "REVOKE EXECUTE ON FUNCTION public.step(int, int) FROM PUBLIC",
    ])
    .await;
    let message = refused(
        &fixture.navigator(),
        "query_database",
        json!({"sql": "SELECT 1 AS one"}),
    )
    .await;
    assert!(message.contains("step(integer,integer)"), "{message}");
    db.drop_db().await;
}

/// Output PostgreSQL could make unbounded is refused or cut: EXPLAIN (the
/// plan holds folded constants), an error quoting a value, more than 100
/// columns, and a first row whose JSON passes the answer's budget.
#[tokio::test]
async fn postgres_unbounded_output_is_refused_or_cut() {
    let Some(db) = Scratch::new("postgres_unbounded_output_is_refused_or_cut").await else {
        return;
    };
    db.setup(SCHEMA).await;
    let fixture = db.fixture();
    let navigator = fixture.navigator();
    let message = refused(
        &navigator,
        "query_database",
        json!({"sql": "EXPLAIN SELECT * FROM pg_class WHERE relname::text = repeat('x', 300000000)"}),
    )
    .await;
    assert!(message.contains("EXPLAIN is not run"), "{message}");

    let message = refused(
        &navigator,
        "query_database",
        json!({"sql": "SELECT repeat('x', 300000)::int"}),
    )
    .await;
    assert!(message.len() < 4 * 1024, "{}", message.len());
    assert!(message.contains("… [cut]"), "{message}");

    let wide: Vec<String> = (0..150).map(|i| format!("{i} AS c{i}")).collect();
    let message = refused(
        &navigator,
        "query_database",
        json!({ "sql": format!("SELECT {}", wide.join(", ")) }),
    )
    .await;
    assert!(message.contains("150 columns"), "{message}");

    let heavy: Vec<String> = (0..100)
        .map(|i| format!("repeat(chr(1), 5000) AS c{i}"))
        .collect();
    let answer = call(
        &navigator,
        "query_database",
        json!({ "sql": format!("SELECT {}", heavy.join(", ")) }),
    )
    .await;
    assert_eq!(answer["truncated"], true);
    let json = serde_json::to_string(&answer["rows"]).unwrap();
    assert!(json.len() <= 1024 * 1024 + 64 * 1024, "{}", json.len());
    db.drop_db().await;
}

/// Statements that the SQL renderer would write back as several (hex
/// strings, custom type modifiers), or that run SQL text of their own
/// (`ts_stat`), are refused, on a default server, and change nothing.
#[tokio::test]
async fn postgres_statements_that_smuggle_sql_are_refused() {
    let Some(db) = Scratch::new("postgres_statements_that_smuggle_sql_are_refused").await else {
        return;
    };
    db.setup(SCHEMA).await;
    let fixture = db.fixture();
    let navigator = fixture.navigator();
    for sql in [
        "SELECT X'0'', pg_terminate_backend(pid), ''' FROM pg_stat_activity",
        "SELECT X'0'', dblink_exec(''dbname=app'', ''DROP TABLE users''), '''",
        "SELECT X'0''; COMMIT; BEGIN READ WRITE; DROP TABLE users; COMMIT; --'",
        "SELECT CAST(1 AS x('1)); COMMIT; DROP TABLE users; SELECT CAST(1 AS x(1'))",
        "SELECT * FROM ts_stat('SELECT to_tsvector(pg_terminate_backend(pid)::text) FROM pg_stat_activity')",
        "SELECT ts_rewrite(to_tsquery('a'), 'SELECT to_tsquery(''b'')')",
        "SELECT query_to_xml('SELECT 1', true, false, '')",
    ] {
        let message = refused(&navigator, "query_database", json!({ "sql": sql })).await;
        assert!(message.starts_with("refused: "), "{sql}: {message}");
    }
    drop(navigator);
    let mut conn = PgConnection::connect(&db.url).await.unwrap();
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&mut conn)
        .await
        .expect("the users table is still there");
    assert_eq!(users, 1);
    conn.close().await.unwrap();
    db.drop_db().await;
}

/// Lighthouse's tables as its migration creates them on PostgreSQL.
const LIGHTHOUSE: &[&str] = &[
    "CREATE TABLE lighthouse_issues (id uuid PRIMARY KEY, fingerprint varchar NOT NULL UNIQUE, kind varchar NOT NULL, \
     level varchar NOT NULL, status varchar NOT NULL, title varchar NOT NULL, culprit varchar NOT NULL, \
     message text NOT NULL, chain text NOT NULL, location varchar, error_type varchar, count bigint NOT NULL, \
     first_seen timestamptz NOT NULL, last_seen timestamptz NOT NULL, first_release varchar, last_release varchar, \
     environment varchar, resolved_at timestamptz, resolved_in_release varchar, ignored_until timestamptz, \
     ignore_remaining bigint, regressed_at timestamptz, comment text, created_at timestamptz NOT NULL, \
     updated_at timestamptz NOT NULL)",
    "CREATE TABLE lighthouse_occurrences (id uuid PRIMARY KEY, issue_id uuid NOT NULL REFERENCES lighthouse_issues(id), \
     at timestamptz NOT NULL, level varchar NOT NULL, message text NOT NULL, chain text NOT NULL, backtrace text, \
     location varchar, request_id varchar, job_id varchar, trace_id varchar, user_id varchar, workspace_id varchar, \
     release varchar, environment varchar, server varchar, process varchar, method varchar, route varchar, \
     path varchar, status integer, sample_id uuid, context text NOT NULL)",
    "CREATE TABLE lighthouse_aggregates (id uuid PRIMARY KEY, resolution integer NOT NULL, bucket timestamptz NOT NULL, \
     kind varchar NOT NULL, key_hash varchar NOT NULL, key text NOT NULL, process varchar NOT NULL, count bigint NOT NULL, \
     errors bigint NOT NULL, extra bigint NOT NULL, total_ms double precision NOT NULL, max_ms double precision NOT NULL, \
     histogram text NOT NULL, detail text)",
    "CREATE TABLE lighthouse_alerts (id uuid PRIMARY KEY, issue_id uuid, kind varchar NOT NULL, channel varchar NOT NULL, \
     subject_key varchar NOT NULL, status varchar NOT NULL, attempts integer NOT NULL DEFAULT 0, error text, \
     payload text NOT NULL, created_at timestamptz NOT NULL, delivered_at timestamptz)",
    "INSERT INTO lighthouse_issues (id, fingerprint, kind, level, status, title, culprit, message, chain, count, \
     first_seen, last_seen, created_at, updated_at) VALUES ('0192f3a4-b5c6-7d8e-9f00-112233445566', 'f1', 'exception', \
     'error', 'open', 'Database connection failed', 'GET /api/v1/deals', 'Database connection failed', \
     '[\"Database connection failed\"]', 3, now() - interval '2 days', now() - interval '1 hour', now(), now())",
    "INSERT INTO lighthouse_occurrences (id, issue_id, at, level, message, chain, request_id, status, context) VALUES \
     (gen_random_uuid(), '0192f3a4-b5c6-7d8e-9f00-112233445566', now() - interval '1 hour', 'error', 'x', '[]', 'req-1', \
     500, '{\"user_agent\":\"curl\"}')",
    "INSERT INTO lighthouse_aggregates (id, resolution, bucket, kind, key_hash, key, process, count, errors, extra, \
     total_ms, max_ms, histogram) VALUES \
     (gen_random_uuid(), 3600, date_trunc('hour', now()), 'request', 'a', 'GET /api/v1/deals', 'serve-1', 4, 1, 0, 40, 25, '10:2,14:2'), \
     (gen_random_uuid(), 3600, date_trunc('hour', now()) - interval '2 hours', 'request', 'a', 'GET /api/v1/deals', 'serve-2', 4, 0, 0, 40, 25, '10:2,14:2'), \
     (gen_random_uuid(), 3600, date_trunc('hour', now()), 'job', 'b', 'SendInvoice', 'worker', 3, 2, 1, 30, 12, '12:3')",
];

#[tokio::test]
async fn postgres_lighthouse_tools_read_its_tables() {
    let Some(db) = Scratch::new("postgres_lighthouse_tools_read_its_tables").await else {
        return;
    };
    db.setup(LIGHTHOUSE).await;
    let fixture = db.fixture();
    crate::support::write(
        &fixture.root().join("src/app.rs"),
        ".module(LighthouseModule::from_config())",
    );
    let navigator = fixture.navigator();

    let issues = call(&navigator, "lighthouse_list_issues", json!({})).await;
    assert_eq!(
        issues["issues"][0]["id"],
        "0192f3a4-b5c6-7d8e-9f00-112233445566"
    );
    assert_eq!(issues["issues"][0]["count"], 3);

    let issue = call(
        &navigator,
        "lighthouse_get_issue",
        json!({"id": "0192f3a4-b5c6-7d8e-9f00-112233445566"}),
    )
    .await;
    assert_eq!(
        issue["issue"]["chain"],
        json!(["Database connection failed"])
    );
    assert_eq!(issue["occurrences"][0]["request_id"], "req-1");
    assert_eq!(issue["occurrences"][0]["context"]["user_agent"], "curl");

    let routes = call(&navigator, "lighthouse_slow_routes", json!({})).await;
    assert_eq!(routes["routes"][0]["endpoint"], "GET /api/v1/deals");
    assert_eq!(routes["routes"][0]["requests"], 8);
    assert_eq!(routes["routes"][0]["avg_ms"], 10.0);

    let jobs = call(&navigator, "lighthouse_failed_jobs", json!({})).await;
    assert_eq!(jobs["jobs"][0]["failed_jobs"], 1);
    drop(navigator);
    db.drop_db().await;
}
