//! Object keys: which ones a store is asked for ([`check_key`]), how one is
//! written into a URL ([`encode_key`]), and the address of an object under
//! its bucket ([`object_url`]).
//!
//! A key is the bytes a caller gave, and those bytes come back from every
//! operation: a space, `+`, `%`, a letter of any script and a `/` at the
//! end are part of the key, encoded on the way out and never read as
//! anything else. What a key cannot be is a way out of its bucket or its
//! prefix. An HTTP address collapses `.` and `..` segments, so a key with
//! one would name another object, or with enough of them another bucket;
//! such a key is refused before anything is sent.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use url::Url;

use super::{Error, ErrorKind, Result};

/// Every byte but the unreserved characters of RFC 3986 (`A-Z a-z 0-9 - .
/// _ ~`): what AWS Signature Version 4, Google's V4 signatures and Azure
/// all take in a path and in a query.
const RESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// `key` as it is written in the path of a URL: every byte but `A-Z a-z
/// 0-9 - . _ ~` and `/` as `%XX`, in upper-case hex. A space is `%20`,
/// never `+`; `+` is `%2B`; `%` is `%25`, so a key that looks encoded
/// already is encoded again and comes back as it was.
///
/// ```
/// use navree_storage::cloud::keys::encode_key;
///
/// assert_eq!(encode_key("reports/2026/q1 final.csv"), "reports/2026/q1%20final.csv");
/// assert_eq!(encode_key("a+b/100%/₹.txt"), "a%2Bb/100%25/%E2%82%B9.txt");
/// ```
pub fn encode_key(key: &str) -> String {
    let mut encoded = String::with_capacity(key.len());
    for (index, segment) in key.split('/').enumerate() {
        if index > 0 {
            encoded.push('/');
        }
        encoded.extend(utf8_percent_encode(segment, RESERVED));
    }
    encoded
}

/// `text` as one segment of a path or one name or value of a query: like
/// [`encode_key`], and `/` is `%2F` too. It is how a key is written where
/// it must be one segment (the object name of Google's JSON API) and how
/// the queries of signed requests and signed links are written.
///
/// ```
/// use navree_storage::cloud::keys::encode_component;
///
/// assert_eq!(encode_component("reports/q1 final.csv"), "reports%2Fq1%20final.csv");
/// ```
pub fn encode_component(text: &str) -> String {
    utf8_percent_encode(text, RESERVED).to_string()
}

/// Whether `key` may be asked of a store. Refused, with
/// [`ErrorKind::Key`]:
///
/// - the empty key, which is the bucket itself;
/// - a key with a `.` or `..` segment (`../a`, `a/../b`, `a/.`), which an
///   HTTP address would collapse into another key or another bucket;
/// - a key with a control character (NUL, a line break, ...), which
///   neither a header nor an XML answer carries back unchanged.
///
/// Everything else is a key: spaces, `+`, `%`, `\`, any script, empty
/// segments (`a//b`) and a `/` at either end.
///
/// A driver checks every key it is given, whatever checked it before: a
/// disk normalises paths, but a store is public and can be handed any.
///
/// ```
/// use navree_storage::cloud::keys::check_key;
///
/// assert!(check_key("tenants/agra/q1 final (2).csv").is_ok());
/// assert!(check_key("folder/").is_ok());
/// assert!(check_key("tenants/agra/../../other-bucket/secrets").is_err());
/// assert!(check_key("").is_err());
/// ```
pub fn check_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(Error::new(ErrorKind::Key, "the key is empty"));
    }
    check_segments(key)
}

/// Whether `prefix` may be listed: the empty prefix is the whole bucket,
/// and any other is held to what [`check_key`] asks of a key.
pub fn check_prefix(prefix: &str) -> Result<()> {
    check_segments(prefix)
}

fn check_segments(key: &str) -> Result<()> {
    if key.chars().any(char::is_control) {
        return Err(Error::new(
            ErrorKind::Key,
            format!("the key {key:?} has a control character"),
        ));
    }
    if key
        .split('/')
        .any(|segment| segment == "." || segment == "..")
    {
        return Err(Error::new(
            ErrorKind::Key,
            format!("the key {key:?} has a `.` or `..` segment"),
        ));
    }
    Ok(())
}

/// The address of the object `key` under `base`, the address of its bucket
/// or container: `https://s3.ap-south-1.amazonaws.com/ludhiana-invoices`,
/// `https://ludhiana-invoices.s3.ap-south-1.amazonaws.com` or
/// `http://127.0.0.1:10000/devstoreaccount1/archive`. The key is
/// [checked](check_key) and [encoded](encode_key), and the address is
/// checked once more after it is made: it has the host of `base` and the
/// path of `base` followed by the encoded key, byte for byte, so nothing
/// in a key can name another host, another bucket or another prefix. The
/// query and the fragment of `base` are left out.
///
/// ```
/// use navree_storage::cloud::keys::object_url;
/// use navree_storage::store::Url;
///
/// let bucket = Url::parse("https://s3.ap-south-1.amazonaws.com/ludhiana-invoices").unwrap();
/// let url = object_url(&bucket, "2026/INV 1101.pdf").unwrap();
/// assert_eq!(
///     url.as_str(),
///     "https://s3.ap-south-1.amazonaws.com/ludhiana-invoices/2026/INV%201101.pdf"
/// );
/// assert!(object_url(&bucket, "../other-bucket/a").is_err());
/// // Not another host: the key starts with two empty segments.
/// let url = object_url(&bucket, "//other.example/a").unwrap();
/// assert_eq!(url.host_str(), Some("s3.ap-south-1.amazonaws.com"));
/// assert_eq!(url.path(), "/ludhiana-invoices///other.example/a");
/// ```
pub fn object_url(base: &Url, key: &str) -> Result<Url> {
    check_key(key)?;
    let outside = || {
        Error::new(
            ErrorKind::Key,
            format!("the key {key:?} does not stay under its bucket"),
        )
    };
    if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
        return Err(Error::new(
            ErrorKind::Refused,
            "a bucket's address is an http or https URL",
        ));
    }
    let path = format!("{}/{}", base.path().trim_end_matches('/'), encode_key(key));
    let url = Url::parse(&format!("{}{path}", base.origin().ascii_serialization()))
        .map_err(|_| outside())?;
    if url.origin() != base.origin()
        || url.path() != path
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(outside());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use percent_encoding::percent_decode_str;

    use super::*;

    /// Keys with what an address gives a meaning of its own.
    const AWKWARD: [&str; 16] = [
        "plain.txt",
        "reports/2026/q1 final.csv",
        "a+b=c&d.txt",
        "100%/50%25/%2e%2e/%2F",
        "पंजाब/ਲੁਧਿਆਣਾ/चालान ₹1,00,000.pdf",
        "emoji 🧾/receipt.png",
        "folder/",
        "/leading",
        "a//b",
        "//other.example/a",
        "a\\..\\b",
        "..a/b../.c./...",
        "q?x=1#frag",
        "semi;colon:at@host",
        "quote\"'<>|{}^`[]",
        "tilde~under_score-dash.dot",
    ];

    #[test]
    fn an_encoded_key_decodes_to_the_same_bytes() {
        for key in AWKWARD {
            let encoded = encode_key(key);
            assert!(
                encoded
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-._~/%".contains(&byte)),
                "{encoded}"
            );
            let decoded = percent_decode_str(&encoded).decode_utf8().unwrap();
            assert_eq!(decoded, key);
            // `/` separates the same segments before and after.
            assert_eq!(encoded.split('/').count(), key.split('/').count());

            let component = encode_component(key);
            assert!(!component.contains('/'), "{component}");
            let decoded = percent_decode_str(&component).decode_utf8().unwrap();
            assert_eq!(decoded, key);
        }
        assert_eq!(encode_key("a b+c%d"), "a%20b%2Bc%25d");
        assert_eq!(encode_key("é"), "%C3%A9");
        assert_eq!(encode_key(""), "");
        assert_eq!(encode_component("a/b c"), "a%2Fb%20c");
    }

    #[test]
    fn keys_that_climb_or_cannot_come_back_are_refused() {
        for key in [
            "",
            ".",
            "..",
            "../a",
            "a/..",
            "a/../b",
            "a/./b",
            "./a",
            "a/.",
            "tenants/agra/../../other-bucket/secrets",
            "/../a",
            "a\0b",
            "a\nb",
            "a\r\nx-injected: 1",
            "a\u{7f}b",
            "a\u{85}b",
        ] {
            let error = check_key(key).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Key, "{key:?}");
            // A control character is shown escaped.
            assert!(!error.to_string().chars().any(char::is_control), "{error}");
        }
        for key in AWKWARD {
            check_key(key).unwrap_or_else(|error| panic!("{key:?}: {error}"));
        }
    }

    #[test]
    fn a_prefix_may_be_empty_and_is_otherwise_a_key() {
        for prefix in ["", "docs", "docs/", "tenants/agra/2026"] {
            check_prefix(prefix).unwrap_or_else(|error| panic!("{prefix:?}: {error}"));
        }
        for prefix in ["..", "docs/../other", "docs/.", "a\0"] {
            assert_eq!(check_prefix(prefix).unwrap_err().kind(), ErrorKind::Key);
        }
    }

    #[test]
    fn the_address_of_an_object_is_under_its_bucket_byte_for_byte() {
        for base in [
            "https://s3.ap-south-1.amazonaws.com/ludhiana-invoices",
            "https://s3.ap-south-1.amazonaws.com/ludhiana-invoices/",
            "https://ludhiana-invoices.s3.ap-south-1.amazonaws.com",
            "http://127.0.0.1:10000/devstoreaccount1/archive",
            "http://[::1]:9000/media?x-id=ignored#top",
        ] {
            let base = Url::parse(base).unwrap();
            let under = base.path().trim_end_matches('/').to_owned();
            for key in AWKWARD {
                let url = object_url(&base, key).unwrap_or_else(|error| panic!("{key:?}: {error}"));
                assert_eq!(url.origin(), base.origin(), "{key:?}");
                assert_eq!(url.path(), format!("{under}/{}", encode_key(key)));
                assert_eq!(url.query(), None);
                assert_eq!(url.fragment(), None);
                // What the provider reads back out of the path is the key.
                let sent = url.path().strip_prefix(&format!("{under}/")).unwrap();
                assert_eq!(percent_decode_str(sent).decode_utf8().unwrap(), key);
            }
            for key in ["", "..", "../other-bucket/a", "a/../../b", "a/./b", "a\0"] {
                assert_eq!(
                    object_url(&base, key).unwrap_err().kind(),
                    ErrorKind::Key,
                    "{key:?}"
                );
            }
        }
    }

    #[test]
    fn a_bucket_address_that_is_not_http_is_refused() {
        for base in ["file:///srv/bucket", "data:text/plain,bucket"] {
            let error = object_url(&Url::parse(base).unwrap(), "a.txt").unwrap_err();
            assert_eq!(error.kind(), ErrorKind::Refused, "{base}");
        }
    }
}
