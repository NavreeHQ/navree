//! `navree_crypto::sns`: messages signed the way SNS signs version 2 (RSA
//! PKCS #1 v1.5 with SHA-256) with the RSA test key, and checked with the
//! key of its certificate. Version 1 (SHA-1), which `ring` cannot sign, is
//! covered by the OpenSSL-signed messages of navree-webhooks and
//! navree-mail.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use navree_crypto::jose::{SigningKey, VerifyingKey};
use navree_crypto::sns::{SnsMessage, SnsSignatureError};

use crate::jose::fixture;

/// An SES bounce, as SNS delivers it.
fn notification() -> SnsMessage {
    SnsMessage {
        message_type: "Notification".to_owned(),
        message_id: "5a1c2f3e-9d8b-4c7a-b6e5-f4d3c2b1a090".to_owned(),
        topic_arn: "arn:aws:sns:eu-west-1:123456789012:ses-events".to_owned(),
        subject: Some("Amazon SES Email Event Notification".to_owned()),
        message: r#"{"eventType":"Bounce","bounce":{"bounceType":"Permanent"},"mail":{"messageId":"0102018c"}}"#
            .to_owned(),
        timestamp: "2026-10-27T10:00:02.000Z".to_owned(),
        signing_cert_url: "https://sns.eu-west-1.amazonaws.com/SimpleNotificationService-0000.pem"
            .to_owned(),
        ..SnsMessage::default()
    }
}

/// `message` with a version 2 signature by the RSA test key.
fn signed(mut message: SnsMessage) -> SnsMessage {
    let key = SigningKey::from_pem(&fixture("rsa-2048.pem")).unwrap();
    message.signature_version = "2".to_owned();
    let signature = key.sign(message.string_to_sign().as_bytes()).unwrap();
    message.signature = STANDARD.encode(signature);
    message
}

/// The key of the test key's certificate, as a receiver reads it.
fn certificate() -> VerifyingKey {
    VerifyingKey::from_pem(&fixture("rsa-2048-cert.pem")).unwrap()
}

#[test]
fn a_version_2_signature_verifies_with_the_certificates_key() {
    let message = signed(notification());
    assert_eq!(message.verify_signature(&certificate()), Ok(()));

    // The body round-trips through JSON as SNS sends it.
    let body = serde_json::to_string(&message).unwrap();
    let read: SnsMessage = serde_json::from_str(&body).unwrap();
    assert_eq!(read.verify_signature(&certificate()), Ok(()));
    let event: serde_json::Value = read.message_json().unwrap();
    assert_eq!(event["eventType"], "Bounce");
}

#[test]
fn confirmations_sign_their_subscribe_url_and_token() {
    let confirmation = signed(SnsMessage {
        message_type: "SubscriptionConfirmation".to_owned(),
        subject: None,
        subscribe_url: Some(
            "https://sns.eu-west-1.amazonaws.com/?Action=ConfirmSubscription&Token=2336412f"
                .to_owned(),
        ),
        token: Some("2336412f".to_owned()),
        ..notification()
    });
    assert_eq!(confirmation.verify_signature(&certificate()), Ok(()));
    let elsewhere = SnsMessage {
        subscribe_url: Some("https://sns.eu-west-1.amazonaws.com/?Action=Other".to_owned()),
        ..confirmation
    };
    assert_eq!(
        elsewhere.verify_signature(&certificate()),
        Err(SnsSignatureError::Mismatch)
    );
}

#[test]
fn a_tampered_message_is_refused() {
    let message = signed(notification());
    let tampered = [
        SnsMessage {
            message: message.message.replace("Permanent", "Transient"),
            ..message.clone()
        },
        SnsMessage {
            message_id: "another-id".to_owned(),
            ..message.clone()
        },
        SnsMessage {
            subject: None,
            ..message.clone()
        },
        SnsMessage {
            timestamp: "2026-10-27T11:00:02.000Z".to_owned(),
            ..message.clone()
        },
        SnsMessage {
            topic_arn: "arn:aws:sns:eu-west-1:999999999999:ses-events".to_owned(),
            ..message.clone()
        },
        SnsMessage {
            message_type: "UnsubscribeConfirmation".to_owned(),
            ..message.clone()
        },
    ];
    for message in tampered {
        assert_eq!(
            message.verify_signature(&certificate()),
            Err(SnsSignatureError::Mismatch),
            "{message:?}"
        );
    }
}

#[test]
fn another_key_or_version_is_refused() {
    let message = signed(notification());

    // Signed by another key.
    let other = SigningKey::generate_es256();
    let forged = SnsMessage {
        signature: STANDARD.encode(other.sign(message.string_to_sign().as_bytes()).unwrap()),
        ..message.clone()
    };
    assert_eq!(
        forged.verify_signature(&certificate()),
        Err(SnsSignatureError::Mismatch)
    );

    // Checked with another certificate's key.
    let p256 = VerifyingKey::from_pem(&fixture("p256-cert.pem")).unwrap();
    assert_eq!(
        message.verify_signature(&p256),
        Err(SnsSignatureError::Mismatch)
    );

    // The version is signed for: a SHA-256 signature is no SHA-1 one, and
    // other versions are unknown.
    let as_version_1 = SnsMessage {
        signature_version: "1".to_owned(),
        ..message.clone()
    };
    assert_eq!(
        as_version_1.verify_signature(&certificate()),
        Err(SnsSignatureError::Mismatch)
    );
    for version in ["3", "", "SHA256"] {
        let unknown = SnsMessage {
            signature_version: version.to_owned(),
            ..message.clone()
        };
        assert_eq!(
            unknown.verify_signature(&certificate()),
            Err(SnsSignatureError::UnsupportedVersion),
            "{version}"
        );
    }
    let garbled = SnsMessage {
        signature: "not base64!".to_owned(),
        ..message
    };
    assert_eq!(
        garbled.verify_signature(&certificate()),
        Err(SnsSignatureError::MalformedSignature)
    );
}
