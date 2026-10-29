//! Row locks on SQLite: the clause is dropped (its write transactions are
//! serialised), and locking reads run inside the unit of work.

use navree_orm::{LockMode, Model, OrmError, Record};

use crate::models::Company;
use crate::support::sqlite;

#[tokio::test]
async fn locking_reads_join_the_unit_of_work() {
    let t = sqlite().await;
    let acme = Company::create(
        &t.db,
        Company {
            name: "Acme".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();

    // A plain read through `&Db` leaves the unit of work's transaction
    // closed; a locking read begins it, since a lock outside a transaction
    // ends with the statement.
    let uow = t.db.uow(None, None).await;
    let db = t.db.clone();
    uow.scope(async {
        Company::query().get(&db).await.unwrap();
    })
    .await;
    assert!(!uow.has_open_transaction().await);
    let locked = uow
        .scope(async {
            Company::query()
                .lock(LockMode::NoKeyUpdate)
                .skip_locked()
                .get(&db)
                .await
        })
        .await
        .unwrap();
    assert_eq!(locked.len(), 1);
    assert!(
        uow.has_open_transaction().await,
        "the locking read began it"
    );
    assert!(!uow.lock_for_update(), "SQLite has no row locks");

    let mut record = Company::find_or_fail(&uow, acme.id).await.unwrap();
    record.name = "changed in memory".into();
    record.refresh_for_update(&uow).await.unwrap();
    assert_eq!(record.name, "Acme", "reloaded from the row");
    assert!(record.is_clean());
    uow.commit().await.unwrap();

    let mut new = Record::new(Company::default());
    let error = new.refresh_for_update(&t.db).await.unwrap_err();
    assert!(matches!(error, OrmError::NotPersisted { .. }), "{error}");
    assert_eq!(
        error.to_string(),
        "cannot lock a Company that has not been saved"
    );
}
