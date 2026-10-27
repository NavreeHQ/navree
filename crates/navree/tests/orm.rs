//! The ORM through the facade: a model declared and queried with
//! `navree::prelude` and `navree::orm`, inside a unit of work.

use navree::BoxError;
use navree::db::sea_orm::ConnectionTrait;
use navree::db::{Db, DbOptions};
use navree::orm::testing::database::{Template, TestDb};
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

/// The contacts table, built once for the tests below.
static CONTACTS: Template = Template::new("facade contacts", |template| {
    Box::pin(async move {
        template
            .db()
            .conn()
            .execute_unprepared(
                "CREATE TABLE contacts (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, email TEXT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
            )
            .await?;
        Ok(())
    })
});

#[navree::test(template = CONTACTS)]
async fn navree_test_gives_each_test_a_copy_of_the_template(db: TestDb) {
    Contact::create(
        db.db(),
        Contact {
            name: "Ada".into(),
            ..Contact::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(Contact::query().count(db.db()).await.unwrap(), 1);
}

#[navree::test(template = CONTACTS)]
async fn navree_test_copies_are_independent(db: TestDb) -> navree::Result<(), BoxError> {
    assert_eq!(Contact::query().count(db.db()).await?, 0);
    Ok(())
}
