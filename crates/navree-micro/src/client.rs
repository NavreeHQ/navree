//! [`ServiceClient`]: `send` (request-response) and `emit` (events).

use std::fmt;
use std::future::IntoFuture;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use http::StatusCode;
use navree_contracts::RequestId;
use navree_db::Uow;
use navree_kernel::BoxFuture;
use navree_telemetry::{TelemetryContext, TraceContext, TracePolicy};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use uuid::Uuid;

use crate::context::CallContext;
use crate::envelope::{LocalContext, headers, now_millis};
use crate::store::Store;
use crate::{Codec, Envelope, MicroError, MicroTransport, RemoteError, check_pattern};

/// The first pause between retries of `send(..).retries(n)`; it doubles.
const FIRST_RETRY_PAUSE: Duration = Duration::from_millis(100);

/// Sends messages and publishes events: inject `Arc<ServiceClient>`.
///
/// The unnamed client uses the app's transport; named clients
/// (`micro.clients.<name>`) use their own and only send.
///
/// ```ignore
/// let deal: DealView = self.client.send("deals.get", &GetDeal { id }).await?;
/// self.client.emit("deal.won", &won).key(won.deal_id).await?;
/// ```
#[derive(Clone)]
pub struct ServiceClient {
    core: Arc<ClientCore>,
}

/// What a client needs to send.
pub(crate) struct ClientCore {
    pub(crate) transport: Arc<dyn MicroTransport>,
    pub(crate) service: String,
    pub(crate) codec: Codec,
    pub(crate) request_timeout: Duration,
    pub(crate) max_message_bytes: usize,
    pub(crate) forward_auth: bool,
    /// The outbox, when `micro.outbox` is on and the app has a database.
    pub(crate) outbox: Option<Outbox>,
    /// In-process extras for every message (`MicroFake` clients).
    pub(crate) local: Option<Arc<LocalContext>>,
    /// How a message sent outside any trace starts one
    /// (`observe.tracing`).
    pub(crate) trace: TracePolicy,
}

impl ServiceClient {
    pub(crate) fn new(core: ClientCore) -> Self {
        Self {
            core: Arc::new(core),
        }
    }

    /// The sender's service name (`navree-service`).
    pub fn service(&self) -> &str {
        &self.core.service
    }

    /// The transport this client sends through.
    pub fn transport(&self) -> &Arc<dyn MicroTransport> {
        &self.core.transport
    }

    /// Sends `payload` to `pattern` and waits for the reply, decoded into
    /// `R` (inferred from the binding). The builder changes the call
    /// before `.await`:
    ///
    /// | Method | Effect |
    /// |---|---|
    /// | `.timeout(duration)` | The deadline of this call, instead of `micro.request_timeout_ms`. |
    /// | `.retries(n)` | Retry up to `n` times on a timeout, a transport failure or no responders. |
    /// | `.bearer(token)` | Send these credentials instead of the forwarded ones. |
    /// | `.without_auth()` | Send no credentials. |
    /// | `.header(name, value)` | Add a header. |
    ///
    /// `&()` sends an empty payload. Ask for `serde_json::Value` when you
    /// don't want a typed reply.
    pub fn send<P, R>(&self, pattern: &str, payload: &P) -> SendBuilder<R>
    where
        P: Serialize + ?Sized,
    {
        SendBuilder {
            core: self.core.clone(),
            pattern: pattern.to_owned(),
            payload: encode(&self.core, pattern, payload),
            timeout: None,
            retries: 0,
            credentials: Credentials::Forwarded,
            headers: Vec::new(),
            _reply: PhantomData,
        }
    }

    /// Publishes `event` under `pattern`. Returns once the event is safe:
    /// stored by the broker or, inside a unit of work with the outbox on,
    /// written to the outbox in the same transaction. The builder adds
    /// `.key(..)` (the ordering key), `.header(name, value)` and
    /// `.immediately()` (publish now even inside a unit of work).
    pub fn emit<P>(&self, pattern: &str, event: &P) -> EmitBuilder
    where
        P: Serialize + ?Sized,
    {
        EmitBuilder {
            core: self.core.clone(),
            pattern: pattern.to_owned(),
            payload: encode(&self.core, pattern, event),
            key: None,
            headers: Vec::new(),
            immediately: false,
            message_id: None,
        }
    }
}

impl fmt::Debug for ServiceClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceClient")
            .field("service", &self.core.service)
            .field("transport", &self.core.transport.name())
            .field("codec", &self.core.codec)
            .field("request_timeout", &self.core.request_timeout)
            .finish_non_exhaustive()
    }
}

/// Encodes a payload, checking the pattern and the size limit.
fn encode<P: Serialize + ?Sized>(
    core: &ClientCore,
    pattern: &str,
    payload: &P,
) -> Result<Bytes, MicroError> {
    check_pattern(pattern, false).map_err(|reason| MicroError::InvalidPattern {
        pattern: pattern.to_owned(),
        reason,
    })?;
    let bytes = core
        .codec
        .encode(payload)
        .map_err(|source| MicroError::Encode {
            pattern: pattern.to_owned(),
            source,
        })?;
    if bytes.len() > core.max_message_bytes {
        return Err(MicroError::PayloadTooLarge {
            size: bytes.len(),
            limit: core.max_message_bytes,
        });
    }
    Ok(bytes)
}

/// The headers every message carries: id, service, content type, request
/// id, trace context and workspace.
fn base_envelope(
    core: &ClientCore,
    pattern: &str,
    payload: Bytes,
    message_id: &str,
    call: Option<&CallContext>,
) -> Envelope {
    let mut envelope = Envelope::new(pattern.to_owned(), payload);
    envelope.local.clone_from(&core.local);
    envelope.set_header(headers::MESSAGE_ID, message_id);
    envelope.set_header(headers::SERVICE, core.service.clone());
    envelope.set_header(headers::CONTENT_TYPE, core.codec.content_type());
    let request_id = call
        .and_then(|call| call.request_id.clone())
        .unwrap_or_else(|| RequestId::generate().as_str().to_owned());
    envelope.set_header(headers::REQUEST_ID, request_id);
    if let Some(span) = message_span(core, call) {
        envelope.set_header(headers::TRACEPARENT, span.traceparent());
        if let Some(state) = &span.trace_state {
            envelope.set_header(headers::TRACESTATE, state.clone());
        }
    }
    if let Some(workspace) = call.and_then(|call| call.workspace.as_deref()) {
        envelope.set_header(headers::WORKSPACE, workspace);
    }
    envelope
}

/// The span a message is sent in: a child of the current
/// `TelemetryContext`'s span (the HTTP request, job or message being
/// handled), else of the call context's, else a new trace (sampled as
/// `observe.tracing.sample_ratio` says); none when tracing is off.
fn message_span(core: &ClientCore, call: Option<&CallContext>) -> Option<TraceContext> {
    let parent = TelemetryContext::with_current(|context| context.trace().cloned())
        .flatten()
        .or_else(|| call.and_then(|call| call.trace.clone()));
    match parent {
        Some(parent) => Some(parent.child()),
        None => core
            .trace
            .enabled
            .then(|| TraceContext::root_sampled(core.trace.sample_ratio)),
    }
}

/// Which credentials a request carries.
#[derive(Clone, Debug)]
enum Credentials {
    Forwarded,
    Bearer(String),
    None,
}

/// A request being built: `.await` sends it. See [`ServiceClient::send`].
#[must_use = "a request does nothing until it is awaited"]
pub struct SendBuilder<R> {
    core: Arc<ClientCore>,
    pattern: String,
    payload: Result<Bytes, MicroError>,
    timeout: Option<Duration>,
    retries: u32,
    credentials: Credentials,
    headers: Vec<(String, String)>,
    _reply: PhantomData<fn() -> R>,
}

impl<R> SendBuilder<R> {
    /// The deadline of this call, instead of `micro.request_timeout_ms`.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Retry up to `n` times on a timeout, a transport failure or no
    /// responders (never on a reply), after a pause of 100 ms that doubles;
    /// every attempt gets the full timeout and the same message id.
    pub fn retries(mut self, n: u32) -> Self {
        self.retries = n;
        self
    }

    /// Send these credentials (`Authorization: Bearer <token>`) instead of
    /// the forwarded ones.
    pub fn bearer(mut self, token: impl Into<String>) -> Self {
        self.credentials = Credentials::Bearer(token.into());
        self
    }

    /// Send no credentials.
    pub fn without_auth(mut self) -> Self {
        self.credentials = Credentials::None;
        self
    }

    /// Adds a header.
    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_ascii_lowercase(), value.into()));
        self
    }

    /// Sends the request and returns the raw reply envelope (any status).
    pub(crate) async fn exchange(self) -> Result<Envelope, MicroError> {
        let payload = self.payload?;
        let core = &self.core;
        let call = CallContext::current();
        let message_id = Uuid::now_v7().to_string();
        let timeout = self.timeout.unwrap_or(core.request_timeout);
        let mut pause = FIRST_RETRY_PAUSE;
        let mut attempt = 0;
        loop {
            let now = SystemTime::now();
            let mut deadline = now + timeout;
            if let Some(inherited) = call.as_ref().and_then(|call| call.deadline) {
                deadline = deadline.min(inherited);
            }
            let left = deadline.duration_since(now).unwrap_or(Duration::ZERO);
            if left.is_zero() {
                return Err(MicroError::Timeout {
                    pattern: self.pattern.clone(),
                    after: Duration::ZERO,
                });
            }
            let mut envelope = base_envelope(
                core,
                &self.pattern,
                payload.clone(),
                &message_id,
                call.as_ref(),
            );
            envelope.set_deadline(deadline);
            if attempt > 0 {
                envelope.set_header(headers::ATTEMPT, (attempt + 1).to_string());
            }
            let authorization = match &self.credentials {
                Credentials::Forwarded if core.forward_auth => {
                    call.as_ref().and_then(|call| call.authorization.clone())
                }
                Credentials::Forwarded | Credentials::None => None,
                Credentials::Bearer(token) => Some(format!("Bearer {token}")),
            };
            if let Some(authorization) = authorization {
                envelope.set_header(headers::AUTHORIZATION, authorization);
            }
            for (name, value) in &self.headers {
                envelope.set_header(name, value.clone());
            }
            let outcome = match tokio::time::timeout(left, core.transport.request(envelope)).await {
                Ok(outcome) => outcome,
                Err(_) => Err(MicroError::Timeout {
                    pattern: self.pattern.clone(),
                    after: left,
                }),
            };
            match outcome {
                Err(error) if error.is_retryable() && attempt < self.retries => {
                    tracing::debug!(
                        target: "navree::micro",
                        pattern = %self.pattern,
                        attempt = attempt + 1,
                        %error,
                        "retrying a request"
                    );
                    tokio::time::sleep(pause).await;
                    pause = pause.saturating_mul(2);
                    attempt += 1;
                }
                outcome => return outcome,
            }
        }
    }
}

impl<R> fmt::Debug for SendBuilder<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SendBuilder")
            .field("pattern", &self.pattern)
            .field("timeout", &self.timeout)
            .field("retries", &self.retries)
            .finish_non_exhaustive()
    }
}

impl<R> IntoFuture for SendBuilder<R>
where
    R: DeserializeOwned + Send + 'static,
{
    type Output = Result<R, MicroError>;
    type IntoFuture = BoxFuture<'static, Result<R, MicroError>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let pattern = self.pattern.clone();
            let reply = self.exchange().await?;
            decode_reply(&pattern, &reply)
        })
    }
}

/// The reply's value, or its error.
pub(crate) fn decode_reply<R: DeserializeOwned>(
    pattern: &str,
    reply: &Envelope,
) -> Result<R, MicroError> {
    let status = reply
        .status()
        .and_then(|status| StatusCode::from_u16(status).ok())
        .unwrap_or(StatusCode::OK);
    if !status.is_success() {
        let body = serde_json::from_slice::<Value>(reply.payload()).unwrap_or(Value::Null);
        return Err(MicroError::Remote(RemoteError::new(status, body)));
    }
    let codec =
        Codec::from_content_type(reply.content_type()).ok_or_else(|| MicroError::Decode {
            pattern: pattern.to_owned(),
            source: format!("unknown content type `{}`", reply.content_type()).into(),
        })?;
    // An empty body is `null` (a handler that returned `()`).
    let bytes: &[u8] = match (reply.payload().is_empty(), codec) {
        (false, _) => reply.payload(),
        (true, Codec::Json) => b"null",
        (true, Codec::Msgpack) => &[0xc0],
    };
    codec.decode(bytes).map_err(|source| MicroError::Decode {
        pattern: pattern.to_owned(),
        source,
    })
}

/// An event being built: `.await` publishes it. See [`ServiceClient::emit`].
#[must_use = "an event is not published until it is awaited"]
pub struct EmitBuilder {
    core: Arc<ClientCore>,
    pattern: String,
    payload: Result<Bytes, MicroError>,
    key: Option<String>,
    headers: Vec<(String, String)>,
    immediately: bool,
    message_id: Option<String>,
}

impl EmitBuilder {
    /// The ordering key: events with the same key keep their order.
    pub fn key(mut self, key: impl fmt::Display) -> Self {
        self.key = Some(key.to_string());
        self
    }

    /// Adds a header.
    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_ascii_lowercase(), value.into()));
        self
    }

    /// Publishes at once, even inside a unit of work (the event goes out
    /// even if the transaction rolls back).
    pub fn immediately(mut self) -> Self {
        self.immediately = true;
        self
    }

    /// Uses `id` as the message id (the `MicroFake` client's
    /// `redelivered`).
    pub(crate) fn message_id(mut self, id: String) -> Self {
        self.message_id = Some(id);
        self
    }

    /// The envelope this event is published as, with the client it goes
    /// through and whether it must skip the outbox.
    pub(crate) fn into_envelope(self) -> Result<(Envelope, Arc<ClientCore>, bool), MicroError> {
        let payload = self.payload?;
        let call = CallContext::current();
        let message_id = self
            .message_id
            .unwrap_or_else(|| Uuid::now_v7().to_string());
        let mut envelope = base_envelope(
            &self.core,
            &self.pattern,
            payload,
            &message_id,
            call.as_ref(),
        );
        let actor = call
            .as_ref()
            .and_then(|call| call.actor)
            .or_else(|| Uow::current().and_then(|uow| uow.actor()));
        if let Some(actor) = actor {
            envelope.set_header(headers::ACTOR, actor.to_string());
        }
        if let Some(key) = &self.key {
            envelope.set_header(headers::KEY, key.clone());
        }
        for (name, value) in &self.headers {
            envelope.set_header(name, value.clone());
        }
        Ok((envelope, self.core, self.immediately))
    }

    async fn publish(self) -> Result<(), MicroError> {
        let (envelope, core, immediately) = self.into_envelope()?;
        if !immediately
            && let Some(outbox) = &core.outbox
            && let Some(uow) = Uow::current()
            && !uow.is_finished().await
        {
            return outbox.stage(&uow, envelope, core.transport.clone()).await;
        }
        core.transport.publish(envelope).await
    }
}

impl fmt::Debug for EmitBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmitBuilder")
            .field("pattern", &self.pattern)
            .field("key", &self.key)
            .field("immediately", &self.immediately)
            .finish_non_exhaustive()
    }
}

impl IntoFuture for EmitBuilder {
    type Output = Result<(), MicroError>;
    type IntoFuture = BoxFuture<'static, Result<(), MicroError>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.publish())
    }
}

/// The transactional outbox: events emitted inside a unit of work are
/// written to `micro_outbox` in its transaction and published right after
/// it commits.
#[derive(Clone)]
pub(crate) struct Outbox {
    pub(crate) store: Store,
    seq: Arc<AtomicI64>,
}

impl Outbox {
    pub(crate) fn new(store: Store) -> Self {
        Self {
            store,
            seq: Arc::new(AtomicI64::new(now_millis().saturating_mul(1_000))),
        }
    }

    /// Writes `envelope` in `uow`'s transaction; publishes and deletes it
    /// once `uow` commits.
    async fn stage(
        &self,
        uow: &Uow,
        envelope: Envelope,
        transport: Arc<dyn MicroTransport>,
    ) -> Result<(), MicroError> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let id = self
            .store
            .outbox_insert(uow, &envelope, seq)
            .await
            .map_err(|error| MicroError::transport(format!("the outbox write failed: {error}")))?;
        let store = self.store.clone();
        uow.after_commit(move || {
            Box::pin(async move {
                match transport.publish(envelope).await {
                    Ok(()) => {
                        if let Err(error) = store.outbox_delete(&id).await {
                            tracing::warn!(target: "navree::micro", %error, outbox_id = %id, "could not delete a published outbox row");
                        }
                    }
                    Err(error) => {
                        tracing::warn!(target: "navree::micro", %error, outbox_id = %id, "publishing from the outbox failed; the relay will retry");
                    }
                }
            })
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn reply(status: u16, content_type: &str, body: &[u8]) -> Envelope {
        Envelope::new("deals.get", body.to_vec()).reply(status, content_type, body.to_vec())
    }

    #[test]
    fn replies_decode_by_content_type_and_status() {
        let ok: Value =
            decode_reply("deals.get", &reply(200, "application/json", br#"{"id":1}"#)).unwrap();
        assert_eq!(ok, json!({ "id": 1 }));
        let packed = Codec::Msgpack.encode(&json!({ "id": 2 })).unwrap();
        let ok: Value =
            decode_reply("deals.get", &reply(200, "application/msgpack", &packed)).unwrap();
        assert_eq!(ok["id"], 2);
        let unit: () = decode_reply("deals.get", &reply(200, "application/json", b"")).unwrap();
        assert_eq!(unit, ());
        let missing = decode_reply::<Value>(
            "deals.get",
            &reply(
                404,
                "application/json",
                br#"{"statusCode":404,"message":"Not Found","error":"Not Found"}"#,
            ),
        )
        .unwrap_err();
        match missing {
            MicroError::Remote(error) => assert_eq!(error.status(), StatusCode::NOT_FOUND),
            other => panic!("{other}"),
        }
        let wrong = decode_reply::<u64>("deals.get", &reply(200, "application/json", b"\"x\""))
            .unwrap_err();
        assert!(matches!(wrong, MicroError::Decode { .. }), "{wrong}");
    }
}
