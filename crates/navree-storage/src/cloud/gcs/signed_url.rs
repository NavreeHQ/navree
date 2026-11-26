//! Google Cloud Storage V4 Signed URLs (`GOOG4-RSA-SHA256`).

use std::time::{Duration, SystemTime};

use ::http::Method;
use ::http::header::{HeaderName, HeaderValue};
use navree_crypto::sha256_hex;
use url::Url;

use crate::cloud::keys::{encode_component, object_url};
use crate::cloud::{Error, ErrorKind, Result};

/// The V4 signing algorithm identifier for RSA-SHA256 signatures.
pub const ALGORITHM: &str = "GOOG4-RSA-SHA256";

/// The longest validity Google Cloud Storage accepts on a V4 signed URL
/// (seven days, in seconds).
pub const MAX_EXPIRES_SECS: u64 = 7 * 24 * 60 * 60;

/// What Google Cloud Storage hashes in place of the request body on V4 signed
/// URLs.
pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";

/// The canonical components of a V4 signed URL prior to attaching the hex
/// RSA-SHA256 signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedSignedUrl {
    /// The six-line canonical request hashed into [`Self::string_to_sign`].
    pub canonical_request: String,
    /// The four-line string signed with `RSASSA-PKCS1-v1_5` SHA-256 or IAM
    /// `signBlob`.
    pub string_to_sign: String,
    /// The sorted, semicolon-separated lowercase header names bound to the URL.
    pub signed_headers: String,
    url_without_signature: String,
}

impl PreparedSignedUrl {
    /// Attaches `signature` (raw RSA-SHA256 signature bytes, hex-encoded in
    /// lowercase) as `X-Goog-Signature` and returns the completed [`Url`].
    pub fn finish(&self, signature: &[u8]) -> Result<Url> {
        if signature.is_empty() {
            return Err(Error::new(
                ErrorKind::Refused,
                "a Google Cloud Storage signed link signature cannot be empty",
            ));
        }
        let signed = format!(
            "{}&X-Goog-Signature={}",
            self.url_without_signature,
            hex::encode(signature),
        );
        Url::parse(&signed)
            .map_err(|_| Error::new(ErrorKind::Other, "could not assemble the signed URL"))
    }
}

/// Builds the V4 canonical request and string-to-sign for `method` (`GET` or
/// `PUT`) on `key` under `bucket_url`, expiring after `expires_in` from `now`
/// and binding every header in `headers`.
pub fn prepare(
    bucket_url: &Url,
    key: &str,
    method: &Method,
    expires_in: Duration,
    headers: &[(HeaderName, HeaderValue)],
    client_email: &str,
    now: SystemTime,
) -> Result<PreparedSignedUrl> {
    if *method != Method::GET && *method != Method::PUT {
        return Err(Error::new(
            ErrorKind::Refused,
            format!("Google Cloud Storage signs GET and PUT links, not {method}"),
        ));
    }
    let expires_secs = expires_in.as_secs();
    if expires_in.is_zero()
        || expires_secs == 0
        || expires_in > Duration::from_secs(MAX_EXPIRES_SECS)
    {
        return Err(Error::new(
            ErrorKind::Refused,
            "a Google Cloud Storage signed link must expire between 1 second and 7 days",
        ));
    }
    if client_email.trim().is_empty() {
        return Err(Error::new(
            ErrorKind::Credential,
            "a Google Cloud Storage signed link needs a service account email",
        ));
    }

    let resource_url = object_url(bucket_url, key)?;
    let host = host_header(&resource_url);

    let mut merged: Vec<(String, String)> = Vec::with_capacity(headers.len() + 1);
    merged.push(("host".to_owned(), host));
    for (name, value) in headers {
        let lower = name.as_str().to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "host"
                | "authorization"
                | "content-length"
                | "user-agent"
                | "x-goog-algorithm"
                | "x-goog-credential"
                | "x-goog-date"
                | "x-goog-expires"
                | "x-goog-signature"
                | "x-goog-signedheaders"
                | "x-goog-oauth-source"
        ) {
            return Err(Error::new(
                ErrorKind::Refused,
                format!("Google Cloud Storage cannot bind the `{name}` header on a signed link"),
            ));
        }
        let raw_value = value.to_str().map_err(|_| {
            Error::new(
                ErrorKind::Refused,
                format!("the value of the `{name}` header is not visible ASCII"),
            )
        })?;
        if raw_value.chars().any(char::is_control) {
            return Err(Error::new(
                ErrorKind::Refused,
                format!("the value of the `{name}` header has a control character"),
            ));
        }
        let collapsed = collapse_whitespace(raw_value);
        match merged.iter_mut().find(|(existing, _)| *existing == lower) {
            Some((_, joined)) => {
                joined.push(',');
                joined.push_str(&collapsed);
            }
            None => merged.push((lower, collapsed)),
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

    let (datestamp, timestamp) = format_utc_timestamp(now)?;
    let scope = format!("{datestamp}/auto/storage/goog4_request");
    let credential = format!("{client_email}/{scope}");
    let expires_text = expires_secs.to_string();

    let mut query_pairs = [
        (
            encode_component("X-Goog-Algorithm"),
            encode_component(ALGORITHM),
        ),
        (
            encode_component("X-Goog-Credential"),
            encode_component(&credential),
        ),
        (
            encode_component("X-Goog-Date"),
            encode_component(&timestamp),
        ),
        (
            encode_component("X-Goog-Expires"),
            encode_component(&expires_text),
        ),
        (
            encode_component("X-Goog-SignedHeaders"),
            encode_component(&signed_headers),
        ),
    ];
    query_pairs.sort();
    let canonical_query = query_pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_path = resource_url.path();
    let canonical_request = format!(
        "{method}\n{canonical_path}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{UNSIGNED_PAYLOAD}"
    );
    let canonical_hash = sha256_hex(canonical_request.as_bytes());
    let string_to_sign = format!("{ALGORITHM}\n{timestamp}\n{scope}\n{canonical_hash}");
    let url_without_signature = format!(
        "{}{canonical_path}?{canonical_query}",
        resource_url.origin().ascii_serialization()
    );

    Ok(PreparedSignedUrl {
        canonical_request,
        string_to_sign,
        signed_headers,
        url_without_signature,
    })
}

/// The `Host` header sent for `url`: includes `:port` only when non-default
/// for the scheme.
fn host_header(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
}

/// Trims outer whitespace and collapses inner runs of whitespace to one space.
fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Formats `now` in UTC as `(YYYYMMDD, YYYYMMDDTHHMMSSZ)`.
fn format_utc_timestamp(now: SystemTime) -> Result<(String, String)> {
    let secs = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| Error::new(ErrorKind::Other, "the clock is set before 1970"))?
        .as_secs();
    let days = i64::try_from(secs / 86_400)
        .map_err(|_| Error::new(ErrorKind::Other, "the clock is out of range"))?;
    let rem = secs % 86_400;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;

    // Howard Hinnant's civil-from-days algorithm (days since 1970-01-01).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    if !(1970..=9999).contains(&year) {
        return Err(Error::new(
            ErrorKind::Other,
            "the clock year is outside the four-digit V4 range",
        ));
    }

    let datestamp = format!("{year:04}{month:02}{day:02}");
    let timestamp = format!("{datestamp}T{hour:02}{minute:02}{second:02}Z");
    Ok((datestamp, timestamp))
}
