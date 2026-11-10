//! The cross-reference: tables, cross-reference streams, the `/Prev`
//! chain of incremental updates, and the scan that rebuilds it when it is
//! broken. Also the reading of one indirect object, which the
//! cross-reference streams need before there is a cross-reference.

use std::collections::btree_map::Entry as Place;
use std::collections::{BTreeMap, HashSet};

use bytes::Bytes;

use crate::budget::Budget;
use crate::error::PdfError;
use crate::filters::{self, DecodeError};
use crate::object::{Dictionary, Object, ObjectId, Stream};
use crate::parse::{Fail, Lexer, MAX_DEPTH, Meter};

/// Where an object is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    /// Free: no object.
    Free,
    /// At `offset` in the file.
    Normal { offset: usize, generation: u16 },
    /// The `index`th object of the object stream numbered `container`.
    Compressed { container: u32, index: u32 },
}

/// What an entry of the cross-reference holds in memory, counted for each.
pub(crate) const ENTRY_MEMORY: usize = 48;
/// What each known start of an object holds in memory, with room to grow.
pub(crate) const OFFSET_MEMORY: usize = 16;
/// What keeping an entry of the cross-reference costs, in bytes of work.
const ENTRY_COST: usize = 16;
/// Object numbers a damaged file's scan accepts (up to 8,388,607): a
/// forged header cannot make the table larger than objects a file can
/// hold.
const MAX_SCANNED_NUMBER: u32 = (1 << 23) - 1;

/// How far after an `endstream` its `endobj` is looked for (64 bytes).
const AFTER_ENDSTREAM: usize = 64;

/// The cross-reference and the trailer.
pub(crate) struct Xref {
    pub(crate) entries: BTreeMap<u32, Entry>,
    pub(crate) trailer: Dictionary,
    /// Where the last cross-reference section starts (bounds the last
    /// object before it).
    pub(crate) starts: Vec<usize>,
    /// Whether it was rebuilt by scanning the file.
    pub(crate) rebuilt: bool,
    /// Where entries find their own object, in no order: where an object
    /// starts, and so where the one before it ends.
    pub(crate) located: Vec<usize>,
    /// Objects a section marks free that an older section has an entry for.
    pub(crate) freed: HashSet<u32>,
}

/// The memory a parse builds for each byte of work it is charged (32): small
/// objects take longer to build than to look at.
const MEMORY_PER_WORK: usize = 32;

impl Meter for Budget {
    fn charge(&self, bytes: usize) -> Result<(), PdfError> {
        self.memory(bytes)?;
        self.work(bytes / MEMORY_PER_WORK)
    }
}

/// Finds `needle` in `haystack` from `from`, the first one.
pub(crate) fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| at + from)
}

/// Finds the last `needle` in `haystack[from..]`.
fn rfind(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .rposition(|window| window == needle)
        .map(|at| at + from)
}

/// The offset `startxref` gives: the last `%%EOF` in the last 1,024 bytes
/// (white space after it allowed), and `startxref` in the 25 bytes before
/// it.
fn start(data: &[u8], budget: &Budget) -> Result<Option<usize>, PdfError> {
    let end = data
        .iter()
        .rposition(|&byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\0' | b'\x0C'))
        .map_or(0, |at| at + 1);
    budget.work(data.len() - end + 1024)?;
    Ok(start_within(data, end))
}

/// The offset `startxref` gives in `data` up to `end`.
fn start_within(data: &[u8], end: usize) -> Option<usize> {
    let data = data.get(..end)?;
    let eof = rfind(data, b"%%EOF", end.saturating_sub(1024)).filter(|&at| at > 25)?;
    let keyword = rfind(data.get(..eof)?, b"startxref", eof - 25)?;
    let mut lexer = Lexer::new(data, keyword + b"startxref".len());
    lexer.skip_space();
    usize::try_from(lexer.unsigned()?).ok()
}

/// `N G obj` at the lexer: the object id, with the lexer past `obj`.
pub(crate) fn header(lexer: &mut Lexer<'_>) -> Option<ObjectId> {
    let start = lexer.pos;
    let id = (|| {
        let number = u32::try_from(lexer.unsigned()?).ok()?;
        lexer.skip_space();
        let generation = u16::try_from(lexer.unsigned()?).ok()?;
        lexer.skip_space();
        lexer.eat(b"obj").then_some((number, generation))
    })();
    if id.is_none() {
        lexer.rewind(start);
    }
    id
}

/// Reads the indirect object at `offset`, whose bytes end by `end` at the
/// latest (where the next object starts). A stream's `Length` comes from
/// the dictionary or, through `length`, from the object it refers to; a
/// stream whose length is wrong or missing, or runs past `end`, ends at the
/// `endstream` before `end`: no stream holds the bytes of the object after
/// it. Returns `None` for bytes that are not an object; an object with nothing
/// in it reads as null. Fails only when a limit runs out. What it looks at is
/// charged, whether it finds an object or not.
pub(crate) fn indirect(
    data: &Bytes,
    offset: usize,
    end: usize,
    budget: &Budget,
    length: &dyn Fn(ObjectId) -> Option<i64>,
) -> Result<Option<(ObjectId, Object)>, PdfError> {
    let mut lexer = Lexer::new(data, offset);
    lexer.skip_space();
    let Some(id) = header(&mut lexer) else {
        budget.work(lexer.reached().saturating_sub(offset))?;
        return Ok(None);
    };
    lexer.skip_space();
    if lexer.starts_with(b"endobj") {
        budget.work(lexer.reached().saturating_sub(offset))?;
        return Ok(Some((id, Object::Null)));
    }
    let parsed = lexer.object(MAX_DEPTH, budget);
    budget.work(lexer.reached().saturating_sub(offset))?;
    let object = match parsed {
        Ok(object) => object,
        Err(Fail::Over(error)) => return Err(error),
        Err(Fail::Syntax | Fail::Deep) => return Ok(None),
    };
    let Object::Dictionary(dict) = object else {
        return Ok(Some((id, object)));
    };
    if !lexer.eat(b"stream") {
        return Ok(Some((id, Object::Dictionary(dict))));
    }
    while matches!(lexer.peek(), Some(b' ' | b'\t')) {
        lexer.pos += 1;
    }
    lexer.eat_eol();
    let start = lexer.pos;
    let declared = match dict.get(b"Length") {
        Some(Object::Reference(target)) => length(*target),
        Some(Object::Integer(value)) => Some(*value),
        Some(Object::Real(value)) if value.fract() == 0.0 => Some(*value as i64),
        _ => None,
    };
    let declared = declared.and_then(|value| usize::try_from(value).ok());
    if let Some(declared) = declared
        && let Some(stop) = start.checked_add(declared)
        && stop <= end.min(data.len())
    {
        let mut after = Lexer::new(data, stop);
        after.eat_eol();
        if after.starts_with(b"endstream") {
            let content = data.slice(start..stop);
            return Ok(Some((id, Stream::new(dict, content).into())));
        }
    }
    // The length is missing or wrong: the data ends at the `endstream` that
    // closes it, before the next object.
    let bound = end.clamp(start, data.len());
    let window = data.get(start..bound).unwrap_or_default();
    let (stop, scanned) = stream_end(window);
    budget.work(scanned)?;
    let Some(stop) = stop else {
        return Ok(None);
    };
    let content = data.slice(start..start + stop);
    Ok(Some((id, Stream::new(dict, content).into())))
}

/// Where the data of a stream whose length is unknown ends in `window`
/// (which starts with the data): before the end of line that precedes an
/// `endstream`, preferring one followed by `endobj` within
/// [`AFTER_ENDSTREAM`] bytes. Also returns how far it looked.
fn stream_end(window: &[u8]) -> (Option<usize>, usize) {
    let mut first = None;
    let mut from = 0;
    while let Some(at) = find(window, b"endstream", from) {
        from = at + 1;
        let before = window.get(..at).unwrap_or_default();
        let stop = if before.ends_with(b"\r\n") {
            at - 2
        } else if before.ends_with(b"\n") || before.ends_with(b"\r") {
            at - 1
        } else {
            at
        };
        // A comment may run on to the next `endstream`: the look ahead
        // has an end, so that each one costs the same.
        let next = at + b"endstream".len();
        let near = window
            .get(..next.saturating_add(AFTER_ENDSTREAM).min(window.len()))
            .unwrap_or_default();
        let mut after = Lexer::new(near, next);
        after.skip_space();
        if after.starts_with(b"endobj") {
            return (Some(stop), at);
        }
        first.get_or_insert(stop);
    }
    (first, window.len())
}

/// Reads the cross-reference of `data`, following `/Prev` and `/XRefStm`,
/// or rebuilds it by scanning the file when it cannot be read.
pub(crate) fn read(data: &Bytes, budget: &Budget) -> Result<Xref, PdfError> {
    match chain(data, budget) {
        Ok(mut xref) => {
            let (mostly, located) = locates_objects(data, &xref, budget)?;
            xref.located = located;
            if mostly {
                Ok(xref)
            } else {
                Ok(rebuild(data, budget)?.unwrap_or(xref))
            }
        }
        Err(PdfError::TooLarge(message)) => Err(PdfError::TooLarge(message)),
        Err(error) => rebuild(data, budget)?.ok_or(error),
    }
}

/// The sections from `startxref` back through `/Prev`, newer entries first.
fn chain(data: &Bytes, budget: &Budget) -> Result<Xref, PdfError> {
    let first =
        start(data, budget)?.ok_or_else(|| PdfError::damaged("the file has no startxref"))?;
    let mut entries = BTreeMap::new();
    let mut trailer: Option<Dictionary> = None;
    let mut starts = Vec::new();
    let mut freed = HashSet::new();
    let mut seen = HashSet::new();
    let mut next = Some(first);
    while let Some(offset) = next.take() {
        if !seen.insert(offset) {
            break;
        }
        if seen.len() > budget.limits.sections {
            return Err(PdfError::TooLarge(format!(
                "a PDF has more than {} cross-reference sections",
                budget.limits.sections
            )));
        }
        let (offset, mut section_trailer) = section(data, offset, budget, &mut entries, &mut freed)
            .ok_or_else(|| PdfError::damaged("the cross-reference cannot be read"))??;
        starts.push(offset);
        // A hybrid file's stream comes after its table, before older ones.
        if let Some(stream) = section_trailer.remove(b"XRefStm").and_then(|o| o.as_i64())
            && let Ok(stream) = usize::try_from(stream)
            && seen.insert(stream)
            && let Some(Ok((at, _))) = section(data, stream, budget, &mut entries, &mut freed)
        {
            starts.push(at);
        }
        next = section_trailer.remove(b"Prev").and_then(|prev| {
            // An integer exactly; a few writers put a real here.
            match prev.as_i64() {
                Some(prev) => usize::try_from(prev).ok(),
                None => prev
                    .as_float()
                    .filter(|prev| *prev >= 0.0)
                    .map(|prev| prev as usize),
            }
        });
        if let Some(prev) = next
            && prev > data.len()
        {
            return Err(PdfError::damaged(
                "a cross-reference section starts past the end",
            ));
        }
        trailer.get_or_insert(section_trailer);
    }
    let mut trailer = trailer.ok_or_else(|| PdfError::damaged("the file has no trailer"))?;
    trailer.remove(b"Prev");
    trailer.remove(b"XRefStm");
    Ok(Xref {
        entries,
        trailer,
        starts,
        rebuilt: false,
        located: Vec::new(),
        freed,
    })
}

/// Reads one section at `offset` (or within 64 bytes of it, for a writer
/// that got it slightly wrong) into `entries`, keeping entries already
/// there: they are newer. An object a newer section marks free stays readable
/// where this one says it is, and goes in `freed`: a file can still refer to
/// it. Returns where it started and its trailer; `None` when there is no
/// section there.
fn section(
    data: &Bytes,
    offset: usize,
    budget: &Budget,
    entries: &mut BTreeMap<u32, Entry>,
    freed: &mut HashSet<u32>,
) -> Option<Result<(usize, Dictionary), PdfError>> {
    let mut candidates = vec![offset];
    let window = offset.saturating_sub(64)..offset.saturating_add(64).min(data.len());
    let mut near: Vec<usize> = window
        .filter(|&at| at != offset && section_starts(data, at))
        .collect();
    near.sort_by_key(|&at| at.abs_diff(offset));
    candidates.extend(near);
    for at in candidates {
        let mut found = BTreeMap::new();
        let result = if data.get(at..).is_some_and(|rest| rest.starts_with(b"xref")) {
            table(data, at, budget, &mut found)
        } else {
            stream(data, at, budget, &mut found)
        };
        match result {
            Ok(Some(trailer)) => {
                for (number, entry) in found {
                    match entries.entry(number) {
                        Place::Vacant(place) => {
                            place.insert(entry);
                        }
                        Place::Occupied(mut place)
                            if *place.get() == Entry::Free && entry != Entry::Free =>
                        {
                            place.insert(entry);
                            freed.insert(number);
                        }
                        Place::Occupied(_) => {}
                    }
                }
                return Some(Ok((at, trailer)));
            }
            Ok(None) => continue,
            Err(error) => return Some(Err(error)),
        }
    }
    None
}

/// Whether a section can start at `at`: `xref`, or an object header.
fn section_starts(data: &[u8], at: usize) -> bool {
    let Some(rest) = data.get(at..) else {
        return false;
    };
    if rest.starts_with(b"xref") {
        // Not the `xref` inside `startxref`.
        return !data
            .get(..at)
            .is_some_and(|before| before.ends_with(b"start"));
    }
    (at == 0 || data.get(at - 1).is_some_and(|b| !b.is_ascii_alphanumeric()))
        // A header is short: only its first bytes are looked at.
        && header(&mut Lexer::new(data.get(..(at + 64).min(data.len())).unwrap_or_default(), at)).is_some()
}

/// A cross-reference table and its trailer.
fn table(
    data: &[u8],
    offset: usize,
    budget: &Budget,
    entries: &mut BTreeMap<u32, Entry>,
) -> Result<Option<Dictionary>, PdfError> {
    let mut lexer = Lexer::new(data, offset + b"xref".len());
    lexer.skip_space();
    // What was looked at so far is charged as it goes.
    let mut charged = offset;
    loop {
        let before = lexer.pos;
        let (Some(first), Some(count)) = ({
            let first = lexer.unsigned();
            lexer.skip_white();
            (first, lexer.unsigned())
        }) else {
            lexer.rewind(before);
            break;
        };
        lexer.skip_space();
        // An entry takes at least 6 bytes (`0 0 n `): a count larger than
        // what is left of the file is a lie.
        let left = data.len().saturating_sub(lexer.pos) / 6;
        let count = usize::try_from(count).unwrap_or(usize::MAX).min(left);
        budget.memory(count.saturating_mul(ENTRY_MEMORY))?;
        budget.work(count.saturating_mul(ENTRY_COST))?;
        for index in 0..count {
            let line = lexer.pos;
            let (Some(offset), Some(generation)) = ({
                let offset = lexer.unsigned();
                lexer.skip_white();
                (offset, lexer.unsigned())
            }) else {
                lexer.rewind(line);
                break;
            };
            lexer.skip_white();
            let kind = lexer.peek();
            lexer.pos += 1;
            lexer.skip_white();
            let number = first
                .checked_add(index as u64)
                .and_then(|number| u32::try_from(number).ok());
            let Some(number) = number else {
                continue;
            };
            let entry = match kind {
                Some(b'n') => match (usize::try_from(offset), u16::try_from(generation)) {
                    (Ok(offset), Ok(generation)) => Entry::Normal { offset, generation },
                    _ => Entry::Free,
                },
                Some(b'f') => Entry::Free,
                _ => {
                    lexer.rewind(line);
                    break;
                }
            };
            entries.entry(number).or_insert(entry);
        }
        budget.work(lexer.reached().saturating_sub(charged))?;
        charged = lexer.reached();
    }
    lexer.skip_space();
    if !lexer.eat(b"trailer") {
        budget.work(lexer.reached().saturating_sub(charged))?;
        return Ok(None);
    }
    lexer.skip_space();
    let parsed = lexer.dictionary(MAX_DEPTH, budget);
    budget.work(lexer.reached().saturating_sub(charged))?;
    match parsed {
        Ok(trailer) => Ok(Some(trailer)),
        Err(Fail::Over(error)) => Err(error),
        Err(_) => Ok(None),
    }
}

/// A cross-reference stream (`/Type /XRef`) and its dictionary, which is
/// the trailer.
fn stream(
    data: &Bytes,
    offset: usize,
    budget: &Budget,
    entries: &mut BTreeMap<u32, Entry>,
) -> Result<Option<Dictionary>, PdfError> {
    let Some((_, Object::Stream(stream))) = indirect(data, offset, data.len(), budget, &|_| None)?
    else {
        return Ok(None);
    };
    if !stream.dict.has_type(b"XRef") {
        return Ok(None);
    }
    let limit = budget.limits.object_stream.min(budget.memory_left());
    let mut spent = 0;
    let decoded = filters::decode(&stream.dict, &stream.content, limit, &Some, &mut spent);
    budget.work(spent)?;
    let content = match decoded {
        Ok(content) => content,
        Err(DecodeError::Over) if limit < budget.limits.object_stream => {
            return Err(budget.out_of_memory());
        }
        Err(_) => return Ok(None),
    };
    budget.memory(content.len())?;
    budget.work(content.len())?;
    let numbers = |key: &[u8]| -> Option<Vec<i64>> {
        stream
            .dict
            .get(key)?
            .as_array()?
            .iter()
            .map(Object::as_i64)
            .collect()
    };
    let Some(size) = stream.dict.get(b"Size").and_then(Object::as_i64) else {
        return Ok(None);
    };
    let Some(widths) = numbers(b"W") else {
        return Ok(None);
    };
    let widths: Vec<usize> = widths
        .iter()
        .take(3)
        .filter_map(|&w| usize::try_from(w).ok().filter(|&w| w <= 8))
        .collect();
    let [w0, w1, w2] = widths[..] else {
        return Ok(None);
    };
    let width = w0 + w1 + w2;
    if width == 0 {
        return Ok(None);
    }
    let index = numbers(b"Index").unwrap_or_else(|| vec![0, size]);
    let total = index
        .as_chunks::<2>()
        .0
        .iter()
        .try_fold(0usize, |total, &[_, count]| {
            total.checked_add(usize::try_from(count).ok()?)
        });
    let Some(total) = total.filter(|&total| total <= content.len() / width) else {
        return Ok(None);
    };
    budget.memory(total.saturating_mul(ENTRY_MEMORY))?;
    budget.work(total.saturating_mul(ENTRY_COST))?;
    let field = |at: &mut usize, width: usize| -> u64 {
        let value = content
            .get(*at..*at + width)
            .unwrap_or_default()
            .iter()
            .fold(0u64, |value, &byte| value << 8 | u64::from(byte));
        *at += width;
        value
    };
    let mut at = 0;
    for &[first, count] in index.as_chunks::<2>().0 {
        for j in 0..count {
            let kind = if w0 == 0 { 1 } else { field(&mut at, w0) };
            let second = field(&mut at, w1);
            let third = field(&mut at, w2);
            let Some(number) = first.checked_add(j).and_then(|n| u32::try_from(n).ok()) else {
                continue;
            };
            let entry = match kind {
                0 => Entry::Free,
                1 => match usize::try_from(second) {
                    Ok(offset) => Entry::Normal {
                        offset,
                        generation: if w2 == 0 {
                            0
                        } else {
                            u16::try_from(third).unwrap_or(0)
                        },
                    },
                    Err(_) => continue,
                },
                2 => match (u32::try_from(second), u32::try_from(third)) {
                    (Ok(container), Ok(index)) => Entry::Compressed { container, index },
                    _ => continue,
                },
                _ => continue,
            };
            entries.entry(number).or_insert(entry);
        }
    }
    let mut trailer = stream.dict;
    for key in [&b"Length"[..], b"W", b"Index", b"Filter", b"DecodeParms"] {
        trailer.remove(key);
    }
    Ok(Some(trailer))
}

/// Whether most entries at an offset point at the header of their own
/// object, as they do in a cross-reference that describes this file. One
/// whose objects moved (bytes inserted or removed before them) does not.
/// Also returns where entries find an object of their number. The bytes
/// looked at are charged as work.
fn locates_objects(
    data: &[u8],
    xref: &Xref,
    budget: &Budget,
) -> Result<(bool, Vec<usize>), PdfError> {
    let (mut normal, mut matching) = (0usize, 0usize);
    let mut located = Vec::new();
    for (&number, entry) in &xref.entries {
        if let Entry::Normal { offset, generation } = *entry {
            normal += 1;
            let mut lexer = Lexer::new(data, offset);
            lexer.skip_space();
            let found = header(&mut lexer);
            if found == Some((number, generation)) {
                matching += 1;
            }
            if found.is_some_and(|(found, _)| found == number) {
                budget.memory(OFFSET_MEMORY)?;
                located.push(offset);
            }
            budget.work(lexer.reached().saturating_sub(offset).max(1))?;
        }
    }
    Ok((normal == 0 || matching * 2 > normal, located))
}

/// Whether the bytes before `at` end a dictionary, white space aside: the
/// word `stream` only opens a stream there, not inside a string.
fn after_dictionary(data: &[u8], at: usize) -> bool {
    let before = data.get(..at).unwrap_or_default();
    let end = before
        .iter()
        .rposition(|&byte| !crate::parse::is_white(byte))
        .map_or(0, |last| last + 1);
    before.get(..end).unwrap_or_default().ends_with(b">>")
}

/// The headers of the objects in `data` (`N G obj` at the start of a
/// line or after an `endobj`), skipping stream data, each with its offset; later ones replace
/// earlier ones, as incremental updates do. One pass over the file: the
/// search for the end of a stream only moves forward, and once it has
/// found no end it does not look again.
pub(crate) fn scan(data: &[u8], budget: &Budget) -> Result<BTreeMap<u32, Entry>, PdfError> {
    budget.work(data.len())?;
    let mut entries = BTreeMap::new();
    let mut at_line_start = true;
    let mut at = 0;
    let mut no_more_ends = false;
    while at < data.len() {
        if !no_more_ends
            && data
                .get(at..)
                .is_some_and(|rest| rest.starts_with(b"stream"))
            && !data
                .get(..at)
                .is_some_and(|before| before.ends_with(b"end"))
            && matches!(data.get(at + 6), Some(b'\r' | b'\n'))
            && after_dictionary(data, at)
        {
            match find(data, b"endstream", at + 6) {
                Some(end) => {
                    at = end + b"endstream".len();
                    at_line_start = false;
                    continue;
                }
                None => {
                    no_more_ends = true;
                    at += 6;
                    continue;
                }
            }
        }
        let byte = data.get(at).copied().unwrap_or(0);
        if at_line_start && byte.is_ascii_digit() {
            let mut lexer = Lexer::new(data, at);
            if let Some((number, generation)) = header(&mut lexer)
                // Object 0 heads the free list: never an object.
                && (1..=MAX_SCANNED_NUMBER).contains(&number)
            {
                if !entries.contains_key(&number) {
                    budget.memory(ENTRY_MEMORY)?;
                }
                entries.insert(
                    number,
                    Entry::Normal {
                        offset: at,
                        generation,
                    },
                );
            }
        }
        at_line_start = match byte {
            b'\r' | b'\n' => true,
            b' ' | b'\t' => at_line_start,
            b'j' => data
                .get(..at + 1)
                .is_some_and(|read| read.ends_with(b"endobj")),
            _ => false,
        };
        at += 1;
    }
    Ok(entries)
}

/// Where an entry says its object is in the file.
pub(crate) fn normal_offset(entry: &Entry) -> Option<usize> {
    match entry {
        Entry::Normal { offset, .. } => Some(*offset),
        _ => None,
    }
}

/// Rebuilds the cross-reference by scanning for objects, with the last
/// trailer whose `Root` is one of them (or the last cross-reference
/// stream's dictionary, for files that have no trailer). `None` when there
/// is nothing to rebuild from.
fn rebuild(data: &Bytes, budget: &Budget) -> Result<Option<Xref>, PdfError> {
    let entries = scan(data, budget)?;
    if entries.is_empty() {
        return Ok(None);
    }
    let has_root = |trailer: &Dictionary| {
        trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .is_some_and(|(number, _)| entries.contains_key(&number))
    };
    let mut trailer = None;
    let mut from = data.len();
    // The search only moves back, and each trailer it finds is charged what
    // its parse looked at.
    budget.work(from)?;
    while let Some(at) = rfind(data.get(..from).unwrap_or_default(), b"trailer", 0) {
        from = at;
        let start = at + b"trailer".len();
        let mut lexer = Lexer::new(data, start);
        lexer.skip_space();
        let parsed = lexer.dictionary(MAX_DEPTH, budget);
        budget.work(lexer.reached().saturating_sub(start))?;
        match parsed {
            Ok(found) if has_root(&found) => {
                trailer = Some(found);
                break;
            }
            Err(Fail::Over(error)) => return Err(error),
            _ => {}
        }
    }
    if trailer.is_none() {
        // The last cross-reference stream with a `Root`.
        let mut offsets: Vec<usize> = entries.values().filter_map(normal_offset).collect();
        offsets.sort_unstable();
        for &offset in offsets.iter().rev() {
            let next = offsets.partition_point(|&at| at <= offset);
            let end = offsets.get(next).copied().unwrap_or(data.len());
            if let Some((_, Object::Stream(stream))) =
                indirect(data, offset, end, budget, &|_| None)?
                && stream.dict.has_type(b"XRef")
                && has_root(&stream.dict)
            {
                let mut found = stream.dict;
                for key in [&b"Length"[..], b"W", b"Index", b"Filter", b"DecodeParms"] {
                    found.remove(key);
                }
                trailer = Some(found);
                break;
            }
        }
    }
    let Some(mut trailer) = trailer else {
        return Ok(None);
    };
    trailer.remove(b"Prev");
    trailer.remove(b"XRefStm");
    budget.memory(entries.len().saturating_mul(OFFSET_MEMORY))?;
    let located = entries.values().filter_map(normal_offset).collect();
    Ok(Some(Xref {
        entries,
        trailer,
        starts: Vec::new(),
        rebuilt: true,
        located,
        freed: HashSet::new(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::ReadLimits;

    fn budget() -> Budget {
        Budget::new(ReadLimits::default())
    }

    /// A file of `objects` (`N 0 obj ... endobj`) with a table and trailer,
    /// offsets computed.
    pub(crate) fn file(objects: &[&str], trailer: &str) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{body}\nendobj\n", index + 1).into_bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).into_bytes());
        for offset in offsets {
            out.extend(format!("{offset:010} 00000 n \n").into_bytes());
        }
        out.extend(format!("trailer\n{trailer}\nstartxref\n{xref}\n%%EOF\n").into_bytes());
        out
    }

    #[test]
    fn a_table_and_its_trailer_are_read() {
        let data = Bytes::from(file(
            &[
                "<< /Type /Catalog /Pages 2 0 R >>",
                "<< /Type /Pages /Kids [] /Count 0 >>",
            ],
            "<< /Size 3 /Root 1 0 R >>",
        ));
        let xref = read(&data, &budget()).unwrap();
        assert!(!xref.rebuilt);
        assert_eq!(xref.entries.len(), 3);
        assert_eq!(xref.entries[&0], Entry::Free);
        assert!(matches!(
            xref.entries[&1],
            Entry::Normal { generation: 0, .. }
        ));
        assert_eq!(xref.trailer.get(b"Root"), Some(&Object::Reference((1, 0))));
    }

    #[test]
    fn a_file_without_a_usable_cross_reference_is_rebuilt() {
        let mut data = file(
            &["<< /Type /Catalog >>", "(two)"],
            "<< /Size 3 /Root 1 0 R >>",
        );
        // startxref points nowhere useful.
        let at = find(&data, b"startxref\n", 0).unwrap() + 10;
        let end = find(&data, b"\n", at).unwrap();
        data.splice(at..end, b"0".iter().copied());
        let xref = read(&Bytes::from(data), &budget()).unwrap();
        assert!(xref.rebuilt);
        assert_eq!(xref.entries.len(), 2);
        assert_eq!(xref.trailer.get(b"Root"), Some(&Object::Reference((1, 0))));
        // Nothing to rebuild from.
        assert!(read(&Bytes::from_static(b"%PDF-1.7 broken"), &budget()).is_err());
    }

    #[test]
    fn streams_end_where_the_length_says_or_at_endstream() {
        let good =
            &Bytes::from_static(b"1 0 obj\n<< /Length 5 >>\nstream\nhello\nendstream\nendobj\n");
        let (id, object) = indirect(good, 0, good.len(), &budget(), &|_| None)
            .unwrap()
            .unwrap();
        assert_eq!(id, (1, 0));
        assert_eq!(&object.as_stream().unwrap().content[..], b"hello");
        let wrong = &Bytes::from_static(
            b"1 0 obj\n<< /Length 50 >>\nstream\r\nhello there\r\nendstream\nendobj\n",
        );
        let (_, object) = indirect(wrong, 0, wrong.len(), &budget(), &|_| None)
            .unwrap()
            .unwrap();
        assert_eq!(&object.as_stream().unwrap().content[..], b"hello there");
        let referred =
            &Bytes::from_static(b"1 0 obj\n<< /Length 2 0 R >>\nstream\nhey\nendstream\nendobj\n");
        let (_, object) = indirect(referred, 0, referred.len(), &budget(), &|_| Some(3))
            .unwrap()
            .unwrap();
        assert_eq!(&object.as_stream().unwrap().content[..], b"hey");
        assert!(
            indirect(&Bytes::from_static(b"junk"), 0, 4, &budget(), &|_| None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_prev_chain_that_loops_ends() {
        // Two tables pointing at each other through /Prev.
        let mut data = file(
            &["<< /Type /Catalog >>"],
            "<< /Size 2 /Root 1 0 R /Prev 9 >>",
        );
        let first = find(&data, b"xref", 0).unwrap();
        let text = String::from_utf8(data.clone())
            .unwrap()
            .replace("/Prev 9", &format!("/Prev {first:<width$}", width = 1));
        data = text.into_bytes();
        let xref = read(&Bytes::from(data), &budget()).unwrap();
        assert_eq!(xref.entries.len(), 2);
    }
}
