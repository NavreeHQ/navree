//! [`Meta`]: `X-Hub-Signature-256` over the body, and the `hub.challenge`
//! subscription handshake.

use http::HeaderMap;

use crate::options::{SignatureEncoding, SourceOptions};
use crate::request::{Handshake, IncomingRequest, Verdict};
use crate::signing;
use crate::verify::{
    Tolerance, Verified, VerifyError, WebhookSource, WebhookVerifier, any_matches, body_fields,
    header, require_secret,
};

/// The webhooks of Meta's platform (the WhatsApp Cloud API, Messenger,
/// Instagram): `X-Hub-Signature-256: sha256=<hex>`, HMAC-SHA256 of the body
/// keyed with the app secret. Meta signs no timestamp and sends no
/// delivery id. When the webhook is set up, Meta calls the URL with `GET
/// ?hub.mode=subscribe&hub.verify_token=..&hub.challenge=..`: when the
/// token equals the source's `verify_token`, the challenge is answered
/// without running the handler (mount the route for `GET` too). The event
/// type is the body's `object` (`whatsapp_business_account`). Source name
/// `meta`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Meta;

impl Meta {
    /// The signature header.
    pub const SIGNATURE_HEADER: &'static str = "x-hub-signature-256";

    /// The `X-Hub-Signature-256` value (`sha256=<hex>`) of `body`.
    pub fn sign(app_secret: &str, body: &[u8]) -> String {
        format!(
            "sha256={}",
            hex::encode(signing::hmac_sha256(app_secret.as_bytes(), &[body]))
        )
    }

    /// Verifies a request signed with `app_secret`.
    pub fn verify(
        app_secret: &str,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result<Verified, VerifyError> {
        require_secret(app_secret)?;
        let signature = header(headers, Self::SIGNATURE_HEADER)?
            .trim()
            .strip_prefix("sha256=")
            .ok_or_else(|| VerifyError::MalformedHeader(Self::SIGNATURE_HEADER.to_owned()))?;
        let expected = signing::hmac_sha256(app_secret.as_bytes(), &[body]);
        if !any_matches([signature], SignatureEncoding::Hex, &expected) {
            return Err(VerifyError::SignatureMismatch);
        }
        let (_, object) = body_fields(body, "id", "object");
        Ok(Verified {
            id: None,
            timestamp: None,
            event_type: object,
        })
    }

    /// The answer to a subscription handshake: the challenge, when `token`
    /// is the source's `verify_token`.
    pub fn handshake(
        source: &SourceOptions,
        token: &str,
        challenge: &str,
    ) -> Result<Handshake, VerifyError> {
        let expected = source
            .verify_token
            .as_deref()
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                VerifyError::Rejected(
                    "the source has no `verify_token` for the handshake".to_owned(),
                )
            })?;
        if !signing::same(token.as_bytes(), expected.as_bytes()) {
            return Err(VerifyError::Rejected(
                "the verify token does not match".to_owned(),
            ));
        }
        Ok(Handshake::text(challenge))
    }
}

impl WebhookVerifier for Meta {
    fn verify_request(
        source: &SourceOptions,
        headers: &HeaderMap,
        body: &[u8],
        _tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        Self::verify(&source.secret, headers, body)
    }

    async fn verify_incoming(request: &IncomingRequest<'_>) -> Result<Verdict, VerifyError> {
        if request.query("hub.mode").as_deref() == Some("subscribe") {
            let token = request.query("hub.verify_token").unwrap_or_default();
            let challenge = request.query("hub.challenge").unwrap_or_default();
            return Self::handshake(request.source(), &token, &challenge).map(Verdict::Handshake);
        }
        Self::verify(&request.source().secret, request.headers(), request.body())
            .map(Verdict::Verified)
    }
}

impl WebhookSource for Meta {
    const NAME: &'static str = "meta";
    type Verifier = Meta;
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;

    use super::*;
    use crate::sources::block_on;

    const SECRET: &str = "meta-app-secret";

    fn signed(body: &[u8]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            Meta::SIGNATURE_HEADER,
            HeaderValue::from_str(&Meta::sign(SECRET, body)).unwrap(),
        );
        headers
    }

    #[test]
    fn a_signed_body_verifies_and_names_its_object() {
        let body = br#"{"object":"whatsapp_business_account","entry":[]}"#;
        let verified = Meta::verify(SECRET, &signed(body), body).unwrap();
        assert_eq!(
            verified.event_type.as_deref(),
            Some("whatsapp_business_account")
        );
        assert_eq!(verified.id, None);
    }

    #[test]
    fn a_changed_body_or_secret_fails() {
        let body = br#"{"object":"page"}"#;
        assert_eq!(
            Meta::verify(SECRET, &signed(body), br#"{"object":"other"}"#),
            Err(VerifyError::SignatureMismatch)
        );
        assert_eq!(
            Meta::verify("other-secret", &signed(body), body),
            Err(VerifyError::SignatureMismatch)
        );
        assert_eq!(
            Meta::verify(SECRET, &HeaderMap::new(), body),
            Err(VerifyError::MissingHeader(
                Meta::SIGNATURE_HEADER.to_owned()
            ))
        );
        let mut bare = HeaderMap::new();
        bare.insert(Meta::SIGNATURE_HEADER, HeaderValue::from_static("abc"));
        assert_eq!(
            Meta::verify(SECRET, &bare, body),
            Err(VerifyError::MalformedHeader(
                Meta::SIGNATURE_HEADER.to_owned()
            ))
        );
    }

    #[test]
    fn the_handshake_echoes_the_challenge_for_the_right_token() {
        let source = SourceOptions {
            verify_token: Some("my-verify-token".to_owned()),
            ..SourceOptions::new(SECRET)
        };
        let headers = HeaderMap::new();
        let ok = IncomingRequest::new(&source, &headers, b"").with_url(
            "https://api.acme.test/hooks/meta?hub.mode=subscribe&hub.verify_token=my-verify-token&hub.challenge=1158201444",
        );
        assert_eq!(
            block_on(Meta::verify_incoming(&ok)).unwrap(),
            Verdict::Handshake(Handshake::text("1158201444"))
        );
        let wrong = IncomingRequest::new(&source, &headers, b"").with_url(
            "https://api.acme.test/hooks/meta?hub.mode=subscribe&hub.verify_token=guess&hub.challenge=1",
        );
        assert!(matches!(
            block_on(Meta::verify_incoming(&wrong)),
            Err(VerifyError::Rejected(_))
        ));
        let unset = SourceOptions::new(SECRET);
        let request = IncomingRequest::new(&unset, &headers, b"")
            .with_url("https://x.test/?hub.mode=subscribe&hub.verify_token=&hub.challenge=1");
        assert!(matches!(
            block_on(Meta::verify_incoming(&request)),
            Err(VerifyError::Rejected(_))
        ));
    }
}
