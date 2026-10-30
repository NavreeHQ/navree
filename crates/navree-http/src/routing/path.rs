//! Request paths: the checks every path passes before it is matched, the
//! percent-decoding of parameter values, and [`PathParams`].
//!
//! The router never normalizes a path: it matches the raw segments, so what
//! a proxy in front checked is what the app routes. A path it cannot route
//! unambiguously is refused with 400 instead:
//!
//! - a `.` or `..` segment, also percent-encoded (`%2e`, `.%2E`, ...);
//! - a backslash, which some clients and servers read as `/`.
//!
//! Parameter values are percent-decoded once matched, strictly: the bytes
//! must be UTF-8, with no NUL and no backslash, and a `{name}` value may not
//! hold a slash (`%2F`, which would make one segment two once decoded); a
//! `{*name}` value may, but none of its `/`-separated parts may be a dot
//! segment. `+` stays `+` (it means a space only in query strings and forms),
//! and a `%` not followed by two hex digits stays as it is.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

/// Why a request path is refused before matching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathProblem {
    /// A `.` or `..` segment, raw or percent-encoded.
    DotSegment,
    /// A backslash.
    Backslash,
}

impl fmt::Display for PathProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PathProblem::DotSegment => "Invalid URL: the path has a `.` or `..` segment",
            PathProblem::Backslash => "Invalid URL: the path has a backslash",
        })
    }
}

/// Checks a request path before matching: refuses dot segments (raw or
/// percent-encoded) and backslashes. Paths that do not start with `/` pass
/// (they match nothing).
///
/// ```
/// use navree_http::routing::{PathProblem, check_path};
///
/// assert_eq!(check_path("/files/a.txt"), Ok(()));
/// assert_eq!(check_path("/files/../etc"), Err(PathProblem::DotSegment));
/// assert_eq!(check_path("/files/%2E%2e/etc"), Err(PathProblem::DotSegment));
/// assert_eq!(check_path("/files\\a"), Err(PathProblem::Backslash));
/// ```
pub fn check_path(path: &str) -> Result<(), PathProblem> {
    let bytes = path.as_bytes();
    if bytes.contains(&b'\\') {
        return Err(PathProblem::Backslash);
    }
    // Dot segments: only a segment of at most six bytes can be one
    // (`%2e%2e`), and every one of them starts with `.` or `%`.
    for segment in path.split('/') {
        if is_dot_segment_raw(segment) {
            return Err(PathProblem::DotSegment);
        }
    }
    Ok(())
}

/// Whether a raw segment is `.` or `..`, each dot possibly `%2e`/`%2E`.
fn is_dot_segment_raw(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    if bytes.is_empty() || bytes.len() > 6 || !matches!(bytes[0], b'.' | b'%') {
        return false;
    }
    let mut dots = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            i += 1;
        } else if bytes[i] == b'%'
            && bytes.get(i + 1) == Some(&b'2')
            && matches!(bytes.get(i + 2), Some(b'e' | b'E'))
        {
            i += 3;
        } else {
            return false;
        }
        dots += 1;
    }
    dots <= 2
}

/// Whether a decoded value is `.` or `..`.
fn is_dot(value: &str) -> bool {
    value == "." || value == ".."
}

/// Why a parameter value is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParamProblem {
    /// Not UTF-8 once percent-decoded.
    InvalidUtf8,
    /// A `{name}` value holding a slash once decoded (`%2F`).
    EncodedSlash,
    /// A backslash once decoded (`%5C`).
    Backslash,
    /// A NUL once decoded (`%00`).
    Nul,
    /// A `.` or `..` value (or, in a catch-all, part) once decoded.
    DotSegment,
}

impl ParamProblem {
    /// The client error's message for parameter `name`.
    pub fn message(self, name: &str) -> String {
        match self {
            // The wording of the previous router's `Path` rejection, kept.
            ParamProblem::InvalidUtf8 => format!("Invalid URL: Invalid UTF-8 in `{name}`"),
            ParamProblem::EncodedSlash => {
                format!("Invalid URL: `{name}` holds an encoded slash")
            }
            ParamProblem::Backslash => format!("Invalid URL: `{name}` holds a backslash"),
            ParamProblem::Nul => format!("Invalid URL: `{name}` holds a NUL character"),
            ParamProblem::DotSegment => {
                format!("Invalid URL: `{name}` is or holds a `.` or `..` segment")
            }
        }
    }
}

/// Percent-decodes a matched parameter value with the checks of the
/// [module documentation](self); `catch_all` for `{*name}` values.
///
/// ```
/// use navree_http::routing::{ParamProblem, decode_param};
///
/// assert_eq!(decode_param("caf%C3%A9", false).unwrap(), "café");
/// assert_eq!(decode_param("a+b", false).unwrap(), "a+b");
/// assert_eq!(decode_param("100%", false).unwrap(), "100%");
/// assert_eq!(decode_param("a%2Fb", false), Err(ParamProblem::EncodedSlash));
/// assert_eq!(decode_param("a%2Fb", true).unwrap(), "a/b");
/// assert_eq!(decode_param("a/%2e%2e/b", true), Err(ParamProblem::DotSegment));
/// assert_eq!(decode_param("%FF", false), Err(ParamProblem::InvalidUtf8));
/// ```
pub fn decode_param(raw: &str, catch_all: bool) -> Result<Cow<'_, str>, ParamProblem> {
    let decoded = percent_decode(raw.as_bytes());
    let value = match decoded {
        Cow::Borrowed(_) => Cow::Borrowed(raw),
        Cow::Owned(bytes) => {
            Cow::Owned(String::from_utf8(bytes).map_err(|_| ParamProblem::InvalidUtf8)?)
        }
    };
    let bytes = value.as_bytes();
    if bytes.contains(&0) {
        return Err(ParamProblem::Nul);
    }
    if bytes.contains(&b'\\') {
        return Err(ParamProblem::Backslash);
    }
    if catch_all {
        if value.split('/').any(is_dot) {
            return Err(ParamProblem::DotSegment);
        }
    } else {
        if bytes.contains(&b'/') {
            return Err(ParamProblem::EncodedSlash);
        }
        if is_dot(&value) {
            return Err(ParamProblem::DotSegment);
        }
    }
    Ok(value)
}

/// Percent-decodes `input`: each `%` followed by two hex digits becomes its
/// byte; any other `%` stays. Borrows when there is nothing to decode.
pub fn percent_decode(input: &[u8]) -> Cow<'_, [u8]> {
    let Some(first) = find_escape(input, 0) else {
        return Cow::Borrowed(input);
    };
    let mut out = Vec::with_capacity(input.len());
    out.extend_from_slice(&input[..first]);
    let mut i = first;
    while i < input.len() {
        match escaped_byte(input, i) {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(input[i]);
                i += 1;
            }
        }
    }
    Cow::Owned(out)
}

/// The position of the first `%XX` escape at or after `from`.
fn find_escape(input: &[u8], from: usize) -> Option<usize> {
    (from..input.len()).find(|&i| escaped_byte(input, i).is_some())
}

/// The byte of the `%XX` escape at `i`, if there is one.
fn escaped_byte(input: &[u8], i: usize) -> Option<u8> {
    if input[i] != b'%' {
        return None;
    }
    let high = hex(*input.get(i + 1)?)?;
    let low = hex(*input.get(i + 2)?)?;
    Some(high << 4 | low)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// The matched route's path parameters, decoded: in the request extensions
/// of every request a route with parameters matched (a route without
/// parameters leaves the extensions as they are). Guards and authorizers
/// read them here (`can(.., from = "id")`); handlers usually take
/// [`Path`](crate::Path).
#[derive(Clone)]
pub struct PathParams {
    names: Arc<[Box<str>]>,
    /// The decoded values, one after the other.
    text: String,
    /// Where each value ends in `text`.
    ends: [u32; super::MAX_PARAMS],
    len: u8,
}

impl PathParams {
    /// Decodes the raw values matched for `names` (in order).
    pub(crate) fn decode(
        names: Arc<[Box<str>]>,
        raw: &[&str],
        catch_all_last: bool,
    ) -> Result<Self, (String, ParamProblem)> {
        let mut text = String::with_capacity(raw.iter().map(|value| value.len()).sum());
        let mut ends = [0; super::MAX_PARAMS];
        for (index, value) in raw.iter().enumerate() {
            let catch_all = catch_all_last && index + 1 == raw.len();
            match decode_param(value, catch_all) {
                Ok(decoded) => text.push_str(&decoded),
                Err(problem) => return Err((names[index].to_string(), problem)),
            }
            // A request path is at most 64 KiB long (`http`'s limit), so
            // its parameters fit.
            ends[index] = u32::try_from(text.len()).unwrap_or(u32::MAX);
        }
        Ok(Self {
            len: u8::try_from(raw.len()).unwrap_or(u8::MAX),
            names,
            text,
            ends,
        })
    }

    /// No parameters.
    pub fn empty() -> Self {
        Self {
            names: Arc::from(Vec::new()),
            text: String::new(),
            ends: [0; super::MAX_PARAMS],
            len: 0,
        }
    }

    /// Parameters set by hand for a request whose parameters are not in its
    /// path, such as the record id of a gRPC call or of a message:
    /// `(name, value)` pairs in order, values as they are (nothing is
    /// decoded or checked).
    ///
    /// # Panics
    ///
    /// With more than [`MAX_PARAMS`](super::MAX_PARAMS) pairs, or values
    /// longer than 4 GiB in all.
    pub fn from_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut names = Vec::new();
        let mut text = String::new();
        let mut ends = [0; super::MAX_PARAMS];
        for (index, (name, value)) in pairs.into_iter().enumerate() {
            assert!(
                index < super::MAX_PARAMS,
                "a request has at most {} path parameters",
                super::MAX_PARAMS
            );
            names.push(Box::from(name));
            text.push_str(value);
            ends[index] = u32::try_from(text.len()).expect("path parameters fit in 4 GiB");
        }
        Self {
            len: u8::try_from(names.len()).unwrap_or(u8::MAX),
            names: Arc::from(names),
            text,
            ends,
        }
    }

    /// The value of parameter `name`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.iter()
            .find(|(param, _)| *param == name)
            .map(|(_, value)| value)
    }

    /// `(name, value)` pairs in route order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &str)> + '_ {
        (0..usize::from(self.len)).map(move |index| (&*self.names[index], self.value(index)))
    }

    /// The value at `index`.
    fn value(&self, index: usize) -> &str {
        let start = if index == 0 {
            0
        } else {
            self.ends[index - 1] as usize
        };
        &self.text[start..self.ends[index] as usize]
    }

    /// How many parameters there are.
    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl fmt::Debug for PathParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

/// The matched route's parameters (none outside a matched route).
impl crate::extract::FromRequestParts for PathParams {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        Ok(parts
            .extensions
            .get::<PathParams>()
            .cloned()
            .unwrap_or_else(PathParams::empty))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_segments_are_refused_raw_and_encoded() {
        for path in [
            "/.",
            "/..",
            "/a/.",
            "/a/..",
            "/a/./b",
            "/a/../b",
            "/%2e",
            "/%2E",
            "/%2e%2e",
            "/%2E%2e/x",
            "/.%2e",
            "/%2e.",
            "/a/%2e/b",
        ] {
            assert_eq!(check_path(path), Err(PathProblem::DotSegment), "{path}");
        }
        for path in [
            "/",
            "/a",
            "/.well-known/x",
            "/...",
            "/.a",
            "/a.",
            "/%2e%2e%2e",
            "/%2ex",
            "/%2f..",
            "/%252e",
            "/%2",
            "/%",
            "/a%2E",
            "/x/%2E%2E%2E",
        ] {
            assert_eq!(check_path(path), Ok(()), "{path}");
        }
    }

    #[test]
    fn backslashes_are_refused() {
        assert_eq!(check_path("/a\\b"), Err(PathProblem::Backslash));
        assert_eq!(check_path("\\"), Err(PathProblem::Backslash));
        assert_eq!(
            check_path("/a%5Cb"),
            Ok(()),
            "encoded: checked per parameter"
        );
    }

    #[test]
    fn percent_decoding_keeps_lone_percent_signs_and_plus() {
        assert_eq!(&*percent_decode(b"abc"), b"abc");
        assert!(matches!(percent_decode(b"abc"), Cow::Borrowed(_)));
        assert_eq!(&*percent_decode(b"a%20b"), b"a b");
        assert_eq!(&*percent_decode(b"%41%4a%4A"), b"AJJ");
        assert_eq!(&*percent_decode(b"100%"), b"100%");
        assert_eq!(&*percent_decode(b"%zz%4"), b"%zz%4");
        assert_eq!(&*percent_decode(b"%%41"), b"%A");
        assert_eq!(&*percent_decode(b"a+b"), b"a+b");
        assert_eq!(&*percent_decode(b"%2541"), b"%41", "decoded once");
        assert_eq!(&*percent_decode(b"%ff%FE"), [0xff, 0xfe]);
    }

    #[test]
    fn parameters_decode_strictly() {
        assert_eq!(decode_param("42", false).unwrap(), "42");
        assert_eq!(decode_param("caf%C3%A9", false).unwrap(), "café");
        assert_eq!(decode_param("josé", false).unwrap(), "josé");
        assert_eq!(decode_param("a%20b", false).unwrap(), "a b");
        assert_eq!(decode_param("...", false).unwrap(), "...");
        assert_eq!(decode_param("%FF", false), Err(ParamProblem::InvalidUtf8));
        assert_eq!(decode_param("%C3", true), Err(ParamProblem::InvalidUtf8));
        assert_eq!(
            decode_param("a%2fb", false),
            Err(ParamProblem::EncodedSlash)
        );
        assert_eq!(decode_param("a%5Cb", false), Err(ParamProblem::Backslash));
        assert_eq!(decode_param("a%5cb", true), Err(ParamProblem::Backslash));
        assert_eq!(decode_param("a%00b", false), Err(ParamProblem::Nul));
        assert_eq!(decode_param("a%00", true), Err(ParamProblem::Nul));
        assert_eq!(decode_param("%2e", false), Err(ParamProblem::DotSegment));
        assert_eq!(decode_param("%2E%2E", false), Err(ParamProblem::DotSegment));
        assert_eq!(decode_param("a/b/c.txt", true).unwrap(), "a/b/c.txt");
        assert_eq!(decode_param("a%2Fb", true).unwrap(), "a/b");
        assert_eq!(decode_param("a//b/", true).unwrap(), "a//b/");
        assert_eq!(decode_param("a/%2E/b", true), Err(ParamProblem::DotSegment));
        assert_eq!(decode_param("..%2Fb", true), Err(ParamProblem::DotSegment));
        assert_eq!(decode_param("a%2F..", true), Err(ParamProblem::DotSegment));
    }

    #[test]
    fn path_params_hold_decoded_values_by_name() {
        let names: Arc<[Box<str>]> = vec!["id".into(), "path".into()].into();
        let params = PathParams::decode(names.clone(), &["4%32", "a%2Fb/c"], true).unwrap();
        assert_eq!(params.len(), 2);
        assert_eq!(params.get("id"), Some("42"));
        assert_eq!(params.get("path"), Some("a/b/c"));
        assert_eq!(params.get("other"), None);
        assert_eq!(
            params.iter().collect::<Vec<_>>(),
            [("id", "42"), ("path", "a/b/c")]
        );
        assert_eq!(format!("{params:?}"), r#"{"id": "42", "path": "a/b/c"}"#);

        let (name, problem) = PathParams::decode(names, &["%FF", "x"], true).unwrap_err();
        assert_eq!((name.as_str(), problem), ("id", ParamProblem::InvalidUtf8));
        assert!(PathParams::empty().is_empty());
    }

    #[test]
    fn path_params_set_by_hand_keep_their_values() {
        let params = PathParams::from_pairs([("id", "a/b%20.."), ("empty", "")]);
        assert_eq!(params.len(), 2);
        assert_eq!(params.get("id"), Some("a/b%20.."));
        assert_eq!(params.get("empty"), Some(""));
        assert!(PathParams::from_pairs([]).is_empty());

        let most: Vec<String> = (0..crate::routing::MAX_PARAMS)
            .map(|i| format!("p{i}"))
            .collect();
        let params = PathParams::from_pairs(most.iter().map(|name| (name.as_str(), name.as_str())));
        assert_eq!(params.get("p15"), Some("p15"));
        let too_many = std::panic::catch_unwind(|| {
            PathParams::from_pairs((0..=crate::routing::MAX_PARAMS).map(|_| ("p", "v")))
        });
        assert!(too_many.is_err());
    }

    #[test]
    fn problems_read_as_client_errors() {
        assert_eq!(
            ParamProblem::InvalidUtf8.message("id"),
            "Invalid URL: Invalid UTF-8 in `id`"
        );
        assert_eq!(
            PathProblem::DotSegment.to_string(),
            "Invalid URL: the path has a `.` or `..` segment"
        );
    }
}
