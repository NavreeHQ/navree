//! Verifying incoming webhooks: [`WebhookVerifier`], [`WebhookSource`] and
//! the built-in verifiers [`StandardWebhooks`], [`Stripe`], [`GitHub`],
//! [`Paddle`] and [`Hmac`] (the others are in `sources`).

use std::future::Future;

use chrono::{DateTime, Utc};
use http::HeaderMap;
use serde_json::Value;

use crate::options::{HmacAlgorithm, SignatureEncoding, SourceOptions};
use crate::request::{IncomingRequest, Verdict};
use crate::signing::{self, standard_key};
use crate::template::{Field, SignatureFormat, SignedTemplate};

/// How old (or how far in the future) a signed timestamp may be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tolerance {
    now: DateTime<Utc>,
    secs: u64,
}

impl Tolerance {
    /// `secs` seconds either side of the system clock's now.
    pub fn secs(secs: u64) -> Self {
        Self::at(Utc::now(), secs)
    }

    /// `secs` seconds either side of `now` (a test clock's time).
    pub fn at(now: DateTime<Utc>, secs: u64) -> Self {
        Self { now, secs }
    }

    /// The time signatures are checked against.
    pub fn now(&self) -> DateTime<Utc> {
        self.now
    }

    /// The accepted age in seconds.
    pub fn window_secs(&self) -> u64 {
        self.secs
    }

    /// Accepts `timestamp` when it is within the window.
    pub(crate) fn check(&self, timestamp: DateTime<Utc>) -> Result<(), VerifyError> {
        let age = (self.now.timestamp() - timestamp.timestamp()).unsigned_abs();
        if age > self.secs {
            return Err(VerifyError::TimestampOutOfTolerance {
                tolerance_secs: self.secs,
            });
        }
        Ok(())
    }
}

/// What a verified request says about itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verified {
    /// The sender's delivery id (`webhook-id`, `X-GitHub-Delivery`, the
    /// Stripe event id), when it sends one.
    pub id: Option<String>,
    /// The signed timestamp, when the scheme signs one.
    pub timestamp: Option<DateTime<Utc>>,
    /// The sender's event type, when it sends one.
    pub event_type: Option<String>,
}

/// Why a request failed verification. The message never contains the
/// secret or a signature.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum VerifyError {
    /// A required header is absent.
    #[error("the `{0}` header is missing")]
    MissingHeader(String),
    /// A header does not have the expected shape.
    #[error("the `{0}` header is malformed")]
    MalformedHeader(String),
    /// No signature in the request matches the body.
    #[error("no signature matches the body")]
    SignatureMismatch,
    /// The signed timestamp is outside the tolerance window.
    #[error("the signed timestamp is more than {tolerance_secs} s away from now")]
    TimestampOutOfTolerance {
        /// The accepted age in seconds.
        tolerance_secs: u64,
    },
    /// The source has no secret.
    #[error("the source has no secret")]
    MissingSecret,
    /// The source's options cannot verify anything (a missing `header`, a
    /// malformed template).
    #[error("the source is misconfigured: {0}")]
    Misconfigured(String),
    /// The body does not have the shape the scheme signs (a missing field,
    /// JSON that does not parse).
    #[error("the body has no valid {0}")]
    MalformedBody(String),
    /// The request is signed, but not by a sender the source accepts: a
    /// topic, audience, account or URL that is not the configured one.
    #[error("the request is refused: {0}")]
    Rejected(String),
    /// A key needed to verify the request could not be fetched (the sender
    /// retries a request answered with 503).
    #[error("{0} could not be fetched")]
    Unavailable(String),
}

/// A signature scheme. The built-in ones are [`StandardWebhooks`],
/// [`Stripe`], [`GitHub`], [`Paddle`], [`AwsSns`](crate::AwsSns),
/// [`GooglePubSub`](crate::GooglePubSub),
/// [`MicrosoftGraph`](crate::MicrosoftGraph), [`Twilio`](crate::Twilio),
/// [`Slack`](crate::Slack), [`Mailgun`](crate::Mailgun),
/// [`Postmark`](crate::Postmark) and the configurable [`Hmac`]; implement
/// it for a sender with a scheme of its own.
///
/// A scheme that signs the headers and body implements
/// [`verify_request`](WebhookVerifier::verify_request). One that needs more
/// (the URL the sender called, keys fetched from the sender, a handshake
/// answered without the handler) implements
/// [`verify_incoming`](WebhookVerifier::verify_incoming) instead.
pub trait WebhookVerifier: Send + Sync + 'static {
    /// Verifies a request with the source's options (its secret, and
    /// whatever else the scheme reads), checking a signed timestamp against
    /// `tolerance` when there is one (`None` skips the age check).
    ///
    /// The default refuses every request: a verifier implements this or
    /// [`verify_incoming`](WebhookVerifier::verify_incoming).
    fn verify_request(
        source: &SourceOptions,
        headers: &HeaderMap,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        let _ = (source, headers, body, tolerance);
        Err(VerifyError::Misconfigured(
            "this verifier checks whole requests: call `verify_incoming`".to_owned(),
        ))
    }

    /// Verifies a whole request: an authentic delivery is
    /// [`Verdict::Verified`], a handshake the sender expects an answer to
    /// is [`Verdict::Handshake`]. What [`VerifiedWebhook`](crate::VerifiedWebhook)
    /// calls. The default runs
    /// [`verify_request`](WebhookVerifier::verify_request).
    fn verify_incoming(
        request: &IncomingRequest<'_>,
    ) -> impl Future<Output = Result<Verdict, VerifyError>> + Send {
        std::future::ready(
            Self::verify_request(
                request.source(),
                request.headers(),
                request.body(),
                request.tolerance(),
            )
            .map(Verdict::Verified),
        )
    }

    /// The setting the source cannot verify without, when it is not set:
    /// by default `secret`. A request for a source that lacks it is answered
    /// with 500, and the error log names the key.
    fn missing_setting(source: &SourceOptions) -> Option<&'static str> {
        source.secret.trim().is_empty().then_some("secret")
    }
}

/// A sender of incoming webhooks, for [`VerifiedWebhook<S>`](crate::VerifiedWebhook):
/// its name (its options and secret are under `webhooks.incoming.<NAME>`)
/// and its verifier.
///
/// ```
/// use navree_webhooks::{StandardWebhooks, WebhookSource};
///
/// /// Delivery events from our mail provider.
/// pub struct MailEvents;
///
/// impl WebhookSource for MailEvents {
///     /// The secret is read from `webhooks.incoming.mail_events.secret`.
///     const NAME: &'static str = "mail_events";
///     type Verifier = StandardWebhooks;
/// }
/// ```
pub trait WebhookSource: Send + Sync + 'static {
    /// The source name: lower case, the key under `webhooks.incoming`.
    const NAME: &'static str;
    /// How its requests are signed.
    type Verifier: WebhookVerifier;
}

/// The value of header `name`.
pub(crate) fn header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, VerifyError> {
    headers
        .get(name)
        .ok_or_else(|| VerifyError::MissingHeader(name.to_owned()))?
        .to_str()
        .map_err(|_| VerifyError::MalformedHeader(name.to_owned()))
}

/// The value of header `name`, if present and text.
pub(crate) fn optional_header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

/// A Unix timestamp in seconds from header `name`.
pub(crate) fn parse_timestamp(raw: &str, name: &str) -> Result<DateTime<Utc>, VerifyError> {
    raw.trim()
        .parse::<i64>()
        .ok()
        .and_then(|secs| DateTime::from_timestamp(secs, 0))
        .ok_or_else(|| VerifyError::MalformedHeader(name.to_owned()))
}

/// Checks `timestamp` against `tolerance`, when there is one.
pub(crate) fn check_age(
    timestamp: DateTime<Utc>,
    tolerance: Option<Tolerance>,
) -> Result<(), VerifyError> {
    tolerance.map_or(Ok(()), |tolerance| tolerance.check(timestamp))
}

/// Refuses an empty secret.
pub(crate) fn require_secret(secret: &str) -> Result<(), VerifyError> {
    if secret.is_empty() {
        Err(VerifyError::MissingSecret)
    } else {
        Ok(())
    }
}

/// String fields `id_field` and `type_field` of a JSON object body.
pub(crate) fn body_fields(
    body: &[u8],
    id_field: &str,
    type_field: &str,
) -> (Option<String>, Option<String>) {
    let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(body) else {
        return (None, None);
    };
    let text = |field: &str| object.get(field).and_then(Value::as_str).map(str::to_owned);
    (text(id_field), text(type_field))
}

/// Whether any candidate (decoded with `encoding`) equals `expected`.
pub(crate) fn any_matches<'a>(
    candidates: impl IntoIterator<Item = &'a str>,
    encoding: SignatureEncoding,
    expected: &[u8],
) -> bool {
    let mut matched = false;
    for candidate in candidates {
        if let Some(bytes) = signing::decode(encoding, candidate) {
            // Keep looking so the time does not depend on which one matched.
            matched |= signing::same(&bytes, expected);
        }
    }
    matched
}

/// The [Standard Webhooks](https://www.standardwebhooks.com) scheme: headers
/// `webhook-id`, `webhook-timestamp` and `webhook-signature` (`v1,<base64>`,
/// several separated by spaces), HMAC-SHA256 of `{id}.{timestamp}.{body}`
/// keyed with the secret's bytes after `whsec_`, base64-decoded. What
/// Navree's outgoing webhooks send. Source name `standard`.
#[derive(Clone, Copy, Debug, Default)]
pub struct StandardWebhooks;

impl StandardWebhooks {
    /// The message id header.
    pub const ID_HEADER: &'static str = "webhook-id";
    /// The timestamp header (Unix seconds).
    pub const TIMESTAMP_HEADER: &'static str = "webhook-timestamp";
    /// The signature header.
    pub const SIGNATURE_HEADER: &'static str = "webhook-signature";

    /// The `webhook-signature` value (`v1,<base64>`) of `body` sent as
    /// message `id` at `timestamp` (Unix seconds).
    pub fn sign(secret: &str, id: &str, timestamp: i64, body: &[u8]) -> String {
        signing::standard_signature(&standard_key(secret), id, timestamp, body)
    }

    /// Verifies a request signed with `secret`. With `tolerance`, the
    /// timestamp must be within its window (in either direction); `None`
    /// checks the signature only. Any `v1` signature in the header may
    /// match (two are sent while a secret is rotated).
    pub fn verify(
        secret: &str,
        headers: &HeaderMap,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        require_secret(secret)?;
        let id = header(headers, Self::ID_HEADER)?;
        let raw_timestamp = header(headers, Self::TIMESTAMP_HEADER)?;
        let timestamp = parse_timestamp(raw_timestamp, Self::TIMESTAMP_HEADER)?;
        let signatures = header(headers, Self::SIGNATURE_HEADER)?;
        let expected = signing::hmac_sha256(
            &standard_key(secret),
            &[
                id.as_bytes(),
                b".",
                raw_timestamp.trim().as_bytes(),
                b".",
                body,
            ],
        );
        let candidates = signatures
            .split(' ')
            .filter_map(|entry| entry.split_once(','))
            .filter(|(version, _)| *version == "v1")
            .map(|(_, signature)| signature);
        if !any_matches(candidates, SignatureEncoding::Base64, &expected) {
            return Err(VerifyError::SignatureMismatch);
        }
        check_age(timestamp, tolerance)?;
        let (_, event_type) = body_fields(body, "id", "type");
        Ok(Verified {
            id: Some(id.to_owned()),
            timestamp: Some(timestamp),
            event_type,
        })
    }
}

impl WebhookVerifier for StandardWebhooks {
    fn verify_request(
        source: &SourceOptions,
        headers: &HeaderMap,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        Self::verify(&source.secret, headers, body, tolerance)
    }
}

impl WebhookSource for StandardWebhooks {
    const NAME: &'static str = "standard";
    type Verifier = StandardWebhooks;
}

/// Stripe's scheme: `Stripe-Signature: t=<timestamp>,v1=<hex>`, HMAC-SHA256
/// of `{t}.{body}` keyed with the endpoint secret as it is (`whsec_...`).
/// The id and event type come from the event in the body (`id`, `type`).
/// Source name `stripe`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stripe;

impl Stripe {
    /// The signature header.
    pub const SIGNATURE_HEADER: &'static str = "stripe-signature";

    /// The `Stripe-Signature` value (`t=..,v1=..`) of `body` at `timestamp`.
    pub fn sign(secret: &str, timestamp: i64, body: &[u8]) -> String {
        let timestamp = timestamp.to_string();
        let mac = signing::hmac_sha256(secret.as_bytes(), &[timestamp.as_bytes(), b".", body]);
        format!("t={timestamp},v1={}", hex::encode(mac))
    }

    /// Verifies a request signed with `secret` (see
    /// [`StandardWebhooks::verify`] for `tolerance`). Any `v1` signature
    /// may match.
    pub fn verify(
        secret: &str,
        headers: &HeaderMap,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        require_secret(secret)?;
        let value = header(headers, Self::SIGNATURE_HEADER)?;
        let malformed = || VerifyError::MalformedHeader(Self::SIGNATURE_HEADER.to_owned());
        let pairs: Vec<(&str, &str)> = value
            .split(',')
            .filter_map(|item| item.trim().split_once('='))
            .collect();
        let raw_timestamp = pairs
            .iter()
            .find(|(key, _)| *key == "t")
            .map(|(_, value)| *value)
            .ok_or_else(malformed)?;
        let timestamp = parse_timestamp(raw_timestamp, Self::SIGNATURE_HEADER)?;
        let candidates: Vec<&str> = pairs
            .iter()
            .filter(|(key, _)| *key == "v1")
            .map(|(_, value)| *value)
            .collect();
        if candidates.is_empty() {
            return Err(malformed());
        }
        let expected =
            signing::hmac_sha256(secret.as_bytes(), &[raw_timestamp.as_bytes(), b".", body]);
        if !any_matches(candidates, SignatureEncoding::Hex, &expected) {
            return Err(VerifyError::SignatureMismatch);
        }
        check_age(timestamp, tolerance)?;
        let (id, event_type) = body_fields(body, "id", "type");
        Ok(Verified {
            id,
            timestamp: Some(timestamp),
            event_type,
        })
    }
}

impl WebhookVerifier for Stripe {
    fn verify_request(
        source: &SourceOptions,
        headers: &HeaderMap,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        Self::verify(&source.secret, headers, body, tolerance)
    }
}

impl WebhookSource for Stripe {
    const NAME: &'static str = "stripe";
    type Verifier = Stripe;
}

/// GitHub's scheme: `X-Hub-Signature-256: sha256=<hex>`, HMAC-SHA256 of the
/// body. GitHub signs no timestamp, so there is no age check: drop replays
/// with [`mark_processed`](crate::VerifiedWebhook::mark_processed). The id
/// is `X-GitHub-Delivery`, the event type `X-GitHub-Event`. Source name
/// `github`.
#[derive(Clone, Copy, Debug, Default)]
pub struct GitHub;

impl GitHub {
    /// The signature header.
    pub const SIGNATURE_HEADER: &'static str = "x-hub-signature-256";
    /// The delivery id header.
    pub const DELIVERY_HEADER: &'static str = "x-github-delivery";
    /// The event type header.
    pub const EVENT_HEADER: &'static str = "x-github-event";

    /// The `X-Hub-Signature-256` value (`sha256=<hex>`) of `body`.
    pub fn sign(secret: &str, body: &[u8]) -> String {
        format!(
            "sha256={}",
            hex::encode(signing::hmac_sha256(secret.as_bytes(), &[body]))
        )
    }

    /// Verifies a request signed with `secret`.
    pub fn verify(secret: &str, headers: &HeaderMap, body: &[u8]) -> Result<Verified, VerifyError> {
        require_secret(secret)?;
        let value = header(headers, Self::SIGNATURE_HEADER)?;
        let signature = value
            .trim()
            .strip_prefix("sha256=")
            .ok_or_else(|| VerifyError::MalformedHeader(Self::SIGNATURE_HEADER.to_owned()))?;
        let expected = signing::hmac_sha256(secret.as_bytes(), &[body]);
        if !any_matches([signature], SignatureEncoding::Hex, &expected) {
            return Err(VerifyError::SignatureMismatch);
        }
        Ok(Verified {
            id: optional_header(headers, Self::DELIVERY_HEADER),
            timestamp: None,
            event_type: optional_header(headers, Self::EVENT_HEADER),
        })
    }
}

impl WebhookVerifier for GitHub {
    fn verify_request(
        source: &SourceOptions,
        headers: &HeaderMap,
        body: &[u8],
        _tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        Self::verify(&source.secret, headers, body)
    }
}

impl WebhookSource for GitHub {
    const NAME: &'static str = "github";
    type Verifier = GitHub;
}

/// Paddle Billing's scheme: `Paddle-Signature: ts=<timestamp>;h1=<hex>`,
/// HMAC-SHA256 of `{ts}:{body}` keyed with the notification secret. The id
/// and event type come from the body (`event_id`, `event_type`). Source
/// name `paddle`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Paddle;

impl Paddle {
    /// The signature header.
    pub const SIGNATURE_HEADER: &'static str = "paddle-signature";

    /// The `Paddle-Signature` value (`ts=..;h1=..`) of `body` at `timestamp`.
    pub fn sign(secret: &str, timestamp: i64, body: &[u8]) -> String {
        let timestamp = timestamp.to_string();
        let mac = signing::hmac_sha256(secret.as_bytes(), &[timestamp.as_bytes(), b":", body]);
        format!("ts={timestamp};h1={}", hex::encode(mac))
    }

    /// Verifies a request signed with `secret` (see
    /// [`StandardWebhooks::verify`] for `tolerance`). Any `h1` signature
    /// may match.
    pub fn verify(
        secret: &str,
        headers: &HeaderMap,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        require_secret(secret)?;
        let value = header(headers, Self::SIGNATURE_HEADER)?;
        let malformed = || VerifyError::MalformedHeader(Self::SIGNATURE_HEADER.to_owned());
        let pairs: Vec<(&str, &str)> = value
            .split(';')
            .filter_map(|item| item.trim().split_once('='))
            .collect();
        let raw_timestamp = pairs
            .iter()
            .find(|(key, _)| *key == "ts")
            .map(|(_, value)| *value)
            .ok_or_else(malformed)?;
        let timestamp = parse_timestamp(raw_timestamp, Self::SIGNATURE_HEADER)?;
        let candidates: Vec<&str> = pairs
            .iter()
            .filter(|(key, _)| *key == "h1")
            .map(|(_, value)| *value)
            .collect();
        if candidates.is_empty() {
            return Err(malformed());
        }
        let expected =
            signing::hmac_sha256(secret.as_bytes(), &[raw_timestamp.as_bytes(), b":", body]);
        if !any_matches(candidates, SignatureEncoding::Hex, &expected) {
            return Err(VerifyError::SignatureMismatch);
        }
        check_age(timestamp, tolerance)?;
        let (id, event_type) = body_fields(body, "event_id", "event_type");
        Ok(Verified {
            id,
            timestamp: Some(timestamp),
            event_type,
        })
    }
}

impl WebhookVerifier for Paddle {
    fn verify_request(
        source: &SourceOptions,
        headers: &HeaderMap,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        Self::verify(&source.secret, headers, body, tolerance)
    }
}

impl WebhookSource for Paddle {
    const NAME: &'static str = "paddle";
    type Verifier = Paddle;
}

/// A configurable HMAC scheme for senders with a scheme of their own, set
/// up by the source's options (`webhooks.incoming.<source>`): the `header`,
/// `algorithm` (`sha1`, `sha256`, `sha512`), `encoding` (`hex`, `base64`),
/// `format` of the header value (`{signature}`, optionally `{timestamp}`)
/// and what is `signed` (`{timestamp}`, `{id}`, `{body}`), plus optional
/// `timestamp_header`, `id_header` and `event_header`.
///
/// ```
/// use navree_webhooks::{Hmac, WebhookSource};
///
/// /// Calendly's `Calendly-Webhook-Signature: t=..,v1=..`.
/// pub struct Calendly;
///
/// impl WebhookSource for Calendly {
///     const NAME: &'static str = "calendly";
///     type Verifier = Hmac;
/// }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct Hmac;

/// The parsed options of an [`Hmac`] source.
struct HmacScheme<'a> {
    header: &'a str,
    format: SignatureFormat,
    signed: SignedTemplate,
    algorithm: HmacAlgorithm,
    encoding: SignatureEncoding,
}

impl<'a> HmacScheme<'a> {
    fn of(source: &'a SourceOptions) -> Result<Self, VerifyError> {
        let header = source
            .header
            .as_deref()
            .filter(|header| !header.trim().is_empty())
            .ok_or_else(|| VerifyError::Misconfigured("`header` is not set".to_owned()))?;
        let format = SignatureFormat::parse(source.format.as_deref().unwrap_or("{signature}"))
            .map_err(|error| VerifyError::Misconfigured(format!("`format`: {error}")))?;
        let signed = SignedTemplate::parse(source.signed.as_deref().unwrap_or("{body}"))
            .map_err(|error| VerifyError::Misconfigured(format!("`signed`: {error}")))?;
        Ok(Self {
            header,
            format,
            signed,
            algorithm: source.algorithm,
            encoding: source.encoding,
        })
    }
}

impl Hmac {
    /// The signature header value `source` expects for `body` (sent at
    /// `timestamp` as delivery `id`, when its templates use them): for
    /// tests that sign requests the way the sender does.
    pub fn sign(
        source: &SourceOptions,
        timestamp: Option<i64>,
        id: Option<&str>,
        body: &[u8],
    ) -> Result<String, VerifyError> {
        require_secret(&source.secret)?;
        let scheme = HmacScheme::of(source)?;
        let timestamp = timestamp.map(|t| t.to_string()).unwrap_or_default();
        let signed = scheme
            .signed
            .render(&timestamp, id.unwrap_or_default(), body);
        let signature = signing::encode(
            scheme.encoding,
            &signing::hmac(scheme.algorithm, source.secret.as_bytes(), &[&signed]),
        );
        let format = source.format.as_deref().unwrap_or("{signature}");
        Ok(format
            .replace("{timestamp}", &timestamp)
            .replace("{signature}", &signature))
    }

    /// Verifies a request against `source`'s options (see
    /// [`StandardWebhooks::verify`] for `tolerance`).
    pub fn verify(
        source: &SourceOptions,
        headers: &HeaderMap,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        require_secret(&source.secret)?;
        let scheme = HmacScheme::of(source)?;
        let value = header(headers, scheme.header)?;
        let extracted = scheme
            .format
            .extract(value.trim())
            .ok_or_else(|| VerifyError::MalformedHeader(scheme.header.to_owned()))?;
        let raw_timestamp = match (&extracted.timestamp, &source.timestamp_header) {
            (Some(raw), _) => Some((raw.clone(), scheme.header.to_owned())),
            (None, Some(name)) => Some((header(headers, name)?.to_owned(), name.clone())),
            (None, None) => None,
        };
        if scheme.signed.uses(Field::Timestamp) && raw_timestamp.is_none() {
            return Err(VerifyError::Misconfigured(
                "`signed` uses {timestamp}, but neither `format` nor `timestamp_header` carries one"
                    .to_owned(),
            ));
        }
        let id = match &source.id_header {
            Some(name) => Some(header(headers, name)?.to_owned()),
            None => None,
        };
        if scheme.signed.uses(Field::Id) && id.is_none() {
            return Err(VerifyError::Misconfigured(
                "`signed` uses {id}, but `id_header` is not set".to_owned(),
            ));
        }
        let timestamp_text = raw_timestamp.as_ref().map_or("", |(raw, _)| raw.trim());
        let signed = scheme
            .signed
            .render(timestamp_text, id.as_deref().unwrap_or_default(), body);
        let expected = signing::hmac(scheme.algorithm, source.secret.as_bytes(), &[&signed]);
        if !any_matches([extracted.signature.as_str()], scheme.encoding, &expected) {
            return Err(VerifyError::SignatureMismatch);
        }
        let timestamp = match &raw_timestamp {
            Some((raw, name)) => {
                let timestamp = parse_timestamp(raw, name)?;
                check_age(timestamp, tolerance)?;
                Some(timestamp)
            }
            None => None,
        };
        Ok(Verified {
            id,
            timestamp,
            event_type: source
                .event_header
                .as_deref()
                .and_then(|name| optional_header(headers, name)),
        })
    }
}

impl WebhookVerifier for Hmac {
    fn verify_request(
        source: &SourceOptions,
        headers: &HeaderMap,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        Self::verify(source, headers, body, tolerance)
    }
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;

    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    const STANDARD_SECRET: &str = "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw";
    const STANDARD_BODY: &[u8] = br#"{"test": 2432232314}"#;

    fn standard_headers(signature: &str) -> HeaderMap {
        headers(&[
            ("webhook-id", "msg_p5jXN8AQM9LWM0D4loKWxJek"),
            ("webhook-timestamp", "1614265330"),
            ("webhook-signature", signature),
        ])
    }

    #[test]
    fn standard_webhooks_accepts_the_spec_vector() {
        let verified = StandardWebhooks::verify(
            STANDARD_SECRET,
            &standard_headers("v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE="),
            STANDARD_BODY,
            Some(Tolerance::at(at(1_614_265_330 + 299), 300)),
        )
        .unwrap();
        assert_eq!(verified.id.as_deref(), Some("msg_p5jXN8AQM9LWM0D4loKWxJek"));
        assert_eq!(verified.timestamp, Some(at(1_614_265_330)));
        assert_eq!(
            StandardWebhooks::sign(
                STANDARD_SECRET,
                "msg_p5jXN8AQM9LWM0D4loKWxJek",
                1_614_265_330,
                STANDARD_BODY
            ),
            "v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE="
        );
    }

    #[test]
    fn standard_webhooks_accepts_any_listed_signature() {
        let headers = standard_headers(
            "v1,Ceo5qEr07ixe2NLpvHk3FH9bwy/WavXrAFQ/9tdO6mc= v2,abc v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE=",
        );
        StandardWebhooks::verify(STANDARD_SECRET, &headers, STANDARD_BODY, None).unwrap();
    }

    #[test]
    fn standard_webhooks_refuses_tampering_old_messages_and_missing_headers() {
        let good = standard_headers("v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE=");
        assert_eq!(
            StandardWebhooks::verify(STANDARD_SECRET, &good, br#"{"test": 1}"#, None),
            Err(VerifyError::SignatureMismatch)
        );
        assert_eq!(
            StandardWebhooks::verify(
                "whsec_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                &good,
                STANDARD_BODY,
                None
            ),
            Err(VerifyError::SignatureMismatch)
        );
        assert_eq!(
            StandardWebhooks::verify(
                STANDARD_SECRET,
                &good,
                STANDARD_BODY,
                Some(Tolerance::at(at(1_614_265_330 + 301), 300))
            ),
            Err(VerifyError::TimestampOutOfTolerance {
                tolerance_secs: 300
            })
        );
        assert_eq!(
            StandardWebhooks::verify(
                STANDARD_SECRET,
                &good,
                STANDARD_BODY,
                Some(Tolerance::at(at(1_614_265_330 - 301), 300))
            ),
            Err(VerifyError::TimestampOutOfTolerance {
                tolerance_secs: 300
            })
        );
        let mut missing = good.clone();
        missing.remove("webhook-id");
        assert_eq!(
            StandardWebhooks::verify(STANDARD_SECRET, &missing, STANDARD_BODY, None),
            Err(VerifyError::MissingHeader("webhook-id".to_owned()))
        );
        assert_eq!(
            StandardWebhooks::verify("", &good, STANDARD_BODY, None),
            Err(VerifyError::MissingSecret)
        );
    }

    const STRIPE_BODY: &[u8] = br#"{"id":"evt_1NzEXAMPLE","object":"event","type":"invoice.paid"}"#;
    const STRIPE_SIGNATURE: &str =
        "5361ced4c2f3afd631cb81c0e56aa59b8d2011ee2d4297e97414fb2f31fb7c98";

    #[test]
    fn stripe_matches_the_reference_signature() {
        let value = format!("t=1792000000,v1={STRIPE_SIGNATURE},v0=deadbeef");
        let verified = Stripe::verify(
            "whsec_test_secret",
            &headers(&[("stripe-signature", &value)]),
            STRIPE_BODY,
            Some(Tolerance::at(at(1_792_000_100), 300)),
        )
        .unwrap();
        assert_eq!(verified.id.as_deref(), Some("evt_1NzEXAMPLE"));
        assert_eq!(verified.event_type.as_deref(), Some("invoice.paid"));
        assert_eq!(verified.timestamp, Some(at(1_792_000_000)));
        assert_eq!(
            Stripe::sign("whsec_test_secret", 1_792_000_000, STRIPE_BODY),
            format!("t=1792000000,v1={STRIPE_SIGNATURE}")
        );
    }

    #[test]
    fn stripe_refuses_bad_signatures_and_old_timestamps() {
        let value = format!("t=1792000000,v1={STRIPE_SIGNATURE}");
        let signed = headers(&[("stripe-signature", &value)]);
        assert_eq!(
            Stripe::verify("whsec_other", &signed, STRIPE_BODY, None),
            Err(VerifyError::SignatureMismatch)
        );
        assert_eq!(
            Stripe::verify(
                "whsec_test_secret",
                &signed,
                STRIPE_BODY,
                Some(Tolerance::at(at(1_792_000_000 + 600), 300))
            ),
            Err(VerifyError::TimestampOutOfTolerance {
                tolerance_secs: 300
            })
        );
        assert_eq!(
            Stripe::verify(
                "whsec_test_secret",
                &headers(&[("stripe-signature", "v1=abc")]),
                STRIPE_BODY,
                None
            ),
            Err(VerifyError::MalformedHeader("stripe-signature".to_owned()))
        );
    }

    #[test]
    fn github_matches_its_documented_example() {
        let signature = "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17";
        let request = headers(&[
            ("x-hub-signature-256", signature),
            ("x-github-delivery", "72d3162e-cc78-11e3-81ab-4c9367dc0958"),
            ("x-github-event", "issues"),
        ]);
        let verified =
            GitHub::verify("It's a Secret to Everybody", &request, b"Hello, World!").unwrap();
        assert_eq!(
            verified.id.as_deref(),
            Some("72d3162e-cc78-11e3-81ab-4c9367dc0958")
        );
        assert_eq!(verified.event_type.as_deref(), Some("issues"));
        assert_eq!(verified.timestamp, None);
        assert_eq!(
            GitHub::sign("It's a Secret to Everybody", b"Hello, World!"),
            signature
        );
        assert_eq!(
            GitHub::verify("It's a Secret to Everybody", &request, b"Hello, World?"),
            Err(VerifyError::SignatureMismatch)
        );
        assert_eq!(
            GitHub::verify("s", &headers(&[("x-hub-signature-256", "sha1=abc")]), b"x"),
            Err(VerifyError::MalformedHeader(
                "x-hub-signature-256".to_owned()
            ))
        );
    }

    const PADDLE_BODY: &[u8] =
        br#"{"event_id":"evt_01hv8x2acma2gz3he9jqzd4cba","event_type":"subscription.created"}"#;
    const PADDLE_SIGNATURE: &str =
        "dfbfd05e844c2673a6dccde60a6d3b543d529596a98086dc4a22d15326284f4d";

    #[test]
    fn paddle_matches_the_reference_signature() {
        let value = format!("ts=1792000000;h1={PADDLE_SIGNATURE}");
        let verified = Paddle::verify(
            "pdl_ntfset_01hv8x0testsecret",
            &headers(&[("paddle-signature", &value)]),
            PADDLE_BODY,
            Some(Tolerance::at(at(1_792_000_005), 5)),
        )
        .unwrap();
        assert_eq!(
            verified.id.as_deref(),
            Some("evt_01hv8x2acma2gz3he9jqzd4cba")
        );
        assert_eq!(verified.event_type.as_deref(), Some("subscription.created"));
        assert_eq!(
            Paddle::sign("pdl_ntfset_01hv8x0testsecret", 1_792_000_000, PADDLE_BODY),
            value
        );
        assert_eq!(
            Paddle::verify(
                "pdl_ntfset_01hv8x0testsecret",
                &headers(&[("paddle-signature", &value)]),
                PADDLE_BODY,
                Some(Tolerance::at(at(1_792_000_006), 5)),
            ),
            Err(VerifyError::TimestampOutOfTolerance { tolerance_secs: 5 })
        );
    }

    fn calendly() -> SourceOptions {
        SourceOptions {
            header: Some("Calendly-Webhook-Signature".to_owned()),
            format: Some("t={timestamp},v1={signature}".to_owned()),
            signed: Some("{timestamp}.{body}".to_owned()),
            ..SourceOptions::new("calendly-key")
        }
    }

    const CALENDLY_BODY: &[u8] = br#"{"event":"invitee.created"}"#;

    #[test]
    fn hmac_sources_follow_their_options() {
        let source = calendly();
        let value =
            "t=1792000000,v1=4e4a42d353d812bbb128b5c8a34fad2cde6afb03c8d5b34d2059122122cb984f";
        assert_eq!(
            Hmac::sign(&source, Some(1_792_000_000), None, CALENDLY_BODY).unwrap(),
            value
        );
        let verified = Hmac::verify(
            &source,
            &headers(&[("calendly-webhook-signature", value)]),
            CALENDLY_BODY,
            Some(Tolerance::at(at(1_792_000_000), 300)),
        )
        .unwrap();
        assert_eq!(verified.timestamp, Some(at(1_792_000_000)));
        assert_eq!(
            Hmac::verify(
                &source,
                &headers(&[("calendly-webhook-signature", value)]),
                b"{}",
                None
            ),
            Err(VerifyError::SignatureMismatch)
        );
    }

    #[test]
    fn hmac_sources_read_timestamp_id_and_event_headers() {
        let source = SourceOptions {
            header: Some("X-Slack-Signature".to_owned()),
            algorithm: HmacAlgorithm::Sha512,
            format: Some("v0={signature}".to_owned()),
            signed: Some("v0:{timestamp}:{body}".to_owned()),
            timestamp_header: Some("X-Slack-Request-Timestamp".to_owned()),
            event_header: Some("X-Event".to_owned()),
            ..SourceOptions::new("slack-key")
        };
        let signature = Hmac::sign(&source, Some(1_792_000_000), None, b"token=abc").unwrap();
        assert!(signature.starts_with("v0=d0cef74b692a9551"), "{signature}");
        let request = headers(&[
            ("x-slack-signature", &signature),
            ("x-slack-request-timestamp", "1792000000"),
            ("x-event", "message"),
        ]);
        let verified = Hmac::verify(&source, &request, b"token=abc", None).unwrap();
        assert_eq!(verified.timestamp, Some(at(1_792_000_000)));
        assert_eq!(verified.event_type.as_deref(), Some("message"));

        let sha1 = SourceOptions {
            header: Some("X-Signature".to_owned()),
            algorithm: HmacAlgorithm::Sha1,
            encoding: SignatureEncoding::Base64,
            signed: Some("{timestamp}.{body}".to_owned()),
            timestamp_header: Some("X-Timestamp".to_owned()),
            id_header: Some("X-Id".to_owned()),
            ..SourceOptions::new("calendly-key")
        };
        let request = headers(&[
            ("x-signature", "hAv2f9h2daKeafdkvN5tM1rwk48="),
            ("x-timestamp", "1792000000"),
            ("x-id", "delivery-1"),
        ]);
        let verified = Hmac::verify(&sha1, &request, CALENDLY_BODY, None).unwrap();
        assert_eq!(verified.id.as_deref(), Some("delivery-1"));
    }

    #[test]
    fn hmac_sources_without_a_header_are_misconfigured() {
        let source = SourceOptions::new("key");
        assert!(matches!(
            Hmac::verify(&source, &HeaderMap::new(), b"", None),
            Err(VerifyError::Misconfigured(_))
        ));
        let no_timestamp = SourceOptions {
            header: Some("X-Sig".to_owned()),
            signed: Some("{timestamp}.{body}".to_owned()),
            ..SourceOptions::new("key")
        };
        assert!(matches!(
            Hmac::verify(&no_timestamp, &headers(&[("x-sig", "00")]), b"", None),
            Err(VerifyError::Misconfigured(_))
        ));
    }
}
