//! AWS Signature Version 4 (HMAC-SHA256), for the SES v2 API.
//!
//! <https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_sigv-create-signed-request.html>

use chrono::{DateTime, Utc};
use hmac::{KeyInit as _, Mac as _};
use sha2::{Digest as _, Sha256};
use url::Url;

/// AWS credentials.
#[derive(Clone)]
pub(crate) struct Credentials {
    pub(crate) access_key_id: String,
    pub(crate) secret_access_key: String,
    pub(crate) session_token: Option<String>,
}

/// Signs requests for one service in one region.
pub(crate) struct Signer<'a> {
    pub(crate) credentials: &'a Credentials,
    pub(crate) region: &'a str,
    pub(crate) service: &'a str,
}

impl Signer<'_> {
    /// The headers to add to a request: `x-amz-date`, `authorization` and,
    /// with temporary credentials, `x-amz-security-token`.
    ///
    /// `headers` are the other headers to sign (`host` is added from
    /// `url`), as `(name, value)` pairs; names are lower-cased here.
    pub(crate) fn sign(
        &self,
        method: &str,
        url: &Url,
        headers: &[(&str, &str)],
        body: &[u8],
        now: DateTime<Utc>,
    ) -> Vec<(String, String)> {
        sign(
            method,
            url,
            headers,
            body,
            (self.credentials, self.region, self.service),
            now,
        )
    }
}

fn sign(
    method: &str,
    url: &Url,
    headers: &[(&str, &str)],
    body: &[u8],
    (credentials, region, service): (&Credentials, &str, &str),
    now: DateTime<Utc>,
) -> Vec<(String, String)> {
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let mut signed: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), normalize_value(value)))
        .collect();
    signed.push(("host".to_owned(), host_header(url)));
    signed.push(("x-amz-date".to_owned(), amz_date.clone()));
    if let Some(token) = &credentials.session_token {
        signed.push(("x-amz-security-token".to_owned(), token.clone()));
    }
    signed.sort();
    let signed_names = signed
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical = canonical_request(method, url, &signed, &signed_names, body);
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    );
    let key = signing_key(&credentials.secret_access_key, &date, region, service);
    let signature = hex::encode(hmac(&key, string_to_sign.as_bytes()));
    let mut out = vec![
        ("x-amz-date".to_owned(), amz_date),
        (
            "authorization".to_owned(),
            format!(
                "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_names}, Signature={signature}",
                credentials.access_key_id
            ),
        ),
    ];
    if let Some(token) = &credentials.session_token {
        out.push(("x-amz-security-token".to_owned(), token.clone()));
    }
    out
}

/// The `Host` header for `url`: the host, with the port when it is not the
/// scheme's default.
fn host_header(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

/// A header value trimmed, with inner runs of spaces collapsed.
fn normalize_value(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The canonical request (task 1).
fn canonical_request(
    method: &str,
    url: &Url,
    headers: &[(String, String)],
    signed_names: &str,
    body: &[u8],
) -> String {
    let path = canonical_path(url.path());
    let query = canonical_query(url);
    let headers: String = headers
        .iter()
        .map(|(name, value)| format!("{name}:{value}\n"))
        .collect();
    format!(
        "{method}\n{path}\n{query}\n{headers}\n{signed_names}\n{}",
        hex::encode(Sha256::digest(body))
    )
}

/// Each path segment URI-encoded (the path is already percent-encoded once
/// by `Url`, so services other than S3 get it encoded twice, as SigV4
/// asks).
fn canonical_path(path: &str) -> String {
    if path.is_empty() {
        return "/".to_owned();
    }
    path.split('/')
        .map(|segment| uri_encode(segment, true))
        .collect::<Vec<_>>()
        .join("/")
}

/// The query parameters, decoded, re-encoded and sorted.
fn canonical_query(url: &Url) -> String {
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(key, value)| (uri_encode(&key, true), uri_encode(&value, true)))
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// SigV4's URI encoding: every byte but `A-Z a-z 0-9 - _ . ~` as `%XX`
/// (and `/` too when `slash` is true).
fn uri_encode(input: &str, slash: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else if byte == b'/' && !slash {
            out.push('/');
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The signing key: `AWS4<secret>` through date, region, service and
/// `aws4_request`.
fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let key = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let key = hmac(&key, region.as_bytes());
    let key = hmac(&key, service.as_bytes());
    hmac(&key, b"aws4_request")
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = <hmac::Hmac<Sha256>>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;

    fn credentials() -> Credentials {
        Credentials {
            access_key_id: "AKIDEXAMPLE".to_owned(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_owned(),
            session_token: None,
        }
    }

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2015, 8, 30, 12, 36, 0).unwrap()
    }

    fn authorization(headers: &[(String, String)]) -> &str {
        &headers
            .iter()
            .find(|(name, _)| name == "authorization")
            .expect("authorization")
            .1
    }

    /// The derived key of AWS's "derive a signing key" example.
    #[test]
    fn signing_key_matches_the_aws_example() {
        let key = signing_key(
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "20150830",
            "us-east-1",
            "iam",
        );
        assert_eq!(
            hex::encode(key),
            "c4afb1cc5771d871763a393e44b703571b55cc28424d1a5e86da6ed3c154a4b9"
        );
    }

    /// AWS's IAM `ListUsers` example of a signed request.
    #[test]
    fn signature_matches_the_aws_iam_example() {
        let url =
            Url::parse("https://iam.amazonaws.com/?Action=ListUsers&Version=2010-05-08").unwrap();
        let headers = sign(
            "GET",
            &url,
            &[(
                "Content-Type",
                "application/x-www-form-urlencoded; charset=utf-8",
            )],
            b"",
            (&credentials(), "us-east-1", "iam"),
            at(),
        );
        assert_eq!(
            authorization(&headers),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date, \
             Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
        );
        assert!(headers.contains(&("x-amz-date".to_owned(), "20150830T123600Z".to_owned())));
    }

    /// `get-vanilla` from the SigV4 test suite.
    #[test]
    fn signature_matches_the_get_vanilla_vector() {
        let url = Url::parse("https://example.amazonaws.com/").unwrap();
        let headers = sign(
            "GET",
            &url,
            &[],
            b"",
            (&credentials(), "us-east-1", "service"),
            at(),
        );
        assert_eq!(
            authorization(&headers),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, \
             Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }

    /// `post-vanilla` from the SigV4 test suite.
    #[test]
    fn signature_matches_the_post_vanilla_vector() {
        let url = Url::parse("https://example.amazonaws.com/").unwrap();
        let headers = sign(
            "POST",
            &url,
            &[],
            b"",
            (&credentials(), "us-east-1", "service"),
            at(),
        );
        assert!(
            authorization(&headers).ends_with(
                "Signature=5da7c1a2acd57cee7505fc6676e4e544621c30862966e37dddb68e92efbe5d6b"
            ),
            "{}",
            authorization(&headers)
        );
    }

    /// `get-vanilla-query-order-key-case` from the SigV4 test suite: query
    /// parameters are sorted.
    #[test]
    fn signature_sorts_query_parameters() {
        let url = Url::parse("https://example.amazonaws.com/?Param2=value2&Param1=value1").unwrap();
        let headers = sign(
            "GET",
            &url,
            &[],
            b"",
            (&credentials(), "us-east-1", "service"),
            at(),
        );
        assert!(
            authorization(&headers).ends_with(
                "Signature=b97d918cfa904a5beff61c982a1b6f458b799221646efd99d3219ec94cdf2500"
            ),
            "{}",
            authorization(&headers)
        );
    }

    #[test]
    fn temporary_credentials_sign_the_security_token() {
        let url =
            Url::parse("https://email.eu-west-1.amazonaws.com/v2/email/outbound-emails").unwrap();
        let mut credentials = credentials();
        credentials.session_token = Some("session".to_owned());
        let headers = sign(
            "POST",
            &url,
            &[],
            b"{}",
            (&credentials, "eu-west-1", "ses"),
            at(),
        );
        assert!(
            authorization(&headers).contains("SignedHeaders=host;x-amz-date;x-amz-security-token")
        );
        assert!(headers.contains(&("x-amz-security-token".to_owned(), "session".to_owned())));
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
}
