//! Amazon SNS message signatures: [`SnsMessage`], the text SNS signs, and
//! the checks every receiver of SNS messages makes before it trusts one.
//!
//! SNS signs each message with RSA: SHA-1 for `SignatureVersion` 1 and
//! SHA-256 for version 2, with the key of the certificate at
//! `SigningCertURL`. A receiver:
//!
//! 1. reads the body as an [`SnsMessage`];
//! 2. checks, with [`SnsMessage::check_envelope`], before it fetches
//!    anything: a known `Type` and `SignatureVersion`; a `TopicArn` that is
//!    one of its own topics (SNS signs every topic's messages with the same
//!    keys, so a valid signature alone only says that some topic sent it);
//!    a `SigningCertURL` that [`is_signing_cert_url`] accepts (HTTPS on an
//!    SNS host, `sns.<region>.amazonaws.com` or `.amazonaws.com.cn`, a
//!    `.pem` path and no query); and a `Timestamp` within the tolerance of
//!    now ([`DEFAULT_TOLERANCE_SECS`]: SNS keeps retrying a delivery for at
//!    most an hour, with the same timestamp and signature);
//! 3. takes the certificate's key from a [`CertificateCache`], or fetches
//!    the certificate over HTTPS, reads its key with
//!    [`VerifyingKey::from_pem`](crate::jose::VerifyingKey::from_pem) and
//!    caches it;
//! 4. calls [`SnsMessage::verify_signature`].
//!
//! Fetching is left to the caller, which owns the HTTP client. navree-mail's
//! SES routes and navree-webhooks' `AwsSns` both receive SNS messages this
//! way.
//!
//! ```
//! use navree_crypto::sns;
//!
//! let certificate = "https://sns.eu-west-1.amazonaws.com/SimpleNotificationService-01d088a6f77103d0fe307c0069e40ed6.pem";
//! assert!(sns::is_signing_cert_url(certificate));
//! assert!(!sns::is_signing_cert_url("http://sns.eu-west-1.amazonaws.com/cert.pem"));
//! assert!(!sns::is_signing_cert_url("https://sns.eu-west-1.amazonaws.com.evil.example/cert.pem"));
//! assert!(!sns::is_signing_cert_url("https://sns.eu-west-1.amazonaws.com/?Action=Publish&x=.pem"));
//! ```

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, TimeDelta, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::jose::{Algorithm, VerifyingKey};

/// An SNS message: the JSON body SNS posts to an HTTP(S) subscription.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SnsMessage {
    /// `Notification`, `SubscriptionConfirmation` or
    /// `UnsubscribeConfirmation`.
    #[serde(rename = "Type")]
    pub message_type: String,
    /// The message id.
    #[serde(rename = "MessageId")]
    pub message_id: String,
    /// The topic.
    #[serde(rename = "TopicArn")]
    pub topic_arn: String,
    /// The subject, when the publisher gave one (notifications).
    #[serde(rename = "Subject", default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// The message: for SES, a JSON event (see
    /// [`message_json`](SnsMessage::message_json)).
    #[serde(rename = "Message")]
    pub message: String,
    /// When it was published (ISO 8601).
    #[serde(rename = "Timestamp")]
    pub timestamp: String,
    /// `1` (SHA-1) or `2` (SHA-256).
    #[serde(rename = "SignatureVersion")]
    pub signature_version: String,
    /// The base64 signature.
    #[serde(rename = "Signature")]
    pub signature: String,
    /// The signing certificate.
    #[serde(rename = "SigningCertURL", alias = "SigningCertUrl")]
    pub signing_cert_url: String,
    /// The URL that confirms the subscription (confirmations).
    #[serde(
        rename = "SubscribeURL",
        alias = "SubscribeUrl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub subscribe_url: Option<String>,
    /// The URL that ends the subscription (notifications).
    #[serde(
        rename = "UnsubscribeURL",
        alias = "UnsubscribeUrl",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub unsubscribe_url: Option<String>,
    /// The confirmation token (confirmations).
    #[serde(rename = "Token", default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// The message attributes, when the publisher set any.
    #[serde(
        rename = "MessageAttributes",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub message_attributes: Option<Value>,
}

impl SnsMessage {
    /// Whether it is a `Notification`.
    pub fn is_notification(&self) -> bool {
        self.message_type == "Notification"
    }

    /// The `Message` read as JSON of type `T` (an SES event, for
    /// instance).
    pub fn message_json<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_str(&self.message)
    }

    /// What SNS signs: `name\nvalue\n` for `Message`, `MessageId`,
    /// `Subject` (when present), `Timestamp`, `TopicArn` and `Type` of a
    /// notification, and for `Message`, `MessageId`, `SubscribeURL`,
    /// `Timestamp`, `Token`, `TopicArn` and `Type` of a confirmation.
    pub fn string_to_sign(&self) -> String {
        let mut fields: Vec<(&str, &str)> =
            vec![("Message", &self.message), ("MessageId", &self.message_id)];
        if self.is_notification() {
            if let Some(subject) = &self.subject {
                fields.push(("Subject", subject));
            }
        } else if let Some(url) = &self.subscribe_url {
            fields.push(("SubscribeURL", url));
        }
        fields.push(("Timestamp", &self.timestamp));
        if !self.is_notification()
            && let Some(token) = &self.token
        {
            fields.push(("Token", token));
        }
        fields.push(("TopicArn", &self.topic_arn));
        fields.push(("Type", &self.message_type));
        fields
            .into_iter()
            .map(|(name, value)| format!("{name}\n{value}\n"))
            .collect()
    }

    /// Verifies the signature with `key`, the public key of the certificate
    /// at `SigningCertURL`: RSA with SHA-1 for `SignatureVersion` 1, RSA
    /// with SHA-256 for version 2. Check the certificate URL with
    /// [`is_signing_cert_url`] before fetching it.
    pub fn verify_signature(&self, key: &VerifyingKey) -> Result<(), SnsSignatureError> {
        let signature = STANDARD
            .decode(self.signature.trim())
            .map_err(|_| SnsSignatureError::MalformedSignature)?;
        let signed = self.string_to_sign();
        let verified = match self.signature_version.trim() {
            "1" => key.verify_legacy_sha1(signed.as_bytes(), &signature),
            "2" => key.verify(Algorithm::RS256, signed.as_bytes(), &signature),
            _ => return Err(SnsSignatureError::UnsupportedVersion),
        };
        verified.map_err(|_| SnsSignatureError::Mismatch)
    }

    /// Every check but the signature, made before the signing certificate
    /// is fetched: the `Type` is `Notification`, `SubscriptionConfirmation`
    /// or `UnsubscribeConfirmation`; the `TopicArn` is one of `topics`; the
    /// `SigningCertURL` is an [SNS certificate](is_signing_cert_url); the
    /// `SignatureVersion` is `1` or `2`; and the `Timestamp` is at most
    /// `tolerance_secs` from `now`, in either direction. Returns when the
    /// message was published.
    pub fn check_envelope(
        &self,
        topics: &[String],
        tolerance_secs: u64,
        now: DateTime<Utc>,
    ) -> Result<DateTime<Utc>, SnsEnvelopeError> {
        if !matches!(
            self.message_type.as_str(),
            "Notification" | "SubscriptionConfirmation" | "UnsubscribeConfirmation"
        ) {
            return Err(SnsEnvelopeError::UnknownType);
        }
        if self.topic_arn.is_empty() || !topics.contains(&self.topic_arn) {
            return Err(SnsEnvelopeError::ForeignTopic);
        }
        if !is_signing_cert_url(self.signing_cert_url.trim()) {
            return Err(SnsEnvelopeError::ForeignCertificate);
        }
        if !matches!(self.signature_version.trim(), "1" | "2") {
            return Err(SnsEnvelopeError::UnsupportedVersion);
        }
        let published = DateTime::parse_from_rfc3339(self.timestamp.trim())
            .map_err(|_| SnsEnvelopeError::MalformedTimestamp)?
            .with_timezone(&Utc);
        if (now - published).num_seconds().unsigned_abs() > tolerance_secs {
            return Err(SnsEnvelopeError::OutOfTolerance);
        }
        Ok(published)
    }
}

/// How far a message's `Timestamp` may be from now by default, in seconds:
/// SNS retries an HTTPS delivery for at most an hour, keeping the message's
/// timestamp and signature, so an older message is a replay.
pub const DEFAULT_TOLERANCE_SECS: u64 = 3_600;

/// Why [`SnsMessage::check_envelope`] refused a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SnsEnvelopeError {
    /// `Type` is not one SNS sends to subscriptions.
    #[error("the SNS message type is unknown")]
    UnknownType,
    /// `TopicArn` is not one of the receiver's topics.
    #[error("the SNS topic is not one of the receiver's topics")]
    ForeignTopic,
    /// `SigningCertURL` is not a certificate on an SNS host.
    #[error("the SNS `SigningCertURL` is not an SNS certificate URL")]
    ForeignCertificate,
    /// `SignatureVersion` is neither `1` nor `2`.
    #[error("the SNS `SignatureVersion` is neither 1 nor 2")]
    UnsupportedVersion,
    /// `Timestamp` is not an RFC 3339 date.
    #[error("the SNS `Timestamp` is not a date")]
    MalformedTimestamp,
    /// `Timestamp` is further from now than the tolerance.
    #[error("the SNS `Timestamp` is further from now than the tolerance")]
    OutOfTolerance,
}

/// How long a fetched signing certificate is kept.
pub const CERTIFICATE_TTL: TimeDelta = TimeDelta::hours(24);

/// The most certificates a [`CertificateCache`] keeps; the oldest goes
/// first.
pub const MAX_CERTIFICATES: usize = 32;

/// The keys of fetched signing certificates, by URL, each kept for
/// [`CERTIFICATE_TTL`], at most [`MAX_CERTIFICATES`] of them. The receiver
/// checks the URL, fetches what [`get`](CertificateCache::get) does not
/// have, and [`insert`](CertificateCache::insert)s it.
#[derive(Default)]
pub struct CertificateCache {
    keys: Mutex<HashMap<String, (VerifyingKey, DateTime<Utc>)>>,
}

impl CertificateCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// The key of the certificate at `url`, when it was fetched less than
    /// [`CERTIFICATE_TTL`] before `now`.
    pub fn get(&self, url: &str, now: DateTime<Utc>) -> Option<VerifyingKey> {
        lock(&self.keys)
            .get(url)
            .filter(|(_, fetched_at)| now - *fetched_at < CERTIFICATE_TTL)
            .map(|(key, _)| key.clone())
    }

    /// Keeps `key`, the key of the certificate at `url`, fetched at `now`.
    /// A full cache first drops the certificate fetched longest ago.
    pub fn insert(&self, url: &str, key: VerifyingKey, now: DateTime<Utc>) {
        let mut keys = lock(&self.keys);
        if !keys.contains_key(url)
            && keys.len() >= MAX_CERTIFICATES
            && let Some(oldest) = keys
                .iter()
                .min_by_key(|(_, (_, fetched_at))| *fetched_at)
                .map(|(url, _)| url.clone())
        {
            keys.remove(&oldest);
        }
        keys.insert(url.to_owned(), (key, now));
    }

    /// How many certificates are kept.
    pub fn len(&self) -> usize {
        lock(&self.keys).len()
    }

    /// Whether no certificate is kept.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl fmt::Debug for CertificateCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CertificateCache")
            .field("certificates", &self.len())
            .finish()
    }
}

/// Locks `mutex`, recovering it from a panic elsewhere (the cache stays
/// usable).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Why [`SnsMessage::verify_signature`] refused a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SnsSignatureError {
    /// `Signature` is not base64.
    #[error("the SNS `Signature` is not base64")]
    MalformedSignature,
    /// `SignatureVersion` is neither `1` nor `2`.
    #[error("the SNS `SignatureVersion` is neither 1 nor 2")]
    UnsupportedVersion,
    /// The signature does not match the message with this key.
    #[error("the SNS signature does not match the message")]
    Mismatch,
}

/// Whether `url` is an HTTPS URL on an SNS host
/// (`sns.<region>.amazonaws.com` or `sns.<region>.amazonaws.com.cn`), with
/// no credentials and the default port. `SubscribeURL`s are checked with it
/// before they are fetched.
pub fn is_sns_url(url: &str) -> bool {
    let Ok(url) = url::Url::parse(url) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let region = host
        .strip_suffix(".amazonaws.com")
        .or_else(|| host.strip_suffix(".amazonaws.com.cn"))
        .and_then(|rest| rest.strip_prefix("sns."));
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && region.is_some_and(|region| {
            region.len() >= 3
                && region
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

/// Whether `url` can be an SNS signing certificate: an [SNS URL](is_sns_url)
/// whose path ends in `.pem`, with no query and no fragment.
pub fn is_signing_cert_url(url: &str) -> bool {
    is_sns_url(url)
        && url::Url::parse(url).is_ok_and(|url| {
            url.path().ends_with(".pem") && url.query().is_none() && url.fragment().is_none()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sns_hosts_are_recognized() {
        for url in [
            "https://sns.us-east-1.amazonaws.com/x.pem",
            "https://sns.eu-central-2.amazonaws.com/?Action=ConfirmSubscription",
            "https://sns.cn-north-1.amazonaws.com.cn/x.pem",
        ] {
            assert!(is_sns_url(url), "{url}");
        }
        for url in [
            "http://sns.us-east-1.amazonaws.com/x.pem",
            "https://sns.us-east-1.amazonaws.com:8443/x.pem",
            "https://user@sns.us-east-1.amazonaws.com/x.pem",
            "https://sns.amazonaws.com/x.pem",
            "https://sns.us.amazonaws.com/x.pem",
            "https://sns.us_east.amazonaws.com/x.pem",
            "https://s3.us-east-1.amazonaws.com/x.pem",
            "https://sns.us-east-1.amazonaws.com.example/x.pem",
            "https://sns.us-east-1.evil.example/x.pem",
            "not a url",
        ] {
            assert!(!is_sns_url(url), "{url}");
        }
    }

    #[test]
    fn signing_certificates_are_pem_files_on_an_sns_host() {
        assert!(is_signing_cert_url(
            "https://sns.us-east-1.amazonaws.com/SimpleNotificationService-9c6465fa7f48f5cacd23014631ec1136.pem"
        ));
        for url in [
            "https://sns.us-east-1.amazonaws.com/cert.txt",
            "https://sns.us-east-1.amazonaws.com/?Action=Publish&x=.pem",
            "https://sns.us-east-1.amazonaws.com/cert.pem?x=1",
            "https://sns.us-east-1.amazonaws.com/cert.pem#x",
            "https://attacker.example/sns.us-east-1.amazonaws.com.pem",
            "http://sns.us-east-1.amazonaws.com/cert.pem",
        ] {
            assert!(!is_signing_cert_url(url), "{url}");
        }
    }

    const TOPIC: &str = "arn:aws:sns:eu-west-1:123456789012:ses";

    /// A notification of `TOPIC` published at 10:00:02 on 27 October 2026.
    fn envelope() -> SnsMessage {
        SnsMessage {
            message_type: "Notification".to_owned(),
            message_id: "m-1".to_owned(),
            topic_arn: TOPIC.to_owned(),
            message: "{}".to_owned(),
            timestamp: "2026-10-27T10:00:02.000Z".to_owned(),
            signature_version: "2".to_owned(),
            signature: "c2lnbmVk".to_owned(),
            signing_cert_url: "https://sns.eu-west-1.amazonaws.com/SimpleNotificationService-01d088a6f77103d0fe307c0069e40ed6.pem".to_owned(),
            ..SnsMessage::default()
        }
    }

    fn at(time: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("2026-10-27T{time}Z"))
            .unwrap()
            .with_timezone(&Utc)
    }

    fn topics() -> Vec<String> {
        vec![TOPIC.to_owned()]
    }

    #[test]
    fn the_envelope_of_a_recent_message_of_a_known_topic_passes() {
        let published = envelope()
            .check_envelope(&topics(), DEFAULT_TOLERANCE_SECS, at("10:01:02"))
            .unwrap();
        assert_eq!(published, at("10:00:02"));
        for message_type in ["SubscriptionConfirmation", "UnsubscribeConfirmation"] {
            let message = SnsMessage {
                message_type: message_type.to_owned(),
                ..envelope()
            };
            assert!(
                message
                    .check_envelope(&topics(), DEFAULT_TOLERANCE_SECS, at("10:01:02"))
                    .is_ok()
            );
        }
        let version_1 = SnsMessage {
            signature_version: " 1 ".to_owned(),
            ..envelope()
        };
        assert!(
            version_1
                .check_envelope(&topics(), DEFAULT_TOLERANCE_SECS, at("10:01:02"))
                .is_ok()
        );
    }

    #[test]
    fn envelopes_that_cannot_be_trusted_are_refused() {
        let check = |message: SnsMessage| {
            message.check_envelope(&topics(), DEFAULT_TOLERANCE_SECS, at("10:01:02"))
        };
        for message_type in ["Publish", "notification", ""] {
            assert_eq!(
                check(SnsMessage {
                    message_type: message_type.to_owned(),
                    ..envelope()
                }),
                Err(SnsEnvelopeError::UnknownType),
                "{message_type}"
            );
        }
        for topic in ["arn:aws:sns:eu-west-1:123456789012:other", ""] {
            assert_eq!(
                check(SnsMessage {
                    topic_arn: topic.to_owned(),
                    ..envelope()
                }),
                Err(SnsEnvelopeError::ForeignTopic),
                "{topic}"
            );
        }
        // No topics configured: nothing is trusted, an empty topic included.
        assert_eq!(
            SnsMessage {
                topic_arn: String::new(),
                ..envelope()
            }
            .check_envelope(&[String::new()], DEFAULT_TOLERANCE_SECS, at("10:01:02")),
            Err(SnsEnvelopeError::ForeignTopic)
        );
        assert_eq!(
            envelope().check_envelope(&[], DEFAULT_TOLERANCE_SECS, at("10:01:02")),
            Err(SnsEnvelopeError::ForeignTopic)
        );
        for url in [
            "http://sns.eu-west-1.amazonaws.com/cert.pem",
            "https://sns.eu-west-1.amazonaws.com.evil.example/cert.pem",
            "https://attacker.example/sns.eu-west-1.amazonaws.com.pem",
            "https://sns.eu-west-1.amazonaws.com/cert.pem?x=1",
            "",
        ] {
            assert_eq!(
                check(SnsMessage {
                    signing_cert_url: url.to_owned(),
                    ..envelope()
                }),
                Err(SnsEnvelopeError::ForeignCertificate),
                "{url}"
            );
        }
        for version in ["3", "0", "", "SHA256"] {
            assert_eq!(
                check(SnsMessage {
                    signature_version: version.to_owned(),
                    ..envelope()
                }),
                Err(SnsEnvelopeError::UnsupportedVersion),
                "{version}"
            );
        }
        for timestamp in ["yesterday", "", "2026-10-27 10:00:02"] {
            assert_eq!(
                check(SnsMessage {
                    timestamp: timestamp.to_owned(),
                    ..envelope()
                }),
                Err(SnsEnvelopeError::MalformedTimestamp),
                "{timestamp}"
            );
        }
    }

    #[test]
    fn the_timestamp_is_checked_both_ways() {
        // Published at 10:00:02; an hour either side passes.
        for time in ["09:00:02", "10:00:02", "11:00:02"] {
            assert!(
                envelope()
                    .check_envelope(&topics(), DEFAULT_TOLERANCE_SECS, at(time))
                    .is_ok(),
                "{time}"
            );
        }
        for time in ["09:00:01", "11:00:03", "23:59:59"] {
            assert_eq!(
                envelope().check_envelope(&topics(), DEFAULT_TOLERANCE_SECS, at(time)),
                Err(SnsEnvelopeError::OutOfTolerance),
                "{time}"
            );
        }
        assert_eq!(
            envelope().check_envelope(&topics(), 30, at("10:01:02")),
            Err(SnsEnvelopeError::OutOfTolerance)
        );
        assert!(
            envelope()
                .check_envelope(&topics(), 60, at("10:01:02"))
                .is_ok()
        );
    }

    #[test]
    fn certificates_are_kept_a_day_and_the_oldest_goes_first() {
        let key = VerifyingKey::hmac(Algorithm::HS256, [7u8; 32]).unwrap();
        let cache = CertificateCache::new();
        let url = "https://sns.eu-west-1.amazonaws.com/a.pem";
        assert!(cache.get(url, at("10:00:00")).is_none());
        cache.insert(url, key.clone(), at("10:00:00"));
        assert!(cache.get(url, at("10:00:00")).is_some());
        assert!(
            cache
                .get(
                    url,
                    at("10:00:00") + CERTIFICATE_TTL - TimeDelta::seconds(1)
                )
                .is_some()
        );
        assert!(cache.get(url, at("10:00:00") + CERTIFICATE_TTL).is_none());
        assert!(
            cache
                .get("https://sns.eu-west-1.amazonaws.com/b.pem", at("10:00:00"))
                .is_none()
        );

        for i in 1..MAX_CERTIFICATES {
            cache.insert(
                &format!("https://sns.eu-west-1.amazonaws.com/{i}.pem"),
                key.clone(),
                at("10:00:00") + TimeDelta::seconds(i64::try_from(i).unwrap()),
            );
        }
        assert_eq!(cache.len(), MAX_CERTIFICATES);
        // Fetching one again does not evict another.
        cache.insert(url, key.clone(), at("10:00:00"));
        assert_eq!(cache.len(), MAX_CERTIFICATES);
        cache.insert(
            "https://sns.eu-west-1.amazonaws.com/new.pem",
            key,
            at("11:00:00"),
        );
        assert_eq!(cache.len(), MAX_CERTIFICATES);
        assert!(
            cache.get(url, at("11:00:00")).is_none(),
            "the oldest went first"
        );
        assert!(
            cache
                .get(
                    "https://sns.eu-west-1.amazonaws.com/new.pem",
                    at("11:00:00")
                )
                .is_some()
        );
        assert!(!format!("{cache:?}").contains("hmac"));
    }

    #[test]
    fn the_string_to_sign_follows_the_message_type() {
        let notification = SnsMessage {
            message_type: "Notification".to_owned(),
            message_id: "m-1".to_owned(),
            topic_arn: "arn:aws:sns:eu-west-1:123456789012:ses".to_owned(),
            subject: Some("Amazon SES Email Event Notification".to_owned()),
            message: "{}".to_owned(),
            timestamp: "2026-10-27T10:00:02.000Z".to_owned(),
            token: Some("ignored for notifications".to_owned()),
            ..SnsMessage::default()
        };
        assert_eq!(
            notification.string_to_sign(),
            "Message\n{}\nMessageId\nm-1\nSubject\nAmazon SES Email Event Notification\n\
             Timestamp\n2026-10-27T10:00:02.000Z\nTopicArn\narn:aws:sns:eu-west-1:123456789012:ses\n\
             Type\nNotification\n"
        );
        let confirmation = SnsMessage {
            message_type: "SubscriptionConfirmation".to_owned(),
            subject: Some("ignored for confirmations".to_owned()),
            subscribe_url: Some(
                "https://sns.eu-west-1.amazonaws.com/?Action=ConfirmSubscription".to_owned(),
            ),
            token: Some("2336412f".to_owned()),
            ..notification
        };
        assert_eq!(
            confirmation.string_to_sign(),
            "Message\n{}\nMessageId\nm-1\n\
             SubscribeURL\nhttps://sns.eu-west-1.amazonaws.com/?Action=ConfirmSubscription\n\
             Timestamp\n2026-10-27T10:00:02.000Z\nToken\n2336412f\n\
             TopicArn\narn:aws:sns:eu-west-1:123456789012:ses\nType\nSubscriptionConfirmation\n"
        );
    }
}
