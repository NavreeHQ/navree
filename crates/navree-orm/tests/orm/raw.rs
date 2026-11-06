//! Raw SQL: `select`, `select_one`, `scalar`, `statement`,
//! `affecting_statement` and `unprepared`, and raw fragments in builders.

use navree_orm::{
    Model, OrmError, Value, affecting_statement, raw, scalar, select, select_one, statement, table,
    unprepared,
};

use crate::models::Company;
use crate::support::sqlite;

async fn seed(db: &navree_db::Db) {
    for (name, employees) in [("Agra", 5), ("Gulati", 50), ("Indrani", 15)] {
        Company::create(
            db,
            Company {
                name: name.into(),
                employees,
                ..Company::default()
            },
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn raw_selects_return_rows_one_row_or_one_value() {
    let t = sqlite().await;
    seed(&t.db).await;
    let rows = select(
        &t.db,
        raw(
            "SELECT name, employees FROM companies WHERE employees > ? ORDER BY employees DESC",
            [10],
        ),
    )
    .await
    .unwrap();
    let names: Vec<String> = rows
        .iter()
        .map(|row| row.get::<String>("name").unwrap())
        .collect();
    assert_eq!(names, ["Gulati", "Indrani"]);

    let first = select_one(
        &t.db,
        raw("SELECT name FROM companies ORDER BY id", [] as [i64; 0]),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(first.get::<String>("name").unwrap(), "Agra");
    assert!(
        select_one(&t.db, raw("SELECT name FROM companies WHERE id = ?", [99]))
            .await
            .unwrap()
            .is_none()
    );

    let total: Option<i64> = scalar(
        &t.db,
        raw("SELECT SUM(employees) FROM companies", [] as [i64; 0]),
    )
    .await
    .unwrap();
    assert_eq!(total, Some(70));
    let none: Option<String> = scalar(&t.db, raw("SELECT name FROM companies WHERE id = ?", [99]))
        .await
        .unwrap();
    assert_eq!(none, None);
    let error = scalar::<i64, _>(&t.db, raw("SELECT name FROM companies", [] as [i64; 0]))
        .await
        .unwrap_err();
    assert!(matches!(error, OrmError::Decode { .. }), "{error:?}");
}

#[tokio::test]
async fn raw_statements_report_the_rows_they_change() {
    let t = sqlite().await;
    seed(&t.db).await;
    let changed = affecting_statement(
        &t.db,
        raw(
            "UPDATE companies SET country = ? WHERE employees >= ?",
            [Value::from("DE"), Value::from(15)],
        ),
    )
    .await
    .unwrap();
    assert_eq!(changed, 2);
    let deleted = statement(
        &t.db,
        raw(
            "DELETE FROM companies WHERE country IS NULL",
            [] as [i64; 0],
        ),
    )
    .await
    .unwrap();
    assert_eq!(deleted, 1);
    unprepared(
        &t.db,
        "CREATE TABLE scratch (id INTEGER PRIMARY KEY); INSERT INTO scratch (id) VALUES (1), (2);",
    )
    .await
    .unwrap();
    assert_eq!(table("scratch").count(&t.db).await.unwrap(), 2);

    let error = statement(
        &t.db,
        raw("DELETE FROM companies WHERE id = ?", [] as [i64; 0]),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: the raw SQL `DELETE FROM companies WHERE id = ?` has 1 placeholder(s) but 0 value(s)"
    );
}

/// Raw statements run where the executor runs: inside a unit of work they
/// see its uncommitted writes, and a rollback undoes them.
#[tokio::test]
async fn raw_statements_join_the_unit_of_work() {
    let t = sqlite().await;
    seed(&t.db).await;
    let uow = t.db.uow(None, None).await;
    statement(&uow, raw("DELETE FROM companies WHERE name = ?", ["Agra"]))
        .await
        .unwrap();
    let left: Option<i64> = scalar(&uow, raw("SELECT COUNT(*) FROM companies", [] as [i64; 0]))
        .await
        .unwrap();
    assert_eq!(left, Some(2));
    uow.rollback().await.unwrap();
    assert_eq!(Company::query().count(&t.db).await.unwrap(), 3);
}

#[tokio::test]
async fn raw_fragments_fit_conditions_selects_and_orders() {
    let t = sqlite().await;
    seed(&t.db).await;
    let companies = Company::query()
        .where_(raw("employees % ? = 0", [5]))
        .add_select([raw("employees * 2 AS doubled", [] as [i64; 0])])
        .order(navree_orm::ir::Order::raw("employees DESC", [] as [i64; 0]))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(
        companies.map(|c| (c.name.clone(), c.extra::<i64>("doubled").unwrap())),
        [
            ("Gulati".to_owned(), Some(100)),
            ("Indrani".to_owned(), Some(30)),
            ("Agra".to_owned(), Some(10))
        ]
    );
    assert_eq!(Company::meta().table(), "companies");
}
