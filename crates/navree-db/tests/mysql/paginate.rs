//! Offset and cursor pagination on MySQL. UUIDs are `BINARY(16)` there, so
//! UUID v7 keys sort by creation time and cursors page in order.

use navree_db::{
    CursorPage, CursorQuery, Factory, PageMeta, PageQuery, create_many, cursor_paginate,
    cursor_paginate_desc, paginate,
};
use sea_orm::{ActiveValue::Set, EntityTrait, QueryOrder};
use uuid::Uuid;

use crate::fixtures::{create_notes, note};
use crate::support::Scratch;

/// Notes titled `note-00`, `note-01`, ...
struct Numbered;

impl Factory for Numbered {
    type Entity = note::Entity;

    fn definition(&self, n: usize) -> note::ActiveModel {
        note::ActiveModel {
            id: Set(Uuid::now_v7()),
            title: Set(format!("note-{n:02}")),
            body: Set(String::new()),
        }
    }
}

#[tokio::test]
async fn paginate_meta_and_pages() {
    let Some(scratch) = Scratch::new("paginate_meta_and_pages").await else {
        return;
    };
    let db = scratch.db().await;
    create_notes(&db).await;
    let empty = paginate(note::Entity::find(), db.conn(), PageQuery::default())
        .await
        .unwrap();
    assert!(empty.data.is_empty());
    assert_eq!(empty.meta.last_page, 1);

    create_many(&Numbered, db.conn(), 23).await.unwrap();
    let select = || note::Entity::find().order_by_asc(note::Column::Title);
    let uow = db.uow(None, None).await;
    let page = paginate(select(), &*uow.tx().await.unwrap(), PageQuery::new(2, 10))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(
        page.meta,
        PageMeta {
            current_page: 2,
            per_page: 10,
            total: 23,
            last_page: 3
        }
    );
    assert_eq!(page.data.len(), 10);
    assert_eq!(page.data[0].title, "note-10");

    let last = paginate(select(), db.conn(), PageQuery::new(3, 10))
        .await
        .unwrap();
    assert_eq!(last.data.len(), 3);
    for (page, per_page) in [(9, 10), (u64::MAX, 100), (u64::MAX / 15 + 2, 15)] {
        let beyond = paginate(select(), db.conn(), PageQuery::new(page, per_page))
            .await
            .unwrap();
        assert!(beyond.data.is_empty(), "page {page}");
        assert_eq!(beyond.meta.total, 23);
    }
}

#[tokio::test]
async fn cursor_pages_follow_uuid_v7_order() {
    let Some(scratch) = Scratch::new("cursor_pages_follow_uuid_v7_order").await else {
        return;
    };
    let db = scratch.db().await;
    create_notes(&db).await;
    create_many(&Numbered, db.conn(), 5).await.unwrap();
    let titles = |page: &CursorPage<note::Model>| {
        page.data
            .iter()
            .map(|n| n.title.clone())
            .collect::<Vec<_>>()
    };
    let page = |cursor: Option<String>, limit| {
        cursor_paginate(
            note::Entity::find(),
            note::Column::Id,
            db.conn(),
            CursorQuery::new(cursor, limit),
        )
    };
    let first = page(None, 2).await.unwrap();
    assert_eq!(titles(&first), ["note-00", "note-01"]);
    let second = page(first.next_cursor.clone(), 2).await.unwrap();
    assert_eq!(titles(&second), ["note-02", "note-03"]);
    let third = page(second.next_cursor.clone(), 2).await.unwrap();
    assert_eq!(titles(&third), ["note-04"]);
    assert_eq!(third.next_cursor, None);

    let newest = cursor_paginate_desc(
        note::Entity::find(),
        note::Column::Id,
        db.conn(),
        CursorQuery::new(None, 3),
    )
    .await
    .unwrap();
    assert_eq!(titles(&newest), ["note-04", "note-03", "note-02"]);
    let older = cursor_paginate_desc(
        note::Entity::find(),
        note::Column::Id,
        db.conn(),
        CursorQuery::new(newest.next_cursor.clone(), 3),
    )
    .await
    .unwrap();
    assert_eq!(titles(&older), ["note-01", "note-00"]);
    assert_eq!(older.next_cursor, None);
}
