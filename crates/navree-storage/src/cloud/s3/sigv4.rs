//! AWS Signature Version 4 (`AWS4-HMAC-SHA256`) for S3 requests and
//! presigned links, built on [`navree_crypto::sigv4`].
//!
//! Every header-signed S3 request carries `x-amz-date`,
//! `x-amz-content-sha256` (the lower-case hex SHA-256 of the body),
//! `authorization`, and `x-amz-security-token` when temporary credentials
//! are in use. Unlike other AWS services, S3 signs the request path as
//! sent without encoding it a second time.
//!
//! Presigned links (`GET` and `PUT`) put the credential scope, timestamp,
//! lifetime (`1` second up to `7` days), signed header names, optional
//! session token and signature in the query string, with
//! `UNSIGNED-PAYLOAD` in place of a body digest.

use std::time::Duration;

use ::http::Method;
use ::http::header::{HeaderName, HeaderValue};
use navree_crypto::sha256_hex;
use navree_crypto::sigv4::{
    self, ALGORITHM, UNSIGNED_PAYLOAD, canonical_path, canonical_query, canonical_request,
    decode_query, scope, signature, uri_encode,
};
use navree_datetime::Timestamp;
use url::Url;

use crate::cloud::http::Request;
use crate::cloud::{Error, ErrorKind, Result};

pub use navree_crypto::sigv4::Credentials;

/// The AWS service name of S3.
pub const SERVICE: &str = "s3";

/// The longest lifetime S3 allows on a presigned link (7 days).
pub const MAX_PRESIGN_EXPIRES: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Headers controlled by the transport or the signer itself, which a
/// caller cannot bind on a presigned link.
const RESERVED_PRESIGN_HEADERS: [&str; 7] = [
    "authorization",
    "content-length",
    "host",
    "user-agent",
    "x-amz-content-sha256",
    "x-amz-date",
    "x-amz-security-token",
];

/// Signs `request` in place for S3 in `region` at `now`.
///
/// Any existing headers on `request` (such as `range`, `if-match` or
/// `content-type`) are folded into `SignedHeaders` alongside `host`,
/// `x-amz-content-sha256`, `x-amz-date` and, when present,
/// `x-amz-security-token`. Both `authorization` and `x-amz-security-token`
/// are marked sensitive so nothing logs their values.
pub fn sign_request(
    request: &mut Request,
    credentials: &Credentials,
    region: &str,
    now: Timestamp,
) -> Result<()> {
    check_signer_inputs(&credentials.access_key_id, region)?;
    request.headers.remove("authorization");
    request.headers.remove("host");
    request.headers.remove("x-amz-date");
    request.headers.remove("x-amz-content-sha256");
    request.headers.remove("x-amz-security-token");

    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let payload_hash = sha256_hex(&request.body);
    let host = host_header(&request.url)?;

    insert_header(&mut request.headers, "x-amz-date", &amz_date, false)?;
    insert_header(
        &mut request.headers,
        "x-amz-content-sha256",
        &payload_hash,
        false,
    )?;
    if let Some(token) = &credentials.session_token {
        insert_header(&mut request.headers, "x-amz-security-token", token, true)?;
    }

    let mut signed_pairs: Vec<(&str, &str)> = Vec::with_capacity(request.headers.len() + 1);
    signed_pairs.push(("host", &host));
    for (name, value) in &request.headers {
        let key = name.as_str();
        if matches!(
            key,
            "authorization" | "content-length" | "host" | "user-agent"
        ) {
            continue;
        }
        let text = value.to_str().map_err(|_| {
            Error::new(
                ErrorKind::Refused,
                format!("the value of the `{name}` header cannot be signed"),
            )
        })?;
        signed_pairs.push((key, text));
    }

    let query_pairs = decode_query(request.url.query());
    let canonical = canonical_request(
        request.method.as_str(),
        &canonical_path(request.url.path(), false),
        &canonical_query(query_pairs.iter().map(|(k, v)| (k.as_str(), v.as_str()))),
        &signed_pairs,
        &payload_hash,
    );
    let sig = signature(
        &credentials.secret_access_key,
        &amz_date,
        region,
        SERVICE,
        &canonical.text,
    );
    let authorization = format!(
        "{ALGORITHM} Credential={}/{}, SignedHeaders={}, Signature={sig}",
        credentials.access_key_id,
        scope(&amz_date[..8], region, SERVICE),
        canonical.signed_headers,
    );
    insert_header(&mut request.headers, "authorization", &authorization, true)
}

/// Builds a presigned `GET` or `PUT` URL for `url` in `region` at `now`,
/// valid for `expires_in` and bound to `headers`.
pub fn presign_url(
    method: &Method,
    url: &Url,
    headers: &[(HeaderName, HeaderValue)],
    expires_in: Duration,
    credentials: &Credentials,
    region: &str,
    now: Timestamp,
) -> Result<Url> {
    if *method != Method::GET && *method != Method::PUT {
        return Err(Error::new(
            ErrorKind::Refused,
            format!("S3 signs GET and PUT links, not {method}"),
        ));
    }
    if expires_in < Duration::from_secs(1) {
        return Err(Error::new(
            ErrorKind::Refused,
            "an S3 signed link must stay valid for at least 1 second",
        ));
    }
    if expires_in > MAX_PRESIGN_EXPIRES {
        return Err(Error::new(
            ErrorKind::Refused,
            "S3 signed links last at most 7 days",
        ));
    }
    check_signer_inputs(&credentials.access_key_id, region)?;

    let existing = decode_query(url.query());
    for (query_key, _) in &existing {
        if query_key
            .as_bytes()
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"x-amz-"))
        {
            return Err(Error::new(
                ErrorKind::Refused,
                "a presigned URL cannot start with reserved X-Amz-* query parameters",
            ));
        }
    }

    let host = host_header(url)?;
    let mut header_pairs: Vec<(&str, &str)> = Vec::with_capacity(headers.len() + 1);
    header_pairs.push(("host", &host));
    for (name, value) in headers {
        let key = name.as_str();
        if RESERVED_PRESIGN_HEADERS.contains(&key) {
            return Err(Error::new(
                ErrorKind::Refused,
                format!("S3 cannot bind the header `{name}` on a signed link"),
            ));
        }
        let text = value.to_str().map_err(|_| {
            Error::new(
                ErrorKind::Refused,
                format!("the value of the `{name}` header cannot be signed"),
            )
        })?;
        header_pairs.push((key, text));
    }

    let signed_headers =
        canonical_request(method.as_str(), "/", "", &header_pairs, UNSIGNED_PAYLOAD).signed_headers;
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let cred_scope = scope(&amz_date[..8], region, SERVICE);
    let credential = format!("{}/{cred_scope}", credentials.access_key_id);
    let expires_text = expires_in.as_secs().to_string();

    let mut query_pairs: Vec<(&str, &str)> = Vec::with_capacity(existing.len() + 6);
    for (k, v) in &existing {
        query_pairs.push((k.as_str(), v.as_str()));
    }
    query_pairs.push(("X-Amz-Algorithm", ALGORITHM));
    query_pairs.push(("X-Amz-Credential", &credential));
    query_pairs.push(("X-Amz-Date", &amz_date));
    query_pairs.push(("X-Amz-Expires", &expires_text));
    query_pairs.push(("X-Amz-SignedHeaders", &signed_headers));
    if let Some(token) = &credentials.session_token {
        query_pairs.push(("X-Amz-Security-Token", token.as_str()));
    }

    let canonical = canonical_request(
        method.as_str(),
        &canonical_path(url.path(), false),
        &canonical_query(query_pairs.iter().copied()),
        &header_pairs,
        UNSIGNED_PAYLOAD,
    );
    let sig = signature(
        &credentials.secret_access_key,
        &amz_date,
        region,
        SERVICE,
        &canonical.text,
    );

    let mut raw_query = String::new();
    for (k, v) in &query_pairs {
        if !raw_query.is_empty() {
            raw_query.push('&');
        }
        raw_query.push_str(&uri_encode(k, true));
        raw_query.push('=');
        raw_query.push_str(&uri_encode(v, true));
    }
    raw_query.push_str("&X-Amz-Signature=");
    raw_query.push_str(&sig);

    let mut signed = url.clone();
    signed.set_query(Some(&raw_query));
    let _ = sigv4::ALGORITHM;
    Ok(signed)
}

/// Validates `access_key_id` and `region` so neither can inject delimiters
/// into `Authorization` or presigned query scopes.
fn check_signer_inputs(access_key_id: &str, region: &str) -> Result<()> {
    if access_key_id.is_empty()
        || access_key_id.len() > 128
        || !access_key_id.bytes().all(|byte| {
            (0x21..=0x7E).contains(&byte)
                && !matches!(byte, b'/' | b'\\' | b'?' | b'#' | b'&' | b'=' | b',' | b';')
        })
    {
        return Err(Error::new(
            ErrorKind::Credential,
            "the S3 access key id is empty or contains invalid characters",
        ));
    }
    if region.is_empty()
        || region.len() > 64
        || !region
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(Error::new(
            ErrorKind::Refused,
            "the S3 region name is invalid",
        ));
    }
    Ok(())
}

/// The HTTP `Host` header value of `url`, including `:port` when non-default.
fn host_header(url: &Url) -> Result<String> {
    let Some(host) = url.host_str().filter(|h| !h.is_empty()) else {
        return Err(Error::new(
            ErrorKind::Refused,
            "an S3 request URL must have a host",
        ));
    };
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}

/// Inserts `name: value` into `headers`, marking `sensitive` values so
/// nothing prints them.
fn insert_header(
    headers: &mut ::http::HeaderMap,
    name: &'static str,
    value: &str,
    sensitive: bool,
) -> Result<()> {
    let header_name = HeaderName::from_static(name);
    let mut header_value = HeaderValue::from_str(value).map_err(|_| {
        Error::new(
            ErrorKind::Refused,
            format!("the value of the `{header_name}` header cannot be sent"),
        )
    })?;
    if sensitive {
        header_value.set_sensitive(true);
    }
    headers.insert(header_name, header_value);
    Ok(())
}
