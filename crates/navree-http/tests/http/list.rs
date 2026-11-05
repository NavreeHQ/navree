//! The list-query grammar: the cases the JavaScript parser reads too (MOD-013),
//! hostile input, and the extractor in a running router.

use http::{Method, StatusCode};
use navree_http::list::{
    Field, List, ListQuery, ListResource, ListSpec, MAX_PARAMS, MAX_QUERY_BYTES, Op,
};
use navree_http::{Json, RouteDef};
use proptest::prelude::*;

use crate::support::{Setup, TestApp};

/// The one file both parsers are held to. `@navree/client`'s own tests read it
/// as well.
const CASES: &str = include_str!("../../../../packages/js/client/test/list-query-cases.json");

#[test]
fn the_parser_reads_every_shared_case() {
    let cases: serde_json::Value = serde_json::from_str(CASES).unwrap();
    let cases = cases["cases"].as_array().unwrap();
    assert!(cases.len() > 50, "the shared cases were not read");
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let input = case["input"].as_str().unwrap();
        let (query, problems) = ListQuery::parse_lenient(input);
        assert_eq!(
            query.to_query_string(),
            case["canonical"].as_str().unwrap(),
            "{name}: {input}"
        );
        let mut found: Vec<String> = problems.into_iter().map(|p| p.param).collect();
        found.sort();
        found.dedup();
        let mut expected: Vec<String> = case["problems"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap().to_owned())
            .collect();
        expected.sort();
        assert_eq!(found, expected, "{name}: {input}");
    }
}

#[test]
fn hostile_input_never_panics_and_stays_inside_the_limits() {
    let long = "a".repeat(100_000);
    let deep = "[".repeat(5000);
    let cases = [
        format!("filter[{long}]=x"),
        format!("filter[a]={long}"),
        format!("filter[a][in]={}", "1,".repeat(50_000)),
        format!("sort={}", "a,".repeat(50_000)),
        format!("search={long}"),
        format!("cursor={long}"),
        format!("filter{deep}=x"),
        format!("filter[a]{deep}=x"),
        "filter[a][in]=%00,%00".to_owned(),
        "filter[%00]=x".to_owned(),
        "filter[a]=%FF%FE%FD".to_owned(),
        "%%%=%%%&&&===&&filter[=]".to_owned(),
        "filter[a][in]&filter[a][in]&filter[a][in]".to_owned(),
        "page=1&page=2&page=3&per_page=1&per_page=2".to_owned(),
        "sort=,,,,-,,-,".to_owned(),
        "filter[a b][c d]=x".to_owned(),
        "FILTER[a]=x&Sort=a&SEARCH=x&PAGE=2".to_owned(),
        "filter[a][null]=%00".to_owned(),
        "\u{0}=\u{0}".to_owned(),
    ];
    for input in cases {
        let (query, _) = ListQuery::parse_lenient(&input);
        assert!(query.filter.len() <= navree_http::list::MAX_CONDITIONS);
        assert!(query.sort.len() <= navree_http::list::MAX_SORTS);
        for condition in &query.filter {
            assert!(!condition.values.is_empty());
            assert!(condition.values.len() <= navree_http::list::MAX_VALUES);
            for value in &condition.values {
                assert!(value.len() <= navree_http::list::MAX_TEXT);
                assert!(!value.contains('\0'));
            }
        }
        if let Some(search) = &query.search {
            assert!(search.len() <= navree_http::list::MAX_TEXT);
        }
        // What was kept reads back as itself.
        let (again, problems) = ListQuery::parse_lenient(&query.to_query_string());
        assert_eq!(again, query);
        assert!(problems.is_empty(), "{input:.60}: {problems:?}");
    }
}

#[test]
fn names_that_are_not_case_exact_are_not_the_list_queries() {
    let (query, problems) = ListQuery::parse_lenient("FILTER[a]=x&Sort=a&SEARCH=x&PAGE=2");
    assert_eq!(query, ListQuery::default());
    assert!(problems.is_empty());
}

/// What reading a query written by the library may still complain about: a
/// query that mixes numbered and cursor pages keeps both and says so each time.
fn only_paging_complaints(problems: &[navree_http::list::Problem]) -> bool {
    problems
        .iter()
        .all(|p| matches!(p.param.as_str(), "cursor" | "before" | "limit"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn any_query_string_reads_without_panic_and_round_trips(input in "\\PC{0,400}") {
        let (query, _) = ListQuery::parse_lenient(&input);
        let (again, problems) = ListQuery::parse_lenient(&query.to_query_string());
        prop_assert_eq!(&again, &query);
        prop_assert!(only_paging_complaints(&problems), "{:?}", problems);
    }

    #[test]
    fn list_shaped_noise_round_trips(
        parts in proptest::collection::vec(
            prop_oneof![
                "filter\\[[a-z_.]{1,8}\\](\\[(eq|ne|gt|gte|lt|lte|in|not_in|contains|starts_with|null)\\])?=[a-z0-9,%]{0,10}",
                "sort=-?[a-z_]{1,6}(,-?[a-z_]{1,6}){0,5}",
                "search=[a-z +%]{0,12}",
                "(page|per_page|limit)=[0-9]{0,6}",
                "(cursor|before)=[A-Za-z0-9_-]{0,20}",
                "[a-z]{1,5}=[a-z]{0,5}",
            ],
            0..12,
        )
    ) {
        let input = parts.join("&");
        let (query, _) = ListQuery::parse_lenient(&input);
        let (again, problems) = ListQuery::parse_lenient(&query.to_query_string());
        prop_assert_eq!(&again, &query);
        prop_assert!(only_paging_complaints(&problems), "{:?}", problems);
    }
}

#[test]
fn a_huge_query_is_cut_and_reported() {
    let many = (0..MAX_PARAMS * 2)
        .map(|n| format!("a{n}=1"))
        .collect::<Vec<_>>()
        .join("&");
    let (_, problems) = ListQuery::parse_lenient(&many);
    assert!(problems.iter().any(|p| p.param == "query"));
    let wide = format!("search={}", "x".repeat(MAX_QUERY_BYTES));
    let (_, problems) = ListQuery::parse_lenient(&wide);
    assert!(problems.iter().any(|p| p.param == "query"));
}

// ---- The extractor in a router --------------------------------------------------------

struct Deals;

impl ListResource for Deals {
    const SPEC: ListSpec = ListSpec::new(&[
        Field::one_of("stage", &["lead", "won", "lost"]),
        Field::int("amount_cents"),
        Field::uuid("owner_id"),
    ])
    .sortable(&["amount_cents"])
    .searchable(&["title"]);
}

async fn app() -> TestApp {
    Setup::new(vec![
        RouteDef::on(Method::GET, "/deals", |list: List<Deals>| async move {
            Json(serde_json::json!({
                "query": list.to_query_string(),
                "filters": list.filter.len(),
                "gte": list.condition("amount_cents", Op::Gte).map(|c| c.value().to_owned()),
            }))
        })
        .public(),
    ])
    .boot()
    .await
}

#[tokio::test]
async fn a_valid_list_query_reaches_the_handler() {
    let app = app().await;
    let response = app
        .get("/api/deals?filter[stage][in]=lead,won&filter[amount_cents][gte]=100&sort=-amount_cents&page=2")
        .await;
    assert_eq!(response.status, StatusCode::OK);
    let body = response.json();
    assert_eq!(body["filters"], 2);
    assert_eq!(body["gte"], "100");
}

#[tokio::test]
async fn an_unknown_field_is_a_400_that_names_the_parameter() {
    let app = app().await;
    let body = app
        .get("/api/deals?filter[password][contains]=x&per_page=1000")
        .await
        .assert_error(400);
    assert_eq!(body["code"], "validation_failed");
    assert!(
        body["errors"]["filter[password][contains]"].is_array(),
        "{body}"
    );
    assert!(body["errors"]["per_page"].is_array(), "{body}");
}

#[tokio::test]
async fn hostile_queries_in_a_router_end_in_a_400_not_a_panic() {
    let app = app().await;
    for query in [
        "filter[__proto__][in]=a",
        "filter[stage]=%00",
        "filter[stage][null]=maybe",
        "sort=-amount_cents,-amount_cents",
        "page=99999999999999999999",
        "filter[a][b][c]=1",
    ] {
        app.get(&format!("/api/deals?{query}"))
            .await
            .assert_error(400);
    }
    // Parameters that are not the list query's are left alone.
    assert_eq!(
        app.get("/api/deals?utm_source=x&owner=3").await.status,
        StatusCode::OK
    );
}
