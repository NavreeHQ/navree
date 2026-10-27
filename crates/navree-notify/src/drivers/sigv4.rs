//! AWS Signature Version 4 (HMAC-SHA256), for Amazon SNS.
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

/// The headers to add to a request to `service` in `region`: `x-amz-date`,
/// `authorization` and, with temporary credentials, `x-amz-security-token`.
/// `headers` are the other headers to sign (`host` is added from `url`).
pub(crate) fn sign(
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
        .map(|(name, value)| {
            (
                name.to_ascii_lowercase(),
                value.split_whitespace().collect::<Vec<_>>().join(" "),
            )
        })
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
    let canonical_headers: String = signed
        .iter()
        .map(|(name, value)| format!("{name}:{value}\n"))
        .collect();
    let canonical = format!(
        "{method}\n{}\n{}\n{canonical_headers}\n{signed_names}\n{}",
        canonical_path(url.path()),
        canonical_query(url),
        hex::encode(Sha256::digest(body))
    );
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

/// The host, with the port when it is not the scheme's default.
fn host_header(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

/// Each path segment URI-encoded.
fn canonical_path(path: &str) -> String {
    if path.is_empty() {
        return "/".to_owned();
    }
    path.split('/')
        .map(uri_encode)
        .collect::<Vec<_>>()
        .join("/")
}

/// The query parameters, re-encoded and sorted.
fn canonical_query(url: &Url) -> String {
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(key, value)| (uri_encode(&key), uri_encode(&value)))
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// Every byte but `A-Z a-z 0-9 - _ . ~` as `%XX`.
pub(crate) fn uri_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `AWS4<secret>` through date, region, service and `aws4_request`.
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

    /// AWS's "derive a signing key" example.
    #[test]
    fn the_signing_key_matches_the_aws_example() {
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
    fn the_signature_matches_the_aws_iam_example() {
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
            Utc.with_ymd_and_hms(2015, 8, 30, 12, 36, 0).unwrap(),
        );
        let authorization = &headers
            .iter()
            .find(|(name, _)| name == "authorization")
            .unwrap()
            .1;
        assert_eq!(
            authorization,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date, \
             Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
        );
    }

    #[test]
    fn temporary_credentials_sign_the_security_token() {
        let url = Url::parse("https://sns.us-east-1.amazonaws.com/").unwrap();
        let mut credentials = credentials();
        credentials.session_token = Some("session".to_owned());
        let headers = sign(
            "POST",
            &url,
            &[],
            b"Action=Publish",
            (&credentials, "us-east-1", "sns"),
            Utc::now(),
        );
        assert!(headers.contains(&("x-amz-security-token".to_owned(), "session".to_owned())));
        assert!(
            headers
                .iter()
                .any(|(n, v)| n == "authorization" && v.contains("x-amz-security-token"))
        );
    }
}
