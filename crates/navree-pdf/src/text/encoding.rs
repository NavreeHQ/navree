//! How a font's codes decode to text, after its `ToUnicode` map (which comes
//! first, as ISO 32000 9.10.2 says, and is read by the font cache): a standard
//! encoding, an encoding's `Differences` over a base, or a predefined UTF-16
//! CMap.
//!
//! A font's `Encoding` decides: a standard encoding's name; a dictionary with
//! a `BaseEncoding` (standard when it has none) and `Differences`; or the
//! name of a predefined CMap (`UniGB-UCS2-H` and `UniGB-UTF16-H` are read
//! as UTF-16; others decode nothing). A font without an `Encoding` reads the
//! standard encoding. An encoding that cannot be read (a cycle of references,
//! `Identity-H` with no map) falls back to the standard encoding. A glyph name
//! that cannot be read loses that one code, and a bad item of `Differences`
//! that one item.

use std::collections::HashMap;

use crate::document::Document;
use crate::object::{Dictionary, Object};

use super::cmap::utf16_bytes_with_bom;
use super::tables;

/// A font's encoding.
pub(crate) enum Encoding<'a> {
    /// A table of 256 codes (0 for none).
    Table(&'static [u16; 256]),
    /// A predefined CMap or another name.
    Named(&'a [u8]),
    /// `Differences` over a base: the text of each code it names, none for
    /// a glyph name that cannot be read.
    Differences {
        base: Box<Encoding<'a>>,
        map: HashMap<u8, Box<str>>,
    },
}

/// The UTF-16 unit of the glyph called `name`.
pub(crate) fn glyph(name: &[u8]) -> Option<u16> {
    tables::GLYPHS
        .binary_search_by(|(known, _)| known.as_bytes().cmp(name))
        .ok()
        .and_then(|index| tables::GLYPHS.get(index))
        .map(|(_, unit)| *unit)
}

/// The text of the glyph called `name`: a name the tables know; `uni`
/// and four hex digits for each UTF-16 unit; `u` and four to six hex digits for
/// a code point; or, as the Adobe Glyph List reads names, the part before a
/// `.`, and the parts between `_` each read so.
pub(crate) fn glyph_text(name: &[u8]) -> Option<String> {
    let name = name.split(|&byte| byte == b'.').next()?;
    if name.is_empty() {
        return None;
    }
    let mut text = String::new();
    for part in name.split(|&byte| byte == b'_') {
        text.push_str(&component(part)?);
    }
    Some(text)
}

/// One part of a glyph name.
fn component(part: &[u8]) -> Option<String> {
    if let Some(unit) = glyph(part) {
        return Some(char::decode_utf16([unit]).flatten().collect());
    }
    let upper_hex = |digits: &[u8]| {
        digits
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'A'..=b'F'))
    };
    let hex = |digits: &[u8]| -> Option<u32> {
        u32::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()
    };
    if let Some(digits) = part.strip_prefix(b"uni")
        && !digits.is_empty()
        && digits.len().is_multiple_of(4)
        && upper_hex(digits)
    {
        let units: Option<Vec<u16>> = digits
            .chunks(4)
            .map(|unit| hex(unit).and_then(|unit| u16::try_from(unit).ok()))
            .collect();
        return String::from_utf16(&units?).ok();
    }
    if let Some(digits) = part.strip_prefix(b"u")
        && (4..=6).contains(&digits.len())
        && upper_hex(digits)
    {
        return char::from_u32(hex(digits)?).map(String::from);
    }
    None
}

fn table_text(table: &[u16; 256], bytes: &[u8], out: &mut String) {
    for &byte in bytes {
        let unit = table[usize::from(byte)];
        if unit != 0 {
            out.extend(char::decode_utf16([unit]).flatten());
        }
    }
}

impl Encoding<'_> {
    /// The text of `bytes`; `None` for an encoding that decodes nothing.
    pub(crate) fn decode(&self, bytes: &[u8]) -> Option<String> {
        let mut out = String::new();
        self.decode_into(bytes, &mut out)?;
        Some(out)
    }

    fn decode_into(&self, bytes: &[u8], out: &mut String) -> Option<()> {
        match self {
            Encoding::Table(table) => table_text(table, bytes, out),
            Encoding::Named(b"UniGB-UCS2-H" | b"UniGB-UTF16-H") => {
                out.push_str(&utf16_bytes_with_bom(bytes))
            }
            Encoding::Named(b"WinAnsiEncoding") => {
                table_text(&tables::WIN_ANSI_ENCODING, bytes, out)
            }
            Encoding::Named(_) => return None,
            Encoding::Differences { base, map } => {
                for &byte in bytes {
                    match map.get(&byte) {
                        Some(text) => out.push_str(text),
                        None => base.decode_into(&[byte], out)?,
                    }
                }
            }
        }
        Some(())
    }
}

/// The encoding of `font`, when it is one (see [`is_font`]):
/// what its codes read as where no `ToUnicode` map says otherwise.
pub(crate) fn font_encoding<'a>(
    document: &'a Document,
    font: &'a Dictionary,
) -> Option<Encoding<'a>> {
    if !is_font(document, font) {
        return None;
    }
    let encoding = match font.get(b"Encoding") {
        Some(encoding) => base_of(document, encoding),
        None => None,
    };
    Some(encoding.unwrap_or(Encoding::Table(&tables::STANDARD_ENCODING)))
}

/// Whether `font` can be a font: its `Type` is `Font`, or it has none and
/// has what a font has.
pub(crate) fn is_font(document: &Document, font: &Dictionary) -> bool {
    match font.get(b"Type") {
        Some(kind) => document.resolve(kind).and_then(Object::as_name) == Some(b"Font"),
        None => font.has(b"Subtype") || font.has(b"BaseFont"),
    }
}

/// The name an `Encoding` entry leads to, through any number of references.
pub(crate) fn encoding_name<'a>(document: &'a Document, font: &'a Dictionary) -> Option<&'a [u8]> {
    document.get(font, b"Encoding")?.as_name()
}

/// The encoding an `Encoding` entry stands for.
fn base_of<'a>(document: &'a Document, encoding: &'a Object) -> Option<Encoding<'a>> {
    match document.resolve(encoding)? {
        Object::Name(name) => named(name),
        Object::Dictionary(dict) => {
            if !matches!(
                document.get(dict, b"Type").and_then(Object::as_name),
                None | Some(b"Encoding")
            ) {
                return None;
            }
            let base = match document
                .get(dict, b"BaseEncoding")
                .and_then(Object::as_name)
            {
                Some(name) => named(name)?,
                None => Encoding::Table(&tables::STANDARD_ENCODING),
            };
            let Some(differences) = dict.get(b"Differences") else {
                return Some(base);
            };
            let items = document.resolve(differences)?.as_array()?;
            let mut map = HashMap::new();
            // The code the next name is for; outside a byte, a name is for
            // no code.
            let mut code = 0i64;
            for item in items {
                match document.resolve(item) {
                    Some(Object::Integer(value)) => code = *value,
                    Some(Object::Name(name)) => {
                        if let Ok(byte) = u8::try_from(code) {
                            map.insert(byte, glyph_text(name).unwrap_or_default().into());
                        }
                        code = code.saturating_add(1);
                    }
                    _ => {}
                }
            }
            Some(Encoding::Differences {
                base: Box::new(base),
                map,
            })
        }
        _ => None,
    }
}

/// The encoding `name` stands for; `Identity-H` and `Identity-V` have none of
/// their own: they read the font's `ToUnicode` map.
fn named(name: &[u8]) -> Option<Encoding<'_>> {
    Some(match name {
        b"StandardEncoding" => Encoding::Table(&tables::STANDARD_ENCODING),
        b"MacRomanEncoding" => Encoding::Table(&tables::MAC_ROMAN_ENCODING),
        b"MacExpertEncoding" => Encoding::Table(&tables::MAC_EXPERT_ENCODING),
        b"WinAnsiEncoding" => Encoding::Table(&tables::WIN_ANSI_ENCODING),
        b"PDFDocEncoding" => Encoding::Table(&tables::PDF_DOC_ENCODING),
        b"Identity-H" | b"Identity-V" => return None,
        other => Encoding::Named(other),
    })
}

/// A PDF text string as the standard reads it: UTF-16BE after its
/// byte-order mark, UTF-8 after its own, PDFDocEncoding otherwise. `None`
/// for UTF-16 or UTF-8 that does not decode.
pub fn decode_text_string(bytes: &[u8]) -> Option<String> {
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_be_bytes(pair))
            .collect();
        String::from_utf16(&units).ok()
    } else if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8(rest.to_vec()).ok()
    } else {
        let mut out = String::new();
        table_text(&tables::PDF_DOC_ENCODING, bytes, &mut out);
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_names_find_their_units() {
        assert_eq!(glyph(b"A"), Some(0x41));
        assert_eq!(glyph(b"eacute"), Some(0xE9));
        assert_eq!(glyph(b"Euro"), Some(0x20AC));
        assert_eq!(glyph(b"no-such-glyph"), None);
    }

    #[test]
    fn glyph_names_of_every_kind_read_as_text() {
        let text = |name: &[u8]| glyph_text(name);
        assert_eq!(text(b"A").as_deref(), Some("A"));
        assert_eq!(text(b"uni0041").as_deref(), Some("A"));
        assert_eq!(text(b"uni00410042").as_deref(), Some("AB"));
        assert_eq!(text(b"uniD83DDE00").as_deref(), Some("\u{1F600}"));
        assert_eq!(text(b"u0041").as_deref(), Some("A"));
        assert_eq!(text(b"u1F600").as_deref(), Some("\u{1F600}"));
        assert_eq!(text(b"a.sc").as_deref(), Some("a"));
        assert_eq!(text(b"f_i").as_deref(), Some("fi"));
        assert_eq!(text(b".notdef"), None);
        assert_eq!(text(b"g12"), None);
        assert_eq!(text(b"uni004"), None);
        assert_eq!(text(b"uni004g"), None);
        assert_eq!(text(b"uD800"), None);
        assert_eq!(text(b"u110000"), None);
    }

    #[test]
    fn standard_tables_decode() {
        let win = Encoding::Table(&tables::WIN_ANSI_ENCODING);
        assert_eq!(win.decode(&[0x41, 0xE9, 0x80, 0xDF]).unwrap(), "Aé€ß");
        let standard = Encoding::Table(&tables::STANDARD_ENCODING);
        assert_eq!(standard.decode(&[0x27, 0x60]).unwrap(), "\u{2019}\u{2018}");
        assert!(Encoding::Named(b"Custom").decode(b"x").is_none());
        assert_eq!(
            Encoding::Named(b"UniGB-UCS2-H")
                .decode(&[0x4E, 0x2D])
                .unwrap(),
            "中"
        );
        let differences = Encoding::Differences {
            base: Box::new(Encoding::Table(&tables::WIN_ANSI_ENCODING)),
            map: HashMap::from([(b'a', "\u{03B1}".into())]),
        };
        assert_eq!(differences.decode(b"ab").unwrap(), "αb");
    }

    #[test]
    fn text_strings_decode_by_their_mark() {
        assert_eq!(
            decode_text_string(&[0xFE, 0xFF, 0x06, 0x2A]).as_deref(),
            Some("ت")
        );
        assert_eq!(decode_text_string(b"\xEF\xBB\xBFhi").as_deref(), Some("hi"));
        assert_eq!(
            decode_text_string(b"Plain \x8B").as_deref(),
            Some("Plain \u{2030}")
        );
        assert_eq!(decode_text_string(&[0xFE, 0xFF, 0xD8, 0x00]), None);
    }
}
