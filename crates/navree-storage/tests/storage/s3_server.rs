//! An S3 server inside the test process: objects with ranges, listing,
//! multipart uploads, copies and bulk deletes, path-style, with every
//! request's SigV4 signature checked (the `Authorization` header, or the
//! query of a presigned URL, including its signed headers and expiry) by
//! `navree_crypto::sigv4`, the signer of Navree's own AWS requests. It
//! lets the s3 driver run the conformance suite without Docker; MinIO runs
//! the same suite when `NAVREE_TEST_S3_URL` is set.
//!
//! [`S3Server::gcs`] speaks Google Cloud Storage's XML API instead, the one
//! the gcs driver writes, reads and lists with (it follows S3's): requests
//! carry the service account's self-signed token (`Authorization: Bearer`,
//! RS256), signed links are `GOOG4-RSA-SHA256`, both checked against the
//! test key, and uploads honour `x-goog-content-length-range` and
//! `x-goog-if-generation-match`. fake-gcs-server reads like GCS but takes no
//! write the driver makes, so the gcs driver's writes run here.

#![cfg_attr(not(all(feature = "s3", feature = "gcs")), allow(dead_code))]

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, Utc};
use http::{HeaderMap, Method, StatusCode};
use navree_crypto::sha256_hex;
use navree_crypto::sigv4;
use navree_http::AppRouter;
use navree_http::body::{Body, Bytes};
use navree_http::extract::Request;
use navree_http::response::{IntoResponse, Response};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

/// The access key the server accepts.
pub const ACCESS_KEY: &str = "navree-test";
/// Its secret.
pub const SECRET_KEY: &str = "navree-test-secret";
/// The region requests must be signed for.
pub const REGION: &str = "us-east-1";
/// The bucket the server starts with.
pub const BUCKET: &str = "navree-test";
/// The service account a [`S3Server::gcs`] server accepts.
pub const GCS_EMAIL: &str = "navree-test@navree-test.iam.gserviceaccount.com";
/// Its private key (the server checks signatures with its public half).
pub const GCS_PRIVATE_KEY: &str = include_str!("../fixtures/rsa-2048.pem");
/// The host the gcs driver signs links for, whatever its endpoint (the
/// server checks them as Google Cloud Storage does).
const GCS_HOST: &str = "storage.googleapis.com";

/// A running server; stops when dropped.
pub struct S3Server {
    /// `http://127.0.0.1:<port>`.
    pub url: String,
    state: Arc<ServerState>,
    task: tokio::task::JoinHandle<()>,
}

/// The API the server speaks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Flavor {
    #[default]
    S3,
    Gcs,
}

#[derive(Default)]
struct ServerState {
    flavor: Flavor,
    buckets: Mutex<BTreeMap<String, BTreeMap<String, Object>>>,
    uploads: Mutex<HashMap<String, Upload>>,
    next_id: AtomicU64,
    /// Every request, as `METHOD /path?query`.
    log: Mutex<Vec<String>>,
    /// Delay before answering, to simulate a slow store.
    delay: Mutex<Option<Duration>>,
}

#[derive(Clone)]
struct Object {
    body: Bytes,
    content_type: Option<String>,
    last_modified: DateTime<Utc>,
    etag: String,
}

struct Upload {
    bucket: String,
    key: String,
    content_type: Option<String>,
    parts: BTreeMap<u32, Bytes>,
}

impl S3Server {
    /// Starts a server with the bucket [`BUCKET`].
    pub async fn start() -> Self {
        Self::start_as(Flavor::S3).await
    }

    /// Starts a server with the bucket [`BUCKET`] that speaks Google Cloud
    /// Storage's XML API, for the service account [`GCS_EMAIL`].
    pub async fn gcs() -> Self {
        Self::start_as(Flavor::Gcs).await
    }

    async fn start_as(flavor: Flavor) -> Self {
        let state = Arc::new(ServerState {
            flavor,
            ..ServerState::default()
        });
        state
            .buckets
            .lock()
            .unwrap()
            .insert(BUCKET.to_owned(), BTreeMap::new());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let served = state.clone();
        let app = AppRouter::from_handler(move |request: Request| handle(served.clone(), request));
        let task = tokio::spawn(async move {
            navree_http::serve(listener, app).await.unwrap();
        });
        Self { url, state, task }
    }

    /// The keys of [`BUCKET`].
    pub fn keys(&self) -> Vec<String> {
        self.state.buckets.lock().unwrap()[BUCKET]
            .keys()
            .cloned()
            .collect()
    }

    /// The stored content type of `key`.
    pub fn content_type(&self, key: &str) -> Option<String> {
        self.state.buckets.lock().unwrap()[BUCKET]
            .get(key)
            .and_then(|object| object.content_type.clone())
    }

    /// Requests received so far, as `METHOD /path?query`.
    pub fn log(&self) -> Vec<String> {
        self.state.log.lock().unwrap().clone()
    }

    /// Answers every request after `delay`.
    pub fn slow_down(&self, delay: Duration) {
        *self.state.delay.lock().unwrap() = Some(delay);
    }
}

impl Drop for S3Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Every request.
async fn handle(state: Arc<ServerState>, request: Request) -> Response {
    let delay = *state.delay.lock().unwrap();
    if let Some(delay) = delay {
        tokio::time::sleep(delay).await;
    }
    let (parts, body) = request.into_parts();
    let raw_path = parts.uri.path().to_owned();
    let raw_query = parts.uri.query().unwrap_or("").to_owned();
    state
        .log
        .lock()
        .unwrap()
        .push(format!("{} {raw_path}?{raw_query}", parts.method));
    let query = parse_query(&raw_query);
    let body = navree_http::body::to_bytes(body, usize::MAX).await.unwrap();

    let authorized = match state.flavor {
        Flavor::S3 => authorize(&parts.method, &raw_path, &query, &parts.headers, &body),
        Flavor::Gcs => authorize_gcs(&parts.method, &raw_path, &query, &parts.headers),
    };
    if let Err(message) = authorized {
        return error(StatusCode::FORBIDDEN, "SignatureDoesNotMatch", &message);
    }
    let gcs = state.flavor == Flavor::Gcs;
    let copy_source = if gcs {
        "x-goog-copy-source"
    } else {
        "x-amz-copy-source"
    };

    let decoded = percent_decode_str(raw_path.trim_start_matches('/'))
        .decode_utf8()
        .unwrap()
        .into_owned();
    let (bucket, key) = match decoded.split_once('/') {
        Some((bucket, key)) => (bucket.to_owned(), key.to_owned()),
        None => (decoded, String::new()),
    };
    if !state.buckets.lock().unwrap().contains_key(&bucket) {
        return error(
            StatusCode::NOT_FOUND,
            "NoSuchBucket",
            "The bucket does not exist",
        );
    }
    let has = |name: &str| query.iter().any(|(key, _)| key == name);
    let param = |name: &str| {
        query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    let content_type = parts
        .headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    match (&parts.method, key.is_empty()) {
        (&Method::GET, true) => list(&state, &bucket, &param("prefix").unwrap_or_default()),
        (&Method::POST, true) if has("delete") => bulk_delete(&state, &bucket, &body),
        (&Method::POST, false) if has("uploads") => {
            let id = format!("upload-{}", state.next_id.fetch_add(1, Ordering::SeqCst));
            state.uploads.lock().unwrap().insert(
                id.clone(),
                Upload {
                    bucket: bucket.clone(),
                    key: key.clone(),
                    content_type,
                    parts: BTreeMap::new(),
                },
            );
            xml(format!(
                "<InitiateMultipartUploadResult><Bucket>{}</Bucket><Key>{}</Key><UploadId>{id}</UploadId></InitiateMultipartUploadResult>",
                escape(&bucket),
                escape(&key)
            ))
        }
        (&Method::PUT, false) if has("uploadId") => {
            let id = param("uploadId").unwrap();
            let number: u32 = param("partNumber").unwrap().parse().unwrap();
            let mut uploads = state.uploads.lock().unwrap();
            let Some(upload) = uploads.get_mut(&id) else {
                return error(StatusCode::NOT_FOUND, "NoSuchUpload", "No such upload");
            };
            upload.parts.insert(number, body.clone());
            let mut response = StatusCode::OK.into_response();
            response
                .headers_mut()
                .insert("etag", etag(&body).parse().unwrap());
            response
        }
        (&Method::POST, false) if has("uploadId") => {
            let id = param("uploadId").unwrap();
            let Some(upload) = state.uploads.lock().unwrap().remove(&id) else {
                return error(StatusCode::NOT_FOUND, "NoSuchUpload", "No such upload");
            };
            let text = String::from_utf8_lossy(&body);
            let mut contents = Vec::new();
            for number in elements(&text, "PartNumber") {
                let number: u32 = number.parse().unwrap();
                match upload.parts.get(&number) {
                    Some(part) => contents.extend_from_slice(part),
                    None => return error(StatusCode::BAD_REQUEST, "InvalidPart", "Unknown part"),
                }
            }
            let contents = Bytes::from(contents);
            let tag = etag(&contents);
            store(
                &state,
                &upload.bucket,
                &upload.key,
                contents,
                upload.content_type,
            );
            xml(format!(
                "<CompleteMultipartUploadResult><Location>/{0}/{1}</Location><Bucket>{0}</Bucket><Key>{1}</Key><ETag>{tag}</ETag></CompleteMultipartUploadResult>",
                escape(&upload.bucket),
                escape(&upload.key),
            ))
        }
        (&Method::DELETE, false) if has("uploadId") => {
            state
                .uploads
                .lock()
                .unwrap()
                .remove(&param("uploadId").unwrap());
            StatusCode::NO_CONTENT.into_response()
        }
        (&Method::PUT, false) => {
            if gcs
                && let Err(refused) =
                    gcs_preconditions(&state, &bucket, &key, &parts.headers, &body)
            {
                return refused;
            }
            if let Some(source) = parts.headers.get(copy_source) {
                let source = percent_decode_str(source.to_str().unwrap())
                    .decode_utf8()
                    .unwrap()
                    .into_owned();
                let source = source.trim_start_matches('/');
                let (from_bucket, from_key) = source.split_once('/').unwrap();
                let object = state
                    .buckets
                    .lock()
                    .unwrap()
                    .get(from_bucket)
                    .and_then(|objects| objects.get(from_key).cloned());
                let Some(object) = object else {
                    return error(StatusCode::NOT_FOUND, "NoSuchKey", "The key does not exist");
                };
                let tag = store(&state, &bucket, &key, object.body, object.content_type);
                return xml(format!(
                    "<CopyObjectResult><ETag>{tag}</ETag></CopyObjectResult>"
                ));
            }
            let tag = store(&state, &bucket, &key, body, content_type);
            let mut response = StatusCode::OK.into_response();
            response.headers_mut().insert("etag", tag.parse().unwrap());
            response
        }
        (&Method::DELETE, false) => {
            let removed = state
                .buckets
                .lock()
                .unwrap()
                .get_mut(&bucket)
                .and_then(|objects| objects.remove(&key));
            // Google Cloud Storage says when there was nothing to delete;
            // S3 does not.
            if gcs && removed.is_none() {
                return error(StatusCode::NOT_FOUND, "NoSuchKey", "The key does not exist");
            }
            StatusCode::NO_CONTENT.into_response()
        }
        (method, false) if method == Method::GET || method == Method::HEAD => {
            let object = state.buckets.lock().unwrap()[&bucket].get(&key).cloned();
            match object {
                Some(object) => get(&object, &parts.headers, method == Method::HEAD),
                None if method == Method::HEAD => StatusCode::NOT_FOUND.into_response(),
                None => error(StatusCode::NOT_FOUND, "NoSuchKey", "The key does not exist"),
            }
        }
        _ => error(
            StatusCode::NOT_IMPLEMENTED,
            "NotImplemented",
            "The fake S3 server does not implement this request",
        ),
    }
}

/// Stores an object and returns its ETag.
fn store(
    state: &ServerState,
    bucket: &str,
    key: &str,
    body: Bytes,
    content_type: Option<String>,
) -> String {
    let tag = etag(&body);
    state
        .buckets
        .lock()
        .unwrap()
        .get_mut(bucket)
        .unwrap()
        .insert(
            key.to_owned(),
            Object {
                body,
                content_type,
                last_modified: Utc::now(),
                etag: tag.clone(),
            },
        );
    tag
}

/// GET or HEAD of an object, with `Range: bytes=a-b` support.
fn get(object: &Object, headers: &HeaderMap, head: bool) -> Response {
    let total = object.body.len();
    let range = headers
        .get("range")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("bytes="))
        .map(|range| {
            let (start, end) = range.split_once('-').unwrap();
            match (start.parse::<usize>(), end.parse::<usize>()) {
                (Ok(start), Ok(end)) => (start, end.min(total.saturating_sub(1))),
                (Ok(start), Err(_)) => (start, total.saturating_sub(1)),
                (Err(_), Ok(suffix)) => (total.saturating_sub(suffix), total.saturating_sub(1)),
                _ => (0, total.saturating_sub(1)),
            }
        });
    let (status, body) = match range {
        Some((start, _)) if start >= total => {
            return error(
                StatusCode::RANGE_NOT_SATISFIABLE,
                "InvalidRange",
                "The requested range is not satisfiable",
            );
        }
        Some((start, end)) => (StatusCode::PARTIAL_CONTENT, object.body.slice(start..=end)),
        None => (StatusCode::OK, object.body.clone()),
    };
    let mut response = Response::builder()
        .status(status)
        .header("content-length", body.len())
        .header("etag", &object.etag)
        .header(
            "last-modified",
            object
                .last_modified
                .format("%a, %d %b %Y %H:%M:%S GMT")
                .to_string(),
        )
        .header("accept-ranges", "bytes");
    if let Some((start, end)) = range {
        response = response.header("content-range", format!("bytes {start}-{end}/{total}"));
    }
    if let Some(content_type) = &object.content_type {
        response = response.header("content-type", content_type);
    }
    let body = if head {
        Body::empty()
    } else {
        Body::from(body)
    };
    response.body(body).unwrap()
}

/// ListObjectsV2: every key under `prefix`, in one page.
fn list(state: &ServerState, bucket: &str, prefix: &str) -> Response {
    let buckets = state.buckets.lock().unwrap();
    let contents: Vec<String> = buckets[bucket]
        .iter()
        .filter(|(key, _)| key.starts_with(prefix))
        .map(|(key, object)| {
            format!(
                "<Contents><Key>{}</Key><LastModified>{}</LastModified><ETag>{}</ETag><Size>{}</Size><StorageClass>STANDARD</StorageClass></Contents>",
                escape(key),
                object.last_modified.format("%Y-%m-%dT%H:%M:%S%.3fZ"),
                escape(&object.etag),
                object.body.len()
            )
        })
        .collect();
    xml(format!(
        "<ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Name>{}</Name><Prefix>{}</Prefix><KeyCount>{}</KeyCount><MaxKeys>1000</MaxKeys><IsTruncated>false</IsTruncated>{}</ListBucketResult>",
        escape(bucket),
        escape(prefix),
        contents.len(),
        contents.concat()
    ))
}

/// DeleteObjects.
fn bulk_delete(state: &ServerState, bucket: &str, body: &[u8]) -> Response {
    let text = String::from_utf8_lossy(body);
    let mut deleted = String::new();
    let mut buckets = state.buckets.lock().unwrap();
    for key in elements(&text, "Key") {
        buckets.get_mut(bucket).unwrap().remove(&key);
        deleted.push_str(&format!("<Deleted><Key>{}</Key></Deleted>", escape(&key)));
    }
    xml(format!("<DeleteResult>{deleted}</DeleteResult>"))
}

/// Checks the request's SigV4 signature: presigned (in the query) or in the
/// `Authorization` header.
fn authorize(
    method: &Method,
    raw_path: &str,
    query: &[(String, String)],
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(), String> {
    let get = |name: &str| {
        query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .unwrap_or("")
    };
    let (credential, amz_date, signed_headers, signature, payload) =
        if let Some(signature) = get("X-Amz-Signature") {
            if get("X-Amz-Algorithm") != Some("AWS4-HMAC-SHA256") {
                return Err("unknown algorithm".into());
            }
            let amz_date = get("X-Amz-Date").ok_or("no X-Amz-Date")?;
            let expires: i64 = get("X-Amz-Expires")
                .and_then(|value| value.parse().ok())
                .ok_or("no X-Amz-Expires")?;
            let signed_at = NaiveDateTime::parse_from_str(amz_date, "%Y%m%dT%H%M%SZ")
                .map_err(|_| "bad X-Amz-Date")?
                .and_utc();
            if Utc::now() > signed_at + chrono::Duration::seconds(expires) {
                return Err("Request has expired".into());
            }
            (
                get("X-Amz-Credential").ok_or("no credential")?.to_owned(),
                amz_date.to_owned(),
                get("X-Amz-SignedHeaders")
                    .ok_or("no signed headers")?
                    .to_owned(),
                signature.to_owned(),
                "UNSIGNED-PAYLOAD".to_owned(),
            )
        } else {
            let authorization = header("authorization");
            let fields = authorization
                .strip_prefix("AWS4-HMAC-SHA256 ")
                .ok_or("no AWS4-HMAC-SHA256 authorization")?;
            let field = |name: &str| {
                fields
                    .split(',')
                    .map(str::trim)
                    .find_map(|part| part.strip_prefix(name))
                    .map(str::to_owned)
                    .ok_or_else(|| format!("no {name}"))
            };
            let payload = header("x-amz-content-sha256").to_owned();
            if payload != "UNSIGNED-PAYLOAD" && payload != sha256_hex(body) {
                return Err("the payload hash does not match".into());
            }
            (
                field("Credential=")?,
                header("x-amz-date").to_owned(),
                field("SignedHeaders=")?,
                field("Signature=")?,
                payload,
            )
        };

    let (access_key, scope) = credential.split_once('/').ok_or("bad credential")?;
    if access_key != ACCESS_KEY {
        return Err(format!("unknown access key `{access_key}`"));
    }
    let scope_parts: Vec<&str> = scope.split('/').collect();
    if scope_parts.len() != 4 || scope_parts[1] != REGION || scope_parts[2] != "s3" {
        return Err(format!("unexpected scope `{scope}`"));
    }
    if !amz_date.starts_with(scope_parts[0]) {
        return Err(format!("the scope's date is not the one of `{amz_date}`"));
    }
    // S3 signs the path as sent, not encoded a second time.
    let canonical_query = sigv4::canonical_query(
        query
            .iter()
            .filter(|(key, _)| key != "X-Amz-Signature")
            .map(|(key, value)| (key.as_str(), value.as_str())),
    );
    let signed: Vec<(&str, &str)> = signed_headers
        .split(';')
        .map(|name| (name, header(name)))
        .collect();
    let canonical = sigv4::canonical_request(
        method.as_str(),
        raw_path,
        &canonical_query,
        &signed,
        &payload,
    );
    if canonical.signed_headers != signed_headers {
        return Err(format!("`{signed_headers}` is not in canonical order"));
    }
    let expected = sigv4::signature(SECRET_KEY, &amz_date, REGION, "s3", &canonical.text);
    if navree_crypto::constant_time_eq(expected.as_bytes(), signature.as_bytes()) {
        Ok(())
    } else {
        Err("The request signature we calculated does not match the signature you provided".into())
    }
}

/// What Google Cloud Storage checks before it stores an upload: the size
/// range a signed link bound (`x-goog-content-length-range: min,max`) and a
/// write only if absent (`x-goog-if-generation-match: 0`).
fn gcs_preconditions(
    state: &ServerState,
    bucket: &str,
    key: &str,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(), Response> {
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    if let Some(range) = header("x-goog-content-length-range") {
        let (min, max) = range
            .split_once(',')
            .and_then(|(min, max)| Some((min.trim().parse().ok()?, max.trim().parse().ok()?)))
            .ok_or_else(|| {
                error(
                    StatusCode::BAD_REQUEST,
                    "InvalidArgument",
                    "Invalid x-goog-content-length-range",
                )
            })?;
        let len = body.len() as u64;
        if len < min || len > max {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "EntityTooLarge",
                "Your proposed upload is outside the size range of the signed link",
            ));
        }
    }
    if header("x-goog-if-generation-match") == Some("0")
        && state.buckets.lock().unwrap()[bucket].contains_key(key)
    {
        return Err(error(
            StatusCode::PRECONDITION_FAILED,
            "PreconditionFailed",
            "At least one of the pre-conditions you specified did not hold",
        ));
    }
    Ok(())
}

/// Checks a Google Cloud Storage request: a `GOOG4-RSA-SHA256` signed link
/// (in the query), or the service account's self-signed token (RS256) in
/// `Authorization: Bearer`. Both are verified with [`GCS_PRIVATE_KEY`]'s
/// public half.
fn authorize_gcs(
    method: &Method,
    raw_path: &str,
    query: &[(String, String)],
    headers: &HeaderMap,
) -> Result<(), String> {
    use navree_crypto::jose::{Algorithm, Jws, SigningKey};

    let key = SigningKey::from_pem(GCS_PRIVATE_KEY)
        .map_err(|error| format!("the test key: {error}"))?
        .verifying_key();
    let get = |name: &str| {
        query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .unwrap_or("")
    };
    let Some(signature) = get("X-Goog-Signature") else {
        let token = header("authorization")
            .strip_prefix("Bearer ")
            .ok_or("no bearer token")?;
        let jws = Jws::parse(token).map_err(|error| format!("the token: {error}"))?;
        return jws
            .verify(&key)
            .map_err(|error| format!("the token's signature: {error}"));
    };
    if get("X-Goog-Algorithm") != Some("GOOG4-RSA-SHA256") {
        return Err("unknown algorithm".into());
    }
    let date = get("X-Goog-Date").ok_or("no X-Goog-Date")?;
    let expires: i64 = get("X-Goog-Expires")
        .and_then(|value| value.parse().ok())
        .ok_or("no X-Goog-Expires")?;
    let signed_at = NaiveDateTime::parse_from_str(date, "%Y%m%dT%H%M%SZ")
        .map_err(|_| "bad X-Goog-Date")?
        .and_utc();
    if Utc::now() > signed_at + chrono::Duration::seconds(expires) {
        return Err("Request has expired".into());
    }
    let credential = get("X-Goog-Credential").ok_or("no credential")?;
    let (email, scope) = credential.split_once('/').ok_or("bad credential")?;
    if email != GCS_EMAIL {
        return Err(format!("unknown service account `{email}`"));
    }
    let scope_parts: Vec<&str> = scope.split('/').collect();
    if scope_parts.len() != 4 || scope_parts[2] != "storage" || scope_parts[3] != "goog4_request" {
        return Err(format!("unexpected scope `{scope}`"));
    }
    let signed_headers = get("X-Goog-SignedHeaders").ok_or("no signed headers")?;
    let mut pairs: Vec<(String, String)> = query
        .iter()
        .filter(|(key, _)| key != "X-Goog-Signature")
        .map(|(key, value)| (aws_encode(key), aws_encode(value)))
        .collect();
    pairs.sort();
    let canonical_query = pairs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    let canonical_headers: String = signed_headers
        .split(';')
        .map(|name| {
            let value = if name == "host" {
                GCS_HOST
            } else {
                header(name)
            };
            format!("{name}:{value}\n")
        })
        .collect();
    let canonical_request = format!(
        "{method}\n{raw_path}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\nUNSIGNED-PAYLOAD"
    );
    let string_to_sign = format!(
        "GOOG4-RSA-SHA256\n{date}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let signature = hex::decode(signature).map_err(|_| "a signature that is not hex")?;
    key.verify(Algorithm::RS256, string_to_sign.as_bytes(), &signature)
        .map_err(|_| {
            "The request signature we calculated does not match the signature you provided".into()
        })
}

/// RFC 3986 encoding as SigV4 wants it.
fn aws_encode(value: &str) -> String {
    const SET: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~');
    utf8_percent_encode(value, SET).to_string()
}

/// `a=1&b` → `[(a, 1), (b, "")]`, percent-decoded.
fn parse_query(query: &str) -> Vec<(String, String)> {
    let decode = |value: &str| {
        percent_decode_str(&value.replace('+', "%20"))
            .decode_utf8()
            .unwrap()
            .into_owned()
    };
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) => (decode(key), decode(value)),
            None => (decode(pair), String::new()),
        })
        .collect()
}

/// The text of every `<name>` element.
fn elements(xml: &str, name: &str) -> Vec<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let mut found = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        rest = &rest[start + open.len()..];
        let end = rest.find(&close).unwrap();
        found.push(unescape(&rest[..end]));
        rest = &rest[end + close.len()..];
    }
    found
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn etag(body: &[u8]) -> String {
    format!("\"{}\"", &sha256_hex(body)[..32])
}

fn xml(body: String) -> Response {
    (
        [("content-type", "application/xml")],
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>{body}"),
    )
        .into_response()
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    let mut response = xml(format!(
        "<Error><Code>{code}</Code><Message>{}</Message></Error>",
        escape(message)
    ));
    *response.status_mut() = status;
    response
}
