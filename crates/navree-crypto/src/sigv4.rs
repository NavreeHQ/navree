//! AWS Signature Version 4 (`AWS4-HMAC-SHA256`): the one signer of the
//! requests Navree sends to AWS (Bedrock in navree-ai, SES in navree-mail,
//! SNS in navree-notify), checked against the requests of AWS's published
//! SigV4 test suite.
//!
//! [`Signer::sign`] gives the headers to add to a request: `x-amz-date`,
//! `authorization` and, with temporary credentials, `x-amz-security-token`.
//! The parts it is made of ([`canonical_request`], [`canonical_path`],
//! [`canonical_query`], [`signature`]) are public too, so a server that
//! receives signed requests (a test double of S3, for one) checks them with
//! the same code.
//!
//! ```
//! use chrono::{TimeZone, Utc};
//! use navree_crypto::sigv4::{Credentials, Signer};
//! use url::Url;
//!
//! let signer = Signer::new(
//!     Credentials::new("AKIDEXAMPLE", "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY"),
//!     "us-east-1",
//!     "service",
//! );
//! let url = Url::parse("https://example.amazonaws.com/").unwrap();
//! let now = Utc.with_ymd_and_hms(2015, 8, 30, 12, 36, 0).unwrap();
//! let headers = signer.sign("GET", &url, &[], b"", now);
//! assert_eq!(headers[0], ("x-amz-date".to_owned(), "20150830T123600Z".to_owned()));
//! assert_eq!(
//!     headers[1].1,
//!     "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
//!      SignedHeaders=host;x-amz-date, \
//!      Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
//! );
//! ```
//!
//! <https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_sigv-create-signed-request.html>

use std::fmt;

use chrono::{DateTime, Utc};
use sha2::{Digest as _, Sha256};
use url::Url;

use crate::tokens::hmac_sha256;

/// The algorithm name, first in the `Authorization` header and the string
/// to sign.
pub const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// What S3 signs in place of the body's hash when the body is not signed.
pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";

/// AWS credentials: an access key and, for temporary credentials, a
/// session token. `Debug` shows none of them.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    /// The access key id (`AKIA...`, or `ASIA...` for temporary ones).
    pub access_key_id: String,
    /// The secret access key.
    pub secret_access_key: String,
    /// The session token of temporary credentials.
    pub session_token: Option<String>,
}

impl Credentials {
    /// Long-term credentials.
    pub fn new(access_key_id: impl Into<String>, secret_access_key: impl Into<String>) -> Self {
        Self {
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
            session_token: None,
        }
    }

    /// These credentials with `token`, when it is not empty.
    #[must_use]
    pub fn with_session_token(mut self, token: Option<String>) -> Self {
        self.session_token = token.filter(|token| !token.is_empty());
        self
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("access_key_id", &"[redacted]")
            .finish_non_exhaustive()
    }
}

/// Signs requests to one service in one region.
#[derive(Clone, Debug)]
pub struct Signer {
    credentials: Credentials,
    region: String,
    service: String,
    encode_path_twice: bool,
    unsigned_payload: bool,
}

impl Signer {
    /// A signer for `service` (`ses`, `sns`, `bedrock`) in `region`.
    pub fn new(
        credentials: Credentials,
        region: impl Into<String>,
        service: impl Into<String>,
    ) -> Self {
        Self {
            credentials,
            region: region.into(),
            service: service.into(),
            encode_path_twice: true,
            unsigned_payload: false,
        }
    }

    /// S3's rule for the path: the path as sent, not encoded a second time
    /// as every other service wants it.
    #[must_use]
    pub fn path_as_sent(mut self) -> Self {
        self.encode_path_twice = false;
        self
    }

    /// Signs [`UNSIGNED_PAYLOAD`] in place of the body's SHA-256, and sends
    /// it as `x-amz-content-sha256` (S3).
    #[must_use]
    pub fn unsigned_payload(mut self) -> Self {
        self.unsigned_payload = true;
        self
    }

    /// The credentials requests are signed with.
    pub fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    /// The region.
    pub fn region(&self) -> &str {
        &self.region
    }

    /// The service.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// The headers to add to a `method` request to `url` with `body`, signed
    /// at `now`: `x-amz-date`, `authorization`, then `x-amz-content-sha256`
    /// with an [unsigned payload](Signer::unsigned_payload) and
    /// `x-amz-security-token` with temporary credentials.
    ///
    /// `headers` are the other headers to sign, such as `content-type`, as
    /// `(name, value)` pairs in any case; a name given twice is signed as one
    /// header with both values, in order. `host` (with the port when it is
    /// not the scheme's default), `x-amz-date` and the token are added here,
    /// so leave them out.
    pub fn sign(
        &self,
        method: &str,
        url: &Url,
        headers: &[(&str, &str)],
        body: &[u8],
        now: DateTime<Utc>,
    ) -> Vec<(String, String)> {
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let payload_hash = if self.unsigned_payload {
            UNSIGNED_PAYLOAD.to_owned()
        } else {
            hex::encode(Sha256::digest(body))
        };
        let host = host_header(url);
        let mut signed: Vec<(&str, &str)> = headers.to_vec();
        signed.push(("host", &host));
        signed.push(("x-amz-date", &amz_date));
        if self.unsigned_payload {
            signed.push(("x-amz-content-sha256", UNSIGNED_PAYLOAD));
        }
        if let Some(token) = &self.credentials.session_token {
            signed.push(("x-amz-security-token", token));
        }
        let canonical = canonical_request(
            method,
            &canonical_path(url.path(), self.encode_path_twice),
            &canonical_query(url.query_pairs()),
            &signed,
            &payload_hash,
        );
        let signature = signature(
            &self.credentials.secret_access_key,
            &amz_date,
            &self.region,
            &self.service,
            &canonical.text,
        );
        let authorization = format!(
            "{ALGORITHM} Credential={}/{}, SignedHeaders={}, Signature={signature}",
            self.credentials.access_key_id,
            scope(&amz_date[..8], &self.region, &self.service),
            canonical.signed_headers,
        );
        let mut out = vec![
            ("x-amz-date".to_owned(), amz_date),
            ("authorization".to_owned(), authorization),
        ];
        if self.unsigned_payload {
            out.push((
                "x-amz-content-sha256".to_owned(),
                UNSIGNED_PAYLOAD.to_owned(),
            ));
        }
        if let Some(token) = &self.credentials.session_token {
            out.push(("x-amz-security-token".to_owned(), token.clone()));
        }
        out
    }
}

/// A request in SigV4's canonical form, and the names of the headers it
/// signs (`content-type;host;x-amz-date`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalRequest {
    /// The canonical request: method, path, query, headers, signed header
    /// names and payload hash, one per line.
    pub text: String,
    /// The signed header names, lower case, sorted, joined with `;`.
    pub signed_headers: String,
}

/// The canonical request of `method` on `path` and `query` (both already
/// canonical: see [`canonical_path`] and [`canonical_query`]) with
/// `headers` and the body's `payload_hash`. Header names are lower-cased
/// and sorted, values trimmed with inner runs of whitespace collapsed, and
/// the values of a name given more than once joined with `,` in order.
pub fn canonical_request(
    method: &str,
    path: &str,
    query: &str,
    headers: &[(&str, &str)],
    payload_hash: &str,
) -> CanonicalRequest {
    let mut merged: Vec<(String, String)> = Vec::with_capacity(headers.len());
    for (name, value) in headers {
        let name = name.to_ascii_lowercase();
        let value = collapse(value);
        match merged.iter_mut().find(|(known, _)| *known == name) {
            Some((_, values)) => {
                values.push(',');
                values.push_str(&value);
            }
            None => merged.push((name, value)),
        }
    }
    merged.sort_by(|a, b| a.0.cmp(&b.0));
    let signed_headers = merged
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical_headers: String = merged
        .iter()
        .map(|(name, value)| format!("{name}:{value}\n"))
        .collect();
    CanonicalRequest {
        text: format!(
            "{method}\n{path}\n{query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
        ),
        signed_headers,
    }
}

/// The canonical path of `path`, the path as sent (percent-encoded once):
/// each segment encoded again with `encode_twice`, as every service but S3
/// wants it, else the path itself. An empty path is `/`.
pub fn canonical_path(path: &str, encode_twice: bool) -> String {
    if path.is_empty() {
        return "/".to_owned();
    }
    if !encode_twice {
        return path.to_owned();
    }
    path.split('/')
        .map(|segment| uri_encode(segment, true))
        .collect::<Vec<_>>()
        .join("/")
}

/// The canonical query of decoded `(name, value)` pairs: each encoded with
/// [`uri_encode`], sorted by name then value, joined with `&`.
pub fn canonical_query<K: AsRef<str>, V: AsRef<str>>(
    pairs: impl IntoIterator<Item = (K, V)>,
) -> String {
    let mut pairs: Vec<(String, String)> = pairs
        .into_iter()
        .map(|(name, value)| {
            (
                uri_encode(name.as_ref(), true),
                uri_encode(value.as_ref(), true),
            )
        })
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// SigV4's URI encoding: every byte but `A-Z a-z 0-9 - _ . ~` as `%XX`
/// (upper-case hex), and `/` too when `encode_slash` is true.
pub fn uri_encode(input: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else if byte == b'/' && !encode_slash {
            out.push('/');
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The credential scope: `{date}/{region}/{service}/aws4_request`.
pub fn scope(date: &str, region: &str, service: &str) -> String {
    format!("{date}/{region}/{service}/aws4_request")
}

/// The string to sign: the algorithm, `amz_date`, the `scope` and the
/// SHA-256 of the canonical request, one per line.
pub fn string_to_sign(amz_date: &str, scope: &str, canonical_request: &str) -> String {
    format!(
        "{ALGORITHM}\n{amz_date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    )
}

/// The signing key: `AWS4<secret>` through the date (`YYYYMMDD`), region,
/// service and `aws4_request`.
pub fn signing_key(secret_access_key: &str, date: &str, region: &str, service: &str) -> [u8; 32] {
    let key = hmac_sha256(
        format!("AWS4{secret_access_key}").as_bytes(),
        date.as_bytes(),
    );
    let key = hmac_sha256(&key, region.as_bytes());
    let key = hmac_sha256(&key, service.as_bytes());
    hmac_sha256(&key, b"aws4_request")
}

/// The signature (lower-case hex) of `canonical_request`, made at `amz_date`
/// (`YYYYMMDDTHHMMSSZ`, whose date is the scope's) for `service` in
/// `region`.
pub fn signature(
    secret_access_key: &str,
    amz_date: &str,
    region: &str,
    service: &str,
    canonical_request: &str,
) -> String {
    let date = amz_date.get(..8).unwrap_or(amz_date);
    let string_to_sign = string_to_sign(amz_date, &scope(date, region, service), canonical_request);
    hex::encode(hmac_sha256(
        &signing_key(secret_access_key, date, region, service),
        string_to_sign.as_bytes(),
    ))
}

/// The `Host` header of `url`: the host, with the port when it is not the
/// scheme's default.
fn host_header(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

/// `value` trimmed, with inner runs of whitespace collapsed to one space.
fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;

    #[test]
    fn values_are_trimmed_and_repeated_names_joined() {
        let canonical = canonical_request(
            "GET",
            "/",
            "",
            &[
                ("X-B", "  two   words "),
                ("x-a", "1"),
                ("X-A", "2"),
                ("x-a", " 3"),
            ],
            UNSIGNED_PAYLOAD,
        );
        assert_eq!(canonical.signed_headers, "x-a;x-b");
        assert_eq!(
            canonical.text,
            "GET\n/\n\nx-a:1,2,3\nx-b:two words\n\nx-a;x-b\nUNSIGNED-PAYLOAD"
        );
    }

    #[test]
    fn paths_are_encoded_twice_except_for_s3() {
        assert_eq!(
            canonical_path("/model/amazon.titan-embed-text-v2%3A0/invoke", true),
            "/model/amazon.titan-embed-text-v2%253A0/invoke"
        );
        assert_eq!(canonical_path("/bucket/a%20b/c", false), "/bucket/a%20b/c");
        assert_eq!(canonical_path("", true), "/");
        assert_eq!(canonical_path("", false), "/");
        assert_eq!(uri_encode("a:b c/d", true), "a%3Ab%20c%2Fd");
        assert_eq!(uri_encode("a:b c/d", false), "a%3Ab%20c/d");
        assert_eq!(uri_encode("\u{1234}", true), "%E1%88%B4");
    }

    #[test]
    fn the_host_header_keeps_a_non_default_port() {
        assert_eq!(
            host_header(&Url::parse("http://127.0.0.1:8080/x").unwrap()),
            "127.0.0.1:8080"
        );
        assert_eq!(
            host_header(&Url::parse("https://example.com:443/x").unwrap()),
            "example.com"
        );
    }

    #[test]
    fn credentials_never_show_in_debug_output() {
        let credentials =
            Credentials::new("AKIDEXAMPLE", "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY")
                .with_session_token(Some("SESSIONTOKEN".to_owned()));
        let signer = Signer::new(credentials, "us-east-1", "service");
        let shown = format!("{signer:?}");
        for secret in ["AKIDEXAMPLE", "wJalrXUtnFEMI", "SESSIONTOKEN"] {
            assert!(!shown.contains(secret), "{shown}");
        }
        assert_eq!(
            Credentials::new("a", "b")
                .with_session_token(Some(String::new()))
                .session_token,
            None
        );
    }

    #[test]
    fn an_unsigned_payload_is_signed_and_sent() {
        let signer = Signer::new(Credentials::new("AKIDEXAMPLE", "secret"), "us-east-1", "s3")
            .path_as_sent()
            .unsigned_payload();
        let url = Url::parse("https://bucket.s3.amazonaws.com/a%20b").unwrap();
        let now = Utc.with_ymd_and_hms(2015, 8, 30, 12, 36, 0).unwrap();
        let headers = signer.sign("PUT", &url, &[], b"body", now);
        assert!(headers.contains(&(
            "x-amz-content-sha256".to_owned(),
            UNSIGNED_PAYLOAD.to_owned()
        )));
        let authorization = &headers[1].1;
        assert!(
            authorization.contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date"),
            "{authorization}"
        );
        // The signature is the one of the canonical request with the path
        // as sent and the unsigned payload.
        let canonical = canonical_request(
            "PUT",
            "/a%20b",
            "",
            &[
                ("host", "bucket.s3.amazonaws.com"),
                ("x-amz-content-sha256", UNSIGNED_PAYLOAD),
                ("x-amz-date", "20150830T123600Z"),
            ],
            UNSIGNED_PAYLOAD,
        );
        let expected = signature(
            "secret",
            "20150830T123600Z",
            "us-east-1",
            "s3",
            &canonical.text,
        );
        assert!(authorization.ends_with(&format!("Signature={expected}")));
    }
}
