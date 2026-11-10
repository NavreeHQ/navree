//! [`Envelope`]: what every transport carries, and the [`headers`] Navree
//! puts on it.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::{BufMut, Bytes, BytesMut};
use navree_contracts::Principal;
use serde::{Deserialize, Serialize};

use crate::MicroError;

/// The header names Navree sets on messages, events and replies. Transports
/// carry them as the broker's native headers (NATS headers, Kafka record
/// headers, AMQP properties, Redis stream fields, MQTT 5 user properties);
/// TCP frames carry them in the envelope. Names are lower case.
pub mod headers {
    /// The pattern, e.g. `deals.get`.
    pub const PATTERN: &str = "navree-pattern";
    /// A UUID v7; the same across retries and redeliveries.
    pub const MESSAGE_ID: &str = "navree-message-id";
    /// The sender's `micro.service`.
    pub const SERVICE: &str = "navree-service";
    /// `application/json` or `application/msgpack`.
    pub const CONTENT_TYPE: &str = "content-type";
    /// The request id of the HTTP request, message or job that sent it.
    pub const REQUEST_ID: &str = "x-request-id";
    /// W3C trace context.
    pub const TRACEPARENT: &str = "traceparent";
    /// W3C trace state.
    pub const TRACESTATE: &str = "tracestate";
    /// Requests: when the caller stops waiting, in Unix milliseconds.
    pub const DEADLINE: &str = "navree-deadline";
    /// Requests: the forwarded `Bearer` credentials.
    pub const AUTHORIZATION: &str = "authorization";
    /// The current workspace, when there is one.
    pub const WORKSPACE: &str = "x-workspace";
    /// Events: the id of the user whose action caused the event.
    pub const ACTOR: &str = "navree-actor";
    /// Replies: `200` for a result, the error's status otherwise.
    pub const STATUS: &str = "navree-status";
    /// Events: the ordering key set with `emit(..).key(..)`.
    pub const KEY: &str = "navree-key";
    /// The delivery attempt, starting at 1, for transports that requeue a
    /// retried message themselves.
    pub const ATTEMPT: &str = "navree-attempt";
    /// Requests: where the reply goes, for transports that route replies
    /// through the broker with headers (Redis Streams, Kafka).
    pub const REPLY_TO: &str = "navree-reply-to";
    /// Requests and replies: pairs a reply with its request on transports
    /// that route replies through the broker with headers (Redis Streams,
    /// Kafka).
    pub const CORRELATION_ID: &str = "navree-correlation-id";
}

/// The content type of JSON payloads (the default codec).
pub(crate) const JSON: &str = "application/json";

/// What travels in-process only, never over the wire: the principal a
/// `MicroFake` client acts as.
#[derive(Clone, Debug, Default)]
pub(crate) struct LocalContext {
    pub(crate) principal: Option<Principal>,
}

/// One message as a transport carries it: a pattern, headers and an
/// encoded payload.
///
/// ```
/// use navree_micro::{Envelope, headers};
///
/// let envelope = Envelope::new("deals.get", r#"{"id":1}"#)
///     .with_header(headers::MESSAGE_ID, "0199c1d2-6f7e-7a31-b5d4-2f0c8e9a1b23");
/// assert_eq!(envelope.pattern(), "deals.get");
/// assert_eq!(envelope.header("Navree-Message-Id"), envelope.message_id());
///
/// // Transports without native headers send the envelope as bytes.
/// let back = Envelope::from_bytes(envelope.to_bytes()).unwrap();
/// assert_eq!(back.headers(), envelope.headers());
/// assert_eq!(back.payload(), envelope.payload());
/// ```
#[derive(Clone)]
pub struct Envelope {
    pattern: String,
    headers: BTreeMap<String, String>,
    payload: Bytes,
    pub(crate) local: Option<Arc<LocalContext>>,
}

impl Envelope {
    /// An envelope for `pattern` with an encoded payload; sets the
    /// `navree-pattern` header.
    pub fn new(pattern: impl Into<String>, payload: impl Into<Bytes>) -> Self {
        let pattern = pattern.into();
        let mut headers = BTreeMap::new();
        headers.insert(headers::PATTERN.to_owned(), pattern.clone());
        Self {
            pattern,
            headers,
            payload: payload.into(),
            local: None,
        }
    }

    /// The pattern.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// The encoded payload.
    pub fn payload(&self) -> &Bytes {
        &self.payload
    }

    /// Replaces the payload.
    pub fn set_payload(&mut self, payload: impl Into<Bytes>) {
        self.payload = payload.into();
    }

    /// Every header, names in lower case.
    pub fn headers(&self) -> &BTreeMap<String, String> {
        &self.headers
    }

    /// The header `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        match self.headers.get(name) {
            Some(value) => Some(value),
            None => self
                .headers
                .get(&name.to_ascii_lowercase())
                .map(String::as_str),
        }
    }

    /// Sets header `name` (stored in lower case).
    pub fn set_header(&mut self, name: &str, value: impl Into<String>) {
        self.headers.insert(name.to_ascii_lowercase(), value.into());
    }

    /// Sets header `name` and returns the envelope.
    pub fn with_header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.set_header(name, value);
        self
    }

    /// Removes header `name`, returning its value.
    pub fn remove_header(&mut self, name: &str) -> Option<String> {
        self.headers.remove(&name.to_ascii_lowercase())
    }

    /// `navree-message-id`.
    pub fn message_id(&self) -> Option<&str> {
        self.header(headers::MESSAGE_ID)
    }

    /// `navree-service`: the sender.
    pub fn service(&self) -> Option<&str> {
        self.header(headers::SERVICE)
    }

    /// `content-type`, `application/json` when absent.
    pub fn content_type(&self) -> &str {
        self.header(headers::CONTENT_TYPE).unwrap_or(JSON)
    }

    /// `navree-key`: the ordering key of an event.
    pub fn key(&self) -> Option<&str> {
        self.header(headers::KEY)
    }

    /// `navree-attempt`, 1 when absent.
    pub fn attempt(&self) -> u32 {
        self.header(headers::ATTEMPT)
            .and_then(|value| value.parse().ok())
            .filter(|attempt| *attempt > 0)
            .unwrap_or(1)
    }

    /// `navree-status` of a reply.
    pub fn status(&self) -> Option<u16> {
        self.header(headers::STATUS)
            .and_then(|value| value.parse().ok())
    }

    /// `navree-deadline`: when the caller stops waiting.
    pub fn deadline(&self) -> Option<SystemTime> {
        self.header(headers::DEADLINE)
            .and_then(|value| value.parse::<u64>().ok())
            .map(|millis| UNIX_EPOCH + Duration::from_millis(millis))
    }

    /// Sets `navree-deadline`.
    pub fn set_deadline(&mut self, deadline: SystemTime) {
        self.set_header(headers::DEADLINE, unix_millis(deadline).to_string());
    }

    /// The time left until the deadline (zero once it has passed); `None`
    /// without a deadline.
    pub fn remaining(&self) -> Option<Duration> {
        self.deadline().map(|deadline| {
            deadline
                .duration_since(SystemTime::now())
                .unwrap_or(Duration::ZERO)
        })
    }

    /// Whether the deadline has passed.
    pub fn is_expired(&self) -> bool {
        self.remaining().is_some_and(|left| left.is_zero())
    }

    /// The reply to this request: `status`, a payload of `content_type`,
    /// and the request's message id, request id, trace context and
    /// correlation id.
    pub fn reply(&self, status: u16, content_type: &str, payload: impl Into<Bytes>) -> Envelope {
        let mut reply = Envelope::new(self.pattern.clone(), payload);
        for name in [
            headers::MESSAGE_ID,
            headers::REQUEST_ID,
            headers::TRACEPARENT,
            headers::TRACESTATE,
            headers::CORRELATION_ID,
        ] {
            if let Some(value) = self.header(name) {
                reply.set_header(name, value);
            }
        }
        reply.set_header(headers::STATUS, status.to_string());
        reply.set_header(headers::CONTENT_TYPE, content_type);
        reply
    }

    /// The envelope as bytes, for transports without native headers:
    /// a 4-byte big-endian length, the JSON of `{"pattern", "headers"}`,
    /// then the payload.
    pub fn to_bytes(&self) -> Bytes {
        let head = serde_json::to_vec(&Head {
            pattern: self.pattern.clone(),
            headers: self.headers.clone(),
        })
        .unwrap_or_default();
        let mut out = BytesMut::with_capacity(4 + head.len() + self.payload.len());
        out.put_u32(u32::try_from(head.len()).unwrap_or(u32::MAX));
        out.put_slice(&head);
        out.put_slice(&self.payload);
        out.freeze()
    }

    /// Reads an envelope written by [`to_bytes`](Envelope::to_bytes).
    pub fn from_bytes(bytes: impl Into<Bytes>) -> Result<Envelope, MicroError> {
        let bytes: Bytes = bytes.into();
        let malformed = || MicroError::Transport("a malformed envelope was received".into());
        if bytes.len() < 4 {
            return Err(malformed());
        }
        let length = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let end = 4usize.checked_add(length).ok_or_else(malformed)?;
        if bytes.len() < end {
            return Err(malformed());
        }
        let head: Head = serde_json::from_slice(&bytes[4..end]).map_err(|_| malformed())?;
        let mut envelope = Envelope::new(head.pattern, bytes.slice(end..));
        for (name, value) in head.headers {
            envelope.set_header(&name, value);
        }
        Ok(envelope)
    }

    /// The principal a `MicroFake` client acts as (in-process only).
    pub(crate) fn local_principal(&self) -> Option<&Principal> {
        self.local
            .as_ref()
            .and_then(|local| local.principal.as_ref())
    }
}

/// The header block of [`Envelope::to_bytes`].
#[derive(Serialize, Deserialize)]
struct Head {
    pattern: String,
    headers: BTreeMap<String, String>,
}

impl fmt::Debug for Envelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: BTreeMap<&str, &str> = self
            .headers
            .iter()
            .map(|(name, value)| {
                let shown = if name == headers::AUTHORIZATION {
                    "[redacted]"
                } else {
                    value.as_str()
                };
                (name.as_str(), shown)
            })
            .collect();
        f.debug_struct("Envelope")
            .field("pattern", &self.pattern)
            .field("headers", &headers)
            .field("payload_bytes", &self.payload.len())
            .finish_non_exhaustive()
    }
}

/// `time` in Unix milliseconds (0 before the epoch).
pub(crate) fn unix_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Now in Unix milliseconds.
pub(crate) fn now_millis() -> i64 {
    i64::try_from(unix_millis(SystemTime::now())).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_are_lower_case_and_read_in_any_case() {
        let mut envelope = Envelope::new("deals.get", "{}");
        envelope.set_header("X-Request-Id", "abc");
        assert_eq!(envelope.header("x-request-id"), Some("abc"));
        assert_eq!(envelope.header("X-REQUEST-ID"), Some("abc"));
        assert_eq!(envelope.header(headers::PATTERN), Some("deals.get"));
        assert_eq!(
            envelope.remove_header("X-Request-ID").as_deref(),
            Some("abc")
        );
        assert_eq!(envelope.header("x-request-id"), None);
    }

    #[test]
    fn defaults_for_missing_headers() {
        let envelope = Envelope::new("deal.won", "{}");
        assert_eq!(envelope.content_type(), JSON);
        assert_eq!(envelope.attempt(), 1);
        assert_eq!(envelope.status(), None);
        assert_eq!(envelope.deadline(), None);
        assert_eq!(envelope.remaining(), None);
        assert!(!envelope.is_expired());
        let bad = envelope.clone().with_header(headers::ATTEMPT, "0");
        assert_eq!(bad.attempt(), 1);
        assert_eq!(bad.with_header(headers::ATTEMPT, "3").attempt(), 3);
    }

    #[test]
    fn deadlines_round_trip_in_unix_milliseconds() {
        let mut envelope = Envelope::new("deals.get", "{}");
        let deadline = SystemTime::now() + Duration::from_secs(5);
        envelope.set_deadline(deadline);
        let millis: u64 = envelope.header(headers::DEADLINE).unwrap().parse().unwrap();
        assert_eq!(millis, unix_millis(deadline));
        let left = envelope.remaining().unwrap();
        assert!(left > Duration::from_secs(4) && left <= Duration::from_secs(5));
        envelope.set_deadline(SystemTime::now() - Duration::from_secs(1));
        assert_eq!(envelope.remaining(), Some(Duration::ZERO));
        assert!(envelope.is_expired());
    }

    #[test]
    fn replies_keep_the_ids_and_carry_the_status() {
        let request = Envelope::new("deals.get", "{}")
            .with_header(headers::MESSAGE_ID, "m1")
            .with_header(headers::REQUEST_ID, "r1")
            .with_header(headers::TRACEPARENT, "00-aa-bb-01")
            .with_header(headers::CORRELATION_ID, "c1")
            .with_header(headers::AUTHORIZATION, "Bearer secret");
        let reply = request.reply(404, JSON, "{}");
        assert_eq!(reply.status(), Some(404));
        assert_eq!(reply.message_id(), Some("m1"));
        assert_eq!(reply.header(headers::REQUEST_ID), Some("r1"));
        assert_eq!(reply.header(headers::TRACEPARENT), Some("00-aa-bb-01"));
        assert_eq!(reply.header(headers::CORRELATION_ID), Some("c1"));
        assert_eq!(reply.header(headers::AUTHORIZATION), None, "never echoed");
        assert_eq!(reply.content_type(), JSON);
    }

    #[test]
    fn bytes_round_trip_and_reject_garbage() {
        let envelope =
            Envelope::new("deal.won", vec![0u8, 159, 146, 150]).with_header(headers::KEY, "42");
        let back = Envelope::from_bytes(envelope.to_bytes()).unwrap();
        assert_eq!(back.pattern(), "deal.won");
        assert_eq!(back.key(), Some("42"));
        assert_eq!(back.payload().as_ref(), &[0u8, 159, 146, 150]);
        assert!(Envelope::from_bytes(Bytes::from_static(b"")).is_err());
        assert!(Envelope::from_bytes(Bytes::from_static(b"\0\0\0\x09{}")).is_err());
        assert!(Envelope::from_bytes(Bytes::from_static(b"\0\0\0\x02{]")).is_err());
    }

    #[test]
    fn debug_redacts_credentials() {
        let envelope =
            Envelope::new("deals.get", "{}").with_header(headers::AUTHORIZATION, "Bearer secret");
        let shown = format!("{envelope:?}");
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("[redacted]"));
    }
}
