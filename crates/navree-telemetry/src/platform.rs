//! The events of the platform modules: model calls, broadcasts, file
//! operations, webhook deliveries, searches, sign-ins and Navree UI
//! operations, each with the constructor that makes an emit one line.
//!
//! None of them carries a payload: no prompt or answer, no message body, no
//! file contents, no search term, no email address, no password or token,
//! no form values. They say what happened, to what, how long it took and
//! how it ended.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::event::millis;
use crate::{EventKind, Telemetry, TelemetryEvent};

/// How an operation ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// It worked.
    Success,
    /// It did not.
    Failure,
}

impl Outcome {
    /// The snake_case name (`success`).
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Success => "success",
            Outcome::Failure => "failure",
        }
    }

    /// Whether it worked.
    pub fn is_success(self) -> bool {
        self == Outcome::Success
    }
}

/// A call to a model provider finished (navree-ai): text, structured
/// output, a stream or an embedding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiCall {
    /// The provider (`anthropic`, `openai`, `gemini`, `mistral`, `ollama`,
    /// `bedrock`, `fake`).
    pub provider: String,
    /// The model the call named.
    pub model: String,
    /// What was asked: `generate`, `stream` or `embed`.
    pub operation: String,
    /// The input tokens the provider counted, when it said.
    pub input_tokens: Option<u64>,
    /// The output tokens the provider counted, when it said.
    pub output_tokens: Option<u64>,
    /// The part of the input read from the provider's prompt cache.
    pub cached_input_tokens: Option<u64>,
    /// What the call cost, in millionths of a US dollar, when the app
    /// prices its models (`None` when it does not).
    pub cost_micros: Option<u64>,
    /// The time from the request to the whole answer (for a stream, to its
    /// last chunk).
    #[serde(with = "millis")]
    pub duration: Duration,
    /// How it ended.
    pub outcome: Outcome,
    /// A short description of the failure; never a prompt, an answer or a
    /// key.
    pub error: Option<String>,
}

impl AiCall {
    /// A call of `operation` to `model` at `provider` that worked and took
    /// `duration`.
    pub fn new(
        provider: impl Into<String>,
        model: impl Into<String>,
        operation: impl Into<String>,
        duration: Duration,
    ) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            operation: operation.into(),
            input_tokens: None,
            output_tokens: None,
            cached_input_tokens: None,
            cost_micros: None,
            duration,
            outcome: Outcome::Success,
            error: None,
        }
    }

    /// The tokens the provider counted.
    #[must_use]
    pub fn tokens(mut self, input: u64, output: u64) -> Self {
        self.input_tokens = Some(input);
        self.output_tokens = Some(output);
        self
    }

    /// The part of the input that came from the prompt cache.
    #[must_use]
    pub fn cached_tokens(mut self, cached: u64) -> Self {
        self.cached_input_tokens = Some(cached);
        self
    }

    /// What the call cost, in millionths of a US dollar.
    #[must_use]
    pub fn cost_micros(mut self, micros: u64) -> Self {
        self.cost_micros = Some(micros);
        self
    }

    /// The call failed with `error`.
    #[must_use]
    pub fn failed(mut self, error: impl Into<String>) -> Self {
        self.outcome = Outcome::Failure;
        self.error = Some(error.into());
        self
    }
}

/// An event was pushed to real-time channels (navree-realtime).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BroadcastSent {
    /// The channels it went to.
    pub channels: Vec<String>,
    /// The event name (`deal.updated`), never its data.
    pub event: String,
    /// The deliveries queued to this process's subscribers (a connection on
    /// two of the channels counts twice).
    pub delivered: u64,
    /// How long the broadcast took in this process.
    #[serde(with = "millis")]
    pub duration: Duration,
    /// How it ended.
    pub outcome: Outcome,
    /// A short description of the failure.
    pub error: Option<String>,
}

impl BroadcastSent {
    /// An `event` sent to `channels` that reached `delivered` subscribers.
    pub fn new(
        channels: impl IntoIterator<Item = impl Into<String>>,
        event: impl Into<String>,
        delivered: u64,
        duration: Duration,
    ) -> Self {
        Self {
            channels: channels.into_iter().map(Into::into).collect(),
            event: event.into(),
            delivered,
            duration,
            outcome: Outcome::Success,
            error: None,
        }
    }

    /// The broadcast failed with `error`.
    #[must_use]
    pub fn failed(mut self, error: impl Into<String>) -> Self {
        self.outcome = Outcome::Failure;
        self.error = Some(error.into());
        self
    }
}

/// What a file operation did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StorageOperation {
    /// A file was read.
    Read,
    /// A file was written.
    Write,
    /// A file was deleted.
    Delete,
    /// A directory was listed.
    List,
    /// A file was copied (navree-storage reports reads and writes; an app
    /// that copies between disks may report the copy itself).
    Copy,
}

impl StorageOperation {
    /// The snake_case name (`write`).
    pub fn as_str(self) -> &'static str {
        match self {
            StorageOperation::Read => "read",
            StorageOperation::Write => "write",
            StorageOperation::Delete => "delete",
            StorageOperation::List => "list",
            StorageOperation::Copy => "copy",
        }
    }
}

/// A file operation finished (navree-storage).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageOp {
    /// The disk's name.
    pub disk: String,
    /// The disk's driver (`local`, `memory`, `s3`, `gcs`, `azure`).
    pub driver: String,
    /// What was done.
    pub operation: StorageOperation,
    /// The path on the disk, or the directory listed; never the contents.
    pub path: String,
    /// The bytes read or written, when known.
    pub bytes: Option<u64>,
    /// How long it took.
    #[serde(with = "millis")]
    pub duration: Duration,
    /// How it ended.
    pub outcome: Outcome,
    /// A short description of the failure.
    pub error: Option<String>,
}

impl StorageOp {
    /// An `operation` on `path` of the disk `disk` (driver `driver`) that
    /// worked and took `duration`.
    pub fn new(
        disk: impl Into<String>,
        driver: impl Into<String>,
        operation: StorageOperation,
        path: impl Into<String>,
        duration: Duration,
    ) -> Self {
        Self {
            disk: disk.into(),
            driver: driver.into(),
            operation,
            path: path.into(),
            bytes: None,
            duration,
            outcome: Outcome::Success,
            error: None,
        }
    }

    /// The bytes read or written.
    #[must_use]
    pub fn bytes(mut self, bytes: u64) -> Self {
        self.bytes = Some(bytes);
        self
    }

    /// The operation failed with `error`.
    #[must_use]
    pub fn failed(mut self, error: impl Into<String>) -> Self {
        self.outcome = Outcome::Failure;
        self.error = Some(error.into());
        self
    }
}

/// One attempt to deliver a webhook finished (navree-webhooks).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookDelivery {
    /// The endpoint: its id, or its URL without query string, fragment or
    /// credentials.
    pub endpoint: String,
    /// The event delivered (`invoice.paid`), never its payload.
    pub event: String,
    /// The attempt (1 for the first).
    pub attempt: u32,
    /// The status the endpoint answered; `None` when no answer came.
    pub status: Option<u16>,
    /// How long the attempt took.
    #[serde(with = "millis")]
    pub duration: Duration,
    /// Whether the endpoint took it (a 2xx answer).
    pub outcome: Outcome,
    /// A short description of the failure; never the endpoint's body.
    pub error: Option<String>,
}

impl WebhookDelivery {
    /// Attempt `attempt` to deliver `event` to `endpoint`, answered with
    /// `status` after `duration`. A 2xx answer is a success, any other
    /// status a failure.
    pub fn new(
        endpoint: impl Into<String>,
        event: impl Into<String>,
        attempt: u32,
        status: u16,
        duration: Duration,
    ) -> Self {
        let outcome = if (200..300).contains(&status) {
            Outcome::Success
        } else {
            Outcome::Failure
        };
        Self {
            endpoint: endpoint.into(),
            event: event.into(),
            attempt,
            status: Some(status),
            duration,
            outcome,
            error: None,
        }
    }

    /// An attempt that got no answer: `error` says why.
    pub fn unanswered(
        endpoint: impl Into<String>,
        event: impl Into<String>,
        attempt: u32,
        duration: Duration,
        error: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            event: event.into(),
            attempt,
            status: None,
            duration,
            outcome: Outcome::Failure,
            error: Some(error.into()),
        }
    }
}

/// How a search ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SearchMode {
    /// Full text.
    Text,
    /// Full text and vectors together.
    Hybrid,
    /// Documents like a given text or document.
    Similar,
    /// One term across several indexes.
    Across,
}

impl SearchMode {
    /// The snake_case name (`hybrid`).
    pub fn as_str(self) -> &'static str {
        match self {
            SearchMode::Text => "text",
            SearchMode::Hybrid => "hybrid",
            SearchMode::Similar => "similar",
            SearchMode::Across => "across",
        }
    }
}

/// A search finished (navree-search). The term is not in the event, only its
/// length: what people look for can name a person.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQueried {
    /// The engine (`postgres`, `meilisearch`, `typesense`, `tantivy`,
    /// `memory`).
    pub engine: String,
    /// The index searched (for `Across`, the indexes joined with a comma).
    pub index: String,
    /// How the search ran.
    pub mode: SearchMode,
    /// The length of the term in characters.
    pub term_length: u32,
    /// The hits the engine found, when it said.
    pub hits: Option<u64>,
    /// How long it took.
    #[serde(with = "millis")]
    pub duration: Duration,
    /// How it ended.
    pub outcome: Outcome,
    /// A short description of the failure.
    pub error: Option<String>,
}

impl SearchQueried {
    /// A `mode` search of `index` on `engine` for a term of `term` (only
    /// its length is kept) that worked and took `duration`.
    pub fn new(
        engine: impl Into<String>,
        index: impl Into<String>,
        mode: SearchMode,
        term: &str,
        duration: Duration,
    ) -> Self {
        Self {
            engine: engine.into(),
            index: index.into(),
            mode,
            term_length: u32::try_from(term.chars().count()).unwrap_or(u32::MAX),
            hits: None,
            duration,
            outcome: Outcome::Success,
            error: None,
        }
    }

    /// The hits the engine found.
    #[must_use]
    pub fn hits(mut self, hits: u64) -> Self {
        self.hits = Some(hits);
        self
    }

    /// The search failed with `error`.
    #[must_use]
    pub fn failed(mut self, error: impl Into<String>) -> Self {
        self.outcome = Outcome::Failure;
        self.error = Some(error.into());
        self
    }
}

/// How a person signed in. The names are those of navree-auth's
/// `SignInMethod::name`, with `code` for its `otp` and `two_factor` for the
/// second step of a sign-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SignInMethod {
    /// An email and a password.
    Password,
    /// The code from an authenticator app, or a recovery code.
    TwoFactor,
    /// A mailed link.
    MagicLink,
    /// A mailed code.
    Code,
    /// A passkey.
    Passkey,
    /// A social sign-in provider.
    Social,
    /// A single sign-on connection.
    Sso,
    /// A token of a trusted issuer.
    TokenExchange,
}

impl SignInMethod {
    /// The snake_case name (`magic_link`).
    pub fn as_str(self) -> &'static str {
        match self {
            SignInMethod::Password => "password",
            SignInMethod::TwoFactor => "two_factor",
            SignInMethod::MagicLink => "magic_link",
            SignInMethod::Code => "code",
            SignInMethod::Passkey => "passkey",
            SignInMethod::Social => "social",
            SignInMethod::Sso => "sso",
            SignInMethod::TokenExchange => "token_exchange",
        }
    }
}

/// A sign-in attempt ended. The event never holds the email address, the
/// password, a code or a token; the user is named only when the sign-in
/// worked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignInEvent {
    /// How the person signed in.
    pub method: SignInMethod,
    /// The social provider's key, the single sign-on connection's id or the
    /// token issuer's name, when the method has one.
    pub provider: Option<String>,
    /// How it ended.
    pub outcome: Outcome,
    /// The user's id; only when the sign-in worked.
    pub user_id: Option<String>,
    /// Why it failed, as a code (`invalid_credentials`, `throttled`,
    /// `invalid_code`, or the code of the sign-in policy that refused it);
    /// `None` when it worked.
    pub reason: Option<String>,
}

impl SignInEvent {
    /// `user_id` signed in with `method`.
    pub fn succeeded(method: SignInMethod, user_id: impl Into<String>) -> Self {
        Self {
            method,
            provider: None,
            outcome: Outcome::Success,
            user_id: Some(user_id.into()),
            reason: None,
        }
    }

    /// A sign-in with `method` was refused for `reason`.
    pub fn refused(method: SignInMethod, reason: impl Into<String>) -> Self {
        Self {
            method,
            provider: None,
            outcome: Outcome::Failure,
            user_id: None,
            reason: Some(reason.into()),
        }
    }

    /// The social provider, connection or issuer.
    #[must_use]
    pub fn provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }
}

/// What a Navree UI operation was.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UiOperationKind {
    /// A form posted.
    FormPost,
    /// A button or menu action that answers operations.
    Action,
    /// A table asked for its rows (sort, filter, search, page).
    TableQuery,
    /// A region fetched again (a poll, a feed, a lazy region).
    LiveRegion,
    /// A fragment of a page fetched for a swap.
    Fragment,
    /// A chunk or a finish of an upload.
    Upload,
}

impl UiOperationKind {
    /// The snake_case name (`table_query`).
    pub fn as_str(self) -> &'static str {
        match self {
            UiOperationKind::FormPost => "form_post",
            UiOperationKind::Action => "action",
            UiOperationKind::TableQuery => "table_query",
            UiOperationKind::LiveRegion => "live_region",
            UiOperationKind::Fragment => "fragment",
            UiOperationKind::Upload => "upload",
        }
    }
}

/// A Navree UI operation was answered (navree-ui): the server's side of a
/// form post, an action, a table query or a region fetch. No values: not
/// the form's fields, not the rows, not the markup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiOperation {
    /// The matched route template (`/deals/{id}/stage`), or the operation's
    /// own name.
    pub name: String,
    /// What kind of operation it was.
    pub kind: UiOperationKind,
    /// How many operations (morphs, toasts, redirects...) the answer held.
    pub ops: u32,
    /// The response status.
    pub status: u16,
    /// The time from the request's arrival to the answer.
    #[serde(with = "millis")]
    pub duration: Duration,
}

impl UiOperation {
    /// A `kind` operation named `name`, answered with `status` and `ops`
    /// operations after `duration`.
    pub fn new(
        name: impl Into<String>,
        kind: UiOperationKind,
        status: u16,
        ops: u32,
        duration: Duration,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            ops,
            status,
            duration,
        }
    }
}

/// One-line emits for the platform events: the closure runs only when some
/// sink wants the kind.
///
/// ```
/// use std::time::Duration;
/// use navree_telemetry::{AiCall, Telemetry};
///
/// let telemetry = Telemetry::disabled();
/// telemetry.ai_call(|| {
///     AiCall::new("anthropic", "claude-sonnet-4-5", "generate", Duration::from_millis(900))
///         .tokens(1200, 340)
/// });
/// ```
impl Telemetry {
    /// Emits an [`AiCall`].
    pub fn ai_call(&self, build: impl FnOnce() -> AiCall) {
        self.emit_with(EventKind::AiCall, || TelemetryEvent::AiCall(build()));
    }

    /// Emits a [`BroadcastSent`].
    pub fn broadcast(&self, build: impl FnOnce() -> BroadcastSent) {
        self.emit_with(EventKind::Broadcast, || {
            TelemetryEvent::Broadcast(build())
        });
    }

    /// Emits a [`StorageOp`].
    pub fn storage_op(&self, build: impl FnOnce() -> StorageOp) {
        self.emit_with(EventKind::StorageOp, || {
            TelemetryEvent::StorageOp(build())
        });
    }

    /// Emits a [`WebhookDelivery`].
    pub fn webhook_delivery(&self, build: impl FnOnce() -> WebhookDelivery) {
        self.emit_with(EventKind::WebhookDelivery, || {
            TelemetryEvent::WebhookDelivery(build())
        });
    }

    /// Emits a [`SearchQueried`].
    pub fn search(&self, build: impl FnOnce() -> SearchQueried) {
        self.emit_with(EventKind::Search, || TelemetryEvent::Search(build()));
    }

    /// Emits a [`SignInEvent`].
    pub fn sign_in(&self, build: impl FnOnce() -> SignInEvent) {
        self.emit_with(EventKind::SignIn, || TelemetryEvent::SignIn(build()));
    }

    /// Emits a [`UiOperation`].
    pub fn ui_operation(&self, build: impl FnOnce() -> UiOperation) {
        self.emit_with(EventKind::UiOperation, || {
            TelemetryEvent::UiOperation(build())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_keeps_the_length_of_its_term_and_not_the_term() {
        let search = SearchQueried::new(
            "postgres",
            "deals",
            SearchMode::Text,
            "Gurpreet Singh",
            Duration::from_millis(4),
        );
        assert_eq!(search.term_length, 14);
        let json = serde_json::to_string(&search).unwrap();
        assert!(!json.contains("Gurpreet"), "{json}");
    }

    #[test]
    fn a_webhook_status_decides_the_outcome() {
        let ok = WebhookDelivery::new("ep-1", "invoice.paid", 1, 204, Duration::from_millis(30));
        assert_eq!(ok.outcome, Outcome::Success);
        let refused =
            WebhookDelivery::new("ep-1", "invoice.paid", 2, 503, Duration::from_millis(30));
        assert_eq!(refused.outcome, Outcome::Failure);
        let down = WebhookDelivery::unanswered(
            "ep-1",
            "invoice.paid",
            3,
            Duration::from_secs(10),
            "timed out",
        );
        assert_eq!(down.status, None);
        assert_eq!(down.outcome, Outcome::Failure);
    }

    #[test]
    fn a_refused_sign_in_names_no_user() {
        let refused = SignInEvent::refused(SignInMethod::Password, "invalid_credentials");
        assert_eq!(refused.user_id, None);
        assert_eq!(refused.reason.as_deref(), Some("invalid_credentials"));
        let worked = SignInEvent::succeeded(SignInMethod::Social, "u-1").provider("github");
        assert_eq!(worked.user_id.as_deref(), Some("u-1"));
        assert_eq!(worked.provider.as_deref(), Some("github"));
        assert_eq!(worked.reason, None);
    }
}
