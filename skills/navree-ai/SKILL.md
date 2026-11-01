---
name: navree-ai
description: "Language models in a Navree app with navree-ai: the opt-in `ai` facade feature, `AiModule` and the `ai` config section (providers anthropic, openai and OpenAI-compatible servers, gemini, mistral, ollama, bedrock; named models with fallbacks), the `Ai` service (`ai.text`, `ai.structured::<T>` with a JSON Schema from `ToSchema` and `#[validate]` rules checked by `.validated()`, `ai.stream` as a `TextStream` that is also an SSE response, `chunked`, `on_finish`), attachments, failover and retries, per-user token budgets, metrics, `AiError` statuses, agents (`Agent`, `#[agent]`, `#[tool]`, `#[tool(read_only)]`, `#[tool(needs_approval)]`, `#[can]` / `#[permissions]` / `#[roles]` on tools, `AgentRun`, `ai.agent::<A>()` with `acting_as`, `conversation`, `prompt`, `structured`, `stream`, `approve`, `reject`), conversation memory (`Conversations`, `ai:prune`), `Embeddings`, `VectorStore` (memory, pgvector, Qdrant) with `Filter`s, `Chunker`, `ai.rerank`, `navree make:agent`, and `AiFake` / `FakeReply` / `ai.fake = true` in tests. Use when adding AI features (summaries, drafts, extraction, classification, chat, an assistant that calls your services, answers from your own data), configuring providers or models, writing or testing agents and tools, indexing text into vectors, or debugging an AI request that fails with 503, 429, 502, 500, `AiError::Config`, `NoFakeReply`, an SSRF refusal, or a tool the model is never offered."
---

# AI SDK, agents and RAG

`navree::ai` (crate `navree-ai`, facade feature `ai`, **opt-in**: not part of
`full`) puts language models behind one service. Code names models by their
job (`fast`, `smart`, `embed`); configuration maps them to a provider and a
vendor model id. Everything below is in the code: when in doubt read
`crates/navree-ai/src` and its tests in `crates/navree-ai/tests/ai/`
(`docs_surface.rs` names every documented item) and `crates/navree/tests/ai.rs`
(an agent through `navree::` paths only).

## Steps: add AI to an app

1. Turn the feature on:

```toml
[dependencies]
navree = { git = "https://github.com/NavreeHQ/navree", features = ["ai"] }
```

   It brings `navree::ai` (the crate and the `#[agent]` macro) and puts `AiModule` and `ModuleBuilderAiExt` in the prelude.
   Add `"postgres"` for the pgvector store. Agents and conversation memory
   need a database driver feature of `navree` (`postgres`, `mysql` or
   `sqlite`).

2. Add the module. It is global and needs only `ConfigModule`.
   `DatabaseModule` is needed for agents (tool units of work), conversation
   memory and pgvector; `CacheModule` shares budgets and the embeddings cache
   between processes; agents also need `AuthModule` and `AccessModule`:

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(DatabaseModule::from_config())
    .module(CacheModule::from_config())
    .module(AuthModule::from_config())
    .module(AccessModule::for_root(AccessOptions { abilities, ..AccessOptions::default() }))
    .module(AiModule::from_config()) // section `ai`; or AiModule::for_root(AiOptions { .. })
```

   It provides `Arc<Ai>`, `Arc<Embeddings>`, `Arc<VectorStore>`,
   `Arc<Conversations>` and `Arc<AiOptions>`, contributes the migration set
   `ai` (`ai_conversations`, `ai_messages`) and the command
   `ai:prune [--days N]`. Run `navree migrate` after adding it.

3. Configure models in `config/app.toml`, keys in `.env`:

```toml
[ai]
default = "fast"            # else the first model by name

[ai.models.fast]
provider = "anthropic"
model = "claude-sonnet-5-5"
max_tokens = 1024           # default 4096
fallbacks = ["backup"]      # or "backup,other" from an env var

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
store = "memory"            # memory | pgvector | qdrant
dimensions = 1536
```

```env
APP_AI__PROVIDERS__ANTHROPIC__API_KEY=sk-ant-...
APP_AI__PROVIDERS__OPENAI__API_KEY=sk-proj-...
```

   A provider named after its driver needs no table. A second account or an
   OpenAI-compatible server gets its own table with `driver = "openai"` and
   `url`. Bedrock takes `region`, `access_key_id`, `secret_access_key`,
   `session_token`, `profile` (else `AWS_*` variables, the shared
   credentials file, then an ECS/EKS container role or the EC2 instance
   role over IMDSv2; IRSA web identity tokens aren't read). Ollama defaults
   to `http://127.0.0.1:11434`.

4. Inject `Arc<Ai>` and call it (see the templates). Return `AiError` with `?`
   from handlers: it becomes Navree's JSON error with its status.

## Templates

Text, with options:

```rust
let response = self
    .ai
    .text(format!("Summarize:\n{timeline}"))     // or vec![Message::user(..), Message::assistant(..)]
    .system("You write two-sentence updates.")
    .model("fast")
    .max_tokens(200)
    .user(access.user)                           // Uuid or Option<Uuid>: budget + log line
    .await?;                                     // TextResponse { text, model, provider, usage, finish }
```

Structured output (`T: Deserialize + ToSchema`; `.validated()` needs
`Validate`):

```rust
use navree::openapi::ToSchema;
use navree::validation::Validate;
use serde::Deserialize;

/// A follow-up email.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct FollowUpEmail {
    /// The subject line, without "Re:".
    #[validate(length(min = 1, max = 80))]
    pub subject: String,
    /// The plain-text body.
    #[validate(length(min = 1, max = 2000))]
    pub body: String,
}

let email = self.ai.structured::<FollowUpEmail>(prompt).model("fast").validated().await?;
```

Streaming to the browser (a `TextStream` is a response):

```rust
#[get("/{id}/summary/stream")]
#[can(Read, Deal, from = "id")]
async fn summary_stream(&self, deal: Authorized<Deal, Read>) -> Result<TextStream> {
    Ok(self.ai.stream(format!("Summarize {}", deal.into_inner().title)).model("fast").await?)
}
// events: `text` {"text"}, then `done` {"model","provider","usage","finish"}; `error` with the error body.
// .chunked(Duration::from_millis(100)) merges text; .on_finish(|response| async move { .. }) stores it.
```

An agent (generate one with `navree make:agent <module> <name>`):

```rust
use navree::ai::{Agent, AgentOptions, AgentRun, agent};
use navree::prelude::*;

#[injectable]
pub struct DealsAssistant {
    deals: Arc<DealService>,
}

#[agent]
impl Agent for DealsAssistant {
    const NAME: &'static str = "deals-assistant";

    fn instructions(&self, run: &AgentRun) -> String {
        format!("You help {}. Use the tools; never guess.", run.user().name)
    }

    fn options(&self) -> AgentOptions {
        AgentOptions { model: Some("smart".into()), max_steps: 8, ..AgentOptions::default() }
    }

    /// Finds the deals the user can see. Returns at most 20.   <- required: the description
    #[tool(read_only)]
    async fn search_deals(&self, run: &AgentRun, args: SearchDeals) -> Result<Vec<DealResource>> {
        let page = PageQuery { per_page: 20, ..PageQuery::default() };
        Ok(self.deals.list(run.uow(), run.accessible::<Deal, Read>(), args.into(), page).await?.data)
    }

    /// Moves a deal to another stage.
    #[tool(needs_approval)]
    async fn move_deal(&self, run: &AgentRun, args: MoveDealArgs) -> Result<DealResource> {
        let deal = run.authorize::<Deal, Move>(args.deal_id).await?; // 404 / 403 to the model
        self.deals.move_to(run.uow(), run.user_id(), deal.into_inner(), args.into()).await
    }

    /// Gives a deal to someone else.
    #[tool(needs_approval)]
    #[can(Manage, Deal)]                  // or #[permissions("deals.manage")], #[roles("manager")]
    async fn reassign_deal(&self, run: &AgentRun, args: ReassignDeal) -> Result<DealResource> { .. }
}

// in the module's wire / register:
m.agent::<DealsAssistant>();

// running it, as the signed-in user:
let reply = self
    .ai
    .agent::<DealsAssistant>()
    .acting_as(access)                    // Arc<PrincipalAccess> from CurrentAccess; or .acting_as_user(id)
    .conversation(input.conversation_id)  // Option<Uuid>; leave out for no memory
    .prompt(input.message)                // or .structured::<T>(..), .stream(..)
    .await?;                              // AgentReply { conversation_id, text, steps, pending_approval, usage }
// pending approval: .conversation(Some(id)).approve(call_id) or .reject(call_id, Some(reason))
```

Tool arguments: one struct deriving `Deserialize`, `ToSchema` and
`Validate`, field doc comments for the model; or no arguments at all.

RAG:

```rust
m.vector_collection(VectorCollection::new("activity_notes")); // created at boot (pgvector, Qdrant)

let chunks = Chunker::new().max_tokens(300).overlap(40).split(&text);
let vectors = embeddings.embed_many(chunks.iter().map(|c| c.text.as_str())).await?;
let notes = vectors_store.collection("activity_notes");
notes.delete_where(Filter::eq("activity_id", id)).await?;
notes.upsert_many(chunks.into_iter().zip(vectors).map(|(c, e)| {
    VectorRecord::new(format!("{id}:{}", c.index), e).text(c.text).metadata(json!({ "activity_id": id, "subject_id": deal_id }))
}).collect()).await?;

let hits = notes.search(question).filter(Filter::eq("subject_id", deal_id)).limit(25).await?;
let best = ai.rerank(question, hits).model("fast").top(6).await?;
```

Tests:

```rust
let app = TestApp::builder()
    .temp_database()            // SQLite: needs navree's `sqlite` feature in [dev-dependencies]
    .config("ai.fake", true)
    // ... the app's modules, AiModule::from_config()
    .build()
    .await;
let fake = app.get::<Ai>().await.fake().unwrap();
fake.respond_with([
    FakeReply::tool_call("search_deals", json!({ "query": "Acme" })),
    FakeReply::text("Acme is in negotiation."),
]);
// ... run the route or the agent ...
fake.assert_prompt_count(2);
fake.assert_prompted(|r| r.model() == "smart" && r.tools().contains(&"search_deals".to_owned()));
assert_eq!(fake.tool_results("search_deals").len(), 1);
```

## Checks

- `cargo test` with `ai.fake = true`: no network, no key. A request with no
  scripted reply fails with `AiError::NoFakeReply` naming the prompt.
- `navree migrate` after adding `AiModule` (conversation tables).
- A tool the user may not use must be missing from
  `fake.requests()[0].tools()` for that user.
- Write tools: assert the database after a refused or failed call (the
  tool's unit of work rolled back).

## Gotchas

- **Opt-in feature.** `features = ["ai"]`; without it `navree::ai` and
  `AiModule` don't exist.
- **SSRF guard.** Provider calls go through navree-http-client and refuse
  private addresses except configured hosts (`ai.providers.*.url`,
  `ai.vectors.url`, and `127.0.0.1` for Ollama at its default). An internal
  gateway must be configured as a provider `url`.
- **Missing keys don't fail the boot**: requests to that provider fail with
  `AiError::Config` (500) and the boot logs a warning. Invalid config (unknown
  provider, fallback, default, a fallback cycle) does fail the boot, naming
  the key.
- **Anthropic gets no `temperature`** (API version `2026-10-21` removed
  sampling parameters).
- **Statuses**: `Unavailable` 503, `RateLimited` and `BudgetExceeded` 429 with
  `Retry-After`, `InvalidOutput` and `Refused` 502, `NotFound` 404,
  `Unauthenticated` 401, the rest 500. The provider's own text never reaches
  the client.
- **Retries in the fake** happen without waiting, and each retry takes the
  next scripted reply: `FakeReply::error(AiError::unavailable(..))` followed
  by `FakeReply::text(..)` succeeds. Set `ai.max_retries = 0` to see the error.
- **A fake installed with `.override_value(Arc::new(fake.ai()))`** isn't tied
  to the app: fine for text, structured, stream, embeddings; agents need
  `ai.fake = true` (the app's own `Ai`).
- **Agents**: every `#[tool]` needs a doc comment and must be `async`, take
  `&self`, `&AgentRun` and at most one arguments value; `#[can]` on a tool
  takes no `from` (use `run.authorize::<S, A>(id)`). Access attributes on a
  method without `#[tool]` are a compile error. Results over 16 KB are cut
  off. Tool 5xx errors reach the model as "The tool failed".
- **Memory needs `.conversation(..)`**: without it a run stores nothing and
  approvals can't be answered later (`approve` is then a 404).
- **`acting_as_user(id)` needs `AccessModule`**; a run without a user is
  `AiError::Unauthenticated`.
- **Vectors**: every embedding must have `ai.vectors.dimensions` entries; a
  different size is `AiError::Config` naming both. Collection names are
  lower-case letters, digits and `_`. Qdrant uses its REST port (6333); on
  Qdrant `gt`/`gte`/`lt`/`lte` filters take numbers and RFC 3339 dates.
  pgvector needs PostgreSQL (the `postgres` feature) with the `vector`
  extension allowed; with `memory` the store needs no database.
- **Budgets** count only requests with `.user(..)` (agents always do);
  embeddings never count.
- **`StreamEvent` is `#[non_exhaustive]`**: end a `match` with `_ => ..`.
