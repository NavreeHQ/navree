//! `ToUnicode` maps: codes of one to four bytes to UTF-16 text.
//!
//! A map is read token by token: its codespace ranges, `bfchar` lines
//! (a code and its text) and `bfrange` lines (a range of codes and the text
//! of the first, or a list of texts, one per code). Anything else in it is
//! skipped, and so is a line whose codes are not one to four bytes, differ
//! in length, or run backwards. A later line wins over an earlier one for
//! the codes they share. A string that never ends ends the map.
//!
//! The caps are enforced as the map is read, by the one tokenizer that reads
//! it: what counts against them is what the reader sees. Reading is one pass
//! over the map, and [`ToUnicode::read_within`] returns what the map is
//! counted as holding.

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::error::PdfError;
use crate::parse::{Lexer, is_white};

// Caps on what a ToUnicode map may hold, enforced as it is read. Real maps
// stay far below each; a map over one is refused.

/// Codespace ranges in one map (64). Every one is checked for each code
/// decoded; real maps have one to four.
pub(super) const MAX_CODESPACE_RANGES: usize = 64;
/// Lines that map codes (`bfchar` and `bfrange` entries) in one map
/// (131,072): twice one line for every code of one or two bytes.
pub(super) const MAX_MAP_LINES: usize = 131_072;
/// Texts in the list of one range (256). A range written to the
/// specification varies only its last byte, so it has at most 256 codes,
/// one text for each.
pub(super) const MAX_LIST_ITEMS: usize = 256;
/// Texts in all the lists of one map (65,792): one for every code of one
/// or two bytes.
pub(super) const MAX_MAP_LIST_ITEMS: usize = 65_792;

// What a ToUnicode map is counted as holding: a bound on what reading builds,
// so that a map of a few bytes that covers millions of codes is refused.

/// For each byte of the map: the lines it parses before it builds the map.
const MAP_TEXT_MEMORY: u64 = 8;
/// For each range it keeps, besides the text the range maps to.
const MAP_RANGE_MEMORY: u64 = 96;
/// For each code it covers, besides the code's text.
pub(super) const MAP_CODE_MEMORY: u64 = 256;
/// For each text of more than one UTF-16 unit, besides its units.
const MAP_TEXT_VALUE_MEMORY: u64 = 32;
/// For each text of a list (`[<..> <..>]`), besides its units.
const MAP_LIST_ITEM_MEMORY: u64 = 56;
/// Codes of one or two bytes there can be.
const SHORT_CODES: u64 = 256 + 65_536;

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

/// A text in angle brackets: its bytes, when it holds only hex digits, white
/// space and comments, a whole number of bytes. The lexer stops after the
/// first byte that is not one of those, or after the closing bracket.
fn hex(lexer: &mut Lexer<'_>) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut high: Option<u8> = None;
    lexer.pos += 1;
    loop {
        let byte = lexer.peek()?;
        lexer.pos += 1;
        match byte {
            b'>' => break,
            b'%' => {
                while lexer
                    .peek()
                    .is_some_and(|byte| byte != b'\r' && byte != b'\n')
                {
                    lexer.pos += 1;
                }
            }
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
        // A string that cannot be read ends the map: what follows it is
        // inside it, and is not looked at again.
        b'(' => {
            lexer.literal_string(&crate::parse::Unmetered).ok()?;
            Token::Other
        }
        b'/' => {
            lexer.name();
            // A `#` that is not an escape does not make a second token.
            lexer.word();
            Token::Other
        }
        byte if crate::parse::is_regular(byte) => Token::Word(lexer.word().to_vec()),
        _ => {
            lexer.pos += 1;
            Token::Other
        }
    })
}

fn map_over(what: &str, cap: usize) -> PdfError {
    PdfError::TooLarge(format!("a font's ToUnicode map has more than {cap} {what}"))
}

/// What a text of `units` UTF-16 units is counted as: one unit is kept as
/// a number.
fn text_memory(units: u64) -> u64 {
    if units <= 1 {
        0
    } else {
        MAP_TEXT_VALUE_MEMORY + 2 * units
    }
}

/// What a map is counted as holding, as it is read.
#[derive(Default)]
struct Counted {
    codespace: usize,
    lines: usize,
    list_items: usize,
    /// The most any one text is counted as.
    text_most: u64,
    /// What the texts of `bfchar` lines and of lists are counted as.
    code_memory: u64,
    /// The texts of more than one unit that lines map to, with the codes of
    /// each line: what splits may copy, and how often.
    copied: Vec<(u64, u64)>,
    /// What the codes of one or two bytes are counted as, and the most one
    /// of them is.
    short: u64,
    short_most: u64,
    /// What longer codes are counted as.
    long: u64,
}

impl Counted {
    /// Counts one more line, refused past the cap.
    fn line(&mut self) -> Result<(), PdfError> {
        self.lines += 1;
        if self.lines > MAX_MAP_LINES {
            return Err(map_over("lines", MAX_MAP_LINES));
        }
        Ok(())
    }

    /// Counts a text of `units` that a `bfchar` line maps its code to.
    fn char_line(&mut self, units: u64) {
        let value = text_memory(units);
        self.text_most = self.text_most.max(value);
        if value > 0 {
            self.copied.push((value, 1));
        }
        self.code_memory = self.code_memory.saturating_add(MAP_CODE_MEMORY + 2 * units);
    }

    /// Counts the codes `first` to `last` of `length` bytes that a `bfrange`
    /// line maps, each to a text of `units`, the line's own text counted as
    /// `value`: `short` for codes of one or two bytes, `long` for longer ones.
    fn range_line(&mut self, first: u32, last: u32, length: u8, units: u64, value: u64) {
        let codes = u64::from(last - first) + 1;
        let per_code = MAP_CODE_MEMORY + 2 * units;
        let memory = codes.saturating_mul(per_code);
        if length <= 2 {
            self.short = self.short.saturating_add(memory);
            self.short_most = self.short_most.max(per_code);
        } else {
            self.long = self.long.saturating_add(memory);
        }
        self.text_most = self.text_most.max(value);
        if value > 0 {
            self.copied.push((value, codes));
        }
    }

    /// What is known to be held so far: what the map's bytes and lines build,
    /// and the texts it keeps.
    fn built(&self, bytes: usize) -> u64 {
        MAP_TEXT_MEMORY
            .saturating_mul(bytes as u64)
            .saturating_add((2 * self.lines as u64).saturating_mul(MAP_RANGE_MEMORY))
            .saturating_add(self.code_memory)
    }

    /// The memory counted for the whole map. It counts
    /// [`MAP_TEXT_MEMORY`] for each byte of the map; for each line, two ranges
    /// of [`MAP_RANGE_MEMORY`]; for each code a range covers,
    /// [`MAP_CODE_MEMORY`] and its text (codes of one or two bytes are at most
    /// 65,792, however many lines cover them); each text of a list; and two
    /// copies of a text longer than one unit for each split of its range.
    ///
    /// A later line that lands inside a range splits it in two. Each line
    /// splits one range at most, and a range can be split no more often than
    /// it has codes. So the splits, as many as the lines at most, are given to
    /// the longest texts first, each counting two copies, plus two copies of
    /// the longest made in passing.
    fn memory(mut self, bytes: usize) -> u64 {
        let built = self.built(bytes);
        let short = self.short.min(SHORT_CODES.saturating_mul(self.short_most));
        self.copied
            .sort_unstable_by_key(|&(value, _)| std::cmp::Reverse(value));
        let (mut copies, mut splits) = (2 * self.text_most, self.lines as u64);
        for (value, codes) in self.copied {
            let these = splits.min(codes.max(1));
            copies = copies.saturating_add((2 * value).saturating_mul(these));
            splits -= these;
            if splits == 0 {
                break;
            }
        }
        built
            .saturating_add(copies)
            .saturating_add(short)
            .saturating_add(self.long)
    }
}

impl ToUnicode {
    /// Reads the map in `data`, and what it is counted as holding. Refused,
    /// as it reads, past one of the caps, or when what it has built passes
    /// `limit`.
    pub(crate) fn read_within(data: &[u8], limit: usize) -> Result<(ToUnicode, u64), PdfError> {
        #[derive(PartialEq)]
        enum In {
            Nothing,
            Codespace,
            Chars,
            Ranges,
        }
        let mut map = ToUnicode::default();
        let mut counted = Counted::default();
        let mut section = In::Nothing;
        let mut line: Vec<Option<Vec<u8>>> = Vec::new();
        let mut list: Option<Vec<Rc<[u16]>>> = None;
        let mut list_valid = true;
        // What the list being read is counted as, and its longest text.
        let (mut list_memory, mut list_most) = (0u64, 0u64);
        let mut lexer = Lexer::new(data, 0);
        while let Some(token) = token(&mut lexer) {
            if counted.built(data.len()) > limit as u64 {
                return Err(super::font_too_large(limit));
            }
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
                        counted.codespace += 1;
                        if counted.codespace > MAX_CODESPACE_RANGES {
                            return Err(map_over("codespace ranges", MAX_CODESPACE_RANGES));
                        }
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
                        counted.line()?;
                        if let (Some((value, length)), Some(text)) = (
                            source.as_deref().and_then(code),
                            text.as_deref().and_then(units),
                        ) {
                            counted.char_line(text.len() as u64);
                            map.put(value, value, length, Target::Text(text));
                        }
                        line.clear();
                    }
                }
                Token::Hex(bytes) if section == In::Ranges => match &mut list {
                    Some(items) => {
                        counted.list_items += 1;
                        if counted.list_items > MAX_MAP_LIST_ITEMS {
                            return Err(map_over("texts in lists", MAX_MAP_LIST_ITEMS));
                        }
                        match bytes.as_deref().and_then(units) {
                            Some(text) => {
                                list_memory += MAP_LIST_ITEM_MEMORY + 2 * text.len() as u64;
                                list_most = list_most.max(text.len() as u64);
                                items.push(text);
                                if items.len() > MAX_LIST_ITEMS {
                                    return Err(map_over("texts in one list", MAX_LIST_ITEMS));
                                }
                            }
                            None => list_valid = false,
                        }
                    }
                    None if line.len() < 2 => line.push(bytes),
                    None => {
                        counted.line()?;
                        if let Some(text) = bytes.as_deref().and_then(units) {
                            if let Some((first, last, length)) = codes(&line) {
                                let units = text.len() as u64;
                                counted.range_line(first, last, length, units, text_memory(units));
                            }
                            map.put_line(&line, Target::Text(text));
                        }
                        line.clear();
                    }
                },
                Token::Open if section == In::Ranges && line.len() == 2 && list.is_none() => {
                    list = Some(Vec::new());
                    list_valid = true;
                    (list_memory, list_most) = (MAP_TEXT_VALUE_MEMORY, 0);
                }
                Token::Close if section == In::Ranges => {
                    if let Some(items) = list.take() {
                        counted.line()?;
                        counted.code_memory = counted.code_memory.saturating_add(list_memory);
                        if list_valid && !items.is_empty() {
                            if let Some((first, last, length)) = codes(&line) {
                                counted.range_line(first, last, length, list_most, list_memory);
                            }
                            map.put_line(&line, Target::List(items.into()));
                        }
                        line.clear();
                    }
                }
                _ => {}
            }
        }
        let memory = counted.memory(data.len());
        Ok((map, memory))
    }

    /// A `bfrange` line: its two codes, and `target`.
    fn put_line(&mut self, line: &[Option<Vec<u8>>], target: Target) {
        if let Some((first, last, length)) = codes(line) {
            self.put(first, last, length, target);
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

/// The two codes of a `bfrange` line, and their length, when it is read: codes
/// of one length, in order.
fn codes(line: &[Option<Vec<u8>>]) -> Option<(u32, u32, u8)> {
    let [Some(low), Some(high)] = line else {
        return None;
    };
    let ((first, a), (last, b)) = (code(low)?, code(high)?);
    (a == b && first <= last).then_some((first, last, a))
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

    impl ToUnicode {
        /// Reads the map in `data`, within any limit.
        pub(crate) fn read(data: &[u8]) -> ToUnicode {
            ToUnicode::read_within(data, usize::MAX).unwrap().0
        }
    }

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
