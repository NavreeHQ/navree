//! `Builder::list`: a list query read against a spec, on a model:
//! each operator, in and not_in, null, the sort allow-list, an unknown field,
//! and numbered and cursor pages.

use navree_http::list::{Field, ListQuery, ListResource, ListSpec};
use navree_orm::list::ListQueryExt;
use navree_orm::{CursorRequest, Dialect, Model};

use crate::models::Company;
use crate::support::{TestDb, sqlite};

struct Companies;

impl ListResource for Companies {
    const SPEC: ListSpec = ListSpec::new(&[
        Field::text("name"),
        Field::text("country"),
        Field::boolean("active"),
        Field::int("employees"),
        Field::datetime("created_at"),
        Field::int("id"),
    ])
    .sortable(&["name", "employees", "id"])
    .default_sort(&["name"])
    .searchable(&["name", "country"])
    .page_size(3, 10);
}

/// Six companies: Sutlej Supply (IN, 120, active), Beas Traders (IN, 40, active),
/// Ravi Foods (IN, 9, inactive), Chenab Textiles (no country, 300, active),
/// Jhelum Tools (PK, 75, active), Ganga Mills (IN, 75, inactive).
async fn companies() -> TestDb {
    let t = sqlite().await;
    let rows = [
        ("Sutlej Supply", Some("IN"), 120, true),
        ("Beas Traders", Some("IN"), 40, true),
        ("Ravi Foods", Some("IN"), 9, false),
        ("Chenab Textiles", None, 300, true),
        ("Jhelum Tools", Some("PK"), 75, true),
        ("Ganga Mills", Some("IN"), 75, false),
    ];
    for (name, country, employees, active) in rows {
        Company::create(
            &t.db,
            Company {
                name: name.to_owned(),
                country: country.map(str::to_owned),
                employees,
                active,
                ..Company::default()
            },
        )
        .await
        .unwrap();
    }
    t
}

async fn names(t: &TestDb, query: &str) -> Vec<String> {
    let list = Companies::SPEC.parse(query).unwrap();
    Company::query()
        .list(&list, &Companies::SPEC)
        .unwrap()
        .get(&t.db)
        .await
        .unwrap()
        .into_vec()
        .into_iter()
        .map(|company| company.into_inner().name)
        .collect()
}

#[tokio::test]
async fn equality_and_the_comparison_operators() {
    let t = companies().await;
    // Rows come back in the list's default order: by name.
    assert_eq!(names(&t, "filter[country]=PK").await, ["Jhelum Tools"]);
    assert_eq!(
        names(&t, "filter[employees]=75").await,
        ["Ganga Mills", "Jhelum Tools"]
    );
    assert_eq!(
        names(&t, "filter[employees][gt]=120").await,
        ["Chenab Textiles"]
    );
    assert_eq!(
        names(&t, "filter[employees][gte]=120").await,
        ["Chenab Textiles", "Sutlej Supply"]
    );
    assert_eq!(
        names(&t, "filter[employees][lt]=9").await,
        Vec::<String>::new()
    );
    assert_eq!(
        names(&t, "filter[employees][lte]=40").await,
        ["Beas Traders", "Ravi Foods"]
    );
    assert_eq!(
        names(
            &t,
            "filter[employees][gte]=40&filter[employees][lte]=75&filter[active]=true"
        )
        .await,
        ["Beas Traders", "Jhelum Tools"]
    );
    assert_eq!(
        names(&t, "filter[active][ne]=true").await,
        ["Ganga Mills", "Ravi Foods"]
    );
}

#[tokio::test]
async fn in_and_not_in() {
    let t = companies().await;
    assert_eq!(
        names(&t, "filter[employees][in]=9,300").await,
        ["Chenab Textiles", "Ravi Foods"]
    );
    assert_eq!(
        names(&t, "filter[employees][in]=9&filter[employees][in]=300").await,
        ["Chenab Textiles", "Ravi Foods"]
    );
    // `not_in` is SQL's: the company with no country is in neither list.
    assert_eq!(
        names(&t, "filter[country][not_in]=PK").await,
        ["Beas Traders", "Ganga Mills", "Ravi Foods", "Sutlej Supply"]
    );
}

#[tokio::test]
async fn null_and_not_null() {
    let t = companies().await;
    assert_eq!(
        names(&t, "filter[country][null]=true").await,
        ["Chenab Textiles"]
    );
    assert_eq!(
        names(&t, "filter[country][null]=false&filter[active]=false").await,
        ["Ganga Mills", "Ravi Foods"]
    );
}

#[tokio::test]
async fn contains_and_starts_with_ignore_case() {
    let t = companies().await;
    assert_eq!(
        names(&t, "filter[name][contains]=TRAD").await,
        ["Beas Traders"]
    );
    assert_eq!(
        names(&t, "filter[name][starts_with]=ra").await,
        ["Ravi Foods"]
    );
    assert_eq!(
        names(&t, "filter[name][starts_with]=raders").await,
        Vec::<String>::new()
    );
    // A wildcard is refused by the list's check, and again by `list` if a
    // query that skipped the check reaches it.
    assert!(
        Companies::SPEC
            .parse("filter[name][starts_with]=r_")
            .is_err()
    );
    let (list, _) = ListQuery::parse_lenient("filter[name][starts_with]=r_");
    assert!(Company::query().list(&list, &Companies::SPEC).is_err());
}

#[tokio::test]
async fn a_date_stands_for_its_whole_day() {
    let t = companies().await;
    assert_eq!(
        names(&t, "filter[created_at][lte]=2999-12-31").await.len(),
        6,
        "everything created before the end of 2999-12-31"
    );
    assert!(
        names(&t, "filter[created_at][gte]=2999-01-01")
            .await
            .is_empty()
    );
    assert!(
        names(&t, "filter[created_at][lt]=2000-01-01T00:00:00Z")
            .await
            .is_empty()
    );
    assert_eq!(
        names(&t, "filter[created_at][gt]=2000-01-01").await.len(),
        6,
        "after the whole day of 2000-01-01"
    );
    assert!(names(&t, "filter[created_at]=2999-12-31").await.is_empty());
}

#[tokio::test]
async fn search_looks_in_the_declared_columns_only() {
    let t = companies().await;
    assert_eq!(names(&t, "search=sutlej").await, ["Sutlej Supply"]);
    // `country` is searched too: three companies are in PK or have a "pk"
    // in a name column.
    assert_eq!(names(&t, "search=PK").await, ["Jhelum Tools"]);
    assert_eq!(
        names(&t, "search=%20").await.len(),
        6,
        "a blank search is no search"
    );
}

#[tokio::test]
async fn the_sort_is_the_clients_inside_the_allow_list() {
    let t = companies().await;
    assert_eq!(
        names(&t, "sort=-employees").await,
        [
            "Chenab Textiles",
            "Sutlej Supply",
            "Ganga Mills",
            "Jhelum Tools",
            "Beas Traders",
            "Ravi Foods"
        ],
        "equal employees (75) keep the primary key's order, in the sort's direction"
    );
    assert_eq!(
        names(&t, "sort=employees,name").await[..2],
        ["Ravi Foods".to_owned(), "Beas Traders".to_owned()]
    );
    // A field outside the allow-list never reaches SQL.
    assert!(Companies::SPEC.parse("sort=country").is_err());
    assert!(
        Companies::SPEC
            .parse("sort=name;drop table companies")
            .is_err()
    );
    assert!(Companies::SPEC.parse("sort=updated_at").is_err());
}

#[tokio::test]
async fn an_unknown_field_is_an_error_not_sql() {
    let t = companies().await;
    let (list, _) = ListQuery::parse_lenient("filter[password]=x");
    assert!(Company::query().list(&list, &Companies::SPEC).is_err());
    let error = Companies::SPEC.parse("filter[password]=x").unwrap_err();
    assert_eq!(error.problems[0].param, "filter[password]");
    // Quotes and SQL in a value are values.
    assert!(
        names(&t, "filter[name]=x'%20OR%201=1%20--")
            .await
            .is_empty()
    );
    assert!(
        names(&t, "filter[name][contains]=%27;DROP%20TABLE%20companies;--")
            .await
            .is_empty()
    );
    assert_eq!(names(&t, "").await.len(), 6, "the table is still there");
}

#[tokio::test]
async fn the_query_renders_as_parameters() {
    let list = Companies::SPEC
        .parse("filter[employees][gte]=40&filter[country][in]=IN,PK&sort=-employees&search=tools")
        .unwrap();
    let sql = Company::query()
        .list(&list, &Companies::SPEC)
        .unwrap()
        .to_sql(Dialect::Postgres)
        .unwrap();
    assert!(sql.sql.contains("ORDER BY"), "{}", sql.sql);
    assert!(
        !sql.sql.contains("tools"),
        "values are bound, not written: {}",
        sql.sql
    );
    assert!(!sql.sql.contains("40"), "{}", sql.sql);
}

#[tokio::test]
async fn numbered_pages_use_the_lists_size() {
    let t = companies().await;
    let list = Companies::SPEC.parse("page=2").unwrap();
    let page = Company::query()
        .list(&list, &Companies::SPEC)
        .unwrap()
        .paginate(&t.db, list.page_request(&Companies::SPEC))
        .await
        .unwrap();
    // The list's page size is 3; the default order is by name.
    assert_eq!(
        page.data
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["Jhelum Tools", "Ravi Foods", "Sutlej Supply"]
    );
    assert_eq!(
        (page.meta.total, page.meta.per_page, page.meta.last_page),
        (6, 3, 2)
    );
    assert!(Companies::SPEC.parse("per_page=11").is_err());
}

#[tokio::test]
async fn cursor_pages_walk_the_whole_list() {
    let t = companies().await;
    let mut seen: Vec<String> = Vec::new();
    let mut request = Companies::SPEC
        .parse("limit=4")
        .unwrap()
        .cursor_request(&Companies::SPEC);
    assert_eq!(
        request,
        CursorRequest {
            cursor: None,
            limit: Some(4)
        }
    );
    loop {
        let list = ListQuery::default();
        let page = Company::query()
            .list(&list, &Companies::SPEC)
            .unwrap()
            .cursor_paginate(&t.db, request.clone())
            .await
            .unwrap();
        seen.extend(page.data.iter().map(|c| c.name.clone()));
        match page.next_cursor {
            Some(next) => request = CursorRequest::after(next).limit(4),
            None => break,
        }
    }
    assert_eq!(seen.len(), 6);
    let mut sorted = seen.clone();
    sorted.sort();
    assert_eq!(
        seen, sorted,
        "the default order is by name, page after page"
    );
}
