//! Record pages on MySQL. UUIDs are `BINARY(16)` there, so UUID v7 keys
//! sort by creation time.

use navree_db::{PageMeta, PageQuery, Record, record};
use uuid::Uuid;

use crate::fixtures::{Note, create_notes};
use crate::support::Scratch;

/// `n` notes titled `note-00`, `note-01`, ...
async fn numbered(db: &navree_db::Db, n: usize) {
    for i in 0..n {
        let note = Note {
            id: Uuid::now_v7(),
            title: format!("note-{i:02}"),
            body: String::new(),
        };
        record::insert(db, &note).await.unwrap();
    }
}

#[tokio::test]
async fn paginate_meta_and_pages() {
    let Some(scratch) = Scratch::new("paginate_meta_and_pages").await else {
        return;
    };
    let db = scratch.db().await;
    create_notes(&db).await;
    let empty = Note::select()
        .page(&db, PageQuery::default())
        .await
        .unwrap();
    assert!(empty.data.is_empty());
    assert_eq!(empty.meta.last_page, 1);

    numbered(&db, 23).await;
    let select = || Note::select().order_by("title");
    let uow = db.uow(None, None).await;
    let page = select().page(&uow, PageQuery::new(2, 10)).await.unwrap();
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

    let last = select().page(&db, PageQuery::new(3, 10)).await.unwrap();
    assert_eq!(last.data.len(), 3);
    for (page, per_page) in [(9, 10), (u64::MAX, 100), (u64::MAX / 15 + 2, 15)] {
        let beyond = select()
            .page(&db, PageQuery::new(page, per_page))
            .await
            .unwrap();
        assert!(beyond.data.is_empty(), "page {page}");
        assert_eq!(beyond.meta.total, 23);
    }
}

#[tokio::test]
async fn uuid_v7_keys_sort_by_creation() {
    let Some(scratch) = Scratch::new("uuid_v7_keys_sort_by_creation").await else {
        return;
    };
    let db = scratch.db().await;
    create_notes(&db).await;
    numbered(&db, 5).await;
    let titles: Vec<String> = Note::select()
        .order_by("id")
        .limit(3)
        .all(&db)
        .await
        .unwrap()
        .into_iter()
        .map(|note| note.title)
        .collect();
    assert_eq!(titles, ["note-00", "note-01", "note-02"]);
    let newest: Vec<String> = Note::select()
        .order_by_desc("id")
        .limit(2)
        .all(&db)
        .await
        .unwrap()
        .into_iter()
        .map(|note| note.title)
        .collect();
    assert_eq!(newest, ["note-04", "note-03"]);
}
