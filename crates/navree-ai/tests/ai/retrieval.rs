//! Embeddings, the vector stores (one conformance suite for memory, Qdrant
//! and pgvector), and reranking.

use navree_ai::{
    Ai, AiError, AiFake, AiModule, AiOptions, Embedding, Embeddings, Filter, ModelConfig,
    ProviderConfig, VectorMatch, VectorRecord, VectorStore, VectorsOptions,
};
use navree_testing::TestApp;
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::json;

fn embeddings(dimensions: usize) -> Embeddings {
    Embeddings::new(AiFake::new().ai_with(AiOptions {
        vectors: VectorsOptions {
            dimensions,
            ..VectorsOptions::default()
        },
        ..AiOptions::default()
    }))
}

#[tokio::test]
async fn fake_embeddings_are_deterministic_and_close_for_shared_words() {
    let embeddings = embeddings(64);

    let a = embeddings
        .embed("Dana wants to be live before January planning")
        .await
        .unwrap();
    let b = embeddings
        .embed("When do they want to be live before January?")
        .await
        .unwrap();
    let c = embeddings.embed("Globex prefers the summer").await.unwrap();

    assert_eq!(a.dimensions(), 64);
    assert!(a.cosine(&b) > a.cosine(&c));
    assert_eq!(
        a,
        embeddings
            .embed("Dana wants to be live before January planning")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn embed_many_batches_in_order_and_caches_by_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(|request: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let data: Vec<Value> = body["input"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    let n = text.as_str().unwrap().len() as f64;
                    json!({ "index": i, "embedding": [n, 1.0] })
                })
                .collect();
            ResponseTemplate::new(200)
                .set_body_json(json!({ "data": data, "usage": { "prompt_tokens": 3 } }))
        })
        .mount(&server)
        .await;
    let ai = Ai::new(AiOptions {
        providers: [(
            "openai".to_owned(),
            ProviderConfig::openai("k").url(server.uri()),
        )]
        .into(),
        models: [(
            "embed".to_owned(),
            ModelConfig::new("openai", "text-embedding-3-small").dimensions(2),
        )]
        .into(),
        embeddings: navree_ai::EmbeddingsOptions {
            model: Some("embed".into()),
            batch_size: 2,
            ..Default::default()
        },
        vectors: VectorsOptions {
            dimensions: 2,
            ..VectorsOptions::default()
        },
        ..AiOptions::default()
    })
    .unwrap();
    let embeddings = Embeddings::new(ai);

    let vectors = embeddings.embed_many(["a", "bb", "ccc"]).await.unwrap();

    assert_eq!(
        vectors.iter().map(|v| v[0]).collect::<Vec<_>>(),
        [1.0, 2.0, 3.0]
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2, "three texts in batches of two");
    let first: Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(first["dimensions"], 2);
    assert_eq!(first["model"], "text-embedding-3-small");

    // Cached by text: only the new one is sent.
    let again = embeddings.embed_many(["ccc", "dddd"]).await.unwrap();
    assert_eq!(again[1][0], 4.0);
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn a_size_other_than_ai_vectors_dimensions_fails_naming_both() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(json(
            200,
            json!({ "data": [{ "index": 0, "embedding": [1.0, 0.0, 0.0] }] }),
        ))
        .mount(&server)
        .await;
    let ai = Ai::new(AiOptions {
        providers: [(
            "openai".to_owned(),
            ProviderConfig::openai("k").url(server.uri()),
        )]
        .into(),
        models: [("embed".to_owned(), ModelConfig::new("openai", "x"))].into(),
        embeddings: navree_ai::EmbeddingsOptions {
            model: Some("embed".into()),
            ..Default::default()
        },
        ..AiOptions::default()
    })
    .unwrap();

    let error = Embeddings::new(ai).embed("hello").await.unwrap_err();

    match error {
        AiError::Config { message } => assert!(
            message.contains("3 dimensions") && message.contains("1536"),
            "{message}"
        ),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn embeddings_without_a_model_are_a_configuration_error() {
    let ai = Ai::new(AiOptions::default()).unwrap();
    let error = Embeddings::new(ai).embed("x").await.unwrap_err();
    assert!(matches!(error, AiError::Config { .. }));
}

/// The vector store conformance suite: what every store must do.
async fn conformance(store: VectorStore, embeddings: &Embeddings, collection: &str) {
    let notes = store.collection(collection);
    notes
        .delete_where(Filter::eq("suite", "conformance"))
        .await
        .unwrap();
    let texts = [
        (
            "a1:0",
            "Dana wants to be live before January planning",
            "d1",
            "call",
            3,
        ),
        (
            "a2:0",
            "Pricing call: a 12-month term with a discount",
            "d1",
            "note",
            5,
        ),
        (
            "a3:0",
            "Globex wants to be live before the summer",
            "d2",
            "call",
            8,
        ),
    ];
    let mut records = Vec::new();
    for (id, text, deal, kind, size) in texts {
        records.push(
            VectorRecord::new(id, embeddings.embed(text).await.unwrap())
                .text(text)
                .metadata(json!({ "suite": "conformance", "subject_id": deal, "kind": kind, "size": size, "created_at": format!("2026-10-0{size}T00:00:00Z") })),
        );
    }
    notes.upsert_many(records).await.unwrap();
    assert_eq!(notes.count().await.unwrap(), 3);

    // Best first, by meaning.
    let matches = notes.search("live before January").limit(3).await.unwrap();
    assert_eq!(matches[0].id, "a1:0");
    assert_eq!(
        matches[0].text,
        "Dana wants to be live before January planning"
    );
    assert_eq!(matches[0].metadata["subject_id"], "d1");
    assert!(matches.windows(2).all(|w| w[0].score >= w[1].score));

    // Filters apply inside the search.
    let in_deal = notes
        .search("live before")
        .filter(Filter::eq("subject_id", "d2"))
        .limit(1)
        .await
        .unwrap();
    assert_eq!(ids(&in_deal), ["a3:0"]);
    let either = notes
        .search("call")
        .filter(Filter::any_of("kind", ["note"]))
        .await
        .unwrap();
    assert_eq!(ids(&either), ["a2:0"]);
    let big = notes
        .search("x")
        .filter(Filter::gte("size", 5))
        .await
        .unwrap();
    assert_eq!(sorted(ids(&big)), ["a2:0", "a3:0"]);
    let early = notes
        .search("x")
        .filter(Filter::lt("created_at", "2026-10-05T00:00:00Z"))
        .await
        .unwrap();
    assert_eq!(ids(&early), ["a1:0"]);
    let not_calls = notes
        .search("x")
        .filter(Filter::not(Filter::eq("kind", "call")))
        .await
        .unwrap();
    assert_eq!(ids(&not_calls), ["a2:0"]);
    let combined = notes
        .search("x")
        .filter(
            Filter::eq("kind", "call")
                .and(Filter::gt("size", 4))
                .or(Filter::eq("subject_id", "nope")),
        )
        .await
        .unwrap();
    assert_eq!(ids(&combined), ["a3:0"]);

    // min_score drops distant records.
    let close = notes
        .search("Dana wants to be live before January planning")
        .min_score(0.99)
        .await
        .unwrap();
    assert_eq!(ids(&close), ["a1:0"]);

    // Upsert replaces by id; delete and delete_where remove.
    let replacement = embeddings.embed("Replaced text").await.unwrap();
    notes
        .upsert(
            VectorRecord::new("a1:0", replacement.clone())
                .text("Replaced text")
                .metadata(json!({ "suite": "conformance", "subject_id": "d1" })),
        )
        .await
        .unwrap();
    assert_eq!(notes.count().await.unwrap(), 3);
    let found = notes.search(replacement.clone()).limit(1).await.unwrap();
    assert_eq!(found[0].text, "Replaced text");
    notes.delete("a2:0").await.unwrap();
    assert_eq!(notes.count().await.unwrap(), 2);

    // A record comes back by id, with its text, metadata and vector.
    let got = notes.get("a3:0").await.unwrap().expect("a3:0 is stored");
    assert_eq!(got.id, "a3:0");
    assert_eq!(got.text, "Globex wants to be live before the summer");
    assert_eq!(got.metadata["subject_id"], "d2");
    assert_eq!(got.embedding.dimensions(), store.dimensions());
    assert!(notes.get("a2:0").await.unwrap().is_none());

    notes
        .delete_where(Filter::eq("subject_id", "d2"))
        .await
        .unwrap();
    assert_eq!(notes.count().await.unwrap(), 1);

    // delete_many skips ids that are not there; clear empties the
    // collection, which stays usable.
    notes
        .upsert(
            VectorRecord::new("a4:0", replacement.clone())
                .metadata(json!({ "suite": "conformance" })),
        )
        .await
        .unwrap();
    notes.delete_many(["a4:0", "missing"]).await.unwrap();
    assert_eq!(notes.count().await.unwrap(), 1);
    notes.clear().await.unwrap();
    assert_eq!(notes.count().await.unwrap(), 0);
    assert!(notes.search(replacement).await.unwrap().is_empty());
    assert!(notes.get("a1:0").await.unwrap().is_none());
}

fn ids(matches: &[VectorMatch]) -> Vec<&str> {
    matches.iter().map(|m| m.id.as_str()).collect()
}

fn sorted(mut ids: Vec<&str>) -> Vec<&str> {
    ids.sort_unstable();
    ids
}

#[tokio::test]
async fn the_memory_store_passes_the_conformance_suite() {
    let embeddings = embeddings(64);
    conformance(
        VectorStore::memory(embeddings.clone()),
        &embeddings,
        "activity_notes",
    )
    .await;
}

#[tokio::test]
async fn records_of_the_wrong_size_and_bad_names_are_refused() {
    let embeddings = embeddings(8);
    let store = VectorStore::memory(embeddings);

    let error = store
        .collection("notes")
        .upsert(VectorRecord::new("x", Embedding::new(vec![1.0; 3])))
        .await
        .unwrap_err();
    assert!(matches!(error, AiError::Config { .. }));
    let error = store.collection("Bad-Name").count().await.unwrap_err();
    assert!(matches!(error, AiError::Config { .. }));
    let error = store
        .collection("notes")
        .search(Embedding::new(vec![0.0; 8]))
        .filter(Filter::eq("x'; drop table", 1))
        .await
        .unwrap_err();
    assert!(matches!(error, AiError::Config { .. }));
}

#[tokio::test]
async fn the_qdrant_store_speaks_the_rest_api() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/collections/notes"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({ "status": { "error": "Not found" } })),
        )
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/collections/notes"))
        .and(body_partial_json(
            json!({ "vectors": { "size": 4, "distance": "Cosine" } }),
        ))
        .respond_with(json(200, json!({ "result": true, "status": "ok" })))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/collections/notes/points"))
        .respond_with(json(
            200,
            json!({ "result": { "status": "completed" }, "status": "ok" }),
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/collections/notes/points/search"))
        .and(body_partial_json(json!({
            "limit": 5,
            "with_payload": true,
            "filter": { "must": [{ "key": "metadata.subject_id", "match": { "value": "d1" } }] }
        })))
        .respond_with(json(200, json!({ "result": [
            { "id": "6a2f...", "score": 0.91, "payload": { "id": "a1:0", "text": "Dana...", "metadata": { "subject_id": "d1" } } }
        ], "status": "ok" })))
        .mount(&server)
        .await;
    let app = TestApp::builder()
        .config("ai.fake", true)
        .config(
            "ai.vectors",
            json!({ "store": "qdrant", "dimensions": 4, "url": server.uri(), "key": "qdrant-key" }),
        )
        .module(AiModule::from_config())
        .build()
        .await;
    let store = app.get::<VectorStore>().await;
    let notes = store.collection("notes");

    notes
        .upsert(
            VectorRecord::new("a1:0", Embedding::new(vec![0.5; 4]))
                .text("Dana...")
                .metadata(json!({ "subject_id": "d1" })),
        )
        .await
        .unwrap();
    let matches = notes
        .search(Embedding::new(vec![0.5; 4]))
        .filter(Filter::eq("subject_id", "d1"))
        .limit(5)
        .await
        .unwrap();

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id, "a1:0");
    assert!((matches[0].score - 0.91).abs() < 1e-6);
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|r| r.headers.get("api-key").is_some_and(|v| v == "qdrant-key"))
    );
    let upsert = requests
        .iter()
        .find(|r| r.url.path() == "/collections/notes/points")
        .unwrap();
    let body: Value = serde_json::from_slice(&upsert.body).unwrap();
    assert_eq!(body["points"][0]["payload"]["id"], "a1:0");
    assert_eq!(body["points"][0]["payload"]["metadata"]["subject_id"], "d1");
    assert!(
        body["points"][0]["id"].as_str().unwrap().len() == 36,
        "a UUID point id"
    );
}

/// Runs the suite against a real Qdrant (`NAVREE_TEST_QDRANT_URL`).
#[tokio::test]
async fn the_qdrant_store_passes_the_conformance_suite() {
    let Ok(url) = std::env::var("NAVREE_TEST_QDRANT_URL") else {
        println!(
            "SKIPPED the_qdrant_store_passes_the_conformance_suite: set NAVREE_TEST_QDRANT_URL"
        );
        assert!(
            std::env::var("NAVREE_TEST_REQUIRE_SERVICES").is_err(),
            "NAVREE_TEST_QDRANT_URL is required"
        );
        return;
    };
    let app = TestApp::builder()
        .config("ai.fake", true)
        .config(
            "ai.vectors",
            json!({ "store": "qdrant", "dimensions": 64, "url": url }),
        )
        .module(AiModule::from_config())
        .build()
        .await;
    let embeddings = app.get::<Embeddings>().await;
    let collection = format!("conformance_{}", uuid::Uuid::now_v7().simple());
    conformance(
        (*app.get::<VectorStore>().await).clone(),
        &embeddings,
        &collection,
    )
    .await;
}

/// Runs the suite against PostgreSQL with pgvector
/// (`NAVREE_TEST_PGVECTOR_URL`, feature `postgres`).
#[cfg(feature = "postgres")]
#[tokio::test]
async fn the_pgvector_store_passes_the_conformance_suite() {
    let Ok(url) = std::env::var("NAVREE_TEST_PGVECTOR_URL") else {
        println!(
            "SKIPPED the_pgvector_store_passes_the_conformance_suite: set NAVREE_TEST_PGVECTOR_URL"
        );
        assert!(
            std::env::var("NAVREE_TEST_REQUIRE_SERVICES").is_err(),
            "NAVREE_TEST_PGVECTOR_URL is required"
        );
        return;
    };
    let app = TestApp::builder()
        .config("database.url", url)
        .config("ai.fake", true)
        .config(
            "ai.vectors",
            json!({ "store": "pgvector", "dimensions": 64 }),
        )
        .module(navree_db::DatabaseModule::from_config())
        .module(AiModule::from_config())
        .build()
        .await;
    let embeddings = app.get::<Embeddings>().await;
    let collection = format!("conformance_{}", uuid::Uuid::now_v7().simple());
    conformance(
        (*app.get::<VectorStore>().await).clone(),
        &embeddings,
        &collection,
    )
    .await;
}

#[tokio::test]
async fn the_fake_reranker_keeps_the_order() {
    let fake = AiFake::new();
    let matches = vec![
        VectorMatch {
            id: "a".into(),
            score: 0.9,
            text: "A".into(),
            metadata: json!({}),
        },
        VectorMatch {
            id: "b".into(),
            score: 0.8,
            text: "B".into(),
            metadata: json!({}),
        },
        VectorMatch {
            id: "c".into(),
            score: 0.7,
            text: "C".into(),
            metadata: json!({}),
        },
    ];

    let best = fake
        .ai()
        .rerank("question", matches)
        .model("fast")
        .top(2)
        .await
        .unwrap();

    assert_eq!(ids(&best), ["a", "b"]);
    fake.assert_nothing_prompted();
}

#[tokio::test]
async fn a_model_reranks_in_one_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(json(200, json!({
            "choices": [{ "message": { "role": "assistant", "content": "{\"scores\":[{\"index\":1,\"score\":0.1},{\"index\":2,\"score\":0.95},{\"index\":3,\"score\":0.5}]}" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 50, "completion_tokens": 20 }
        })))
        .mount(&server)
        .await;
    let ai = Ai::new(crate::support::provider_options(
        "openai",
        &server.uri(),
        "fast",
        "gpt-5",
    ))
    .unwrap();
    let matches = vec![
        VectorMatch {
            id: "a".into(),
            score: 0.9,
            text: "About pricing".into(),
            metadata: json!({}),
        },
        VectorMatch {
            id: "b".into(),
            score: 0.8,
            text: "Live before January".into(),
            metadata: json!({}),
        },
        VectorMatch {
            id: "c".into(),
            score: 0.7,
            text: "Onboarding".into(),
            metadata: json!({}),
        },
    ];

    let best = ai
        .rerank("When do they go live?", matches)
        .top(2)
        .await
        .unwrap();

    assert_eq!(ids(&best), ["b", "c"]);
    assert!((best[0].score - 0.95).abs() < 1e-6);
    let body: Value =
        serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(body["response_format"]["type"], "json_schema");
    assert!(
        body["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("[2] Live before January")
    );
}
