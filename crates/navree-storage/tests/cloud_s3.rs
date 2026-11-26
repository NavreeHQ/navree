//! Runs the shared [`cloud_suite`] against [`S3Store`] backed by a live S3
//! emulator (`MinIO` or `RustFS`, configured through `NAVREE_TEST_S3_URL`).

#![cfg(feature = "s3")]

mod cloud_suite;

use std::sync::Arc;

use chrono::Utc;
use http::Method;
use navree_storage::S3Config;
use navree_storage::cloud::http::{Client, Request};
use navree_storage::cloud::s3::S3Store;
use navree_storage::cloud::s3::sigv4::{Credentials, sign_request};
use navree_storage::store::{PrefixedStore, Store, Url};
use tokio::sync::OnceCell;

use crate::cloud_suite::{Subject, emulator_url};

static BUCKET_READY: OnceCell<()> = OnceCell::const_new();

/// Ensures `bucket` exists on the S3 emulator (`200 OK` created, `409`
/// `BucketAlreadyOwnedByYou` / `BucketAlreadyExists`).
async fn ensure_bucket(endpoint: &str, bucket: &str, access_key: &str, secret_key: &str) {
    let client = Client::for_endpoint(Some(endpoint)).expect("S3 client");
    let url = Url::parse(&format!("{endpoint}/{bucket}")).expect("bucket URL");
    let mut request = Request::new(Method::PUT, url);
    let creds = Credentials::new(access_key, secret_key);
    sign_request(&mut request, &creds, "us-east-1", Utc::now()).expect("sign CreateBucket");
    let answer = client.send(request).await.expect("S3 emulator answers");
    let status = answer.status().as_u16();
    assert!(
        status == 200 || status == 409,
        "creating S3 bucket `{bucket}` at `{endpoint}` answered {status}"
    );
}

async fn connect(test: &str) -> Option<Subject> {
    let raw_url = emulator_url("NAVREE_TEST_S3_URL", test)?;
    let parsed = Url::parse(&raw_url).expect("NAVREE_TEST_S3_URL is a valid URL");
    let bucket = parsed.path().trim_matches('/').to_owned();
    assert!(
        !bucket.is_empty(),
        "NAVREE_TEST_S3_URL must name the bucket in its path"
    );
    let endpoint = parsed.origin().ascii_serialization();
    let access_key = if parsed.username().is_empty() {
        "minioadmin".to_owned()
    } else {
        parsed.username().to_owned()
    };
    let secret_key = parsed.password().unwrap_or("minioadmin").to_owned();

    BUCKET_READY
        .get_or_init(|| ensure_bucket(&endpoint, &bucket, &access_key, &secret_key))
        .await;

    let config = S3Config::new(&bucket)
        .endpoint(&endpoint)
        .region("us-east-1")
        .credentials(&access_key, &secret_key);
    let raw: Arc<dyn Store> = Arc::new(S3Store::new(config).expect("S3Store builds"));
    let prefix = format!("suite-{test}-{}", navree_crypto::random_token(10));
    let store = Arc::new(PrefixedStore::new(raw.clone(), prefix));

    Some(Subject {
        driver: "s3",
        store,
        raw,
        binds_put_headers: true,
        unsigned_put_headers: &[],
        guard: None,
    })
}

declare_cloud_suite!(connect);
