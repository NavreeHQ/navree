//! Provider drivers: one [`Driver`] per vendor API, behind the failover
//! engine of [`Ai`](crate::Ai). Each turns a [`ChatRequest`] into the
//! vendor's HTTP request and its answer (or its stream of events) back.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::Stream;
use navree_http_client::HttpClient;
use navree_kernel::BoxFuture;
use serde_json::Value;
use uuid::Uuid;

use crate::error::{AiError, ProviderError};
use crate::message::{Message, Part, ToolCall};
use crate::options::ProviderConfig;
use crate::response::{Finish, Usage};

pub(crate) mod anthropic;
pub(crate) mod aws_credentials;
pub(crate) mod aws_role;
pub(crate) mod bedrock;
#[cfg(test)]
mod conformance;
pub(crate) mod eventstream;
pub(crate) mod gemini;
pub(crate) mod ollama;
pub(crate) mod openai;
pub(crate) mod wire;

/// A tool the model may call.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ToolDefinition {
    pub(crate) name: String,
    pub(crate) description: String,
    /// The JSON Schema of its arguments (an object).
    pub(crate) parameters: Value,
}

/// Whether and how the model may call tools.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ToolChoice {
    /// It decides.
    #[default]
    Auto,
    /// It must answer without calling one (the last step of an agent run).
    None,
}

/// Structured output: the schema the answer must match.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OutputFormat {
    /// A name for the schema (the type's name).
    pub(crate) name: String,
    /// The JSON Schema (an object, `$defs` included).
    pub(crate) schema: Value,
}

/// One request to one model, as every driver receives it.
#[derive(Clone, Debug)]
pub(crate) struct ChatRequest {
    /// The model's name in configuration (`fast`).
    pub(crate) model_name: String,
    /// The vendor's model id (`claude-sonnet-5-5`).
    pub(crate) model: String,
    pub(crate) system: Option<String>,
    pub(crate) messages: Vec<Message>,
    pub(crate) max_tokens: u32,
    pub(crate) temperature: Option<f32>,
    pub(crate) stop: Vec<String>,
    pub(crate) tools: Vec<ToolDefinition>,
    pub(crate) tool_choice: ToolChoice,
    /// Native structured output (only for drivers that support it).
    pub(crate) output: Option<OutputFormat>,
    pub(crate) user: Option<Uuid>,
    /// The attempt limit (for streams, the wait for the first bytes).
    pub(crate) timeout: Duration,
}

/// A whole answer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ChatResponse {
    /// Text, tool calls and reasoning parts, in order.
    pub(crate) parts: Vec<Part>,
    pub(crate) usage: Usage,
    pub(crate) finish: Finish,
}

impl ChatResponse {
    /// The text parts, joined.
    pub(crate) fn text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|part| match part {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The tool calls.
    #[cfg(test)]
    pub(crate) fn tool_calls(&self) -> Vec<ToolCall> {
        self.parts
            .iter()
            .filter_map(|part| match part {
                Part::ToolCall(call) => Some(call.clone()),
                _ => None,
            })
            .collect()
    }
}

/// A piece of a streamed answer.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StreamChunk {
    /// More text.
    Text(String),
    /// A complete tool call (drivers assemble the argument fragments).
    ToolCall(ToolCall),
    /// A provider-specific part to keep with the message.
    Reasoning(Part),
    /// The end: the tokens and why it stopped.
    Done { usage: Usage, finish: Finish },
}

/// A driver's stream of chunks.
pub(crate) type ChunkStream =
    Pin<Box<dyn Stream<Item = Result<StreamChunk, ProviderError>> + Send>>;

/// Vectors for texts.
#[derive(Clone, Debug)]
pub(crate) struct EmbedRequest {
    pub(crate) model: String,
    pub(crate) texts: Vec<String>,
    pub(crate) dimensions: Option<usize>,
    pub(crate) timeout: Duration,
}

/// The vectors, in input order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EmbedResponse {
    pub(crate) vectors: Vec<Vec<f32>>,
    pub(crate) input_tokens: u64,
}

/// What a driver can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Capabilities {
    pub(crate) images: bool,
    pub(crate) pdfs: bool,
    pub(crate) embeddings: bool,
    /// Structured output constrained by the provider (else the schema goes
    /// into the system prompt).
    pub(crate) native_structured: bool,
}

/// One vendor API.
pub(crate) trait Driver: Send + Sync {
    /// What it supports.
    fn capabilities(&self) -> Capabilities;

    /// One answer.
    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> BoxFuture<'a, Result<ChatResponse, ProviderError>>;

    /// A streamed answer; returns once the provider starts answering.
    fn stream<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> BoxFuture<'a, Result<ChunkStream, ProviderError>>;

    /// Vectors for texts.
    fn embed<'a>(
        &'a self,
        request: &'a EmbedRequest,
    ) -> BoxFuture<'a, Result<EmbedResponse, ProviderError>> {
        let _ = request;
        Box::pin(async {
            Err(ProviderError::unsupported(
                "this provider has no embeddings",
            ))
        })
    }
}

/// The driver for provider `name` configured with `config` (an empty
/// configuration for a built-in provider without a table).
pub(crate) fn build_driver(
    name: &str,
    config: &ProviderConfig,
    http: &HttpClient,
) -> Result<Arc<dyn Driver>, AiError> {
    let driver = config.driver_for(name);
    let http = http.clone();
    Ok(match driver {
        "anthropic" => Arc::new(anthropic::Anthropic::new(name, config, http)),
        "openai" => Arc::new(openai::OpenAi::new(
            name,
            config,
            http,
            openai::Flavor::OpenAi,
        )),
        "mistral" => Arc::new(openai::OpenAi::new(
            name,
            config,
            http,
            openai::Flavor::Mistral,
        )),
        "gemini" => Arc::new(gemini::Gemini::new(name, config, http)),
        "ollama" => Arc::new(ollama::Ollama::new(name, config, http)),
        "bedrock" => Arc::new(bedrock::Bedrock::new(name, config, http)),
        other => {
            return Err(AiError::config(format!(
                "provider `{name}`: unknown driver `{other}`"
            )));
        }
    })
}

/// Whether a provider of this driver needs an API key, and has none.
pub(crate) fn missing_key(name: &str, config: &ProviderConfig) -> bool {
    let driver = config.driver_for(name);
    let custom_openai = driver == "openai" && config.url.is_some();
    matches!(driver, "anthropic" | "openai" | "gemini" | "mistral")
        && !custom_openai
        && config
            .api_key
            .as_ref()
            .is_none_or(|key| key.expose().is_empty())
}
