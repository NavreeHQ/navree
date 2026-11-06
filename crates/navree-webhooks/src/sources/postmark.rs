//! [`Postmark`]: the HTTP Basic credentials of the webhook URL.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use http::HeaderMap;

use crate::options::SourceOptions;
use crate::signing;
use crate::verify::{
    Tolerance, Verified, VerifyError, WebhookSource, WebhookVerifier, body_fields, header,
    require_secret,
};

/// Postmark's scheme: Postmark signs nothing, and authenticates its webhook
/// requests (deliveries, bounces, opens, clicks, spam complaints,
/// subscription changes, inbound mail) with the HTTP Basic credentials of
/// the webhook URL (`https://user:password@api.example.com/...`). The secret
/// is `username:password`, compared in constant time with the
/// `Authorization` header. The id is the body's `MessageID`, the event type
/// its `RecordType` (`Delivery`, `Bounce`...). No timestamp is signed: drop
/// replays with [`mark_processed`](crate::VerifiedWebhook::mark_processed).
/// Source name `postmark`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Postmark;

impl Postmark {
    /// The `Authorization` value Postmark sends for `credentials`
    /// (`username:password`).
    pub fn authorization(credentials: &str) -> String {
        format!("Basic {}", STANDARD.encode(credentials))
    }

    /// Verifies a request carrying `credentials` (`username:password`).
    pub fn verify(
        credentials: &str,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result<Verified, VerifyError> {
        require_secret(credentials)?;
        if !credentials.contains(':') {
            return Err(VerifyError::Misconfigured(
                "the secret of a Postmark source is `username:password`".to_owned(),
            ));
        }
        let value = header(headers, "authorization")?.trim();
        let malformed = || VerifyError::MalformedHeader("authorization".to_owned());
        let (scheme, encoded) = value.split_once(' ').ok_or_else(malformed)?;
        if !scheme.eq_ignore_ascii_case("basic") {
            return Err(malformed());
        }
        let sent = STANDARD.decode(encoded.trim()).map_err(|_| malformed())?;
        if !signing::same(&sent, credentials.as_bytes()) {
            return Err(VerifyError::SignatureMismatch);
        }
        let (id, event_type) = body_fields(body, "MessageID", "RecordType");
        Ok(Verified {
            id,
            timestamp: None,
            event_type,
        })
    }
}

impl WebhookVerifier for Postmark {
    fn verify_request(
        source: &SourceOptions,
        headers: &HeaderMap,
        body: &[u8],
        _tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        Self::verify(source.secret.expose(), headers, body)
    }
}

impl WebhookSource for Postmark {
    const NAME: &'static str = "postmark";
    type Verifier = Postmark;
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;

    use super::*;

    const CREDENTIALS: &str = "postmark:s3cr3t-pa55";
    const BOUNCE: &[u8] = br#"{"RecordType":"Bounce","MessageID":"883953f4-6105-42a2-a16a-77a8eac79483","Type":"HardBounce","Email":"anu@example.com"}"#;

    fn authorized(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn basic_credentials_are_checked() {
        // RFC 7617: `Basic ` and the base64 of `user:password`.
        assert_eq!(
            Postmark::authorization("Aladdin:open sesame"),
            "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
        let verified = Postmark::verify(
            CREDENTIALS,
            &authorized(&Postmark::authorization(CREDENTIALS)),
            BOUNCE,
        )
        .unwrap();
        assert_eq!(
            verified.id.as_deref(),
            Some("883953f4-6105-42a2-a16a-77a8eac79483")
        );
        assert_eq!(verified.event_type.as_deref(), Some("Bounce"));
        // The scheme name is case-insensitive.
        let lower = Postmark::authorization(CREDENTIALS).replace("Basic", "basic");
        Postmark::verify(CREDENTIALS, &authorized(&lower), BOUNCE).unwrap();
    }

    #[test]
    fn wrong_or_missing_credentials_fail() {
        assert_eq!(
            Postmark::verify(
                CREDENTIALS,
                &authorized(&Postmark::authorization("postmark:guess")),
                BOUNCE
            ),
            Err(VerifyError::SignatureMismatch)
        );
        assert_eq!(
            Postmark::verify(CREDENTIALS, &HeaderMap::new(), BOUNCE),
            Err(VerifyError::MissingHeader("authorization".to_owned()))
        );
        assert_eq!(
            Postmark::verify(CREDENTIALS, &authorized("Bearer abc"), BOUNCE),
            Err(VerifyError::MalformedHeader("authorization".to_owned()))
        );
        assert!(matches!(
            Postmark::verify("no-colon", &authorized("Basic x"), BOUNCE),
            Err(VerifyError::Misconfigured(_))
        ));
    }
}
