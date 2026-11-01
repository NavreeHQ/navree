//! A form read whole, straight from its grammar: the reference the streaming
//! extractor is held to when no limit applies. RFC 2046 (section 5.1.1) for
//! the delimiters, RFC 7578 for the parts, the parameters of
//! `Content-Disposition` read where a parameter starts, `filename*` as in
//! RFC 8187.
//!
//! Written to be plainly right on any input, not to be fast or bounded. The
//! fuzz target `http_multipart` uses it too.

use http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};

/// A part: its name, its file name and its value.
pub type Part = (Option<String>, Option<String>, Vec<u8>);

/// The parts of `body`, or `None` when it is not a whole form.
pub fn parts(body: &[u8], boundary: &str) -> Option<Vec<Part>> {
    let dash_boundary = format!("--{boundary}").into_bytes();
    let delimiter = format!("\r\n--{boundary}").into_bytes();
    // The first delimiter may be anywhere.
    let first = position(body, &dash_boundary)?;
    let mut rest = &body[first + dash_boundary.len()..];
    let mut parts = Vec::new();
    loop {
        if rest.starts_with(b"--") {
            return Some(parts);
        }
        let padding = rest
            .iter()
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .count();
        rest = rest[padding..].strip_prefix(b"\r\n")?;
        let head_length = position(rest, b"\r\n\r\n")? + 4;
        let (name, file_name) = names(&rest[..head_length])?;
        rest = &rest[head_length..];
        let value_length = position(rest, &delimiter)?;
        parts.push((name, file_name, rest[..value_length].to_vec()));
        rest = &rest[value_length + delimiter.len()..];
    }
}

fn position(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The name and file name of a head; `None` when it is not a head.
fn names(head: &[u8]) -> Option<(Option<String>, Option<String>)> {
    let mut slots = [httparse::EMPTY_HEADER; 32];
    let httparse::Status::Complete((_, raw)) = httparse::parse_headers(head, &mut slots).ok()?
    else {
        return None;
    };
    let mut headers = HeaderMap::new();
    for header in raw {
        let name = HeaderName::from_bytes(header.name.as_bytes()).ok()?;
        let value = HeaderValue::from_bytes(header.value).ok()?;
        if (name == CONTENT_DISPOSITION || name == CONTENT_TYPE) && headers.contains_key(&name) {
            return None;
        }
        headers.insert(name, value);
    }
    match headers.get(CONTENT_DISPOSITION) {
        Some(value) => disposition(value.as_bytes()),
        None => Some((None, None)),
    }
}

/// A parameter: its name in lower case, its value, and whether the value
/// was quoted.
type Parameter = (Vec<u8>, Vec<u8>, bool);

/// The parameters of a `Content-Disposition`, up to the first thing that
/// is not one.
fn parameters(value: &[u8]) -> Vec<Parameter> {
    let mut parameters = Vec::new();
    let Some(semicolon) = value.iter().position(|byte| *byte == b';') else {
        return parameters;
    };
    let at_end = |at: usize| at >= value.len();
    let skip_spaces = |mut at: usize| {
        while !at_end(at) && value[at].is_ascii_whitespace() {
            at += 1;
        }
        at
    };
    let mut at = semicolon + 1;
    loop {
        at = skip_spaces(at);
        if at_end(at) {
            return parameters;
        }
        let key_start = at;
        while !at_end(at) && value[at] != b'=' && value[at] != b';' {
            at += 1;
        }
        if at_end(at) {
            return parameters;
        }
        let key = value[key_start..at].trim_ascii_end().to_ascii_lowercase();
        if value[at] == b';' {
            at += 1;
            continue;
        }
        at = skip_spaces(at + 1);
        let (raw, quoted) = if value.get(at) == Some(&b'"') {
            let start = at + 1;
            at = start;
            loop {
                if at_end(at) {
                    return parameters;
                }
                if value[at] == b'"' && !(at > start && value[at - 1] == b'\\') {
                    break;
                }
                at += 1;
            }
            let raw = value[start..at].to_vec();
            at += 1;
            (raw, true)
        } else {
            let start = at;
            while !at_end(at) && value[at] != b';' && value[at] != b' ' {
                at += 1;
            }
            (value[start..at].to_vec(), false)
        };
        parameters.push((key, raw, quoted));
        at = skip_spaces(at);
        if value.get(at) != Some(&b';') {
            return parameters;
        }
        at += 1;
    }
}

/// The name and file name of a `Content-Disposition`; `None` when it names
/// a parameter twice.
fn disposition(value: &[u8]) -> Option<(Option<String>, Option<String>)> {
    let parameters = parameters(value);
    // `None` when `key` is there twice.
    let one = |key: &[u8]| {
        let mut found = parameters.iter().filter(|(name, ..)| name == key);
        let first = found.next();
        match found.next() {
            Some(_) => None,
            None => Some(first),
        }
    };
    let text = |(_, raw, quoted): &Parameter| {
        let text = String::from_utf8(raw.clone()).ok()?;
        Some(if *quoted {
            text.replace("\\\"", "\"")
        } else {
            text
        })
    };
    let name = one(b"name")?.and_then(text);
    let plain = one(b"filename")?.and_then(text);
    let extended = one(b"filename*")?.and_then(|(_, raw, _)| extended(raw));
    Some((name, extended.or(plain)))
}

/// `charset'language'value`, the value percent-encoded.
fn extended(raw: &[u8]) -> Option<String> {
    let first = raw.iter().position(|byte| *byte == b'\'')?;
    let second = first + 1 + raw[first + 1..].iter().position(|byte| *byte == b'\'')?;
    let charset = raw[..first].to_ascii_lowercase();
    let encoded = &raw[second + 1..];
    let mut decoded = Vec::new();
    let mut at = 0;
    while at < encoded.len() {
        let byte = encoded[at];
        if byte == b'%' {
            let digits = encoded.get(at + 1..at + 3)?;
            if !digits.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            decoded.push(u8::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()?);
            at += 3;
        } else if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
            decoded.push(byte);
            at += 1;
        } else {
            return None;
        }
    }
    match &charset[..] {
        b"utf-8" => String::from_utf8(decoded).ok(),
        b"iso-8859-1" => Some(decoded.iter().map(|byte| char::from(*byte)).collect()),
        _ => None,
    }
}
