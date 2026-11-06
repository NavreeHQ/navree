//! [`Slack`]: `X-Slack-Signature` (`v0=`), HMAC-SHA256 over the version,
//! the timestamp and the body.

use http::HeaderMap;
use serde_json::Value;

use crate::options::{SignatureEncoding, SourceOptions};
use crate::request::{Handshake, IncomingRequest, Verdict};
use crate::signing;
use crate::verify::{
    Tolerance, Verified, VerifyError, WebhookSource, WebhookVerifier, any_matches, check_age,
    header, parse_timestamp, require_secret,
};

/// Slack's scheme: `X-Slack-Signature: v0=<hex>`, HMAC-SHA256 of
/// `v0:{X-Slack-Request-Timestamp}:{body}` keyed with the app's signing
/// secret. The id is the Events API `event_id`, the event type the inner
/// `event.type` (else the payload's `type`). An Events API
/// `url_verification` request is answered with its `challenge` without
/// running the handler. Source name `slack`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Slack;

impl Slack {
    /// The signature header.
    pub const SIGNATURE_HEADER: &'static str = "x-slack-signature";
    /// The timestamp header (Unix seconds).
    pub const TIMESTAMP_HEADER: &'static str = "x-slack-request-timestamp";

    /// The `X-Slack-Signature` value (`v0=<hex>`) of `body` sent at
    /// `timestamp`.
    pub fn sign(signing_secret: &str, timestamp: i64, body: &[u8]) -> String {
        let timestamp = timestamp.to_string();
        let mac = signing::hmac_sha256(
            signing_secret.as_bytes(),
            &[b"v0:", timestamp.as_bytes(), b":", body],
        );
        format!("v0={}", hex::encode(mac))
    }

    /// Verifies a request signed with `signing_secret` (see
    /// [`StandardWebhooks::verify`](crate::StandardWebhooks::verify) for
    /// `tolerance`; Slack asks for five minutes).
    pub fn verify(
        signing_secret: &str,
        headers: &HeaderMap,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        require_secret(signing_secret)?;
        let raw_timestamp = header(headers, Self::TIMESTAMP_HEADER)?.trim();
        let timestamp = parse_timestamp(raw_timestamp, Self::TIMESTAMP_HEADER)?;
        let signature = header(headers, Self::SIGNATURE_HEADER)?
            .trim()
            .strip_prefix("v0=")
            .ok_or_else(|| VerifyError::MalformedHeader(Self::SIGNATURE_HEADER.to_owned()))?;
        let expected = signing::hmac_sha256(
            signing_secret.as_bytes(),
            &[b"v0:", raw_timestamp.as_bytes(), b":", body],
        );
        if !any_matches([signature], SignatureEncoding::Hex, &expected) {
            return Err(VerifyError::SignatureMismatch);
        }
        check_age(timestamp, tolerance)?;
        let payload = serde_json::from_slice::<Value>(body).ok();
        let text = |value: Option<&Value>| value.and_then(Value::as_str).map(str::to_owned);
        Ok(Verified {
            id: text(payload.as_ref().and_then(|p| p.get("event_id"))),
            timestamp: Some(timestamp),
            event_type: text(payload.as_ref().and_then(|p| p.pointer("/event/type")))
                .or_else(|| text(payload.as_ref().and_then(|p| p.get("type")))),
        })
    }
}

impl WebhookVerifier for Slack {
    fn verify_request(
        source: &SourceOptions,
        headers: &HeaderMap,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        Self::verify(source.secret.expose(), headers, body, tolerance)
    }

    async fn verify_incoming(request: &IncomingRequest<'_>) -> Result<Verdict, VerifyError> {
        let verified = Self::verify(
            request.source().secret.expose(),
            request.headers(),
            request.body(),
            request.tolerance(),
        )?;
        if verified.event_type.as_deref() == Some("url_verification") {
            let challenge = serde_json::from_slice::<Value>(request.body())
                .ok()
                .and_then(|payload| payload.get("challenge")?.as_str().map(str::to_owned))
                .ok_or_else(|| VerifyError::MalformedBody("`challenge`".to_owned()))?;
            return Ok(Verdict::Handshake(Handshake::text(challenge)));
        }
        Ok(Verdict::Verified(verified))
    }
}

impl WebhookSource for Slack {
    const NAME: &'static str = "slack";
    type Verifier = Slack;
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use http::HeaderValue;

    use super::*;
    use crate::sources::block_on;

    /// Slack's documented example ("Verifying requests from Slack").
    const SECRET: &str = "8f742231b10e8888abcd99yyyzzz85a5";
    const TIMESTAMP: i64 = 1_531_420_618;
    const BODY: &str = "token=xyzz0WbapA4vBCDEFasx0q6G&team_id=T1DC2JH3J&team_domain=testteamnow&channel_id=G8PSS9T3V&channel_name=foobar&user_id=U2CERLKJA&user_name=roadrunner&command=%2Fwebhook-collect&text=&response_url=https%3A%2F%2Fhooks.slack.com%2Fcommands%2FT1DC2JH3J%2F397700885554%2F96rGlfmibIGlgcZRskXaIFfN&trigger_id=398738663015.47445629121.803a0bc887a14d10d2c447fce8b6703c";
    const SIGNATURE: &str = "v0=a2114d57b48eac39b9ad189dd8316235a7b4a8d21a10bd27519666489c69b503";

    fn signed(signature: &str, timestamp: i64) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-slack-signature",
            HeaderValue::from_str(signature).unwrap(),
        );
        headers.insert(
            "x-slack-request-timestamp",
            HeaderValue::from_str(&timestamp.to_string()).unwrap(),
        );
        headers
    }

    fn at(secs: i64) -> DateTime<chrono::Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    #[test]
    fn the_documented_example_signs_and_verifies() {
        assert_eq!(Slack::sign(SECRET, TIMESTAMP, BODY.as_bytes()), SIGNATURE);
        let verified = Slack::verify(
            SECRET,
            &signed(SIGNATURE, TIMESTAMP),
            BODY.as_bytes(),
            Some(Tolerance::at(at(TIMESTAMP + 299), 300)),
        )
        .unwrap();
        assert_eq!(verified.timestamp, Some(at(TIMESTAMP)));
        assert_eq!(verified.id, None);
    }

    #[test]
    fn tampering_old_requests_and_other_versions_fail() {
        let headers = signed(SIGNATURE, TIMESTAMP);
        assert_eq!(
            Slack::verify(SECRET, &headers, b"token=other", None),
            Err(VerifyError::SignatureMismatch)
        );
        assert_eq!(
            Slack::verify(
                SECRET,
                &signed(SIGNATURE, TIMESTAMP + 1),
                BODY.as_bytes(),
                None
            ),
            Err(VerifyError::SignatureMismatch)
        );
        assert_eq!(
            Slack::verify(
                SECRET,
                &headers,
                BODY.as_bytes(),
                Some(Tolerance::at(at(TIMESTAMP + 301), 300))
            ),
            Err(VerifyError::TimestampOutOfTolerance {
                tolerance_secs: 300
            })
        );
        assert_eq!(
            Slack::verify(
                SECRET,
                &signed(&SIGNATURE.replace("v0=", "v1="), TIMESTAMP),
                BODY.as_bytes(),
                None
            ),
            Err(VerifyError::MalformedHeader("x-slack-signature".to_owned()))
        );
    }

    #[test]
    fn events_carry_their_id_and_inner_type() {
        let body = br#"{"type":"event_callback","event_id":"Ev0PV52K21","event":{"type":"app_mention","text":"hi"}}"#;
        let headers = signed(&Slack::sign(SECRET, TIMESTAMP, body), TIMESTAMP);
        let verified = Slack::verify(SECRET, &headers, body, None).unwrap();
        assert_eq!(verified.id.as_deref(), Some("Ev0PV52K21"));
        assert_eq!(verified.event_type.as_deref(), Some("app_mention"));
    }

    #[test]
    fn url_verification_is_answered_with_its_challenge() {
        let body = br#"{"token":"Jhj5dZrVaK7ZwHHjRyZWjbDl","challenge":"3eZbrw1aBm2rZgRNFdxV2595E9CY3gmdALWMmHkvFXO7tYXAYM8P","type":"url_verification"}"#;
        let headers = signed(&Slack::sign(SECRET, TIMESTAMP, body), TIMESTAMP);
        let source = SourceOptions::new(SECRET);
        let request = IncomingRequest::new(&source, &headers, body);
        assert_eq!(
            block_on(Slack::verify_incoming(&request)),
            Ok(Verdict::Handshake(Handshake::text(
                "3eZbrw1aBm2rZgRNFdxV2595E9CY3gmdALWMmHkvFXO7tYXAYM8P"
            )))
        );
        // Unsigned, it is refused like any other request.
        let unsigned = signed("v0=00", TIMESTAMP);
        let request = IncomingRequest::new(&source, &unsigned, body);
        assert_eq!(
            block_on(Slack::verify_incoming(&request)),
            Err(VerifyError::SignatureMismatch)
        );
    }
}
