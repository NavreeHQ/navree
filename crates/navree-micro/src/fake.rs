//! [`MicroFake`]: an in-memory transport with stubs and assertions, the
//! usual way to test a service.

use std::collections::HashMap;
use std::fmt;
use std::future::IntoFuture;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use http::header::{CONTENT_TYPE, HeaderValue};
use http::{HeaderMap, StatusCode};
use navree_contracts::Principal;
use navree_http::AppError;
use navree_kernel::BoxFuture;
use navree_telemetry::TracePolicy;
use navree_testing::TestResponse;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::mpsc;

use navree_contracts::RequestId;

use crate::client::{ClientCore, EmitBuilder, SendBuilder};
use crate::envelope::LocalContext;
use crate::{
    Codec, DeadLetter, Delivery, Envelope, MemoryTransport, MicroError, MicroTransport,
    ServiceClient, Subscription, Transport, headers,
};

/// An in-memory transport for tests that records what the app sends and
/// emits, answers the messages it sends to other services, and sends
/// messages and events into the app's handlers.
///
/// ```ignore
/// let micro = MicroFake::new();
/// let app = TestApp::builder()
///     .module(MicroModule::for_root(micro.transport()))
///     .module(DealsModule)
///     .build()
///     .await;
///
/// micro.on("companies.get", Reply::json(&company));
/// let deal: DealView = micro.client().acting_as(principal(rep)).send("deals.get", &GetDeal { id }).await?;
/// micro.assert_emitted("deal.viewed", |event: &DealViewed| event.deal_id == id);
/// ```
///
/// The app's listener starts at boot. Failed events are retried at once,
/// without waiting for the backoff.
#[derive(Clone)]
pub struct MicroFake {
    inner: Arc<FakeInner>,
}

struct FakeInner {
    broker: MemoryTransport,
    stubs: Mutex<HashMap<String, Stub>>,
    sent: Mutex<Vec<Envelope>>,
    emitted: Mutex<Vec<Envelope>>,
    dead: Mutex<Vec<DeadLetter>>,
    /// The message id last emitted per pattern by a fake client, for
    /// `redelivered`.
    last_ids: Mutex<HashMap<String, String>>,
}

impl Default for MicroFake {
    fn default() -> Self {
        Self::new()
    }
}

impl MicroFake {
    /// A fake with an empty in-memory broker.
    pub fn new() -> Self {
        let broker = MemoryTransport::new();
        broker.set_instant_retries(true);
        Self {
            inner: Arc::new(FakeInner {
                broker,
                stubs: Mutex::default(),
                sent: Mutex::default(),
                emitted: Mutex::default(),
                dead: Mutex::default(),
                last_ids: Mutex::default(),
            }),
        }
    }

    /// The transport to give the app: `MicroModule::for_root(micro.transport())`.
    pub fn transport(&self) -> Transport {
        Transport::custom(FakeTransport {
            inner: self.inner.clone(),
        })
    }

    /// A client that sends into the app's handlers through the whole
    /// pipeline (authentication, guards, access, validation, unit of work).
    /// Its messages are not recorded as sent by the app.
    pub fn client(&self) -> FakeClient {
        FakeClient {
            fake: self.clone(),
            principal: None,
        }
    }

    /// Answers messages to `pattern` that the app sends and does not handle
    /// itself; a later `on` for the same pattern replaces this one. A
    /// pattern without an answer fails with `MicroError::NoResponders`.
    pub fn on(&self, pattern: &str, reply: Reply) {
        lock(&self.inner.stubs).insert(pattern.to_owned(), Stub { reply, turn: 0 });
    }

    /// The envelopes the app sent to `pattern` (requests).
    pub fn sent_envelopes(&self, pattern: &str) -> Vec<Envelope> {
        of_pattern(&lock(&self.inner.sent), pattern)
    }

    /// The decoded payloads the app sent to `pattern`.
    ///
    /// # Panics
    ///
    /// When a payload does not decode into `T`.
    #[track_caller]
    pub fn sent<T: DeserializeOwned>(&self, pattern: &str) -> Vec<T> {
        decode_all(&self.sent_envelopes(pattern), pattern)
    }

    /// Asserts that the app sent a message to `pattern` whose payload
    /// satisfies `check`.
    ///
    /// # Panics
    ///
    /// When none did.
    #[track_caller]
    pub fn assert_sent<T: DeserializeOwned>(&self, pattern: &str, check: impl Fn(&T) -> bool) {
        let sent = self.sent::<T>(pattern);
        assert!(
            sent.iter().any(check),
            "expected a message to `{pattern}` matching the check; sent {} to it{}",
            sent.len(),
            describe_all(&lock(&self.inner.sent))
        );
    }

    /// Asserts that the app sent `times` messages to `pattern`.
    ///
    /// # Panics
    ///
    /// When it sent another number.
    #[track_caller]
    pub fn assert_sent_times(&self, pattern: &str, times: usize) {
        let sent = self.sent_envelopes(pattern).len();
        assert_eq!(sent, times, "messages sent to `{pattern}`");
    }

    /// Asserts that the app sent no message at all.
    ///
    /// # Panics
    ///
    /// When it sent one.
    #[track_caller]
    pub fn assert_nothing_sent(&self) {
        let sent = lock(&self.inner.sent);
        assert!(
            sent.is_empty(),
            "expected no messages; sent{}",
            describe_all(&sent)
        );
    }

    /// The envelopes of the events the app published under `pattern`
    /// (inside a unit of work: once it committed).
    pub fn emitted_envelopes(&self, pattern: &str) -> Vec<Envelope> {
        of_pattern(&lock(&self.inner.emitted), pattern)
    }

    /// The decoded events the app published under `pattern`.
    ///
    /// # Panics
    ///
    /// When an event does not decode into `T`.
    #[track_caller]
    pub fn emitted<T: DeserializeOwned>(&self, pattern: &str) -> Vec<T> {
        decode_all(&self.emitted_envelopes(pattern), pattern)
    }

    /// Asserts that the app published an event under `pattern` that
    /// satisfies `check`.
    ///
    /// # Panics
    ///
    /// When none did.
    #[track_caller]
    pub fn assert_emitted<T: DeserializeOwned>(&self, pattern: &str, check: impl Fn(&T) -> bool) {
        let emitted = self.emitted::<T>(pattern);
        assert!(
            emitted.iter().any(check),
            "expected an event `{pattern}` matching the check; emitted {} of them{}",
            emitted.len(),
            describe_all(&lock(&self.inner.emitted))
        );
    }

    /// Asserts that the app published `times` events under `pattern`.
    ///
    /// # Panics
    ///
    /// When it published another number.
    #[track_caller]
    pub fn assert_emitted_times(&self, pattern: &str, times: usize) {
        let emitted = self.emitted_envelopes(pattern).len();
        assert_eq!(emitted, times, "events emitted as `{pattern}`");
    }

    /// Asserts that the app published no event at all.
    ///
    /// # Panics
    ///
    /// When it published one.
    #[track_caller]
    pub fn assert_nothing_emitted(&self) {
        let emitted = lock(&self.inner.emitted);
        assert!(
            emitted.is_empty(),
            "expected no events; emitted{}",
            describe_all(&emitted)
        );
    }

    /// The events the app's handlers gave up on, whatever
    /// `micro.events.dead_letter` says: pattern, payload, attempts and the
    /// last error.
    pub fn dead_letters(&self) -> Vec<DeadLetter> {
        lock(&self.inner.dead).clone()
    }
}

impl fmt::Debug for MicroFake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MicroFake")
            .field("sent", &lock(&self.inner.sent).len())
            .field("emitted", &lock(&self.inner.emitted).len())
            .field("dead_letters", &lock(&self.inner.dead).len())
            .finish_non_exhaustive()
    }
}

fn of_pattern(envelopes: &[Envelope], pattern: &str) -> Vec<Envelope> {
    envelopes
        .iter()
        .filter(|envelope| envelope.pattern() == pattern)
        .cloned()
        .collect()
}

#[track_caller]
fn decode_all<T: DeserializeOwned>(envelopes: &[Envelope], pattern: &str) -> Vec<T> {
    envelopes
        .iter()
        .map(|envelope| {
            let codec = Codec::from_content_type(envelope.content_type()).unwrap_or_default();
            codec.decode(envelope.payload()).unwrap_or_else(|error| {
                panic!(
                    "a payload of `{pattern}` is not a `{}` ({error}): {}",
                    std::any::type_name::<T>(),
                    String::from_utf8_lossy(envelope.payload())
                )
            })
        })
        .collect()
}

fn describe_all(envelopes: &[Envelope]) -> String {
    if envelopes.is_empty() {
        return " nothing".to_owned();
    }
    let mut out = String::from(":");
    for envelope in envelopes {
        out.push_str(&format!(
            "\n  {} {}",
            envelope.pattern(),
            String::from_utf8_lossy(envelope.payload())
        ));
    }
    out
}

/// How the fake answers a pattern ([`MicroFake::on`]).
///
/// | Answer | Behaves like |
/// |---|---|
/// | `Reply::json(&value)` | A handler that returned `Ok(value)`. |
/// | `Reply::error(app_error)` | A handler that returned the error. |
/// | `Reply::from_fn(\|payload: T\| ...)` | A handler you write inline: it receives the decoded payload and returns `Result<R, AppError>`. |
/// | `Reply::timeout()` | A service that never answers: the call times out at once. |
/// | `Reply::sequence([..])` | Each answer in turn, then the last one again. |
#[derive(Clone)]
pub struct Reply(ReplyKind);

type ReplyFn = Arc<dyn Fn(&Envelope) -> Answer + Send + Sync>;

#[derive(Clone)]
enum ReplyKind {
    Answer(Answer),
    Fn(ReplyFn),
    Timeout,
    Sequence(Vec<Reply>),
}

/// A status and a JSON body.
#[derive(Clone)]
struct Answer {
    status: u16,
    body: Bytes,
}

impl Reply {
    /// A handler that returned `Ok(value)`.
    pub fn json<T: Serialize + ?Sized>(value: &T) -> Reply {
        let body = serde_json::to_vec(value)
            .unwrap_or_else(|error| panic!("Reply::json: the value does not serialize: {error}"));
        Reply(ReplyKind::Answer(Answer {
            status: 200,
            body: body.into(),
        }))
    }

    /// A handler that returned `error`.
    pub fn error(error: AppError) -> Reply {
        Reply(ReplyKind::Answer(error_answer(&error)))
    }

    /// A handler written inline: `f` receives the decoded payload.
    pub fn from_fn<T, R, F>(f: F) -> Reply
    where
        T: DeserializeOwned,
        R: Serialize,
        F: Fn(T) -> Result<R, AppError> + Send + Sync + 'static,
    {
        Reply(ReplyKind::Fn(Arc::new(move |envelope: &Envelope| {
            let codec = Codec::from_content_type(envelope.content_type()).unwrap_or_default();
            let payload = match codec.decode::<T>(envelope.payload()) {
                Ok(payload) => payload,
                Err(error) => {
                    return error_answer(&AppError::bad_request(format!(
                        "The payload does not decode: {error}"
                    )));
                }
            };
            match f(payload) {
                Ok(value) => match serde_json::to_vec(&value) {
                    Ok(body) => Answer {
                        status: 200,
                        body: body.into(),
                    },
                    Err(error) => error_answer(&AppError::internal(error)),
                },
                Err(error) => error_answer(&error),
            }
        })))
    }

    /// A service that never answers: the call fails with
    /// `MicroError::Timeout` at once.
    pub fn timeout() -> Reply {
        Reply(ReplyKind::Timeout)
    }

    /// Each answer in turn, then the last one again.
    pub fn sequence(replies: impl IntoIterator<Item = Reply>) -> Reply {
        Reply(ReplyKind::Sequence(replies.into_iter().collect()))
    }
}

impl fmt::Debug for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match &self.0 {
            ReplyKind::Answer(answer) => format!("answer {}", answer.status),
            ReplyKind::Fn(_) => "fn".to_owned(),
            ReplyKind::Timeout => "timeout".to_owned(),
            ReplyKind::Sequence(list) => format!("sequence of {}", list.len()),
        };
        f.debug_tuple("Reply").field(&kind).finish()
    }
}

fn error_answer(error: &AppError) -> Answer {
    Answer {
        status: error.status.as_u16(),
        body: serde_json::to_vec(&error.body()).unwrap_or_default().into(),
    }
}

/// A stub and how far its sequence has gone.
struct Stub {
    reply: Reply,
    turn: usize,
}

impl Stub {
    /// The answer to `envelope`; `None` for a timeout.
    fn answer(&mut self, envelope: &Envelope) -> Option<Answer> {
        let reply = match &self.reply.0 {
            ReplyKind::Sequence(list) if !list.is_empty() => {
                let reply = list[self.turn.min(list.len() - 1)].clone();
                self.turn += 1;
                reply
            }
            _ => self.reply.clone(),
        };
        match reply.0 {
            ReplyKind::Answer(answer) => Some(answer),
            ReplyKind::Fn(f) => Some(f(envelope)),
            ReplyKind::Timeout | ReplyKind::Sequence(_) => None,
        }
    }
}

/// The transport the app gets: the fake's broker, recording and stubbing.
struct FakeTransport {
    inner: Arc<FakeInner>,
}

impl MicroTransport for FakeTransport {
    fn name(&self) -> &'static str {
        "memory"
    }

    fn request(&self, envelope: Envelope) -> BoxFuture<'_, Result<Envelope, MicroError>> {
        Box::pin(async move {
            lock(&self.inner.sent).push(envelope.clone());
            let pattern = envelope.pattern().to_owned();
            if self.inner.broker.has_responder(&pattern) {
                return self.inner.broker.request(envelope).await;
            }
            let answer = {
                let mut stubs = lock(&self.inner.stubs);
                match stubs.get_mut(&pattern) {
                    Some(stub) => stub.answer(&envelope),
                    None => return Err(MicroError::NoResponders { pattern }),
                }
            };
            match answer {
                Some(answer) => Ok(envelope.reply(answer.status, "application/json", answer.body)),
                None => Err(MicroError::Timeout {
                    pattern,
                    after: envelope.remaining().unwrap_or(Duration::ZERO),
                }),
            }
        })
    }

    fn publish(&self, envelope: Envelope) -> BoxFuture<'_, Result<(), MicroError>> {
        lock(&self.inner.emitted).push(envelope.clone());
        self.inner.broker.publish(envelope)
    }

    fn listen(
        &self,
        subscriptions: Vec<Subscription>,
        deliveries: mpsc::Sender<Delivery>,
    ) -> BoxFuture<'_, Result<(), MicroError>> {
        self.inner.broker.listen(subscriptions, deliveries)
    }

    fn dead_letter(&self, letter: DeadLetter) -> BoxFuture<'_, Result<(), MicroError>> {
        self.inner.broker.dead_letter(letter)
    }

    fn observe_dead_letter(&self, letter: &DeadLetter) {
        lock(&self.inner.dead).push(letter.clone());
    }

    fn listens_at_boot(&self) -> bool {
        true
    }

    fn instant_retries(&self) -> bool {
        true
    }
}

/// Sends messages and events into the app's handlers
/// ([`MicroFake::client`]).
#[derive(Clone)]
pub struct FakeClient {
    fake: MicroFake,
    principal: Option<Principal>,
}

impl FakeClient {
    /// Authenticates the messages as `principal`, as `TestApp::acting_as`
    /// does for HTTP.
    pub fn acting_as(mut self, principal: Principal) -> Self {
        self.principal = Some(principal);
        self
    }

    fn client(&self) -> ServiceClient {
        let local = Some(Arc::new(LocalContext {
            principal: self.principal.clone(),
        }));
        ServiceClient::new(ClientCore {
            transport: Arc::new(self.fake.inner.broker.clone()),
            service: "micro-fake".to_owned(),
            codec: Codec::Json,
            request_timeout: Duration::from_secs(30),
            max_message_bytes: usize::MAX,
            forward_auth: false,
            outbox: None,
            local,
            trace: TracePolicy::default(),
        })
    }

    /// Sends a message and returns the reply decoded into `R` (or the
    /// error), as `ServiceClient::send` does.
    pub fn send<P, R>(&self, pattern: &str, payload: &P) -> SendBuilder<R>
    where
        P: Serialize + ?Sized,
    {
        self.client().send(pattern, payload)
    }

    /// Sends a message and returns the reply for assertions:
    /// `assert_status`, `assert_error`, `assert_validation_error`,
    /// `assert_json_path` and `json::<T>()`. A call that gets no reply is
    /// the error it would become in a handler (504, 503).
    pub async fn request<P>(&self, pattern: &str, payload: &P) -> TestResponse
    where
        P: Serialize + ?Sized,
    {
        let request: SendBuilder<serde_json::Value> = self.client().send(pattern, payload);
        let (status, content_type, body, request_id) = match request.exchange().await {
            Ok(reply) => (
                reply.status().unwrap_or(200),
                reply.content_type().to_owned(),
                reply.payload().clone(),
                reply.header(headers::REQUEST_ID).map(str::to_owned),
            ),
            Err(error) => {
                let error = error.to_app_error();
                (
                    error.status.as_u16(),
                    "application/json".to_owned(),
                    serde_json::to_vec(&error.body()).unwrap_or_default().into(),
                    None,
                )
            }
        };
        let mut response_headers = HeaderMap::new();
        if let Ok(value) = HeaderValue::from_str(&content_type) {
            response_headers.insert(CONTENT_TYPE, value);
        }
        // The reply's request id, as an HTTP response carries it.
        let request_id = request_id.unwrap_or_else(|| RequestId::generate().as_str().to_owned());
        if let Ok(value) = HeaderValue::from_str(&request_id) {
            response_headers.insert(headers::REQUEST_ID, value);
        }
        let headers = response_headers;
        TestResponse::from_parts(
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            headers,
            body,
        )
    }

    /// Delivers an event to the app's handlers through the whole pipeline;
    /// `.await` returns once every handler has handled it or dead-lettered
    /// it. Builder methods: `.key(..)`, `.header(..)` and
    /// `.redelivered()`.
    pub fn emit<P>(&self, pattern: &str, event: &P) -> FakeEmit
    where
        P: Serialize + ?Sized,
    {
        FakeEmit {
            builder: self.client().emit(pattern, event),
            pattern: pattern.to_owned(),
            redelivered: false,
            client: self.clone(),
        }
    }
}

impl fmt::Debug for FakeClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeClient")
            .field("principal", &self.principal.as_ref().map(|p| p.user_id))
            .finish_non_exhaustive()
    }
}

/// An event the fake client delivers ([`FakeClient::emit`]).
#[must_use = "the event is not delivered until it is awaited"]
pub struct FakeEmit {
    builder: EmitBuilder,
    pattern: String,
    redelivered: bool,
    client: FakeClient,
}

impl FakeEmit {
    /// The ordering key.
    pub fn key(mut self, key: impl fmt::Display) -> Self {
        self.builder = self.builder.key(key);
        self
    }

    /// Adds a header.
    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.builder = self.builder.header(name, value);
        self
    }

    /// Sends it with the message id of the previous event a client of this
    /// fake emitted under the same pattern, as a broker does after a crash.
    pub fn redelivered(mut self) -> Self {
        self.redelivered = true;
        self
    }
}

impl fmt::Debug for FakeEmit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeEmit")
            .field("pattern", &self.pattern)
            .field("redelivered", &self.redelivered)
            .finish_non_exhaustive()
    }
}

impl IntoFuture for FakeEmit {
    type Output = Result<(), MicroError>;
    type IntoFuture = BoxFuture<'static, Result<(), MicroError>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let mut builder = self.builder;
            if self.redelivered {
                let previous = lock(&self.client.fake.inner.last_ids)
                    .get(&self.pattern)
                    .cloned();
                match previous {
                    Some(id) => builder = builder.message_id(id),
                    None => {
                        return Err(MicroError::transport(format!(
                            "redelivered: no client of this fake has emitted `{}` before",
                            self.pattern
                        )));
                    }
                }
            }
            let (envelope, _, _) = builder.into_envelope()?;
            if let Some(id) = envelope.message_id() {
                lock(&self.client.fake.inner.last_ids).insert(self.pattern.clone(), id.to_owned());
            }
            self.client
                .fake
                .inner
                .broker
                .publish_and_wait(envelope)
                .await
        })
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn sequences_answer_in_turn_then_repeat_the_last() {
        let mut stub = Stub {
            reply: Reply::sequence([
                Reply::timeout(),
                Reply::json(&json!(1)),
                Reply::json(&json!(2)),
            ]),
            turn: 0,
        };
        let envelope = Envelope::new("a.b", "{}");
        assert!(stub.answer(&envelope).is_none());
        assert_eq!(stub.answer(&envelope).unwrap().body.as_ref(), b"1");
        assert_eq!(stub.answer(&envelope).unwrap().body.as_ref(), b"2");
        assert_eq!(stub.answer(&envelope).unwrap().body.as_ref(), b"2");
    }

    #[test]
    fn from_fn_decodes_the_payload() {
        let reply = Reply::from_fn(|input: Value| -> Result<Value, AppError> {
            if input["ok"] == true {
                Ok(json!({ "echo": input }))
            } else {
                Err(AppError::conflict("no"))
            }
        });
        let mut stub = Stub { reply, turn: 0 };
        let ok = stub
            .answer(&Envelope::new("a.b", r#"{"ok":true}"#))
            .unwrap();
        assert_eq!(ok.status, 200);
        let refused = stub
            .answer(&Envelope::new("a.b", r#"{"ok":false}"#))
            .unwrap();
        assert_eq!(refused.status, 409);
        let garbage = stub.answer(&Envelope::new("a.b", "not json")).unwrap();
        assert_eq!(garbage.status, 400);
    }
}
