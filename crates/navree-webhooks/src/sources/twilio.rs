//! [`Twilio`]: `X-Twilio-Signature`, HMAC-SHA1 over the URL and the sorted
//! POST parameters.

use http::HeaderMap;
use sha2::{Digest as _, Sha256};

use crate::options::{HmacAlgorithm, SignatureEncoding};
use crate::request::{IncomingRequest, Verdict};
use crate::signing;
use crate::verify::{
    Verified, VerifyError, WebhookSource, WebhookVerifier, any_matches, header, optional_header,
    require_secret,
};

/// Twilio's scheme: `X-Twilio-Signature` is the base64 HMAC-SHA1, keyed with
/// the account's auth token, of the full URL Twilio called (query string
/// included) followed by each POST parameter's name and value, sorted by
/// name. A request with a JSON body is signed over the URL alone, which
/// then carries the body's SHA-256 in `bodySHA256`. The secret is the auth
/// token; the id is the `I-Twilio-Idempotency-Token` header. Twilio signs
/// no timestamp: drop replays with
/// [`mark_processed`](crate::VerifiedWebhook::mark_processed). Source name
/// `twilio`.
///
/// Twilio signs the URL it called, so behind a proxy set the source's
/// `url` to the public URL of the route.
#[derive(Clone, Copy, Debug, Default)]
pub struct Twilio;

impl Twilio {
    /// The signature header.
    pub const SIGNATURE_HEADER: &'static str = "x-twilio-signature";
    /// The idempotency token header (the delivery id).
    pub const IDEMPOTENCY_HEADER: &'static str = "i-twilio-idempotency-token";
    /// The query parameter carrying the SHA-256 of a JSON body.
    pub const BODY_SHA256: &'static str = "bodySHA256";

    /// The `X-Twilio-Signature` of a request to `url` with POST `params`
    /// (none for a JSON body, whose hash is in the URL).
    pub fn sign(auth_token: &str, url: &str, params: &[(&str, &str)]) -> String {
        let params: Vec<(String, String)> = params
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        signing::encode(
            SignatureEncoding::Base64,
            &signing::hmac(
                HmacAlgorithm::Sha1,
                auth_token.as_bytes(),
                &[signed_text(url, &params).as_bytes()],
            ),
        )
    }

    /// Verifies a request to `url` (the full URL Twilio called, query
    /// string included) signed with `auth_token`. The URL is also tried with
    /// its default port added or removed, as Twilio's own libraries do.
    pub fn verify(
        auth_token: &str,
        url: &str,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result<Verified, VerifyError> {
        require_secret(auth_token)?;
        let signature = header(headers, Self::SIGNATURE_HEADER)?;
        let params = match query_param(url, Self::BODY_SHA256) {
            Some(expected) => {
                let actual = hex::encode(Sha256::digest(body));
                if !signing::same(actual.as_bytes(), expected.to_ascii_lowercase().as_bytes()) {
                    return Err(VerifyError::SignatureMismatch);
                }
                Vec::new()
            }
            None => form_urlencoded::parse(body).into_owned().collect(),
        };
        let mut matched = false;
        for candidate in url_variants(url) {
            let expected = signing::hmac(
                HmacAlgorithm::Sha1,
                auth_token.as_bytes(),
                &[signed_text(&candidate, &params).as_bytes()],
            );
            matched |= any_matches([signature], SignatureEncoding::Base64, &expected);
        }
        if !matched {
            return Err(VerifyError::SignatureMismatch);
        }
        Ok(Verified {
            id: optional_header(headers, Self::IDEMPOTENCY_HEADER),
            timestamp: None,
            event_type: None,
        })
    }
}

impl WebhookVerifier for Twilio {
    async fn verify_incoming(request: &IncomingRequest<'_>) -> Result<Verdict, VerifyError> {
        Self::verify(
            &request.source().secret,
            request.url(),
            request.headers(),
            request.body(),
        )
        .map(Verdict::Verified)
    }
}

impl WebhookSource for Twilio {
    const NAME: &'static str = "twilio";
    type Verifier = Twilio;
}

/// The URL followed by each parameter name and value: names sorted, and the
/// values of a repeated name sorted, duplicates once.
fn signed_text(url: &str, params: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = params.iter().collect();
    sorted.sort();
    sorted.dedup();
    let mut text = url.to_owned();
    for (name, value) in sorted {
        text.push_str(name);
        text.push_str(value);
    }
    text
}

/// The first value of query parameter `name` of `url`.
fn query_param(url: &str, name: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    form_urlencoded::parse(query.split('#').next().unwrap_or_default().as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

/// `url` as it is, and with the scheme's default port added (when it has
/// no port) or removed (when it has the default one).
fn url_variants(url: &str) -> Vec<String> {
    let mut variants = vec![url.to_owned()];
    let Some((scheme, rest)) = url.split_once("://") else {
        return variants;
    };
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "https" => "443",
        "http" => "80",
        _ => return variants,
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if authority.contains('@') {
        return variants;
    }
    let host_end = authority.rfind(':').filter(|colon| !authority[*colon..].contains(']'));
    match host_end {
        Some(colon) if &authority[colon + 1..] == default_port => {
            variants.push(format!("{scheme}://{}{tail}", &authority[..colon]));
        }
        Some(_) => {}
        None => variants.push(format!("{scheme}://{authority}:{default_port}{tail}")),
    }
    variants
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;

    use super::*;
    use crate::options::SourceOptions;
    use crate::sources::block_on;

    /// Twilio's documented example (webhook security, "Validating
    /// signatures from Twilio").
    const URL: &str = "https://mycompany.com/myapp.php?foo=1&bar=2";
    const PARAMS: &[(&str, &str)] = &[
        ("CallSid", "CA1234567890ABCDE"),
        ("Caller", "+12349013030"),
        ("Digits", "1234"),
        ("From", "+12349013030"),
        ("To", "+18005551212"),
    ];
    const SIGNATURE: &str = "0/KCTR6DLpKmkAf8muzZqo1nDgQ=";

    fn form(params: &[(&str, &str)]) -> Vec<u8> {
        form_urlencoded::Serializer::new(String::new())
            .extend_pairs(params)
            .finish()
            .into_bytes()
    }

    fn signed(signature: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-twilio-signature", HeaderValue::from_str(signature).unwrap());
        headers.insert(
            "i-twilio-idempotency-token",
            HeaderValue::from_static("0f3a8a3c-idem"),
        );
        headers
    }

    #[test]
    fn the_documented_example_signs_and_verifies() {
        assert_eq!(Twilio::sign("12345", URL, PARAMS), SIGNATURE);
        // The body's parameter order does not matter.
        let mut shuffled = PARAMS.to_vec();
        shuffled.reverse();
        let verified = Twilio::verify("12345", URL, &signed(SIGNATURE), &form(&shuffled)).unwrap();
        assert_eq!(verified.id.as_deref(), Some("0f3a8a3c-idem"));
        assert_eq!(verified.timestamp, None);
    }

    #[test]
    fn a_changed_url_parameter_or_token_fails() {
        let body = form(PARAMS);
        for (token, url, body) in [
            ("12345", "https://mycompany.com/myapp.php?foo=1&bar=3", body.clone()),
            ("12346", URL, body.clone()),
            ("12345", URL, form(&PARAMS[..4])),
        ] {
            assert_eq!(
                Twilio::verify(token, url, &signed(SIGNATURE), &body),
                Err(VerifyError::SignatureMismatch)
            );
        }
        assert_eq!(
            Twilio::verify("12345", URL, &HeaderMap::new(), &body),
            Err(VerifyError::MissingHeader("x-twilio-signature".to_owned()))
        );
        assert_eq!(
            Twilio::verify("", URL, &signed(SIGNATURE), &body),
            Err(VerifyError::MissingSecret)
        );
    }

    #[test]
    fn the_url_is_tried_with_and_without_its_default_port() {
        let with_port = "https://mycompany.com:443/myapp.php?foo=1&bar=2";
        let body = form(PARAMS);
        // Signed without the port, received with it.
        Twilio::verify("12345", with_port, &signed(SIGNATURE), &body).unwrap();
        // Signed with the port, received without it.
        let signature = Twilio::sign("12345", with_port, PARAMS);
        Twilio::verify("12345", URL, &signed(&signature), &body).unwrap();
        assert_eq!(
            url_variants("http://a.example:8080/x"),
            ["http://a.example:8080/x"]
        );
        assert_eq!(
            url_variants("http://a.example?x=1"),
            ["http://a.example?x=1", "http://a.example:80?x=1"]
        );
    }

    #[test]
    fn repeated_parameters_are_sorted_by_value() {
        let params = [("To", "+2"), ("Media", "b"), ("Media", "a")];
        let signature = Twilio::sign("t", "https://a.example/sms", &params);
        assert_eq!(
            signature,
            Twilio::sign(
                "t",
                "https://a.example/sms",
                &[("Media", "a"), ("Media", "b"), ("To", "+2")]
            )
        );
        Twilio::verify("t", "https://a.example/sms", &signed(&signature), &form(&params)).unwrap();
    }

    #[test]
    fn json_bodies_are_signed_through_body_sha256() {
        let body = br#"{"ParentAccountSid":"AC1","Payload":"{}"}"#;
        let url = format!(
            "https://a.example/events?bodySHA256={}",
            hex::encode(Sha256::digest(body))
        );
        let signature = Twilio::sign("t", &url, &[]);
        Twilio::verify("t", &url, &signed(&signature), body).unwrap();
        assert_eq!(
            Twilio::verify("t", &url, &signed(&signature), br#"{"Payload":"x"}"#),
            Err(VerifyError::SignatureMismatch)
        );
    }

    #[test]
    fn whole_requests_use_the_public_url_and_the_auth_token() {
        let source = SourceOptions::new("12345");
        let headers = signed(SIGNATURE);
        let body = form(PARAMS);
        let request = IncomingRequest::new(&source, &headers, &body).with_url(URL);
        let verdict = block_on(Twilio::verify_incoming(&request));
        assert!(matches!(verdict, Ok(Verdict::Verified(_))), "{verdict:?}");
        let elsewhere = IncomingRequest::new(&source, &headers, &body)
            .with_url("https://mycompany.com/other.php?foo=1&bar=2");
        assert_eq!(
            block_on(Twilio::verify_incoming(&elsewhere)),
            Err(VerifyError::SignatureMismatch)
        );
    }

    #[test]
    fn the_base64_signature_may_use_either_alphabet() {
        let url_safe = SIGNATURE.replace('/', "_");
        Twilio::verify("12345", URL, &signed(&url_safe), &form(PARAMS)).unwrap();
    }
}
