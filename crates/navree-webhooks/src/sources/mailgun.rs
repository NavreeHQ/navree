//! [`Mailgun`]: the `signature` of the body, HMAC-SHA256 over the timestamp
//! and the token.

use http::HeaderMap;
use serde_json::Value;

use crate::options::{SignatureEncoding, SourceOptions};
use crate::signing;
use crate::verify::{
    Tolerance, Verified, VerifyError, WebhookSource, WebhookVerifier, any_matches, check_age,
    parse_timestamp, require_secret,
};

/// Mailgun's scheme: the body carries `timestamp`, `token` and
/// `signature`, the hex HMAC-SHA256 of `{timestamp}{token}` keyed with the
/// webhook signing key. Event webhooks send them as the JSON `signature`
/// object; routes (inbound mail) and older webhooks as form fields. The id
/// is `event-data.id` (else the token), the event type `event-data.event`
/// (else the form's `event`). Source name `mailgun`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Mailgun;

/// The signature fields of a Mailgun body.
struct Signed {
    timestamp: String,
    token: String,
    signature: String,
    id: Option<String>,
    event_type: Option<String>,
}

impl Mailgun {
    /// The `signature` of `token` sent at `timestamp`.
    pub fn sign(signing_key: &str, timestamp: i64, token: &str) -> String {
        hex::encode(signing::hmac_sha256(
            signing_key.as_bytes(),
            &[timestamp.to_string().as_bytes(), token.as_bytes()],
        ))
    }

    /// Verifies a JSON or form body signed with `signing_key` (see
    /// [`StandardWebhooks::verify`](crate::StandardWebhooks::verify) for
    /// `tolerance`).
    pub fn verify(
        signing_key: &str,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        require_secret(signing_key)?;
        let signed = signed_fields(body)
            .ok_or_else(|| VerifyError::MalformedBody("Mailgun `signature`".to_owned()))?;
        let timestamp = parse_timestamp(&signed.timestamp, "timestamp")
            .map_err(|_| VerifyError::MalformedBody("`timestamp`".to_owned()))?;
        let expected = signing::hmac_sha256(
            signing_key.as_bytes(),
            &[signed.timestamp.trim().as_bytes(), signed.token.as_bytes()],
        );
        if !any_matches([signed.signature.as_str()], SignatureEncoding::Hex, &expected) {
            return Err(VerifyError::SignatureMismatch);
        }
        check_age(timestamp, tolerance)?;
        Ok(Verified {
            id: signed.id.or(Some(signed.token)),
            timestamp: Some(timestamp),
            event_type: signed.event_type,
        })
    }
}

impl WebhookVerifier for Mailgun {
    fn verify_request(
        source: &SourceOptions,
        _headers: &HeaderMap,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        Self::verify(&source.secret, body, tolerance)
    }
}

impl WebhookSource for Mailgun {
    const NAME: &'static str = "mailgun";
    type Verifier = Mailgun;
}

/// The signature fields of a JSON body (`signature` object, `event-data`)
/// or of a form body.
fn signed_fields(body: &[u8]) -> Option<Signed> {
    if let Ok(Value::Object(payload)) = serde_json::from_slice::<Value>(body) {
        let signature = payload.get("signature")?;
        let text = |value: Option<&Value>| match value? {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        };
        let event = payload.get("event-data");
        return Some(Signed {
            timestamp: text(signature.get("timestamp"))?,
            token: text(signature.get("token"))?,
            signature: text(signature.get("signature"))?,
            id: text(event.and_then(|event| event.get("id"))),
            event_type: text(event.and_then(|event| event.get("event"))),
        });
    }
    let field = |name: &str| {
        form_urlencoded::parse(body)
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    Some(Signed {
        timestamp: field("timestamp")?,
        token: field("token")?,
        signature: field("signature")?,
        id: None,
        event_type: field("event"),
    })
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use serde_json::json;

    use super::*;

    const KEY: &str = "key-7f2e5b1c9d8a4e3f6b0a1c2d3e4f5a6b";
    const TIMESTAMP: i64 = 1_792_000_000;
    const TOKEN: &str = "a8ce0edb2dd8301dee6c2405235584e45aa91d1e9f979f3de0";

    fn at(secs: i64) -> DateTime<chrono::Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn event(signature: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "signature": {
                "timestamp": TIMESTAMP.to_string(),
                "token": TOKEN,
                "signature": signature,
            },
            "event-data": {
                "id": "CPgfbmQMTCKtHW6uIWtuVe",
                "event": "delivered",
                "recipient": "ada@example.com",
            },
        }))
        .unwrap()
    }

    #[test]
    fn signatures_match_an_independent_hmac() {
        // `printf '%s%s' 1792000000 $TOKEN | openssl dgst -sha256 -hmac $KEY`
        assert_eq!(
            Mailgun::sign(KEY, TIMESTAMP, TOKEN),
            "25ebc31c13b0acdc658d433f4ebc4caf562d1924921dfbaf5a291726734cf249"
        );
    }

    #[test]
    fn event_webhooks_verify_from_their_json_signature() {
        let body = event(&Mailgun::sign(KEY, TIMESTAMP, TOKEN));
        let verified =
            Mailgun::verify(KEY, &body, Some(Tolerance::at(at(TIMESTAMP + 60), 300))).unwrap();
        assert_eq!(verified.id.as_deref(), Some("CPgfbmQMTCKtHW6uIWtuVe"));
        assert_eq!(verified.event_type.as_deref(), Some("delivered"));
        assert_eq!(verified.timestamp, Some(at(TIMESTAMP)));
        assert_eq!(
            Mailgun::verify("key-other", &body, None),
            Err(VerifyError::SignatureMismatch)
        );
        assert_eq!(
            Mailgun::verify(KEY, &body, Some(Tolerance::at(at(TIMESTAMP + 301), 300))),
            Err(VerifyError::TimestampOutOfTolerance {
                tolerance_secs: 300
            })
        );
        // The token is signed: another one fails.
        let swapped = String::from_utf8(body)
            .unwrap()
            .replace(TOKEN, "b8ce0edb2dd8301dee6c2405235584e45aa91d1e9f979f3de0");
        assert_eq!(
            Mailgun::verify(KEY, swapped.as_bytes(), None),
            Err(VerifyError::SignatureMismatch)
        );
    }

    #[test]
    fn routes_verify_from_form_fields() {
        let signature = Mailgun::sign(KEY, TIMESTAMP, TOKEN);
        let body = form_urlencoded::Serializer::new(String::new())
            .append_pair("recipient", "reply+42@in.example.com")
            .append_pair("subject", "Re: your ticket")
            .append_pair("timestamp", &TIMESTAMP.to_string())
            .append_pair("token", TOKEN)
            .append_pair("signature", &signature)
            .finish();
        let verified = Mailgun::verify(KEY, body.as_bytes(), None).unwrap();
        assert_eq!(verified.id.as_deref(), Some(TOKEN));
        assert_eq!(verified.event_type, None);
    }

    #[test]
    fn bodies_without_a_signature_are_malformed() {
        for body in [&br#"{"event-data":{}}"#[..], b"recipient=a", b""] {
            assert!(matches!(
                Mailgun::verify(KEY, body, None),
                Err(VerifyError::MalformedBody(_))
            ));
        }
        assert_eq!(
            Mailgun::verify("", b"", None),
            Err(VerifyError::MissingSecret)
        );
    }
}
