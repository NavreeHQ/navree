//! How a font's codes decode to text: a standard encoding, an encoding's
//! `Differences` over a base, a `ToUnicode` map, or a predefined UTF-16
//! CMap.
//!
//! A font's `Encoding` decides: a standard encoding's name; `Identity-H`
//! or `Identity-V`, which read the `ToUnicode` map; a dictionary with a
//! `BaseEncoding` (standard when it has none) and `Differences`; or the
//! name of a predefined CMap (`UniGB-UCS2-H` and `UniGB-UTF16-H` are read
//! as UTF-16; others decode nothing). A font without an `Encoding` reads
//! its `ToUnicode` map, or else the standard encoding. An encoding that
//! cannot be read (a cycle of references, an unknown glyph name in
//! `Differences`, a missing map) falls back to the standard encoding.

use std::collections::{HashMap, HashSet};

use crate::document::{ContentError, Document};
use crate::object::{Dictionary, Object, Stream};

use super::cmap::{ToUnicode, utf16_bytes_with_bom};
use super::tables;

/// A font's encoding.
pub(crate) enum Encoding<'a> {
    /// A table of 256 codes (0 for none).
    Table(&'static [u16; 256]),
    /// A predefined CMap or another name.
    Named(&'a [u8]),
    /// A `ToUnicode` map.
    Map(ToUnicode),
    /// `Differences` over a base.
    Differences {
        base: Box<Encoding<'a>>,
        map: HashMap<u8, u16>,
    },
}

/// Why an encoding could not be had.
pub(crate) enum Unreadable {
    /// The `ToUnicode` map inflates past the limit.
    Over,
    /// Anything else: the standard encoding is used.
    Other,
}

/// The UTF-16 unit of the glyph called `name`.
pub(crate) fn glyph(name: &[u8]) -> Option<u16> {
    tables::GLYPHS
        .binary_search_by(|(known, _)| known.as_bytes().cmp(name))
        .ok()
        .and_then(|index| tables::GLYPHS.get(index))
        .map(|(_, unit)| *unit)
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
            Encoding::Map(map) => out.push_str(&map.decode(bytes)),
            Encoding::Differences { base, map } => {
                for &byte in bytes {
                    match map.get(&byte) {
                        Some(&unit) => out.extend(char::decode_utf16([unit]).flatten()),
                        None => base.decode_into(&[byte], out)?,
                    }
                }
            }
        }
        Some(())
    }
}

/// The encoding of `font`; an error only for a map over `limit` (other
/// failures read as the standard encoding), or for an object that is not
/// a font.
pub(crate) fn font_encoding<'a>(
    document: &'a Document,
    font: &'a Dictionary,
    limit: usize,
) -> Result<Encoding<'a>, Unreadable> {
    if !font.has_type(b"Font") {
        return Err(Unreadable::Other);
    }
    let result = match font.get(b"Encoding") {
        Some(encoding) => base_of(document, font, encoding, limit),
        None => match document.get(font, b"ToUnicode").and_then(Object::as_stream) {
            Some(stream) => to_unicode(document, stream, limit),
            None => Ok(Encoding::Table(&tables::STANDARD_ENCODING)),
        },
    };
    match result {
        Err(Unreadable::Over) => Err(Unreadable::Over),
        Err(Unreadable::Other) => Ok(Encoding::Table(&tables::STANDARD_ENCODING)),
        Ok(encoding) => Ok(encoding),
    }
}

/// The encoding an `Encoding` entry stands for.
fn base_of<'a>(
    document: &'a Document,
    font: &'a Dictionary,
    mut encoding: &'a Object,
    limit: usize,
) -> Result<Encoding<'a>, Unreadable> {
    let mut seen = HashSet::new();
    loop {
        match encoding {
            Object::Name(name) => return named(document, font, name, limit),
            Object::Reference(id) => {
                if !seen.insert(*id) {
                    return Err(Unreadable::Other);
                }
                encoding = document.object(*id).ok_or(Unreadable::Other)?;
            }
            Object::Dictionary(dict) => {
                if !matches!(dict.type_name(), None | Some(b"Encoding")) {
                    return Err(Unreadable::Other);
                }
                let base = match dict.get(b"BaseEncoding").and_then(Object::as_name) {
                    Some(name) => named(document, font, name, limit)?,
                    None => Encoding::Table(&tables::STANDARD_ENCODING),
                };
                let Some(differences) = dict.get(b"Differences") else {
                    return Ok(base);
                };
                let items = differences.as_array().ok_or(Unreadable::Other)?;
                let mut map = HashMap::new();
                let mut code = 0u8;
                for item in items {
                    match item {
                        Object::Integer(value) => {
                            code = u8::try_from(*value).map_err(|_| Unreadable::Other)?;
                        }
                        Object::Name(name) => {
                            map.insert(code, glyph(name).ok_or(Unreadable::Other)?);
                            code = code.wrapping_add(1);
                        }
                        _ => return Err(Unreadable::Other),
                    }
                }
                return Ok(Encoding::Differences {
                    base: Box::new(base),
                    map,
                });
            }
            _ => return Err(Unreadable::Other),
        }
    }
}

fn named<'a>(
    document: &'a Document,
    font: &'a Dictionary,
    name: &'a [u8],
    limit: usize,
) -> Result<Encoding<'a>, Unreadable> {
    Ok(match name {
        b"StandardEncoding" => Encoding::Table(&tables::STANDARD_ENCODING),
        b"MacRomanEncoding" => Encoding::Table(&tables::MAC_ROMAN_ENCODING),
        b"MacExpertEncoding" => Encoding::Table(&tables::MAC_EXPERT_ENCODING),
        b"WinAnsiEncoding" => Encoding::Table(&tables::WIN_ANSI_ENCODING),
        b"PDFDocEncoding" => Encoding::Table(&tables::PDF_DOC_ENCODING),
        b"Identity-H" | b"Identity-V" => {
            let stream = document
                .get(font, b"ToUnicode")
                .and_then(Object::as_stream)
                .ok_or(Unreadable::Other)?;
            return to_unicode(document, stream, limit);
        }
        other => Encoding::Named(other),
    })
}

/// The map in `stream`, inflated no further than `limit`.
pub(crate) fn to_unicode<'a>(
    document: &Document,
    stream: &Stream,
    limit: usize,
) -> Result<Encoding<'a>, Unreadable> {
    match document.decode(stream, limit) {
        Ok(data) => Ok(Encoding::Map(ToUnicode::read(&data))),
        Err(ContentError::Over) => Err(Unreadable::Over),
        Err(_) => Err(Unreadable::Other),
    }
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
            map: HashMap::from([(b'a', 0x03B1)]),
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
