//! A model over a table that other code also writes with statements of its
//! own: a renamed column (`#[model(column = "type")]`), a JSON column and
//! the model's own serde. Rows written by hand are read as models, and rows
//! the ORM writes read back by hand.

use chrono::{DateTime, Utc};
use navree_db::{Executor as _, ExecutorExt as _, Statement};
use navree_orm::Model as _;
use serde_json::json;
use uuid::Uuid;

use crate::support;

/// The `notices` table.
mod notice {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    #[derive(Clone, Debug, PartialEq, navree_orm::Model, Serialize)]
    #[model(table = "notices")]
    pub struct Notice {
        pub id: Uuid,
        /// The column `type`.
        #[model(column = "type")]
        #[serde(rename = "type")]
        pub notice_type: String,
        pub data: serde_json::Value,
        pub read_at: Option<DateTime<Utc>>,
        pub created_at: DateTime<Utc>,
    }
}

use notice::Notice;

async fn database() -> support::TestDb {
    let db = support::sqlite().await;
    db.db
        .pool()
        .execute_script(
            "CREATE TABLE notices (id BLOB PRIMARY KEY NOT NULL, type VARCHAR NOT NULL, \
             data TEXT NOT NULL, read_at TIMESTAMP NULL, created_at TIMESTAMP NOT NULL)",
        )
        .await
        .expect("creates notices");
    db
}

fn notice(kind: &str) -> Notice {
    Notice {
        id: Uuid::now_v7(),
        notice_type: kind.to_owned(),
        data: json!({"title": kind}),
        read_at: None,
        created_at: support::epoch(),
    }
}

#[tokio::test]
async fn rows_written_by_hand_are_read_as_models_and_back() {
    let db = database().await;
    let written = notice("welcome");
    db.db
        .pool()
        .execute(&Statement::portable(
            "INSERT INTO notices (id, type, data, read_at, created_at) VALUES (?, ?, ?, ?, ?)",
            [
                written.id.into(),
                written.notice_type.clone().into(),
                written.data.clone().into(),
                Option::<DateTime<Utc>>::None.into(),
                written.created_at.into(),
            ],
        ))
        .await
        .unwrap();

    let read = Notice::query()
        .where_(Notice::NOTICE_TYPE.eq("welcome"))
        .first(&db.db)
        .await
        .unwrap()
        .expect("the row");
    assert_eq!(read.model(), &written);
    assert_eq!(Notice::table_name(), "notices");

    let created = Notice::create(&db.db, notice("digest")).await.unwrap();
    let id: Uuid = db
        .db
        .pool()
        .fetch_scalar(&Statement::portable(
            "SELECT id FROM notices WHERE type = ?",
            ["digest".into()],
        ))
        .await
        .expect("written through the ORM, read by hand");
    assert_eq!(id, created.id);
}

#[tokio::test]
async fn records_track_changes_and_save() {
    let db = database().await;
    let mut record = Notice::create(&db.db, notice("welcome")).await.unwrap();
    record.read_at = Some(support::epoch());
    assert!(record.is_dirty_on([Notice::READ_AT]));
    record.save(&db.db).await.unwrap();

    let stored: Option<DateTime<Utc>> = db
        .db
        .pool()
        .fetch_scalar(&Statement::portable(
            "SELECT read_at FROM notices WHERE id = ?",
            [record.id.into()],
        ))
        .await
        .unwrap();
    assert_eq!(stored, Some(support::epoch()));
    assert_eq!(
        Notice::query()
            .where_null(Notice::READ_AT)
            .count(&db.db)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn a_model_serializes_through_its_own_serde() {
    let db = database().await;
    let record = Notice::create(&db.db, notice("welcome")).await.unwrap();
    let json = record.to_value().unwrap();
    assert_eq!(json["type"], json!("welcome"));
    assert_eq!(json["data"], json!({"title": "welcome"}));
    assert!(json.get("notice_type").is_none());

    db.db
        .pool()
        .execute(&Statement::portable(
            "UPDATE notices SET type = ? WHERE id = ?",
            ["renamed".into(), record.id.into()],
        ))
        .await
        .unwrap();
    assert_eq!(
        Notice::query()
            .where_(Notice::NOTICE_TYPE.eq("renamed"))
            .count(&db.db)
            .await
            .unwrap(),
        1
    );
}
