//! Webhook signatures: `Stripe-Signature` and `Paddle-Signature`, both
//! HMAC-SHA256 over a timestamp and the raw body, checked by
//! navree-webhooks' [`Stripe`] and [`Paddle`] verifiers, the ones its
//! `VerifiedWebhook` routes use.
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
use http::HeaderMap;
use navree_webhooks::{Paddle, Stripe, Tolerance, VerifyError};

use crate::provider::WebhookError;

/// The header Stripe signs webhooks in.
pub const STRIPE_SIGNATURE_HEADER: &str = Stripe::SIGNATURE_HEADER;
/// The header Paddle signs webhooks in.
pub const PADDLE_SIGNATURE_HEADER: &str = Paddle::SIGNATURE_HEADER;

/// The `Stripe-Signature` header value for `body` signed at `timestamp`
/// (Unix seconds) with `secret`. For tests that post webhooks.
pub fn sign_stripe_webhook(secret: &str, timestamp: i64, body: &[u8]) -> String {
    Stripe::sign(secret, timestamp, body)
}

/// The `Paddle-Signature` header value for `body` signed at `timestamp`
/// (Unix seconds) with `secret`. For tests that post webhooks.
pub fn sign_paddle_webhook(secret: &str, timestamp: i64, body: &[u8]) -> String {
    Paddle::sign(secret, timestamp, body)
}

/// The window around `now` a signed timestamp must fall in.
fn window(now: DateTime<Utc>, tolerance: Duration) -> Option<Tolerance> {
    Some(Tolerance::at(now, tolerance.as_secs()))
}

/// Billing's reason for a refusal by navree-webhooks' verifier. A reason
/// the two schemes never give is still a refusal.
fn refusal(error: VerifyError, header: &'static str) -> WebhookError {
    match error {
        VerifyError::MissingHeader(_) => WebhookError::MissingSignature(header),
        VerifyError::MalformedHeader(_) => WebhookError::MalformedSignature(header),
        VerifyError::TimestampOutOfTolerance { .. } => WebhookError::Expired,
        VerifyError::MissingSecret => WebhookError::NoSecret,
        _ => WebhookError::SignatureMismatch,
    }
}

/// Verifies a Stripe webhook.
pub(crate) fn verify_stripe(
    headers: &HeaderMap,
    body: &[u8],
    secret: &str,
    now: DateTime<Utc>,
    tolerance: Duration,
) -> Result<(), WebhookError> {
    Stripe::verify(secret, headers, body, window(now, tolerance))
        .map(drop)
        .map_err(|error| refusal(error, STRIPE_SIGNATURE_HEADER))
}

/// Verifies a Paddle webhook.
pub(crate) fn verify_paddle(
    headers: &HeaderMap,
    body: &[u8],
    secret: &str,
    now: DateTime<Utc>,
    tolerance: Duration,
) -> Result<(), WebhookError> {
    Paddle::verify(secret, headers, body, window(now, tolerance))
        .map(drop)
        .map_err(|error| refusal(error, PADDLE_SIGNATURE_HEADER))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use http::HeaderValue;
    use sha2::{Digest, Sha256};

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
    fn the_reference_hmac_holds_the_rfc_4231_vector() {
        // RFC 4231, test case 2.
        assert_eq!(
            reference_hmac(b"Jefe", b"what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn the_headers_are_the_ones_navree_webhooks_reads() {
        assert_eq!(STRIPE_SIGNATURE_HEADER, "stripe-signature");
        assert_eq!(PADDLE_SIGNATURE_HEADER, "paddle-signature");
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
        // The edges of the window pass; a second past either is refused.
        for secs in [1_700_000_300, 1_699_999_700] {
            verify_stripe(&headers, BODY, "whsec_test", at(secs), tolerance).unwrap();
        }
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
        // No secret verifies nothing, however the request is signed.
        assert_eq!(
            verify_stripe(&headers, BODY, "", at(1_700_000_000), tolerance),
            Err(WebhookError::NoSecret)
        );
        assert_eq!(
            verify_stripe(
                &with(STRIPE_SIGNATURE_HEADER, &sign_stripe_webhook("", 1, BODY)),
                BODY,
                "",
                at(1),
                tolerance
            ),
            Err(WebhookError::NoSecret)
        );
        // A signature that is not hex never matches.
        assert_eq!(
            verify_stripe(
                &with(STRIPE_SIGNATURE_HEADER, "t=1700000000,v1=not-hex"),
                BODY,
                "whsec_test",
                at(1_700_000_000),
                tolerance
            ),
            Err(WebhookError::SignatureMismatch)
        );
        // A signature cut short is no match either.
        let short = &header[..header.len() - 2];
        assert_eq!(
            verify_stripe(
                &with(STRIPE_SIGNATURE_HEADER, short),
                BODY,
                "whsec_test",
                at(1_700_000_000),
                tolerance
            ),
            Err(WebhookError::SignatureMismatch)
        );
        // Stripe's scheme is not Paddle's: Paddle's header with Stripe's
        // separator does not parse, and Stripe's MAC is not Paddle's.
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
        let stripe_mac = sign_stripe_webhook("s", 1_700_000_000, BODY);
        let v1 = stripe_mac.split_once("v1=").unwrap().1;
        assert_eq!(
            verify_paddle(
                &with(PADDLE_SIGNATURE_HEADER, &format!("ts=1700000000;h1={v1}")),
                BODY,
                "s",
                at(1_700_000_000),
                tolerance
            ),
            Err(WebhookError::SignatureMismatch)
        );
    }

    #[test]
    fn paddle_refuses_what_stripe_refuses() {
        let tolerance = Duration::from_secs(5);
        let header = sign_paddle_webhook("pdl", 1_700_000_000, BODY);
        let headers = with(PADDLE_SIGNATURE_HEADER, &header);
        assert_eq!(
            verify_paddle(&headers, b"{}", "pdl", at(1_700_000_000), tolerance),
            Err(WebhookError::SignatureMismatch)
        );
        assert_eq!(
            verify_paddle(&headers, BODY, "other", at(1_700_000_000), tolerance),
            Err(WebhookError::SignatureMismatch)
        );
        assert_eq!(
            verify_paddle(&headers, BODY, "pdl", at(1_700_000_006), tolerance),
            Err(WebhookError::Expired)
        );
        assert_eq!(
            verify_paddle(&headers, BODY, "pdl", at(1_699_999_994), tolerance),
            Err(WebhookError::Expired)
        );
        assert_eq!(
            verify_paddle(&HeaderMap::new(), BODY, "pdl", at(1), tolerance),
            Err(WebhookError::MissingSignature(PADDLE_SIGNATURE_HEADER))
        );
        for malformed in ["h1=00", "ts=soon;h1=00", "ts=1", "garbage"] {
            assert_eq!(
                verify_paddle(
                    &with(PADDLE_SIGNATURE_HEADER, malformed),
                    BODY,
                    "pdl",
                    at(1),
                    tolerance
                ),
                Err(WebhookError::MalformedSignature(PADDLE_SIGNATURE_HEADER)),
                "{malformed}"
            );
        }
        assert_eq!(
            verify_paddle(&headers, BODY, "", at(1_700_000_000), tolerance),
            Err(WebhookError::NoSecret)
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
