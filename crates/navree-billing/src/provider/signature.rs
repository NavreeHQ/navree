//! Webhook signatures: `Stripe-Signature` and `Paddle-Signature`, both
//! HMAC-SHA256 over a timestamp and the raw body.
//!
//! - Stripe: `Stripe-Signature: t=<unix>,v1=<hex>[,v1=<hex>...]`, the MAC of
//!   `"{t}.{body}"` keyed with the endpoint's signing secret (`whsec_...`).
//! - Paddle: `Paddle-Signature: ts=<unix>;h1=<hex>[;h1=<hex>...]`, the MAC of
//!   `"{ts}:{body}"` keyed with the destination's secret key.
//!
//! Several signatures are accepted (secret rotation); the comparison is
//! constant-time; the timestamp must be within the tolerance of now, in
//! either direction.

use std::time::Duration;

use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use http::HeaderMap;
use sha2::Sha256;

use crate::provider::WebhookError;

/// The header Stripe signs webhooks in.
pub const STRIPE_SIGNATURE_HEADER: &str = "stripe-signature";
/// The header Paddle signs webhooks in.
pub const PADDLE_SIGNATURE_HEADER: &str = "paddle-signature";

fn mac(secret: &[u8], timestamp: i64, separator: u8, body: &[u8]) -> Hmac<Sha256> {
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(secret).expect("HMAC accepts keys of any length");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(&[separator]);
    mac.update(body);
    mac
}

/// The `Stripe-Signature` header value for `body` signed at `timestamp`
/// (Unix seconds) with `secret`. For tests that post webhooks.
pub fn sign_stripe_webhook(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let digest = mac(secret.as_bytes(), timestamp, b'.', body)
        .finalize()
        .into_bytes();
    format!("t={timestamp},v1={}", hex::encode(digest))
}

/// The `Paddle-Signature` header value for `body` signed at `timestamp`
/// (Unix seconds) with `secret`. For tests that post webhooks.
pub fn sign_paddle_webhook(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let digest = mac(secret.as_bytes(), timestamp, b':', body)
        .finalize()
        .into_bytes();
    format!("ts={timestamp};h1={}", hex::encode(digest))
}

/// A parsed signature header: the timestamp and the candidate MACs.
struct Signed {
    timestamp: i64,
    signatures: Vec<Vec<u8>>,
}

fn header<'a>(headers: &'a HeaderMap, name: &'static str) -> Result<&'a str, WebhookError> {
    headers
        .get(name)
        .ok_or(WebhookError::MissingSignature(name))?
        .to_str()
        .map_err(|_| WebhookError::MalformedSignature(name))
}

/// Parses `t=..,v1=..` (Stripe: `,` and `t`/`v1`) or `ts=..;h1=..`
/// (Paddle: `;` and `ts`/`h1`). Unknown keys (Stripe's `v0`) are ignored.
fn parse(
    value: &str,
    name: &'static str,
    separator: char,
    time_key: &str,
    signature_key: &str,
) -> Result<Signed, WebhookError> {
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for part in value.split(separator) {
        let Some((key, val)) = part.trim().split_once('=') else {
            return Err(WebhookError::MalformedSignature(name));
        };
        if key == time_key {
            timestamp = Some(
                val.parse::<i64>()
                    .map_err(|_| WebhookError::MalformedSignature(name))?,
            );
        } else if key == signature_key {
            // A signature that is not hex can never match; skip it.
            if let Ok(bytes) = hex::decode(val) {
                signatures.push(bytes);
            }
        }
    }
    let timestamp = timestamp.ok_or(WebhookError::MalformedSignature(name))?;
    if signatures.is_empty() {
        return Err(WebhookError::MalformedSignature(name));
    }
    Ok(Signed {
        timestamp,
        signatures,
    })
}

fn check(
    signed: &Signed,
    secret: &str,
    separator: u8,
    body: &[u8],
    now: DateTime<Utc>,
    tolerance: Duration,
) -> Result<(), WebhookError> {
    if secret.is_empty() {
        return Err(WebhookError::NoSecret);
    }
    let matches = signed.signatures.iter().any(|signature| {
        mac(secret.as_bytes(), signed.timestamp, separator, body)
            .verify_slice(signature)
            .is_ok()
    });
    if !matches {
        return Err(WebhookError::SignatureMismatch);
    }
    let skew = now.timestamp().abs_diff(signed.timestamp);
    if skew > tolerance.as_secs() {
        return Err(WebhookError::Expired);
    }
    Ok(())
}

/// Verifies a Stripe webhook.
pub(crate) fn verify_stripe(
    headers: &HeaderMap,
    body: &[u8],
    secret: &str,
    now: DateTime<Utc>,
    tolerance: Duration,
) -> Result<(), WebhookError> {
    let value = header(headers, STRIPE_SIGNATURE_HEADER)?;
    let signed = parse(value, STRIPE_SIGNATURE_HEADER, ',', "t", "v1")?;
    check(&signed, secret, b'.', body, now, tolerance)
}

/// Verifies a Paddle webhook.
pub(crate) fn verify_paddle(
    headers: &HeaderMap,
    body: &[u8],
    secret: &str,
    now: DateTime<Utc>,
    tolerance: Duration,
) -> Result<(), WebhookError> {
    let value = header(headers, PADDLE_SIGNATURE_HEADER)?;
    let signed = parse(value, PADDLE_SIGNATURE_HEADER, ';', "ts", "h1")?;
    check(&signed, secret, b':', body, now, tolerance)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use http::HeaderValue;

    use super::*;

    const BODY: &[u8] = br#"{"id":"evt_1","type":"invoice.paid"}"#;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    fn with(name: &'static str, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, HeaderValue::from_str(value).unwrap());
        headers
    }

    /// An independent HMAC-SHA256 (RFC 2104 by hand over SHA-256), so the
    /// signer is not only checked against itself.
    fn reference_hmac(key: &[u8], message: &[u8]) -> String {
        use sha2::Digest;
        let mut block = [0u8; 64];
        if key.len() > 64 {
            block[..32].copy_from_slice(&Sha256::digest(key));
        } else {
            block[..key.len()].copy_from_slice(key);
        }
        let inner: Vec<u8> = block.iter().map(|b| b ^ 0x36).collect();
        let outer: Vec<u8> = block.iter().map(|b| b ^ 0x5c).collect();
        let inner_hash = Sha256::digest([inner.as_slice(), message].concat());
        hex::encode(Sha256::digest(
            [outer.as_slice(), inner_hash.as_slice()].concat(),
        ))
    }

    #[test]
    fn the_rfc_4231_vector_holds() {
        // RFC 4231, test case 2.
        let digest = <Hmac<Sha256> as KeyInit>::new_from_slice(b"Jefe")
            .unwrap()
            .chain_update(b"what do ya want for nothing?")
            .finalize()
            .into_bytes();
        assert_eq!(
            hex::encode(digest),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            reference_hmac(b"Jefe", b"what do ya want for nothing?"),
            hex::encode(digest)
        );
    }

    #[test]
    fn stripe_signs_timestamp_dot_body() {
        let header = sign_stripe_webhook("whsec_test", 1_700_000_000, BODY);
        let expected = reference_hmac(b"whsec_test", &[b"1700000000.".as_slice(), BODY].concat());
        assert_eq!(header, format!("t=1700000000,v1={expected}"));
        let headers = with(STRIPE_SIGNATURE_HEADER, &header);
        verify_stripe(
            &headers,
            BODY,
            "whsec_test",
            at(1_700_000_100),
            Duration::from_secs(300),
        )
        .unwrap();
    }

    #[test]
    fn paddle_signs_timestamp_colon_body() {
        let header = sign_paddle_webhook("pdl_ntfset_test", 1_700_000_000, BODY);
        let expected = reference_hmac(
            b"pdl_ntfset_test",
            &[b"1700000000:".as_slice(), BODY].concat(),
        );
        assert_eq!(header, format!("ts=1700000000;h1={expected}"));
        let headers = with(PADDLE_SIGNATURE_HEADER, &header);
        verify_paddle(
            &headers,
            BODY,
            "pdl_ntfset_test",
            at(1_700_000_000),
            Duration::from_secs(5),
        )
        .unwrap();
    }

    #[test]
    fn tampering_a_wrong_secret_and_old_timestamps_are_refused() {
        let tolerance = Duration::from_secs(300);
        let header = sign_stripe_webhook("whsec_test", 1_700_000_000, BODY);
        let headers = with(STRIPE_SIGNATURE_HEADER, &header);
        assert_eq!(
            verify_stripe(&headers, b"{}", "whsec_test", at(1_700_000_000), tolerance),
            Err(WebhookError::SignatureMismatch)
        );
        assert_eq!(
            verify_stripe(&headers, BODY, "whsec_other", at(1_700_000_000), tolerance),
            Err(WebhookError::SignatureMismatch)
        );
        assert_eq!(
            verify_stripe(&headers, BODY, "whsec_test", at(1_700_000_301), tolerance),
            Err(WebhookError::Expired)
        );
        assert_eq!(
            verify_stripe(&headers, BODY, "whsec_test", at(1_699_999_699), tolerance),
            Err(WebhookError::Expired)
        );
        assert_eq!(
            verify_stripe(&HeaderMap::new(), BODY, "whsec_test", at(0), tolerance),
            Err(WebhookError::MissingSignature(STRIPE_SIGNATURE_HEADER))
        );
        for malformed in ["v1=abc", "t=x,v1=00", "t=1", "garbage"] {
            assert_eq!(
                verify_stripe(
                    &with(STRIPE_SIGNATURE_HEADER, malformed),
                    BODY,
                    "whsec_test",
                    at(1),
                    tolerance
                ),
                Err(WebhookError::MalformedSignature(STRIPE_SIGNATURE_HEADER)),
                "{malformed}"
            );
        }
        assert_eq!(
            verify_stripe(&headers, BODY, "", at(1_700_000_000), tolerance),
            Err(WebhookError::NoSecret)
        );
        // Paddle's header with Stripe's separator is not Paddle's format.
        let paddle = sign_paddle_webhook("s", 1_700_000_000, BODY);
        assert!(
            verify_paddle(
                &with(PADDLE_SIGNATURE_HEADER, &paddle.replace(';', ",")),
                BODY,
                "s",
                at(1_700_000_000),
                tolerance
            )
            .is_err()
        );
    }

    #[test]
    fn any_listed_signature_may_match() {
        let good = sign_stripe_webhook("whsec_new", 1_700_000_000, BODY);
        let v1 = good.split_once("v1=").unwrap().1;
        let header = format!("t=1700000000,v1={},v0=ignored,v1={v1}", "00".repeat(32));
        verify_stripe(
            &with(STRIPE_SIGNATURE_HEADER, &header),
            BODY,
            "whsec_new",
            at(1_700_000_000),
            Duration::from_secs(300),
        )
        .unwrap();
        let good = sign_paddle_webhook("pdl", 1_700_000_000, BODY);
        let h1 = good.split_once("h1=").unwrap().1;
        let header = format!("ts=1700000000;h1={};h1={h1}", "ff".repeat(32));
        verify_paddle(
            &with(PADDLE_SIGNATURE_HEADER, &header),
            BODY,
            "pdl",
            at(1_700_000_000),
            Duration::from_secs(300),
        )
        .unwrap();
    }
}
