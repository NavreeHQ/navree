//! Every public item the docs page (`packages/search.mdx`) names, so a
//! rename fails this build: types, methods, config keys, commands, the job
//! and the tables.

use std::sync::Arc;

use navree_access::{Accessible, Read};
use navree_db::{Page, PageQuery};
use navree_search::{
    AcrossHit, AcrossQuery, DOCUMENTS_TABLE, Document, Embeddings, Engine, FTS_TABLE, Filter, Hit,
    ImportReport, IndexSettings, IndexStatus, SEARCH_MIGRATIONS, SYNC_JOB, Search, SearchContext,
    SearchEngine, SearchError, SearchModule, SearchOptions, SearchQuery, Searchable, SimilarQuery,
    SyncDocuments, SyncMode, VECTORS_TABLE, VectorStoreKind,
};

use crate::support::{Contact, contact};

/// Compiles only if the documented query surface exists with these shapes.
#[allow(dead_code, unused_must_use, clippy::let_underscore_future)]
async fn query_surface(search: Arc<Search>, scope: Accessible<Contact, Read>, page: PageQuery) {
    let query: SearchQuery<Contact> = search
        .query::<Contact>("ada")
        .filter("status", "customer")
        .filter_not("status", "lost")
        .filter_in("owner_id", [uuid::Uuid::nil()])
        .range("score", 10..20)
        .filter_null("score")
        .filter_by(Filter::Any(vec![Filter::eq("status", "a"), Filter::null("status")]))
        .sort_asc("score")
        .sort_desc("created_at")
        .accessible(scope.clone())
        .workspace(uuid::Uuid::nil())
        .hybrid(0.4)
        .highlight_tags("<em>", "</em>");
    let _: Result<Page<contact::Model>, SearchError> = query.paginate(page).await;
    let _: Result<Page<Hit<contact::Model>>, SearchError> =
        search.query::<Contact>("ada").hits(page).await;
    let _: Result<Vec<String>, SearchError> = search.query::<Contact>("").ids(10).await;
    let _: Result<u64, SearchError> = search.query::<Contact>("").count().await;

    let similar: SimilarQuery<Contact> = search
        .similar::<Contact>("onboarding")
        .filter("status", "customer")
        .accessible(scope.clone())
        .limit(10);
    let _: Result<Vec<Hit<contact::Model>>, SearchError> = similar.run().await;
    let _ = search.similar_to::<Contact>(uuid::Uuid::nil()).limit(5).run().await;

    let across: AcrossQuery = search.across("ada").include::<Contact>(scope).limit(10);
    let hits: Vec<AcrossHit> = across.run().await.unwrap();
    for hit in hits {
        let (_kind, _id, _title, _score) = (&hit.kind, &hit.id, &hit.title, hit.score);
    }

    let _: Result<(), SearchError> = search.reindex::<Contact>([uuid::Uuid::nil()]).await;
    let _: Result<ImportReport, SearchError> = search.import::<Contact>().await;
    let _: Result<ImportReport, SearchError> = search.import_fresh::<Contact>().await;
    let _: Result<(), SearchError> = search.flush::<Contact>().await;
    let _: Result<(), SearchError> = search.push_settings::<Contact>().await;
    let _: Result<Vec<IndexStatus>, SearchError> = search.status().await;
    let _: u8 = search.without_syncing(async { 1u8 }).await;
    let _: Result<(), SearchError> = search.flush_pending().await;
    search.assert_indexed::<Contact>(uuid::Uuid::nil()).await;
    search.assert_not_indexed::<Contact>(uuid::Uuid::nil()).await;
    let _: &Arc<dyn SearchEngine> = search.engine();
    let _: &SearchOptions = search.options();
}

#[test]
fn the_documented_names_exist() {
    // Modules and options.
    let _ = SearchModule::for_root(Engine::tantivy("storage/search"));
    let _ = SearchModule::for_root(SearchOptions {
        engine: Engine::meilisearch("http://127.0.0.1:7700").key("k"),
        prefix: "crm_".to_owned(),
        ..SearchOptions::default()
    });
    let _ = SearchModule::from_config();
    let _ = SearchModule::for_feature::<Contact>();
    for engine in [
        Engine::postgres(),
        Engine::meilisearch("http://x"),
        Engine::typesense("http://x").nodes(["http://y"]),
        Engine::memory(),
    ] {
        assert!(
            ["postgres", "meilisearch", "typesense", "memory"].contains(&engine.name()),
            "{engine:?}"
        );
    }
    assert_eq!(SyncMode::default(), SyncMode::Queued);
    let _ = [VectorStoreKind::Pgvector, VectorStoreKind::Qdrant, VectorStoreKind::Memory];

    // Settings and documents.
    let settings = IndexSettings::new("Deal", "deals")
        .text("title", 3)
        .filter("stage")
        .sort("amount_cents")
        .embed("notes")
        .soft_delete("deleted_at")
        .workspace("workspace_id");
    settings.validate().unwrap();
    let _ = Document::new(1).field("title", "x").workspace("w");
    let _: fn() -> IndexSettings = <Contact as Searchable>::settings;
    let _ = SearchContext::new;

    // The job, tables and migration set.
    assert_eq!(SYNC_JOB, "search.sync");
    assert_eq!(<SyncDocuments as navree_contracts::Job>::NAME, "search.sync");
    assert_eq!(SEARCH_MIGRATIONS, "search");
    assert_eq!(
        (DOCUMENTS_TABLE, FTS_TABLE, VECTORS_TABLE),
        ("search_documents", "search_documents_fts", "search_vectors")
    );
    let _: Option<Arc<dyn Embeddings>> = None;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_documented_commands_and_config_keys_exist() {
    let app = crate::support::builder("memory", "off")
        .config("search.prefix", "crm_")
        .config("search.queue", "search")
        .config("search.unique_for_secs", 5)
        .config("search.chunk_size", 500)
        .config("search.timeout_secs", 5)
        .config("search.postgres.language", "english")
        .config("search.postgres.trigram", false)
        .config("search.tantivy.path", "storage/search")
        .config("search.tantivy.memory_budget_mb", 64)
        .config("search.indexes.companies.synonyms", serde_json::json!({"acme": ["acme corp"]}))
        .config("search.indexes.companies.stop_words", serde_json::json!(["inc"]))
        .build()
        .await;
    let commands: Vec<&str> = app.ctx().commands().iter().map(|c| c.name).collect();
    for name in ["search:import", "search:flush", "search:settings", "search:status"] {
        assert!(commands.contains(&name), "{name} missing from {commands:?}");
    }
    let options = app.get::<SearchOptions>().await;
    assert_eq!(options.indexes["companies"].stop_words, ["inc"]);
}
