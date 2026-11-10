//! The vector store conformance suite: what every `VectorStore` must do.
//! It runs on the memory store, on pgvector (in `postgres.rs`, with the
//! `postgres` feature and `NAVREE_TEST_POSTGRES_URL`) and on a Qdrant
//! server when `NAVREE_TEST_QDRANT_URL` names one.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use navree_search::{
    Filter, MemoryVectorStore, Metric, QdrantStore, VectorRecord, VectorStore, WORKSPACE_FIELD,
};
use serde_json::json;

use crate::support;

fn record(id: &str, vector: [f32; 3], stage: &str, workspace: &str) -> VectorRecord {
    VectorRecord {
        id: id.to_owned(),
        vector: vector.to_vec(),
        payload: BTreeMap::from([
            ("stage".to_owned(), json!(stage)),
            (WORKSPACE_FIELD.to_owned(), json!(workspace)),
        ]),
    }
}

async fn nearest(
    store: &dyn VectorStore,
    index: &str,
    to: [f32; 3],
    filter: Filter,
    limit: u64,
) -> Vec<String> {
    let found = store
        .nearest(index, &to, &filter.normalized(), limit)
        .await
        .unwrap();
    for pair in found.windows(2) {
        assert!(pair[0].score >= pair[1].score, "nearest first: {found:?}");
    }
    assert!(
        found.iter().all(|m| (0.0..=1.0).contains(&m.score)),
        "{found:?}"
    );
    found.into_iter().map(|m| m.id).collect()
}

/// Runs the whole suite on `store` (3 dimensions, cosine).
pub async fn vector_suite(store: Arc<dyn VectorStore>) {
    let store = store.as_ref();
    let index = "conformance_vectors";
    store.prepare(index).await.unwrap();
    store.flush(index).await.unwrap();
    store.prepare(index).await.unwrap();
    store
        .upsert(
            index,
            vec![
                record("1", [1.0, 0.0, 0.0], "won", "w1"),
                record("2", [0.8, 0.3, 0.0], "lost", "w1"),
                record("3", [0.0, 0.1, 1.0], "won", "w2"),
            ],
        )
        .await
        .unwrap();

    // Nearest first, filtered by payload values, limited.
    let to = [1.0, 0.05, 0.0];
    assert_eq!(
        nearest(store, index, to, Filter::Const(true), 10).await,
        ["1", "2", "3"]
    );
    assert_eq!(
        nearest(store, index, to, Filter::eq("stage", "won"), 10).await,
        ["1", "3"]
    );
    assert_eq!(
        nearest(store, index, to, Filter::ne("stage", "won"), 10).await,
        ["2"]
    );
    assert_eq!(
        nearest(store, index, to, Filter::Const(true), 1).await,
        ["1"]
    );
    assert_eq!(
        nearest(store, index, to, Filter::eq(WORKSPACE_FIELD, "w2"), 10).await,
        ["3"]
    );

    // A record's vector comes back. A cosine store may return it
    // normalized (Qdrant does), so compare its direction.
    let vector = store
        .get(index, "2")
        .await
        .unwrap()
        .expect("record 2 is stored");
    assert_eq!(vector.len(), 3);
    let want = [0.8f32, 0.3, 0.0];
    let dot: f32 = vector.iter().zip(want).map(|(a, b)| a * b).sum();
    let length = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    let cosine = dot / (length(&vector) * length(&want));
    assert!((cosine - 1.0).abs() < 1e-5, "{vector:?} points elsewhere");
    assert_eq!(store.get(index, "9").await.unwrap(), None);

    // Upserts replace; deletes remove; flush empties.
    store
        .upsert(index, vec![record("1", [0.0, 0.0, 1.0], "won", "w1")])
        .await
        .unwrap();
    assert_eq!(
        nearest(store, index, [0.0, 0.0, 1.0], Filter::Const(true), 1).await,
        ["1"]
    );
    store
        .delete(index, &["1".to_owned(), "99".to_owned()])
        .await
        .unwrap();
    assert_eq!(store.get(index, "1").await.unwrap(), None);
    assert_eq!(
        nearest(store, index, to, Filter::Const(true), 10).await,
        ["2", "3"]
    );
    store.flush(index).await.unwrap();
    store.prepare(index).await.unwrap();
    assert!(
        nearest(store, index, to, Filter::Const(true), 10)
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn memory_store_conforms() {
    vector_suite(Arc::new(MemoryVectorStore::new(Metric::Cosine))).await;
}

#[tokio::test]
async fn qdrant_server_conforms() {
    let Some(url) = support::service("NAVREE_TEST_QDRANT_URL", "qdrant_server_conforms") else {
        return;
    };
    let key = std::env::var("NAVREE_TEST_QDRANT_KEY").ok();
    let store = QdrantStore::new(&url, key, 3, Metric::Cosine, Duration::from_secs(10)).unwrap();
    vector_suite(Arc::new(store)).await;
}
