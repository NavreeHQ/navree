//! Runs the shared [`cloud_suite`] against [`GcsStore`] backed by
//! `fsouza/fake-gcs-server` (`NAVREE_TEST_GCS_URL`), with a local loopback
//! OAuth 2.0 token server verifying every RS256 service-account JWT assertion.

#![cfg(feature = "gcs")]

mod cloud_suite;

use std::sync::Arc;

use navree_crypto::jose::{Jws, SigningKey};
use navree_storage::GcsConfig;
use navree_storage::cloud::gcs::GcsStore;
use navree_storage::store::{PrefixedStore, Store, Url};
use percent_encoding::percent_decode_str;
use serde_json::json;
use tokio::sync::OnceCell;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request as MockRequest, ResponseTemplate};

use crate::cloud_suite::{Subject, emulator_url};

const PRIVATE_KEY: &str = include_str!("fixtures/rsa-2048.pem");
const CLIENT_EMAIL: &str = "harpreet@agra-media.iam.gserviceaccount.com";

static BUCKET_READY: OnceCell<()> = OnceCell::const_new();

/// Ensures `bucket` exists on `fake-gcs-server` (`200 OK` created, `409`
/// already exists).
async fn ensure_bucket(endpoint: &str, bucket: &str) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let url = format!("{endpoint}/storage/v1/b?project=navree-test");
    let reply = reqwest::Client::new()
        .post(&url)
        .json(&json!({ "name": bucket }))
        .send()
        .await
        .expect("fake-gcs-server answers bucket creation");
    let status = reply.status().as_u16();
    assert!(
        status == 200 || status == 409,
        "creating GCS bucket `{bucket}` at `{endpoint}` answered {status}"
    );
}

/// Starts a local OAuth 2.0 token endpoint on `127.0.0.1:0` that verifies the
/// service account's RS256 JWT assertion before issuing a bearer token.
async fn start_token_server() -> MockServer {
    let server = MockServer::start().await;
    let verifying_key = SigningKey::from_pem(PRIVATE_KEY)
        .expect("test RSA key parses")
        .verifying_key();
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(move |req: &MockRequest| {
            let body = std::str::from_utf8(&req.body).unwrap_or_default();
            let assertion = body
                .split('&')
                .find_map(|pair| pair.strip_prefix("assertion="))
                .map(|encoded| {
                    percent_decode_str(&encoded.replace('+', "%20"))
                        .decode_utf8()
                        .expect("UTF-8 assertion")
                        .into_owned()
                })
                .expect("JWT assertion in token exchange");
            Jws::parse(&assertion)
                .expect("compact JWS")
                .verify(&verifying_key)
                .expect("valid RS256 signature on service-account assertion");
            ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "gcs-emulator-token-agra",
                "expires_in": 3600,
                "token_type": "Bearer",
            }))
        })
        .mount(&server)
        .await;
    server
}

async fn connect(test: &str) -> Option<Subject> {
    let raw_url = emulator_url("NAVREE_TEST_GCS_URL", test)?;
    let parsed = Url::parse(&raw_url).expect("NAVREE_TEST_GCS_URL is a valid URL");
    let bucket = parsed.path().trim_matches('/').to_owned();
    assert!(
        !bucket.is_empty(),
        "NAVREE_TEST_GCS_URL must name the bucket in its path"
    );
    let endpoint = parsed.origin().ascii_serialization();

    BUCKET_READY
        .get_or_init(|| ensure_bucket(&endpoint, &bucket))
        .await;

    let token_server = Arc::new(start_token_server().await);
    let key_json = json!({
        "type": "service_account",
        "private_key": PRIVATE_KEY,
        "private_key_id": "agra-test-key-1",
        "client_email": CLIENT_EMAIL,
        "token_uri": format!("{}/token", token_server.uri()),
    });
    let config = GcsConfig {
        service_account_key: Some(key_json.to_string().into()),
        endpoint: Some(endpoint),
        ..GcsConfig::new(&bucket)
    };
    // 1 MiB (4 * 256 KiB) chunk size ensures the 5.5 MiB multipart test flushes
    // multiple intermediate 308 chunks before completing on fake-gcs-server.
    let gcs_store = GcsStore::builder(config)
        .chunk_size(1024 * 1024)
        .build()
        .expect("GcsStore builds");
    let raw: Arc<dyn Store> = Arc::new(gcs_store);
    let prefix = format!("suite-{test}-{}", navree_crypto::random_token(10));
    let store = Arc::new(PrefixedStore::new(raw.clone(), prefix));

    Some(Subject {
        driver: "gcs",
        store,
        raw,
        binds_put_headers: true,
        unsigned_put_headers: &[],
        guard: Some(token_server),
    })
}

declare_cloud_suite!(connect);
