//! SeaORM entities as ORM models: one struct derives both
//! `DeriveEntityModel` and `navree_orm::Model`, so a table keeps its SeaORM
//! entity (`Entity`, `Column`, `ActiveModel`) for code built on it while
//! the ORM reads and writes the same rows. The framework's own tables
//! (audit entries, notifications, users, workspaces, ...) are defined this
//! way.

use navree_db::sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
};
use navree_orm::Model as _;
use serde_json::json;
use uuid::Uuid;

use crate::support;

/// The `notices` table as both a SeaORM entity and an ORM model.
mod notice {
    use navree_db::sea_orm::entity::prelude::*;
    use serde::Serialize;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, navree_orm::Model, Serialize)]
    #[sea_orm(table_name = "notices")]
    #[model(table = "notices")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The column `type` (a renamed column in both).
        #[sea_orm(column_name = "type")]
        #[model(column = "type")]
        #[serde(rename = "type")]
        pub notice_type: String,
        #[sea_orm(column_type = "JsonBinary")]
        pub data: Json,
        pub read_at: Option<DateTimeUtc>,
        pub created_at: DateTimeUtc,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

async fn database() -> support::TestDb {
    let db = support::sqlite().await;
    db.db
        .conn()
        .execute_unprepared(
            "CREATE TABLE notices (id BLOB PRIMARY KEY NOT NULL, type VARCHAR NOT NULL, \
             data TEXT NOT NULL, read_at TIMESTAMP NULL, created_at TIMESTAMP NOT NULL)",
        )
        .await
        .expect("creates notices");
    db
}

fn notice(kind: &str) -> notice::Model {
    notice::Model {
        id: Uuid::now_v7(),
        notice_type: kind.to_owned(),
        data: json!({"title": kind}),
        read_at: None,
        created_at: support::epoch(),
    }
}

#[tokio::test]
async fn rows_written_through_the_entity_are_read_as_models_and_back() {
    let db = database().await;
    let written = notice("welcome");
    notice::ActiveModel::from(written.clone())
        .insert(db.db.conn())
        .await
        .unwrap();

    let read = notice::Model::query()
        .where_(notice::Model::NOTICE_TYPE.eq("welcome"))
        .first(&db.db)
        .await
        .unwrap()
        .expect("the row");
    assert_eq!(read.model(), &written);
    assert_eq!(notice::Model::table_name(), "notices");

    let created = notice::Model::create(&db.db, notice("digest"))
        .await
        .unwrap();
    let found = notice::Entity::find()
        .filter(notice::Column::NoticeType.eq("digest"))
        .one(db.db.conn())
        .await
        .unwrap()
        .expect("written through the ORM, read through the entity");
    assert_eq!(&found, created.model());
}

#[tokio::test]
async fn records_of_entity_models_track_changes_and_save() {
    let db = database().await;
    let mut record = notice::Model::create(&db.db, notice("welcome"))
        .await
        .unwrap();
    record.read_at = Some(support::epoch());
    assert!(record.is_dirty_on([notice::Model::READ_AT]));
    record.save(&db.db).await.unwrap();

    let stored = notice::Entity::find_by_id(record.id)
        .one(db.db.conn())
        .await
        .unwrap()
        .expect("stored");
    assert_eq!(stored.read_at, Some(support::epoch()));
    assert_eq!(
        notice::Model::query()
            .where_null(notice::Model::READ_AT)
            .count(&db.db)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn an_entity_model_serializes_through_its_own_serde() {
    let db = database().await;
    let record = notice::Model::create(&db.db, notice("welcome"))
        .await
        .unwrap();
    let json = record.to_value().unwrap();
    assert_eq!(json["type"], json!("welcome"));
    assert_eq!(json["data"], json!({"title": "welcome"}));
    assert!(json.get("notice_type").is_none());

    let mut active = notice::ActiveModel::from(record.into_inner());
    active.notice_type = Set("renamed".to_owned());
    active.update(db.db.conn()).await.unwrap();
    assert_eq!(
        notice::Model::query()
            .where_(notice::Model::NOTICE_TYPE.eq("renamed"))
            .count(&db.db)
            .await
            .unwrap(),
        1
    );
}
