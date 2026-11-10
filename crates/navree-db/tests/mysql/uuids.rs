//! The schema builder's UUID columns on MySQL and MariaDB: 16-byte
//! binaries, the form navree-db binds a `Uuid` in. A `Uuid` goes in and
//! comes back as it is, and a foreign key to a `uuid_primary()` key (the
//! users table of every app) works.

use navree_db::{Executor, ExecutorExt, Statement};
use navree_schema::Schema;
use uuid::Uuid;

use crate::support::Scratch;

#[tokio::test]
async fn uuid_columns_hold_the_bound_uuids_and_take_foreign_keys() {
    let Some(scratch) =
        Scratch::new("uuid_columns_hold_the_bound_uuids_and_take_foreign_keys").await
    else {
        return;
    };
    let db = scratch.db().await;
    let schema = Schema::new(&db);
    schema
        .create("users", |t| {
            t.uuid_primary();
            t.string("name");
        })
        .await
        .unwrap();
    schema
        .create("deals", |t| {
            t.uuid_primary();
            t.foreign_uuid("owner_id")
                .nullable()
                .constrained_on("users", "id")
                .null_on_delete();
            t.string("title");
        })
        .await
        .unwrap();

    let column_type = |column: &'static str| {
        let db = &db;
        async move {
            db.fetch_scalar::<String>(&Statement::portable(
                "SELECT CAST(column_type AS CHAR) FROM information_schema.columns \
                 WHERE table_schema = DATABASE() AND table_name = 'deals' AND column_name = ?",
                [column.into()],
            ))
            .await
            .unwrap()
        }
    };
    assert_eq!(column_type("id").await, "binary(16)");
    assert_eq!(column_type("owner_id").await, "binary(16)");

    let owner = Uuid::now_v7();
    db.execute(&Statement::portable(
        "INSERT INTO users (id, name) VALUES (?, ?)",
        [owner.into(), "Harpreet Kaur".into()],
    ))
    .await
    .unwrap();
    let deal = Uuid::now_v7();
    db.execute(&Statement::portable(
        "INSERT INTO deals (id, owner_id, title) VALUES (?, ?, ?)",
        [deal.into(), owner.into(), "Ludhiana mill renewal".into()],
    ))
    .await
    .unwrap();
    let found: Uuid = db
        .fetch_scalar(&Statement::portable(
            "SELECT owner_id FROM deals WHERE id = ?",
            [deal.into()],
        ))
        .await
        .unwrap();
    assert_eq!(found, owner);
    let hex: String = db
        .fetch_scalar(&Statement::text("SELECT LOWER(HEX(id)) FROM users"))
        .await
        .unwrap();
    assert_eq!(hex, owner.simple().to_string(), "the 16 bytes of the UUID");

    // The foreign key holds: an owner that does not exist is refused, and
    // deleting the owner empties the deal's owner.
    let refused = db
        .execute(&Statement::portable(
            "INSERT INTO deals (id, owner_id, title) VALUES (?, ?, ?)",
            [
                Uuid::now_v7().into(),
                Uuid::now_v7().into(),
                "Amritsar depot".into(),
            ],
        ))
        .await;
    assert!(refused.is_err(), "{refused:?}");
    db.execute(&Statement::portable(
        "DELETE FROM users WHERE id = ?",
        [owner.into()],
    ))
    .await
    .unwrap();
    let owners: i64 = db
        .fetch_scalar(&Statement::text(
            "SELECT COUNT(*) FROM deals WHERE owner_id IS NULL",
        ))
        .await
        .unwrap();
    assert_eq!(owners, 1);
}
