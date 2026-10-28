//! SQLite end to end: tables created, altered (in place and by rebuild,
//! with the data, indexes, triggers and constraints intact), read back, and
//! the TimescaleDB fallbacks, on a real database file.

use std::sync::Arc;

use navree_schema::{
    ContinuousAggregate, DumpOptions, Hypertable, IndexKind, MigrationContext, ReferentialAction,
    Schema, SchemaConnection, SchemaError, SchemaOptions, raw,
};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

use crate::support::TempSqlite;

async fn exec(conn: &DatabaseConnection, sql: &str) {
    conn.execute_unprepared(sql)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

/// Every row of `sql`, each column read as text (`NULL` as `None`).
async fn rows(conn: &DatabaseConnection, sql: &str) -> Vec<Vec<Option<String>>> {
    let found = conn
        .query_all_raw(Statement::from_string(conn.get_database_backend(), sql))
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
    found
        .iter()
        .map(|row| {
            (0..row.column_names().len())
                .map(|i| {
                    row.try_get_by_index::<Option<String>>(i)
                        .ok()
                        .flatten()
                        .or_else(|| {
                            row.try_get_by_index::<Option<i64>>(i)
                                .ok()
                                .flatten()
                                .map(|n| n.to_string())
                        })
                        .or_else(|| {
                            row.try_get_by_index::<Option<f64>>(i)
                                .ok()
                                .flatten()
                                .map(|n| n.to_string())
                        })
                })
                .collect()
        })
        .collect()
}

fn texts(rows: Vec<Vec<Option<String>>>) -> Vec<Vec<String>> {
    rows.into_iter()
        .map(|row| {
            row.into_iter()
                .map(|c| c.unwrap_or_else(|| "NULL".to_owned()))
                .collect()
        })
        .collect()
}

async fn table_sql(conn: &DatabaseConnection, table: &str) -> String {
    rows(
        conn,
        &format!("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = '{table}'"),
    )
    .await
    .remove(0)
    .remove(0)
    .expect("the table exists")
}

#[tokio::test]
async fn creates_and_reads_back_tables() {
    let db = TempSqlite::new();
    let conn = db.connect().await;
    let schema = Schema::new(&conn);
    schema
        .create("companies", |t| {
            t.id();
            t.string("name").unique();
            t.string("domain").nullable().collation("NOCASE");
            t.timestamps_tz();
        })
        .await
        .unwrap();
    schema
        .create("deals", |t| {
            t.id();
            t.foreign_id("company_id").constrained().cascade_on_delete();
            t.string("title").comment("SQLite keeps no comments");
            t.decimal("amount", 12, 2).default(0);
            t.enumeration("stage", ["lead", "won", "lost"])
                .default("lead");
            t.integer("total").stored_as("amount * 100");
            t.timestamps();
            t.soft_deletes();
            t.index(["stage", "created_at"]);
        })
        .await
        .unwrap();

    assert!(schema.has_table("deals").await.unwrap());
    assert!(schema.has_table("DEALS").await.unwrap(), "case-insensitive");
    assert!(!schema.has_table("missing").await.unwrap());
    assert_eq!(
        schema.table_listing().await.unwrap(),
        ["companies", "deals"],
        "SQLite's own tables (sqlite_sequence) are not listed"
    );
    assert!(schema.has_column("deals", "TITLE").await.unwrap());
    assert!(
        schema
            .has_columns("deals", ["title", "stage", "deleted_at"])
            .await
            .unwrap()
    );
    assert!(
        !schema
            .has_columns("deals", ["title", "nope"])
            .await
            .unwrap()
    );
    assert_eq!(
        schema.column_listing("deals").await.unwrap(),
        [
            "id",
            "company_id",
            "title",
            "amount",
            "stage",
            "total",
            "created_at",
            "updated_at",
            "deleted_at"
        ]
    );
    assert_eq!(
        schema.column_type("deals", "amount", false).await.unwrap(),
        "numeric"
    );
    let columns = schema.columns("deals").await.unwrap();
    let id = &columns[0];
    assert!(id.auto_increment && !id.nullable);
    let total = columns.iter().find(|c| c.name == "total").unwrap();
    let generation = total.generation.as_ref().expect("generated");
    assert!(generation.stored);
    assert_eq!(generation.expression.as_deref(), Some("amount * 100"));
    let stage = columns.iter().find(|c| c.name == "stage").unwrap();
    assert_eq!(stage.default.as_deref(), Some("'lead'"));
    let domain = schema.columns("companies").await.unwrap();
    assert_eq!(domain[2].collation.as_deref(), Some("nocase"));

    assert!(
        schema
            .has_index("deals", ["stage", "created_at"], Some(IndexKind::Index))
            .await
            .unwrap()
    );
    assert!(
        schema
            .has_index(
                "companies",
                "companies_name_unique",
                Some(IndexKind::Unique)
            )
            .await
            .unwrap()
    );
    assert!(
        schema
            .has_index("deals", ["id"], Some(IndexKind::Primary))
            .await
            .unwrap()
    );
    let foreign = schema.foreign_keys("deals").await.unwrap();
    assert_eq!(foreign.len(), 1);
    assert_eq!(foreign[0].columns, ["company_id"]);
    assert_eq!(foreign[0].foreign_table, "companies");
    assert_eq!(foreign[0].foreign_columns, ["id"]);
    assert_eq!(foreign[0].on_delete, Some(ReferentialAction::Cascade));
    assert!(
        schema
            .has_foreign_key("deals", ["company_id"])
            .await
            .unwrap()
    );

    exec(&conn, "INSERT INTO companies (name) VALUES ('Acme')").await;
    exec(
        &conn,
        "INSERT INTO deals (company_id, title) VALUES (1, 'Big one')",
    )
    .await;
    assert_eq!(
        texts(rows(&conn, "SELECT stage, amount, total FROM deals").await),
        [["lead", "0", "0"]]
    );
    let refused = conn
        .execute_unprepared("INSERT INTO deals (company_id, title, stage) VALUES (1, 'x', 'maybe')")
        .await;
    assert!(refused.is_err(), "the enumeration is checked");
}

/// Rows, indexes, triggers and table constraints survive every rebuild.
#[tokio::test]
async fn alterations_keep_the_data() {
    let db = TempSqlite::new();
    let conn = db.connect().await;
    let schema = Schema::new(&conn);
    schema
        .create("products", |t| {
            t.id();
            t.string("sku").unique();
            t.string("name");
            t.integer("price").nullable();
            t.string("legacy").nullable();
            t.index("name").where_raw("price IS NOT NULL");
        })
        .await
        .unwrap();
    exec(
        &conn,
        "CREATE TRIGGER products_touch AFTER UPDATE OF price ON products BEGIN SELECT 1; END",
    )
    .await;
    exec(
        &conn,
        "INSERT INTO products (sku, name, price, legacy) VALUES ('a-1', 'Anvil', 100, 'x'), ('b-2', 'Bell', NULL, 'y')",
    )
    .await;

    schema
        .table("products", |t| {
            // In place: nullable, or with a constant default.
            t.text("notes").nullable();
            t.boolean("active").default(true);
            // Rebuilds: a type change, and NOT NULL with a default computed
            // by the database.
            t.decimal("price", 10, 2).nullable().default(0).change();
            t.timestamp("listed_at").use_current();
            t.rename_column("name", "title");
            t.drop_column("legacy");
        })
        .await
        .unwrap();

    assert_eq!(
        texts(
            rows(
                &conn,
                "SELECT id, sku, title, price, notes, active FROM products ORDER BY id"
            )
            .await
        ),
        [
            ["1", "a-1", "Anvil", "100", "NULL", "1"],
            ["2", "b-2", "Bell", "NULL", "NULL", "1"]
        ],
        "a NULL stays NULL when the column gains a default"
    );
    let listed = rows(&conn, "SELECT listed_at FROM products").await;
    assert!(
        listed.iter().all(|r| r[0].is_some()),
        "CURRENT_TIMESTAMP filled the rows"
    );
    let sql = table_sql(&conn, "products").await;
    assert!(sql.contains("\"price\" NUMERIC DEFAULT 0"), "{sql}");
    assert!(!sql.contains("legacy"), "{sql}");
    let indexes = schema.index_listing("products").await.unwrap();
    assert!(
        indexes.contains(&"products_sku_unique".to_owned()),
        "{indexes:?}"
    );
    assert!(
        indexes.contains(&"products_name_index".to_owned()),
        "{indexes:?}"
    );
    let partial = rows(
        &conn,
        "SELECT sql FROM sqlite_master WHERE name = 'products_name_index'",
    )
    .await;
    assert_eq!(
        partial[0][0].as_deref(),
        Some(
            "CREATE INDEX \"products_name_index\" ON \"products\" (\"title\") WHERE price IS NOT NULL"
        ),
        "the renamed column is renamed in the index too"
    );
    let triggers = rows(
        &conn,
        "SELECT name FROM sqlite_master WHERE type = 'trigger'",
    )
    .await;
    assert_eq!(texts(triggers), [["products_touch"]]);
    let duplicate = conn
        .execute_unprepared("INSERT INTO products (sku, title) VALUES ('a-1', 'Again')")
        .await;
    assert!(duplicate.is_err(), "the unique index survived the rebuild");

    // The new id continues after the existing rows (AUTOINCREMENT kept).
    exec(
        &conn,
        "INSERT INTO products (sku, title) VALUES ('c-3', 'Clamp')",
    )
    .await;
    assert_eq!(
        texts(rows(&conn, "SELECT max(id) FROM products").await),
        [["3"]]
    );
}

/// A drop after a change that rebuilds the table is part of that rebuild.
#[tokio::test]
async fn changing_one_column_and_dropping_another_rebuilds_once() {
    let db = TempSqlite::new();
    let conn = db.connect().await;
    let schema = Schema::new(&conn);
    schema
        .create("companies", |t| {
            t.id();
            t.text("name");
            t.string("domain").nullable();
        })
        .await
        .unwrap();
    exec(
        &conn,
        "INSERT INTO companies (name, domain) VALUES ('Acme', 'acme.test')",
    )
    .await;
    let before = schema.statements().len();
    schema
        .table("companies", |t| {
            t.string("name").change();
            t.drop_column("domain");
        })
        .await
        .unwrap();
    let ran = &schema.statements()[before..];
    assert!(
        ran[0].starts_with("CREATE TABLE \"__temp__companies\""),
        "{ran:?}"
    );
    assert!(
        !ran.iter().any(|s| s.contains("DROP COLUMN")),
        "the rebuild leaves the column out: {ran:?}"
    );
    assert_eq!(
        schema.column_listing("companies").await.unwrap(),
        ["id", "name"]
    );
    assert_eq!(
        texts(rows(&conn, "SELECT name FROM companies").await),
        [["Acme"]]
    );
}

#[tokio::test]
async fn adding_a_required_column_to_a_table_with_rows_says_how_to_fix_it() {
    let db = TempSqlite::new();
    let conn = db.connect().await;
    let schema = Schema::new(&conn);
    schema
        .create("notes", |t| {
            t.id();
            t.text("body");
        })
        .await
        .unwrap();
    exec(&conn, "INSERT INTO notes (body) VALUES ('hello')").await;
    let error = schema
        .table("notes", |t| {
            t.string("title");
        })
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid definition of `notes`: column `title` is NOT NULL without a default, and the table has rows: give it a default or make it nullable"
    );
    assert_eq!(
        texts(rows(&conn, "SELECT body FROM notes").await),
        [["hello"]]
    );
}

#[tokio::test]
async fn keys_and_foreign_keys_change_by_rebuild() {
    let db = TempSqlite::new();
    // A migrator connection: foreign keys off while tables are rebuilt.
    let conn = db.connect_with_foreign_keys(false).await;
    let schema = Schema::new(&conn);
    schema
        .create("authors", |t| {
            t.id();
            t.string("name");
        })
        .await
        .unwrap();
    schema
        .create("books", |t| {
            t.uuid("id");
            t.unsigned_big_integer("author_id");
            t.string("title");
        })
        .await
        .unwrap();
    exec(&conn, "INSERT INTO authors (name) VALUES ('Ann')").await;
    exec(
        &conn,
        "INSERT INTO books (id, author_id, title) VALUES ('b1', 1, 'First')",
    )
    .await;

    schema
        .table("books", |t| {
            t.primary("id");
            t.foreign("author_id")
                .references("id")
                .on("authors")
                .cascade_on_delete();
        })
        .await
        .unwrap();
    assert!(
        schema
            .has_index("books", ["id"], Some(IndexKind::Primary))
            .await
            .unwrap()
    );
    assert!(
        schema
            .has_foreign_key("books", ["author_id"])
            .await
            .unwrap()
    );
    assert_eq!(
        texts(rows(&conn, "SELECT title FROM books").await),
        [["First"]]
    );

    // Dropping a referenced table's column by rebuild keeps the children.
    schema
        .table("authors", |t| {
            t.string("name").length(100).nullable().change();
        })
        .await
        .unwrap();
    assert_eq!(
        texts(rows(&conn, "SELECT count(*) FROM books").await),
        [["1"]]
    );
    assert!(
        rows(&conn, "PRAGMA foreign_key_check").await.is_empty(),
        "no dangling references"
    );

    schema
        .table("books", |t| {
            t.drop_foreign(["author_id"]);
            t.drop_primary();
        })
        .await
        .unwrap();
    assert!(schema.foreign_keys("books").await.unwrap().is_empty());
    assert!(
        !schema
            .has_index("books", ["id"], Some(IndexKind::Primary))
            .await
            .unwrap()
    );
    let error = schema
        .table("books", |t| t.drop_foreign("books_author_id_foreign"))
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("SQLite foreign keys have no names"),
        "{error}"
    );
}

#[tokio::test]
async fn rebuilding_a_referenced_table_with_foreign_keys_on_is_refused() {
    let db = TempSqlite::new();
    let conn = db.connect().await;
    let schema = Schema::new(&conn);
    schema
        .create("parents", |t| {
            t.id();
            t.string("name");
        })
        .await
        .unwrap();
    schema
        .create("children", |t| {
            t.id();
            t.foreign_id("parent_id").constrained().cascade_on_delete();
        })
        .await
        .unwrap();
    exec(&conn, "INSERT INTO parents (name) VALUES ('p')").await;
    exec(&conn, "INSERT INTO children (parent_id) VALUES (1)").await;
    let error = schema
        .table("parents", |t| {
            t.text("name").change();
        })
        .await
        .unwrap_err();
    assert!(
        matches!(error, SchemaError::Unsupported { .. }),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "SQLite does not support rebuilding `parents` while foreign keys are on and children references it: dropping the old table would run their ON DELETE actions; run the change through the migrator (it migrates SQLite with foreign keys off and checks them before committing), or turn `PRAGMA foreign_keys` off outside a transaction"
    );
    assert_eq!(
        texts(rows(&conn, "SELECT count(*) FROM children").await),
        [["1"]],
        "nothing was deleted"
    );
}

/// CHECK constraints: created with the table, then added and dropped by
/// rebuilds that keep the rows.
#[tokio::test]
async fn check_constraints_are_enforced_and_rebuilt() {
    let db = TempSqlite::new();
    let conn = db.connect().await;
    let schema = Schema::new(&conn);
    schema
        .create("templates", |t| {
            t.id();
            t.string("scope");
            t.integer("workspace_id").nullable();
            t.integer("position").default(0);
            t.check(
                "templates_owner_check",
                "(scope = 'global' AND workspace_id IS NULL) OR (scope = 'workspace' AND workspace_id IS NOT NULL)",
            );
        })
        .await
        .unwrap();
    assert!(
        table_sql(&conn, "templates")
            .await
            .contains("CONSTRAINT \"templates_owner_check\" CHECK ((scope = 'global'"),
        "{}",
        table_sql(&conn, "templates").await
    );
    exec(
        &conn,
        "INSERT INTO templates (scope, workspace_id, position) VALUES ('global', NULL, 1), ('workspace', 7, -1)",
    )
    .await;
    let refused = conn
        .execute_unprepared("INSERT INTO templates (scope, workspace_id) VALUES ('global', 7)")
        .await;
    assert!(
        refused
            .unwrap_err()
            .to_string()
            .contains("CHECK constraint failed"),
        "a global row with a workspace is refused"
    );

    // Adding a check rebuilds the table: the rows and the first check stay.
    // A row that violates it fails the rebuild (in the migration's
    // transaction, which rolls back).
    let tx = sea_orm::TransactionTrait::begin(&conn).await.unwrap();
    let refused = Schema::new(&tx)
        .table("templates", |t| {
            t.check("templates_position_check", "position >= 0");
        })
        .await;
    assert!(
        refused.is_err(),
        "the existing row with position -1 violates the new check"
    );
    tx.rollback().await.unwrap();
    exec(
        &conn,
        "UPDATE templates SET position = 0 WHERE position < 0",
    )
    .await;
    schema
        .table("templates", |t| {
            t.check("templates_position_check", "position >= 0");
        })
        .await
        .unwrap();
    assert_eq!(
        texts(
            rows(
                &conn,
                "SELECT scope, workspace_id, position FROM templates ORDER BY id"
            )
            .await
        ),
        [["global", "NULL", "1"], ["workspace", "7", "0"]]
    );
    for (sql, check) in [
        (
            "INSERT INTO templates (scope, workspace_id, position) VALUES ('global', NULL, -5)",
            "templates_position_check",
        ),
        (
            "INSERT INTO templates (scope, workspace_id, position) VALUES ('workspace', NULL, 0)",
            "templates_owner_check",
        ),
    ] {
        let error = conn.execute_unprepared(sql).await.unwrap_err().to_string();
        assert!(
            error.contains("CHECK constraint failed"),
            "{check}: {error}"
        );
    }

    // Dropping one keeps the other.
    schema
        .table("templates", |t| t.drop_check("templates_owner_check"))
        .await
        .unwrap();
    exec(
        &conn,
        "INSERT INTO templates (scope, workspace_id, position) VALUES ('global', 9, 0)",
    )
    .await;
    assert!(
        conn.execute_unprepared(
            "INSERT INTO templates (scope, workspace_id, position) VALUES ('global', NULL, -1)"
        )
        .await
        .is_err()
    );
    let missing = schema
        .table("templates", |t| t.drop_check("templates_owner_check"))
        .await
        .unwrap_err();
    assert!(
        missing
            .to_string()
            .contains("`templates` has no CHECK constraint `templates_owner_check`"),
        "{missing}"
    );
}

#[tokio::test]
async fn renames_drops_and_indexes() {
    let db = TempSqlite::new();
    let conn = db.connect().await;
    let schema = Schema::new(&conn);
    schema
        .create("people", |t| {
            t.id();
            t.string("email");
            t.string("name");
            t.unique("email");
            t.index("name");
        })
        .await
        .unwrap();
    schema
        .table("people", |t| {
            t.rename_index("people_name_index", "people_by_name");
            t.drop_unique(["email"]);
        })
        .await
        .unwrap();
    assert_eq!(
        schema.index_listing("people").await.unwrap(),
        ["primary", "people_by_name"]
    );
    schema.rename("people", "contacts").await.unwrap();
    assert!(schema.has_table("contacts").await.unwrap());
    assert!(!schema.has_table("people").await.unwrap());
    schema
        .when_table_has_column("contacts", "name", |t| t.drop_column("name"))
        .await
        .unwrap();
    schema
        .when_table_doesnt_have_column("contacts", "phone", |t| {
            t.string("phone").nullable();
        })
        .await
        .unwrap();
    assert_eq!(
        schema.column_listing("contacts").await.unwrap(),
        ["id", "email", "phone"]
    );
    schema.drop("contacts").await.unwrap();
    schema.drop_if_exists("contacts").await.unwrap();
    assert!(schema.table_listing().await.unwrap().is_empty());
}

#[tokio::test]
async fn hypertables_are_indexed_tables_on_sqlite() {
    let db = TempSqlite::new();
    let conn = db.connect().await;
    let schema = Schema::new(&conn);
    schema
        .create_hypertable("events", "occurred_at", |t| {
            t.uuid_primary();
            t.string("kind");
            t.integer("amount").default(0);
            t.timestamp_tz("occurred_at");
            t.hypertable("occurred_at").retain_for("90 days");
        })
        .await
        .unwrap();
    assert_eq!(
        schema.notices(),
        [
            "the primary key of `events` is (id, occurred_at), not (id): a hypertable's unique keys must include its time column",
            "SQLite does not support TimescaleDB: `events` is a regular table with an index on `occurred_at`; its retention (90 days) policy is skipped"
        ]
    );
    assert!(
        schema
            .has_index("events", ["occurred_at"], None)
            .await
            .unwrap()
    );

    exec(
        &conn,
        "INSERT INTO events (id, kind, amount, occurred_at) VALUES \
         ('a', 'sale', 5, '2026-10-26 10:00:00'), ('b', 'sale', 7, '2026-10-26 23:00:00'), \
         ('c', 'refund', 2, '2026-10-27 01:00:00')",
    )
    .await;
    schema
        .create_continuous_aggregate(
            ContinuousAggregate::over("events_daily", "events")
                .time_bucket("1 day", "occurred_at", "day")
                .group_by("kind")
                .aggregate("count(*)", "total")
                .aggregate("sum(amount)", "amount"),
        )
        .await
        .unwrap();
    assert!(schema.has_view("events_daily").await.unwrap());
    assert_eq!(
        texts(
            rows(
                &conn,
                "SELECT day, kind, total, amount FROM events_daily ORDER BY day, kind"
            )
            .await
        ),
        [
            ["2026-10-26 00:00:00", "sale", "2", "12"],
            ["2026-10-27 00:00:00", "refund", "1", "2"]
        ]
    );
    schema
        .drop_continuous_aggregate("events_daily")
        .await
        .unwrap();
    assert!(!schema.has_view("events_daily").await.unwrap());

    // Converting an existing table adds the index it lacks.
    schema
        .create("audit_logs", |t| {
            t.id();
            t.string("action");
            t.timestamp("created_at");
        })
        .await
        .unwrap();
    exec(
        &conn,
        "INSERT INTO audit_logs (action, created_at) VALUES ('login', '2026-10-27 08:00:00')",
    )
    .await;
    schema
        .convert_to_hypertable(
            "audit_logs",
            Hypertable::on("created_at")
                .chunk_interval("1 day")
                .migrate_data(),
        )
        .await
        .unwrap();
    assert!(
        schema
            .has_index("audit_logs", ["created_at"], None)
            .await
            .unwrap()
    );
    assert_eq!(
        schema.notices().last().map(String::as_str),
        Some(
            "SQLite does not support TimescaleDB: `audit_logs` is a regular table with an index on `created_at`"
        )
    );
    // Converting again finds the index and adds nothing.
    let before = schema.statements().len();
    schema
        .convert_to_hypertable("audit_logs", "created_at")
        .await
        .unwrap();
    assert_eq!(schema.statements().len(), before);
    // Without TimescaleDB, nothing is a hypertable.
    assert!(!schema.is_hypertable("audit_logs").await.unwrap());
    assert!(!schema.is_hypertable("events").await.unwrap());

    // Declaring the hypertable while altering a table converts it too.
    schema
        .create("readings", |t| {
            t.id();
            t.timestamp("taken_at");
        })
        .await
        .unwrap();
    schema
        .table("readings", |t| {
            t.double("value").nullable();
            t.hypertable("taken_at");
        })
        .await
        .unwrap();
    assert!(schema.has_column("readings", "value").await.unwrap());
    assert!(
        schema
            .has_index("readings", ["taken_at"], None)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn pretending_reads_the_database_and_changes_nothing() {
    let db = TempSqlite::new();
    let conn = db.connect().await;
    Schema::new(&conn)
        .create("notes", |t| {
            t.id();
            t.string("body");
        })
        .await
        .unwrap();

    let schema = Schema::new(&conn).pretending();
    assert!(schema.is_connected() && schema.is_pretending());
    schema
        .table("notes", |t| {
            t.text("body").change();
            t.string("title").nullable();
        })
        .await
        .unwrap();
    let planned = schema.statements();
    assert!(
        planned[0].starts_with("CREATE TABLE \"__temp__notes\""),
        "the rebuild was planned from the real table: {planned:?}"
    );
    assert_eq!(
        planned.last().map(String::as_str),
        Some("ALTER TABLE \"notes\" ADD COLUMN \"title\" VARCHAR")
    );
    assert_eq!(
        Schema::new(&conn).column_listing("notes").await.unwrap(),
        ["id", "body"],
        "nothing ran"
    );

    // The migrator pretends through the migration context: reads go to its
    // reader, statements are recorded in the context.
    let reader: Arc<dyn SchemaConnection> = Arc::new(conn.clone());
    let cx =
        MigrationContext::pretending("notes/m0002_drop_body", SchemaOptions::default(), reader);
    cx.clone()
        .scope(async {
            let schema = Schema::new(&conn);
            assert!(schema.is_pretending());
            assert!(schema.has_column("notes", "body").await?);
            schema.table("notes", |t| t.drop_column("body")).await
        })
        .await
        .unwrap();
    assert_eq!(
        cx.statements(),
        ["ALTER TABLE \"notes\" DROP COLUMN \"body\""]
    );
    assert!(
        Schema::new(&conn)
            .has_column("notes", "body")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn dumps_load_into_an_empty_database_and_dump_the_same() {
    let source = TempSqlite::new();
    let conn = source.connect().await;
    let schema = Schema::new(&conn);
    schema
        .create("companies", |t| {
            t.id();
            t.string("name").unique();
        })
        .await
        .unwrap();
    schema
        .create("deals", |t| {
            t.uuid_primary();
            t.foreign_id("company_id").constrained().cascade_on_delete();
            t.string("title");
            t.integer("cents").stored_as("1 * 100");
            t.index("title").where_raw("title <> ''");
        })
        .await
        .unwrap();
    schema
        .create("navree_migrations", |t| {
            t.string("module");
            t.string("name");
            t.big_integer("batch");
            t.primary(["module", "name"]);
        })
        .await
        .unwrap();
    exec(
        &conn,
        "CREATE VIEW big_deals AS SELECT title FROM deals WHERE cents > 100;\n\
         CREATE TRIGGER deals_stamp AFTER INSERT ON deals BEGIN UPDATE deals SET title = CASE WHEN new.title = '' THEN 'untitled' ELSE new.title END WHERE id = new.id; END;\n\
         INSERT INTO companies (name) VALUES ('Acme ''Corp''');\n\
         INSERT INTO navree_migrations VALUES ('crm', 'm0002_create_deals', 1), ('crm', 'm0001_create_companies', 1);",
    )
    .await;

    let dump = schema
        .dump(&DumpOptions::default().with_data("navree_migrations"))
        .await
        .unwrap();
    insta::assert_snapshot!(dump);

    let target = TempSqlite::new();
    let fresh = target.connect().await;
    let loaded = Schema::new(&fresh).load(&dump).await.unwrap();
    assert_eq!(
        loaded, 9,
        "tables, indexes, the view, the trigger and two rows"
    );
    let again = Schema::new(&fresh)
        .dump(&DumpOptions::default().with_data("navree_migrations"))
        .await
        .unwrap();
    assert_eq!(
        again, dump,
        "loading a dump and dumping again gives the same script"
    );
    assert_eq!(
        texts(
            rows(
                &fresh,
                "SELECT module, name, batch FROM navree_migrations ORDER BY name"
            )
            .await
        ),
        [
            ["crm", "m0001_create_companies", "1"],
            ["crm", "m0002_create_deals", "1"]
        ]
    );
    assert!(
        rows(&fresh, "SELECT * FROM companies").await.is_empty(),
        "only the named tables' rows are dumped"
    );
}

#[tokio::test]
async fn database_wide_operations() {
    let db = TempSqlite::new();
    let conn = db.connect_with_foreign_keys(false).await;
    let schema = Schema::new(&conn);
    for name in ["a", "b"] {
        schema
            .create(name, |t| {
                t.id();
                t.string("label").default(raw("'x'"));
            })
            .await
            .unwrap();
    }
    exec(&conn, "CREATE VIEW labels AS SELECT label FROM a").await;
    assert_eq!(schema.views().await.unwrap()[0].name, "labels");
    schema.drop_all_views().await.unwrap();
    assert!(schema.views().await.unwrap().is_empty());
    schema.drop_all_tables().await.unwrap();
    assert!(schema.table_listing().await.unwrap().is_empty());
    let error = schema.drop_all_types().await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "SQLite does not support dropping user-defined types: types are a PostgreSQL feature"
    );
}
