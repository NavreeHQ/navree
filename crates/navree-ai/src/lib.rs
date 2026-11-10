//! # navree-ai
//!
//! `AiModule`: language models behind one service. Name your models in
//! configuration (`smart`, `fast`, `embed`), inject `Arc<Ai>` and ask:
//!
//! Items hidden from this documentation are not public API: Navree's own
//! macros and crates use them, and they can change in any release.
//!
//! - [`Ai::text`] for text, [`Ai::structured`] for a typed value (JSON
//!   Schema from `ToSchema`, the provider's structured-output mode,
//!   a repair loop, and validation rules with `.validated()`), [`Ai::stream`] for
//!   a [`TextStream`] that is also a server-sent events response.
//! - **Providers are drivers**: Anthropic (Messages API), OpenAI (chat
//!   completions, and any compatible server), Google Gemini, Mistral,
//!   Ollama and AWS Bedrock (Converse, signed with navree-crypto's SigV4
//!   signer). Their HTTP goes through navree-http-client and its SSRF guard.
//! - **Failover**: each model is retried on a 429, a 5xx, a timeout or a
//!   connection failure (exponential backoff, `retry-after` honored), then
//!   its `fallbacks` are tried in order.
//! - **Limits**: `max_tokens` per request, `ai.budget.daily_tokens_per_user`
//!   per user and UTC day (counted in the app's cache), `max_steps` per
//!   agent run; usage metrics and one log line per request.
//! - **Agents**: the [`Agent`] trait with `#[tool]` methods (see
//!   [`agent`](macro@agent)), run as a user with their access, each tool
//!   call in its own unit of work, with conversation memory, approvals and
//!   streaming.
//! - **Retrieval**: [`Embeddings`] (batched, cached), [`VectorStore`]
//!   (memory, pgvector, Qdrant) with metadata [`Filter`]s, [`Chunker`] and
//!   [`Ai::rerank`].
//! - **Tests**: [`AiFake`] answers from scripted [`FakeReply`]s and records
//!   every request; `ai.fake = true` installs it.
//!
//! ```no_run
//! use navree_ai::AiModule;
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_kernel::Navree;
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(AiModule::from_config()) // section `ai`
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod agent;
mod ai;
mod chunker;
mod commands;
mod embeddings;
mod error;
mod fake;
mod memory;
mod message;
mod module;
mod options;
mod provider;
mod request;
mod rerank;
mod response;
mod schema;
mod stream;
#[cfg(feature = "ui")]
pub mod ui;
mod vectors;

pub use agent::{
    Agent, AgentOptions, AgentReply, AgentRun, AgentRunBuilder, AgentStep, AgentStream, Memory,
    PageAnswer, PageTool, PageTools, PendingApproval, ToolAccess, ToolSpec,
};
pub use ai::Ai;
pub use chunker::{Chunk, Chunker};
pub use embeddings::{Embedding, Embeddings};
pub use error::AiError;
pub use fake::{AiFake, FakeReply, FakeRequest};
pub use memory::{AI_MIGRATIONS, ConversationSummary, Conversations, StoredMessage};
pub use message::{Attachment, AttachmentKind, Message, Part, Prompt, Role, ToolCall, ToolResult};
pub use module::{AiModule, ModuleBuilderAiExt};
pub use options::{
    AiOptions, BudgetOptions, EmbeddingsOptions, MemoryOptions, Metric, ModelConfig,
    ProviderConfig, VectorStoreKind, VectorsOptions,
};
pub use request::{StreamRequest, StructuredRequest, TextRequest};
pub use rerank::RerankRequest;
pub use response::{Completion, Finish, TextResponse, Usage};
pub use schema::json_schema;
pub use stream::{StreamEvent, TextStream};
pub use vectors::{
    Collection, Filter, SearchQuery, SearchRequest, VectorCollection, VectorMatch, VectorRecord,
    VectorStore,
};

/// Marks `impl Agent for X` and collects its `#[tool]` methods: their
/// JSON Schemas (from the argument type's `ToSchema`), descriptions (doc
/// comments), flags (`read_only`, `needs_approval`) and access attributes
/// (`#[can(Action, Subject)]`, `#[permissions(..)]`, `#[roles(..)]`), and
/// the dispatch that deserializes, validates and calls them.
pub use navree_macros::agent;

/// What `#[agent]` expands to. Not a public API.
#[doc(hidden)]
pub mod __private {
    pub use crate::agent::{
        ToolOutcome, call_tool, call_tool_no_args, no_args_schema, tool_schema,
    };
    pub use navree_kernel::BoxFuture;
    pub use serde_json::Value;
}
