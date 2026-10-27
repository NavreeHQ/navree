//! [`GooglePubSub`]: push requests authenticated with a Google-signed OIDC
//! token.

use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE};
use chrono::{DateTime, Utc};
use navree_crypto::jose::{Algorithm, Jws};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::keys::GOOGLE_KEYS_URL;
use crate::options::SourceOptions;
use crate::request::{IncomingRequest, Verdict};
use crate::verify::{Verified, VerifyError, WebhookSource, WebhookVerifier};

/// How far the token's `exp` and `iat` may be off the app's clock.
const CLOCK_SKEW_SECS: i64 = 60;

/// Google Cloud Pub/Sub push subscriptions with authentication on (Gmail
/// push notifications among them). Each request carries `Authorization:
/// Bearer <OIDC token>`, an RS256 JWT signed with Google's OAuth 2.0 keys
/// (fetched and cached by [`SenderKeys`](crate::SenderKeys)). Its `iss`
/// must be Google, its `aud` the source's `audience` (default: the public
/// URL of the request, which is what Pub/Sub uses when the subscription
/// names none), its `email` the source's `service_account` with
/// `email_verified`, and it must not have expired. The service account
/// check is what ties the request to your subscription: any Google account
/// can mint a token for any audience.
///
/// The id is the message's `messageId`, the timestamp the token's `iat`.
/// Source name `google_pubsub`; no secret. Read the body as
/// [`PubSubPush`].
#[derive(Clone, Copy, Debug, Default)]
pub struct GooglePubSub;

impl GooglePubSub {
    /// Google's OAuth 2.0 public keys (a JWKS), which sign the tokens.
    pub const KEYS_URL: &'static str = GOOGLE_KEYS_URL;

    /// The issuers of Google's OIDC tokens.
    pub const ISSUERS: [&'static str; 2] = ["https://accounts.google.com", "accounts.google.com"];

    /// Checks the claims of a token whose signature verified: issuer,
    /// `audience`, `service_account` (verified email) and lifetime against
    /// `now` (a minute of clock skew either way). Returns the `iat`.
    pub fn check_claims(
        claims: &Value,
        audience: &str,
        service_account: &str,
        now: DateTime<Utc>,
    ) -> Result<DateTime<Utc>, VerifyError> {
        let refuse = |why: &str| Err(VerifyError::Rejected(why.to_owned()));
        let issuer = claims["iss"].as_str().unwrap_or_default();
        if !Self::ISSUERS.contains(&issuer) {
            return refuse("the token was not issued by Google");
        }
        let audience_matches = match &claims["aud"] {
            Value::String(aud) => aud == audience,
            Value::Array(auds) => auds.iter().any(|aud| aud.as_str() == Some(audience)),
            _ => false,
        };
        if !audience_matches {
            return refuse("the token is for another audience");
        }
        let email = claims["email"].as_str().unwrap_or_default();
        let verified = match &claims["email_verified"] {
            Value::Bool(verified) => *verified,
            Value::String(verified) => verified == "true",
            _ => false,
        };
        if !verified || !email.eq_ignore_ascii_case(service_account.trim()) {
            return refuse("the token is for another service account");
        }
        let expires = claims["exp"].as_i64().unwrap_or(i64::MIN);
        if expires.saturating_add(CLOCK_SKEW_SECS) <= now.timestamp() {
            return refuse("the token has expired");
        }
        let issued = claims["iat"]
            .as_i64()
            .and_then(|iat| DateTime::from_timestamp(iat, 0))
            .ok_or_else(|| VerifyError::Rejected("the token has no `iat`".to_owned()))?;
        if issued.timestamp() - CLOCK_SKEW_SECS > now.timestamp() {
            return refuse("the token was issued in the future");
        }
        Ok(issued)
    }
}

impl WebhookVerifier for GooglePubSub {
    async fn verify_incoming(request: &IncomingRequest<'_>) -> Result<Verdict, VerifyError> {
        let source = request.source();
        let service_account = source
            .service_account
            .as_deref()
            .filter(|account| !account.trim().is_empty())
            .ok_or_else(|| VerifyError::Misconfigured("`service_account` is not set".to_owned()))?;
        let malformed = || VerifyError::MalformedHeader("authorization".to_owned());
        let value = request
            .header("authorization")
            .ok_or_else(|| VerifyError::MissingHeader("authorization".to_owned()))?
            .trim();
        let (scheme, token) = value.split_once(' ').ok_or_else(malformed)?;
        if !scheme.eq_ignore_ascii_case("bearer") {
            return Err(malformed());
        }
        let jws = Jws::parse(token.trim()).map_err(|_| malformed())?;
        if jws.header().alg != Algorithm::RS256 {
            return Err(VerifyError::Rejected(
                "the token is not signed with RS256".to_owned(),
            ));
        }
        let keys = request
            .keys()
            .await?
            .google_keys(jws.header().kid.as_deref(), request.now())
            .await?;
        jws.verify_with(&keys)
            .map_err(|_| VerifyError::SignatureMismatch)?;
        let claims: Value = jws.claims().map_err(|_| malformed())?;
        let audience = source.audience.as_deref().unwrap_or(request.url());
        let issued = Self::check_claims(&claims, audience, service_account, request.now())?;
        let push: PubSubPush = serde_json::from_slice(request.body())
            .map_err(|_| VerifyError::MalformedBody("Pub/Sub push message".to_owned()))?;
        Ok(Verdict::Verified(Verified {
            id: Some(push.message.message_id),
            timestamp: Some(issued),
            event_type: None,
        }))
    }

    fn missing_setting(source: &SourceOptions) -> Option<&'static str> {
        source
            .service_account
            .as_deref()
            .is_none_or(|account| account.trim().is_empty())
            .then_some("service_account")
    }
}

impl WebhookSource for GooglePubSub {
    const NAME: &'static str = "google_pubsub";
    type Verifier = GooglePubSub;
}

/// The body of a Pub/Sub push request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubSubPush {
    /// The message.
    pub message: PubSubMessage,
    /// The subscription (`projects/{project}/subscriptions/{name}`).
    #[serde(default)]
    pub subscription: String,
}

/// A Pub/Sub message.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PubSubMessage {
    /// The payload, base64 (see [`data`](PubSubMessage::data)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// The attributes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
    /// The message id.
    pub message_id: String,
    /// When it was published (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish_time: Option<String>,
    /// The ordering key, when the publisher set one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordering_key: Option<String>,
}

impl PubSubMessage {
    /// The decoded payload (empty when there is none); `None` when `data`
    /// is not base64.
    pub fn data(&self) -> Option<Vec<u8>> {
        let Some(data) = self.data.as_deref() else {
            return Some(Vec::new());
        };
        STANDARD
            .decode(data.trim())
            .or_else(|_| URL_SAFE.decode(data.trim()))
            .ok()
    }

    /// The payload read as JSON of type `T` (Gmail's `{emailAddress,
    /// historyId}`, for instance).
    pub fn data_json<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        let data = self.data().unwrap_or_default();
        serde_json::from_slice(&data)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use http::{HeaderMap, HeaderValue, Method};
    use navree_crypto::jose::{JwkSet, SigningKey};
    use navree_http_client::{FakeResponse, FakeServer, HttpClient};
    use serde_json::json;

    use super::*;
    use crate::keys::SenderKeys;

    const KEY: &str = include_str!("../../tests/fixtures/sources/signer-key.pem");
    const ACCOUNT: &str = "gmail-push@navree-test.iam.gserviceaccount.com";
    const URL: &str = "https://api.example.com/api/hooks/pubsub";

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_792_000_000, 0).unwrap()
    }

    fn signer() -> SigningKey {
        SigningKey::from_pem(KEY).unwrap().with_kid("google-test-1")
    }

    fn claims() -> Value {
        json!({
            "aud": URL,
            "azp": "113774264463038321964",
            "email": ACCOUNT,
            "email_verified": true,
            "exp": now().timestamp() + 3600,
            "iat": now().timestamp() - 5,
            "iss": "https://accounts.google.com",
            "sub": "113774264463038321964",
        })
    }

    fn body() -> Vec<u8> {
        // `data` is `{"emailAddress":"ada@example.com","historyId":"9876543210"}`.
        serde_json::to_vec(&json!({
            "message": {
                "data": STANDARD.encode(br#"{"emailAddress":"ada@example.com","historyId":"9876543210"}"#),
                "messageId": "2070443601311540",
                "message_id": "2070443601311540",
                "publishTime": "2026-10-26T07:59:55.749Z",
                "publish_time": "2026-10-26T07:59:55.749Z",
            },
            "subscription": "projects/navree-test/subscriptions/gmail-push",
        }))
        .unwrap()
    }

    fn fake_keys() -> (Arc<SenderKeys>, FakeServer) {
        let (client, server) = HttpClient::fake();
        let set: JwkSet = signer().public_jwk().into_iter().collect();
        server.on(
            Method::GET,
            GOOGLE_KEYS_URL,
            FakeResponse::new(200)
                .header("cache-control", "public, max-age=19193, must-revalidate")
                .json(&set),
        );
        (Arc::new(SenderKeys::new(client)), server)
    }

    fn source() -> SourceOptions {
        SourceOptions {
            service_account: Some(ACCOUNT.to_owned()),
            ..SourceOptions::default()
        }
    }

    async fn verify(
        source: &SourceOptions,
        token: &str,
        keys: &Arc<SenderKeys>,
    ) -> Result<Verdict, VerifyError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        let body = body();
        let request = IncomingRequest::new(source, &headers, &body)
            .with_url(URL)
            .at(now())
            .with_keys(keys.clone());
        GooglePubSub::verify_incoming(&request).await
    }

    fn token(claims: &Value) -> String {
        signer().sign_jwt(claims).unwrap()
    }

    #[tokio::test]
    async fn tokens_signed_by_google_for_the_service_account_verify() {
        let (keys, server) = fake_keys();
        let verdict = verify(&source(), &token(&claims()), &keys).await;
        let Ok(Verdict::Verified(verified)) = verdict else {
            panic!("{verdict:?}");
        };
        assert_eq!(verified.id.as_deref(), Some("2070443601311540"));
        assert_eq!(
            verified.timestamp.unwrap().timestamp(),
            now().timestamp() - 5
        );
        // Cached: the second request fetches nothing.
        verify(&source(), &token(&claims()), &keys).await.unwrap();
        server.assert_sent_count(1);

        let push: PubSubPush = serde_json::from_slice(&body()).unwrap();
        let gmail: Value = push.message.data_json().unwrap();
        assert_eq!(gmail["historyId"], "9876543210");
        assert_eq!(
            push.subscription,
            "projects/navree-test/subscriptions/gmail-push"
        );
    }

    #[tokio::test]
    async fn the_audience_defaults_to_the_url_and_can_be_configured() {
        let (keys, _server) = fake_keys();
        let mut other = claims();
        other["aud"] = json!("https://api.example.com/other");
        assert!(matches!(
            verify(&source(), &token(&other), &keys).await,
            Err(VerifyError::Rejected(_))
        ));
        let configured = SourceOptions {
            audience: Some("https://api.example.com/other".to_owned()),
            ..source()
        };
        verify(&configured, &token(&other), &keys).await.unwrap();
    }

    #[tokio::test]
    async fn wrong_claims_are_rejected() {
        let (keys, _server) = fake_keys();
        let cases: [(&str, Value); 6] = [
            ("iss", json!("https://evil.example")),
            ("email", json!("someone-else@other.iam.gserviceaccount.com")),
            ("email_verified", json!(false)),
            ("exp", json!(now().timestamp() - 61)),
            ("iat", json!(now().timestamp() + 61)),
            ("aud", json!(["https://api.example.com/x"])),
        ];
        for (claim, value) in cases {
            let mut claims = claims();
            claims[claim] = value;
            assert!(
                matches!(
                    verify(&source(), &token(&claims), &keys).await,
                    Err(VerifyError::Rejected(_))
                ),
                "{claim}"
            );
        }
        // Within the skew, and with the other issuer spelling.
        let mut claims = claims();
        claims["exp"] = json!(now().timestamp() - 30);
        claims["iss"] = json!("accounts.google.com");
        claims["email_verified"] = json!("true");
        verify(&source(), &token(&claims), &keys).await.unwrap();
    }

    #[tokio::test]
    async fn other_signers_and_malformed_headers_fail() {
        let (keys, _server) = fake_keys();
        // A token signed by a key Google does not publish.
        let stranger = SigningKey::generate_es256().with_kid("google-test-1");
        assert!(matches!(
            verify(&source(), &stranger.sign_jwt(&claims()).unwrap(), &keys).await,
            Err(VerifyError::Rejected(_))
        ));
        let other_rsa = signer().sign_jwt(&claims()).unwrap();
        let mut parts: Vec<&str> = other_rsa.split('.').collect();
        let forged = token(&json!({ "email": "x" }));
        parts[2] = forged.split('.').nth(2).unwrap();
        assert_eq!(
            verify(&source(), &parts.join("."), &keys).await,
            Err(VerifyError::SignatureMismatch)
        );
        assert_eq!(
            verify(&source(), "not-a-token", &keys).await,
            Err(VerifyError::MalformedHeader("authorization".to_owned()))
        );
        let headers = HeaderMap::new();
        let source = source();
        let request = IncomingRequest::new(&source, &headers, b"{}").with_keys(keys);
        assert_eq!(
            GooglePubSub::verify_incoming(&request).await,
            Err(VerifyError::MissingHeader("authorization".to_owned()))
        );
    }

    #[tokio::test]
    async fn an_unknown_key_id_refreshes_the_set_at_most_once_a_minute() {
        let (keys, server) = fake_keys();
        verify(&source(), &token(&claims()), &keys).await.unwrap();
        let rotated = SigningKey::from_pem(KEY).unwrap().with_kid("google-test-2");
        let rotated_token = rotated.sign_jwt(&claims()).unwrap();
        // Unknown kid within a minute of the last fetch: no new fetch.
        assert_eq!(
            verify(&source(), &rotated_token, &keys).await,
            Err(VerifyError::SignatureMismatch)
        );
        server.assert_sent_count(1);
    }

    #[tokio::test]
    async fn unreachable_keys_are_unavailable() {
        let (client, server) = HttpClient::fake();
        server.on(
            Method::GET,
            GOOGLE_KEYS_URL,
            FakeResponse::connection_failed(),
        );
        let keys = Arc::new(SenderKeys::new(client));
        assert_eq!(
            verify(&source(), &token(&claims()), &keys).await,
            Err(VerifyError::Unavailable("Google's signing keys".to_owned()))
        );
    }

    #[test]
    fn the_source_needs_a_service_account() {
        assert_eq!(
            GooglePubSub::missing_setting(&SourceOptions::default()),
            Some("service_account")
        );
        assert_eq!(GooglePubSub::missing_setting(&source()), None);
    }

    #[test]
    fn message_data_decodes_from_either_base64_alphabet() {
        let message = PubSubMessage {
            data: Some(URL_SAFE.encode([0xfb, 0xff])),
            ..PubSubMessage::default()
        };
        assert_eq!(message.data(), Some(vec![0xfb, 0xff]));
        assert_eq!(PubSubMessage::default().data(), Some(Vec::new()));
        let bad = PubSubMessage {
            data: Some("***".to_owned()),
            ..PubSubMessage::default()
        };
        assert_eq!(bad.data(), None);
    }
}
