//! The RAG endpoint of `ai/embeddings-and-rag`, end to end with the fake:
//! index passages, search inside one deal, rerank, then a structured answer
//! that cites its sources by number.

use navree_ai::{
    Ai, AiModule, Chunker, Embeddings, FakeReply, Filter, ModuleBuilderAiExt, VectorCollection,
    VectorMatch, VectorRecord, VectorStore,
};
use navree_kernel::{Module, ModuleBuilder};
use navree_openapi::ToSchema;
use navree_testing::TestApp;
use navree_validation::Validate;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

/// The model's answer, citing sources by their number.
#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
pub struct GroundedAnswer {
    /// The answer in at most four sentences, using only the sources.
    #[validate(length(min = 1, max = 2000))]
    pub answer: String,
    /// The numbers of the sources the answer uses, e.g. [1, 3].
    #[validate(skip)]
    pub sources: Vec<usize>,
    /// False when the sources don't contain the answer.
    #[validate(skip)]
    pub answered: bool,
}

/// A source the answer used.
#[derive(Debug, PartialEq)]
struct Source {
    activity_id: Uuid,
    excerpt: String,
}

/// Declares the collection, as the activities module does.
struct ActivitiesModule;

impl Module for ActivitiesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.vector_collection(VectorCollection::new("activity_notes"));
    }
}

/// `IndexActivity`: replaces an activity's chunks.
async fn index(app: &TestApp, activity_id: Uuid, deal_id: Uuid, text: &str) {
    let embeddings = app.get::<Embeddings>().await;
    let notes = app.get::<VectorStore>().await.collection("activity_notes");
    notes
        .delete_where(Filter::eq("activity_id", activity_id))
        .await
        .unwrap();
    let chunks = Chunker::new().max_tokens(300).overlap(40).split(text);
    let vectors = embeddings
        .embed_many(chunks.iter().map(|c| c.text.as_str()))
        .await
        .unwrap();
    let records = chunks.into_iter().zip(vectors).map(|(chunk, embedding)| {
        VectorRecord::new(format!("{activity_id}:{}", chunk.index), embedding)
            .text(chunk.text)
            .metadata(json!({ "activity_id": activity_id, "subject_id": deal_id }))
    });
    notes.upsert_many(records.collect()).await.unwrap();
}

/// `DealQuestions::ask`.
async fn ask(app: &TestApp, deal_id: Uuid, question: &str) -> (GroundedAnswer, Vec<Source>) {
    let ai = app.get::<Ai>().await;
    let candidates = app
        .get::<VectorStore>()
        .await
        .collection("activity_notes")
        .search(question)
        .filter(Filter::eq("subject_id", deal_id))
        .limit(25)
        .await
        .unwrap();
    let passages: Vec<VectorMatch> = ai
        .rerank(question, candidates)
        .model("fast")
        .top(6)
        .await
        .unwrap();
    let numbered = passages
        .iter()
        .enumerate()
        .map(|(i, p)| format!("[{}] {}", i + 1, p.text))
        .collect::<Vec<_>>()
        .join("\n\n");
    let reply = ai
        .structured::<GroundedAnswer>(format!("Sources:\n{numbered}\n\nQuestion: {question}"))
        .system("You answer using only the numbered sources.")
        .model("smart")
        .validated()
        .await
        .unwrap();
    let sources = reply
        .sources
        .iter()
        .filter_map(|n| passages.get(n.checked_sub(1)?))
        .map(|p| Source {
            activity_id: p.metadata["activity_id"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or_default(),
            excerpt: p.text.chars().take(240).collect(),
        })
        .collect();
    (reply, sources)
}

#[tokio::test]
async fn answers_use_only_the_deals_own_activity() {
    let app = TestApp::builder()
        .config("ai.fake", true)
        .config("ai.vectors.store", "memory")
        .module(AiModule::from_config())
        .module(ActivitiesModule)
        .build()
        .await;
    let fake = app.get::<Ai>().await.fake().unwrap();
    let (agra, gulati) = (Uuid::now_v7(), Uuid::now_v7());
    let (agra_note, gulati_note) = (Uuid::now_v7(), Uuid::now_v7());
    index(
        &app,
        agra_note,
        agra,
        "Daljit wants to be live before January planning",
    )
    .await;
    index(
        &app,
        gulati_note,
        gulati,
        "Gulati wants to be live before the summer",
    )
    .await;

    // The answer cites source 1 and a source that doesn't exist.
    fake.respond(FakeReply::json(json!({
        "answer": "Before January planning [1].",
        "sources": [1, 7],
        "answered": true
    })));

    let (reply, sources) = ask(&app, agra, "When do they want to be live?").await;

    assert!(reply.answered);
    assert_eq!(
        sources,
        [Source {
            activity_id: agra_note,
            excerpt: "Daljit wants to be live before January planning".into()
        }],
        "the invented source 7 is dropped"
    );
    // One structured call (the fake reranker makes none), with only Agra's note.
    fake.assert_prompt_count(1);
    fake.assert_prompted(|r| {
        r.is_structured() && r.last_user_message().contains("January planning")
    });
    fake.assert_not_prompted(|r| r.last_user_message().contains("Gulati"));
    // Indexing embedded both notes, and asking embedded the question.
    assert_eq!(fake.embedded().len(), 3);
}

#[tokio::test]
async fn reindexing_replaces_an_activitys_chunks() {
    let app = TestApp::builder()
        .config("ai.fake", true)
        .module(AiModule::from_config())
        .module(ActivitiesModule)
        .build()
        .await;
    let notes = app.get::<VectorStore>().await.collection("activity_notes");
    let (deal, activity) = (Uuid::now_v7(), Uuid::now_v7());
    let long = "Pricing call with Daljit. ".repeat(200);

    index(&app, activity, deal, &long).await;
    let before = notes.count().await.unwrap();
    index(&app, activity, deal, "Short now.").await;

    assert!(before > 1, "a long note is several chunks");
    assert_eq!(notes.count().await.unwrap(), 1, "the old chunks are gone");
    // The cache: re-indexing the same text embeds nothing new.
    let fake = app.get::<Ai>().await.fake().unwrap();
    let embedded = fake.embedded().len();
    index(&app, activity, deal, "Short now.").await;
    assert_eq!(fake.embedded().len(), embedded);
}
