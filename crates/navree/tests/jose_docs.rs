//! The examples of the signatures and keys docs page
//! (`apps/website/content/docs/security/signatures-and-keys.mdx`), written as
//! an app writes them (the facade, the prelude and the macros), compiled and
//! run.

#![cfg(all(
    feature = "crypto",
    feature = "cache",
    feature = "http-client",
    feature = "testing"
))]

use navree::axum::http::Method;
use navree::crypto::jose::{
    Algorithm, Header, JoseError, Jwk, JwkSet, Jws, SigningKey, VerifyingKey,
};
use navree::http_client::{FakeResponse, HttpClient};
use navree::prelude::*;
use navree::testing::TestApp;
use serde_json::{Value, json};

/// An RSA key in PKCS#8, from navree-crypto's fixtures.
const RSA_PEM: &str = include_str!("../../navree-crypto/tests/fixtures/jose/rsa-2048.pem");

/// A self-signed certificate of [`RSA_PEM`]'s key.
const RSA_CERT: &str = include_str!("../../navree-crypto/tests/fixtures/jose/rsa-2048-cert.pem");

/// "Verifying keys": a certificate (PEM or DER) gives its public key.
#[test]
fn certificates_give_their_public_key() -> Result<(), JoseError> {
    use base64::Engine as _;
    let body: String = RSA_CERT
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(body)
        .expect("base64");
    let signature = SigningKey::from_pem(RSA_PEM)?.sign(b"signed")?;
    for key in [
        VerifyingKey::from_pem(RSA_CERT)?,
        VerifyingKey::from_certificate_der(&der)?,
    ] {
        key.verify(Algorithm::RS256, b"signed", &signature)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// "Signing a token": the page's function, as written.

mod assertion {
    use navree::crypto::jose::{JoseError, SigningKey};
    use serde::Serialize;

    #[derive(Serialize)]
    struct Assertion<'a> {
        iss: &'a str,
        aud: &'a str,
        iat: i64,
        exp: i64,
    }

    /// The signed assertion the partner API expects, valid for five minutes.
    pub fn partner_assertion(pem: &str, key_id: &str, now: i64) -> Result<String, JoseError> {
        let key = SigningKey::from_pem(pem)?.with_kid(key_id);
        key.sign_jwt(&Assertion {
            iss: "acme",
            aud: "https://api.partner.example",
            iat: now,
            exp: now + 300,
        })
    }
}

/// The decoded header of a compact token.
fn header_of(token: &str) -> Value {
    let jws = Jws::parse(token).unwrap();
    serde_json::to_value(jws.header()).unwrap()
}

#[test]
fn the_partner_assertion_is_an_es256_jwt() {
    let key = SigningKey::generate_es256();
    let pem = key.to_pkcs8_pem().unwrap();
    let token = assertion::partner_assertion(&pem, "key-2026", 1_761_436_800).unwrap();
    assert_eq!(
        header_of(&token),
        json!({"alg": "ES256", "kid": "key-2026", "typ": "JWT"})
    );
    let jws = Jws::parse(&token).unwrap();
    jws.verify(&key.verifying_key()).unwrap();
    let claims: Value = jws.claims().unwrap();
    assert_eq!(claims["exp"], 1_761_437_100);
    assert_eq!(claims["aud"], "https://api.partner.example");
    // The same key in one environment variable, with `\n` escapes.
    let escaped = pem.trim().replace('\n', "\\n");
    assert!(assertion::partner_assertion(&escaped, "k", 0).is_ok());
}

#[test]
fn custom_headers_sign_with_sign_json() -> Result<(), JoseError> {
    let key = SigningKey::generate_es256().with_kid("KEYID");
    let claims = json!({"iss": "TEAMID", "sub": "com.acme.web"});
    let thumbprint = "x5t-value";

    // Sign in with Apple wants `{"alg":"ES256","kid":"<key id>"}`, no `typ`.
    let token = key.sign_json(&key.header(), &claims)?;
    assert_eq!(header_of(&token), json!({"alg": "ES256", "kid": "KEYID"}));

    // Extra header parameters.
    let header = key.header().with_typ("JWT").with_param("x5t", thumbprint);
    let token = key.sign_json(&header, &claims)?;
    assert_eq!(
        header_of(&token),
        json!({"alg": "ES256", "kid": "KEYID", "typ": "JWT", "x5t": "x5t-value"})
    );

    // A header naming another algorithm is refused.
    assert_eq!(
        key.sign_json(&Header::new(Algorithm::EdDSA), &claims),
        Err(JoseError::NoKey)
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// "Keys".

#[test]
fn generated_keys_export_as_pkcs8() {
    let key = SigningKey::generate_ed25519();
    let pem = key.to_pkcs8_pem().expect("Ed25519 keys export as PKCS#8");
    assert!(pem.starts_with("-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEI"));
    assert_eq!(pem.lines().count(), 3);
    assert_eq!(key.to_pkcs8_der().unwrap().len(), 48);
    assert_eq!(
        SigningKey::hmac(Algorithm::HS256, [0_u8; 32])
            .unwrap()
            .to_pkcs8_pem(),
        None
    );
}

#[test]
fn key_ids_and_algorithms() -> Result<(), JoseError> {
    let key = SigningKey::generate_ed25519().with_thumbprint_kid();
    assert_eq!(
        key.kid(),
        Some(key.public_jwk().unwrap().thumbprint()?.as_str())
    );

    let pem = RSA_PEM.to_owned();
    let key = SigningKey::from_pem(&pem)?.with_algorithm(Algorithm::PS256)?;
    assert_eq!(key.algorithm(), Algorithm::PS256);

    assert!(
        SigningKey::generate_es256()
            .with_algorithm(Algorithm::ES384)
            .is_err()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// "Verifying a token": the page's function, as written.

mod callback {
    use navree::crypto::jose::{Jws, VerifyingKey};
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub struct PartnerCallback {
        pub sub: String,
        pub exp: i64,
    }

    /// The claims of a callback token the partner signed, if it hasn't expired.
    pub fn verify_callback(token: &str, public_pem: &str, now: i64) -> Option<PartnerCallback> {
        let key = VerifyingKey::from_pem(public_pem).ok()?;
        let jws = Jws::parse(token).ok()?;
        jws.verify(&key).ok()?;
        let claims: PartnerCallback = jws.claims().ok()?;
        (claims.exp > now).then_some(claims)
    }
}

/// `der` as a PEM block.
fn pem(label: &str, der: &[u8]) -> String {
    use base64::Engine as _;
    let body = base64::engine::general_purpose::STANDARD.encode(der);
    format!("-----BEGIN {label}-----\n{body}\n-----END {label}-----\n")
}

/// The SubjectPublicKeyInfo of an RSA key, from its JWK (DER by hand).
fn rsa_public_pem(jwk: &Jwk) -> String {
    use base64::Engine as _;
    let decode = |v: &Option<String>| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(v.as_deref().unwrap())
            .unwrap()
    };
    let element = |tag: u8, body: &[u8]| {
        let mut out = vec![tag];
        let len = body.len();
        if len < 0x80 {
            out.push(len as u8);
        } else if len < 0x100 {
            out.extend([0x81, len as u8]);
        } else {
            out.extend([0x82, (len >> 8) as u8, len as u8]);
        }
        out.extend_from_slice(body);
        out
    };
    let int = |bytes: Vec<u8>| {
        let mut value = if bytes[0] & 0x80 != 0 {
            vec![0]
        } else {
            vec![]
        };
        value.extend(bytes);
        element(0x02, &value)
    };
    let rsa_key = element(0x30, &[int(decode(&jwk.n)), int(decode(&jwk.e))].concat());
    let algorithm = element(
        0x30,
        &[
            element(
                0x06,
                &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01],
            ),
            vec![0x05, 0x00],
        ]
        .concat(),
    );
    let spki = element(
        0x30,
        &[algorithm, element(0x03, &[&[0][..], &rsa_key].concat())].concat(),
    );
    pem("PUBLIC KEY", &spki)
}

#[test]
fn callbacks_verify_until_they_expire() {
    let partner = SigningKey::from_pem(RSA_PEM).unwrap();
    let public_pem = rsa_public_pem(&partner.public_jwk().unwrap());
    let token = partner
        .sign_jwt(&json!({"sub": "order-7", "exp": 2_000}))
        .unwrap();
    let claims = callback::verify_callback(&token, &public_pem, 1_000).unwrap();
    assert_eq!(claims.sub, "order-7");
    assert_eq!(claims.exp, 2_000);
    assert!(callback::verify_callback(&token, &public_pem, 3_000).is_none());
    // Signed by someone else.
    let other = SigningKey::generate_es256()
        .sign_jwt(&json!({"sub": "order-7", "exp": 2_000}))
        .unwrap();
    assert!(callback::verify_callback(&other, &public_pem, 1_000).is_none());
}

#[test]
fn pinned_keys_verify_one_algorithm() -> Result<(), JoseError> {
    let jwk = SigningKey::from_pem(RSA_PEM)?
        .verifying_key()
        .to_jwk()
        .unwrap();
    let key = VerifyingKey::from_jwk(&jwk)?.with_algorithm(Algorithm::RS256)?;
    let ps256 = SigningKey::from_pem(RSA_PEM)?
        .with_algorithm(Algorithm::PS256)?
        .sign_jwt(&json!({}))?;
    assert_eq!(Jws::parse(&ps256)?.verify(&key), Err(JoseError::NoKey));
    let rs256 = SigningKey::from_pem(RSA_PEM)?.sign_jwt(&json!({}))?;
    Jws::parse(&rs256)?.verify(&key)?;
    // A JWK with `alg` is pinned already.
    let pinned = VerifyingKey::from_jwk(&Jwk {
        alg: Some("RS256".into()),
        ..jwk
    })?;
    assert_eq!(pinned.algorithm(), Some(Algorithm::RS256));
    Ok(())
}

// ---------------------------------------------------------------------------
// "Verifying against a key set": the page's service, as written.

mod id_tokens {
    use std::time::Duration;

    use navree::cache::Cache;
    use navree::crypto::jose::{JwkSet, Jws};
    use navree::http_client::HttpClient;
    use navree::prelude::*;
    use serde_json::Value;

    const GOOGLE_KEYS: &str = "https://www.googleapis.com/oauth2/v3/certs";

    /// Verifies Google ID tokens.
    #[injectable]
    pub struct GoogleIdTokens {
        http: Arc<HttpClient>,
        cache: Arc<Cache>,
    }

    impl GoogleIdTokens {
        /// The claims of an ID token Google issued to `client_id`.
        pub async fn verify(&self, token: &str, client_id: &str, now: i64) -> Result<Value> {
            let jws = Jws::parse(token).map_err(|_| AppError::unauthenticated())?;
            let keys: JwkSet = self
                .cache
                .remember("google-jwks", Duration::from_secs(3600), || async {
                    Ok(self.http.get(GOOGLE_KEYS).send().await?.json::<JwkSet>()?)
                })
                .await
                .map_err(|_| AppError::service_unavailable("Google's keys are unavailable"))?;
            jws.verify_with(&keys)
                .map_err(|_| AppError::unauthenticated())?;
            let claims: Value = jws.claims().map_err(|_| AppError::unauthenticated())?;
            let issuer = claims["iss"].as_str().unwrap_or_default();
            let valid = matches!(
                issuer,
                "https://accounts.google.com" | "accounts.google.com"
            ) && claims["aud"] == client_id
                && claims["exp"].as_i64().is_some_and(|exp| exp > now);
            if !valid {
                return Err(AppError::unauthenticated());
            }
            Ok(claims)
        }
    }

    /// Provides [`GoogleIdTokens`].
    #[module(providers = [GoogleIdTokens], exports = [GoogleIdTokens])]
    pub struct SsoModule;
}

#[tokio::test]
async fn google_id_tokens_verify_against_the_cached_key_set() {
    // A stand-in for Google: an RSA key published in a key set.
    let google = SigningKey::from_pem(RSA_PEM).unwrap().with_kid("g-1");
    let jwks: JwkSet = google.public_jwk().into_iter().collect();
    let (client, server) = HttpClient::fake();
    server.on(
        Method::GET,
        "https://www.googleapis.com/oauth2/v3/certs",
        FakeResponse::new(200).json(&jwks),
    );
    let app = TestApp::builder()
        .module(CacheModule::from_config())
        .module(HttpClientModule::from_config())
        .module(id_tokens::SsoModule)
        .override_value(Arc::new(client))
        .build()
        .await;
    let tokens = app.get::<id_tokens::GoogleIdTokens>().await;

    let claims = |aud: &str, exp: i64| json!({"iss": "https://accounts.google.com", "aud": aud, "sub": "1093", "exp": exp});
    let token = google.sign_jwt(&claims("client-1", 2_000)).unwrap();
    let verified = tokens.verify(&token, "client-1", 1_000).await.unwrap();
    assert_eq!(verified["sub"], "1093");
    // The key set was fetched once and cached.
    tokens.verify(&token, "client-1", 1_500).await.unwrap();
    server.assert_sent_count(1);

    // Another audience, an expired token, another signer.
    assert!(tokens.verify(&token, "client-2", 1_000).await.is_err());
    assert!(tokens.verify(&token, "client-1", 3_000).await.is_err());
    let forged = SigningKey::generate_es256()
        .with_kid("g-1")
        .sign_jwt(&claims("client-1", 2_000))
        .unwrap();
    assert!(tokens.verify(&forged, "client-1", 1_000).await.is_err());
}

// ---------------------------------------------------------------------------
// "Publishing a key set": the page's controller, as written.

mod jwks {
    use navree::crypto::jose::{JwkSet, SigningKey};
    use navree::prelude::*;

    /// The keys that sign our tokens: the current one first, then the previous
    /// ones still published.
    pub struct TokenKeys(pub Vec<SigningKey>);

    #[injectable]
    pub struct JwksController {
        keys: Arc<TokenKeys>,
    }

    #[controller("/tokens")]
    impl JwksController {
        /// `GET /.well-known/jwks.json`
        #[get("!/.well-known/jwks.json")]
        #[public]
        async fn jwks(&self) -> Json<JwkSet> {
            Json(
                self.keys
                    .0
                    .iter()
                    .filter_map(SigningKey::public_jwk)
                    .collect(),
            )
        }
    }

    /// Mounts [`JwksController`] with the keys.
    #[module(controllers = [JwksController], register = Self::wire)]
    pub struct TokensModule {
        pub keys: Vec<SigningKey>,
    }

    impl TokensModule {
        fn wire(&self, m: &mut ModuleBuilder) {
            m.value(TokenKeys(self.keys.clone()));
        }
    }
}

#[tokio::test]
async fn the_jwks_endpoint_publishes_public_keys_only() {
    use base64::Engine as _;
    // RFC 8037's key, as on the page.
    let seed: [u8; 32] = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode("nWGxne_9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A")
        .unwrap()
        .try_into()
        .unwrap();
    let current = SigningKey::ed25519_from_seed(&seed).with_thumbprint_kid();
    let previous = SigningKey::generate_es256().with_kid("2025-10");
    let hmac = SigningKey::hmac(Algorithm::HS256, [1_u8; 32]).unwrap();
    let app = TestApp::builder()
        .module(jwks::TokensModule {
            keys: vec![current.clone(), previous.clone(), hmac],
        })
        .build()
        .await;
    let body = app
        .client()
        .get("/.well-known/jwks.json")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body["keys"][0],
        json!({
            "kty": "OKP",
            "use": "sig",
            "alg": "EdDSA",
            "kid": "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k",
            "crv": "Ed25519",
            "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"
        })
    );
    assert_eq!(
        body["keys"].as_array().unwrap().len(),
        2,
        "HMAC keys are skipped"
    );
    // A client verifies both keys' tokens against the published set.
    let published: JwkSet = serde_json::from_value(body).unwrap();
    for key in [&current, &previous] {
        let token = key.sign_jwt(&json!({"sub": "ada"})).unwrap();
        let jws = Jws::parse(&token).unwrap();
        assert_eq!(
            jws.verify_with(&published).unwrap().kid.as_deref(),
            key.kid()
        );
    }
    assert!(published.keys.iter().all(|key| !key.is_private()));
}
