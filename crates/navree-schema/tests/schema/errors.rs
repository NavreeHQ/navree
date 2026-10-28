//! Blueprints no database would accept fail before any SQL runs, with a
//! message that says how to fix them.

use navree_schema::{Blueprint, Schema, SchemaError, Target};

async fn create_error(define: impl FnOnce(&mut Blueprint)) -> String {
    let schema = Schema::offline(Target::postgres(17));
    let error = schema.create("things", define).await.unwrap_err();
    assert!(schema.statements().is_empty(), "nothing ran");
    error.to_string()
}

#[tokio::test]
async fn modifiers_that_do_not_apply() {
    assert_eq!(
        create_error(|t| {
            t.integer("count").length(10);
        })
        .await,
        "invalid definition of `things`: `length` does not apply to integer column `count`"
    );
    assert_eq!(
        create_error(|t| {
            t.string("name").auto_increment();
        })
        .await,
        "invalid definition of `things`: `auto_increment` does not apply to string column `name`"
    );
    assert_eq!(
        create_error(|t| {
            t.boolean("flag").use_current();
        })
        .await,
        "invalid definition of `things`: `use_current` does not apply to boolean column `flag`"
    );
}

#[tokio::test]
async fn conflicting_definitions() {
    assert_eq!(
        create_error(|t| {
            t.integer("a");
            t.integer("a");
        })
        .await,
        "invalid definition of `things`: column `a` is declared twice"
    );
    assert_eq!(
        create_error(|t| {
            t.integer("total").virtual_as("1").stored_as("2");
        })
        .await,
        "invalid definition of `things`: column `total` is both `virtual_as` and `stored_as`; pick one"
    );
    assert_eq!(
        create_error(|t| {
            t.integer("total").stored_as("1").default(3);
        })
        .await,
        "invalid definition of `things`: generated column `total` cannot have a default"
    );
    assert_eq!(
        create_error(|t| {
            t.uuid("a").primary();
            t.uuid("b").primary();
        })
        .await,
        "invalid definition of `things`: more than one primary key is declared; list every key column in one `primary([...])`"
    );
    assert_eq!(
        create_error(|t| {
            t.enumeration("stage", Vec::<String>::new());
        })
        .await,
        "invalid definition of `things`: enumeration column `stage` needs at least one value"
    );
    assert_eq!(
        create_error(|t| {
            t.decimal("amount", 2, 4);
        })
        .await,
        "invalid definition of `things`: decimal column `amount` needs 1 or more digits and a scale no larger than them (got 2, 4)"
    );
    assert_eq!(
        create_error(|t| {
            t.timestamp("at").precision(9);
        })
        .await,
        "invalid definition of `things`: column `at` asks for 9 fractional-second digits; databases keep at most 6"
    );
    assert_eq!(
        create_error(|t| {
            t.double("ratio").default(f64::NAN);
        })
        .await,
        "invalid definition of `things`: column `ratio` has a non-finite default (NaN); use a raw expression"
    );
    assert_eq!(
        create_error(|_| {}).await,
        "invalid definition of `things`: a table needs at least one column"
    );
}

#[tokio::test]
async fn incomplete_foreign_keys() {
    assert_eq!(
        create_error(|t| {
            t.uuid("owner_id");
            t.foreign("owner_id");
        })
        .await,
        "invalid definition of `things`: foreign key `things_owner_id_foreign` on (owner_id) names no table: add `.on(\"table\")` (or use `constrained()`)"
    );
    assert_eq!(
        create_error(|t| {
            t.uuid("owner_id");
            t.foreign("owner_id").on("users");
        })
        .await,
        "invalid definition of `things`: foreign key `things_owner_id_foreign` on (owner_id) names no referenced column: add `.references(\"id\")`"
    );
    assert_eq!(
        create_error(|t| {
            t.uuid("a");
            t.uuid("b");
            t.foreign(["a", "b"]).references("id").on("pairs");
        })
        .await,
        "invalid definition of `things`: foreign key `things_a_b_foreign` has 2 column(s) but references 1"
    );
}

#[tokio::test]
async fn unsupported_features_name_the_database_and_the_way_out() {
    let schema = Schema::offline(Target::sqlite((3, 46)));
    let error = schema
        .create("posts", |t| {
            t.text("body");
            t.fulltext("body");
        })
        .await
        .unwrap_err();
    assert!(matches!(error, SchemaError::Unsupported { .. }));
    assert_eq!(
        error.to_string(),
        "SQLite does not support full-text indexes (index `posts_body_fulltext`): use an FTS5 virtual table, or PostgreSQL or MySQL"
    );

    let schema = Schema::offline(Target::mysql((8, 4)));
    let error = schema
        .create("posts", |t| {
            t.text("body");
            t.index("body").where_raw("body IS NOT NULL");
        })
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "MySQL does not support partial indexes (index `posts_body_index`): index every row, or add a generated column holding the condition and index it"
    );
}

#[tokio::test]
async fn database_errors_name_the_statement_and_keep_the_driver_error() {
    let db = crate::support::TempSqlite::new();
    let conn = db.connect().await;
    let error = Schema::new(&conn).drop("missing").await.unwrap_err();
    let SchemaError::Database { statement, source } = &error else {
        panic!("a database error: {error:?}");
    };
    assert_eq!(statement, "DROP TABLE \"missing\"");
    assert!(
        source.downcast_ref::<sqlx::Error>().is_some(),
        "the connection's own error stays reachable: {source:?}"
    );
    assert!(
        error
            .to_string()
            .ends_with("\n  while running: DROP TABLE \"missing\""),
        "{error}"
    );
}
