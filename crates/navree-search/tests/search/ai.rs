//! Vector search on navree-ai (the `ai` feature): its `Embeddings` as the
//! provider when the app registers none, and, with
//! `search.vectors.store = "ai"`, its `VectorStore` keeping search's
//! vectors next to the app's AI collections, configured once in
//! `ai.vectors`.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use navree_ai::{
    AiFake, AiModule, AiOptions, Embeddings as AiEmbeddings, VectorRecord, VectorStore as AiStore,
    VectorsOptions,
};
use navree_db::Db;
use navree_search::{AiVectorStore, Embeddings, Search};
use navree_testing::TestApp;
use serde_json::json;

use crate::support::{self, Contact, WordEmbeddings, insert_contacts, new_contact, settle};
use crate::vector_stores::vector_suite;

fn ai_embeddings(dimensions: usize) -> AiEmbeddings {
    AiEmbeddings::new(AiFake::new().ai_with(AiOptions {
        vectors: VectorsOptions {
            dimensions,
            ..VectorsOptions::default()
        },
        ..AiOptions::default()
    }))
}

fn with_notes(first: &str, last: &str, notes: &str) -> Contact {
    let mut row = new_contact(first, last);
    row.notes = Some(notes.to_owned());
    row
}

/// The contacts of the tests: two about onboarding, one about billing.
fn contacts() -> Vec<Contact> {
    vec![
        with_notes(
            "Gurpreet",
            "Sandhu",
            "struggled with onboarding the sales team",
        ),
        with_notes("Ananya", "Sharma", "billing questions about the invoice"),
        with_notes("Harjit", "Gill", "onboarding call for the sales team"),
    ]
}

/// An app with AiModule on the fake, its vectors in memory (64 numbers),
/// and `search.vectors.store = "ai"`.
async fn ai_app() -> TestApp {
    support::builder("memory", "inline")
        .config("ai.fake", true)
        .config("ai.vectors", json!({ "store": "memory", "dimensions": 64 }))
        .config("search.vectors.store", "ai")
        .module(AiModule::from_config())
        .build()
        .await
}

fn first_names(hits: &[navree_search::Hit<navree_orm::Record<Contact>>]) -> Vec<&str> {
    hits.iter()
        .map(|hit| hit.record.first_name.as_str())
        .collect()
}

#[tokio::test]
async fn navree_ais_store_passes_the_vector_store_suite() {
    vector_suite(Arc::new(AiVectorStore::new(AiStore::memory(
        ai_embeddings(3),
    ))))
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_searchable_model_and_an_ai_collection_share_one_store() {
    let app = ai_app().await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let store = app.get::<AiStore>().await;
    let rows = insert_contacts(&db, contacts()).await;
    settle(&app).await;

    // The model's vectors are the AI collection `search_contacts`, in the
    // app's one store, with its size; the filter values are the metadata.
    let indexed = store.collection("search_contacts");
    assert_eq!(indexed.count().await.unwrap(), 3);
    let nearest = indexed
        .search("onboarding the sales team")
        .limit(3)
        .await
        .unwrap();
    assert_eq!(nearest.len(), 3);
    assert!(
        nearest
            .iter()
            .all(|found| found.metadata["status"] == "lead")
    );
    assert_eq!(store.dimensions(), 64);

    // An AI collection of the app sits next to it.
    let embeddings = app.get::<AiEmbeddings>().await;
    let notes = store.collection("activity_notes");
    let text = "Onboarding call: the sales team starts on Monday";
    notes
        .upsert(
            VectorRecord::new("n1", embeddings.embed(text).await.unwrap())
                .text(text)
                .metadata(json!({ "subject": "Gurpreet" })),
        )
        .await
        .unwrap();
    assert_eq!(notes.count().await.unwrap(), 1);
    assert_eq!(indexed.count().await.unwrap(), 3);

    // Search finds records by meaning through the shared store, with the
    // same embeddings as the AI collection.
    let similar = search
        .similar::<Contact>("onboarding the sales team")
        .limit(2)
        .run()
        .await
        .unwrap();
    let mut names = first_names(&similar);
    names.sort_unstable();
    assert_eq!(names, ["Gurpreet", "Harjit"]);
    assert!(
        similar.iter().all(|hit| (0.0..=1.0).contains(&hit.score)),
        "scores are 0 to 1"
    );
    let billing = search
        .similar::<Contact>("billing questions")
        .limit(1)
        .run()
        .await
        .unwrap();
    assert_eq!(first_names(&billing), ["Ananya"]);
    let like = search
        .similar_to::<Contact>(rows[0].id)
        .limit(1)
        .run()
        .await
        .unwrap();
    assert_eq!(
        first_names(&like),
        ["Harjit"],
        "the record itself is left out"
    );

    // Deleting a contact removes its vector; flushing the index empties the
    // collection and leaves the app's own collection alone.
    support::delete(&db, &rows[1]).await;
    settle(&app).await;
    assert_eq!(indexed.count().await.unwrap(), 2);
    search.flush::<Contact>().await.unwrap();
    assert_eq!(indexed.count().await.unwrap(), 0);
    assert_eq!(notes.count().await.unwrap(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn navree_ais_embeddings_need_no_adapter_on_searchs_own_store() {
    let app = support::builder("memory", "inline")
        .config("ai.fake", true)
        .config("ai.vectors.dimensions", 64)
        .config("search.vectors.store", "memory")
        .config("search.vectors.dimensions", 64)
        .module(AiModule::from_config())
        .build()
        .await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    insert_contacts(&db, contacts()).await;
    settle(&app).await;
    let similar = search
        .similar::<Contact>("billing questions")
        .limit(1)
        .run()
        .await
        .unwrap();
    assert_eq!(first_names(&similar), ["Ananya"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_apps_own_embeddings_still_win() {
    let words = Arc::new(WordEmbeddings::default());
    let app = support::builder("memory", "inline")
        .config("ai.fake", true)
        .config("ai.vectors", json!({ "store": "memory", "dimensions": 3 }))
        .config("search.vectors.store", "ai")
        .module(AiModule::from_config())
        .override_value(words.clone() as Arc<dyn Embeddings>)
        .build()
        .await;
    let db = app.get::<Db>().await;
    insert_contacts(&db, contacts()).await;
    settle(&app).await;
    assert!(words.calls.load(Ordering::SeqCst) >= 3);
    let store = app.get::<AiStore>().await;
    assert_eq!(
        store.collection("search_contacts").count().await.unwrap(),
        3
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_ai_store_needs_ai_module() {
    let Err(error) = support::builder("memory", "inline")
        .config("search.vectors.store", "ai")
        .try_build()
        .await
    else {
        panic!("the app booted without AiModule");
    };
    assert!(error.to_string().contains("AiModule"), "{error}");
}
