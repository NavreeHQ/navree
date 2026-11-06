//! Every name, key and default the AI docs pages (`ai/ai-sdk`,
//! `ai/agents-and-tools`, `ai/embeddings-and-rag`) and the `navree-ai`
//! skill use exists: a rename fails here. The `#[allow(dead_code)]`
//! functions are type-checked, not run; the tests check values.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use navree_access::{CurrentAccess, PrincipalAccess};
use navree_ai::{
    AgentOptions, AgentReply, AgentStream, Ai, AiError, AiFake, AiModule, AiOptions, Attachment,
    Chunk, Chunker, Completion, Conversations, Embedding, Embeddings, FakeReply, FakeRequest,
    Filter, Finish, Memory, Message, Metric, ModelConfig, ModuleBuilderAiExt, ProviderConfig,
    StoredMessage, StreamEvent, TextResponse, TextStream, Usage, VectorCollection, VectorMatch,
    VectorRecord, VectorStore, VectorStoreKind,
};
use navree_config::{Config, ConfigSources};
use navree_db::PageQuery;
use navree_http::AppError;
use navree_http::response::{IntoResponse, Response};
use navree_kernel::{Module, ModuleBuilder};
use navree_openapi::ToSchema;
use navree_validation::Validate;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::agents::NotesAssistant;

/// A follow-up email the rep can edit before sending (`ai/ai-sdk`).
#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
pub struct FollowUpEmail {
    /// The subject line, without a prefix like "Re:".
    #[validate(length(min = 1, max = 80))]
    pub subject: String,
    /// The plain-text body, signed with the rep's first name.
    #[validate(length(min = 1, max = 2000))]
    pub body: String,
    /// The day to send it, when the activity suggests one.
    #[validate(skip)]
    pub send_on: Option<chrono::NaiveDate>,
}

/// How a customer email reads.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Sentiment {
    Positive,
    Neutral,
    /// The customer is unhappy, considering leaving, or asking to cancel.
    AtRisk,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, ToSchema)]
pub struct Triage {
    pub sentiment: Sentiment,
    /// One sentence for the account owner.
    pub reason: String,
}

/// `ai/ai-sdk`: the service, the request options, responses, attachments,
/// structured output, streams and errors.
#[allow(dead_code)]
async fn ai_sdk_page(
    ai: Arc<Ai>,
    disk: navree_storage::Disk,
    access: PrincipalAccess,
) -> Result<(), AppError> {
    let response: TextResponse = ai
        .text(format!("Summarize this deal's activity:\n\n{}", "..."))
        .system("You write two-sentence updates for sales managers.")
        .model("fast")
        .max_tokens(200)
        .temperature(0.2)
        .stop(["END"])
        .attach(Attachment::image(vec![0u8], "image/png"))
        .attach(Attachment::pdf(vec![0u8]))
        .user(access.user)
        .user(Uuid::now_v7())
        .timeout(Duration::from_secs(20))
        .await?;
    let (_text, _model, _provider): (String, String, String) = (
        response.text.clone(),
        response.model.clone(),
        response.provider.clone(),
    );
    let Usage {
        input_tokens: _,
        output_tokens: _,
        cached_input_tokens: _,
    } = response.usage;
    let _ = matches!(
        response.finish,
        Finish::Stop | Finish::MaxTokens | Finish::Refused
    );
    let _: String = response.to_string();

    ai.text(vec![
        Message::user("Draft a one-line subject for the Acme renewal email."),
        Message::assistant("Your Acme renewal: two options for next year"),
        Message::user("Shorter, and without the colon."),
    ])
    .await?;

    let _: ContractLike = ai
        .structured::<ContractLike>("Extract the commercial terms from this contract.")
        .attach(Attachment::from_disk(&disk, "contracts/acme.pdf").await?)
        .model("smart")
        .await?;
    let _: FollowUpEmail = ai
        .structured::<FollowUpEmail>("Write a follow-up")
        .validated()
        .await?;
    let _: Triage = ai
        .structured::<Triage>("I want to cancel.")
        .model("fast")
        .await?;

    let stream: TextStream = ai
        .stream("Summarize the deal")
        .system("Two sentences.")
        .model("fast")
        .await?;
    let stream = stream.on_finish(move |response: TextResponse| async move {
        let _ = response.text;
        Ok::<(), AppError>(())
    });
    let _: Response = stream.into_response();

    let mut stream = ai
        .stream("Draft")
        .model("fast")
        .user(Uuid::now_v7())
        .await?
        .chunked(Duration::from_millis(100));
    while let Some(event) = stream.next().await {
        match event? {
            StreamEvent::Text(_text) => {}
            StreamEvent::Done(done) => {
                let _: (String, Usage, Finish) = (done.model, done.usage, done.finish);
                let _: Option<Completion> = None;
            }
            _ => continue,
        }
    }

    let _: Option<AiFake> = ai.fake();
    // An optional AI feature: handle the error at the call site.
    let _summary = match ai.text("Summarize").model("fast").await {
        Ok(response) => Some(response.text),
        Err(error) if error.status().is_server_error() => None,
        Err(error) => return Err(error.into()),
    };
    Ok(())
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, ToSchema)]
struct ContractLike {
    /// The term in months.
    term_months: u32,
}

/// `ai/agents-and-tools`: running, approving, streaming and the services.
#[allow(dead_code)]
async fn agents_page(
    ai: Arc<Ai>,
    conversations: Arc<Conversations>,
    CurrentAccess(access): CurrentAccess,
    user: Uuid,
    conversation_id: Uuid,
) -> Result<(), AiError> {
    let reply: AgentReply = ai
        .agent::<NotesAssistant>()
        .acting_as(access.clone())
        .conversation(Some(conversation_id))
        .prompt("Which notes are stale?")
        .await?;
    let AgentReply {
        conversation_id: _,
        text: _,
        steps,
        pending_approval,
        usage: _,
    } = reply;
    let _ = steps
        .iter()
        .map(|step| (&step.tool, &step.arguments, step.ok));
    if let Some(pending) = pending_approval {
        let _ = (&pending.call_id, &pending.tool, &pending.arguments);
    }

    ai.agent::<NotesAssistant>()
        .acting_as_user(user)
        .read_only()
        .model("smart")
        .max_steps(4)
        .prompt("Review")
        .await?;
    ai.agent::<NotesAssistant>()
        .acting_as(access.clone())
        .conversation(Some(conversation_id))
        .approve("toolu_01")
        .await?;
    ai.agent::<NotesAssistant>()
        .acting_as(access.clone())
        .conversation(Some(conversation_id))
        .reject("toolu_01", Some("Not now".to_owned()))
        .await?;
    let _: Triage = ai
        .agent::<NotesAssistant>()
        .acting_as(access.clone())
        .structured::<Triage>("Classify")
        .await?;
    let stream: AgentStream = ai
        .agent::<NotesAssistant>()
        .acting_as(access)
        .conversation(None)
        .stream("Hello")
        .await?;
    let _: Response = stream.into_response();

    let page = conversations.list_for(user, PageQuery::default()).await?;
    let _ = page
        .data
        .iter()
        .map(|c| (&c.id, &c.agent, &c.title, c.created_at, c.updated_at));
    let messages: Vec<StoredMessage> = conversations.messages(user, conversation_id).await?;
    let _ = messages.iter().map(|m| (&m.role, &m.text, &m.parts));
    conversations
        .rename(user, conversation_id, "Q4 pipeline")
        .await?;
    conversations.delete(user, conversation_id).await?;
    Ok(())
}

/// `m.agent::<A>()` and `m.vector_collection(..)`.
struct DocsModule;

impl Module for DocsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.agent::<NotesAssistant>()
            .vector_collection(VectorCollection::new("activity_notes"));
    }
}

/// `ai/embeddings-and-rag`: embeddings, collections, filters, chunks and
/// reranking.
#[allow(dead_code)]
async fn embeddings_page(
    ai: Arc<Ai>,
    embeddings: Arc<Embeddings>,
    vectors: Arc<VectorStore>,
    deal_id: Uuid,
    ids: Vec<Uuid>,
) -> Result<(), AiError> {
    let one: Embedding = embeddings
        .embed("Dana asked whether onboarding takes more than a month")
        .await?;
    let many: Vec<Embedding> = embeddings
        .embed_many(&["first passage", "second passage"])
        .await?;
    let _: usize = one.dimensions();
    let _: f32 = one.cosine(&many[0]);
    let _: &[f32] = &one;

    let notes = vectors.collection("activity_notes");
    notes
        .upsert(
            VectorRecord::new("0199c1d2:0", one.clone())
                .text("Pricing call with Dana.")
                .metadata(json!({ "activity_id": Uuid::now_v7(), "subject_id": deal_id })),
        )
        .await?;
    let chunks: Vec<Chunk> = Chunker::new()
        .max_tokens(300)
        .overlap(40)
        .split("A long note.");
    let vectors_of_chunks = embeddings
        .embed_many(chunks.iter().map(|c| c.text.as_str()))
        .await?;
    let records = chunks
        .into_iter()
        .zip(vectors_of_chunks)
        .map(|(chunk, embedding)| {
            VectorRecord::new(format!("a:{}", chunk.index), embedding).text(chunk.text)
        });
    notes.upsert_many(records.collect()).await?;
    notes.delete("0199c1d2:0").await?;
    notes.delete_many(["a:0", "a:1"]).await?;
    notes
        .delete_where(Filter::eq("activity_id", Uuid::now_v7()))
        .await?;
    let _: Option<VectorRecord> = notes.get("a:2").await?;
    let _: u64 = notes.count().await?;
    let _: usize = vectors.dimensions();
    let _: Metric = vectors.metric();
    notes.clear().await?;

    let question = String::from("what did they say about the rollout timeline");
    let matches: Vec<VectorMatch> = notes
        .search(&question)
        .filter(
            Filter::eq("subject_id", deal_id)
                .and(Filter::any_of("subject_id", ids))
                .or(Filter::not(Filter::gte("created_at", "2026-10-01")))
                .and(
                    Filter::gt("n", 1)
                        .or(Filter::lt("n", 0))
                        .or(Filter::lte("n", 9)),
                ),
        )
        .limit(8)
        .min_score(0.2)
        .await?;
    for m in &matches {
        let _ = (&m.id, m.score, &m.text, &m.metadata["activity_id"]);
    }
    let _ = notes.search(one).limit(3).await?;
    let best: Vec<VectorMatch> = ai.rerank(&question, matches).model("fast").top(6).await?;
    let _ = best;
    let _ = Chunker::markdown().split("# Pricing\n\n## Discounts\n\nTen percent.");
    Ok(())
}

#[test]
fn the_documented_configuration_reads_from_toml() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let path = dir.path().join("app.toml");
    std::fs::write(
        &path,
        r#"
[ai]
default = "smart"           # the model used when a request names none

[ai.models.smart]
provider = "anthropic"
model = "claude-opus-5-5"
fallbacks = ["fast"]        # tried in order when `smart` is unavailable

[ai.models.fast]
provider = "anthropic"
model = "claude-sonnet-5-5"
max_tokens = 1024
fallbacks = ["backup"]

[ai.models.backup]
provider = "openai"
model = "gpt-5"

[ai.models.embed]
provider = "openai"
model = "text-embedding-3-small"
dimensions = 1536

[ai.embeddings]
model = "embed"

[ai.vectors]
store = "pgvector"          # pgvector | qdrant | memory
dimensions = 1536           # must match the embedding model
metric = "cosine"           # cosine | dot | euclidean

[ai.providers.local]
driver = "openai"
url = "http://127.0.0.1:8000/v1"

[ai.models.private]
provider = "local"
model = "qwen3-32b"
"#,
    )
    .unwrap();
    let sources = ConfigSources::new().file(&path).env_vars(
        "APP_",
        [
            ("APP_AI__PROVIDERS__ANTHROPIC__API_KEY", "sk-ant-api03-..."),
            ("APP_AI__PROVIDERS__OPENAI__API_KEY", "sk-proj-..."),
            ("APP_AI__MODELS__SMART__FALLBACKS", "fast,backup"),
        ],
    );
    let config = Config::load(&sources).unwrap();
    let options: AiOptions = config.typed().unwrap();

    assert!(options.check().is_empty(), "{:?}", options.check());
    assert_eq!(options.default, "smart");
    assert_eq!(options.models["smart"].fallbacks, ["fast", "backup"]);
    assert_eq!(options.models["fast"].max_tokens, 1024);
    assert_eq!(options.models["backup"].max_tokens, 4096);
    assert_eq!(options.models["embed"].dimensions, Some(1536));
    assert_eq!(options.embeddings.model.as_deref(), Some("embed"));
    assert_eq!(options.vectors.store, VectorStoreKind::Pgvector);
    assert_eq!(options.vectors.metric, Metric::Cosine);
    assert_eq!(options.providers["local"].driver.as_deref(), Some("openai"));
    assert_eq!(
        options.providers["anthropic"]
            .api_key
            .as_ref()
            .map(|key| key.expose().as_str()),
        Some("sk-ant-api03-...")
    );

    // `Config::redacted()` masks every key.
    let redacted = config.redacted();
    assert_eq!(
        redacted["ai"]["providers"]["anthropic"]["api_key"],
        "[redacted]"
    );
    assert_eq!(
        redacted["ai"]["providers"]["openai"]["api_key"],
        "[redacted]"
    );
}

#[test]
fn every_documented_key_and_default() {
    let options: AiOptions = serde_json::from_value(json!({
        "default": "fast",
        "timeout_secs": 30,
        "max_retries": 1,
        "structured_retries": 3,
        "budget": { "daily_tokens_per_user": 200000 },
        "log_content": true,
        "fake": true,
        "providers": {
            "anthropic": { "api_key": "a", "url": "https://gateway.internal" },
            "openai": { "api_key": "o", "url": "https://openai.internal/v1", "organization": "org-1" },
            "gemini": { "api_key": "g" },
            "mistral": { "api_key": "m" },
            "ollama": { "url": "http://127.0.0.1:11434" },
            "bedrock": { "region": "us-east-1", "access_key_id": "AKID", "secret_access_key": "s" }
        },
        "models": {
            "fast": {
                "provider": "anthropic", "model": "claude-sonnet-5-5", "max_tokens": 1024,
                "temperature": 0.2, "fallbacks": "backup", "timeout_secs": 20
            },
            "backup": { "provider": "openai", "model": "gpt-5" },
            "embed": { "provider": "openai", "model": "text-embedding-3-small", "dimensions": 1536 }
        },
        "embeddings": { "model": "embed", "batch_size": 32, "cache_ttl_days": 0 },
        "vectors": { "store": "qdrant", "dimensions": 1536, "metric": "dot", "url": "http://127.0.0.1:6333", "key": "q" },
        "memory": { "max_messages": 20, "summary_model": "fast", "retention_days": 30 }
    }))
    .unwrap();
    assert!(options.check().is_empty(), "{:?}", options.check());
    assert_eq!(options.budget.daily_tokens_per_user, 200_000);
    assert_eq!(options.models["fast"].fallbacks, ["backup"]);
    assert_eq!(options.models["fast"].temperature, Some(0.2));
    assert_eq!(options.models["fast"].timeout_secs, Some(20));
    assert_eq!(
        options.providers["openai"].organization.as_deref(),
        Some("org-1")
    );
    assert_eq!(
        options.providers["bedrock"].region.as_deref(),
        Some("us-east-1")
    );
    assert_eq!(options.embeddings.batch_size, 32);
    assert_eq!(options.embeddings.cache_ttl_days, 0);
    assert_eq!(options.vectors.metric, Metric::Dot);
    assert_eq!(
        options
            .vectors
            .key
            .as_ref()
            .map(|key| key.expose().as_str()),
        Some("q")
    );
    assert_eq!(options.memory.summary_model.as_deref(), Some("fast"));
    assert_eq!(options.memory.retention_days, 30);

    // The defaults the pages list.
    let defaults = AiOptions::default();
    assert_eq!(
        (
            defaults.timeout_secs,
            defaults.max_retries,
            defaults.structured_retries
        ),
        (60, 2, 2)
    );
    assert_eq!(defaults.budget.daily_tokens_per_user, 0);
    assert!(!defaults.log_content && !defaults.fake);
    assert_eq!(
        (
            defaults.embeddings.batch_size,
            defaults.embeddings.cache_ttl_days
        ),
        (96, 30)
    );
    assert_eq!(defaults.vectors.store, VectorStoreKind::Memory);
    assert_eq!(
        (defaults.vectors.dimensions, defaults.vectors.metric),
        (1536, Metric::Cosine)
    );
    assert_eq!(
        (defaults.memory.max_messages, defaults.memory.retention_days),
        (40, 90)
    );
    assert_eq!(ModelConfig::default().max_tokens, 4096);
    let agent = AgentOptions::default();
    assert_eq!(agent.model, None);
    assert_eq!(agent.max_steps, 10);
    assert_eq!(agent.memory, Memory::Conversation);
    assert!(agent.parallel_tools);
    assert_eq!(agent.tool_timeout_secs, 30);
    assert_ne!(Memory::None, Memory::Conversation);
}

#[test]
fn configuring_in_code() {
    let options = AiOptions {
        default: "local".into(),
        providers: [(
            "ollama".to_owned(),
            ProviderConfig::ollama("http://127.0.0.1:11434"),
        )]
        .into(),
        models: [("local".to_owned(), ModelConfig::new("ollama", "llama3.3"))].into(),
        ..AiOptions::default()
    };
    assert!(options.check().is_empty());
    let _ = AiModule::for_root(options);
    let _ = AiModule::from_config();
    let _ = DocsModule;
    let _ = (
        ProviderConfig::anthropic("k"),
        ProviderConfig::openai("k"),
        ProviderConfig::openai_compatible("http://127.0.0.1:8000/v1"),
        ProviderConfig::gemini("k"),
        ProviderConfig::mistral("k"),
        ProviderConfig::bedrock("us-east-1"),
    );
}

#[test]
fn errors_and_their_statuses() {
    let cases = [
        (AiError::unavailable("anthropic"), 503),
        (
            AiError::rate_limited("anthropic", Some(Duration::from_secs(3))),
            429,
        ),
        (
            AiError::BudgetExceeded {
                retry_after: Duration::from_secs(60),
            },
            429,
        ),
        (
            AiError::InvalidOutput {
                message: "x".into(),
            },
            502,
        ),
        (
            AiError::Refused {
                model: "fast".into(),
            },
            502,
        ),
        (AiError::request("openai", "too long"), 500),
        (
            AiError::Unsupported {
                message: "pdf".into(),
            },
            500,
        ),
        (AiError::config("unknown model `x`"), 500),
        (
            AiError::NoFakeReply {
                prompt: "hi".into(),
            },
            500,
        ),
    ];
    for (error, status) in cases {
        assert_eq!(error.status().as_u16(), status, "{error}");
    }
    assert_eq!(
        AiError::unavailable("anthropic").to_app_error().body(),
        json!({ "statusCode": 503, "message": "The AI provider is unavailable", "error": "Service Unavailable", "code": "service_unavailable" })
    );
}

#[tokio::test]
async fn the_fake_api() {
    let fake = AiFake::new();
    // No retries, so the scripted error is the answer rather than retried.
    let ai = fake.ai_with(AiOptions {
        max_retries: 0,
        ..AiOptions::default()
    });
    fake.respond(FakeReply::text("Two sentences.").with_usage(10, 2));
    fake.respond(FakeReply::json(
        json!({ "sentiment": "neutral", "reason": "Fine." }),
    ));
    fake.respond(FakeReply::error(AiError::unavailable("anthropic")));
    fake.respond_with([FakeReply::text("first"), FakeReply::text("second")]);
    fake.respond_when(
        |request| request.model() == "rare",
        FakeReply::text("always"),
    );
    let _ = FakeReply::tool_call("move_deal", json!({ "deal_id": 1 }));

    let response = ai
        .text("Summarize Acme")
        .model("fast")
        .max_tokens(77)
        .user(Uuid::nil())
        .await
        .unwrap();
    assert_eq!(response.usage, Usage::new(10, 2));
    let _: Triage = ai.structured("Classify").await.unwrap();
    assert!(matches!(
        ai.text("x").await,
        Err(AiError::Unavailable { .. })
    ));
    assert_eq!(ai.text("rare").model("rare").await.unwrap().text, "always");

    let requests: Vec<FakeRequest> = fake.requests();
    let first = &requests[0];
    assert_eq!(first.model(), "fast");
    assert_eq!(first.system(), None);
    assert_eq!(first.messages().len(), 1);
    assert_eq!(first.last_user_message(), "Summarize Acme");
    assert!(first.tools().is_empty());
    assert_eq!(first.max_tokens(), 77);
    assert_eq!(first.user(), Some(Uuid::nil()));
    fake.assert_prompted(|r| r.last_user_message().contains("Acme"));
    fake.assert_not_prompted(|r| r.last_user_message().contains("Globex"));
    fake.assert_prompt_count(requests.len());
    fake.assert_tool_not_called("move_deal");
    assert!(fake.tool_results("move_deal").is_empty());
    AiFake::new().assert_nothing_prompted();
}
