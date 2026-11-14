//! [`AiDriver`]: the driver of an AI provider a module adds, what it is
//! asked ([`AiDriverRequest`], [`AiEmbedRequest`]) and what it answers
//! ([`AiDriverAnswer`], [`AiDriverChunk`], [`AiEmbedAnswer`]); and the
//! adapter that runs it behind the failover engine of [`Ai`](crate::Ai),
//! exactly where a built-in driver runs.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{Stream, StreamExt};
use navree_kernel::BoxFuture;
use serde_json::Value;
use uuid::Uuid;

use crate::error::{AiError, ProviderError};
use crate::message::{Message, Part, ToolCall};
use crate::provider::{
    Capabilities, ChatRequest, ChatResponse, ChunkStream, Driver, EmbedRequest, EmbedResponse,
    StreamChunk, ToolChoice,
};
use crate::response::{Finish, Usage};

/// The driver of an AI provider: one vendor API, or anything else that
/// answers prompts. A module registers one under a name with
/// `m.ai_provider`, and a provider table (`[ai.providers.vaani]`) selects it
/// by that name; `AiModule` then runs it behind the same model chains,
/// retries, failover, budgets, usage metrics and telemetry as the built-in
/// drivers.
///
/// Each method is one whole attempt: the failover engine retries, falls
/// over and gives up, so a driver never retries by itself. The class of an
/// [`AiError`] it returns decides what happens next:
///
/// - `RateLimited` (with the provider's `retry_after`, when it sent one) and
///   `Unavailable` (a 5xx, a timeout, a connection failure) are retried up
///   to `ai.max_retries` times, then the model's fallbacks are tried;
/// - `Unsupported` skips the model and tries the next one in its chain;
/// - `Request` (the provider rejected the request) fails the request at
///   once, and so does `Config` (a missing or rejected key), reported for
///   the provider table `vaani` as
///   ``AI configuration error: provider `vaani`: {message}``;
/// - any other error is returned as it is.
///
/// A stream whose first chunk is an error fails over as `chat` does; after
/// the first chunk the answer has started and an error ends it.
///
/// A driver brings its own HTTP client: the client `AiModule` gives the
/// built-in drivers, with its guard against requests to internal
/// addresses, does not apply to it. Never put a key, a token or a prompt in
/// an error's message: errors reach the log.
///
/// ```no_run
/// use navree_ai::{
///     AiCapabilities, AiDriver, AiDriverAnswer, AiDriverChunk, AiDriverRequest, AiDriverStream,
///     AiError, Finish, Part, Usage,
/// };
/// use navree_kernel::BoxFuture;
///
/// /// A provider that greets.
/// struct Namaste;
///
/// impl AiDriver for Namaste {
///     fn capabilities(&self) -> AiCapabilities {
///         AiCapabilities::default()
///     }
///
///     fn chat<'a>(
///         &'a self,
///         _request: &'a AiDriverRequest,
///     ) -> BoxFuture<'a, Result<AiDriverAnswer, AiError>> {
///         Box::pin(async {
///             let text = Part::Text { text: "Namaste".to_owned() };
///             Ok(AiDriverAnswer::new(vec![text], Usage::new(3, 1), Finish::Stop))
///         })
///     }
///
///     fn stream<'a>(
///         &'a self,
///         _request: &'a AiDriverRequest,
///     ) -> BoxFuture<'a, Result<AiDriverStream, AiError>> {
///         Box::pin(async {
///             let chunks: Vec<Result<AiDriverChunk, AiError>> = vec![
///                 Ok(AiDriverChunk::Text("Namaste".to_owned())),
///                 Ok(AiDriverChunk::Done { usage: Usage::new(3, 1), finish: Finish::Stop }),
///             ];
///             Ok(Box::pin(futures_util::stream::iter(chunks)) as AiDriverStream)
///         })
///     }
/// }
/// ```
pub trait AiDriver: Send + Sync + 'static {
    /// What the provider can read and do.
    fn capabilities(&self) -> AiCapabilities;

    /// One whole answer to `request`.
    fn chat<'a>(
        &'a self,
        request: &'a AiDriverRequest,
    ) -> BoxFuture<'a, Result<AiDriverAnswer, AiError>>;

    /// A streamed answer to `request`; returns once the provider starts
    /// answering. The stream ends with one [`AiDriverChunk::Done`].
    fn stream<'a>(
        &'a self,
        request: &'a AiDriverRequest,
    ) -> BoxFuture<'a, Result<AiDriverStream, AiError>>;

    /// Vectors for texts. The default answers that the provider has no
    /// embeddings (`AiError::Unsupported`), so the model is skipped.
    fn embed<'a>(
        &'a self,
        request: &'a AiEmbedRequest,
    ) -> BoxFuture<'a, Result<AiEmbedAnswer, AiError>> {
        let _ = request;
        Box::pin(async {
            Err(AiError::Unsupported {
                message: "this provider has no embeddings".to_owned(),
            })
        })
    }
}

/// What an [`AiDriver`] can do. Every flag is off by default.
///
/// ```
/// use navree_ai::AiCapabilities;
///
/// let capabilities = AiCapabilities::default().images(true).embeddings(true);
/// assert!(capabilities.images && capabilities.embeddings);
/// assert!(!capabilities.pdfs && !capabilities.native_structured);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct AiCapabilities {
    /// It reads image attachments.
    pub images: bool,
    /// It reads PDF attachments.
    pub pdfs: bool,
    /// It computes embeddings.
    pub embeddings: bool,
    /// It constrains structured output to a JSON Schema itself
    /// ([`AiDriverRequest::output_schema`]); without it, the schema goes
    /// into the system prompt.
    pub native_structured: bool,
}

impl AiCapabilities {
    /// With image attachments read, or not.
    #[must_use]
    pub fn images(mut self, on: bool) -> Self {
        self.images = on;
        self
    }

    /// With PDF attachments read, or not.
    #[must_use]
    pub fn pdfs(mut self, on: bool) -> Self {
        self.pdfs = on;
        self
    }

    /// With embeddings, or not.
    #[must_use]
    pub fn embeddings(mut self, on: bool) -> Self {
        self.embeddings = on;
        self
    }

    /// With structured output constrained by the provider, or not.
    #[must_use]
    pub fn native_structured(mut self, on: bool) -> Self {
        self.native_structured = on;
        self
    }
}

/// What an [`AiDriver`] is asked: one request to one model, after the
/// model was chosen from its chain and the system prompt completed.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct AiDriverRequest {
    chat: ChatRequest,
    tools: Vec<AiTool>,
}

impl AiDriverRequest {
    /// The request a driver gets for `chat`.
    pub(crate) fn from_chat(chat: &ChatRequest) -> AiDriverRequest {
        let tools = chat
            .tools
            .iter()
            .map(|tool| AiTool {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            })
            .collect();
        AiDriverRequest {
            chat: chat.clone(),
            tools,
        }
    }

    /// The provider's model id (`ai.models.{name}.model`, sent as it is),
    /// not the name the app's code uses.
    pub fn model(&self) -> &str {
        &self.chat.model
    }

    /// The system prompt, if any.
    pub fn system(&self) -> Option<&str> {
        self.chat.system.as_deref()
    }

    /// The conversation, attachments included, in order.
    pub fn messages(&self) -> &[Message] {
        &self.chat.messages
    }

    /// The output limit, in tokens.
    pub fn max_tokens(&self) -> u32 {
        self.chat.max_tokens
    }

    /// The sampling temperature, when one is set.
    pub fn temperature(&self) -> Option<f32> {
        self.chat.temperature
    }

    /// Stop sequences.
    pub fn stop(&self) -> &[String] {
        &self.chat.stop
    }

    /// The tools the model may call.
    pub fn tools(&self) -> &[AiTool] {
        &self.tools
    }

    /// Whether the model may call a tool now; `false` asks for an answer
    /// without one (the last step of an agent run).
    pub fn tools_allowed(&self) -> bool {
        self.chat.tool_choice == ToolChoice::Auto
    }

    /// The name and the JSON Schema the answer must match, for a driver
    /// with `native_structured`.
    pub fn output_schema(&self) -> Option<(&str, &Value)> {
        self.chat
            .output
            .as_ref()
            .map(|output| (output.name.as_str(), &output.schema))
    }

    /// The user the request is made for, when the app said.
    pub fn user(&self) -> Option<Uuid> {
        self.chat.user
    }

    /// The limit of this attempt (for a stream, the wait for its first
    /// bytes).
    pub fn timeout(&self) -> Duration {
        self.chat.timeout
    }
}

/// A tool the model may call.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct AiTool {
    /// The tool's name.
    pub name: String,
    /// What it does, for the model.
    pub description: String,
    /// The JSON Schema of its arguments (an object).
    pub parameters: Value,
}

/// A whole answer of an [`AiDriver`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct AiDriverAnswer {
    /// Text and tool calls, in order.
    pub parts: Vec<Part>,
    /// The tokens the request used.
    pub usage: Usage,
    /// Why the model stopped.
    pub finish: Finish,
}

impl AiDriverAnswer {
    /// An answer of `parts`, with its `usage` and `finish`.
    pub fn new(parts: Vec<Part>, usage: Usage, finish: Finish) -> AiDriverAnswer {
        AiDriverAnswer {
            parts,
            usage,
            finish,
        }
    }
}

/// A piece of a streamed answer of an [`AiDriver`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum AiDriverChunk {
    /// More text.
    Text(String),
    /// A complete tool call.
    ToolCall(ToolCall),
    /// The end of the answer.
    Done {
        /// The tokens the request used.
        usage: Usage,
        /// Why the model stopped.
        finish: Finish,
    },
}

/// The stream of an [`AiDriver`]'s answer.
pub type AiDriverStream = Pin<Box<dyn Stream<Item = Result<AiDriverChunk, AiError>> + Send>>;

/// What an [`AiDriver`] is asked for embeddings.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct AiEmbedRequest {
    embed: EmbedRequest,
}

impl AiEmbedRequest {
    /// The provider's model id.
    pub fn model(&self) -> &str {
        &self.embed.model
    }

    /// The texts, in order.
    pub fn texts(&self) -> &[String] {
        &self.embed.texts
    }

    /// The vector size asked for, when the model is configured with one.
    pub fn dimensions(&self) -> Option<usize> {
        self.embed.dimensions
    }

    /// The limit of this attempt.
    pub fn timeout(&self) -> Duration {
        self.embed.timeout
    }
}

/// The embeddings an [`AiDriver`] computed.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct AiEmbedAnswer {
    /// One vector per text, in the texts' order.
    pub vectors: Vec<Vec<f32>>,
    /// The input tokens the request used.
    pub input_tokens: u64,
}

impl AiEmbedAnswer {
    /// The answer of `vectors`, which used `input_tokens`.
    pub fn new(vectors: Vec<Vec<f32>>, input_tokens: u64) -> AiEmbedAnswer {
        AiEmbedAnswer {
            vectors,
            input_tokens,
        }
    }
}

/// A registered [`AiDriver`] as one of the engine's drivers.
pub(crate) struct PublicDriver(pub(crate) Arc<dyn AiDriver>);

impl Driver for PublicDriver {
    fn capabilities(&self) -> Capabilities {
        let capabilities = self.0.capabilities();
        Capabilities {
            images: capabilities.images,
            pdfs: capabilities.pdfs,
            embeddings: capabilities.embeddings,
            native_structured: capabilities.native_structured,
        }
    }

    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> BoxFuture<'a, Result<ChatResponse, ProviderError>> {
        Box::pin(async move {
            let request = AiDriverRequest::from_chat(request);
            let answer = self.0.chat(&request).await.map_err(provider_error)?;
            Ok(ChatResponse {
                parts: answer.parts,
                usage: answer.usage,
                finish: answer.finish,
            })
        })
    }

    fn stream<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> BoxFuture<'a, Result<ChunkStream, ProviderError>> {
        Box::pin(async move {
            let request = AiDriverRequest::from_chat(request);
            let chunks = self.0.stream(&request).await.map_err(provider_error)?;
            let chunks = chunks.map(|chunk| chunk.map(stream_chunk).map_err(provider_error));
            Ok(Box::pin(chunks) as ChunkStream)
        })
    }

    fn embed<'a>(
        &'a self,
        request: &'a EmbedRequest,
    ) -> BoxFuture<'a, Result<EmbedResponse, ProviderError>> {
        Box::pin(async move {
            let request = AiEmbedRequest {
                embed: request.clone(),
            };
            let answer = self.0.embed(&request).await.map_err(provider_error)?;
            Ok(EmbedResponse {
                vectors: answer.vectors,
                input_tokens: answer.input_tokens,
            })
        })
    }
}

/// A chunk of a registered driver's stream as the engine's chunk.
fn stream_chunk(chunk: AiDriverChunk) -> StreamChunk {
    match chunk {
        AiDriverChunk::Text(text) => StreamChunk::Text(text),
        AiDriverChunk::ToolCall(call) => StreamChunk::ToolCall(call),
        AiDriverChunk::Done { usage, finish } => StreamChunk::Done { usage, finish },
    }
}

/// The failover class of a registered driver's error: rate limited and
/// unavailable are retried then fall over, unsupported skips the model, a
/// rejected request and a configuration error fail at once, and every
/// other error is returned as it is.
pub(crate) fn provider_error(error: AiError) -> ProviderError {
    match error {
        AiError::RateLimited { retry_after, .. } => ProviderError::RateLimited { retry_after },
        AiError::Unavailable { message, .. } => ProviderError::Unavailable { message },
        AiError::Unsupported { message } => ProviderError::Unsupported { message },
        AiError::Request { message, .. } => ProviderError::Request { message },
        AiError::Config { message } => ProviderError::Auth { message },
        other => ProviderError::Scripted(other),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::provider::{OutputFormat, ToolDefinition};

    /// A driver of the test: it answers with the model id it was asked
    /// for, streams text, a tool call and the end, and fails as told.
    struct Vaani {
        fail: Option<AiError>,
    }

    impl AiDriver for Vaani {
        fn capabilities(&self) -> AiCapabilities {
            AiCapabilities::default()
                .images(true)
                .pdfs(false)
                .embeddings(false)
                .native_structured(true)
        }

        fn chat<'a>(
            &'a self,
            request: &'a AiDriverRequest,
        ) -> BoxFuture<'a, Result<AiDriverAnswer, AiError>> {
            Box::pin(async move {
                if let Some(error) = &self.fail {
                    return Err(error.clone());
                }
                let text = format!("Namaste from {}", request.model());
                Ok(AiDriverAnswer::new(
                    vec![Part::Text { text }],
                    Usage::new(3, 1),
                    Finish::Stop,
                ))
            })
        }

        fn stream<'a>(
            &'a self,
            _request: &'a AiDriverRequest,
        ) -> BoxFuture<'a, Result<AiDriverStream, AiError>> {
            Box::pin(async move {
                let chunks: Vec<Result<AiDriverChunk, AiError>> = match &self.fail {
                    Some(error) => vec![Err(error.clone())],
                    None => vec![
                        Ok(AiDriverChunk::Text("Namaste".to_owned())),
                        Ok(AiDriverChunk::ToolCall(lookup())),
                        Ok(AiDriverChunk::Done {
                            usage: Usage::new(3, 1),
                            finish: Finish::ToolCalls,
                        }),
                    ],
                };
                Ok(Box::pin(futures_util::stream::iter(chunks)) as AiDriverStream)
            })
        }
    }

    fn lookup() -> ToolCall {
        ToolCall {
            id: "call_1".to_owned(),
            name: "lookup_city".to_owned(),
            arguments: json!({"city": "Ludhiana"}),
            meta: None,
        }
    }

    fn chat_request() -> ChatRequest {
        ChatRequest {
            model_name: "smart".to_owned(),
            model: "vaani-large-2".to_owned(),
            system: Some("Answer in Punjabi.".to_owned()),
            messages: vec![Message::user("Namaste")],
            max_tokens: 256,
            temperature: Some(0.2),
            stop: vec!["END".to_owned()],
            tools: vec![ToolDefinition {
                name: "lookup_city".to_owned(),
                description: "Finds a city".to_owned(),
                parameters: json!({"type": "object"}),
            }],
            tool_choice: ToolChoice::None,
            output: Some(OutputFormat {
                name: "Deal".to_owned(),
                schema: json!({"type": "object"}),
            }),
            user: None,
            timeout: Duration::from_secs(30),
        }
    }

    #[tokio::test]
    async fn a_public_driver_keeps_answers_chunks_and_error_classes() {
        let driver = PublicDriver(Arc::new(Vaani { fail: None }));
        assert_eq!(
            driver.capabilities(),
            Capabilities {
                images: true,
                pdfs: false,
                embeddings: false,
                native_structured: true,
            }
        );

        let request = chat_request();
        assert_eq!(
            driver.chat(&request).await.unwrap(),
            ChatResponse {
                parts: vec![Part::Text {
                    text: "Namaste from vaani-large-2".to_owned()
                }],
                usage: Usage::new(3, 1),
                finish: Finish::Stop,
            }
        );
        let chunks: Vec<Result<StreamChunk, ProviderError>> =
            driver.stream(&request).await.unwrap().collect().await;
        assert_eq!(
            chunks,
            vec![
                Ok(StreamChunk::Text("Namaste".to_owned())),
                Ok(StreamChunk::ToolCall(lookup())),
                Ok(StreamChunk::Done {
                    usage: Usage::new(3, 1),
                    finish: Finish::ToolCalls,
                }),
            ]
        );
        let embed = EmbedRequest {
            model: "vaani-embed".to_owned(),
            texts: vec!["Ludhiana".to_owned()],
            dimensions: None,
            timeout: Duration::from_secs(5),
        };
        assert_eq!(
            driver.embed(&embed).await.unwrap_err(),
            ProviderError::unsupported("this provider has no embeddings")
        );

        // The request: the provider's model id, never the app's name for it.
        let public = AiDriverRequest::from_chat(&request);
        assert_eq!(public.model(), "vaani-large-2");
        assert_eq!(public.system(), Some("Answer in Punjabi."));
        assert_eq!(public.messages().to_vec(), vec![Message::user("Namaste")]);
        assert_eq!(public.max_tokens(), 256);
        assert_eq!(public.temperature(), Some(0.2));
        assert_eq!(public.stop().to_vec(), vec!["END".to_owned()]);
        assert_eq!(
            public.tools().to_vec(),
            vec![AiTool {
                name: "lookup_city".to_owned(),
                description: "Finds a city".to_owned(),
                parameters: json!({"type": "object"}),
            }]
        );
        assert!(!public.tools_allowed());
        let schema = json!({"type": "object"});
        assert_eq!(public.output_schema(), Some(("Deal", &schema)));
        assert_eq!(public.user(), None);
        assert_eq!(public.timeout(), Duration::from_secs(30));
        let auto = ChatRequest {
            tool_choice: ToolChoice::Auto,
            output: None,
            ..chat_request()
        };
        let public = AiDriverRequest::from_chat(&auto);
        assert!(public.tools_allowed());
        assert_eq!(public.output_schema(), None);

        // The error classes of the failover engine.
        let two = Some(Duration::from_secs(2));
        assert_eq!(
            provider_error(AiError::rate_limited("vaani", two)),
            ProviderError::RateLimited { retry_after: two }
        );
        assert_eq!(
            provider_error(AiError::unavailable("vaani")),
            ProviderError::unavailable("the provider is unavailable")
        );
        assert_eq!(
            provider_error(AiError::Unsupported {
                message: "no Punjabi voice".to_owned()
            }),
            ProviderError::unsupported("no Punjabi voice")
        );
        assert_eq!(
            provider_error(AiError::request("vaani", "too long")),
            ProviderError::request("too long")
        );
        assert_eq!(
            provider_error(AiError::config("no key")),
            ProviderError::Auth {
                message: "no key".to_owned()
            }
        );
        let other = provider_error(AiError::not_found("model"));
        assert_eq!(other, ProviderError::Scripted(AiError::not_found("model")));
        assert!(!other.is_transient());

        // A failing driver's errors reach the engine with their class, in
        // a chat and as a stream's first chunk.
        let failing = PublicDriver(Arc::new(Vaani {
            fail: Some(AiError::unavailable("vaani")),
        }));
        let error = failing.chat(&request).await.unwrap_err();
        assert!(error.is_transient(), "{error:?}");
        let mut chunks = failing.stream(&request).await.unwrap();
        let first = chunks.next().await.unwrap().unwrap_err();
        assert!(first.is_transient(), "{first:?}");
        let failing = PublicDriver(Arc::new(Vaani {
            fail: Some(AiError::request("vaani", "too long")),
        }));
        assert_eq!(
            failing.chat(&request).await.unwrap_err(),
            ProviderError::request("too long")
        );
    }
}
