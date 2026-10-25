//! Offset pagination (`PageQuery`, `Page`) and cursor pagination.

use axum::extract::FromRequestParts;
use axum::response::IntoResponse;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use keel_db::{
    CursorPage, CursorQuery, Page, PageMeta, PageQuery, create_many, cursor_paginate,
    cursor_paginate_desc, paginate,
};
use keel_http::AppError;
use sea_orm::{EntityTrait, QueryOrder};
use serde_json::json;

use crate::fixtures::{note, notes_db};
use crate::support::TempDb;

/// Notes titled `note-0`, `note-1`, ... (factory from the seed tests' shape).
struct Numbered;

impl keel_db::Factory for Numbered {
    type Entity = note::Entity;

    fn definition(&self, n: usize) -> note::ActiveModel {
        use sea_orm::ActiveValue::Set;
        note::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            title: Set(format!("note-{n:02}")),
            body: Set(String::new()),
        }
    }
}

async fn page_query(uri: &str) -> Result<PageQuery, AppError> {
    let (mut parts, ()) = Request::get(uri).body(()).unwrap().into_parts();
    PageQuery::from_request_parts(&mut parts, &()).await
}

async fn cursor_query(uri: &str) -> Result<CursorQuery, AppError> {
    let (mut parts, ()) = Request::get(uri).body(()).unwrap().into_parts();
    CursorQuery::from_request_parts(&mut parts, &()).await
}

#[tokio::test]
async fn paginate_meta() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    create_many(&Numbered, db.conn(), 23).await.unwrap();
    let select = || note::Entity::find().order_by_asc(note::Column::Title);

    let page = paginate(select(), db.conn(), PageQuery::new(2, 10))
        .await
        .unwrap();
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

    let beyond = paginate(select(), db.conn(), PageQuery::new(9, 10))
        .await
        .unwrap();
    assert!(beyond.data.is_empty());
    assert_eq!(beyond.meta.current_page, 9);
    assert_eq!(beyond.meta.last_page, 3);

    let json = serde_json::to_value(page.map(|n| n.title)).unwrap();
    assert_eq!(
        json["meta"],
        json!({"current_page": 2, "per_page": 10, "total": 23, "last_page": 3})
    );
    assert_eq!(json["data"][0], "note-10");
}

/// Any page number a client can send is an empty page past the end, not a
/// panic in the offset arithmetic (`page * per_page` overflowing `u64`, or
/// an offset above `i64::MAX` for the SQL bind).
#[tokio::test]
async fn huge_page_numbers_are_empty_pages() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    create_many(&Numbered, db.conn(), 3).await.unwrap();
    for (page, per_page) in [
        (u64::MAX, 100),
        (100_000_000_000_000_000, 100),
        (1_152_921_504_606_846_977, 16),
        (u64::MAX / 15 + 2, 15),
    ] {
        let result = paginate(
            note::Entity::find(),
            db.conn(),
            PageQuery::new(page, per_page),
        )
        .await
        .unwrap();
        assert!(result.data.is_empty(), "page {page}: {:?}", result.data);
        assert_eq!(result.meta.total, 3);
        assert_eq!(result.meta.current_page, page);
    }
    let query = page_query("/notes?page=18446744073709551615&per_page=100")
        .await
        .unwrap();
    let result = paginate(note::Entity::find(), db.conn(), query)
        .await
        .unwrap();
    assert!(result.data.is_empty());
}

#[tokio::test]
async fn paginate_empty_table_has_one_page() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let page = paginate(note::Entity::find(), db.conn(), PageQuery::default())
        .await
        .unwrap();
    assert!(page.data.is_empty());
    assert_eq!(
        page.meta,
        PageMeta {
            current_page: 1,
            per_page: 15,
            total: 0,
            last_page: 1
        }
    );
}

#[tokio::test]
async fn page_query_clamps_per_page() {
    let defaults = page_query("/x").await.unwrap();
    assert_eq!((defaults.page, defaults.per_page), (1, 15));

    let clamped = page_query("/x?per_page=500&page=3&search=acme")
        .await
        .unwrap();
    assert_eq!((clamped.page, clamped.per_page), (3, 100));
    assert_eq!(PageQuery::new(1, 1000).per_page, 100);

    for uri in [
        "/x?page=0",
        "/x?page=abc",
        "/x?per_page=0",
        "/x?per_page=-1",
    ] {
        let error = page_query(uri).await.unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST, "{uri}");
        assert!(error.errors.is_some(), "{uri}: field errors");
    }
    let error = page_query("/x?page=0&per_page=x").await.unwrap_err();
    assert_eq!(
        error.body(),
        json!({
            "statusCode": 400,
            "message": ["page must be a positive integer", "per_page must be a positive integer"],
            "error": "Bad Request",
            "errors": {
                "page": ["page must be a positive integer"],
                "per_page": ["per_page must be a positive integer"],
            },
        })
    );
}

#[tokio::test]
async fn page_renders_as_json() {
    let page = Page {
        data: vec![1, 2],
        meta: PageMeta {
            current_page: 1,
            per_page: 15,
            total: 2,
            last_page: 1,
        },
    };
    let response = page.into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["data"], json!([1, 2]));
    assert_eq!(json["meta"]["total"], 2);
}

#[tokio::test]
async fn cursor_pagination() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let notes = create_many(&Numbered, db.conn(), 5).await.unwrap();
    let titles = |page: &CursorPage<note::Model>| {
        page.data
            .iter()
            .map(|n| n.title.clone())
            .collect::<Vec<_>>()
    };

    let first = cursor_paginate(
        note::Entity::find(),
        note::Column::Id,
        db.conn(),
        CursorQuery::new(None, 2),
    )
    .await
    .unwrap();
    assert_eq!(titles(&first), ["note-00", "note-01"]);
    let second = cursor_paginate(
        note::Entity::find(),
        note::Column::Id,
        db.conn(),
        CursorQuery::new(first.next_cursor.clone(), 2),
    )
    .await
    .unwrap();
    assert_eq!(titles(&second), ["note-02", "note-03"]);
    let third = cursor_paginate(
        note::Entity::find(),
        note::Column::Id,
        db.conn(),
        CursorQuery::new(second.next_cursor.clone(), 2),
    )
    .await
    .unwrap();
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
    assert_eq!(notes.len(), 5);

    let json = serde_json::to_value(&first).unwrap();
    assert_eq!(json["next_cursor"], json!(first.next_cursor));
}

#[tokio::test]
async fn cursor_query_validates_and_clamps() {
    let defaults = cursor_query("/x").await.unwrap();
    assert_eq!((defaults.cursor, defaults.limit), (None, 15));
    assert_eq!(cursor_query("/x?limit=1000").await.unwrap().limit, 100);
    for uri in ["/x?cursor=not-a-cursor", "/x?limit=0", "/x?limit=x"] {
        let error = cursor_query(uri).await.unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST, "{uri}");
    }
    let error = cursor_query("/x?cursor=not-a-cursor&limit=0")
        .await
        .unwrap_err();
    assert_eq!(
        error.body()["message"],
        json!(["cursor is invalid", "limit must be a positive integer"])
    );

    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let error = cursor_paginate(
        note::Entity::find(),
        note::Column::Id,
        db.conn(),
        CursorQuery::new(Some("garbage".into()), 2),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("cursor"), "{error}");
}
