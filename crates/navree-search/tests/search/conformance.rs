//! The engine conformance suite: what every `SearchEngine` must do. It
//! runs on the memory engine, the `postgres` engine on SQLite (FTS5), the
//! Tantivy engine (with the `tantivy` feature), and on Meilisearch and
//! Typesense servers when `NAVREE_TEST_MEILISEARCH_URL` /
//! `NAVREE_TEST_TYPESENSE_URL` name one.

use std::sync::Arc;

use navree_search::{
    Document, EngineQuery, EngineResults, Filter, HighlightTags, IndexSettings, MemoryEngine,
    Search, SearchEngine, SortBy, WORKSPACE_FIELD,
};

use crate::support;

/// What an engine supports beyond the common ground.
#[derive(Clone, Copy)]
pub struct Caps {
    /// Typo tolerance.
    pub typos: bool,
    /// Filters on missing values (`filter_null`).
    pub nulls: bool,
}

fn settings() -> IndexSettings {
    IndexSettings::new("Deal", "deals")
        .text("title", 3)
        .text("company_name", 1)
        .filter("stage")
        .filter("amount")
        .sort("amount")
        .filter("closes")
        .sort("closes")
        .sort("signed_on")
}

fn doc(id: &str, title: &str, company: &str, stage: &str, amount: i64) -> Document {
    Document::new(id)
        .field("title", title)
        .field("company_name", company)
        .field("stage", stage)
        .field("amount", amount)
}

/// Five deals in `index`, two of them in workspace `w2`.
async fn seed(engine: &dyn SearchEngine, index: &str) {
    let settings = settings();
    engine.drop_index(index).await.unwrap();
    engine.prepare(index, &settings).await.unwrap();
    let docs = vec![
        doc(
            "1",
            "Lakhotia renewal",
            "Analytical Engines",
            "negotiation",
            5_000,
        )
        .field("closes", 1_000)
        .workspace("w1"),
        doc(
            "2",
            "Bhambri expansion",
            "Lakhotia Analytics",
            "won",
            12_000,
        )
        .field("closes", 3_000)
        .field("signed_on", "2026-01-15")
        .workspace("w1"),
        doc("3", "Hundal <migration>", "Navy & Co", "negotiation", 900).workspace("w1"),
        doc("4", "Tiwari audit", "Bletchley", "lost", 2_000)
            .field("closes", 2_000)
            .field("signed_on", "2025-12-31")
            .workspace("w2"),
        doc("5", "Lakhotia pilot", "Bletchley", "won", 7_000)
            .field("signed_on", "2026-03-01")
            .workspace("w2"),
    ];
    engine.upsert(index, &settings, docs).await.unwrap();
}

async fn search(
    engine: &dyn SearchEngine,
    index: &str,
    text: &str,
    filter: Filter,
    sort: &[SortBy],
    offset: u64,
    limit: u64,
) -> EngineResults {
    let settings = settings();
    let filter = filter.normalized();
    let tags = HighlightTags::default();
    engine
        .search(
            index,
            EngineQuery {
                settings: &settings,
                text,
                filter: &filter,
                sort,
                offset,
                limit,
                highlight: Some(&tags),
                with_title: true,
            },
        )
        .await
        .unwrap()
}

fn ids(results: &EngineResults) -> Vec<&str> {
    results.hits.iter().map(|h| h.id.as_str()).collect()
}

fn sorted(mut ids: Vec<&str>) -> Vec<&str> {
    ids.sort_unstable();
    ids
}

/// Runs the whole suite on `engine`.
pub async fn suite(engine: Arc<dyn SearchEngine>, caps: Caps) {
    let engine = engine.as_ref();
    let index = "conformance_deals";
    seed(engine, index).await;

    // Count and membership.
    assert_eq!(engine.count(index).await.unwrap(), 5);
    assert!(engine.contains(index, "1").await.unwrap());
    assert!(!engine.contains(index, "99").await.unwrap());

    // An empty text matches everything.
    let all = search(engine, index, "", Filter::Const(true), &[], 0, 10).await;
    assert_eq!(all.total, 5);
    assert_eq!(sorted(ids(&all)), ["1", "2", "3", "4", "5"]);

    // Every word must match; the last one may be a prefix.
    let found = search(
        engine,
        index,
        "lakhotia ren",
        Filter::Const(true),
        &[],
        0,
        10,
    )
    .await;
    assert_eq!(ids(&found), ["1"]);
    let found = search(engine, index, "lak", Filter::Const(true), &[], 0, 10).await;
    assert_eq!(sorted(ids(&found)), ["1", "2", "5"]);

    // A match in a heavier field ranks first.
    let found = search(engine, index, "lakhotia", Filter::Const(true), &[], 0, 10).await;
    assert_eq!(found.total, 3);
    assert_eq!(
        found.hits.last().map(|h| h.id.as_str()),
        Some("2"),
        "the company name weighs less than the title"
    );
    assert!(found.hits.iter().all(|h| (0.0..=1.0).contains(&h.score)));

    // Highlights mark the matches and escape the text; titles come back.
    let found = search(engine, index, "hundal", Filter::Const(true), &[], 0, 10).await;
    assert_eq!(ids(&found), ["3"]);
    assert_eq!(
        found.hits[0].highlights.get("title").map(String::as_str),
        Some("<mark>Hundal</mark> &lt;migration&gt;")
    );
    assert!(!found.hits[0].highlights.contains_key("company_name"));
    assert_eq!(found.hits[0].title.as_deref(), Some("Hundal <migration>"));

    // Filters.
    let won = search(engine, index, "", Filter::eq("stage", "won"), &[], 0, 10).await;
    assert_eq!(sorted(ids(&won)), ["2", "5"]);
    let not_won = search(engine, index, "", Filter::ne("stage", "won"), &[], 0, 10).await;
    assert_eq!(sorted(ids(&not_won)), ["1", "3", "4"]);
    let some = search(
        engine,
        index,
        "",
        Filter::is_in("stage", ["lost", "won"]),
        &[],
        0,
        10,
    )
    .await;
    assert_eq!(sorted(ids(&some)), ["2", "4", "5"]);
    let range = search(
        engine,
        index,
        "",
        Filter::range("amount", 2_000..=7_000),
        &[],
        0,
        10,
    )
    .await;
    assert_eq!(sorted(ids(&range)), ["1", "4", "5"]);
    let combined = search(
        engine,
        index,
        "lakhotia",
        Filter::All(vec![
            Filter::eq("stage", "won"),
            Filter::range("amount", ..10_000),
        ]),
        &[],
        0,
        10,
    )
    .await;
    assert_eq!(ids(&combined), ["5"]);
    let either = search(
        engine,
        index,
        "",
        Filter::Any(vec![
            Filter::eq("stage", "lost"),
            Filter::range("amount", 10_000..),
        ]),
        &[],
        0,
        10,
    )
    .await;
    assert_eq!(sorted(ids(&either)), ["2", "4"]);
    let workspace = search(
        engine,
        index,
        "",
        Filter::eq(WORKSPACE_FIELD, "w2"),
        &[],
        0,
        10,
    )
    .await;
    assert_eq!(sorted(ids(&workspace)), ["4", "5"]);
    let nothing = search(
        engine,
        index,
        "lakhotia",
        Filter::eq("stage", "lost"),
        &[],
        0,
        10,
    )
    .await;
    assert_eq!(nothing.total, 0);
    if caps.nulls {
        let open = search(engine, index, "", Filter::null("closes"), &[], 0, 10).await;
        assert_eq!(sorted(ids(&open)), ["3", "5"]);
    }

    // Sorting, values first, then documents without one.
    let by_amount = search(
        engine,
        index,
        "",
        Filter::Const(true),
        &[SortBy {
            field: "amount".into(),
            descending: true,
        }],
        0,
        10,
    )
    .await;
    assert_eq!(ids(&by_amount), ["2", "5", "1", "4", "3"]);
    let by_close = search(
        engine,
        index,
        "",
        Filter::Const(true),
        &[SortBy {
            field: "closes".into(),
            descending: false,
        }],
        0,
        3,
    )
    .await;
    assert_eq!(ids(&by_close), ["1", "4", "2"]);
    // Dates are text: sorted as text, and documents without one still go
    // last when sorting down.
    for (descending, expected) in [(false, ["4", "2", "5"]), (true, ["5", "2", "4"])] {
        let sort = [SortBy {
            field: "signed_on".into(),
            descending,
        }];
        let by_signing = search(engine, index, "", Filter::Const(true), &sort, 0, 3).await;
        assert_eq!(ids(&by_signing), expected, "descending: {descending}");
    }

    // Pages.
    let sort = [SortBy {
        field: "amount".into(),
        descending: false,
    }];
    let first = search(engine, index, "", Filter::Const(true), &sort, 0, 2).await;
    let second = search(engine, index, "", Filter::Const(true), &sort, 2, 2).await;
    let count_only = search(engine, index, "", Filter::Const(true), &sort, 0, 0).await;
    assert_eq!((first.total, second.total, count_only.total), (5, 5, 5));
    assert_eq!(ids(&first), ["3", "4"]);
    assert_eq!(ids(&second), ["1", "5"]);
    assert!(count_only.hits.is_empty());

    // Typo tolerance.
    if caps.typos {
        let found = search(engine, index, "lakotia", Filter::Const(true), &[], 0, 10).await;
        assert_eq!(sorted(ids(&found)), ["1", "2", "5"]);
    }

    // Upserts replace; deletes remove.
    let settings = settings();
    engine
        .upsert(
            index,
            &settings,
            vec![doc(
                "1",
                "Lakhotia renewal 2027",
                "Analytical Engines",
                "won",
                5_000,
            )],
        )
        .await
        .unwrap();
    let won = search(engine, index, "", Filter::eq("stage", "won"), &[], 0, 10).await;
    assert_eq!(sorted(ids(&won)), ["1", "2", "5"]);
    engine
        .delete(index, &["1".to_owned(), "99".to_owned()])
        .await
        .unwrap();
    assert_eq!(engine.count(index).await.unwrap(), 4);
    assert!(!engine.contains(index, "1").await.unwrap());

    // A fresh index swapped in replaces the documents at once.
    let staged = "conformance_deals__fresh";
    engine.drop_index(staged).await.unwrap();
    engine.prepare(staged, &settings).await.unwrap();
    engine
        .upsert(
            staged,
            &settings,
            vec![doc("9", "Fresh start", "New Co", "won", 1)],
        )
        .await
        .unwrap();
    let before = search(engine, index, "fresh", Filter::Const(true), &[], 0, 10).await;
    assert_eq!(
        before.total, 0,
        "the staged index is not live before the swap"
    );
    engine.swap(index, staged).await.unwrap();
    let after = search(engine, index, "", Filter::Const(true), &[], 0, 10).await;
    assert_eq!(ids(&after), ["9"]);
    assert_eq!(engine.count(index).await.unwrap(), 1);

    // Flush empties the index.
    engine.flush(index).await.unwrap();
    assert_eq!(engine.count(index).await.unwrap(), 0);
    engine.drop_index(index).await.unwrap();
}

#[tokio::test]
async fn memory_engine_conforms() {
    suite(
        Arc::new(MemoryEngine::new()),
        Caps {
            typos: true,
            nulls: true,
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn database_engine_on_sqlite_conforms() {
    use crate::servers::Server;
    if !support::runs_on(&[Server::Sqlite], "database_engine_on_sqlite_conforms") {
        return;
    }
    let app = support::app("postgres", "off").await;
    let search = app.get::<Search>().await;
    suite(
        search.engine().clone(),
        Caps {
            typos: false,
            nulls: true,
        },
    )
    .await;
}

#[cfg(feature = "tantivy")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tantivy_engine_conforms() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let engine = navree_search::TantivyEngine::open(&navree_search::TantivyOptions {
        path: dir.path().to_path_buf(),
        memory_budget_mb: 15,
    })
    .unwrap();
    suite(
        Arc::new(engine),
        Caps {
            typos: true,
            nulls: true,
        },
    )
    .await;
}

#[tokio::test]
async fn meilisearch_server_conforms() {
    let Some(url) = support::service("NAVREE_TEST_MEILISEARCH_URL", "meilisearch_server_conforms")
    else {
        return;
    };
    let key = std::env::var("NAVREE_TEST_MEILISEARCH_KEY").ok();
    let engine =
        navree_search::MeilisearchEngine::new(&url, key, std::time::Duration::from_secs(10))
            .unwrap();
    suite(
        Arc::new(engine),
        Caps {
            typos: true,
            nulls: true,
        },
    )
    .await;
}

#[tokio::test]
async fn typesense_server_conforms() {
    let Some(url) = support::service("NAVREE_TEST_TYPESENSE_URL", "typesense_server_conforms")
    else {
        return;
    };
    let key = std::env::var("NAVREE_TEST_TYPESENSE_KEY").unwrap_or_default();
    let engine =
        navree_search::TypesenseEngine::new(&url, &[], key, std::time::Duration::from_secs(10))
            .unwrap();
    suite(
        Arc::new(engine),
        Caps {
            typos: true,
            nulls: false,
        },
    )
    .await;
}
