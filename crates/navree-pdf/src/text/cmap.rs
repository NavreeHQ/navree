//! `ToUnicode` maps: codes of one to four bytes to UTF-16 text.
//!
//! A map is read token by token: its codespace ranges, `bfchar` lines
//! (a code and its text) and `bfrange` lines (a range of codes and the text
//! of the first, or a list of texts, one per code). Anything else in it is
//! skipped, and so is a line whose codes are not one to four bytes, differ
//! in length, or run backwards. A later line wins over an earlier one for
//! the codes they share. The caps of [`map_shape`](super::map_shape) are
//! checked before a map is read.

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::parse::{Lexer, is_white};

/// The replacement character, for a code a map does not cover.
pub(crate) const REPLACEMENT: u16 = 0xFFFD;

/// What a range of codes maps to.
#[derive(Clone, Debug)]
enum Target {
    /// The text of the first code; each later code adds one to the last
    /// unit.
    Text(Rc<[u16]>),
    /// One text for each code, in order.
    List(Rc<[Rc<[u16]>]>),
}

/// A range of codes, from its first to `last`, and what it maps to; `base`
/// is the first code of the line it came from, which offsets are counted
/// from when a later line splits it.
#[derive(Clone, Debug)]
struct Range {
    last: u32,
    base: u32,
    target: Target,
}

/// A `ToUnicode` map.
#[derive(Debug, Default)]
pub(crate) struct ToUnicode {
    /// Ranges by first code, one map for each code length.
    ranges: [BTreeMap<u32, Range>; 4],
    /// The codespace: (low, high, length).
    codespace: Vec<(u32, u32, u8)>,
}

/// A text in angle brackets: its bytes, and whether it held only hex
/// digits and white space.
fn hex(lexer: &mut Lexer<'_>) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut high: Option<u8> = None;
    lexer.pos += 1;
    loop {
        let byte = lexer.peek()?;
        lexer.pos += 1;
        match byte {
            b'>' => break,
            byte if is_white(byte) => {}
            byte => {
                let value = (byte as char).to_digit(16)? as u8;
                match high.take() {
                    Some(high) => out.push(high << 4 | value),
                    None => high = Some(value),
                }
            }
        }
    }
    if high.is_some() {
        return None;
    }
    Some(out)
}

/// A code of one to four bytes, and its length.
fn code(bytes: &[u8]) -> Option<(u32, u8)> {
    if bytes.is_empty() || bytes.len() > 4 {
        return None;
    }
    let value = bytes
        .iter()
        .fold(0u32, |value, &byte| value << 8 | u32::from(byte));
    Some((value, bytes.len() as u8))
}

/// A text of UTF-16 units; a text with an odd byte is not one.
fn units(bytes: &[u8]) -> Option<Rc<[u16]>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
        return None;
    }
    Some(
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_be_bytes(pair))
            .collect(),
    )
}

/// One token of a map.
enum Token {
    Hex(Option<Vec<u8>>),
    Open,
    Close,
    Word(Vec<u8>),
    Other,
}

fn token(lexer: &mut Lexer<'_>) -> Option<Token> {
    lexer.skip_space();
    let byte = lexer.peek()?;
    Some(match byte {
        b'<' if lexer.starts_with(b"<<") => {
            lexer.pos += 2;
            Token::Other
        }
        b'<' => Token::Hex(hex(lexer)),
        b'[' => {
            lexer.pos += 1;
            Token::Open
        }
        b']' => {
            lexer.pos += 1;
            Token::Close
        }
        b'(' => {
            if lexer.literal_string(&crate::parse::Unmetered).is_err() {
                lexer.pos += 1;
            }
            Token::Other
        }
        b'/' => {
            lexer.name();
            Token::Other
        }
        byte if crate::parse::is_regular(byte) => Token::Word(lexer.word().to_vec()),
        _ => {
            lexer.pos += 1;
            Token::Other
        }
    })
}

impl ToUnicode {
    /// Reads the map in `data`.
    pub(crate) fn read(data: &[u8]) -> ToUnicode {
        #[derive(PartialEq)]
        enum In {
            Nothing,
            Codespace,
            Chars,
            Ranges,
        }
        let mut map = ToUnicode::default();
        let mut section = In::Nothing;
        let mut line: Vec<Option<Vec<u8>>> = Vec::new();
        let mut list: Option<Vec<Rc<[u16]>>> = None;
        let mut list_valid = true;
        let mut lexer = Lexer::new(data, 0);
        while let Some(token) = token(&mut lexer) {
            match token {
                Token::Word(word) => {
                    section = match word.as_slice() {
                        b"begincodespacerange" => In::Codespace,
                        b"beginbfchar" => In::Chars,
                        b"beginbfrange" => In::Ranges,
                        _ => In::Nothing,
                    };
                    line.clear();
                    list = None;
                }
                Token::Hex(bytes) if section == In::Codespace => {
                    line.push(bytes);
                    if let [low, high] = &line[..] {
                        if let (Some((low, a)), Some((high, b))) = (
                            low.as_deref().and_then(code),
                            high.as_deref().and_then(code),
                        ) && a == b
                        {
                            map.codespace.push((low, high, a));
                        }
                        line.clear();
                    }
                }
                Token::Hex(bytes) if section == In::Chars => {
                    line.push(bytes);
                    if let [source, text] = &line[..] {
                        if let (Some((value, length)), Some(text)) = (
                            source.as_deref().and_then(code),
                            text.as_deref().and_then(units),
                        ) {
                            map.put(value, value, length, Target::Text(text));
                        }
                        line.clear();
                    }
                }
                Token::Hex(bytes) if section == In::Ranges => match &mut list {
                    Some(items) => match bytes.as_deref().and_then(units) {
                        Some(text) => items.push(text),
                        None => list_valid = false,
                    },
                    None if line.len() < 2 => line.push(bytes),
                    None => {
                        if let Some(text) = bytes.as_deref().and_then(units) {
                            map.put_line(&line, Target::Text(text));
                        }
                        line.clear();
                    }
                },
                Token::Open if section == In::Ranges && line.len() == 2 && list.is_none() => {
                    list = Some(Vec::new());
                    list_valid = true;
                }
                Token::Close if section == In::Ranges => {
                    if let Some(items) = list.take() {
                        if list_valid && !items.is_empty() {
                            map.put_line(&line, Target::List(items.into()));
                        }
                        line.clear();
                    }
                }
                _ => {}
            }
        }
        map
    }

    /// A `bfrange` line: its two codes, and `target`.
    fn put_line(&mut self, line: &[Option<Vec<u8>>], target: Target) {
        let [Some(low), Some(high)] = line else {
            return;
        };
        let (Some((first, a)), Some((last, b))) = (code(low), code(high)) else {
            return;
        };
        if a == b && first <= last {
            self.put(first, last, a, target);
        }
    }

    /// Maps codes `first` to `last` of `length` bytes to `target`, over
    /// what earlier lines mapped them to.
    fn put(&mut self, first: u32, last: u32, length: u8, target: Target) {
        let Some(ranges) = self.ranges.get_mut(usize::from(length) - 1) else {
            return;
        };
        // Ranges that overlap [first, last]: the one starting before it, and
        // those starting inside it.
        let mut overlapping: Vec<u32> = ranges
            .range(first..=last)
            .map(|(&start, _)| start)
            .collect();
        if let Some((&start, range)) = ranges.range(..first).next_back()
            && range.last >= first
        {
            overlapping.push(start);
        }
        for start in overlapping {
            let Some(range) = ranges.remove(&start) else {
                continue;
            };
            if start < first {
                ranges.insert(
                    start,
                    Range {
                        last: first - 1,
                        ..range.clone()
                    },
                );
            }
            if range.last > last {
                ranges.insert(last + 1, range);
            }
        }
        ranges.insert(
            first,
            Range {
                last,
                base: first,
                target,
            },
        );
    }

    /// The text of `code` of `length` bytes, if a line maps it.
    pub(crate) fn get(&self, code: u32, length: u8) -> Option<Vec<u16>> {
        let ranges = self.ranges.get(usize::from(length).checked_sub(1)?)?;
        let (_, range) = ranges.range(..=code).next_back()?;
        if range.last < code {
            return None;
        }
        let offset = code.wrapping_sub(range.base);
        Some(match &range.target {
            Target::Text(text) => {
                let mut text = text.to_vec();
                if let Some(last) = text.last_mut() {
                    *last = last.wrapping_add(offset as u16);
                }
                text
            }
            Target::List(items) => match items.get(offset as usize) {
                Some(text) => text.to_vec(),
                None => vec![REPLACEMENT],
            },
        })
    }

    /// The text of `code` when it is a whole code: one in the codespace (or
    /// any code a line maps, when the map has no codespace); the
    /// replacement character for a whole code no line maps.
    fn decoding(&self, code: u32, length: u8) -> Option<Vec<u16>> {
        if self.codespace.is_empty() {
            return self.get(code, length);
        }
        let inside = self.codespace.iter().any(|&(low, high, size)| {
            size == length
                && (0..u32::from(length)).all(|i| {
                    let shift = 8 * i;
                    let byte = (code >> shift) as u8;
                    (low >> shift) as u8 <= byte && byte <= (high >> shift) as u8
                })
        });
        inside.then(|| self.get(code, length).unwrap_or_else(|| vec![REPLACEMENT]))
    }

    /// The text of `bytes`: codes are read one byte at a time until a
    /// whole code is found, up to four bytes.
    pub(crate) fn decode(&self, bytes: &[u8]) -> String {
        let mut units = Vec::new();
        let (mut length, mut code) = (0u8, 0u32);
        for &byte in bytes {
            if length == 4 {
                units.extend(self.get(code, 4).unwrap_or_else(|| vec![REPLACEMENT]));
                (length, code) = (0, 0);
            }
            length += 1;
            code = code << 8 | u32::from(byte);
            if let Some(text) = self.decoding(code, length) {
                units.extend(text);
                (length, code) = (0, 0);
            }
        }
        if length > 0 {
            units.extend(self.get(code, length).unwrap_or_else(|| vec![REPLACEMENT]));
        }
        utf16_with_bom(&units)
    }
}

/// UTF-16 text; a byte-order mark at the start picks the encoding of the
/// rest (UTF-16 in either order, or UTF-8). Unpaired surrogates become
/// replacement characters.
pub(crate) fn utf16_with_bom(units: &[u16]) -> String {
    let bytes: Vec<u8> = units.iter().flat_map(|unit| unit.to_be_bytes()).collect();
    utf16_bytes_with_bom(&bytes)
}

/// UTF-16BE `bytes` with the same rules as [`utf16_with_bom`]; an odd byte
/// at the end is a replacement character.
pub(crate) fn utf16_bytes_with_bom(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let (rest, little) = match bytes {
        [0xFE, 0xFF, rest @ ..] => (rest, false),
        [0xFF, 0xFE, rest @ ..] => (rest, true),
        rest => (rest, false),
    };
    let (pairs, odd) = rest.as_chunks::<2>();
    let units: Vec<u16> = pairs
        .iter()
        .map(|&pair| {
            if little {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        })
        .collect();
    let mut text = String::from_utf16_lossy(&units);
    if !odd.is_empty() {
        text.push('\u{FFFD}');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
                        /CMapName /Sample def\n1 begincodespacerange <0000> <FFFF> endcodespacerange\n";

    fn read_map(body: &str) -> ToUnicode {
        ToUnicode::read(
            format!("{HEAD}{body}endcmap CMapName currentdict /CMap defineresource pop end end")
                .as_bytes(),
        )
    }

    #[test]
    fn chars_ranges_and_lists() {
        let map = read_map(
            "2 beginbfchar <0001> <0041> <0002> <00660069> endbfchar\n\
                       2 beginbfrange <0010> <0012> <0061> <0020> <0021> [<0058> <00590059>] endbfrange\n",
        );
        assert_eq!(map.decode(&[0, 1]), "A");
        assert_eq!(map.decode(&[0, 2]), "fi");
        assert_eq!(map.decode(&[0, 0x12]), "c");
        assert_eq!(map.decode(&[0, 0x21]), "YY");
        // In the codespace, mapped by nothing: the replacement character.
        assert_eq!(map.decode(&[0, 0x30]), "\u{FFFD}");
    }

    #[test]
    fn a_later_line_wins_and_splits_keep_their_offsets() {
        let map = read_map(
            "1 beginbfrange <0000> <00FF> [<0041> <0042> <0043> <0044>] endbfrange\n\
                       1 beginbfchar <0001> <005A> endbfchar\n",
        );
        assert_eq!(map.decode(&[0, 0]), "A");
        assert_eq!(map.decode(&[0, 1]), "Z");
        assert_eq!(map.decode(&[0, 2]), "C");
        assert_eq!(map.decode(&[0, 3]), "D");
        assert_eq!(map.decode(&[0, 9]), "\u{FFFD}");
        let map = read_map(
            "1 beginbfrange <0000> <0010> <00410041> endbfrange\n\
                       1 beginbfchar <0005> <005A> endbfchar\n",
        );
        assert_eq!(map.decode(&[0, 7]), "AH");
    }

    #[test]
    fn codes_are_read_by_the_codespace() {
        let map = ToUnicode::read(
            b"2 begincodespacerange <00> <7F> <8000> <FFFF> endcodespacerange\n\
              2 beginbfchar <41> <0041> <8140> <3000> endbfchar\n",
        );
        assert_eq!(map.decode(&[0x41, 0x81, 0x40]), "A\u{3000}");
        // Without a codespace, codes grow a byte at a time until one maps.
        let open = ToUnicode::read(b"1 beginbfchar <0102> <0042> endbfchar");
        assert_eq!(open.decode(&[1, 2]), "B");
    }

    #[test]
    fn bad_lines_are_skipped_not_the_map() {
        let map = read_map(
            "3 beginbfrange <0001> <01> <0041> <0005> <0001> <0041> <0002> <0003> <0061> endbfrange\n\
                       1 beginbfchar <0009> <004> endbfchar\n",
        );
        assert_eq!(map.decode(&[0, 3]), "b");
        assert_eq!(map.decode(&[0, 9]), "\u{FFFD}");
    }

    #[test]
    fn byte_order_marks_pick_the_encoding() {
        assert_eq!(utf16_with_bom(&[0xFEFF, 0x0041]), "A");
        assert_eq!(utf16_bytes_with_bom(&[0xFF, 0xFE, 0x41, 0x00]), "A");
        assert_eq!(utf16_bytes_with_bom(&[0xEF, 0xBB, 0xBF, b'h', b'i']), "hi");
        assert_eq!(utf16_bytes_with_bom(&[0x00, 0x41, 0x00]), "A\u{FFFD}");
        assert_eq!(utf16_with_bom(&[0xD800]), "\u{FFFD}");
    }
}
