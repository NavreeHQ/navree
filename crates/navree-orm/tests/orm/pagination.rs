//! `paginate`, `simple_paginate` and `cursor_paginate`.

use navree_orm::{CursorRequest, Model, OrmError, PageQuery};
use serde_json::json;

use crate::models::{Company, Deal};
use crate::support::{TestDb, sqlite};

async fn companies(count: i32) -> TestDb {
    let t = sqlite().await;
    for n in 1..=count {
        Company::create(
            &t.db,
            Company {
                name: format!("C{n:02}"),
                employees: n % 3,
                ..Company::default()
            },
        )
        .await
        .unwrap();
    }
    t
}

#[tokio::test]
async fn pages_carry_their_total_and_use_the_model_page_size() {
    let t = sqlite().await;
    for n in 0..5 {
        Deal::create(&t.db, Deal::new(&format!("D{n}"), n))
            .await
            .unwrap();
    }
    // Deal's per_page is 2.
    let page = Deal::query()
        .order_by(Deal::TITLE)
        .paginate(&t.db, 2)
        .await
        .unwrap();
    assert_eq!(
        page.data
            .iter()
            .map(|d| d.title.as_str())
            .collect::<Vec<_>>(),
        ["D2", "D3"]
    );
    assert_eq!(
        (
            page.meta.current_page,
            page.meta.per_page,
            page.meta.total,
            page.meta.last_page
        ),
        (2, 2, 5, 3)
    );

    let page = Deal::query()
        .order_by(Deal::TITLE)
        .paginate(&t.db, PageQuery::new(1, 4))
        .await
        .unwrap();
    assert_eq!(page.data.len(), 4);
    assert_eq!(page.meta.last_page, 2);

    let past = Deal::query().paginate(&t.db, (9, 2)).await.unwrap();
    assert!(past.data.is_empty());
    assert_eq!(past.meta.total, 5);

    let json = serde_json::to_value(
        Deal::query()
            .order_by(Deal::TITLE)
            .paginate(&t.db, (1, 1))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        json["meta"],
        json!({"current_page": 1, "per_page": 1, "total": 5, "last_page": 5})
    );
    assert_eq!(json["data"][0]["title"], "D0");
}

#[tokio::test]
async fn simple_pages_know_whether_more_follow() {
    let t = companies(5).await;
    let first = Company::query()
        .order_by(Company::NAME)
        .simple_paginate(&t.db, (1, 2))
        .await
        .unwrap();
    assert!(first.meta.has_more);
    assert_eq!(first.data.len(), 2);
    let last = Company::query()
        .order_by(Company::NAME)
        .simple_paginate(&t.db, (3, 2))
        .await
        .unwrap();
    assert!(!last.meta.has_more);
    assert_eq!(last.data[0].name, "C05");
    let json = serde_json::to_value(&last).unwrap();
    assert_eq!(
        json["meta"],
        json!({"current_page": 3, "per_page": 2, "has_more": false})
    );
}

#[tokio::test]
async fn cursor_pages_walk_forward_and_back() {
    let t = companies(7).await;
    // Mixed directions: employees descending, then the key ascending.
    let query = || Company::query().order_by_desc(Company::EMPLOYEES);
    let names = |page: &navree_orm::CursorPage<navree_orm::Record<Company>>| {
        page.data.iter().map(|c| c.name.clone()).collect::<Vec<_>>()
    };

    let first = query()
        .cursor_paginate(&t.db, CursorRequest::first().limit(3))
        .await
        .unwrap();
    // employees: C01=1 C02=2 C03=0 C04=1 C05=2 C06=0 C07=1
    assert_eq!(names(&first), ["C02", "C05", "C01"]);
    assert!(first.prev_cursor.is_none());
    let next = first.next_cursor.clone().unwrap();

    let second = query()
        .cursor_paginate(&t.db, CursorRequest::after(next).limit(3))
        .await
        .unwrap();
    assert_eq!(names(&second), ["C04", "C07", "C03"]);
    let third = query()
        .cursor_paginate(
            &t.db,
            CursorRequest::after(second.next_cursor.clone().unwrap()).limit(3),
        )
        .await
        .unwrap();
    assert_eq!(names(&third), ["C06"]);
    assert!(third.next_cursor.is_none());

    let back = query()
        .cursor_paginate(
            &t.db,
            CursorRequest::after(third.prev_cursor.clone().unwrap()).limit(3),
        )
        .await
        .unwrap();
    assert_eq!(names(&back), ["C04", "C07", "C03"]);
    let start = query()
        .cursor_paginate(
            &t.db,
            CursorRequest::after(back.prev_cursor.clone().unwrap()).limit(3),
        )
        .await
        .unwrap();
    assert_eq!(names(&start), ["C02", "C05", "C01"]);
    assert!(start.prev_cursor.is_none(), "nothing before the first page");
    assert!(start.next_cursor.is_some());
}

#[tokio::test]
async fn unordered_queries_page_by_key_and_cursors_must_match_the_query() {
    let t = companies(3).await;
    let page = Company::query()
        .cursor_paginate(&t.db, CursorRequest::first().limit(2))
        .await
        .unwrap();
    assert_eq!(page.data.iter().map(|c| c.id).collect::<Vec<_>>(), [1, 2]);
    let cursor = page.next_cursor.unwrap();
    let rest = Company::query()
        .cursor_paginate(&t.db, CursorRequest::after(cursor.clone()))
        .await
        .unwrap();
    assert_eq!(rest.data.len(), 1);

    let error = Company::query()
        .order_by(Company::NAME)
        .cursor_paginate(&t.db, CursorRequest::after(cursor))
        .await
        .unwrap_err();
    assert!(matches!(error, OrmError::InvalidQuery(_)));
    assert!(
        error
            .to_string()
            .contains("the pagination cursor is for columns [\"id\"]"),
        "{error}"
    );
    let error = Company::query()
        .cursor_paginate(&t.db, CursorRequest::after("garbage"))
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "invalid query: the pagination cursor is invalid"
    );
}
