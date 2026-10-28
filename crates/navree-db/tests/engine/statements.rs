//! Statements: portable placeholders, native SQL, scripts, inserted keys,
//! `RETURNING`, and classified errors.

use navree_db::{Backend, ErrorKind, Executor, ExecutorExt, Statement, Value, map_db_error};

use crate::support::{Scratch, script, serial};

fn schema(backend: Backend) -> String {
    format!(
        "CREATE TABLE companies (id {serial}, name VARCHAR(100) NOT NULL UNIQUE, country VARCHAR(2) NULL);
         CREATE TABLE deals (
            id {serial},
            company_id BIGINT NOT NULL,
            title VARCHAR(200) NOT NULL,
            amount BIGINT NOT NULL CHECK (amount >= 0),
            FOREIGN KEY (company_id) REFERENCES companies (id)
         )",
        serial = serial(backend)
    )
}

/// `?` runs everywhere; native SQL runs on its backend; scripts run
/// several statements; inserted keys come back.
pub async fn placeholders_scripts_and_keys(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.server.backend();
    // A script: several statements in one call, nothing bound.
    let created = db
        .execute_script(&schema(backend).replace(";\n", ";"))
        .await;
    created.unwrap_or_else(|error| panic!("the schema script: {error}"));

    let insert = Statement::portable(
        "INSERT INTO companies (name, country) VALUES (?, ?)",
        ["Acme".into(), Value::Null(navree_db::Kind::String)],
    );
    let done = db.execute(&insert).await.unwrap();
    assert_eq!(done.rows_affected, 1);
    match backend {
        Backend::Postgres => assert_eq!(done.last_insert_id, None),
        Backend::MySql | Backend::Sqlite => assert_eq!(done.last_insert_id, Some(1)),
    }
    if backend.supports_returning() {
        let row = db
            .fetch_one(&Statement::portable(
                "INSERT INTO companies (name, country) VALUES (?, ?) RETURNING id, name",
                ["Globex".into(), "DE".into()],
            ))
            .await
            .unwrap();
        assert_eq!(row.get::<i64>("id").unwrap(), 2);
        assert_eq!(row.get::<String>("name").unwrap(), "Globex");
    } else {
        db.execute(&Statement::portable(
            "INSERT INTO companies (name, country) VALUES (?, ?)",
            ["Globex".into(), "DE".into()],
        ))
        .await
        .unwrap();
    }

    // Native SQL for this backend; a native statement of another backend
    // is refused before it runs.
    let native = match backend {
        Backend::Postgres => Statement::new(
            backend,
            "SELECT name FROM companies WHERE id = $1",
            [2_i64.into()],
        ),
        _ => Statement::new(
            backend,
            "SELECT name FROM companies WHERE id = ?",
            [2_i64.into()],
        ),
    };
    let name: String = db.fetch_scalar(&native).await.unwrap();
    assert_eq!(name, "Globex");
    let other = if backend == Backend::Postgres {
        Backend::MySql
    } else {
        Backend::Postgres
    };
    let error = db
        .fetch_all(&Statement::new(other, "SELECT 1", []))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidStatement);

    // Quoted question marks are text, not placeholders.
    let concat = match backend {
        Backend::MySql => "SELECT CONCAT('why?', ?)",
        Backend::Postgres | Backend::Sqlite => "SELECT 'why?' || ?",
    };
    let quoted: String = db
        .fetch_scalar(&Statement::portable(concat, ["!".into()]))
        .await
        .unwrap();
    assert_eq!(quoted, "why?!");
    let mismatch = db
        .fetch_all(&Statement::portable("SELECT ?", []))
        .await
        .unwrap_err();
    assert_eq!(mismatch.kind(), ErrorKind::InvalidStatement);

    if backend == Backend::Postgres {
        // `??` is PostgreSQL's `?` JSON operator.
        let has: bool = db
            .fetch_scalar(&Statement::portable(
                "SELECT CAST(? AS jsonb) ?? 'stage'",
                [Value::Json(serde_json::json!({"stage": "won"}))],
            ))
            .await
            .unwrap();
        assert!(has);
    }

    // `fetch_optional` and `fetch_one` on no rows.
    let none = db
        .fetch_optional(&Statement::portable(
            "SELECT id FROM companies WHERE name = ?",
            ["Nobody".into()],
        ))
        .await
        .unwrap();
    assert!(none.is_none());
    let missing = db
        .fetch_one(&Statement::portable(
            "SELECT id FROM companies WHERE name = ?",
            ["Nobody".into()],
        ))
        .await
        .unwrap_err();
    assert!(missing.is_not_found());
}

/// Errors are classified the same way on every backend, and map to the
/// same HTTP responses as SeaORM's.
pub async fn errors_are_classified(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.server.backend();
    script(&db, &schema(backend)).await;
    let add_company =
        |name: &str| Statement::portable("INSERT INTO companies (name) VALUES (?)", [name.into()]);
    db.execute(&add_company("Acme")).await.unwrap();

    let unique = db.execute(&add_company("Acme")).await.unwrap_err();
    assert_eq!(unique.kind(), ErrorKind::UniqueViolation, "{unique:?}");
    assert!(unique.is_unique_violation());
    assert!(unique.code().is_some());
    let http = map_db_error::<sea_orm::DbErr>(&unique.clone().into()).expect("a 409");
    assert_eq!(http.status.as_u16(), 409);

    let foreign = db
        .execute(&Statement::portable(
            "INSERT INTO deals (company_id, title, amount) VALUES (?, ?, ?)",
            [999_i64.into(), "x".into(), 1_i64.into()],
        ))
        .await
        .unwrap_err();
    assert_eq!(
        foreign.kind(),
        ErrorKind::ForeignKeyViolation,
        "{foreign:?}"
    );
    assert_eq!(
        map_db_error::<sea_orm::DbErr>(&foreign.into())
            .unwrap()
            .status
            .as_u16(),
        409
    );

    let not_null = db
        .execute(&Statement::portable(
            "INSERT INTO deals (company_id, title, amount) VALUES (?, ?, ?)",
            [
                1_i64.into(),
                Value::Null(navree_db::Kind::String),
                1_i64.into(),
            ],
        ))
        .await
        .unwrap_err();
    assert_eq!(not_null.kind(), ErrorKind::NotNullViolation, "{not_null:?}");

    let check = db
        .execute(&Statement::portable(
            "INSERT INTO deals (company_id, title, amount) VALUES (?, ?, ?)",
            [1_i64.into(), "negative".into(), (-5_i64).into()],
        ))
        .await
        .unwrap_err();
    assert_eq!(check.kind(), ErrorKind::CheckViolation, "{check:?}");

    let syntax = db.execute(&Statement::text("SELEKT 1")).await.unwrap_err();
    assert_eq!(syntax.kind(), ErrorKind::Database, "{syntax:?}");
    assert!(
        map_db_error::<sea_orm::DbErr>(&syntax.into()).is_none(),
        "a 500"
    );

    let not_found = db
        .fetch_one(&Statement::text("SELECT id FROM companies WHERE 1 = 0"))
        .await
        .unwrap_err();
    assert_eq!(
        map_db_error::<sea_orm::DbErr>(&not_found.into())
            .unwrap()
            .status
            .as_u16(),
        404
    );
}
