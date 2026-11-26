//! S3 signing, credential chain, IMDSv2/v1, mid-multipart credential
//! refresh, and secret redaction tests (`navree_storage::cloud::s3`).

#![cfg(feature = "s3")]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use http::StatusCode;
use navree_contracts::Secret;
use navree_crypto::sha256_hex;
use navree_crypto::sigv4::{self, Signer};
use navree_http::AppRouter;
use navree_http::extract::Request as HttpRequest;
use navree_http::response::{IntoResponse, Response as HttpResponse};
use navree_storage::S3Config;
use navree_storage::cloud::ErrorKind;
use navree_storage::cloud::http::Request;
use navree_storage::cloud::retry::RetryPolicy;
use navree_storage::cloud::s3::S3Store;
use navree_storage::cloud::s3::sigv4::{Credentials, presign_url, sign_request};
use navree_storage::store::{HeaderName, HeaderValue, Method, Store, StoreError, Url};
use serde::Deserialize;
use serde_json::json;
use wiremock::matchers::{any, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURE: &str = include_str!("fixtures/sigv4/vectors.json");

#[derive(Deserialize)]
struct FixtureFile {
    suite_credentials: SuiteCredentials,
    suite_vectors: Vec<SuiteVector>,
    s3_header_vectors: Vec<S3HeaderVector>,
    s3_presigned_vectors: Vec<S3PresignedVector>,
}

#[derive(Deserialize)]
struct SuiteCredentials {
    access_key_id: String,
    secret_access_key: String,
    region: String,
    service: String,
    timestamp: String,
}

#[derive(Deserialize)]
struct SuiteVector {
    name: String,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: String,
    signed_headers: String,
    signature: String,
}

#[derive(Deserialize)]
struct S3HeaderVector {
    name: String,
    access_key_id: String,
    secret_access_key: String,
    region: String,
    timestamp: String,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: String,
    content_sha256: String,
    authorization: String,
}

#[derive(Deserialize)]
struct S3PresignedVector {
    name: String,
    access_key_id: String,
    secret_access_key: String,
    region: String,
    timestamp: String,
    expires_secs: u64,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    expected_url: String,
}

fn quick_retry() -> RetryPolicy {
    RetryPolicy {
        max_retries: 2,
        first_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(2),
        ..RetryPolicy::default()
    }
}

fn env_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[derive(Clone)]
struct Hands(Arc<AtomicU64>);

impl Hands {
    fn at(unix_secs: u64) -> Self {
        Self(Arc::new(AtomicU64::new(unix_secs)))
    }

    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(self.0.load(Ordering::SeqCst))
    }

    fn advance_secs(&self, secs: u64) {
        self.0.fetch_add(secs, Ordering::SeqCst);
    }
}

#[test]
fn sigv4_matches_the_published_test_suite() {
    let fixtures: FixtureFile = serde_json::from_str(FIXTURE).unwrap();
    let suite_creds = &fixtures.suite_credentials;
    let signed_at = DateTime::parse_from_rfc3339(&suite_creds.timestamp)
        .unwrap()
        .with_timezone(&Utc);
    let signer = Signer::new(
        Credentials::new(&suite_creds.access_key_id, &suite_creds.secret_access_key),
        &suite_creds.region,
        &suite_creds.service,
    )
    .path_as_sent();

    assert!(!fixtures.suite_vectors.is_empty());
    for vector in &fixtures.suite_vectors {
        let url = Url::parse(&vector.url).unwrap();
        let pairs: Vec<(&str, &str)> = vector
            .headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let headers = signer.sign(
            &vector.method,
            &url,
            &pairs,
            vector.body.as_bytes(),
            signed_at,
        );
        let auth = &headers
            .iter()
            .find(|(name, _)| name == "authorization")
            .unwrap()
            .1;
        assert_eq!(
            *auth,
            format!(
                "AWS4-HMAC-SHA256 Credential={}/20150830/{}/{}/aws4_request, SignedHeaders={}, Signature={}",
                suite_creds.access_key_id,
                suite_creds.region,
                suite_creds.service,
                vector.signed_headers,
                vector.signature,
            ),
            "{}",
            vector.name
        );
    }

    // S3 header-signed requests include and sign x-amz-content-sha256.
    assert!(!fixtures.s3_header_vectors.is_empty());
    for vector in &fixtures.s3_header_vectors {
        let now = DateTime::parse_from_rfc3339(&vector.timestamp)
            .unwrap()
            .with_timezone(&Utc);
        let method: Method = vector.method.parse().unwrap();
        let url = Url::parse(&vector.url).unwrap();
        let mut request = Request::new(method, url).with_body(vector.body.clone());
        for (name, value) in &vector.headers {
            request = request.header(name, value).unwrap();
        }
        let creds = Credentials::new(&vector.access_key_id, &vector.secret_access_key);
        sign_request(&mut request, &creds, &vector.region, now).unwrap();

        assert_eq!(
            request.headers["x-amz-content-sha256"].to_str().unwrap(),
            vector.content_sha256,
            "{}",
            vector.name
        );
        assert_eq!(
            request.headers["authorization"].to_str().unwrap(),
            vector.authorization,
            "{}",
            vector.name
        );
        assert!(request.headers["authorization"].is_sensitive());
    }
}

#[tokio::test]
async fn a_presigned_get_matches_the_documented_example() {
    let fixtures: FixtureFile = serde_json::from_str(FIXTURE).unwrap();
    assert!(!fixtures.s3_presigned_vectors.is_empty());

    for vector in &fixtures.s3_presigned_vectors {
        let now = DateTime::parse_from_rfc3339(&vector.timestamp)
            .unwrap()
            .with_timezone(&Utc);
        let method: Method = vector.method.parse().unwrap();
        let url = Url::parse(&vector.url).unwrap();
        let creds = Credentials::new(&vector.access_key_id, &vector.secret_access_key);
        let bound: Vec<(HeaderName, HeaderValue)> = vector
            .headers
            .iter()
            .map(|(name, value)| {
                (
                    HeaderName::from_bytes(name.as_bytes()).unwrap(),
                    HeaderValue::from_str(value).unwrap(),
                )
            })
            .collect();
        let signed = presign_url(
            &method,
            &url,
            &bound,
            Duration::from_secs(vector.expires_secs),
            &creds,
            &vector.region,
            now,
        )
        .unwrap();
        assert_eq!(signed.as_str(), vector.expected_url, "{}", vector.name);
    }

    // Through S3Store::signed_url on virtual-hosted AWS at the documented instant.
    let doc = &fixtures.s3_presigned_vectors[0];
    let instant = DateTime::parse_from_rfc3339(&doc.timestamp)
        .unwrap()
        .with_timezone(&Utc);
    let fixed_time = SystemTime::UNIX_EPOCH + Duration::from_secs(instant.timestamp() as u64);
    let config = S3Config::new("examplebucket")
        .region(&doc.region)
        .credentials(&doc.access_key_id, &doc.secret_access_key);
    let store = S3Store::with_env_and_clock(&config, &BTreeMap::new(), move || fixed_time).unwrap();
    // On AWS default virtual-hosted addressing, bucket_url is
    // `https://examplebucket.s3.us-east-1.amazonaws.com`; check its signed
    // query matches the SigV4 calculation for that host too.
    let store_link = store
        .signed_url("test.txt", Method::GET, Duration::from_secs(86_400))
        .await
        .unwrap();
    assert_eq!(
        store_link.host_str(),
        Some("examplebucket.s3.us-east-1.amazonaws.com")
    );
    assert!(store_link.as_str().contains("X-Amz-Expires=86400"));

    // Lifetimes outside [1s, 7d], unsupported methods and reserved headers are refused.
    assert!(matches!(
        store
            .signed_url("test.txt", Method::DELETE, Duration::from_secs(60))
            .await,
        Err(StoreError::Failed(_))
    ));
    assert!(matches!(
        store
            .signed_url("test.txt", Method::GET, Duration::ZERO)
            .await,
        Err(StoreError::Failed(_))
    ));
    assert!(matches!(
        store
            .signed_url("test.txt", Method::GET, Duration::from_secs(7 * 86_400 + 1))
            .await,
        Err(StoreError::Failed(_))
    ));
    let reserved = [(
        HeaderName::from_static("content-length"),
        HeaderValue::from_static("100"),
    )];
    assert!(matches!(
        store
            .signed_url_with("test.txt", Method::PUT, Duration::from_secs(60), &reserved)
            .await,
        Err(StoreError::Failed(_))
    ));
}

/// Extracts the access key id and optional `X-Amz-Security-Token` from a
/// presigned GET link of `store`.
async fn signed_identity(store: &S3Store) -> (String, Option<String>) {
    let url = store
        .signed_url(
            "challans/amritsar-01.pdf",
            Method::GET,
            Duration::from_secs(300),
        )
        .await
        .unwrap();
    let pairs: BTreeMap<String, String> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let key_id = pairs["X-Amz-Credential"]
        .split('/')
        .next()
        .unwrap()
        .to_owned();
    let token = pairs.get("X-Amz-Security-Token").cloned();
    (key_id, token)
}

#[tokio::test]
async fn the_credential_chain_order_is_todays() {
    let temp = tempfile::tempdir().unwrap();
    let web_token_path = temp.path().join("eks-token");
    std::fs::write(&web_token_path, "eyJhbGciOiJSUzI1NiJ9.amritsar-v1\n").unwrap();
    let pod_token_path = temp.path().join("pod-token");
    std::fs::write(&pod_token_path, "pod-bearer-ludhiana\n").unwrap();

    // Fake STS server for AssumeRoleWithWebIdentity.
    let sts = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(|req: &wiremock::Request| {
            let body = String::from_utf8_lossy(&req.body);
            assert!(body.contains("Action=AssumeRoleWithWebIdentity"), "{body}");
            assert!(body.contains("Version=2011-06-15"), "{body}");
            assert!(
                body.contains("RoleArn=arn%3Aaws%3Aiam%3A%3A111122223333%3Arole%2Fagra-uploads"),
                "{body}"
            );
            assert!(body.contains("RoleSessionName=jaipur-worker"), "{body}");
            let session_tag = if body.contains("amritsar-v2") {
                "STSKEY_ROTATED"
            } else {
                "STSKEY_FIRST"
            };
            ResponseTemplate::new(200).set_body_string(format!(
                r#"<AssumeRoleWithWebIdentityResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
                  <AssumeRoleWithWebIdentityResult>
                    <Credentials>
                      <AccessKeyId>{session_tag}</AccessKeyId>
                      <SecretAccessKey>sts-secret-value</SecretAccessKey>
                      <SessionToken>sts-token-value</SessionToken>
                      <Expiration>2099-01-01T00:00:00Z</Expiration>
                    </Credentials>
                  </AssumeRoleWithWebIdentityResult>
                </AssumeRoleWithWebIdentityResponse>"#
            ))
        })
        .mount(&sts)
        .await;

    // Fake EKS Pod Identity container credentials server on loopback.
    let container = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/credentials"))
        .respond_with(|req: &wiremock::Request| {
            assert_eq!(
                req.headers["authorization"].to_str().unwrap(),
                "pod-bearer-ludhiana"
            );
            ResponseTemplate::new(200).set_body_json(json!({
                "AccessKeyId": "CONTAINERKEY",
                "SecretAccessKey": "container-secret-value",
                "Token": "container-token-value",
                "Expiration": "2099-01-01T00:00:00Z"
            }))
        })
        .mount(&container)
        .await;

    // Fake IMDSv2 server on loopback.
    let imds = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/latest/api/token"))
        .respond_with(ResponseTemplate::new(200).set_body_string("imds-v2-token"))
        .mount(&imds)
        .await;
    Mock::given(method("GET"))
        .and(path("/latest/meta-data/iam/security-credentials/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ludhiana-node\n"))
        .mount(&imds)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/latest/meta-data/iam/security-credentials/ludhiana-node",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "AccessKeyId": "IMDSKEY",
            "SecretAccessKey": "imds-secret-value",
            "Token": "imds-token-value",
            "Expiration": "2099-01-01T00:00:00Z"
        })))
        .mount(&imds)
        .await;

    let container_uri = format!("{}/v1/credentials", container.uri());
    let mut vars = env_map(&[
        ("AWS_ACCESS_KEY_ID", "ENVKEY"),
        ("AWS_SECRET_ACCESS_KEY", "env-secret-value"),
        ("AWS_SESSION_TOKEN", "env-token-value"),
        (
            "AWS_WEB_IDENTITY_TOKEN_FILE",
            web_token_path.to_str().unwrap(),
        ),
        (
            "AWS_ROLE_ARN",
            "arn:aws:iam::111122223333:role/agra-uploads",
        ),
        ("AWS_ROLE_SESSION_NAME", "jaipur-worker"),
        ("AWS_ENDPOINT_URL_STS", &sts.uri()),
        ("AWS_CONTAINER_CREDENTIALS_FULL_URI", &container_uri),
        (
            "AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE",
            pod_token_path.to_str().unwrap(),
        ),
        ("AWS_METADATA_ENDPOINT", &imds.uri()),
    ]);

    // 1. Disk keys win over everything in the environment, and ignore AWS_SESSION_TOKEN.
    let disk_cfg = S3Config::new("ludhiana-invoices")
        .region("ap-south-1")
        .credentials("DISKKEY", "disk-secret-value");
    let store = S3Store::with_env(&disk_cfg, &vars).unwrap();
    assert_eq!(signed_identity(&store).await, ("DISKKEY".to_owned(), None));

    // 2. Without disk keys, AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY (+ AWS_SESSION_TOKEN) win.
    let bare_cfg = S3Config::new("ludhiana-invoices").region("ap-south-1");
    let store = S3Store::with_env(&bare_cfg, &vars).unwrap();
    assert_eq!(
        signed_identity(&store).await,
        ("ENVKEY".to_owned(), Some("env-token-value".to_owned()))
    );

    // 3. Remove env keys: Web identity (STS AssumeRoleWithWebIdentity) wins,
    //    and re-reads the projected token file when rotated.
    vars.remove("AWS_ACCESS_KEY_ID");
    vars.remove("AWS_SECRET_ACCESS_KEY");
    vars.remove("AWS_SESSION_TOKEN");
    let hands = Hands::at(1_795_000_000);
    let clock = hands.clone();
    let store = S3Store::with_env_and_clock(&bare_cfg, &vars, move || clock.now()).unwrap();
    assert_eq!(
        signed_identity(&store).await,
        (
            "STSKEY_FIRST".to_owned(),
            Some("sts-token-value".to_owned())
        )
    );
    // Rotate the file and step past the expiry so STS is called again.
    std::fs::write(&web_token_path, "eyJhbGciOiJSUzI1NiJ9.amritsar-v2\n").unwrap();
    hands.advance_secs(4_100_000_000);
    assert_eq!(
        signed_identity(&store).await,
        (
            "STSKEY_ROTATED".to_owned(),
            Some("sts-token-value".to_owned())
        )
    );

    // 4. Remove web identity: container credentials win.
    vars.remove("AWS_WEB_IDENTITY_TOKEN_FILE");
    vars.remove("AWS_ROLE_ARN");
    let store = S3Store::with_env(&bare_cfg, &vars).unwrap();
    assert_eq!(
        signed_identity(&store).await,
        (
            "CONTAINERKEY".to_owned(),
            Some("container-token-value".to_owned())
        )
    );

    // Plain HTTP container FULL_URI to an arbitrary host is refused upfront.
    let mut bad_container = vars.clone();
    bad_container.insert(
        "AWS_CONTAINER_CREDENTIALS_FULL_URI".to_owned(),
        "http://10.20.30.40/v1/credentials".to_owned(),
    );
    let err = S3Store::with_env(&bare_cfg, &bad_container).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Refused);

    // 5. Remove container credentials: IMDSv2 wins.
    vars.remove("AWS_CONTAINER_CREDENTIALS_FULL_URI");
    vars.remove("AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE");
    let store = S3Store::with_env(&bare_cfg, &vars).unwrap();
    assert_eq!(
        signed_identity(&store).await,
        ("IMDSKEY".to_owned(), Some("imds-token-value".to_owned()))
    );
}

/// Fake metadata server bound directly on `127.0.0.1:0` that records the
/// exact sequence of `(method, path, imdsv2_token_header)` calls it sees.
struct FakeImds {
    endpoint: String,
    calls: Arc<Mutex<Vec<(String, String, Option<String>)>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for FakeImds {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeImds {
    async fn start(token_status: StatusCode) -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let app = AppRouter::from_handler(move |req: HttpRequest| {
            let recorded = recorded.clone();
            async move {
                let method_str = req.method().as_str().to_owned();
                let path_str = req.uri().path().to_owned();
                let token_hdr = req
                    .headers()
                    .get("x-aws-ec2-metadata-token")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                let ttl_hdr = req
                    .headers()
                    .get("x-aws-ec2-metadata-token-ttl-seconds")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                recorded.lock().unwrap().push((
                    method_str.clone(),
                    path_str.clone(),
                    token_hdr.clone(),
                ));

                match (method_str.as_str(), path_str.as_str()) {
                    ("PUT", "/latest/api/token") => {
                        assert!(ttl_hdr.is_some(), "IMDSv2 token PUT must send a TTL header");
                        if token_status == StatusCode::OK {
                            (StatusCode::OK, "imdsv2-agra-token").into_response()
                        } else {
                            (token_status, "IMDSv2 disabled").into_response()
                        }
                    }
                    ("GET", "/latest/meta-data/iam/security-credentials/") => {
                        if token_status == StatusCode::OK
                            && token_hdr.as_deref() != Some("imdsv2-agra-token")
                        {
                            return (StatusCode::UNAUTHORIZED, "missing token").into_response();
                        }
                        (StatusCode::OK, "amritsar-app-role").into_response()
                    }
                    ("GET", "/latest/meta-data/iam/security-credentials/amritsar-app-role") => {
                        if token_status == StatusCode::OK
                            && token_hdr.as_deref() != Some("imdsv2-agra-token")
                        {
                            return (StatusCode::UNAUTHORIZED, "missing token").into_response();
                        }
                        let body = json!({
                            "Code": "Success",
                            "AccessKeyId": "ASIAIMDSAGRA",
                            "SecretAccessKey": "imds-agra-secret",
                            "Token": "imds-agra-session",
                            "Expiration": "2099-01-01T00:00:00Z"
                        })
                        .to_string();
                        (StatusCode::OK, [("content-type", "application/json")], body)
                            .into_response()
                    }
                    _ => StatusCode::NOT_FOUND.into_response(),
                }
            }
        });
        let task = tokio::spawn(async move {
            navree_http::serve(listener, app).await.unwrap();
        });
        Self {
            endpoint,
            calls,
            task,
        }
    }

    fn log(&self) -> Vec<(String, String, Option<String>)> {
        self.calls.lock().unwrap().clone()
    }
}

#[tokio::test]
async fn imdsv2_token_is_asked_before_credentials_and_v1_only_when_allowed() {
    let cfg = S3Config::new("agra-uploads").region("ap-south-1");

    // 1. IMDSv2 works: PUT /latest/api/token comes first, then both GETs carry the token.
    let v2 = FakeImds::start(StatusCode::OK).await;
    let store = S3Store::with_env(&cfg, &env_map(&[("AWS_METADATA_ENDPOINT", &v2.endpoint)]))
        .unwrap()
        .with_retry_policy(quick_retry());
    assert_eq!(
        signed_identity(&store).await,
        (
            "ASIAIMDSAGRA".to_owned(),
            Some("imds-agra-session".to_owned())
        )
    );
    assert_eq!(
        v2.log(),
        vec![
            ("PUT".into(), "/latest/api/token".into(), None),
            (
                "GET".into(),
                "/latest/meta-data/iam/security-credentials/".into(),
                Some("imdsv2-agra-token".into())
            ),
            (
                "GET".into(),
                "/latest/meta-data/iam/security-credentials/amritsar-app-role".into(),
                Some("imdsv2-agra-token".into())
            ),
        ]
    );

    // 2. IMDSv2 token endpoint refuses (403/404/405) and AWS_IMDSV1_FALLBACK is off:
    //    no unauthenticated IMDSv1 GET is ever sent.
    for refused in [
        StatusCode::FORBIDDEN,
        StatusCode::NOT_FOUND,
        StatusCode::METHOD_NOT_ALLOWED,
    ] {
        let no_v1 = FakeImds::start(refused).await;
        let store = S3Store::with_env(
            &cfg,
            &env_map(&[("AWS_METADATA_ENDPOINT", &no_v1.endpoint)]),
        )
        .unwrap()
        .with_retry_policy(quick_retry());
        let result = store
            .signed_url("a.pdf", Method::GET, Duration::from_secs(60))
            .await;
        assert!(matches!(result, Err(StoreError::Failed(_))), "{refused}");
        assert_eq!(
            no_v1.log(),
            vec![("PUT".into(), "/latest/api/token".into(), None)],
            "must stop after the token refusal on {refused}"
        );
    }

    // 3. With AWS_IMDSV1_FALLBACK=true, IMDSv2 is still tried first and then
    //    falls back to IMDSv1 when the token endpoint answers 403.
    let v1 = FakeImds::start(StatusCode::FORBIDDEN).await;
    let store = S3Store::with_env(
        &cfg,
        &env_map(&[
            ("AWS_METADATA_ENDPOINT", &v1.endpoint),
            ("AWS_IMDSV1_FALLBACK", "true"),
        ]),
    )
    .unwrap()
    .with_retry_policy(quick_retry());
    assert_eq!(
        signed_identity(&store).await,
        (
            "ASIAIMDSAGRA".to_owned(),
            Some("imds-agra-session".to_owned())
        )
    );
    assert_eq!(
        v1.log(),
        vec![
            ("PUT".into(), "/latest/api/token".into(), None),
            (
                "GET".into(),
                "/latest/meta-data/iam/security-credentials/".into(),
                None
            ),
            (
                "GET".into(),
                "/latest/meta-data/iam/security-credentials/amritsar-app-role".into(),
                None
            ),
        ]
    );
}

#[tokio::test]
async fn an_expired_session_is_refreshed_between_parts() {
    let start_unix = 1_795_000_000u64;
    let hands = Hands::at(start_unix);

    // Rotating IMDSv2 server: each fetch issues `session-<n>` good for 1 hour
    // from `hands.now()`, with its own secret key so signatures only verify
    // when the S3 request uses the matching rotated credential.
    let fetches = Arc::new(AtomicUsize::new(0));
    let imds_fetches = fetches.clone();
    let imds_hands = hands.clone();
    let imds_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let imds_endpoint = format!("http://{}", imds_listener.local_addr().unwrap());
    let imds_app = AppRouter::from_handler(move |req: HttpRequest| {
        let imds_fetches = imds_fetches.clone();
        let imds_hands = imds_hands.clone();
        async move {
            match (req.method().as_str(), req.uri().path()) {
                ("PUT", "/latest/api/token") => (StatusCode::OK, "rot-token").into_response(),
                ("GET", "/latest/meta-data/iam/security-credentials/") => {
                    (StatusCode::OK, "jaipur-uploader").into_response()
                }
                ("GET", "/latest/meta-data/iam/security-credentials/jaipur-uploader") => {
                    let n = imds_fetches.fetch_add(1, Ordering::SeqCst) + 1;
                    let exp: DateTime<Utc> = (imds_hands.now() + Duration::from_secs(3600)).into();
                    let body = json!({
                        "AccessKeyId": format!("ASIAJAIPUR{n}"),
                        "SecretAccessKey": format!("jaipur-secret-{n}"),
                        "Token": format!("jaipur-session-{n}"),
                        "Expiration": exp.to_rfc3339(),
                    })
                    .to_string();
                    (StatusCode::OK, body).into_response()
                }
                _ => StatusCode::NOT_FOUND.into_response(),
            }
        }
    });
    let imds_task = tokio::spawn(async move {
        navree_http::serve(imds_listener, imds_app).await.unwrap();
    });

    // S3 server that verifies every SigV4 signature (including x-amz-security-token
    // and x-amz-content-sha256) against the session's own secret, injects one
    // 200-OK `<Error><Code>InternalError</Code></Error>` on CompleteMultipartUpload
    // to prove busy-body retry, and records `(step, session_token)`.
    let seen_tokens: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let uploaded_parts: Arc<Mutex<BTreeMap<u32, Bytes>>> = Arc::new(Mutex::new(BTreeMap::new()));
    let finished_body: Arc<Mutex<Option<Bytes>>> = Arc::new(Mutex::new(None));
    let complete_attempts = Arc::new(AtomicUsize::new(0));

    let s3_tokens = seen_tokens.clone();
    let s3_parts = uploaded_parts.clone();
    let s3_finished = finished_body.clone();
    let s3_complete_attempts = complete_attempts.clone();
    let s3_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let s3_addr = s3_listener.local_addr().unwrap();
    let s3_endpoint = format!("http://{s3_addr}");
    let s3_app = AppRouter::from_handler(move |req: HttpRequest| {
        let s3_tokens = s3_tokens.clone();
        let s3_parts = s3_parts.clone();
        let s3_finished = s3_finished.clone();
        let s3_complete_attempts = s3_complete_attempts.clone();
        async move {
            let (parts, body) = req.into_parts();
            let raw_path = parts.uri.path().to_owned();
            let raw_query = parts.uri.query().unwrap_or("").to_owned();
            let body = navree_http::body::to_bytes(body, 1024 * 1024)
                .await
                .unwrap();

            // Verify SigV4 header signature for whichever session signed it.
            let auth = parts.headers["authorization"].to_str().unwrap();
            let session_tok = parts.headers["x-amz-security-token"]
                .to_str()
                .unwrap()
                .to_owned();
            let session_num: usize = session_tok
                .strip_prefix("jaipur-session-")
                .unwrap()
                .parse()
                .unwrap();
            let secret = format!("jaipur-secret-{session_num}");
            let amz_date = parts.headers["x-amz-date"].to_str().unwrap();
            let content_sha = parts.headers["x-amz-content-sha256"].to_str().unwrap();
            assert_eq!(content_sha, sha256_hex(&body));

            let signed_list = auth
                .split("SignedHeaders=")
                .nth(1)
                .unwrap()
                .split(',')
                .next()
                .unwrap();
            let provided_sig = auth.rsplit_once("Signature=").unwrap().1;
            let host_str = format!("{s3_addr}");
            let query_pairs: Vec<(String, String)> =
                url::form_urlencoded::parse(raw_query.as_bytes())
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect();
            let header_pairs: Vec<(&str, &str)> = signed_list
                .split(';')
                .map(|name| {
                    let val = if name == "host" {
                        host_str.as_str()
                    } else {
                        parts.headers[name].to_str().unwrap()
                    };
                    (name, val)
                })
                .collect();
            let canonical = sigv4::canonical_request(
                parts.method.as_str(),
                &raw_path,
                &sigv4::canonical_query(query_pairs.iter().map(|(k, v)| (k.as_str(), v.as_str()))),
                &header_pairs,
                content_sha,
            );
            let expected_sig =
                sigv4::signature(&secret, amz_date, "ap-south-1", "s3", &canonical.text);
            assert_eq!(provided_sig, expected_sig, "SigV4 mismatch on {raw_query}");

            let xml_resp = |xml_body: &str| -> HttpResponse {
                (
                    StatusCode::OK,
                    [("content-type", "application/xml")],
                    xml_body.to_owned(),
                )
                    .into_response()
            };

            match (parts.method.as_str(), raw_query.as_str()) {
                ("POST", "uploads") => {
                    s3_tokens
                        .lock()
                        .unwrap()
                        .push(("create".into(), session_tok));
                    xml_resp(
                        "<InitiateMultipartUploadResult><UploadId>up-jaipur-1</UploadId></InitiateMultipartUploadResult>",
                    )
                }
                ("PUT", q) if q.contains("partNumber=") => {
                    let num: u32 = query_pairs
                        .iter()
                        .find(|(k, _)| k == "partNumber")
                        .unwrap()
                        .1
                        .parse()
                        .unwrap();
                    s3_tokens
                        .lock()
                        .unwrap()
                        .push((format!("part-{num}"), session_tok));
                    let tag = format!("\"etag-part-{num}\"");
                    s3_parts.lock().unwrap().insert(num, body);
                    (StatusCode::OK, [("etag", tag.as_str())], "").into_response()
                }
                ("POST", "uploadId=up-jaipur-1") => {
                    let attempt = s3_complete_attempts.fetch_add(1, Ordering::SeqCst);
                    s3_tokens
                        .lock()
                        .unwrap()
                        .push((format!("complete-{attempt}"), session_tok));
                    if attempt == 0 {
                        // S3 200-OK streaming failure: driver must retry CompleteMultipartUpload.
                        return xml_resp(
                            "<Error><Code>InternalError</Code><Message>Please try again</Message></Error>",
                        );
                    }
                    let map = s3_parts.lock().unwrap();
                    let mut combined = Vec::new();
                    for chunk in map.values() {
                        combined.extend_from_slice(chunk);
                    }
                    *s3_finished.lock().unwrap() = Some(Bytes::from(combined));
                    xml_resp(
                        "<CompleteMultipartUploadResult><ETag>\"done\"</ETag></CompleteMultipartUploadResult>",
                    )
                }
                _ => StatusCode::NOT_IMPLEMENTED.into_response(),
            }
        }
    });
    let s3_task = tokio::spawn(async move {
        navree_http::serve(s3_listener, s3_app).await.unwrap();
    });

    let cfg = S3Config::new("jaipur-ledger")
        .region("ap-south-1")
        .endpoint(&s3_endpoint);
    let clock = hands.clone();
    let store = S3Store::with_env_and_clock(
        &cfg,
        &env_map(&[("AWS_METADATA_ENDPOINT", &imds_endpoint)]),
        move || clock.now(),
    )
    .unwrap()
    .with_retry_policy(quick_retry())
    .with_part_size(16);

    let mut upload = store.begin_upload("fy26/q3-gst.csv").await.unwrap();
    // Part 1 (16 bytes): signed with session-1.
    upload
        .write(Bytes::from_static(b"city,amount_inr\n"))
        .await
        .unwrap();
    assert_eq!(fetches.load(Ordering::SeqCst), 1);

    // Advance 56 minutes: session-1 is inside the 5-minute refresh window.
    hands.advance_secs(56 * 60);

    // Part 2 (16 bytes) + tail + CompleteMultipartUpload: refreshes to session-2!
    upload
        .write(Bytes::from_static(b"Jaipur,125000\nLu"))
        .await
        .unwrap();
    upload
        .write(Bytes::from_static(b"dhiana,98000\n"))
        .await
        .unwrap();
    upload.finish().await.unwrap();

    assert_eq!(fetches.load(Ordering::SeqCst), 2);
    assert_eq!(
        finished_body.lock().unwrap().as_deref(),
        Some(b"city,amount_inr\nJaipur,125000\nLudhiana,98000\n".as_slice())
    );
    assert_eq!(
        *seen_tokens.lock().unwrap(),
        vec![
            ("create".into(), "jaipur-session-1".into()),
            ("part-1".into(), "jaipur-session-1".into()),
            ("part-2".into(), "jaipur-session-2".into()),
            ("part-3".into(), "jaipur-session-2".into()),
            ("complete-0".into(), "jaipur-session-2".into()),
            ("complete-1".into(), "jaipur-session-2".into()),
        ]
    );

    imds_task.abort();
    s3_task.abort();
}

#[tokio::test]
async fn credentials_never_appear_in_an_error() {
    let secrets = [
        "AKIAAMRITSARSECRETID",
        "super-secret-s3-signing-key-98765",
        "session-token-harpreet-445566",
        "web-jwt-gurpreet-token-778899",
        "imds-token-kiran-112233",
    ];
    let assert_clean = |text: &str| {
        for secret in secrets {
            assert!(!text.contains(secret), "leaked `{secret}` in: {text}");
        }
    };

    // 1. S3 XML error body echoing canonical request and session token.
    let s3_server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(403).set_body_string(format!(
            "<Error><Code>SignatureDoesNotMatch</Code>\
             <Message>Key {} with secret {} failed</Message>\
             <CanonicalRequest>x-amz-security-token:{}</CanonicalRequest></Error>",
            secrets[0], secrets[1], secrets[2]
        )))
        .mount(&s3_server)
        .await;

    let mut cfg = S3Config::new("amritsar-vault")
        .region("ap-south-1")
        .endpoint(s3_server.uri())
        .credentials(secrets[0], secrets[1]);
    cfg.session_token = Some(Secret::new(secrets[2].to_owned()));

    let store = S3Store::with_env(&cfg, &BTreeMap::new())
        .unwrap()
        .with_retry_policy(quick_retry());
    assert_clean(&format!("{store} {store:?}"));

    let err = store
        .put("docs/gst.pdf", Bytes::from_static(b"pdf"))
        .await
        .unwrap_err();
    let text = format!("{err} {err:?}");
    assert_clean(&text);
    assert!(
        text.contains("403 Forbidden (SignatureDoesNotMatch)"),
        "{text}"
    );

    // 2. STS error echoing web identity JWT and secret in XML.
    let temp = tempfile::tempdir().unwrap();
    let jwt_file = temp.path().join("jwt");
    std::fs::write(&jwt_file, secrets[3]).unwrap();
    let sts_server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(403).set_body_string(format!(
            "<ErrorResponse><Error><Code>InvalidIdentityToken</Code>\
             <Message>Rejected token {} and secret {}</Message></Error></ErrorResponse>",
            secrets[3], secrets[1]
        )))
        .mount(&sts_server)
        .await;

    let sts_store = S3Store::with_env(
        &S3Config::new("amritsar-vault")
            .region("ap-south-1")
            .endpoint(s3_server.uri()),
        &env_map(&[
            ("AWS_WEB_IDENTITY_TOKEN_FILE", jwt_file.to_str().unwrap()),
            ("AWS_ROLE_ARN", "arn:aws:iam::111122223333:role/agra"),
            ("AWS_ENDPOINT_URL_STS", &sts_server.uri()),
        ]),
    )
    .unwrap()
    .with_retry_policy(quick_retry());
    let sts_err = sts_store
        .signed_url("docs/gst.pdf", Method::GET, Duration::from_secs(60))
        .await
        .unwrap_err();
    let sts_text = format!("{sts_err} {sts_err:?}");
    assert_clean(&sts_text);
    assert!(sts_text.contains("InvalidIdentityToken"), "{sts_text}");

    // 3. Malformed IMDS answer containing tokens/secrets.
    let imds_server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/latest/api/token"))
        .respond_with(ResponseTemplate::new(200).set_body_string(secrets[4]))
        .mount(&imds_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/latest/meta-data/iam/security-credentials/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("agra-role"))
        .mount(&imds_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/latest/meta-data/iam/security-credentials/agra-role"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(format!("not-json: {} {}", secrets[1], secrets[4])),
        )
        .mount(&imds_server)
        .await;

    let imds_store = S3Store::with_env(
        &S3Config::new("amritsar-vault")
            .region("ap-south-1")
            .endpoint(s3_server.uri()),
        &env_map(&[("AWS_METADATA_ENDPOINT", &imds_server.uri())]),
    )
    .unwrap()
    .with_retry_policy(quick_retry());
    let imds_err = imds_store.head("docs/gst.pdf").await.unwrap_err();
    assert_clean(&format!("{imds_err} {imds_err:?}"));
}

#[tokio::test]
async fn s3_store_lists_pages_reads_ranges_and_deletes_missing_as_not_found() {
    let server = MockServer::start().await;

    // Two-page ListObjectsV2 response.
    let page_calls = Arc::new(AtomicUsize::new(0));
    let counted = page_calls.clone();
    Mock::given(method("GET"))
        .and(path("/ludhiana-files"))
        .respond_with(move |req: &wiremock::Request| {
            let q = req.url.query().unwrap_or("");
            assert!(q.contains("list-type=2"), "{q}");
            assert!(q.contains("prefix=invoices%2F"), "{q}");
            if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(200).set_body_string(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
                    <ListBucketResult>
                      <IsTruncated>true</IsTruncated>
                      <NextContinuationToken>page-2-tok</NextContinuationToken>
                      <Contents>
                        <Key>invoices/INV 1101 + 18% ₹.pdf</Key>
                        <LastModified>2026-11-25T10:00:00Z</LastModified>
                        <Size>12</Size>
                      </Contents>
                    </ListBucketResult>"#,
                )
            } else {
                assert!(q.contains("continuation-token=page-2-tok"), "{q}");
                ResponseTemplate::new(200).set_body_string(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
                    <ListBucketResult>
                      <IsTruncated>false</IsTruncated>
                      <Contents>
                        <Key>invoices/INV-1102.pdf</Key>
                        <LastModified>2026-11-25T11:00:00Z</LastModified>
                        <Size>5</Size>
                      </Contents>
                    </ListBucketResult>"#,
                )
            }
        })
        .mount(&server)
        .await;

    // Missing file answers 404 on HEAD -> Store::delete returns StoreError::NotFound.
    Mock::given(method("HEAD"))
        .and(path("/ludhiana-files/missing.pdf"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    // Out-of-bounds range answers 416 on GET and 200 on HEAD -> cut short to empty Bytes.
    Mock::given(method("GET"))
        .and(path("/ludhiana-files/short.txt"))
        .respond_with(ResponseTemplate::new(416))
        .mount(&server)
        .await;
    Mock::given(method("HEAD"))
        .and(path("/ludhiana-files/short.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "4")
                .insert_header("last-modified", "Wed, 25 Nov 2026 10:00:00 GMT"),
        )
        .mount(&server)
        .await;

    // Streaming open returns full size and body chunks.
    Mock::given(method("GET"))
        .and(path("/ludhiana-files/stream.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "8")
                .insert_header("etag", "\"v1\"")
                .set_body_string("Amritsar"),
        )
        .mount(&server)
        .await;

    let store = S3Store::new(
        S3Config::new("ludhiana-files")
            .region("ap-south-1")
            .endpoint(server.uri())
            .credentials("AKIALUDHIANA", "ludhiana-secret"),
    )
    .unwrap()
    .with_retry_policy(quick_retry());

    let listed = store.list("invoices").await.unwrap();
    let paths: Vec<&str> = listed.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(
        paths,
        ["invoices/INV 1101 + 18% ₹.pdf", "invoices/INV-1102.pdf"]
    );
    assert_eq!((listed[0].size, listed[1].size), (12, 5));

    assert!(matches!(
        store.delete("missing.pdf").await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(store.read("short.txt", Some(50..100)).await.unwrap(), "");
    assert_eq!(store.read("short.txt", Some(3..3)).await.unwrap(), "");

    let opened = store.open("stream.txt").await.unwrap();
    assert_eq!(opened.size, 8);
    let chunks: Vec<Bytes> = opened.stream.try_collect().await.unwrap();
    assert_eq!(chunks.concat(), b"Amritsar");
}

#[test]
fn presign_and_header_signing_handle_multibyte_query_keys_and_literal_plus() {
    let now = DateTime::parse_from_rfc3339("2026-11-26T06:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let creds = Credentials::new("AKIALUDHIANA", "ludhiana-secret-key");

    // Multi-byte UTF-8 query keys where byte index 6 splits a multi-byte scalar
    // must never panic when checking for reserved `X-Amz-*` prefixes.
    for raw in [
        "https://agra-uploads.s3.ap-south-1.amazonaws.com/a.pdf?abcdeé=1",
        "https://agra-uploads.s3.ap-south-1.amazonaws.com/a.pdf?aचालान=1",
        "https://agra-uploads.s3.ap-south-1.amazonaws.com/a.pdf?ਲੁਧਿਆਣਾ=1&tag=a+b",
    ] {
        let url = Url::parse(raw).unwrap();
        let signed = presign_url(
            &Method::GET,
            &url,
            &[],
            Duration::from_secs(300),
            &creds,
            "ap-south-1",
            now,
        )
        .unwrap();
        assert!(signed.as_str().contains("X-Amz-Signature="), "{signed}");
        if raw.contains("tag=a+b") {
            assert!(signed.as_str().contains("tag=a%2Bb"), "{signed}");
        }
    }

    // Literal `+` in a raw query string is signed per RFC 3986 (`%2B`), not
    // form-decoded to `%20`, and any pre-existing `host` header is stripped.
    let url = Url::parse("https://agra-uploads.s3.ap-south-1.amazonaws.com/a.pdf?tag=agra+jaipur")
        .unwrap();
    let mut req = Request::new(Method::GET, url)
        .header("host", "spoofed.amritsar.example")
        .unwrap();
    sign_request(&mut req, &creds, "ap-south-1", now).unwrap();
    assert!(!req.headers.contains_key("host"));

    let expected_canonical = sigv4::canonical_request(
        "GET",
        "/a.pdf",
        "tag=agra%2Bjaipur",
        &[
            ("host", "agra-uploads.s3.ap-south-1.amazonaws.com"),
            ("x-amz-content-sha256", &sha256_hex(b"")),
            ("x-amz-date", "20261126T060000Z"),
        ],
        &sha256_hex(b""),
    );
    let expected_sig = sigv4::signature(
        "ludhiana-secret-key",
        "20261126T060000Z",
        "ap-south-1",
        "s3",
        &expected_canonical.text,
    );
    assert!(
        req.headers["authorization"]
            .to_str()
            .unwrap()
            .ends_with(&format!("Signature={expected_sig}"))
    );

    // Invalid access_key_id or region delimiters are refused upfront.
    let bad_creds = Credentials::new("AKIA/injected", "secret");
    let clean_url = Url::parse("https://agra-uploads.s3.ap-south-1.amazonaws.com/a.pdf").unwrap();
    assert!(
        presign_url(
            &Method::GET,
            &clean_url,
            &[],
            Duration::from_secs(60),
            &bad_creds,
            "ap-south-1",
            now,
        )
        .is_err()
    );
    assert!(
        presign_url(
            &Method::GET,
            &clean_url,
            &[],
            Duration::from_secs(60),
            &creds,
            "ap-south-1/extra",
            now,
        )
        .is_err()
    );
}

#[tokio::test]
async fn partial_s3_credentials_bucket_dots_and_malformed_vhost_endpoints_are_refused() {
    let imds = FakeImds::start(StatusCode::OK).await;
    let base_cfg = S3Config::new("agra-uploads").region("ap-south-1");

    // Partial Web Identity and partial AWS_SESSION_TOKEN fail closed before
    // ever reaching IMDS.
    for partial in [
        vec![
            ("AWS_WEB_IDENTITY_TOKEN_FILE", "/tmp/projected-token"),
            ("AWS_METADATA_ENDPOINT", imds.endpoint.as_str()),
        ],
        vec![
            ("AWS_ROLE_ARN", "arn:aws:iam::111122223333:role/agra"),
            ("AWS_METADATA_ENDPOINT", imds.endpoint.as_str()),
        ],
        vec![
            ("AWS_SESSION_TOKEN", "orphan-session"),
            ("AWS_METADATA_ENDPOINT", imds.endpoint.as_str()),
        ],
    ] {
        let err = match S3Store::with_env(&base_cfg, &env_map(&partial)) {
            Err(err) => err,
            Ok(_) => panic!("expected partial S3 env {partial:?} to fail closed"),
        };
        assert_eq!(err.kind(), ErrorKind::Credential, "{partial:?}");
    }
    assert!(imds.log().is_empty(), "IMDS must never be contacted");

    // `session_token` alone on `S3Config` fails closed too.
    let mut orphan_session_cfg = S3Config::new("agra-uploads").region("ap-south-1");
    orphan_session_cfg.session_token = Some(Secret::new("tok".to_owned()));
    let orphan_err = match S3Store::with_env(&orphan_session_cfg, &BTreeMap::new()) {
        Err(err) => err,
        Ok(_) => panic!("expected orphan session_token to fail closed"),
    };
    assert_eq!(orphan_err.kind(), ErrorKind::Credential);

    // Invalid bucket dot/dash labels are refused.
    for bad_bucket in [".agra", "agra.", "-agra", "agra-", "agra..media"] {
        let cfg = S3Config::new(bad_bucket).credentials("AKIAAGRA", "secret");
        let err = match S3Store::with_env(&cfg, &BTreeMap::new()) {
            Err(err) => err,
            Ok(_) => panic!("expected bucket `{bad_bucket}` to be refused"),
        };
        assert_eq!(err.kind(), ErrorKind::Refused, "{bad_bucket}");
    }

    // Dotted AWS bucket defaults to path-style (`https://s3.<region>.amazonaws.com/<bucket>/...`)
    // so wildcard TLS verification succeeds.
    let dotted = S3Store::with_env(
        &S3Config::new("media.ludhiana.in")
            .region("ap-south-1")
            .credentials("AKIALUDHIANA", "secret"),
        &BTreeMap::new(),
    )
    .unwrap();
    let link = dotted
        .signed_url("inv.pdf", Method::GET, Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(link.host_str(), Some("s3.ap-south-1.amazonaws.com"));
    assert_eq!(link.path(), "/media.ludhiana.in/inv.pdf");

    // Malformed authority `http:///...` and IP-literal virtual-hosted endpoints
    // are refused.
    for bad_ep in [
        "http:///internal.ludhiana.example:9000",
        "http://127.0.0.1:9000",
        "http://10.0.1:9000",
    ] {
        let cfg = S3Config::new("127")
            .endpoint(bad_ep)
            .path_style(false)
            .credentials("AKIALUDHIANA", "secret");
        let err = match S3Store::with_env(&cfg, &BTreeMap::new()) {
            Err(err) => err,
            Ok(_) => panic!("expected endpoint `{bad_ep}` with path_style=false to fail"),
        };
        assert_eq!(err.kind(), ErrorKind::Refused, "{bad_ep}");
    }
}

#[tokio::test]
async fn s3_listing_rejects_cycles_and_traversal_keys_and_presign_refreshes_expiring_sessions() {
    // 1. 2-step continuation cycle (`tok-a` -> `tok-b` -> `tok-a`) fails cleanly.
    let cycle_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/agra-cycle"))
        .respond_with(|req: &wiremock::Request| {
            let q = req.url.query().unwrap_or_default();
            let next = if q.contains("continuation-token=tok-a") {
                "tok-b"
            } else {
                "tok-a"
            };
            ResponseTemplate::new(200).set_body_string(format!(
                "<ListBucketResult>\
                   <IsTruncated>true</IsTruncated>\
                   <NextContinuationToken>{next}</NextContinuationToken>\
                 </ListBucketResult>"
            ))
        })
        .mount(&cycle_server)
        .await;
    let cycle_store = S3Store::new(
        S3Config::new("agra-cycle")
            .endpoint(cycle_server.uri())
            .credentials("AKIAAGRA", "secret"),
    )
    .unwrap();
    assert!(cycle_store.list("docs").await.is_err());

    // 2. Traversal `<Key>` in listing fails with ErrorKind::Key; exact folder
    //    marker `docs/` is skipped cleanly.
    let marker_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/agra-markers"))
        .respond_with(|req: &wiremock::Request| {
            let q = req.url.query().unwrap_or_default();
            if q.contains("prefix=bad%2F") {
                ResponseTemplate::new(200).set_body_string(
                    "<ListBucketResult>\
                       <IsTruncated>false</IsTruncated>\
                       <Contents><Key>bad/../secret.pdf</Key><Size>5</Size></Contents>\
                     </ListBucketResult>",
                )
            } else {
                ResponseTemplate::new(200).set_body_string(
                    "<ListBucketResult>\
                       <IsTruncated>false</IsTruncated>\
                       <Contents><Key>docs/</Key><Size>0</Size></Contents>\
                       <Contents><Key>docs/ok.pdf</Key><Size>9</Size></Contents>\
                     </ListBucketResult>",
                )
            }
        })
        .mount(&marker_server)
        .await;
    let marker_store = S3Store::new(
        S3Config::new("agra-markers")
            .endpoint(marker_server.uri())
            .credentials("AKIAAGRA", "secret"),
    )
    .unwrap();
    let listed = marker_store.list("docs").await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].path, "docs/ok.pdf");
    assert!(marker_store.list("bad").await.is_err());

    // 3. Presigned link whose `expires_in` exceeds the remaining lifetime of
    //    a cached temporary credential triggers an early refresh.
    let hands = Hands::at(1_795_000_000);
    let fetches = Arc::new(AtomicUsize::new(0));
    let counted = fetches.clone();
    let source_hands = hands.clone();
    let clock_hands = hands.clone();
    let provider = navree_storage::cloud::credentials::Provider::with_clock(
        move || {
            let n = counted.fetch_add(1, Ordering::SeqCst) + 1;
            let exp = source_hands.now() + Duration::from_secs(1800);
            async move {
                Ok(navree_storage::cloud::credentials::Credential::until(
                    Credentials::new(format!("ASIAAGRA{n}"), "secret")
                        .with_session_token(Some(format!("tok-{n}"))),
                    exp,
                ))
            }
        },
        move || clock_hands.now(),
    );
    let clock_store = hands.clone();
    let store = S3Store::with_env_and_clock(
        &S3Config::new("agra-uploads").region("ap-south-1"),
        &env_map(&[
            ("AWS_ACCESS_KEY_ID", "UNUSED"),
            ("AWS_SECRET_ACCESS_KEY", "UNUSED"),
        ]),
        move || clock_store.now(),
    )
    .unwrap()
    .with_credentials(provider);

    // First 5-minute link uses `ASIAAGRA1` (30 minutes left).
    assert_eq!(signed_identity(&store).await.0, "ASIAAGRA1");
    // Advance 10 minutes (20 minutes left) and sign a 1-hour link: refreshes to `ASIAAGRA2`.
    hands.advance_secs(600);
    let long_link = store
        .signed_url("a.pdf", Method::GET, Duration::from_secs(3600))
        .await
        .unwrap();
    assert!(long_link.as_str().contains("ASIAAGRA2"), "{long_link}");
}
