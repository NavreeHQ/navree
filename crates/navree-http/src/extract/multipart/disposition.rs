//! A part's `Content-Disposition` (RFC 7578, section 4.2): its `name`, and
//! its file name from `filename`, or from `filename*` (RFC 8187) when that
//! one can be read.
//!
//! The header is a disposition type, then parameters separated by `;`, each
//! `name=value` with the value a token or a quoted string. A parameter is
//! only ever read where a parameter starts: never inside another one's value.
//! Whatever two readers could take differently is refused rather than guessed
//! at, so that a reader in front of the app and the app see the same form:
//! something that is not a parameter where one should be, a value that runs on
//! past its quotes or its token, a quote that may not close where it seems to.
//! A value that is not UTF-8 reads as absent.

/// What a part's `Content-Disposition` says.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Disposition {
    /// `name`.
    pub(super) name: Option<String>,
    /// `filename*` when it decodes, else `filename`.
    pub(super) file_name: Option<String>,
}

/// Why a `Content-Disposition` is refused.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Refused {
    /// A parameter given twice: two readers may each take a different one, so
    /// the part names neither.
    Repeated,
    /// Two readers may find different parameters in it.
    Ambiguous,
}

use Refused::{Ambiguous, Repeated};

/// The parameters this reads, each at most once.
#[derive(Default)]
struct Found<'a> {
    name: Option<Value<'a>>,
    file_name: Option<Value<'a>>,
    extended: Option<Value<'a>>,
}

/// A parameter's value, as it was sent.
#[derive(Clone, Copy)]
enum Value<'a> {
    Token(&'a [u8]),
    /// A quoted string without its quotes, escapes still in.
    Quoted(&'a [u8]),
}

impl<'a> Value<'a> {
    /// As text: a quoted string unescaped, and `None` for anything that is
    /// not UTF-8.
    fn text(self) -> Option<String> {
        match self {
            Value::Token(bytes) => std::str::from_utf8(bytes).ok().map(str::to_owned),
            Value::Quoted(bytes) => std::str::from_utf8(bytes)
                .ok()
                .map(|text| text.replace("\\\"", "\"")),
        }
    }

    fn bytes(self) -> &'a [u8] {
        match self {
            Value::Token(bytes) | Value::Quoted(bytes) => bytes,
        }
    }
}

/// Reads the value of a `Content-Disposition` header.
pub(super) fn parse(header: &[u8]) -> Result<Disposition, Refused> {
    let mut found = Found::default();
    // The disposition type comes first, and is not a parameter.
    let Some(start) = header.iter().position(|byte| *byte == b';') else {
        return Ok(Disposition::default());
    };
    let mut rest = header.get(start + 1..).unwrap_or_default();
    loop {
        rest = trim_start(rest);
        if rest.is_empty() {
            break;
        }
        let key_end = rest
            .iter()
            .position(|byte| matches!(byte, b'=' | b';'))
            .unwrap_or(rest.len());
        let key = trim_end(rest.get(..key_end).unwrap_or_default());
        if !key.iter().all(|byte| is_token(*byte)) {
            return Err(Ambiguous);
        }
        match rest.get(key_end) {
            // A parameter without a value says nothing.
            Some(b';') => {
                rest = rest.get(key_end + 1..).unwrap_or_default();
                continue;
            }
            Some(_) => {}
            None => break,
        }
        let value_start = trim_start(rest.get(key_end + 1..).unwrap_or_default());
        let (value, after) = match value_start.strip_prefix(b"\"") {
            Some(quoted) => quoted_string(quoted)?,
            None => {
                let end = value_start
                    .iter()
                    .position(|byte| matches!(byte, b';' | b' '))
                    .unwrap_or(value_start.len());
                let (token, after) = value_start.split_at(end);
                if token
                    .iter()
                    .any(|byte| *byte == b'"' || byte.is_ascii_control())
                {
                    return Err(Ambiguous);
                }
                (Value::Token(token), after)
            }
        };
        let slot = if key.eq_ignore_ascii_case(b"name") {
            Some(&mut found.name)
        } else if key.eq_ignore_ascii_case(b"filename") {
            Some(&mut found.file_name)
        } else if key.eq_ignore_ascii_case(b"filename*") {
            Some(&mut found.extended)
        } else {
            None
        };
        if let Some(slot) = slot {
            if slot.is_some() {
                return Err(Repeated);
            }
            *slot = Some(value);
        }
        // Only spaces may come between a value and the next `;`.
        match trim_start(after).split_first() {
            Some((b';', next)) => rest = next,
            Some(_) => return Err(Ambiguous),
            None => break,
        }
    }
    let file_name = found
        .extended
        .and_then(|value| extended(value.bytes()))
        .or_else(|| found.file_name.and_then(Value::text));
    Ok(Disposition {
        name: found.name.and_then(Value::text),
        file_name,
    })
}

/// A quoted string, `rest` starting after its opening quote: what is inside,
/// and what follows the closing quote. A quote with a backslash before it
/// does not close the string; any other backslash is kept, so a Windows path
/// sent as it was typed comes back as sent.
///
/// Not every reader keeps backslashes, or escapes with them, so what would
/// end the string for another reader is refused: a quote after an even run of
/// backslashes, a quote after a backslash with a `;` next, and a string that
/// never closes.
fn quoted_string(rest: &[u8]) -> Result<(Value<'_>, &[u8]), Refused> {
    let mut from = 0;
    loop {
        let unread = rest.get(from..).unwrap_or_default();
        let at = unread.iter().position(|byte| *byte == b'"');
        let at = from + at.ok_or(Ambiguous)?;
        let inside = rest.get(..at).unwrap_or_default();
        let after = rest.get(at + 1..).unwrap_or_default();
        let backslashes = inside.iter().rev().take_while(|b| **b == b'\\').count();
        if backslashes == 0 {
            return Ok((Value::Quoted(inside), after));
        }
        if backslashes.is_multiple_of(2) || trim_start(after).starts_with(b";") {
            return Err(Ambiguous);
        }
        from = at + 1;
    }
}

/// A `filename*` value (RFC 8187): `charset'language'percent-encoded`, in
/// UTF-8 or ISO-8859-1. `None` when it is not one.
fn extended(value: &[u8]) -> Option<String> {
    let mut sections = value.splitn(3, |byte| *byte == b'\'');
    let charset = sections.next()?;
    let _language = sections.next()?;
    let encoded = sections.next()?;
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut bytes = encoded.iter();
    while let Some(&byte) = bytes.next() {
        if byte == b'%' {
            let high = hex(*bytes.next()?)?;
            let low = hex(*bytes.next()?)?;
            decoded.push((high << 4) | low);
        } else if is_attr_char(byte) {
            decoded.push(byte);
        } else {
            return None;
        }
    }
    if charset.eq_ignore_ascii_case(b"utf-8") {
        String::from_utf8(decoded).ok()
    } else if charset.eq_ignore_ascii_case(b"iso-8859-1") {
        Some(decoded.into_iter().map(char::from).collect())
    } else {
        None
    }
}

/// RFC 9110's `tchar`: what a parameter's name is made of.
fn is_token(byte: u8) -> bool {
    is_attr_char(byte) || matches!(byte, b'%' | b'\'' | b'*')
}

/// RFC 8187's `attr-char`: what an extended value holds without encoding.
fn is_attr_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
        )
}

fn hex(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
}

fn trim_start(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    bytes.get(start..).unwrap_or_default()
}

fn trim_end(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(0, |at| at + 1);
    bytes.get(..end).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(header: &str) -> Disposition {
        parse(header.as_bytes()).unwrap()
    }

    fn names(header: &str) -> (Option<String>, Option<String>) {
        let read = read(header);
        (read.name, read.file_name)
    }

    fn both(name: &str, file_name: &str) -> (Option<String>, Option<String>) {
        (Some(name.to_owned()), Some(file_name.to_owned()))
    }

    #[test]
    fn quoted_and_unquoted_values_are_read() {
        assert_eq!(
            names(r#"form-data; name="field"; filename="a b.txt""#),
            both("field", "a b.txt")
        );
        assert_eq!(
            names("form-data; name=field; filename=a.txt"),
            both("field", "a.txt")
        );
        assert_eq!(
            names(r#"form-data; filename="a.txt"; name="field""#),
            both("field", "a.txt")
        );
        assert_eq!(
            names("form-data; name  =  field  "),
            (Some("field".into()), None)
        );
        assert_eq!(names(r#"form-data;name="a";filename="b""#), both("a", "b"));
        assert_eq!(names("form-data; name=; filename=b"), both("", "b"));
        assert_eq!(names(r#"form-data; name=""; filename="""#), both("", ""));
        assert_eq!(names("form-data; name  =  "), (Some(String::new()), None));
        assert_eq!(
            names("form-data; name=\"\u{4f60}\u{597d}\"; filename=\"\u{995}.txt\""),
            both("\u{4f60}\u{597d}", "\u{995}.txt")
        );
    }

    #[test]
    fn a_quoted_string_holds_separators_and_escaped_quotes() {
        assert_eq!(
            names(r#"form-data; name="a;b;c""#),
            (Some("a;b;c".into()), None)
        );
        assert_eq!(
            names(r#"form-data; name=";"; filename=";""#),
            both(";", ";")
        );
        assert_eq!(
            names(r#"form-data; name="my\"field\"name""#),
            (Some(r#"my"field"name"#.into()), None)
        );
        assert_eq!(
            names(r#"form-data; name="a"; filename="C:\fakepath\a.png""#),
            both("a", r"C:\fakepath\a.png")
        );
    }

    #[test]
    fn parameter_names_are_read_without_regard_to_case() {
        assert_eq!(
            names(r#"form-data; NAME="a"; FileName="b""#),
            both("a", "b")
        );
    }

    /// The attack: a value that looks like a parameter, for a reader that
    /// searches the header for `name=` rather than reading it in order.
    #[test]
    fn a_parameter_inside_a_value_is_not_read() {
        assert_eq!(
            names(r#"form-data; filename="a name=evil.txt""#),
            (None, Some("a name=evil.txt".into()))
        );
        assert_eq!(
            names(r#"form-data; x="; name=evil"; filename="a""#),
            (None, Some("a".into()))
        );
        assert_eq!(names("form-data name=evil"), (None, None));
        assert_eq!(names(r#"name="evil""#), (None, None));
        assert_eq!(names(r#"form-data; username="x""#), (None, None));
        assert_eq!(
            names(r#"form-data; xname="x"; xfilename="y""#),
            (None, None)
        );
    }

    #[test]
    fn what_two_readers_could_take_differently_is_refused() {
        for header in [
            r#"form-data; name="a" junk; filename="b""#,
            r#"form-data; name="a" filename="b""#,
            "form-data; name=a b; filename=c",
            "form-data; name=a\tfilename=c",
            r#"form-data; name=a"; filename=c"#,
            r#"form-data; name="a"; junk filename="b""#,
            r#"form-data; name="a"#,
            r#"form-data; filename="a\""#,
            r#"form-data; name="a\\"; filename="b""#,
            r#"form-data; name="a\"; filename="b""#,
        ] {
            assert_eq!(parse(header.as_bytes()), Err(Ambiguous), "{header}");
        }
        assert_eq!(names("form-data; flag; name=a"), (Some("a".into()), None));
    }

    #[test]
    fn a_parameter_given_twice_is_refused() {
        for header in [
            r#"form-data; name="a"; name="b""#,
            r#"form-data; name="a"; NAME="a""#,
            r#"form-data; filename="a"; filename="b""#,
            "form-data; filename*=UTF-8''a; filename*=UTF-8''b",
        ] {
            assert_eq!(parse(header.as_bytes()), Err(Repeated), "{header}");
        }
        assert!(parse(br#"form-data; name="a"; filename="a"; filename*=UTF-8''a"#).is_ok());
    }

    #[test]
    fn a_value_that_is_not_utf8_reads_as_absent() {
        let read = parse(b"form-data; name=\"\xff\"; filename=\"\xfe.txt\"").unwrap();
        assert_eq!(read, Disposition::default());
    }

    #[test]
    fn an_extended_file_name_is_preferred_when_it_decodes() {
        assert_eq!(
            names(
                r#"form-data; name="f"; filename*=UTF-8''na%C3%AFve%20file.txt; filename="naive file.txt""#
            ),
            both("f", "na\u{ef}ve file.txt")
        );
        assert_eq!(
            names(r#"form-data; name="f"; filename="naive.txt"; filename*=utf-8'en'%E2%82%AC.txt"#),
            both("f", "\u{20ac}.txt")
        );
        assert_eq!(
            names("form-data; name=f; filename*=ISO-8859-1''caf%E9.txt"),
            both("f", "caf\u{e9}.txt")
        );
        assert_eq!(
            names("form-data; name=f; filename*=\"UTF-8''quoted.txt\""),
            both("f", "quoted.txt")
        );
        assert_eq!(
            names("form-data; name=f; filename*=UTF-8''..%2F..%2Fetc%2Fpasswd"),
            both("f", "../../etc/passwd")
        );
    }

    #[test]
    fn an_extended_file_name_that_does_not_decode_gives_way() {
        for extended in [
            "UTF-8''%FF.txt",
            "UTF-8''%4",
            "UTF-8''%zz",
            "\"UTF-8''a b\"",
            "UTF-8''\u{e9}",
            "UTF-16''a",
            "UTF-8'a",
            "a",
        ] {
            let header = format!("form-data; name=f; filename*={extended}; filename=\"plain.txt\"");
            assert_eq!(names(&header), both("f", "plain.txt"), "{extended}");
            let header = format!("form-data; name=f; filename*={extended}");
            assert_eq!(names(&header), (Some("f".into()), None), "{extended}");
        }
    }

    #[test]
    fn a_header_with_no_parameters_names_nothing() {
        for header in ["", "form-data", "form-data;", "form-data; ;", ";;;"] {
            assert_eq!(read(header), Disposition::default(), "{header:?}");
        }
    }

    #[test]
    fn hex_digits_read_in_either_case() {
        assert_eq!(hex(b'a'), Some(10));
        assert_eq!(hex(b'F'), Some(15));
        assert_eq!(hex(b'g'), None);
    }
}
