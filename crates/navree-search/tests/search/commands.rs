//! `search:import`, `search:flush`, `search:settings` and `search:status`,
//! and the fresh-import swap.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use navree_db::{DatabaseModule, Db};
use navree_kernel::BoxFuture;
use navree_search::{
    Document, Engine, EngineQuery, EngineResults, IndexSettings, MemoryEngine, Search,
    SearchEngine, SearchError, SearchModule, SearchOptions, SyncMode,
};
use navree_testing::TestApp;
use tokio::sync::Notify;

use crate::support::{self, Contact, Deal, insert, insert_contacts, new_contact, new_deal};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn import_indexes_every_row_and_skips_filtered_ones() {
    for &engine in support::engines() {
        let app = support::app(engine, "off").await;
        let db = app.get::<Db>().await;
        let search = app.get::<Search>().await;
        let mut gone = new_contact("Old", "Record");
        gone.deleted_at = Some(support::now());
        let rows = insert_contacts(
            &db,
            vec![
                new_contact("Ada", "Lovelace"),
                new_contact("Grace", "Hopper"),
                gone,
            ],
        )
        .await;
        app.run_command("search:import", &["Contact"])
            .await
            .unwrap();
        search.assert_indexed::<Contact>(rows[0].id).await;
        search.assert_indexed::<Contact>(rows[1].id).await;
        search.assert_not_indexed::<Contact>(rows[2].id).await;
        let status = search.status().await.unwrap();
        let contacts = status.iter().find(|s| s.name == "Contact").unwrap();
        assert_eq!((contacts.documents, contacts.rows), (2, 3), "{engine}");
        assert_eq!(contacts.engine, engine);

        app.run_command("search:flush", &["Contact"]).await.unwrap();
        search.assert_not_indexed::<Contact>(rows[0].id).await;
        app.run_command("search:import", &["--all"]).await.unwrap();
        search.assert_indexed::<Contact>(rows[0].id).await;
        app.run_command("search:settings", &[]).await.unwrap();
        app.run_command("search:status", &[]).await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn imports_run_in_chunks() {
    let app = support::builder("memory", "off")
        .config("search.chunk_size", 2)
        .build()
        .await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let rows: Vec<_> = (0..5)
        .map(|i| new_contact(&format!("Person{i}"), "Smith"))
        .collect();
    insert_contacts(&db, rows).await;
    let report = search.import::<Contact>().await.unwrap();
    assert_eq!((report.rows, report.documents), (5, 5));
    assert_eq!(report.index, "contacts");
    assert_eq!(search.query::<Contact>("smith").count().await.unwrap(), 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_imports_swap_in_a_rebuilt_index() {
    for &engine in support::engines() {
        let app = support::builder(engine, "off")
            .config("search.prefix", "crm_")
            .build()
            .await;
        let db = app.get::<Db>().await;
        let search = app.get::<Search>().await;
        let [deal] = insert(&db, vec![new_deal("Lovelace renewal", "won", 1)])
            .await
            .try_into()
            .unwrap();
        search.import::<Deal>().await.unwrap();
        // The row is archived while syncing is off; the fresh import
        // rebuilds the index from the rows and swaps it in.
        support::update(&db, &deal, |d| d.archived = true).await;
        search.assert_indexed::<Deal>(deal.id).await;
        app.run_command("search:import", &["Deal", "--fresh"])
            .await
            .unwrap();
        search.assert_not_indexed::<Deal>(deal.id).await;
        assert_eq!(
            search.engine().count("crm_deals__fresh").await.unwrap(),
            0,
            "{engine}: the staged index is gone"
        );
        let report = search.import_fresh::<Deal>().await.unwrap();
        assert_eq!(
            (report.rows, report.skipped, report.index.as_str()),
            (1, 1, "crm_deals")
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn commands_name_unknown_entities() {
    let app = support::app("memory", "off").await;
    let error = app
        .run_command("search:import", &["Invoice"])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Invoice"), "{error}");
    let error = app.run_command("search:import", &[]).await.unwrap_err();
    assert!(error.to_string().contains("usage"), "{error}");
}

/// The memory engine, with the first write into a staged (`__fresh`)
/// index held until the test releases it.
struct Gated {
    inner: MemoryEngine,
    armed: AtomicBool,
    reached: Notify,
    release: Notify,
}

impl SearchEngine for Gated {
    fn name(&self) -> &'static str {
        "gated"
    }

    fn prepare<'a>(
        &'a self,
        index: &'a str,
        settings: &'a IndexSettings,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        self.inner.prepare(index, settings)
    }

    fn upsert<'a>(
        &'a self,
        index: &'a str,
        settings: &'a IndexSettings,
        documents: Vec<Document>,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        Box::pin(async move {
            if index.ends_with("__fresh") && self.armed.swap(false, Ordering::SeqCst) {
                self.reached.notify_one();
                self.release.notified().await;
            }
            self.inner.upsert(index, settings, documents).await
        })
    }

    fn delete<'a>(
        &'a self,
        index: &'a str,
        ids: &'a [String],
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        self.inner.delete(index, ids)
    }

    fn flush<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        self.inner.flush(index)
    }

    fn drop_index<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<(), SearchError>> {
        self.inner.drop_index(index)
    }

    fn swap<'a>(
        &'a self,
        index: &'a str,
        staged: &'a str,
    ) -> BoxFuture<'a, Result<(), SearchError>> {
        self.inner.swap(index, staged)
    }

    fn search<'a>(
        &'a self,
        index: &'a str,
        query: EngineQuery<'a>,
    ) -> BoxFuture<'a, Result<EngineResults, SearchError>> {
        self.inner.search(index, query)
    }

    fn count<'a>(&'a self, index: &'a str) -> BoxFuture<'a, Result<u64, SearchError>> {
        self.inner.count(index)
    }

    fn contains<'a>(
        &'a self,
        index: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<bool, SearchError>> {
        self.inner.contains(index, id)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_imports_answer_queries_and_keep_writes_made_meanwhile() {
    let gated = Arc::new(Gated {
        inner: MemoryEngine::new(),
        armed: AtomicBool::new(true),
        reached: Notify::new(),
        release: Notify::new(),
    });
    let app = crate::servers::database(TestApp::builder())
        .module(DatabaseModule::from_config())
        .module(SearchModule::for_root(SearchOptions {
            engine: Engine::custom(gated.clone()),
            sync: SyncMode::Inline,
            ..SearchOptions::default()
        }))
        .module(support::CrmModule)
        .build()
        .await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let rows = insert_contacts(
        &db,
        vec![
            new_contact("Ada", "Lovelace"),
            new_contact("Grace", "Hopper"),
        ],
    )
    .await;
    support::settle(&app).await;
    let ada = rows[0].id.to_string();

    // The import has read the rows and waits to write the staged index.
    let importer = tokio::spawn({
        let search = search.clone();
        async move { search.import_fresh::<Contact>().await }
    });
    gated.reached.notified().await;

    // Searches keep answering from the live index...
    let found = search.query::<Contact>("lovelace").ids(10).await.unwrap();
    assert_eq!(found, std::slice::from_ref(&ada));
    // ...and writes keep syncing into it.
    support::update(&db, &rows[0], |c| c.last_name = "Byron".into()).await;
    support::settle(&app).await;
    let found = search.query::<Contact>("byron").ids(10).await.unwrap();
    assert_eq!(found, std::slice::from_ref(&ada));

    // The staged index holds the rows as they were read; records synced
    // meanwhile are synced again after the swap.
    gated.release.notify_one();
    let report = importer.await.unwrap().unwrap();
    assert_eq!(report.documents, 2);
    let found = search.query::<Contact>("byron").ids(10).await.unwrap();
    assert_eq!(found, [ada], "the write made during the import survives");
    let stale = search.query::<Contact>("lovelace").ids(10).await.unwrap();
    assert!(stale.is_empty(), "{stale:?}");
}
