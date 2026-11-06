//! The ORM through the facade: a model declared and queried with
//! `navree::prelude` and `navree::orm`, inside a unit of work.

/// The temporary-directory guard of navree-testing, included so these tests
/// need no feature for it: its reaper retries a folder Windows will not
/// delete while SQLite has a file open, and its sweep removes the folders of
/// earlier runs.
#[allow(dead_code)]
#[path = "../../navree-testing/src/temp.rs"]
mod temp;
use temp::TempDirGuard;

use navree::BoxError;
use navree::db::Executor as _;
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
    let dir = TempDirGuard::new();
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
    db.pool()
        .execute_script(
            "CREATE TABLE contacts (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, email TEXT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
        )
        .await
        .unwrap();

    let uow = db.uow(None, None).await;
    let ada: Record<Contact> = Contact::create(
        &uow,
        Contact {
            name: "Anu".into(),
            ..Contact::default()
        },
    )
    .await
    .unwrap();
    Contact::create(
        &uow,
        Contact {
            name: "Gauri".into(),
            email: Some("gauri@example.com".into()),
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
    assert_eq!(with_email.map(|c| c.name.clone()), ["Gauri"]);
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
        "Anu"
    );
}

/// A customer with people, related through the facade's `#[relations]`.
#[derive(Model, Clone, Debug, Default, Serialize)]
struct Customer {
    id: i64,
    name: String,
}

#[navree::orm::relations]
impl Customer {
    fn people(&self) -> navree::orm::relations::HasMany<Customer, Person> {
        self.has_many(Person::CUSTOMER_ID)
    }
}

#[derive(Model, Clone, Debug, Default, Serialize)]
struct Person {
    id: i64,
    customer_id: Option<i64>,
    name: String,
}

#[navree::orm::relations]
impl Person {
    fn customer(&self) -> navree::orm::relations::BelongsTo<Person, Customer> {
        self.belongs_to(Person::CUSTOMER_ID)
    }
}

#[tokio::test]
async fn relations_work_through_the_facade() {
    let dir = TempDirGuard::new();
    let url = format!(
        "sqlite://{}?mode=rwc",
        dir.path().join("relations.db").display()
    );
    let db = Db::connect(&DbOptions {
        url,
        ..DbOptions::default()
    })
    .await
    .unwrap();
    for statement in [
        "CREATE TABLE customers (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
        "CREATE TABLE people (id INTEGER PRIMARY KEY AUTOINCREMENT, customer_id INTEGER NULL, name TEXT NOT NULL)",
    ] {
        db.pool().execute_script(statement).await.unwrap();
    }
    let agra = Customer::create(
        &db,
        Customer {
            name: "Agra".into(),
            ..Customer::default()
        },
    )
    .await
    .unwrap();
    for name in ["Anu", "Gauri"] {
        agra.people()
            .create(
                &db,
                Person {
                    name: name.into(),
                    ..Person::default()
                },
            )
            .await
            .unwrap();
    }
    let customers = Customer::query()
        .with("people")
        .has("people")
        .get(&db)
        .await
        .unwrap();
    assert_eq!(customers[0].loaded(Customer::people).unwrap().len(), 2);
    let people = Person::query()
        .with(Person::customer)
        .where_relation("customer", col("name").eq("Agra"))
        .get(&db)
        .await
        .unwrap();
    assert_eq!(people.len(), 2);
    assert_eq!(
        people[0]
            .loaded(Person::customer)
            .unwrap()
            .as_ref()
            .unwrap()
            .name,
        "Agra"
    );
}

/// The contacts table, built once for the tests below.
static CONTACTS: Template = Template::new("facade contacts", |template| {
    Box::pin(async move {
        template
            .db()
            .pool()
            .execute_script(
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
            name: "Anu".into(),
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
