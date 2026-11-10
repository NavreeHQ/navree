//! The Amazon SNS messages of the SES routes (`POST /mail/webhooks/ses` and
//! `POST /mail/inbound/ses`), checked before any event or mail in them is
//! trusted. The checks are navree-webhooks' `AwsSns` ones, from the same
//! `navree_crypto::sns`: a known message type, one of the route's topics, a
//! signing certificate on an SNS host, a recent timestamp
//! ([`SnsMessage::check_envelope`]), then a valid signature with the key of
//! a certificate fetched over HTTPS and cached a day
//! ([`CertificateCache`]). The URL token of the routes is checked on top of
//! it.

use chrono::{DateTime, Utc};
use navree_crypto::jose::VerifyingKey;
use navree_crypto::sns::{CertificateCache, SnsEnvelopeError, SnsMessage};
use navree_http_client::HttpClient;
use serde::de::IgnoredAny;

/// How old (or how far ahead) a message may be by default, in seconds: SNS
/// retries an HTTPS delivery for at most an hour, keeping the message's
/// timestamp and signature.
pub(crate) const DEFAULT_TOLERANCE_SECS: u64 = navree_crypto::sns::DEFAULT_TOLERANCE_SECS;

/// Why an SNS message is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SnsRefusal {
    /// The body is not JSON: a 400.
    NotJson,
    /// The message cannot be trusted, for the reason given (logged, never
    /// sent): a 401.
    Unverified(&'static str),
    /// The signing certificate could not be fetched: a 503, which SNS
    /// retries.
    Unavailable,
}

/// Checks SNS messages, fetching each signing certificate once a day.
pub(crate) struct SnsVerifier {
    client: Option<HttpClient>,
    certificates: CertificateCache,
}

impl SnsVerifier {
    /// A verifier that fetches certificates with `client`; without one,
    /// every message is [`SnsRefusal::Unavailable`].
    pub(crate) fn new(client: Option<HttpClient>) -> Self {
        Self {
            client,
            certificates: CertificateCache::new(),
        }
    }

    /// The message in `body`, once it is trusted: a `Notification`,
    /// `SubscriptionConfirmation` or `UnsubscribeConfirmation` of one of
    /// `topics`, whose `SigningCertURL` is an SNS certificate, whose
    /// `Timestamp` is within `tolerance_secs` of `now`, and whose signature
    /// (version 1 or 2) that certificate's key verifies. Everything but the
    /// signature is checked before a certificate is fetched.
    pub(crate) async fn verify(
        &self,
        body: &[u8],
        topics: &[String],
        tolerance_secs: u64,
        now: DateTime<Utc>,
    ) -> Result<SnsMessage, SnsRefusal> {
        let message: SnsMessage = match serde_json::from_slice(body) {
            Ok(message) => message,
            Err(_) if serde_json::from_slice::<IgnoredAny>(body).is_ok() => {
                return Err(SnsRefusal::Unverified(
                    "the body is not a signed SNS message",
                ));
            }
            Err(_) => return Err(SnsRefusal::NotJson),
        };
        message
            .check_envelope(topics, tolerance_secs, now)
            .map_err(|error| SnsRefusal::Unverified(reason(error)))?;
        let key = self
            .certificate(message.signing_cert_url.trim(), now)
            .await?;
        message
            .verify_signature(&key)
            .map_err(|_| SnsRefusal::Unverified("the signature does not match"))?;
        Ok(message)
    }

    /// The key of the certificate at `url` (an SNS certificate URL, checked
    /// by the caller), from the cache or fetched.
    async fn certificate(&self, url: &str, now: DateTime<Utc>) -> Result<VerifyingKey, SnsRefusal> {
        if let Some(key) = self.certificates.get(url, now) {
            return Ok(key);
        }
        let client = self.client.as_ref().ok_or(SnsRefusal::Unavailable)?;
        let response = client
            .get(url)
            .send()
            .await
            .ok()
            .filter(|response| response.is_success())
            .ok_or(SnsRefusal::Unavailable)?;
        let key = VerifyingKey::from_pem(&response.text())
            .map_err(|_| SnsRefusal::Unverified("the signing certificate is not valid"))?;
        self.certificates.insert(url, key.clone(), now);
        Ok(key)
    }
}

/// What the log says about a message refused before its signature is
/// checked.
fn reason(error: SnsEnvelopeError) -> &'static str {
    match error {
        SnsEnvelopeError::UnknownType => "the SNS message type is unknown",
        SnsEnvelopeError::ForeignTopic => "the topic is not in topic_arns",
        SnsEnvelopeError::ForeignCertificate => "SigningCertURL is not an SNS certificate URL",
        SnsEnvelopeError::UnsupportedVersion => "SignatureVersion is neither 1 nor 2",
        SnsEnvelopeError::MalformedTimestamp => "Timestamp is not a date",
        SnsEnvelopeError::OutOfTolerance => "Timestamp is further from now than tolerance_secs",
        _ => "the SNS message cannot be trusted",
    }
}

#[cfg(test)]
mod tests {
    use http::Method;
    use navree_http_client::{FakeResponse, FakeServer};
    use serde_json::{Value, json};

    use super::*;

    const EVENTS_TOPIC: &str = "arn:aws:sns:eu-west-1:123456789012:ses";
    const INBOUND_TOPIC: &str = "arn:aws:sns:eu-west-1:123456789012:inbound-mail";
    const CERT_URL: &str = "https://sns.eu-west-1.amazonaws.com/SimpleNotificationService-01d088a6f77103d0fe307c0069e40ed6.pem";
    const CERTIFICATE: &str = include_str!("../tests/fixtures/sns/signer-cert.pem");
    const MESSAGES: &str = include_str!("../tests/fixtures/sns/sns-messages.json");
    const SNS_ACTION: &str = include_str!("../tests/fixtures/inbound/ses-sns-action.json");
    const S3_ACTION: &str = include_str!("../tests/fixtures/inbound/ses-s3-action.json");

    /// A signed message of `sns-messages.json`.
    fn fixture(name: &str) -> Value {
        let messages: Value = serde_json::from_str(MESSAGES).unwrap();
        assert!(messages[name].is_object(), "no fixture {name}");
        messages[name].clone()
    }

    fn body(message: &Value) -> Vec<u8> {
        serde_json::to_vec(message).unwrap()
    }

    /// 2026-10-27 at `time` (UTC).
    fn at(time: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("2026-10-27T{time}Z"))
            .unwrap()
            .with_timezone(&Utc)
    }

    /// A minute after the bounce was published.
    fn now() -> DateTime<Utc> {
        at("10:01:02")
    }

    fn topics() -> Vec<String> {
        vec![EVENTS_TOPIC.to_owned(), INBOUND_TOPIC.to_owned()]
    }

    /// A verifier whose fake client serves the test certificate.
    fn verifier() -> (SnsVerifier, FakeServer) {
        let (client, server) = HttpClient::fake();
        server.on(
            Method::GET,
            CERT_URL,
            FakeResponse::new(200).text(CERTIFICATE),
        );
        (SnsVerifier::new(Some(client)), server)
    }

    async fn check(verifier: &SnsVerifier, message: &Value) -> Result<SnsMessage, SnsRefusal> {
        verifier
            .verify(&body(message), &topics(), DEFAULT_TOLERANCE_SECS, now())
            .await
    }

    #[tokio::test]
    async fn signed_messages_of_both_versions_pass() {
        let (verifier, server) = verifier();
        for name in [
            "ses_bounce_v1",
            "ses_bounce_v2",
            "ses_subscription_confirmation_v1",
            "ses_unsubscribe_confirmation_v2",
        ] {
            let message = fixture(name);
            let verified = check(&verifier, &message).await;
            let Ok(verified) = verified else {
                panic!("{name}: {verified:?}");
            };
            assert_eq!(verified.message_id, message["MessageId"].as_str().unwrap());
        }
        let bounce = check(&verifier, &fixture("ses_bounce_v2")).await.unwrap();
        let event: Value = bounce.message_json().unwrap();
        assert_eq!(event["mail"]["messageId"], "fake-1");

        // The receipt notifications of the inbound route (version 1 and 2).
        for (name, payload) in [("ses-sns-action", SNS_ACTION), ("ses-s3-action", S3_ACTION)] {
            let verified = verifier
                .verify(
                    payload.as_bytes(),
                    &topics(),
                    DEFAULT_TOLERANCE_SECS,
                    at("09:15:00"),
                )
                .await;
            assert!(verified.is_ok(), "{name}: {verified:?}");
        }

        // The certificate was fetched once.
        server.assert_sent_count(1);
    }

    #[tokio::test]
    async fn tampered_messages_are_refused() {
        let (verifier, _server) = verifier();
        let mismatch = Err(SnsRefusal::Unverified("the signature does not match"));
        for (field, value) in [
            (
                "Message",
                json!(
                    fixture("ses_bounce_v1")["Message"]
                        .as_str()
                        .unwrap()
                        .replace("Permanent", "Transient")
                ),
            ),
            ("MessageId", json!("another-message")),
            ("Subject", json!("Another subject")),
            ("Timestamp", json!("2026-10-27T10:00:03.000Z")),
            ("TopicArn", json!(INBOUND_TOPIC)),
        ] {
            for name in ["ses_bounce_v1", "ses_bounce_v2"] {
                let mut message = fixture(name);
                message[field] = value.clone();
                assert_eq!(
                    check(&verifier, &message).await,
                    mismatch,
                    "{name}: {field}"
                );
            }
        }
        // A notification passed off as a confirmation signs other fields.
        let mut message = fixture("ses_bounce_v2");
        message["Type"] = json!("UnsubscribeConfirmation");
        assert_eq!(check(&verifier, &message).await, mismatch);
        // A confirmation pointing somewhere else.
        let mut message = fixture("ses_subscription_confirmation_v1");
        message["SubscribeURL"] = json!(
            "https://sns.eu-west-1.amazonaws.com/?Action=ConfirmSubscription&TopicArn=arn:aws:sns:eu-west-1:999999999999:other"
        );
        assert_eq!(check(&verifier, &message).await, mismatch);
    }

    #[tokio::test]
    async fn a_message_signed_by_another_key_is_refused() {
        let (verifier, _server) = verifier();
        assert_eq!(
            check(&verifier, &fixture("ses_bounce_other_key_v2")).await,
            Err(SnsRefusal::Unverified("the signature does not match"))
        );
        // The same signature, any version.
        let mut message = fixture("ses_bounce_other_key_v2");
        message["SignatureVersion"] = json!("1");
        assert!(check(&verifier, &message).await.is_err());
    }

    #[tokio::test]
    async fn foreign_certificate_hosts_are_refused_before_any_fetch() {
        let (verifier, server) = verifier();
        for url in [
            "https://sns.eu-west-1.amazonaws.com.evil.example/cert.pem",
            "https://attacker.example/sns.eu-west-1.amazonaws.com.pem",
            "http://sns.eu-west-1.amazonaws.com/SimpleNotificationService-01d088a6f77103d0fe307c0069e40ed6.pem",
            "https://sns.eu-west-1.amazonaws.com:8443/cert.pem",
            "https://user@sns.eu-west-1.amazonaws.com/cert.pem",
            "https://s3.eu-west-1.amazonaws.com/cert.pem",
            "https://sns.eu-west-1.amazonaws.com/cert.txt",
            "https://sns.eu-west-1.amazonaws.com/?Action=Publish&x=.pem",
            "",
        ] {
            let mut message = fixture("ses_bounce_v2");
            message["SigningCertURL"] = json!(url);
            assert_eq!(
                check(&verifier, &message).await,
                Err(SnsRefusal::Unverified(
                    "SigningCertURL is not an SNS certificate URL"
                )),
                "{url}"
            );
        }
        server.assert_nothing_sent();
    }

    #[tokio::test]
    async fn other_signature_versions_are_refused() {
        let (verifier, server) = verifier();
        for version in ["3", "0", "", "SHA256"] {
            let mut message = fixture("ses_bounce_v2");
            message["SignatureVersion"] = json!(version);
            assert_eq!(
                check(&verifier, &message).await,
                Err(SnsRefusal::Unverified(
                    "SignatureVersion is neither 1 nor 2"
                )),
                "{version}"
            );
        }
        server.assert_nothing_sent();
        // A SHA-1 signature is no SHA-256 one, and the other way around.
        for (name, version) in [("ses_bounce_v1", "2"), ("ses_bounce_v2", "1")] {
            let mut message = fixture(name);
            message["SignatureVersion"] = json!(version);
            assert_eq!(
                check(&verifier, &message).await,
                Err(SnsRefusal::Unverified("the signature does not match")),
                "{name} as version {version}"
            );
        }
    }

    #[tokio::test]
    async fn stale_and_future_messages_are_refused() {
        let (verifier, server) = verifier();
        let message = body(&fixture("ses_bounce_v1"));
        let stale = Err(SnsRefusal::Unverified(
            "Timestamp is further from now than tolerance_secs",
        ));
        // Published at 10:00:02; an hour either side passes.
        for time in ["09:00:02", "10:00:02", "11:00:02"] {
            let verified = verifier
                .verify(&message, &topics(), DEFAULT_TOLERANCE_SECS, at(time))
                .await;
            assert!(verified.is_ok(), "{time}: {verified:?}");
        }
        for time in ["09:00:01", "11:00:03", "23:59:59"] {
            assert_eq!(
                verifier
                    .verify(&message, &topics(), DEFAULT_TOLERANCE_SECS, at(time))
                    .await,
                stale,
                "{time}"
            );
        }
        // A route's own tolerance.
        assert_eq!(verifier.verify(&message, &topics(), 30, now()).await, stale);
        assert!(
            verifier
                .verify(&message, &topics(), 60, now())
                .await
                .is_ok()
        );

        let mut undated = fixture("ses_bounce_v1");
        undated["Timestamp"] = json!("yesterday");
        assert_eq!(
            check(&verifier, &undated).await,
            Err(SnsRefusal::Unverified("Timestamp is not a date"))
        );
        server.assert_sent_count(1);
    }

    #[tokio::test]
    async fn other_topics_types_and_bodies_are_refused() {
        let (verifier, server) = verifier();
        let only_inbound = [INBOUND_TOPIC.to_owned()];
        assert_eq!(
            verifier
                .verify(
                    &body(&fixture("ses_bounce_v2")),
                    &only_inbound,
                    DEFAULT_TOLERANCE_SECS,
                    now(),
                )
                .await,
            Err(SnsRefusal::Unverified("the topic is not in topic_arns"))
        );
        assert_eq!(
            verifier
                .verify(
                    &body(&fixture("ses_bounce_v2")),
                    &[],
                    DEFAULT_TOLERANCE_SECS,
                    now()
                )
                .await,
            Err(SnsRefusal::Unverified("the topic is not in topic_arns"))
        );
        let mut message = fixture("ses_bounce_v2");
        message["Type"] = json!("Publish");
        assert_eq!(
            check(&verifier, &message).await,
            Err(SnsRefusal::Unverified("the SNS message type is unknown"))
        );
        // An unsigned envelope, as the routes used to accept.
        let unsigned = json!({
            "Type": "Notification",
            "MessageId": "b6a0d6b0",
            "TopicArn": EVENTS_TOPIC,
            "Message": "{}",
        });
        assert_eq!(
            check(&verifier, &unsigned).await,
            Err(SnsRefusal::Unverified(
                "the body is not a signed SNS message"
            ))
        );
        assert_eq!(
            verifier
                .verify(b"not json", &topics(), DEFAULT_TOLERANCE_SECS, now())
                .await,
            Err(SnsRefusal::NotJson)
        );
        server.assert_nothing_sent();
    }

    #[tokio::test]
    async fn an_unreachable_or_invalid_certificate_is_never_trusted() {
        let (client, server) = HttpClient::fake();
        server.on(Method::GET, CERT_URL, FakeResponse::new(503));
        let verifier = SnsVerifier::new(Some(client));
        assert_eq!(
            check(&verifier, &fixture("ses_bounce_v2")).await,
            Err(SnsRefusal::Unavailable)
        );
        assert_eq!(
            SnsVerifier::new(None)
                .verify(
                    &body(&fixture("ses_bounce_v2")),
                    &topics(),
                    DEFAULT_TOLERANCE_SECS,
                    now(),
                )
                .await,
            Err(SnsRefusal::Unavailable)
        );

        let (client, server) = HttpClient::fake();
        server.on(
            Method::GET,
            CERT_URL,
            FakeResponse::new(200).text("not a certificate"),
        );
        let verifier = SnsVerifier::new(Some(client));
        assert_eq!(
            check(&verifier, &fixture("ses_bounce_v2")).await,
            Err(SnsRefusal::Unverified(
                "the signing certificate is not valid"
            ))
        );
    }
}
