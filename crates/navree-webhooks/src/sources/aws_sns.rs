//! [`AwsSns`]: SNS messages signed with the signing certificate of the
//! topic's region (signature version 1 and 2), and subscription
//! confirmation.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use navree_crypto::jose::{Algorithm, VerifyingKey};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::options::SourceOptions;
use crate::request::{Handshake, IncomingRequest, Verdict};
use crate::verify::{Tolerance, Verified, VerifyError, WebhookSource, WebhookVerifier};

/// Amazon SNS HTTP(S) subscriptions (SES events and inbound mail among
/// them). SNS signs each message with RSA, SHA-1 for signature version 1
/// and SHA-256 for version 2, using the certificate at `SigningCertURL`. The
/// URL must be HTTPS on an SNS host (`sns.<region>.amazonaws.com`, or
/// `.amazonaws.com.cn`) with a path ending in `.pem` and no query; the
/// certificate is fetched once
/// and cached ([`SenderKeys`](crate::SenderKeys)). The message's
/// `TopicArn` must be one of the source's `topics`: SNS signs every topic's
/// messages with the same keys.
///
/// A `SubscriptionConfirmation` is confirmed (its `SubscribeURL`, on an SNS
/// host, is fetched) and an `UnsubscribeConfirmation` acknowledged, both
/// without running the handler, unless the source sets `auto_confirm =
/// false`. The id is the `MessageId`, the event type the message `Type`,
/// the timestamp its `Timestamp`, which is checked only against the
/// source's own `tolerance_secs` (SNS retries a delivery for a while). Turn
/// raw message delivery off: a raw message carries no signature. Source
/// name `aws_sns`; no secret.
///
/// Read the body as [`SnsMessage`].
#[derive(Clone, Copy, Debug, Default)]
pub struct AwsSns;

impl AwsSns {
    /// The header naming the message type.
    pub const TYPE_HEADER: &'static str = "x-amz-sns-message-type";

    /// Whether `url` is an HTTPS URL on an SNS host
    /// (`sns.<region>.amazonaws.com` or `sns.<region>.amazonaws.com.cn`),
    /// with no credentials and the default port.
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

    /// Verifies `message`'s signature with `key`, the public key of its
    /// signing certificate.
    pub fn verify_signature(message: &SnsMessage, key: &VerifyingKey) -> Result<(), VerifyError> {
        let signature = STANDARD
            .decode(message.signature.trim())
            .map_err(|_| VerifyError::MalformedBody("`Signature`".to_owned()))?;
        let signed = message.string_to_sign();
        let verified = match message.signature_version.trim() {
            "1" => key.verify_legacy_sha1(signed.as_bytes(), &signature),
            "2" => key.verify(Algorithm::RS256, signed.as_bytes(), &signature),
            _ => return Err(VerifyError::MalformedBody("`SignatureVersion`".to_owned())),
        };
        verified.map_err(|_| VerifyError::SignatureMismatch)
    }
}

impl WebhookVerifier for AwsSns {
    async fn verify_incoming(request: &IncomingRequest<'_>) -> Result<Verdict, VerifyError> {
        let source = request.source();
        let message: SnsMessage = serde_json::from_slice(request.body())
            .map_err(|_| VerifyError::MalformedBody("SNS message".to_owned()))?;
        if !matches!(
            message.message_type.as_str(),
            "Notification" | "SubscriptionConfirmation" | "UnsubscribeConfirmation"
        ) {
            return Err(VerifyError::MalformedBody("`Type`".to_owned()));
        }
        if let Some(sent) = request.header(Self::TYPE_HEADER)
            && sent != message.message_type
        {
            return Err(VerifyError::MalformedHeader(Self::TYPE_HEADER.to_owned()));
        }
        if !source.topics.contains(&message.topic_arn) {
            return Err(VerifyError::Rejected(
                "the topic is not one of the source's `topics`".to_owned(),
            ));
        }
        let certificate_url = message.signing_cert_url.trim();
        let is_pem = url::Url::parse(certificate_url)
            .is_ok_and(|url| url.path().ends_with(".pem") && url.query().is_none());
        if !Self::is_sns_url(certificate_url) || !is_pem {
            return Err(VerifyError::Rejected(
                "`SigningCertURL` is not an SNS certificate URL".to_owned(),
            ));
        }
        let timestamp = DateTime::parse_from_rfc3339(message.timestamp.trim())
            .map(|timestamp| timestamp.with_timezone(&Utc))
            .map_err(|_| VerifyError::MalformedBody("`Timestamp`".to_owned()))?;
        let keys = request.keys().await?;
        let key = keys.certificate(certificate_url, request.now()).await?;
        Self::verify_signature(&message, &key)?;
        if let Some(secs) = source.tolerance_secs {
            Tolerance::at(request.now(), secs).check(timestamp)?;
        }
        if source.auto_confirm {
            match message.message_type.as_str() {
                "SubscriptionConfirmation" => {
                    let url = message.subscribe_url.as_deref().unwrap_or_default().trim();
                    if !Self::is_sns_url(url) {
                        return Err(VerifyError::Rejected(
                            "`SubscribeURL` is not an SNS URL".to_owned(),
                        ));
                    }
                    let confirmed = keys
                        .client()
                        .get(url)
                        .send()
                        .await
                        .is_ok_and(|response| response.is_success());
                    if !confirmed {
                        return Err(VerifyError::Unavailable(
                            "the subscription confirmation".to_owned(),
                        ));
                    }
                    tracing::info!(
                        target: "navree::webhooks",
                        topic = %message.topic_arn,
                        "confirmed an SNS subscription"
                    );
                    return Ok(Verdict::Handshake(Handshake::ok()));
                }
                "UnsubscribeConfirmation" => return Ok(Verdict::Handshake(Handshake::ok())),
                _ => {}
            }
        }
        Ok(Verdict::Verified(Verified {
            id: Some(message.message_id),
            timestamp: Some(timestamp),
            event_type: Some(message.message_type),
        }))
    }

    fn missing_setting(source: &SourceOptions) -> Option<&'static str> {
        source.topics.is_empty().then_some("topics")
    }
}

impl WebhookSource for AwsSns {
    const NAME: &'static str = "aws_sns";
    type Verifier = AwsSns;
}

/// An SNS message: the JSON body SNS posts.
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
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use http::{HeaderMap, Method};
    use navree_http_client::{FakeResponse, FakeServer, HttpClient};
    use serde_json::json;

    use super::*;
    use crate::keys::SenderKeys;

    const TOPIC: &str = "arn:aws:sns:us-east-1:123456789012:ses-events";
    const CERT_URL: &str = "https://sns.us-east-1.amazonaws.com/SimpleNotificationService-9c6465fa7f48f5cacd23014631ec1136.pem";
    const CERTIFICATE: &str = include_str!("../../tests/fixtures/sources/signer-cert.pem");
    const MESSAGES: &str = include_str!("../../tests/fixtures/sources/sns-messages.json");

    fn fixture(name: &str) -> Value {
        let messages: Value = serde_json::from_str(MESSAGES).unwrap();
        messages[name].clone()
    }

    fn body(message: &Value) -> Vec<u8> {
        serde_json::to_vec(message).unwrap()
    }

    fn source() -> SourceOptions {
        SourceOptions {
            topics: vec![TOPIC.to_owned()],
            ..SourceOptions::default()
        }
    }

    /// Sender keys on a fake client that serves the certificate.
    fn fake_keys() -> (Arc<SenderKeys>, FakeServer) {
        let (client, server) = HttpClient::fake();
        server.on(
            Method::GET,
            CERT_URL,
            FakeResponse::new(200).text(CERTIFICATE),
        );
        server.on(
            Method::GET,
            "https://sns.us-east-1.amazonaws.com/?Action=ConfirmSubscription*",
            FakeResponse::new(200).text("<ConfirmSubscriptionResponse/>"),
        );
        (Arc::new(SenderKeys::new(client)), server)
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-26T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    async fn verify(
        source: &SourceOptions,
        body: &[u8],
        keys: &Arc<SenderKeys>,
    ) -> Result<Verdict, VerifyError> {
        let headers = HeaderMap::new();
        let request = IncomingRequest::new(source, &headers, body)
            .at(now())
            .with_keys(keys.clone());
        AwsSns::verify_incoming(&request).await
    }

    #[tokio::test]
    async fn notifications_signed_with_sha1_and_sha256_verify() {
        let (keys, server) = fake_keys();
        for name in [
            "notification_v1",
            "notification_v2",
            "notification_without_subject_v1",
        ] {
            let message = fixture(name);
            let verdict = verify(&source(), &body(&message), &keys).await;
            let Ok(Verdict::Verified(verified)) = verdict else {
                panic!("{name}: {verdict:?}");
            };
            assert_eq!(verified.id.as_deref(), message["MessageId"].as_str());
            assert_eq!(verified.event_type.as_deref(), Some("Notification"));
            assert_eq!(
                verified.timestamp.unwrap().to_rfc3339(),
                DateTime::parse_from_rfc3339(message["Timestamp"].as_str().unwrap())
                    .unwrap()
                    .to_rfc3339()
            );
        }
        // The certificate was fetched once.
        server.assert_sent_count(1);
        let parsed: SnsMessage = serde_json::from_value(fixture("notification_v1")).unwrap();
        let event: Value = parsed.message_json().unwrap();
        assert_eq!(event["eventType"], "Bounce");
    }

    #[tokio::test]
    async fn tampered_messages_fail() {
        let (keys, _server) = fake_keys();
        for (field, value) in [
            ("Message", json!("{}")),
            ("Subject", json!("Another subject")),
            ("Timestamp", json!("2026-10-26T07:59:31.000Z")),
            ("Type", json!("SubscriptionConfirmation")),
            ("SignatureVersion", json!("2")),
        ] {
            let mut message = fixture("notification_v1");
            message[field] = value;
            assert_eq!(
                verify(&source(), &body(&message), &keys).await,
                Err(VerifyError::SignatureMismatch),
                "{field}"
            );
        }
        let mut message = fixture("notification_v1");
        message["SignatureVersion"] = json!("3");
        assert_eq!(
            verify(&source(), &body(&message), &keys).await,
            Err(VerifyError::MalformedBody("`SignatureVersion`".to_owned()))
        );
        assert!(matches!(
            verify(&source(), b"not json", &keys).await,
            Err(VerifyError::MalformedBody(_))
        ));
    }

    #[tokio::test]
    async fn other_topics_and_certificate_hosts_are_refused_before_any_fetch() {
        let (keys, server) = fake_keys();
        let other_topic = SourceOptions {
            topics: vec!["arn:aws:sns:us-east-1:123456789012:other".to_owned()],
            ..SourceOptions::default()
        };
        assert!(matches!(
            verify(&other_topic, &body(&fixture("notification_v1")), &keys).await,
            Err(VerifyError::Rejected(_))
        ));
        for url in [
            "https://sns.us-east-1.amazonaws.com.evil.example/cert.pem",
            "http://sns.us-east-1.amazonaws.com/cert.pem",
            "https://sns.us-east-1.amazonaws.com/cert.txt",
            "https://sns.us-east-1.amazonaws.com/?Action=Publish&x=.pem",
            "https://attacker.example/sns.us-east-1.amazonaws.com.pem",
        ] {
            let mut message = fixture("notification_v1");
            message["SigningCertURL"] = json!(url);
            assert!(
                matches!(
                    verify(&source(), &body(&message), &keys).await,
                    Err(VerifyError::Rejected(_))
                ),
                "{url}"
            );
        }
        server.assert_nothing_sent();
    }

    #[test]
    fn sns_hosts_are_recognized() {
        for url in [
            "https://sns.us-east-1.amazonaws.com/x.pem",
            "https://sns.eu-central-2.amazonaws.com/?Action=ConfirmSubscription",
            "https://sns.cn-north-1.amazonaws.com.cn/x.pem",
        ] {
            assert!(AwsSns::is_sns_url(url), "{url}");
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
            "not a url",
        ] {
            assert!(!AwsSns::is_sns_url(url), "{url}");
        }
    }

    #[tokio::test]
    async fn subscriptions_are_confirmed_without_the_handler() {
        for name in [
            "subscription_confirmation_v1",
            "subscription_confirmation_v2",
        ] {
            let (keys, server) = fake_keys();
            assert_eq!(
                verify(&source(), &body(&fixture(name)), &keys).await,
                Ok(Verdict::Handshake(Handshake::ok())),
                "{name}"
            );
            server.assert_sent(|request| {
                request.method() == Method::GET
                    && request
                        .url()
                        .as_str()
                        .contains("Action=ConfirmSubscription")
            });
        }

        // Unsubscribing is acknowledged, not confirmed.
        let (keys, server) = fake_keys();
        assert_eq!(
            verify(
                &source(),
                &body(&fixture("unsubscribe_confirmation_v2")),
                &keys
            )
            .await,
            Ok(Verdict::Handshake(Handshake::ok()))
        );
        server.assert_not_sent(|request| request.url().as_str().contains("Action="));

        // With auto_confirm off, the handler gets the confirmation.
        let (keys, server) = fake_keys();
        let manual = SourceOptions {
            auto_confirm: false,
            ..source()
        };
        let verdict = verify(
            &manual,
            &body(&fixture("subscription_confirmation_v1")),
            &keys,
        )
        .await;
        let Ok(Verdict::Verified(verified)) = verdict else {
            panic!("{verdict:?}");
        };
        assert_eq!(
            verified.event_type.as_deref(),
            Some("SubscriptionConfirmation")
        );
        server.assert_not_sent(|request| request.url().as_str().contains("Action="));
    }

    #[tokio::test]
    async fn unreachable_certificates_and_confirmations_are_unavailable() {
        let (client, server) = HttpClient::fake();
        server.on(Method::GET, CERT_URL, FakeResponse::new(503));
        let keys = Arc::new(SenderKeys::new(client));
        assert_eq!(
            verify(&source(), &body(&fixture("notification_v1")), &keys).await,
            Err(VerifyError::Unavailable(
                "the signing certificate".to_owned()
            ))
        );

        let (client, server) = HttpClient::fake();
        server.on(
            Method::GET,
            CERT_URL,
            FakeResponse::new(200).text(CERTIFICATE),
        );
        server.on(
            Method::GET,
            "https://sns.us-east-1.amazonaws.com/?Action=*",
            FakeResponse::new(500),
        );
        let keys = Arc::new(SenderKeys::new(client));
        assert_eq!(
            verify(
                &source(),
                &body(&fixture("subscription_confirmation_v2")),
                &keys
            )
            .await,
            Err(VerifyError::Unavailable(
                "the subscription confirmation".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn the_timestamp_is_checked_against_the_sources_own_tolerance() {
        let (keys, _server) = fake_keys();
        let message = body(&fixture("notification_v2"));
        // Published 30 s before now.
        let strict = SourceOptions {
            tolerance_secs: Some(10),
            ..source()
        };
        assert_eq!(
            verify(&strict, &message, &keys).await,
            Err(VerifyError::TimestampOutOfTolerance { tolerance_secs: 10 })
        );
        let lenient = SourceOptions {
            tolerance_secs: Some(60),
            ..source()
        };
        assert!(verify(&lenient, &message, &keys).await.is_ok());
    }

    #[tokio::test]
    async fn the_type_header_must_match_the_message() {
        let (keys, _server) = fake_keys();
        let mut headers = HeaderMap::new();
        headers.insert(
            AwsSns::TYPE_HEADER,
            "SubscriptionConfirmation".parse().unwrap(),
        );
        let source = source();
        let message = body(&fixture("notification_v1"));
        let request = IncomingRequest::new(&source, &headers, &message)
            .at(now())
            .with_keys(keys);
        assert_eq!(
            AwsSns::verify_incoming(&request).await,
            Err(VerifyError::MalformedHeader(AwsSns::TYPE_HEADER.to_owned()))
        );
    }

    #[test]
    fn the_source_needs_topics_not_a_secret() {
        assert_eq!(
            AwsSns::missing_setting(&SourceOptions::default()),
            Some("topics")
        );
        assert_eq!(AwsSns::missing_setting(&source()), None);
    }

    #[test]
    fn the_string_to_sign_follows_the_message_type() {
        let notification: SnsMessage =
            serde_json::from_value(fixture("notification_without_subject_v1")).unwrap();
        assert!(
            notification
                .string_to_sign()
                .starts_with("Message\n{\"eventType\"")
        );
        assert!(!notification.string_to_sign().contains("Subject\n"));
        let confirmation: SnsMessage =
            serde_json::from_value(fixture("subscription_confirmation_v1")).unwrap();
        let text = confirmation.string_to_sign();
        assert!(text.contains("\nSubscribeURL\nhttps://sns.us-east-1.amazonaws.com/"));
        assert!(text.contains("\nToken\n2336412f"));
        assert!(text.ends_with("Type\nSubscriptionConfirmation\n"));
    }
}
