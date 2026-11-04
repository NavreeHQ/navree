//! Amazon SNS message signatures: [`SnsMessage`], the text SNS signs, and
//! the checks every receiver of SNS messages makes before it trusts one.
//!
//! SNS signs each message with RSA: SHA-1 for `SignatureVersion` 1 and
//! SHA-256 for version 2, with the key of the certificate at
//! `SigningCertURL`. A receiver:
//!
//! 1. reads the body as an [`SnsMessage`];
//! 2. checks that `TopicArn` is one of its own topics (SNS signs every
//!    topic's messages with the same keys, so a valid signature alone only
//!    says that some topic sent it);
//! 3. checks `SigningCertURL` with [`is_signing_cert_url`] before it fetches
//!    anything: HTTPS on an SNS host (`sns.<region>.amazonaws.com`, or
//!    `.amazonaws.com.cn`), a `.pem` path and no query;
//! 4. fetches the certificate (and caches it), reads its key with
//!    [`VerifyingKey::from_pem`](crate::jose::VerifyingKey::from_pem), and
//!    calls [`SnsMessage::verify_signature`];
//! 5. checks `Timestamp`: SNS keeps retrying a delivery for at most an
//!    hour, with the same timestamp and signature.
//!
//! Fetching is left to the caller, which owns the HTTP client.
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

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
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
