//! The store conformance suite: the memory store and the SQLite store
//! behave the same.

use std::sync::Arc;

use chrono::{Duration, Utc};
use navree_logbook::{
    Entry, EntryKind, Filter, ListQuery, LogbookStore, MemoryStore, SqliteStore, StatusMatch, Term,
};

use navree_testing::TempDirGuard;

use crate::support::{dump, entry_at, query, request};

/// Runs the whole suite against a fresh store from `make`.
macro_rules! conformance {
    ($module:ident, $make:expr) => {
        mod $module {
            use super::*;

            #[tokio::test]
            async fn inserts_and_lists_newest_first() {
                let (store, _guard) = $make.await;
                inserts_and_lists_newest_first_on(store).await;
            }

            #[tokio::test]
            async fn pages_with_before_and_after() {
                let (store, _guard) = $make.await;
                pages_with_before_and_after_on(store).await;
            }

            #[tokio::test]
            async fn filters_every_term() {
                let (store, _guard) = $make.await;
                filters_every_term_on(store).await;
            }

            #[tokio::test]
            async fn keeps_tags_per_batch() {
                let (store, _guard) = $make.await;
                keeps_tags_per_batch_on(store).await;
            }

            #[tokio::test]
            async fn finds_entries_and_batches() {
                let (store, _guard) = $make.await;
                finds_entries_and_batches_on(store).await;
            }

            #[tokio::test]
            async fn prunes_whole_batches_by_age() {
                let (store, _guard) = $make.await;
                prunes_whole_batches_by_age_on(store).await;
            }

            #[tokio::test]
            async fn prunes_whole_batches_by_count() {
                let (store, _guard) = $make.await;
                prunes_whole_batches_by_count_on(store).await;
            }

            #[tokio::test]
            async fn clears_everything() {
                let (store, _guard) = $make.await;
                clears_everything_on(store).await;
            }
        }
    };
}

async fn memory() -> (Arc<dyn LogbookStore>, Option<TempDirGuard>) {
    (Arc::new(MemoryStore::new()), None)
}

async fn sqlite() -> (Arc<dyn LogbookStore>, Option<TempDirGuard>) {
    let dir = TempDirGuard::new();
    let store = SqliteStore::open(dir.path().join("storage").join("logbook.db"))
        .await
        .unwrap();
    (Arc::new(store), Some(dir))
}

conformance!(memory_store, memory());
conformance!(sqlite_store, sqlite());

async fn list(store: &dyn LogbookStore, filter: &str) -> Vec<Entry> {
    store
        .list(&ListQuery::new(Filter::parse(filter).unwrap(), 100))
        .await
        .unwrap()
}

fn titles(entries: &[Entry]) -> Vec<String> {
    entries.iter().map(Entry::title).collect()
}

async fn inserts_and_lists_newest_first_on(store: Arc<dyn LogbookStore>) {
    let now = Utc::now();
    store
        .insert(vec![
            entry_at(now, None, dump("first")),
            entry_at(now, None, dump("second")),
        ])
        .await
        .unwrap();
    store
        .insert(vec![entry_at(now, None, dump("third"))])
        .await
        .unwrap();
    let all = list(&*store, "").await;
    assert_eq!(titles(&all), ["third", "second", "first"]);
    assert!(all[0].seq > all[1].seq && all[1].seq > all[2].seq);
    assert_eq!(store.count().await.unwrap(), 3);
}

async fn pages_with_before_and_after_on(store: Arc<dyn LogbookStore>) {
    let now = Utc::now();
    let entries: Vec<Entry> = (0..5)
        .map(|i| entry_at(now, None, dump(&format!("e{i}"))))
        .collect();
    store.insert(entries).await.unwrap();
    let all = list(&*store, "").await;
    let page = store
        .list(&ListQuery {
            filter: Filter::all(),
            before: Some(all[1].seq),
            after: None,
            limit: 2,
        })
        .await
        .unwrap();
    assert_eq!(titles(&page), ["e2", "e1"]);
    let newer = store
        .list(&ListQuery {
            filter: Filter::all(),
            before: None,
            after: Some(all[2].seq),
            limit: 10,
        })
        .await
        .unwrap();
    assert_eq!(titles(&newer), ["e4", "e3"]);
}

async fn filters_every_term_on(store: Arc<dyn LogbookStore>) {
    let now = Utc::now();
    let mut deals = entry_at(
        now,
        Some("req-1"),
        request("GET", "/api/v1/deals/7", "/api/v1/deals/{id}", 404),
    );
    deals.user_id = Some("0192F0C1-aaaa".into());
    deals.duration_ms = Some(250.0);
    deals.trace_id = Some("4bf92f35".into());
    deals.tags = vec!["deal:0199c1d2-6f7e".into()];
    let mut slow_query = entry_at(now, Some("req-1"), query("SELECT * FROM deals", 300.0));
    slow_query.slow = true;
    slow_query.duration_ms = Some(300.0);
    slow_query.trace_id = Some("4bf92f35".into());
    let mut failed = entry_at(
        now,
        Some("req-2"),
        request("POST", "/api/v1/companies", "/api/v1/companies", 500),
    );
    failed.failed = true;
    failed.duration_ms = Some(5.0);
    let standalone = entry_at(now, None, dump("boot"));
    store
        .insert(vec![slow_query, deals, failed, standalone])
        .await
        .unwrap();

    // Without type:/batch:/trace:, only batches and loose entries.
    assert_eq!(
        titles(&list(&*store, "").await),
        ["boot", "POST /api/v1/companies", "GET /api/v1/deals/7"]
    );
    assert_eq!(
        titles(&list(&*store, "user:0192f0c1").await),
        ["GET /api/v1/deals/7"]
    );
    assert_eq!(
        titles(&list(&*store, "tag:DEAL:0199c1d2").await),
        ["GET /api/v1/deals/7"]
    );
    assert_eq!(
        titles(&list(&*store, "status:4xx").await),
        ["GET /api/v1/deals/7"]
    );
    assert_eq!(
        titles(&list(&*store, "status:500").await),
        ["POST /api/v1/companies"]
    );
    assert_eq!(
        titles(&list(&*store, "route:/api/v1/deals/{id}").await),
        ["GET /api/v1/deals/7"]
    );
    assert_eq!(
        titles(&list(&*store, "type:query").await),
        ["SELECT * FROM deals"]
    );
    assert_eq!(
        titles(&list(&*store, "type:query type:request").await).len(),
        3,
        "type: terms are alternatives"
    );
    assert_eq!(
        titles(&list(&*store, "slow type:query").await),
        ["SELECT * FROM deals"]
    );
    assert!(list(&*store, "slow").await.is_empty(), "no slow batch");
    assert_eq!(
        titles(&list(&*store, "failed").await),
        ["POST /api/v1/companies"]
    );
    assert_eq!(
        titles(&list(&*store, "duration>200").await),
        ["GET /api/v1/deals/7"]
    );
    assert_eq!(
        titles(&list(&*store, "duration<10").await),
        ["POST /api/v1/companies"]
    );
    assert_eq!(
        titles(&list(&*store, "batch:req-1").await),
        ["GET /api/v1/deals/7", "SELECT * FROM deals"]
    );
    assert_eq!(list(&*store, "trace:4BF9").await.len(), 2);
    assert_eq!(
        titles(&list(&*store, "deals").await),
        ["GET /api/v1/deals/7"]
    );
    assert_eq!(
        titles(&list(&*store, "type:query FROM").await),
        ["SELECT * FROM deals"],
        "free text is case-insensitive"
    );
    assert_eq!(
        titles(&list(&*store, "user:0192 status:4xx route:/api/v1/deals/{id}").await),
        ["GET /api/v1/deals/7"]
    );
    let filter = Filter::of([Term::Status(StatusMatch::Class(2))]);
    assert!(
        store
            .list(&ListQuery::new(filter, 10))
            .await
            .unwrap()
            .is_empty()
    );
}

async fn keeps_tags_per_batch_on(store: Arc<dyn LogbookStore>) {
    let now = Utc::now();
    store
        .add_tags("req-1".into(), vec!["import:42".into()])
        .await
        .unwrap();
    let mut head = entry_at(now, Some("req-1"), request("GET", "/x", "/x", 200));
    head.tags = vec!["status:200".into(), "import:42".into()];
    store
        .insert(vec![
            entry_at(now, Some("req-1"), query("SELECT 1", 1.0)),
            head,
        ])
        .await
        .unwrap();
    store
        .add_tags("req-1".into(), vec!["late".into()])
        .await
        .unwrap();
    let batch = store.batch("req-1").await.unwrap();
    assert_eq!(batch.len(), 2);
    for entry in &batch {
        assert_eq!(entry.tags, ["import:42", "late", "status:200"]);
    }
    assert_eq!(list(&*store, "tag:late").await.len(), 1);
    assert_eq!(list(&*store, "tag:import type:query").await.len(), 1);
}

async fn finds_entries_and_batches_on(store: Arc<dyn LogbookStore>) {
    let now = Utc::now();
    let entry = entry_at(now, Some("job-1"), query("SELECT 2", 2.5));
    let id = entry.id.clone();
    store.insert(vec![entry.clone()]).await.unwrap();
    let found = store.entry(&id).await.unwrap().unwrap();
    assert_eq!(found.content, entry.content);
    assert_eq!(found.batch_id.as_deref(), Some("job-1"));
    assert_eq!(found.at.timestamp_micros(), now.timestamp_micros());
    assert!(store.entry("nope").await.unwrap().is_none());
    assert!(store.batch("nope").await.unwrap().is_empty());
    assert_eq!(found.kind(), EntryKind::Query);
}

async fn prunes_whole_batches_by_age_on(store: Arc<dyn LogbookStore>) {
    let now = Utc::now();
    let old = now - Duration::hours(30);
    store
        .insert(vec![
            entry_at(old, Some("old"), query("SELECT old", 1.0)),
            entry_at(now, Some("old"), request("GET", "/old", "/old", 200)),
            entry_at(now, Some("new"), request("GET", "/new", "/new", 200)),
        ])
        .await
        .unwrap();
    store
        .add_tags("old".into(), vec!["gone".into()])
        .await
        .unwrap();
    let deleted = store.prune(1000, now - Duration::hours(24)).await.unwrap();
    assert_eq!(deleted, 2, "the old batch goes whole");
    assert_eq!(titles(&list(&*store, "").await), ["GET /new"]);
    assert!(list(&*store, "tag:gone").await.is_empty());
}

async fn prunes_whole_batches_by_count_on(store: Arc<dyn LogbookStore>) {
    let now = Utc::now();
    store
        .insert(vec![
            entry_at(now, Some("a"), query("SELECT a", 1.0)),
            entry_at(now, Some("b"), query("SELECT b1", 1.0)),
            entry_at(now, Some("a"), request("GET", "/a", "/a", 200)),
            entry_at(now, Some("b"), request("GET", "/b", "/b", 200)),
            entry_at(now, None, dump("loose")),
        ])
        .await
        .unwrap();
    // Keeping 3 means dropping the 2 oldest entries, which belong to
    // batches a and b: both go whole.
    let deleted = store.prune(3, now - Duration::hours(24)).await.unwrap();
    assert_eq!(deleted, 4);
    assert_eq!(titles(&list(&*store, "").await), ["loose"]);
    assert_eq!(store.prune(3, now - Duration::hours(24)).await.unwrap(), 0);
}

async fn clears_everything_on(store: Arc<dyn LogbookStore>) {
    let now = Utc::now();
    store
        .insert(vec![
            entry_at(now, None, dump("x")),
            entry_at(now, None, dump("y")),
        ])
        .await
        .unwrap();
    assert_eq!(store.clear().await.unwrap(), 2);
    assert_eq!(store.count().await.unwrap(), 0);
    store
        .insert(vec![entry_at(now, None, dump("z"))])
        .await
        .unwrap();
    assert_eq!(titles(&list(&*store, "").await), ["z"]);
}

#[tokio::test]
async fn the_file_store_survives_a_restart() {
    let dir = TempDirGuard::new();
    let path = dir.path().join("logbook.db");
    {
        let store = SqliteStore::open(&path).await.unwrap();
        store
            .insert(vec![entry_at(Utc::now(), None, dump("before the restart"))])
            .await
            .unwrap();
        store.add_tags("x".into(), vec!["t".into()]).await.unwrap();
    }
    let reopened = SqliteStore::open(&path).await.unwrap();
    assert_eq!(reopened.path(), path);
    assert_eq!(titles(&list(&reopened, "").await), ["before the restart"]);
}
