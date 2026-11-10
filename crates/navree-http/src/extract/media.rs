//! `Content-Type` values: the media type a body extractor checks.
//!
//! The grammar is RFC 9110's `media-type` as it is read in practice:
//! `type "/" subtype *( ";" [ " "* ] name "=" value )`, where `type`,
//! `subtype` and `name` are tokens, a `value` is a token (possibly empty)
//! or a quoted string, spaces may only start a parameter (or follow a
//! quoted value), and type and subtype compare without regard to case. A
//! value that does not read is no media type, so no body extractor accepts
//! it.

use http::HeaderMap;
use http::header::CONTENT_TYPE;

/// A parsed media type's type and subtype (as sent; compare them without
/// regard to case).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MediaType<'a> {
    pub(crate) kind: &'a str,
    pub(crate) subtype: &'a str,
}

impl<'a> MediaType<'a> {
    /// The request's media type: its one `Content-Type` header, read.
    /// Two `Content-Type` headers name none.
    pub(crate) fn of(headers: &'a HeaderMap) -> Option<Self> {
        let mut values = headers.get_all(CONTENT_TYPE).iter();
        let (Some(value), None) = (values.next(), values.next()) else {
            return None;
        };
        Self::parse(value.to_str().ok()?)
    }

    /// Reads `value` (see the [module documentation](self)).
    pub(crate) fn parse(value: &'a str) -> Option<Self> {
        let bytes = value.as_bytes();
        let slash = bytes.iter().position(|byte| !is_token(*byte))?;
        if slash == 0 || bytes[slash] != b'/' {
            return None;
        }
        let rest = &bytes[slash + 1..];
        let end = rest
            .iter()
            .position(|byte| !is_token(*byte))
            .unwrap_or(rest.len());
        match rest.get(end) {
            None => {}
            Some(b';') if end > 0 => parameters(&rest[end + 1..])?,
            Some(_) => return None,
        }
        Some(Self {
            kind: &value[..slash],
            subtype: &value[slash + 1..slash + 1 + end],
        })
    }

    /// Whether this is `kind/subtype`.
    pub(crate) fn is(&self, kind: &str, subtype: &str) -> bool {
        self.kind.eq_ignore_ascii_case(kind) && self.subtype.eq_ignore_ascii_case(subtype)
    }

    /// Whether this is JSON: an `application/` type whose subtype, split at
    /// its last `+` (one that does not start it), has `json` on either side
    /// (`json`, `merge-patch+json`, and also `json+xml`).
    pub(crate) fn is_json(&self) -> bool {
        if !self.kind.eq_ignore_ascii_case("application") {
            return false;
        }
        let (name, suffix) = match self.subtype.rfind('+') {
            Some(at) if at > 0 => (&self.subtype[..at], &self.subtype[at + 1..]),
            _ => (self.subtype, ""),
        };
        name.eq_ignore_ascii_case("json") || suffix.eq_ignore_ascii_case("json")
    }
}

/// Checks the parameters after the first `;`.
fn parameters(mut input: &[u8]) -> Option<()> {
    loop {
        // Spaces may start a parameter; an empty tail is fine.
        while let [b' ', rest @ ..] = input {
            input = rest;
        }
        if input.is_empty() {
            return Some(());
        }
        let name_end = input.iter().position(|byte| !is_token(*byte))?;
        if name_end == 0 || input[name_end] != b'=' {
            return None;
        }
        input = &input[name_end + 1..];
        if let [b'"', rest @ ..] = input {
            let close = rest.iter().position(|byte| *byte == b'"')?;
            if close == 0 || !rest[..close].iter().all(|byte| *byte > 31 && *byte != 127) {
                return None;
            }
            input = &rest[close + 1..];
            while let [b' ', rest @ ..] = input {
                input = rest;
            }
            match input {
                [] => return Some(()),
                [b';', rest @ ..] => input = rest,
                _ => return None,
            }
        } else {
            let value_end = input
                .iter()
                .position(|byte| !is_token(*byte))
                .unwrap_or(input.len());
            match input.get(value_end) {
                None => return Some(()),
                Some(b';') if value_end > 0 => input = &input[value_end + 1..],
                _ => return None,
            }
        }
    }
}

/// RFC 9110 `tchar`.
fn is_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(value: &str) -> bool {
        MediaType::parse(value).is_some_and(|media| media.is_json())
    }

    #[test]
    fn json_types_with_and_without_parameters() {
        for yes in [
            "application/json",
            "Application/JSON",
            "application/json;charset=utf-8",
            "application/json; charset=utf-8",
            "application/json;  charset=\"utf-8\"",
            "application/json;",
            "application/json; ",
            "application/json;charset=",
            "application/json; a=b; c=d",
            "application/merge-patch+json",
            "application/vnd.api+JSON",
            "application/++json",
            "application/json+",
            "application/json+xml",
        ] {
            assert!(json(yes), "{yes}");
        }
        for no in [
            "",
            "json",
            "text/json",
            "application/",
            "/json",
            "application/jsonx",
            "application/+json",
            "application/json++",
            " application/json",
            "application/json ",
            "application/json ;charset=utf-8",
            "application/json; charset",
            "application/json;=x",
            "application/json; charset=\"utf-8",
            "application/json; charset=\"\"",
            "application/json; charset=\"a\" x",
            "application/json;;",
            "application /json",
        ] {
            assert!(!json(no), "{no}");
        }
    }

    #[test]
    fn exact_types_compare_without_case() {
        let media = MediaType::parse("Application/X-WWW-Form-Urlencoded; charset=utf-8").unwrap();
        assert!(media.is("application", "x-www-form-urlencoded"));
        assert!(!media.is("application", "json"));
    }

    #[test]
    fn two_headers_name_no_type() {
        let mut headers = HeaderMap::new();
        headers.append(CONTENT_TYPE, "application/json".parse().unwrap());
        assert!(MediaType::of(&headers).is_some());
        headers.append(CONTENT_TYPE, "application/json".parse().unwrap());
        assert!(MediaType::of(&headers).is_none());
    }
}
