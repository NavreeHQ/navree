//! A form read whole, straight from its grammar: the reference the streaming
//! extractor is held to when no limit applies. RFC 2046 (section 5.1.1) for
//! the delimiters, RFC 7578 for the parts, the parameters of
//! `Content-Disposition` read where a parameter starts after the type
//! `form-data`, `filename*` as in RFC 8187.
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
    // The first delimiter starts the body or a line.
    let mut rest = match body.strip_prefix(&dash_boundary[..]) {
        Some(rest) => rest,
        None => {
            let first = position(body, &delimiter)?;
            &body[first + delimiter.len()..]
        }
    };
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
    let bare_line_feed =
        (0..head.len()).any(|at| head[at] == b'\n' && (at == 0 || head[at - 1] != b'\r'));
    if bare_line_feed {
        return None;
    }
    let httparse::Status::Complete((read, raw)) = httparse::parse_headers(head, &mut slots).ok()?
    else {
        return None;
    };
    // Nothing of a head is left unread, but the second blank line of a part
    // with no headers.
    if read != head.len() && head != b"\r\n\r\n" {
        return None;
    }
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

/// The parameters of a `Content-Disposition`; `None` when its type is not
/// `form-data`, or when two readers could find different parameters: something
/// else where a parameter should be, a value that runs on, a quote another
/// reader would close elsewhere.
fn parameters(value: &[u8]) -> Option<Vec<Parameter>> {
    let mut parameters = Vec::new();
    let semicolon = value.iter().position(|byte| *byte == b';');
    if !value[..semicolon.unwrap_or(value.len())]
        .trim_ascii()
        .eq_ignore_ascii_case(b"form-data")
    {
        return None;
    }
    let Some(semicolon) = semicolon else {
        return Some(parameters);
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
            return Some(parameters);
        }
        let key_start = at;
        while !at_end(at) && value[at] != b'=' && value[at] != b';' {
            at += 1;
        }
        let key = value[key_start..at].trim_ascii_end().to_ascii_lowercase();
        let token = |byte: &u8| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(byte);
        if !key.iter().all(token) {
            return None;
        }
        if at_end(at) {
            return Some(parameters);
        }
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
                    return None;
                }
                if value[at] == b'"' {
                    let backslashes = value[start..at]
                        .iter()
                        .rev()
                        .take_while(|byte| **byte == b'\\')
                        .count();
                    if backslashes == 0 {
                        break;
                    }
                    if backslashes % 2 == 0 || value.get(skip_spaces(at + 1)) == Some(&b';') {
                        return None;
                    }
                }
                at += 1;
            }
            let raw = value[start..at].to_vec();
            at += 1;
            (raw, true)
        } else {
            let start = at;
            while !at_end(at) && value[at] != b';' && value[at] != b' ' {
                if value[at] == b'"' || value[at].is_ascii_control() {
                    return None;
                }
                at += 1;
            }
            (value[start..at].to_vec(), false)
        };
        parameters.push((key, raw, quoted));
        at = skip_spaces(at);
        if at_end(at) {
            return Some(parameters);
        }
        if value[at] != b';' {
            return None;
        }
        at += 1;
    }
}

/// The name and file name of a `Content-Disposition`; `None` when it names
/// a parameter twice or can be read two ways.
fn disposition(value: &[u8]) -> Option<(Option<String>, Option<String>)> {
    let parameters = parameters(value)?;
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
    let plain_parameter = one(b"filename")?;
    let plain = plain_parameter.and_then(text);
    let encoded = one(b"filename*")?.map(|(_, raw, _)| extended(raw));
    // Both given: the same name, or the head can be read two ways.
    let file_name = match (plain_parameter.is_some(), encoded) {
        (true, Some(encoded)) => match (plain, encoded) {
            (Some(plain), Some(encoded)) if plain == encoded => Some(encoded),
            _ => return None,
        },
        (false, Some(encoded)) => encoded,
        (_, None) => plain,
    };
    Some((name, file_name))
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
