//! Shared Key header signing (`Authorization: SharedKey <account>:<sig>`)
//! for Azure Blob Storage.
//!
//! Signs the twelve standard HTTP headers (`Content-Length` left blank when
//! zero, `Date` left blank when `x-ms-date` is sent), the sorted lower-case
//! `x-ms-*` headers, and the canonicalised resource (`/<account><path>` plus
//! sorted lower-case decoded query parameters).

use std::collections::BTreeMap;
use std::fmt;
use std::time::SystemTime;

use ::http::Method;
use ::http::header::{
    CONTENT_ENCODING, CONTENT_LANGUAGE, CONTENT_LENGTH, CONTENT_TYPE, DATE, HeaderMap, HeaderName,
    HeaderValue, IF_MATCH, IF_MODIFIED_SINCE, IF_NONE_MATCH, IF_UNMODIFIED_SINCE, RANGE,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Utc};
use navree_crypto::hmac_sha256;
use percent_encoding::percent_decode_str;

use crate::cloud::Result;
use crate::cloud::http::Request;

/// The REST API version sent in `x-ms-version` when none is set on the
/// request.
pub const VERSION: &str = crate::azure_sas::VERSION;

static CONTENT_MD5: HeaderName = HeaderName::from_static("content-md5");
static MS_DATE: HeaderName = HeaderName::from_static("x-ms-date");
static MS_VERSION: HeaderName = HeaderName::from_static("x-ms-version");

/// Formats `at` as an HTTP date (`EEE, dd MMM yyyy HH:mm:ss GMT`) for
/// `x-ms-date`.
pub fn http_date(at: SystemTime) -> String {
    let dt: DateTime<Utc> = at.into();
    dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// An Azure Storage account's Shared Key signer.
#[derive(Clone, PartialEq, Eq)]
pub struct SharedKey {
    account: String,
    key: Vec<u8>,
}

/// The account name; never the key.
impl fmt::Debug for SharedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedKey")
            .field("account", &self.account)
            .finish_non_exhaustive()
    }
}

impl SharedKey {
    /// Decodes the base64 `access_key` of `account`.
    pub fn new(
        account: impl Into<String>,
        access_key: &str,
    ) -> std::result::Result<Self, base64::DecodeError> {
        Ok(Self {
            account: account.into(),
            key: STANDARD.decode(access_key.trim())?,
        })
    }

    /// The storage account name.
    pub fn account(&self) -> &str {
        &self.account
    }

    /// The decoded key bytes, for building a [`SharedKeySas`](crate::azure_sas::SharedKeySas).
    pub(crate) fn key_bytes(&self) -> &[u8] {
        &self.key
    }

    /// Adds `x-ms-date`, `x-ms-version`, `Content-Length` (for write methods
    /// or non-empty bodies, matching [`Client::send`](crate::cloud::http::Client::send))
    /// and a sensitive `Authorization: SharedKey <account>:<signature>`
    /// header to `request`.
    pub fn sign(&self, mut request: Request, now: SystemTime) -> Result<Request> {
        if !request.headers.contains_key(&MS_DATE) {
            request = request.header("x-ms-date", &http_date(now))?;
        }
        if !request.headers.contains_key(&MS_VERSION) {
            request
                .headers
                .insert(MS_VERSION.clone(), HeaderValue::from_static(VERSION));
        }
        let writes = matches!(request.method, Method::PUT | Method::POST | Method::PATCH);
        if (writes || !request.body.is_empty()) && !request.headers.contains_key(CONTENT_LENGTH) {
            request.headers.insert(
                CONTENT_LENGTH,
                HeaderValue::from(request.body.len() as u64),
            );
        }
        let header_value = self.authorization(&request);
        request.secret("authorization", &header_value)
    }

    /// The `SharedKey <account>:<signature>` value for `request`.
    pub fn authorization(&self, request: &Request) -> String {
        format!("SharedKey {}:{}", self.account, self.signature(request))
    }

    /// The base64 HMAC-SHA-256 signature of [`string_to_sign`](Self::string_to_sign).
    pub fn signature(&self, request: &Request) -> String {
        let to_sign = self.string_to_sign(request);
        STANDARD.encode(hmac_sha256(&self.key, to_sign.as_bytes()))
    }

    /// The canonical `StringToSign` for Azure Shared Key authorization
    /// (service versions 2015-02-21 and later).
    pub fn string_to_sign(&self, request: &Request) -> String {
        let headers = &request.headers;
        let content_length = match header_str(headers, &CONTENT_LENGTH) {
            Some("0") => "",
            Some(length) => length,
            None if !request.body.is_empty() => {
                return self.string_to_sign_with_length(request, &request.body.len().to_string());
            }
            None => "",
        };
        self.string_to_sign_with_length(request, content_length)
    }

    fn string_to_sign_with_length(&self, request: &Request, content_length: &str) -> String {
        let headers = &request.headers;
        // When `x-ms-date` is present, Azure specifies an empty `Date` slot
        // and signs `x-ms-date` in `CanonicalizedHeaders`.
        let date = if headers.contains_key(&MS_DATE) {
            ""
        } else {
            header_str(headers, &DATE).unwrap_or_default()
        };

        let mut out = String::with_capacity(256);
        out.push_str(request.method.as_str());
        out.push('\n');
        for value in [
            header_str(headers, &CONTENT_ENCODING).unwrap_or_default(),
            header_str(headers, &CONTENT_LANGUAGE).unwrap_or_default(),
            content_length,
            header_str(headers, &CONTENT_MD5).unwrap_or_default(),
            header_str(headers, &CONTENT_TYPE).unwrap_or_default(),
            date,
            header_str(headers, &IF_MODIFIED_SINCE).unwrap_or_default(),
            header_str(headers, &IF_MATCH).unwrap_or_default(),
            header_str(headers, &IF_NONE_MATCH).unwrap_or_default(),
            header_str(headers, &IF_UNMODIFIED_SINCE).unwrap_or_default(),
            header_str(headers, &RANGE).unwrap_or_default(),
        ] {
            out.push_str(value);
            out.push('\n');
        }
        canonicalize_headers(headers, &mut out);
        canonicalize_resource(&self.account, &request.url, &mut out);
        out
    }
}

fn header_str<'a>(headers: &'a HeaderMap, name: &HeaderName) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

/// Appends sorted `x-ms-*` headers as `name:unfolded_value\n`.
fn canonicalize_headers(headers: &HeaderMap, out: &mut String) {
    let mut names: Vec<&HeaderName> = headers
        .keys()
        .filter(|name| name.as_str().starts_with("x-ms-"))
        .collect();
    names.sort_unstable_by(|a, b| a.as_str().cmp(b.as_str()));

    for name in names {
        out.push_str(name.as_str());
        out.push(':');
        let mut first_value = true;
        for value in headers.get_all(name) {
            let Ok(text) = value.to_str() else {
                continue;
            };
            if !first_value {
                out.push(',');
            }
            first_value = false;
            push_unfolded(text, out);
        }
        out.push('\n');
    }
}

/// Trims outer ASCII white space and collapses linear white-space breaks into
/// single spaces, as Azure's `CanonicalizedHeaders` rule specifies.
fn push_unfolded(text: &str, out: &mut String) {
    let mut in_space = false;
    for ch in text.trim().chars() {
        if matches!(ch, ' ' | '\t' | '\r' | '\n') {
            in_space = true;
        } else {
            if in_space {
                out.push(' ');
                in_space = false;
            }
            out.push(ch);
        }
    }
}

/// Appends `/<account><encoded_path>` and sorted lower-case decoded query
/// parameters (`\nname:value1,value2`).
fn canonicalize_resource(account: &str, url: &url::Url, out: &mut String) {
    out.push('/');
    out.push_str(account);
    out.push_str(url.path());

    let Some(query) = url.query() else {
        return;
    };
    let mut params: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (raw_name, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        let name = percent_decode_str(raw_name)
            .decode_utf8_lossy()
            .to_lowercase();
        let value = percent_decode_str(raw_value)
            .decode_utf8_lossy()
            .into_owned();
        params.entry(name).or_default().push(value);
    }

    for (name, mut values) in params {
        values.sort_unstable();
        out.push('\n');
        out.push_str(&name);
        out.push(':');
        for (index, value) in values.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(value);
        }
    }
}
