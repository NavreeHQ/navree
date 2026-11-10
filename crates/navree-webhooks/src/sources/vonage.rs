//! [`Vonage`]: signed webhooks, a JWT (HS256) in `Authorization` that
//! carries the body's SHA-256.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::DateTime;
use http::HeaderMap;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::options::SourceOptions;
use crate::signing;
use crate::verify::{
    Tolerance, Verified, VerifyError, WebhookSource, WebhookVerifier, body_fields, check_age,
    header, require_secret,
};

/// Vonage's signed webhooks (the Messages API's status and inbound
/// webhooks): `Authorization: Bearer <JWT>`, signed HS256 with the
/// account's signature secret, whose `payload_hash` claim is the hex
/// SHA-256 of the body and whose `iat` is checked against the tolerance.
/// The id is the token's `jti`, the event type the body's `status` (status
/// webhooks) or `message_type` (inbound messages). Source name `vonage`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Vonage;

impl Vonage {
    /// The header carrying the token.
    pub const AUTHORIZATION_HEADER: &'static str = "authorization";

    /// The `Authorization` value Vonage sends for `body` at `iat` (Unix
    /// seconds), with token id `jti`.
    pub fn sign(signature_secret: &str, body: &[u8], iat: i64, jti: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(json!({ "alg": "HS256", "typ": "JWT" }).to_string());
        let claims = json!({
            "iat": iat,
            "jti": jti,
            "iss": "Vonage",
            "payload_hash": hex::encode(Sha256::digest(body)),
        });
        let claims = URL_SAFE_NO_PAD.encode(claims.to_string());
        let input = format!("{header}.{claims}");
        let signature = signing::hmac_sha256(signature_secret.as_bytes(), &[input.as_bytes()]);
        format!("Bearer {input}.{}", URL_SAFE_NO_PAD.encode(signature))
    }

    /// Verifies a request signed with `signature_secret`. With `tolerance`,
    /// `iat` must be within its window.
    pub fn verify(
        signature_secret: &str,
        headers: &HeaderMap,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        require_secret(signature_secret)?;
        let malformed = || VerifyError::MalformedHeader(Self::AUTHORIZATION_HEADER.to_owned());
        let token = header(headers, Self::AUTHORIZATION_HEADER)?
            .trim()
            .strip_prefix("Bearer ")
            .ok_or_else(malformed)?
            .trim();
        let mut parts = token.split('.');
        let (Some(encoded_header), Some(encoded_claims), Some(encoded_signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(malformed());
        };
        let decode = |part: &str| URL_SAFE_NO_PAD.decode(part).map_err(|_| malformed());
        let jwt_header: Value =
            serde_json::from_slice(&decode(encoded_header)?).map_err(|_| malformed())?;
        if jwt_header["alg"] != "HS256" {
            return Err(malformed());
        }
        let expected = signing::hmac_sha256(
            signature_secret.as_bytes(),
            &[encoded_header.as_bytes(), b".", encoded_claims.as_bytes()],
        );
        if !signing::same(&decode(encoded_signature)?, &expected) {
            return Err(VerifyError::SignatureMismatch);
        }
        let claims: Value =
            serde_json::from_slice(&decode(encoded_claims)?).map_err(|_| malformed())?;
        let hash = claims["payload_hash"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !signing::same(
            hash.as_bytes(),
            hex::encode(Sha256::digest(body)).as_bytes(),
        ) {
            return Err(VerifyError::SignatureMismatch);
        }
        let timestamp = claims["iat"]
            .as_i64()
            .and_then(|secs| DateTime::from_timestamp(secs, 0))
            .ok_or_else(malformed)?;
        check_age(timestamp, tolerance)?;
        let (_, status) = body_fields(body, "message_uuid", "status");
        let event_type = status.or_else(|| body_fields(body, "message_uuid", "message_type").1);
        Ok(Verified {
            id: claims["jti"].as_str().map(str::to_owned),
            timestamp: Some(timestamp),
            event_type,
        })
    }
}

impl WebhookVerifier for Vonage {
    fn verify_request(
        source: &SourceOptions,
        headers: &HeaderMap,
        body: &[u8],
        tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        Self::verify(source.secret.expose(), headers, body, tolerance)
    }
}

impl WebhookSource for Vonage {
    const NAME: &'static str = "vonage";
    type Verifier = Vonage;
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone as _, Utc};
    use http::HeaderValue;

    use super::*;

    const SECRET: &str = "vonage-signature-secret";
    const IAT: i64 = 1_800_000_000;

    fn headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_str(value).unwrap());
        headers
    }

    fn at(secs: i64) -> Option<Tolerance> {
        Some(Tolerance::at(Utc.timestamp_opt(secs, 0).unwrap(), 300))
    }

    #[test]
    fn a_signed_status_verifies_with_its_jti_and_status() {
        let body = br#"{"message_uuid":"aaaa","status":"delivered","channel":"sms"}"#;
        let auth = Vonage::sign(SECRET, body, IAT, "jti-1");
        let verified = Vonage::verify(SECRET, &headers(&auth), body, at(IAT + 10)).unwrap();
        assert_eq!(verified.id.as_deref(), Some("jti-1"));
        assert_eq!(verified.event_type.as_deref(), Some("delivered"));
        assert_eq!(verified.timestamp.map(|t| t.timestamp()), Some(IAT));

        let inbound = br#"{"message_uuid":"bbbb","message_type":"text","text":"STOP"}"#;
        let auth = Vonage::sign(SECRET, inbound, IAT, "jti-2");
        let verified = Vonage::verify(SECRET, &headers(&auth), inbound, None).unwrap();
        assert_eq!(verified.event_type.as_deref(), Some("text"));
    }

    #[test]
    fn another_body_secret_or_time_fails() {
        let body = br#"{"status":"delivered"}"#;
        let auth = Vonage::sign(SECRET, body, IAT, "j");
        assert_eq!(
            Vonage::verify(SECRET, &headers(&auth), br#"{"status":"read"}"#, None),
            Err(VerifyError::SignatureMismatch),
            "the payload hash covers the body"
        );
        assert_eq!(
            Vonage::verify("other", &headers(&auth), body, None),
            Err(VerifyError::SignatureMismatch)
        );
        assert_eq!(
            Vonage::verify(SECRET, &headers(&auth), body, at(IAT + 3600)),
            Err(VerifyError::TimestampOutOfTolerance {
                tolerance_secs: 300
            })
        );
    }

    #[test]
    fn malformed_tokens_are_refused() {
        let body = b"{}";
        for value in [
            "Basic abc",
            "Bearer a.b",
            "Bearer a.b.c.d",
            "Bearer %%.%%.%%",
        ] {
            assert_eq!(
                Vonage::verify(SECRET, &headers(value), body, None),
                Err(VerifyError::MalformedHeader("authorization".to_owned())),
                "{value}"
            );
        }
        // A token that is not HS256.
        let none = format!(
            "Bearer {}.{}.",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"none"}"#),
            URL_SAFE_NO_PAD.encode(r#"{"iat":1}"#)
        );
        assert_eq!(
            Vonage::verify(SECRET, &headers(&none), body, None),
            Err(VerifyError::MalformedHeader("authorization".to_owned()))
        );
        assert_eq!(
            Vonage::verify(SECRET, &HeaderMap::new(), body, None),
            Err(VerifyError::MissingHeader("authorization".to_owned()))
        );
    }
}
