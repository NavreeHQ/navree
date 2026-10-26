//! AWS Signature Version 4, written with `hmac` and `sha2`, for Bedrock.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

/// AWS credentials.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Credentials {
    pub(crate) access_key_id: String,
    pub(crate) secret_access_key: String,
    pub(crate) session_token: Option<String>,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("access_key_id", &"[redacted]")
            .finish_non_exhaustive()
    }
}

/// What a request signs: its method, path (as sent, percent-encoded once),
/// query pairs (raw), headers (names lower-case) and body.
pub(crate) struct Signable<'a> {
    pub(crate) method: &'a str,
    pub(crate) path: &'a str,
    pub(crate) query: &'a [(&'a str, &'a str)],
    pub(crate) headers: &'a [(&'a str, &'a str)],
    pub(crate) body: &'a [u8],
}

/// The `Authorization` header value for `request`, signed at `now` for
/// `service` in `region`.
pub(crate) fn authorization(
    request: &Signable<'_>,
    credentials: &Credentials,
    region: &str,
    service: &str,
    now: DateTime<Utc>,
) -> String {
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let canonical = canonical_request(request);
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    );
    let key = signing_key(&credentials.secret_access_key, &date, region, service);
    let signature = hex::encode(hmac(&key, string_to_sign.as_bytes()));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={}, Signature={signature}",
        credentials.access_key_id,
        signed_headers(request.headers)
    )
}

/// The canonical request (the AWS documentation's six lines).
pub(crate) fn canonical_request(request: &Signable<'_>) -> String {
    let path = if request.path.is_empty() {
        "/".to_owned()
    } else {
        // Every service but S3 encodes each segment of the (already encoded)
        // path a second time.
        request
            .path
            .split('/')
            .map(uri_encode)
            .collect::<Vec<_>>()
            .join("/")
    };
    let mut query: Vec<(String, String)> = request
        .query
        .iter()
        .map(|(k, v)| (uri_encode(k), uri_encode(v)))
        .collect();
    query.sort();
    let query = query
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let mut headers: Vec<(String, String)> = request
        .headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), collapse(v)))
        .collect();
    headers.sort();
    let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    format!(
        "{}\n{path}\n{query}\n{canonical_headers}\n{}\n{}",
        request.method,
        signed_headers(request.headers),
        hex::encode(Sha256::digest(request.body))
    )
}

/// `content-type;host;x-amz-date`.
fn signed_headers(headers: &[(&str, &str)]) -> String {
    let mut names: Vec<String> = headers
        .iter()
        .map(|(k, _)| k.to_ascii_lowercase())
        .collect();
    names.sort();
    names.join(";")
}

/// Trims and collapses inner whitespace.
fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// RFC 3986 encoding: everything but `A-Z a-z 0-9 - _ . ~`.
pub(crate) fn uri_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes any key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// The derived signing key.
pub(crate) fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, service.as_bytes());
    hmac(&k_service, b"aws4_request")
}

/// The credentials from the explicit keys, else `AWS_ACCESS_KEY_ID` and
/// `AWS_SECRET_ACCESS_KEY` (and `AWS_SESSION_TOKEN`), else the shared
/// credentials file's `profile` (default `AWS_PROFILE`, else `default`).
/// `env` reads an environment variable and `read` a file.
pub(crate) fn resolve_credentials(
    explicit: (Option<&str>, Option<&str>, Option<&str>),
    profile: Option<&str>,
    env: impl Fn(&str) -> Option<String>,
    read: impl Fn(&Path) -> Option<String>,
) -> Option<Credentials> {
    let non_empty = |value: Option<String>| value.filter(|v| !v.is_empty());
    if let (Some(id), Some(secret)) = (explicit.0, explicit.1)
        && !id.is_empty()
        && !secret.is_empty()
    {
        return Some(Credentials {
            access_key_id: id.to_owned(),
            secret_access_key: secret.to_owned(),
            session_token: explicit.2.map(str::to_owned),
        });
    }
    if let (Some(id), Some(secret)) = (
        non_empty(env("AWS_ACCESS_KEY_ID")),
        non_empty(env("AWS_SECRET_ACCESS_KEY")),
    ) {
        return Some(Credentials {
            access_key_id: id,
            secret_access_key: secret,
            session_token: non_empty(env("AWS_SESSION_TOKEN")),
        });
    }
    let path = match non_empty(env("AWS_SHARED_CREDENTIALS_FILE")) {
        Some(path) => PathBuf::from(path),
        None => {
            let home = non_empty(env("HOME")).or_else(|| non_empty(env("USERPROFILE")))?;
            Path::new(&home).join(".aws").join("credentials")
        }
    };
    let profile = profile
        .map(str::to_owned)
        .or_else(|| non_empty(env("AWS_PROFILE")))
        .unwrap_or_else(|| "default".to_owned());
    parse_profile(&read(&path)?, &profile)
}

/// The keys of `[profile]` in an INI credentials file.
fn parse_profile(file: &str, profile: &str) -> Option<Credentials> {
    let mut current = None;
    let (mut id, mut secret, mut token) = (None, None, None);
    for line in file.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = Some(section.trim().trim_start_matches("profile ").to_owned());
            continue;
        }
        if current.as_deref() != Some(profile) {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim().to_owned();
            match key.trim() {
                "aws_access_key_id" => id = Some(value),
                "aws_secret_access_key" => secret = Some(value),
                "aws_session_token" => token = Some(value),
                _ => {}
            }
        }
    }
    Some(Credentials {
        access_key_id: id?,
        secret_access_key: secret?,
        session_token: token,
    })
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    /// The AWS Signature Version 4 documentation example (IAM
    /// `ListUsers`, 2015-08-30).
    #[test]
    fn signs_the_aws_documentation_example() {
        let credentials = Credentials {
            access_key_id: "AKIDEXAMPLE".into(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            session_token: None,
        };
        let request = Signable {
            method: "GET",
            path: "/",
            query: &[("Action", "ListUsers"), ("Version", "2010-05-08")],
            headers: &[
                (
                    "Content-Type",
                    "application/x-www-form-urlencoded; charset=utf-8",
                ),
                ("Host", "iam.amazonaws.com"),
                ("X-Amz-Date", "20150830T123600Z"),
            ],
            body: b"",
        };
        let canonical = canonical_request(&request);
        assert_eq!(
            hex::encode(Sha256::digest(canonical.as_bytes())),
            "f536975d06c0309214f805bb90ccff089219ecd68b2577efef23edd43b7e1a59"
        );
        assert_eq!(
            hex::encode(signing_key(
                &credentials.secret_access_key,
                "20150830",
                "us-east-1",
                "iam"
            )),
            "c4afb1cc5771d871763a393e44b703571b55cc28424d1a5e86da6ed3c154a4b9"
        );
        let now = Utc.with_ymd_and_hms(2015, 8, 30, 12, 36, 0).unwrap();
        assert_eq!(
            authorization(&request, &credentials, "us-east-1", "iam", now),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date, \
             Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
        );
    }

    #[test]
    fn paths_are_encoded_twice() {
        let request = Signable {
            method: "POST",
            path: "/model/us.anthropic.claude-opus-5-5%3A0/converse",
            query: &[],
            headers: &[("host", "bedrock-runtime.us-east-1.amazonaws.com")],
            body: b"{}",
        };
        assert!(
            canonical_request(&request)
                .contains("/model/us.anthropic.claude-opus-5-5%253A0/converse\n")
        );
        assert_eq!(uri_encode("a:b c"), "a%3Ab%20c");
    }

    #[test]
    fn credentials_resolve_in_order() {
        let none = |_: &str| None::<String>;
        let no_file = |_: &Path| None::<String>;
        let explicit =
            resolve_credentials((Some("A"), Some("S"), None), None, none, no_file).unwrap();
        assert_eq!(explicit.access_key_id, "A");

        let env = |name: &str| match name {
            "AWS_ACCESS_KEY_ID" => Some("ENVID".to_owned()),
            "AWS_SECRET_ACCESS_KEY" => Some("ENVSECRET".to_owned()),
            "AWS_SESSION_TOKEN" => Some("TOKEN".to_owned()),
            _ => None,
        };
        let from_env = resolve_credentials((None, None, None), None, env, no_file).unwrap();
        assert_eq!(from_env.session_token.as_deref(), Some("TOKEN"));

        let home = |name: &str| (name == "HOME").then(|| "/home/me".to_owned());
        let file = |path: &Path| {
            assert!(path.ends_with(Path::new(".aws").join("credentials")));
            Some(
                "[default]\naws_access_key_id = D\naws_secret_access_key = DS\n\n\
                 [profile work]\naws_access_key_id=W\naws_secret_access_key=WS\n"
                    .to_owned(),
            )
        };
        let default = resolve_credentials((None, None, None), None, home, file).unwrap();
        assert_eq!(default.access_key_id, "D");
        let work = resolve_credentials((None, None, None), Some("work"), home, file).unwrap();
        assert_eq!(work.secret_access_key, "WS");
        assert!(resolve_credentials((None, None, None), Some("ghost"), home, file).is_none());
        assert!(resolve_credentials((None, None, None), None, none, no_file).is_none());
        assert!(!format!("{default:?}").contains('D'));
    }
}
