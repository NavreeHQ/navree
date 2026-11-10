//! `Content-Type` and `Content-Disposition` values (RFC 2045, RFC 2183):
//! a type, a subtype and parameters, with RFC 2231 extended and continued
//! parameters (`filename*0*=utf-8''...`) and RFC 2047 encoded words in
//! plain parameter values (`name="=?utf-8?B?...?="`), which mail clients
//! write although the RFC doesn't allow it.

use super::charset;
use super::transfer::hex_value;
use super::words;

/// A parsed `Content-Type` or `Content-Disposition`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ContentType {
    /// The type (`text`) or the disposition (`attachment`), in lower case.
    pub kind: String,
    /// The subtype (`plain`), in lower case.
    pub subtype: Option<String>,
    /// The parameters in the order they first appear: names in lower case,
    /// values decoded.
    pub params: Vec<(String, String)>,
}

impl ContentType {
    /// The first parameter named `name` (lower case).
    pub(super) fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub(super) fn has_param(&self, name: &str) -> bool {
        self.params.iter().any(|(key, _)| key == name)
    }

    /// `type/subtype` (or the type alone) in lower case.
    pub(super) fn media_type(&self) -> String {
        match &self.subtype {
            Some(subtype) => format!("{}/{subtype}", self.kind),
            None => self.kind.clone(),
        }
    }
}

/// One `name*N` or `name*N*` piece of an RFC 2231 parameter.
struct Piece {
    /// Where the parameter goes in the list.
    slot: usize,
    index: u32,
    extended: bool,
    value: Vec<u8>,
}

/// The parameters the reader uses; the others are skipped as they are read,
/// so a value can't make it hold a list of them.
const KEPT: [&str; 4] = ["boundary", "charset", "filename", "name"];

/// The most RFC 2231 pieces one value may be put together from.
const MAX_PIECES: usize = 256;

/// Parses a header value (unfolded). `None` when it has no type.
pub(super) fn parse(value: &[u8]) -> Option<ContentType> {
    let mut cursor = Cursor { text: value, at: 0 };
    cursor.skip_space();
    let kind = lower(cursor.token(b"/;"));
    if kind.is_empty() {
        return None;
    }
    cursor.skip_space();
    let subtype = if cursor.eat(b'/') {
        cursor.skip_space();
        Some(lower(cursor.token(b";"))).filter(|subtype| !subtype.is_empty())
    } else {
        None
    };

    // Parameters as written; RFC 2231 pieces are put together at the end.
    let mut params: Vec<(String, Option<String>)> = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    loop {
        // Anything up to the next separator that isn't a parameter is
        // skipped.
        if !cursor.skip_separator() {
            break;
        }
        cursor.skip_space();
        let name = lower(cursor.token(b"=;"));
        cursor.skip_space();
        if name.is_empty() || !cursor.eat(b'=') {
            continue;
        }
        cursor.skip_space();
        let raw = if cursor.peek() == Some(b'"') {
            cursor.quoted()
        } else {
            let value = cursor.until_separator();
            // A comment ends an unquoted value.
            let value = value.split(|&byte| byte == b'(').next().unwrap_or(value);
            trim_end(value).to_vec()
        };
        let (base, index, extended) = split_name(&name);
        if !KEPT.contains(&base) {
            continue;
        }
        // The first parameter of a name is the one that counts.
        let slot = params.iter().position(|(key, _)| key == base);
        match (index, slot) {
            (None, Some(_)) => {}
            (None, None) if extended => {
                params.push((base.to_owned(), Some(extended_value(&raw))));
            }
            (None, None) => params.push((base.to_owned(), Some(words::decode_inline(&raw)))),
            (Some(index), slot) => {
                let slot = match slot {
                    // Pieces of a name that already has a plain value.
                    Some(slot) if params.get(slot).is_some_and(|(_, value)| value.is_some()) => {
                        continue;
                    }
                    Some(slot) => slot,
                    None => {
                        params.push((base.to_owned(), None));
                        params.len() - 1
                    }
                };
                if pieces.len() < MAX_PIECES {
                    pieces.push(Piece {
                        slot,
                        index,
                        extended,
                        value: raw,
                    });
                }
            }
        }
    }

    if !pieces.is_empty() {
        pieces.sort_by_key(|piece| (piece.slot, piece.index));
        let mut start = 0;
        while let Some(first) = pieces.get(start) {
            let slot = first.slot;
            let end = pieces
                .iter()
                .skip(start)
                .position(|piece| piece.slot != slot)
                .map_or(pieces.len(), |count| start + count);
            let value = continued_value(pieces.get(start..end).unwrap_or_default());
            if let Some((_, param)) = params.get_mut(slot) {
                *param = Some(value);
            }
            start = end;
        }
    }

    Some(ContentType {
        kind,
        subtype,
        params: params
            .into_iter()
            .filter_map(|(name, value)| Some((name, value?)))
            .collect(),
    })
}

/// The bytes as text in lower case.
fn lower(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_ascii_lowercase()
}

fn is_blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// The bytes without the blanks they end with.
fn trim_end(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|&byte| !is_blank(byte))
        .map_or(0, |last| last + 1);
    bytes.get(..end).unwrap_or_default()
}

/// `name`, `name*`, `name*2` or `name*2*`: the name, the piece's index and
/// whether the value is extended (`charset'language'%XX`).
fn split_name(name: &str) -> (&str, Option<u32>, bool) {
    let (name, extended) = match name.strip_suffix('*') {
        Some(name) => (name, true),
        None => (name, false),
    };
    if let Some((base, index)) = name.rsplit_once('*')
        && !index.is_empty()
        && index.bytes().all(|byte| byte.is_ascii_digit())
        && let Ok(index) = index.parse::<u32>()
    {
        return (base, Some(index), extended);
    }
    (name, None, extended)
}

/// `charset'language'%XX...` (RFC 2231): the bytes decoded in the charset.
fn extended_value(raw: &[u8]) -> String {
    let (charset, encoded) = split_charset(raw);
    let charset = charset.map(String::from_utf8_lossy);
    charset::decode(&percent_decode(encoded), charset.as_deref())
}

/// The pieces of one continued parameter, in order: their bytes joined,
/// then decoded in the first piece's charset.
fn continued_value(pieces: &[Piece]) -> String {
    let mut bytes = Vec::new();
    let mut charset = None;
    for (position, piece) in pieces.iter().enumerate() {
        if piece.extended {
            let encoded = if position == 0 {
                let (set, encoded) = split_charset(&piece.value);
                charset = set.map(String::from_utf8_lossy);
                encoded
            } else {
                &piece.value
            };
            bytes.extend_from_slice(&percent_decode(encoded));
        } else {
            bytes.extend_from_slice(&piece.value);
        }
    }
    charset::decode(&bytes, charset.as_deref())
}

/// `utf-8'en'rest` -> (`utf-8`, `rest`). A value without the two quotes is
/// all data.
fn split_charset(raw: &[u8]) -> (Option<&[u8]>, &[u8]) {
    let mut parts = raw.splitn(3, |&byte| byte == b'\'');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(charset), Some(_language), Some(rest)) => {
            (Some(charset).filter(|charset| !charset.is_empty()), rest)
        }
        _ => (None, raw),
    }
}

/// `%XX` escapes decoded; a `%` that starts none is kept.
fn percent_decode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(index + 1).copied().and_then(hex_value),
                bytes.get(index + 2).copied().and_then(hex_value),
            )
        {
            out.push((high << 4) | low);
            index += 3;
            continue;
        }
        out.push(byte);
        index += 1;
    }
    out
}

/// A position in a header value.
struct Cursor<'a> {
    text: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn rest(&self) -> &'a [u8] {
        self.text.get(self.at..).unwrap_or_default()
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    /// Skips blanks and comments (nested, with escapes).
    fn skip_space(&mut self) {
        let mut depth = 0usize;
        let mut escaped = false;
        while let Some(byte) = self.peek() {
            if depth > 0 {
                match byte {
                    _ if escaped => escaped = false,
                    b'\\' => escaped = true,
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
            } else if byte == b'(' {
                depth = 1;
            } else if !is_blank(byte) {
                return;
            }
            self.at += 1;
        }
    }

    /// A token: up to a blank, a comment or one of `stops`.
    fn token(&mut self, stops: &[u8]) -> &'a [u8] {
        let rest = self.rest();
        let end = rest
            .iter()
            .position(|&byte| is_blank(byte) || byte == b'(' || stops.contains(&byte))
            .unwrap_or(rest.len());
        self.at += end;
        rest.get(..end).unwrap_or_default()
    }

    /// Everything up to the next separator (or the end).
    fn until_separator(&mut self) -> &'a [u8] {
        let rest = self.rest();
        let end = memchr::memchr2(b';', b'\n', rest).unwrap_or(rest.len());
        self.at += end;
        rest.get(..end).unwrap_or_default()
    }

    /// Moves past the next separator: a `;`, or a line break (a parameter
    /// on a folded line of its own, its `;` left out, is still one);
    /// `false` at the end.
    fn skip_separator(&mut self) -> bool {
        match memchr::memchr2(b';', b'\n', self.rest()) {
            Some(at) => {
                self.at += at + 1;
                true
            }
            None => {
                self.at = self.text.len();
                false
            }
        }
    }

    /// A quoted string (the cursor is on its `"`), without its quotes and
    /// escapes. An unclosed string runs to the end.
    fn quoted(&mut self) -> Vec<u8> {
        let rest = self.text.get(self.at + 1..).unwrap_or_default();
        let mut out = Vec::new();
        let mut escaped = false;
        for (offset, &byte) in rest.iter().enumerate() {
            if escaped {
                out.push(byte);
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                self.at += 1 + offset + 1;
                return out;
            } else if byte != b'\r' && byte != b'\n' {
                out.push(byte);
            }
        }
        self.at = self.text.len();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(value: &str) -> Option<ContentType> {
        parse(value.as_bytes())
    }

    fn params(value: &str) -> Vec<(String, String)> {
        read(value).unwrap().params
    }

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    #[test]
    fn a_folded_line_starts_a_parameter_even_without_its_semicolon() {
        for value in [
            "multipart/mixed;\r\n\tboundary=\"b\"",
            "multipart/mixed\r\n\tboundary=\"b\"",
            "multipart/mixed; charset=x\r\n boundary=b",
        ] {
            assert_eq!(
                read(value).unwrap().param("boundary"),
                Some("b"),
                "{value:?}"
            );
        }
        assert_eq!(
            params("attachment;\r\n filename=\"a\r\n b.txt\""),
            vec![pair("filename", "a b.txt")],
            "a fold inside quotes is part of the value"
        );
    }

    #[test]
    fn types_and_parameters_are_read() {
        let ct = read(" Text/HTML ; Charset=\"UTF-8\"; format=flowed; name=x").unwrap();
        assert_eq!(ct.kind, "text");
        assert_eq!(ct.subtype.as_deref(), Some("html"));
        assert_eq!(ct.media_type(), "text/html");
        assert_eq!(ct.param("charset"), Some("UTF-8"));
        assert!(ct.has_param("name"));
        assert_eq!(
            ct.param("format"),
            None,
            "only the parameters the reader uses are kept"
        );

        let cd = read("attachment; filename=\"q3 report.pdf\"").unwrap();
        assert_eq!(cd.kind, "attachment");
        assert_eq!(cd.subtype, None);
        assert_eq!(cd.param("filename"), Some("q3 report.pdf"));

        assert_eq!(
            read("multipart/mixed; boundary=\"outer\"")
                .unwrap()
                .param("boundary"),
            Some("outer")
        );
        assert_eq!(
            read("multipart/mixed; boundary=simple;")
                .unwrap()
                .param("boundary"),
            Some("simple")
        );
        assert_eq!(
            params("text/plain; name=my file.txt ; charset=x"),
            vec![pair("name", "my file.txt"), pair("charset", "x")]
        );
        assert_eq!(
            params("text/plain (a comment); charset=us-ascii (Plain text)"),
            vec![pair("charset", "us-ascii")]
        );
        assert_eq!(
            params("text/plain; name=\"q\\\"uote\\\\d\"; ; junk; charset=2"),
            vec![pair("name", "q\"uote\\d"), pair("charset", "2")]
        );
        assert_eq!(
            params("text/plain; name=\"unclosed"),
            vec![pair("name", "unclosed")]
        );
        assert_eq!(
            params("text/plain; name=first; name=second"),
            vec![pair("name", "first")]
        );
        assert_eq!(read(""), None);
        assert_eq!(read(" ; charset=utf-8"), None);
        assert_eq!(read("text").unwrap().media_type(), "text");
    }

    #[test]
    fn rfc_2231_parameters_are_put_together() {
        assert_eq!(
            params("attachment; filename*=utf-8''Gr%C3%BC%C3%9Fe.txt"),
            vec![pair("filename", "Grüße.txt")]
        );
        assert_eq!(
            params("attachment; filename*=iso-8859-1'de'Gr%FC%DFe.txt"),
            vec![pair("filename", "Grüße.txt")]
        );
        assert_eq!(
            params("attachment; filename*1=\"two.\"; filename*0=\"part \"; filename*2=txt; name=3"),
            vec![pair("filename", "part two.txt"), pair("name", "3")]
        );
        // A character split between two pieces.
        assert_eq!(
            params("attachment; filename*0*=utf-8''Gr%C3; filename*1*=%BC%C3%9Fe.txt"),
            vec![pair("filename", "Grüße.txt")]
        );
        assert_eq!(
            params(
                "x/y; name*0*=us-ascii'en'This%20is%20even%20more%20; name*1*=%2A%2A%2Afun%2A%2A%2A%20; name*2=\"isn't it!\""
            ),
            vec![pair("name", "This is even more ***fun*** isn't it!")]
        );
        assert_eq!(
            params("x/y; name*=100%; filename*='',c"),
            vec![pair("name", "100%"), pair("filename", ",c")]
        );
        // Pieces of a name that already has a value are left out.
        assert_eq!(
            params("x/y; name=plain; name*0=a; name*1=b"),
            vec![pair("name", "plain")]
        );
    }

    #[test]
    fn a_value_full_of_parameters_holds_only_what_is_kept() {
        let value = format!("x/y{}", "; a=b; name*7=c".repeat(100_000));
        let ct = read(&value).unwrap();
        assert_eq!(ct.params, vec![pair("name", &"c".repeat(MAX_PIECES))]);
    }

    #[test]
    fn encoded_words_in_plain_values_are_decoded() {
        assert_eq!(
            params("application/pdf; name=\"=?UTF-8?B?w6TDtsO8LnBkZg==?=\""),
            vec![("name".into(), "äöü.pdf".into())]
        );
        assert_eq!(
            params("application/pdf; name==?utf-8?q?a?= =?utf-8?q?b?=.pdf"),
            vec![("name".into(), "ab.pdf".into())]
        );
        assert_eq!(
            params("application/pdf; name=\"x =?utf-8?q?=C3=A9?= y\""),
            vec![("name".into(), "x é y".into())]
        );
    }
}
