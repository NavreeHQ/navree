//! Offset pages (`PageQuery`, `Page`) and cursor pages (`CursorQuery`,
//! `CursorPage`): the extractors, the response shapes, and a record
//! select's pages.

use axum::extract::FromRequestParts;
use axum::response::IntoResponse;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use navree_db::{CursorPage, CursorQuery, Page, PageMeta, PageQuery, Record, record};
use navree_http::AppError;
use serde_json::json;

use crate::fixtures::{Note, notes_db};
use crate::support::TempDb;

async fn page_query(uri: &str) -> Result<PageQuery, AppError> {
    let (mut parts, ()) = Request::get(uri).body(()).unwrap().into_parts();
    PageQuery::from_request_parts(&mut parts, &()).await
}

async fn cursor_query(uri: &str) -> Result<CursorQuery, AppError> {
    let (mut parts, ()) = Request::get(uri).body(()).unwrap().into_parts();
    CursorQuery::from_request_parts(&mut parts, &()).await
}

/// `n` notes titled `note-00`, `note-01`, ...
async fn numbered(db: &navree_db::Db, n: usize) {
    for i in 0..n {
        let note = Note {
            id: uuid::Uuid::now_v7(),
            title: format!("note-{i:02}"),
            body: String::new(),
        };
        record::insert(db, &note).await.unwrap();
    }
}

#[tokio::test]
async fn record_pages_carry_their_meta() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    numbered(&db, 23).await;
    let select = || Note::select().order_by("title");

    let page = select().page(&db, PageQuery::new(2, 10)).await.unwrap();
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

    let beyond = select().page(&db, PageQuery::new(9, 10)).await.unwrap();
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
/// failure in the offset arithmetic (`page * per_page` overflowing `u64`,
/// or an offset above `i64::MAX` for the SQL bind).
#[tokio::test]
async fn huge_page_numbers_are_empty_pages() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    numbered(&db, 3).await;
    for (page, per_page) in [
        (u64::MAX, 100),
        (100_000_000_000_000_000, 100),
        (1_152_921_504_606_846_977, 16),
        (u64::MAX / 15 + 2, 15),
    ] {
        let result = Note::select()
            .page(&db, PageQuery::new(page, per_page))
            .await
            .unwrap();
        assert!(result.data.is_empty(), "page {page}: {:?}", result.data);
        assert_eq!(result.meta.total, 3);
        assert_eq!(result.meta.current_page, page);
    }
    let query = page_query("/notes?page=18446744073709551615&per_page=100")
        .await
        .unwrap();
    let result = Note::select().page(&db, query).await.unwrap();
    assert!(result.data.is_empty());
}

#[tokio::test]
async fn an_empty_table_has_one_page() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    let page = Note::select()
        .page(&db, PageQuery::default())
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

#[test]
fn pages_compute_their_last_page() {
    let page = Page::new(vec!["a"], PageQuery::new(1, 10), 21);
    assert_eq!(page.meta.last_page, 3);
    assert_eq!(
        Page::<u8>::new(Vec::new(), PageQuery::new(0, 0), 0)
            .meta
            .last_page,
        1
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
            "code": "validation_failed",
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
async fn cursor_pages_render_as_json() {
    let page = CursorPage {
        data: vec!["a", "b"],
        next_cursor: Some("AZoAAAAAcAAAgAAAAAAAAA".to_owned()),
    };
    let mapped = page.map(str::to_uppercase);
    assert_eq!(mapped.data, ["A", "B"]);
    let response = mapped.into_response();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json,
        json!({"data": ["A", "B"], "next_cursor": "AZoAAAAAcAAAgAAAAAAAAA"})
    );
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
    // A cursor is a URL-safe base64 UUID.
    let id = uuid::Uuid::now_v7();
    use base64::Engine as _;
    let cursor = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(id.as_bytes());
    let query = cursor_query(&format!("/x?cursor={cursor}&limit=2"))
        .await
        .unwrap();
    assert_eq!(query.cursor.as_deref(), Some(cursor.as_str()));
    assert_eq!(query.limit, 2);
}
