//! [`AiFake`]: answers requests without a network or a key, and records
//! them for assertions.

use std::collections::{BTreeSet, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use navree_kernel::BoxFuture;
use serde_json::Value;
use uuid::Uuid;

use crate::error::{AiError, ProviderError};
use crate::message::{Message, Part, Role, ToolCall};
use crate::provider::{
    Capabilities, ChatRequest, ChatResponse, ChunkStream, Driver, EmbedRequest, EmbedResponse,
    StreamChunk, ToolChoice,
};
use crate::response::{Finish, Usage};
use crate::{Ai, AiOptions};

/// One scripted answer of [`AiFake`].
#[derive(Clone, Debug)]
pub struct FakeReply {
    kind: ReplyKind,
    usage: Option<Usage>,
}

#[derive(Clone, Debug)]
enum ReplyKind {
    Text(String),
    Json(Value),
    ToolCall { name: String, arguments: Value },
    Refusal,
    Error(AiError),
}

impl FakeReply {
    /// Text. Streams split it into words.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            kind: ReplyKind::Text(text.into()),
            usage: None,
        }
    }

    /// A structured value, checked against `T` and its rules like a real
    /// answer (a value that doesn't match triggers the repair loop).
    pub fn json(value: Value) -> Self {
        Self {
            kind: ReplyKind::Json(value),
            usage: None,
        }
    }

    /// A tool call, for agents: the "model" calls tool `name` with `args`.
    pub fn tool_call(name: impl Into<String>, args: Value) -> Self {
        Self {
            kind: ReplyKind::ToolCall {
                name: name.into(),
                arguments: args,
            },
            usage: None,
        }
    }

    /// A refusal: the model declines (`Finish::Refused`).
    pub fn refusal() -> Self {
        Self {
            kind: ReplyKind::Refusal,
            usage: None,
        }
    }

    /// A failure, which goes through retries and failover like a real one.
    pub fn error(error: AiError) -> Self {
        Self {
            kind: ReplyKind::Error(error),
            usage: None,
        }
    }

    /// Reports these token counts (by default about four characters per
    /// token of the prompt and the answer).
    pub fn with_usage(mut self, input: u64, output: u64) -> Self {
        self.usage = Some(Usage::new(input, output));
        self
    }
}

/// A request [`AiFake`] received.
#[derive(Clone, Debug)]
pub struct FakeRequest {
    model: String,
    system: Option<String>,
    messages: Vec<Message>,
    tools: Vec<String>,
    max_tokens: u32,
    user: Option<Uuid>,
    structured: bool,
}

impl FakeRequest {
    /// The model name from `ai.models` (`fast`), not the vendor's id.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The system prompt.
    pub fn system(&self) -> Option<&str> {
        self.system.as_deref()
    }

    /// The messages sent (history, tool calls and results included).
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// The text of the last user message that has text.
    pub fn last_user_message(&self) -> String {
        self.messages
            .iter()
            .rev()
            .filter(|m| m.role == Role::User)
            .map(Message::text)
            .find(|text| !text.is_empty())
            .unwrap_or_default()
    }

    /// The names of the tools the model may call (none on the last step of
    /// an agent run).
    pub fn tools(&self) -> &[String] {
        &self.tools
    }

    /// The output limit.
    pub fn max_tokens(&self) -> u32 {
        self.max_tokens
    }

    /// The user the request is for.
    pub fn user(&self) -> Option<Uuid> {
        self.user
    }

    /// Whether the request asked for structured output.
    pub fn is_structured(&self) -> bool {
        self.structured
    }

    fn from_chat(request: &ChatRequest) -> Self {
        Self {
            model: request.model_name.clone(),
            system: request.system.clone(),
            messages: request.messages.clone(),
            // The last step of an agent run lists its tools but may call none.
            tools: match request.tool_choice {
                ToolChoice::Auto => request.tools.iter().map(|t| t.name.clone()).collect(),
                ToolChoice::None => Vec::new(),
            },
            max_tokens: request.max_tokens,
            user: request.user,
            structured: request.output.is_some()
                || request
                    .system
                    .as_deref()
                    .is_some_and(|s| s.contains(crate::ai::SCHEMA_INSTRUCTION)),
        }
    }
}

type Predicate = Arc<dyn Fn(&FakeRequest) -> bool + Send + Sync>;

/// The shared state of a fake and its clones.
#[derive(Default)]
struct State {
    queue: Mutex<VecDeque<FakeReply>>,
    rules: Mutex<Vec<(Predicate, FakeReply)>>,
    requests: Mutex<Vec<FakeRequest>>,
    embedded: Mutex<Vec<String>>,
    calls: AtomicU64,
}

/// Answers requests from scripted replies, without a network or a key, and
/// records every request. Turn it on with `ai.fake = true` and take it
/// from the app (`app.get::<Ai>().await.fake().unwrap()`), or build one and
/// install [`AiFake::ai`].
///
/// ```
/// use navree_ai::{AiFake, FakeReply};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navree_ai::AiError> {
/// let fake = AiFake::new();
/// let ai = fake.ai();
/// fake.respond(FakeReply::text("Agra is in negotiation."));
///
/// let summary = ai.text("Summarize the Agra deal").model("fast").await?;
///
/// assert_eq!(summary.text, "Agra is in negotiation.");
/// fake.assert_prompted(|r| r.model() == "fast" && r.last_user_message().contains("Agra"));
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Default)]
pub struct AiFake {
    state: Arc<State>,
}

impl AiFake {
    /// A fake with no replies.
    pub fn new() -> Self {
        Self::default()
    }

    /// An [`Ai`] that answers every model from this fake (any model name
    /// works). Install it in a test app with
    /// `.override_value(Arc::new(fake.ai()))`.
    pub fn ai(&self) -> Ai {
        self.ai_with(AiOptions {
            fake: true,
            ..AiOptions::default()
        })
    }

    /// An [`Ai`] with `options` that answers from this fake.
    pub fn ai_with(&self, mut options: AiOptions) -> Ai {
        options.fake = true;
        Ai::with_fake(options, self.clone(), None)
    }

    /// Queues `reply`; replies answer requests in order.
    pub fn respond(&self, reply: FakeReply) -> &Self {
        lock(&self.state.queue).push_back(reply);
        self
    }

    /// Queues several replies.
    pub fn respond_with(&self, replies: impl IntoIterator<Item = FakeReply>) -> &Self {
        lock(&self.state.queue).extend(replies);
        self
    }

    /// Answers every request that matches `predicate` with `reply`, ahead
    /// of the queue.
    pub fn respond_when(
        &self,
        predicate: impl Fn(&FakeRequest) -> bool + Send + Sync + 'static,
        reply: FakeReply,
    ) -> &Self {
        lock(&self.state.rules).push((Arc::new(predicate), reply));
        self
    }

    /// The recorded requests, oldest first.
    pub fn requests(&self) -> Vec<FakeRequest> {
        lock(&self.state.requests).clone()
    }

    /// The texts embedded so far.
    pub fn embedded(&self) -> Vec<String> {
        lock(&self.state.embedded).clone()
    }

    /// What each call of tool `name` returned to the model, as JSON, in
    /// call order (an error is `{"error": {...}}`).
    pub fn tool_results(&self, name: &str) -> Vec<Value> {
        let mut seen = BTreeSet::new();
        let mut results = Vec::new();
        for request in lock(&self.state.requests).iter() {
            for message in &request.messages {
                for result in message.tool_results() {
                    if result.name == name && seen.insert(result.call_id.clone()) {
                        results.push(result.content.clone());
                    }
                }
            }
        }
        results
    }

    /// Panics unless some recorded request matches `predicate`.
    #[track_caller]
    pub fn assert_prompted(&self, predicate: impl Fn(&FakeRequest) -> bool) {
        let requests = self.requests();
        assert!(
            requests.iter().any(predicate),
            "no AI request matches the predicate; requests: {}",
            summary(&requests)
        );
    }

    /// Panics if some recorded request matches `predicate`.
    #[track_caller]
    pub fn assert_not_prompted(&self, predicate: impl Fn(&FakeRequest) -> bool) {
        let requests = self.requests();
        assert!(
            !requests.iter().any(predicate),
            "an AI request matches the predicate; requests: {}",
            summary(&requests)
        );
    }

    /// Panics unless exactly `count` requests were made.
    #[track_caller]
    pub fn assert_prompt_count(&self, count: usize) {
        let requests = self.requests();
        assert_eq!(
            requests.len(),
            count,
            "expected {count} AI requests; requests: {}",
            summary(&requests)
        );
    }

    /// Panics if any request was made.
    #[track_caller]
    pub fn assert_nothing_prompted(&self) {
        self.assert_prompt_count(0);
    }

    /// Panics unless an agent called tool `name` (its result reached the
    /// model).
    #[track_caller]
    pub fn assert_tool_called(&self, name: &str) {
        assert!(
            !self.tool_results(name).is_empty(),
            "the tool `{name}` was not called"
        );
    }

    /// Panics if an agent called tool `name`.
    #[track_caller]
    pub fn assert_tool_not_called(&self, name: &str) {
        let results = self.tool_results(name);
        assert!(
            results.is_empty(),
            "the tool `{name}` was called {} time(s)",
            results.len()
        );
    }

    /// The driver that answers from this fake.
    pub(crate) fn driver(&self) -> Arc<dyn Driver> {
        Arc::new(FakeDriver { fake: self.clone() })
    }

    /// Records `request` and picks its reply.
    fn answer(&self, request: &ChatRequest) -> Result<FakeReply, ProviderError> {
        let recorded = FakeRequest::from_chat(request);
        lock(&self.state.requests).push(recorded.clone());
        let rule = lock(&self.state.rules)
            .iter()
            .find(|(predicate, _)| predicate(&recorded))
            .map(|(_, reply)| reply.clone());
        let reply = match rule {
            Some(reply) => Some(reply),
            None => lock(&self.state.queue).pop_front(),
        };
        reply.ok_or_else(|| {
            ProviderError::Scripted(AiError::NoFakeReply {
                prompt: recorded.last_user_message(),
            })
        })
    }

    /// A reply as an answer.
    fn response(
        &self,
        request: &ChatRequest,
        reply: FakeReply,
    ) -> Result<ChatResponse, ProviderError> {
        let (parts, finish, output) = match reply.kind {
            ReplyKind::Error(error) => return Err(ProviderError::Scripted(error)),
            ReplyKind::Text(text) => {
                let len = text.len();
                (vec![Part::Text { text }], Finish::Stop, len)
            }
            ReplyKind::Json(value) => {
                let text = value.to_string();
                let len = text.len();
                (vec![Part::Text { text }], Finish::Stop, len)
            }
            ReplyKind::Refusal => (Vec::new(), Finish::Refused, 0),
            ReplyKind::ToolCall { name, arguments } => {
                let id = format!(
                    "call_{}",
                    self.state.calls.fetch_add(1, Ordering::Relaxed) + 1
                );
                let len = arguments.to_string().len();
                (
                    vec![Part::ToolCall(ToolCall {
                        id,
                        name,
                        arguments,
                        meta: None,
                    })],
                    Finish::ToolCalls,
                    len,
                )
            }
        };
        let usage = reply.usage.unwrap_or_else(|| {
            let input: usize = request.system.as_deref().map_or(0, str::len)
                + request
                    .messages
                    .iter()
                    .map(|m| m.text().len())
                    .sum::<usize>();
            Usage::new(tokens(input), tokens(output))
        });
        Ok(ChatResponse {
            parts,
            usage,
            finish,
        })
    }
}

/// About four characters per token, at least one.
fn tokens(chars: usize) -> u64 {
    (chars as u64).div_ceil(4).max(1)
}

impl fmt::Debug for AiFake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AiFake")
            .field("queued", &lock(&self.state.queue).len())
            .field("requests", &lock(&self.state.requests).len())
            .finish()
    }
}

/// [`AiFake`] as a provider driver.
struct FakeDriver {
    fake: AiFake,
}

impl Driver for FakeDriver {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            images: true,
            pdfs: true,
            embeddings: true,
            native_structured: true,
        }
    }

    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> BoxFuture<'a, Result<ChatResponse, ProviderError>> {
        Box::pin(async move {
            let reply = self.fake.answer(request)?;
            self.fake.response(request, reply)
        })
    }

    fn stream<'a>(
        &'a self,
        request: &'a ChatRequest,
    ) -> BoxFuture<'a, Result<ChunkStream, ProviderError>> {
        Box::pin(async move {
            let reply = self.fake.answer(request)?;
            let response = self.fake.response(request, reply)?;
            let mut chunks = Vec::new();
            for part in response.parts {
                match part {
                    Part::Text { text } => {
                        chunks.extend(words(&text).into_iter().map(|w| Ok(StreamChunk::Text(w))));
                    }
                    Part::ToolCall(call) => chunks.push(Ok(StreamChunk::ToolCall(call))),
                    _ => {}
                }
            }
            chunks.push(Ok(StreamChunk::Done {
                usage: response.usage,
                finish: response.finish,
            }));
            Ok(Box::pin(futures_util::stream::iter(chunks)) as ChunkStream)
        })
    }

    fn embed<'a>(
        &'a self,
        request: &'a EmbedRequest,
    ) -> BoxFuture<'a, Result<EmbedResponse, ProviderError>> {
        Box::pin(async move {
            lock(&self.fake.state.embedded).extend(request.texts.iter().cloned());
            let dimensions = request.dimensions.unwrap_or(1536);
            Ok(EmbedResponse {
                vectors: request
                    .texts
                    .iter()
                    .map(|text| hashed_embedding(text, dimensions))
                    .collect(),
                input_tokens: request.texts.iter().map(|t| tokens(t.len())).sum(),
            })
        })
    }
}

/// `text` split before each space, so the pieces join back to `text`.
fn words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, word) in text.split(' ').enumerate() {
        if i == 0 {
            out.push(word.to_owned());
        } else {
            out.push(format!(" {word}"));
        }
    }
    out.retain(|w| !w.is_empty());
    out
}

/// A deterministic embedding: each lower-cased word (letters and digits)
/// hashed into one of `dimensions` buckets with a sign, then normalized, so
/// texts that share words are close and texts that don't are far.
pub(crate) fn hashed_embedding(text: &str, dimensions: usize) -> Vec<f32> {
    let mut vector = vec![0f32; dimensions.max(1)];
    for word in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
    {
        let hash = fnv1a(word.as_bytes());
        let index = (hash % vector.len() as u64) as usize;
        let sign = if (hash >> 63) == 0 { 1.0 } else { -1.0 };
        vector[index] += sign;
    }
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        vector.iter_mut().for_each(|v| *v /= norm);
    }
    vector
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// One line per request, for assertion messages.
fn summary(requests: &[FakeRequest]) -> String {
    if requests.is_empty() {
        return "none".to_owned();
    }
    requests
        .iter()
        .map(|r| format!("[{}] {:?}", r.model, truncate(&r.last_user_message(), 80)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        format!("{}...", text.chars().take(max).collect::<String>())
    }
}

/// Locks `mutex`, ignoring poisoning.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_join_back() {
        assert_eq!(words("Agra moved on"), ["Agra", " moved", " on"]);
        assert_eq!(words("Agra moved on").concat(), "Agra moved on");
    }

    #[test]
    fn hashed_embeddings_are_close_for_shared_words() {
        let a = hashed_embedding("Dana wants to be live before January planning", 64);
        let b = hashed_embedding("live before January", 64);
        let c = hashed_embedding("Gulati prefers summer", 64);
        let dot = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(p, q)| p * q).sum::<f32>();
        assert!(dot(&a, &b) > dot(&a, &c));
        assert!((dot(&a, &a) - 1.0).abs() < 1e-5);
        assert_eq!(
            a,
            hashed_embedding("Dana wants to be live before January planning", 64)
        );
    }
}
