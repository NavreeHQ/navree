//! The ORM through the facade: a model declared and queried with
//! `navree::prelude` and `navree::orm`, inside a unit of work.

use navree::db::sea_orm::ConnectionTrait;
use navree::db::{Db, DbOptions};
use navree::orm::{Dialect, col};
use navree::prelude::*;
use serde::Serialize;

#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(timestamps)]
struct Contact {
    id: i64,
    name: String,
    email: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[tokio::test]
async fn models_work_through_the_facade() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!(
        "sqlite://{}?mode=rwc",
        dir.path().join("facade.db").display()
    );
    let db = Db::connect(&DbOptions {
        url,
        ..DbOptions::default()
    })
    .await
    .unwrap();
    db.conn()
        .execute_unprepared(
            "CREATE TABLE contacts (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, email TEXT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
        )
        .await
        .unwrap();

    let uow = db.uow(None, None).await;
    let ada: Record<Contact> = Contact::create(
        &uow,
        Contact {
            name: "Ada".into(),
            ..Contact::default()
        },
    )
    .await
    .unwrap();
    Contact::create(
        &uow,
        Contact {
            name: "Grace".into(),
            email: Some("grace@example.com".into()),
            ..Contact::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        uow.changes().len(),
        2,
        "writes are recorded for the audit trail"
    );
    uow.commit().await.unwrap();

    let with_email: Collection<Contact> = Contact::query()
        .where_not_null(Contact::EMAIL)
        .get(&db)
        .await
        .unwrap();
    assert_eq!(with_email.map(|c| c.name.clone()), ["Grace"]);
    let query: Builder<Contact> = Contact::query().where_(col("name").like("a%"));
    assert_eq!(query.clone().count(&db).await.unwrap(), 1);
    assert!(
        query
            .to_sql(Dialect::Postgres)
            .unwrap()
            .sql
            .contains("ILIKE")
    );
    assert_eq!(
        Contact::find_or_fail(&db, ada.id).await.unwrap().name,
        "Ada"
    );
}
