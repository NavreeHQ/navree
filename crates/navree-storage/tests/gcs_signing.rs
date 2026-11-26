//! Google Cloud Storage signing, credential resolution, JSON API operations
//! and mid-upload token refresh (`navree_storage::cloud::gcs`).

#![cfg(feature = "gcs")]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use navree_crypto::jose::{Algorithm, Jws, SigningKey};
use navree_crypto::sha256_hex;
use navree_storage::GcsConfig;
use navree_storage::cloud::gcs::signed_url::{self, ALGORITHM};
use navree_storage::cloud::gcs::token::{DEFAULT_SCOPE, RsaSigningKey};
use navree_storage::cloud::gcs::{GcsStore, UPLOAD_QUANTUM};
use navree_storage::store::{HeaderName, HeaderValue, Method, Store, StoreError, Url};
use percent_encoding::percent_decode_str;
use serde::Deserialize;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request as MockRequest, ResponseTemplate};

const PRIVATE_KEY: &str = include_str!("fixtures/rsa-2048.pem");

/// A controllable clock anchored at `2026-11-25T12:00:00Z` (`1_795_608_000`).
#[derive(Clone)]
struct Hands(Arc<AtomicU64>);

impl Hands {
    const START_SECS: u64 = 1_795_608_000;

    fn new() -> Self {
        Self(Arc::new(AtomicU64::new(0)))
    }

    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH
            + Duration::from_secs(Self::START_SECS + self.0.load(Ordering::SeqCst))
    }

    fn advance(&self, by: Duration) {
        self.0.fetch_add(by.as_secs(), Ordering::SeqCst);
    }
}

fn decode_form(body: &[u8]) -> BTreeMap<String, String> {
    let text = std::str::from_utf8(body).unwrap();
    text.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let decode = |s: &str| {
                percent_decode_str(&s.replace('+', "%20"))
                    .decode_utf8()
                    .unwrap()
                    .into_owned()
            };
            (decode(k), decode(v))
        })
        .collect()
}

#[tokio::test]
async fn a_v4_signed_url_matches_googles_example() {
    let hands = Hands::new();
    let email = "harpreet@agra-media.iam.gserviceaccount.com";
    let verifier = SigningKey::from_pem(PRIVATE_KEY).unwrap().verifying_key();

    // 1. Canonical V4 GET against storage.googleapis.com, matching Google's
    //    documented six-line canonical request and four-line string-to-sign.
    let bucket_url = Url::parse("https://storage.googleapis.com/agra-media").unwrap();
    let prepared = signed_url::prepare(
        &bucket_url,
        "challans/CH-1101.pdf",
        &Method::GET,
        Duration::from_secs(900),
        &[],
        email,
        hands.now(),
    )
    .unwrap();

    let expected_canonical = concat!(
        "GET\n",
        "/agra-media/challans/CH-1101.pdf\n",
        "X-Goog-Algorithm=GOOG4-RSA-SHA256",
        "&X-Goog-Credential=harpreet%40agra-media.iam.gserviceaccount.com%2F20261125%2Fauto%2Fstorage%2Fgoog4_request",
        "&X-Goog-Date=20261125T120000Z",
        "&X-Goog-Expires=900",
        "&X-Goog-SignedHeaders=host\n",
        "host:storage.googleapis.com\n",
        "\n",
        "host\n",
        "UNSIGNED-PAYLOAD"
    );
    assert_eq!(prepared.canonical_request, expected_canonical);
    assert_eq!(prepared.signed_headers, "host");

    let expected_to_sign = format!(
        "{ALGORITHM}\n20261125T120000Z\n20261125/auto/storage/goog4_request\n{}",
        sha256_hex(expected_canonical.as_bytes())
    );
    assert_eq!(prepared.string_to_sign, expected_to_sign);

    // 2. End-to-end signing through GcsStore (local RSA key, zero network traffic).
    let key_json = json!({
        "type": "service_account",
        "private_key": PRIVATE_KEY,
        "private_key_id": "key-agra-1",
        "client_email": email,
    });
    let clock = hands.clone();
    let store = GcsStore::builder(GcsConfig {
        service_account_key: Some(key_json.to_string().into()),
        ..GcsConfig::new("agra-media")
    })
    .env(std::iter::empty::<(String, String)>())
    .clock(move || clock.now())
    .build()
    .unwrap();

    let get_link = store
        .signed_url(
            "challans/CH-1101.pdf",
            Method::GET,
            Duration::from_secs(900),
        )
        .await
        .unwrap();
    let (without_sig, sig_hex) = get_link.as_str().split_once("&X-Goog-Signature=").unwrap();
    assert_eq!(
        without_sig,
        "https://storage.googleapis.com/agra-media/challans/CH-1101.pdf?\
         X-Goog-Algorithm=GOOG4-RSA-SHA256&\
         X-Goog-Credential=harpreet%40agra-media.iam.gserviceaccount.com%2F20261125%2Fauto%2Fstorage%2Fgoog4_request&\
         X-Goog-Date=20261125T120000Z&\
         X-Goog-Expires=900&\
         X-Goog-SignedHeaders=host"
    );
    let sig_bytes = hex::decode(sig_hex).unwrap();
    verifier
        .verify(Algorithm::RS256, expected_to_sign.as_bytes(), &sig_bytes)
        .unwrap();

    // 3. Signed PUT binding Content-Type and x-goog-content-length-range on a
    //    key with spaces, `+`, `%` and Devanagari characters.
    let bound_headers = [
        (
            HeaderName::from_static("x-goog-content-length-range"),
            HeaderValue::from_static("0,5000000"),
        ),
        (
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("  image/png  "),
        ),
    ];
    let put_key = "tenants/ludhiana/चालान + रसीद 100%.png";
    let put_link = store
        .signed_url_with(
            put_key,
            Method::PUT,
            Duration::from_secs(600),
            &bound_headers,
        )
        .await
        .unwrap();
    assert!(
        put_link
            .as_str()
            .contains("X-Goog-SignedHeaders=content-type%3Bhost%3Bx-goog-content-length-range&"),
        "{put_link}"
    );
    let (_, put_sig_hex) = put_link.as_str().split_once("&X-Goog-Signature=").unwrap();
    let put_prepared = signed_url::prepare(
        &bucket_url,
        put_key,
        &Method::PUT,
        Duration::from_secs(600),
        &bound_headers,
        email,
        hands.now(),
    )
    .unwrap();
    verifier
        .verify(
            Algorithm::RS256,
            put_prepared.string_to_sign.as_bytes(),
            &hex::decode(put_sig_hex).unwrap(),
        )
        .unwrap();

    // 4. Emulator base URL override signs the emulator's host:port and path.
    let clock = hands.clone();
    let emulator_store = GcsStore::builder(GcsConfig {
        service_account_key: Some(key_json.to_string().into()),
        endpoint: Some("http://127.0.0.1:4443".to_owned()),
        ..GcsConfig::new("agra-media")
    })
    .env(std::iter::empty::<(String, String)>())
    .clock(move || clock.now())
    .build()
    .unwrap();
    let emulator_link = emulator_store
        .signed_url("docs/a.txt", Method::GET, Duration::from_secs(60))
        .await
        .unwrap();
    assert!(
        emulator_link
            .as_str()
            .starts_with("http://127.0.0.1:4443/agra-media/docs/a.txt?"),
        "{emulator_link}"
    );

    // 5. Out-of-bounds lifetimes and unsupported methods are refused.
    for bad_ttl in [Duration::ZERO, Duration::from_secs(7 * 24 * 3600 + 1)] {
        assert!(matches!(
            store.signed_url("a.txt", Method::GET, bad_ttl).await,
            Err(StoreError::Failed(_))
        ));
    }
    assert!(matches!(
        store
            .signed_url("a.txt", Method::DELETE, Duration::from_secs(60))
            .await,
        Err(StoreError::Failed(_))
    ));
}

#[derive(Deserialize)]
struct AssertionClaims {
    iss: String,
    scope: String,
    aud: String,
    iat: u64,
    exp: u64,
}

#[tokio::test]
async fn the_jwt_assertion_is_signed_with_rs256_and_the_scopes_asked() {
    let oauth = MockServer::start().await;
    let storage = MockServer::start().await;
    let hands = Hands::new();
    let email = "gurpreet@ludhiana-ledger.iam.gserviceaccount.com";
    let verifier = SigningKey::from_pem(PRIVATE_KEY).unwrap().verifying_key();

    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "ya29.ludhiana-oauth-token",
            "expires_in": 3600,
            "token_type": "Bearer",
        })))
        .expect(1)
        .mount(&oauth)
        .await;

    Mock::given(method("POST"))
        .and(path("/upload/storage/v1/b/ludhiana-ledger/o"))
        .and(header("authorization", "Bearer ya29.ludhiana-oauth-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "challans/CH-2001.pdf",
            "size": "12",
            "updated": "2026-11-25T12:00:00Z",
        })))
        .expect(2)
        .mount(&storage)
        .await;

    let token_uri = format!("{}/token", oauth.uri());
    let key_json = json!({
        "type": "service_account",
        "private_key": PRIVATE_KEY,
        "private_key_id": "kid-ludhiana-01",
        "client_email": email,
        "token_uri": token_uri,
    });
    let clock = hands.clone();
    let store = GcsStore::builder(GcsConfig {
        service_account_key: Some(key_json.to_string().into()),
        endpoint: Some(storage.uri()),
        ..GcsConfig::new("ludhiana-ledger")
    })
    .env(std::iter::empty::<(String, String)>())
    .clock(move || clock.now())
    .build()
    .unwrap();

    // Two writes share one cached token exchange.
    store
        .put(
            "challans/CH-2001.pdf",
            Bytes::from_static("₹1,00,000".as_bytes()),
        )
        .await
        .unwrap();
    store
        .put(
            "challans/CH-2002.pdf",
            Bytes::from_static("₹2,50,000".as_bytes()),
        )
        .await
        .unwrap();

    let token_requests = oauth.received_requests().await.unwrap();
    assert_eq!(token_requests.len(), 1);
    assert_eq!(
        token_requests[0].headers["content-type"].to_str().unwrap(),
        "application/x-www-form-urlencoded"
    );
    let form = decode_form(&token_requests[0].body);
    assert_eq!(
        form.get("grant_type").map(String::as_str),
        Some("urn:ietf:params:oauth:grant-type:jwt-bearer")
    );

    let jws = Jws::parse(form.get("assertion").unwrap()).unwrap();
    assert_eq!(jws.header().alg, Algorithm::RS256);
    assert_eq!(jws.header().typ.as_deref(), Some("JWT"));
    assert_eq!(jws.header().kid.as_deref(), Some("kid-ludhiana-01"));
    jws.verify(&verifier).unwrap();

    let claims: AssertionClaims = jws.claims().unwrap();
    assert_eq!(claims.iss, email);
    assert_eq!(claims.scope, DEFAULT_SCOPE);
    assert_eq!(claims.aud, token_uri);
    assert_eq!(claims.iat, Hands::START_SECS);
    assert_eq!(claims.exp, Hands::START_SECS + 3600);

    // Custom scopes requested explicitly appear space-separated in `scope`.
    let rsa_key = RsaSigningKey::from_pem(PRIVATE_KEY)
        .unwrap()
        .with_kid("kid-amritsar-02");
    let custom_scopes = [
        "https://www.googleapis.com/auth/devstorage.read_only",
        "https://www.googleapis.com/auth/cloud-platform",
    ];
    let custom_jwt = navree_storage::cloud::gcs::token::assertion(
        &rsa_key,
        email,
        &token_uri,
        &custom_scopes,
        hands.now(),
    )
    .unwrap();
    let parsed_custom = Jws::parse(&custom_jwt).unwrap();
    parsed_custom.verify(&verifier).unwrap();
    let custom_claims: AssertionClaims = parsed_custom.claims().unwrap();
    assert_eq!(
        custom_claims.scope,
        "https://www.googleapis.com/auth/devstorage.read_only https://www.googleapis.com/auth/cloud-platform"
    );
}

#[tokio::test]
async fn metadata_server_tokens_are_used_when_nothing_else_is_set() {
    let metadata = MockServer::start().await;
    let storage = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path(
            "/computeMetadata/v1/instance/service-accounts/default/token",
        ))
        .and(header("metadata-flavor", "Google"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "ya29.amritsar-metadata-token",
            "expires_in": 3600,
            "token_type": "Bearer",
        })))
        .expect(1)
        .mount(&metadata)
        .await;

    // JSON API: upload, head, media read, paginated list, delete.
    Mock::given(method("POST"))
        .and(path("/upload/storage/v1/b/amritsar-docs/o"))
        .and(header(
            "authorization",
            "Bearer ya29.amritsar-metadata-token",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "reports/q1.csv",
            "size": "11",
            "updated": "2026-11-25T12:00:00Z",
        })))
        .mount(&storage)
        .await;

    Mock::given(method("GET"))
        .and(path("/storage/v1/b/amritsar-docs/o/reports%2Fq1.csv"))
        .and(header(
            "authorization",
            "Bearer ya29.amritsar-metadata-token",
        ))
        .respond_with(|req: &MockRequest| {
            if req.url.query() == Some("alt=media") {
                ResponseTemplate::new(200)
                    .insert_header("etag", "\"v1\"")
                    .set_body_bytes(b"city,total\n".to_vec())
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "name": "reports/q1.csv",
                    "size": "11",
                    "updated": "2026-11-25T12:00:00Z",
                }))
            }
        })
        .mount(&storage)
        .await;

    Mock::given(method("GET"))
        .and(path("/storage/v1/b/amritsar-docs/o"))
        .and(header(
            "authorization",
            "Bearer ya29.amritsar-metadata-token",
        ))
        .respond_with(|req: &MockRequest| {
            let query = req.url.query().unwrap_or_default();
            if query.contains("pageToken=page-2") {
                ResponseTemplate::new(200).set_body_json(json!({
                    "items": [{
                        "name": "reports/q2.csv",
                        "size": "22",
                        "updated": "2026-11-25T12:05:00Z",
                    }]
                }))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "items": [{
                        "name": "reports/q1.csv",
                        "size": "11",
                        "updated": "2026-11-25T12:00:00Z",
                    }],
                    "nextPageToken": "page-2",
                }))
            }
        })
        .mount(&storage)
        .await;

    Mock::given(method("DELETE"))
        .and(path("/storage/v1/b/amritsar-docs/o/reports%2Fq1.csv"))
        .and(header(
            "authorization",
            "Bearer ya29.amritsar-metadata-token",
        ))
        .respond_with(ResponseTemplate::new(204))
        .mount(&storage)
        .await;

    // Nothing set on GcsConfig; only GCE_METADATA_HOST points at our local metadata double.
    let host_without_scheme = metadata.uri().strip_prefix("http://").unwrap().to_owned();
    let store = GcsStore::builder(GcsConfig {
        endpoint: Some(storage.uri()),
        ..GcsConfig::new("amritsar-docs")
    })
    .env([("GCE_METADATA_HOST", host_without_scheme)])
    .build()
    .unwrap();

    store
        .put("reports/q1.csv", Bytes::from_static(b"city,total\n"))
        .await
        .unwrap();
    let meta = store.head("reports/q1.csv").await.unwrap();
    assert_eq!((meta.path.as_str(), meta.size), ("reports/q1.csv", 11));
    assert_eq!(
        store.read("reports/q1.csv", None).await.unwrap(),
        "city,total\n"
    );
    assert_eq!(
        store.read("reports/q1.csv", Some(0..4)).await.unwrap(),
        "city"
    );
    let listed = store.list("reports").await.unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].path, "reports/q1.csv");
    assert_eq!(listed[1].path, "reports/q2.csv");
    store.delete("reports/q1.csv").await.unwrap();

    // When GOOGLE_APPLICATION_CREDENTIALS (authorized_user) IS set, it wins
    // over the metadata server.
    let adc_oauth = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/adc-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "ya29.adc-user-token",
            "expires_in": 3600,
        })))
        .expect(1)
        .mount(&adc_oauth)
        .await;

    let adc_storage = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/storage/v1/b/amritsar-docs/o/a.txt"))
        .and(header("authorization", "Bearer ya29.adc-user-token"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&adc_storage)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let adc_file = dir.path().join("adc.json");
    std::fs::write(
        &adc_file,
        json!({
            "type": "authorized_user",
            "client_id": "jaipur-cli.apps.googleusercontent.com",
            "client_secret": "jaipur-client-secret",
            "refresh_token": "1//jaipur-refresh-token",
            "token_uri": format!("{}/adc-token", adc_oauth.uri()),
        })
        .to_string(),
    )
    .unwrap();

    let adc_store = GcsStore::builder(GcsConfig {
        endpoint: Some(adc_storage.uri()),
        ..GcsConfig::new("amritsar-docs")
    })
    .env([
        (
            "GOOGLE_APPLICATION_CREDENTIALS",
            adc_file.to_string_lossy().into_owned(),
        ),
        ("GCE_METADATA_HOST", metadata.uri()),
    ])
    .build()
    .unwrap();
    adc_store.delete("a.txt").await.unwrap();
}

#[tokio::test]
async fn sign_blob_signs_when_there_is_no_key() {
    let metadata = MockServer::start().await;
    let iam = MockServer::start().await;
    let hands = Hands::new();
    let email = "simran@jaipur-archive.iam.gserviceaccount.com";
    let signer_key = SigningKey::from_pem(PRIVATE_KEY).unwrap();
    let verifier = signer_key.verifying_key();

    Mock::given(method("GET"))
        .and(path(
            "/computeMetadata/v1/instance/service-accounts/default/token",
        ))
        .and(header("metadata-flavor", "Google"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "ya29.jaipur-meta-token",
            "expires_in": 3600,
            "token_type": "Bearer",
        })))
        .expect(1)
        .mount(&metadata)
        .await;

    Mock::given(method("GET"))
        .and(path(
            "/computeMetadata/v1/instance/service-accounts/default/email",
        ))
        .and(header("metadata-flavor", "Google"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!("{email}\n")))
        .expect(1)
        .mount(&metadata)
        .await;

    Mock::given(method("POST"))
        .and(path(format!(
            "/v1/projects/-/serviceAccounts/{email}:signBlob"
        )))
        .and(header("authorization", "Bearer ya29.jaipur-meta-token"))
        .respond_with(move |req: &MockRequest| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            let payload_b64 = body["payload"].as_str().unwrap();
            let payload = STANDARD.decode(payload_b64).unwrap();
            let sig = signer_key.sign(&payload).unwrap();
            ResponseTemplate::new(200).set_body_json(json!({
                "keyId": "iam-jaipur-key-1",
                "signedBlob": STANDARD.encode(sig),
            }))
        })
        .expect(2)
        .mount(&iam)
        .await;

    let clock = hands.clone();
    let store = GcsStore::builder(GcsConfig::new("jaipur-archive"))
        .env(std::iter::empty::<(String, String)>())
        .metadata_endpoint(metadata.uri())
        .iam_endpoint(iam.uri())
        .clock(move || clock.now())
        .build()
        .unwrap();

    let headers = [
        (
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/pdf"),
        ),
        (
            HeaderName::from_static("x-goog-content-length-range"),
            HeaderValue::from_static("0,1048576"),
        ),
    ];
    let put_url = store
        .signed_url_with(
            "invoices/INV 2026.pdf",
            Method::PUT,
            Duration::from_secs(600),
            &headers,
        )
        .await
        .unwrap();

    assert!(
        put_url
            .as_str()
            .starts_with("https://storage.googleapis.com/jaipur-archive/invoices/INV%202026.pdf?"),
        "{put_url}"
    );
    assert!(
        put_url
            .as_str()
            .contains("X-Goog-Credential=simran%40jaipur-archive.iam.gserviceaccount.com%2F"),
        "{put_url}"
    );
    let (_, sig_hex) = put_url.as_str().split_once("&X-Goog-Signature=").unwrap();

    let bucket_url = Url::parse("https://storage.googleapis.com/jaipur-archive").unwrap();
    let expected = signed_url::prepare(
        &bucket_url,
        "invoices/INV 2026.pdf",
        &Method::PUT,
        Duration::from_secs(600),
        &headers,
        email,
        hands.now(),
    )
    .unwrap();
    verifier
        .verify(
            Algorithm::RS256,
            expected.string_to_sign.as_bytes(),
            &hex::decode(sig_hex).unwrap(),
        )
        .unwrap();

    // A second signed URL reuses both the cached service-account email and the
    // cached metadata bearer token, calling only IAM `:signBlob` again.
    let get_url = store
        .signed_url(
            "invoices/INV 2026.pdf",
            Method::GET,
            Duration::from_secs(300),
        )
        .await
        .unwrap();
    assert!(get_url.as_str().contains("X-Goog-Signature="));
}

#[tokio::test]
async fn an_expired_token_is_refreshed_between_parts() {
    let metadata = MockServer::start().await;
    let storage = MockServer::start().await;
    let hands = Hands::new();

    // Each token lasts 10 minutes (`600` seconds).
    let token_fetches = Arc::new(AtomicUsize::new(0));
    let active_tokens: Arc<Mutex<BTreeMap<String, u64>>> = Arc::new(Mutex::new(BTreeMap::new()));
    {
        let counted = token_fetches.clone();
        let issued = active_tokens.clone();
        let clock = hands.clone();
        Mock::given(method("GET"))
            .and(path(
                "/computeMetadata/v1/instance/service-accounts/default/token",
            ))
            .and(header("metadata-flavor", "Google"))
            .respond_with(move |_: &MockRequest| {
                let n = counted.fetch_add(1, Ordering::SeqCst) + 1;
                let now_secs = clock
                    .now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let secret = format!("ya29.part-token-{n}");
                issued
                    .lock()
                    .unwrap()
                    .insert(secret.clone(), now_secs + 600);
                ResponseTemplate::new(200).set_body_json(json!({
                    "access_token": secret,
                    "expires_in": 600,
                    "token_type": "Bearer",
                }))
            })
            .mount(&metadata)
            .await;
    }

    let assembled: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let part_tokens: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let aborted = Arc::new(AtomicUsize::new(0));
    let storage_uri = storage.uri();

    // Resumable upload initiation (`POST ?uploadType=resumable`).
    {
        let issued = active_tokens.clone();
        let clock = hands.clone();
        let base_uri = storage_uri.clone();
        Mock::given(method("POST"))
            .and(path("/upload/storage/v1/b/ludhiana-ledger/o"))
            .respond_with(move |req: &MockRequest| {
                let Some(auth) = req
                    .headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("Bearer "))
                else {
                    return ResponseTemplate::new(401);
                };
                let now_secs = clock
                    .now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let valid = issued
                    .lock()
                    .unwrap()
                    .get(auth)
                    .is_some_and(|&exp| now_secs < exp);
                if !valid {
                    return ResponseTemplate::new(401).set_body_json(json!({
                        "error": { "code": 401, "status": "UNAUTHENTICATED", "message": auth }
                    }));
                }
                let session = format!(
                    "{base_uri}/upload/storage/v1/b/ludhiana-ledger/o?uploadType=resumable&upload_id=up-1"
                );
                ResponseTemplate::new(200).insert_header("location", session.as_str())
            })
            .mount(&storage)
            .await;
    }

    // Resumable upload parts (`PUT ?uploadType=resumable&upload_id=up-1`).
    {
        let issued = active_tokens.clone();
        let clock = hands.clone();
        let sink = assembled.clone();
        let seen_tokens = part_tokens.clone();
        Mock::given(method("PUT"))
            .and(path("/upload/storage/v1/b/ludhiana-ledger/o"))
            .respond_with(move |req: &MockRequest| {
                let Some(auth) = req
                    .headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("Bearer "))
                else {
                    return ResponseTemplate::new(401);
                };
                let now_secs = clock
                    .now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let valid = issued
                    .lock()
                    .unwrap()
                    .get(auth)
                    .is_some_and(|&exp| now_secs < exp);
                if !valid {
                    return ResponseTemplate::new(401).set_body_json(json!({
                        "error": { "code": 401, "status": "UNAUTHENTICATED", "message": auth }
                    }));
                }
                seen_tokens.lock().unwrap().push(auth.to_owned());
                let range = req.headers["content-range"].to_str().unwrap();
                sink.lock().unwrap().extend_from_slice(&req.body);
                if range.ends_with("/*") {
                    ResponseTemplate::new(308)
                } else {
                    ResponseTemplate::new(200).set_body_json(json!({
                        "name": "exports/2026-q4.bin",
                        "size": sink.lock().unwrap().len().to_string(),
                        "updated": "2026-11-25T12:15:00Z",
                    }))
                }
            })
            .mount(&storage)
            .await;
    }

    // Resumable session cancellation (`DELETE` -> `499 Client Closed Request`).
    {
        let cancelled = aborted.clone();
        Mock::given(method("DELETE"))
            .and(path("/upload/storage/v1/b/ludhiana-ledger/o"))
            .respond_with(move |_: &MockRequest| {
                cancelled.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(499)
            })
            .mount(&storage)
            .await;
    }

    let clock = hands.clone();
    let store = GcsStore::builder(GcsConfig {
        endpoint: Some(storage_uri),
        ..GcsConfig::new("ludhiana-ledger")
    })
    .env(std::iter::empty::<(String, String)>())
    .metadata_endpoint(metadata.uri())
    .chunk_size(UPLOAD_QUANTUM)
    .clock(move || clock.now())
    .build()
    .unwrap();

    let mut upload = store.begin_upload("exports/2026-q4.bin").await.unwrap();
    // 300 KiB > 256 KiB quantum: flushes the first 256 KiB immediately under token 1.
    let first_chunk = vec![0xA5u8; 300 * 1024];
    upload
        .write(Bytes::from(first_chunk.clone()))
        .await
        .unwrap();
    assert_eq!(token_fetches.load(Ordering::SeqCst), 1);

    // Advance 11 minutes: `ya29.part-token-1` is now past its 10-minute lifetime.
    hands.advance(Duration::from_secs(11 * 60));

    // Second write triggers part 2 (`256 KiB`) after refreshing to `ya29.part-token-2`.
    let second_chunk = vec![0x5Au8; 300 * 1024];
    upload
        .write(Bytes::from(second_chunk.clone()))
        .await
        .unwrap();
    upload.finish().await.unwrap();

    assert_eq!(token_fetches.load(Ordering::SeqCst), 2);
    assert_eq!(
        *part_tokens.lock().unwrap(),
        [
            "ya29.part-token-1",
            "ya29.part-token-2",
            "ya29.part-token-2",
        ]
    );
    let mut expected_bytes = first_chunk;
    expected_bytes.extend_from_slice(&second_chunk);
    assert_eq!(*assembled.lock().unwrap(), expected_bytes);

    // Aborting a resumable session accepts Google's `499 Client Closed Request`.
    let doomed = store.begin_upload("exports/aborted.bin").await.unwrap();
    doomed.abort().await.unwrap();
    assert_eq!(aborted.load(Ordering::SeqCst), 1);

    // Debug and Display never expose credentials or tokens.
    let shown = format!("{store} {store:?}");
    assert!(!shown.contains("ya29."), "{shown}");
}
