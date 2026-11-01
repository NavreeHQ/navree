//! BIFF records: the workbook's globals ([`read_globals`]) and a sheet's
//! cells ([`Context::cells_at`]), per [MS-XLS] for BIFF8 and the BIFF5
//! layout where it differs (strings of bytes in the workbook's code page).

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use encoding_rs::{Encoding, UTF_16BE, UTF_16LE};

use super::super::workbook::dates::shows_date;
use super::super::workbook::{MAX_SHEETS, make_room, too_many_sheets};
use super::{
    Globals, MAX_CELL_FORMATS, STOP_EVERY, XlsError, XlsValue, damaged, is_custom_format_id,
};

const FORMULA: u16 = 0x0006;
const EOF: u16 = 0x000A;
const EXTERNSHEET: u16 = 0x0017;
const NAME: u16 = 0x0018;
const DATEMODE: u16 = 0x0022;
const FILEPASS: u16 = 0x002F;
const CONTINUE: u16 = 0x003C;
const CODEPAGE: u16 = 0x0042;
const BOUNDSHEET: u16 = 0x0085;
const MULRK: u16 = 0x00BD;
const RSTRING: u16 = 0x00D6;
const XF: u16 = 0x00E0;
const MERGECELLS: u16 = 0x00E5;
const SST: u16 = 0x00FC;
const LABELSST: u16 = 0x00FD;
const DIMENSIONS: u16 = 0x0200;
const NUMBER: u16 = 0x0203;
const LABEL: u16 = 0x0204;
const BOOLERR: u16 = 0x0205;
const STRING: u16 = 0x0207;
const RK: u16 = 0x027E;
const BOF: u16 = 0x0809;
const FORMAT: u16 = 0x041E;

/// The BIFF version, as far as reading differs: BIFF8 strings are UTF-16
/// (one byte a character when it fits), earlier ones bytes in the code
/// page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Biff {
    /// BIFF5 and before.
    Bytes,
    /// BIFF8.
    Unicode,
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let bytes: [u8; 2] = data.get(at..at.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let bytes: [u8; 4] = data.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn f64_at(data: &[u8], at: usize) -> Option<f64> {
    let bytes: [u8; 8] = data.get(at..at.checked_add(8)?)?.try_into().ok()?;
    Some(f64::from_le_bytes(bytes))
}

/// A record too short for what it holds.
fn short(what: &str) -> XlsError {
    damaged(format!("a {what} record is too short"))
}

/// One record, and the CONTINUE records after it that carry its data on.
#[derive(Clone, Copy, Debug)]
pub(super) struct Record<'a> {
    pub(super) kind: u16,
    pub(super) data: &'a [u8],
    /// The CONTINUE records, whole (headers and data).
    continues: &'a [u8],
}

impl<'a> Record<'a> {
    /// The record's data, and then its CONTINUE records' data, as they are
    /// asked for.
    fn pieces(&self) -> Pieces<'a> {
        Pieces {
            data: self.data,
            rest: self.continues,
        }
    }
}

/// The records of a stream from an offset, each with its CONTINUE
/// records; ends at the end of the stream, or after the first error.
#[derive(Clone, Debug)]
pub(super) struct Records<'a> {
    rest: &'a [u8],
    /// The flag that tells the reading to stop, when there is one, and how
    /// many records may go by before it is looked at again.
    stop: Option<&'a AtomicBool>,
    until_look: u32,
}

impl<'a> Records<'a> {
    pub(super) fn new(stream: &'a [u8]) -> Self {
        Records::watched(stream, None)
    }

    /// The records of `stream`, until `stop` is raised.
    pub(super) fn watched(stream: &'a [u8], stop: Option<&'a AtomicBool>) -> Self {
        Records {
            rest: stream,
            stop,
            // The first record looks.
            until_look: 1,
        }
    }

    fn fail(&mut self, what: &str) -> Option<Result<Record<'a>, XlsError>> {
        self.rest = &[];
        Some(Err(damaged(what)))
    }

    /// Counts a record (a CONTINUE record too), and every [`STOP_EVERY`]th
    /// time looks at the flag: whether the reading has been told to stop.
    fn told_to_stop(&mut self) -> bool {
        let Some(stop) = self.stop else {
            return false;
        };
        self.until_look = self.until_look.saturating_sub(1);
        if self.until_look > 0 {
            return false;
        }
        self.until_look = STOP_EVERY;
        stop.load(Ordering::Relaxed)
    }

    fn interrupt(&mut self) -> Option<Result<Record<'a>, XlsError>> {
        self.rest = &[];
        Some(Err(XlsError::Interrupted))
    }
}

/// The header of the record at the start of `rest`: its kind, its data
/// and what follows it.
fn split_record(rest: &[u8]) -> Option<(u16, &[u8], &[u8])> {
    let kind = u16_at(rest, 0)?;
    let len = usize::from(u16_at(rest, 2)?);
    let end = len.checked_add(4)?;
    Some((kind, rest.get(4..end)?, rest.get(end..)?))
}

impl<'a> Iterator for Records<'a> {
    type Item = Result<Record<'a>, XlsError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        if self.told_to_stop() {
            return self.interrupt();
        }
        if self.rest.len() < 4 {
            return self.fail("a record is cut short by the end of the stream");
        }
        let Some((kind, data, mut rest)) = split_record(self.rest) else {
            return self.fail("a record runs past the end of the stream");
        };
        // CONTINUE records that follow belong to this one (one that is the
        // last four bytes of the stream, with no data, is a record of its
        // own).
        let continues = rest;
        while rest.len() > 4 && u16_at(rest, 0) == Some(CONTINUE) {
            if self.told_to_stop() {
                return self.interrupt();
            }
            match split_record(rest) {
                Some((_, _, after)) => rest = after,
                None => return self.fail("a CONTINUE record runs past the end of the stream"),
            }
        }
        let used = continues.len().saturating_sub(rest.len());
        self.rest = rest;
        Some(Ok(Record {
            kind,
            data,
            continues: continues.get(..used).unwrap_or_default(),
        }))
    }
}

/// A record's data and its CONTINUE records', one at a time.
struct Pieces<'a> {
    data: &'a [u8],
    rest: &'a [u8],
}

impl Pieces<'_> {
    /// Moves on to the next CONTINUE record's data; false when there is
    /// none.
    fn next_piece(&mut self) -> bool {
        match split_record(self.rest) {
            Some((_, data, rest)) => {
                self.data = data;
                self.rest = rest;
                true
            }
            None => false,
        }
    }

    /// Skips `len` bytes, across CONTINUE records.
    fn skip(&mut self, mut len: u64, what: &str) -> Result<(), XlsError> {
        while len > 0 {
            if self.data.is_empty() && !self.next_piece() {
                return Err(damaged(format!("{what} runs past the end of its records")));
            }
            let take = usize::try_from(len)
                .unwrap_or(usize::MAX)
                .min(self.data.len());
            self.data = self.data.get(take..).unwrap_or_default();
            len = len.saturating_sub(take as u64);
        }
        Ok(())
    }
}

/// Appends the characters in `bytes` to `out`: UTF-16 when `wide`,
/// otherwise one byte each (the low half of a UTF-16 unit).
fn push_chars(bytes: &[u8], wide: bool, out: &mut String) {
    if wide {
        out.push_str(&UTF_16LE.decode_without_bom_handling(bytes).0);
    } else {
        out.extend(bytes.iter().map(|byte| char::from(*byte)));
    }
}

/// Appends `bytes`, UTF-16, to `out`. A pair of surrogates may be split
/// between two records: a high surrogate that ends `bytes` waits in `pending`
/// for what comes next.
fn push_wide(mut bytes: &[u8], pending: &mut Option<u16>, out: &mut String) {
    if bytes.is_empty() {
        return;
    }
    if let Some(high) = pending.take() {
        match u16_at(bytes, 0) {
            Some(low) if (0xDC00..=0xDFFF).contains(&low) => {
                out.extend(
                    char::decode_utf16([high, low])
                        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER)),
                );
                bytes = bytes.get(2..).unwrap_or_default();
            }
            _ => out.push(char::REPLACEMENT_CHARACTER),
        }
    }
    let last = bytes.len().checked_sub(2).and_then(|at| u16_at(bytes, at));
    if let Some(high @ 0xD800..=0xDBFF) = last {
        *pending = Some(high);
        bytes = bytes
            .get(..bytes.len().saturating_sub(2))
            .unwrap_or_default();
    }
    out.push_str(&UTF_16LE.decode_without_bom_handling(bytes).0);
}

/// Half a pair of surrogates that nothing completes is a replacement.
fn flush_surrogate(pending: &mut Option<u16>, out: &mut String) {
    if pending.take().is_some() {
        out.push(char::REPLACEMENT_CHARACTER);
    }
}

/// Appends `bytes`, a BIFF5 string in `codepage`, to `out`. A UTF-16 code
/// page (the default when the workbook names none) reads one byte a
/// character.
fn push_bytes(bytes: &[u8], codepage: &'static Encoding, out: &mut String) {
    if codepage == UTF_16LE || codepage == UTF_16BE {
        push_chars(bytes, false, out);
    } else {
        out.push_str(&codepage.decode_without_bom_handling(bytes).0);
    }
}

/// What happens when a string's characters run past its records.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AtTheEnd {
    /// The string is damaged.
    Refuse,
    /// The string is what there was.
    Cut,
}

/// Reads `count` UTF-16 characters (one byte each unless `wide`) from
/// `pieces` into `out`, on into CONTINUE records, where each part starts
/// with a flags byte of its own. A pair of surrogates split between two parts
/// is one character.
fn read_chars(
    pieces: &mut Pieces<'_>,
    mut count: usize,
    mut wide: bool,
    at_the_end: AtTheEnd,
    out: &mut String,
) -> Result<(), XlsError> {
    // A byte-order mark that starts the string is not part of its text.
    if wide
        && count > 0
        && let Some(rest) = pieces.data.strip_prefix(&[0xFF, 0xFE])
    {
        pieces.data = rest;
        count = count.saturating_sub(1);
    }
    let mut pending = None;
    loop {
        let width = if wide { 2 } else { 1 };
        let here = count.min(pieces.data.len().checked_div(width).unwrap_or(0));
        let bytes = here.saturating_mul(width);
        let piece = pieces.data.get(..bytes).unwrap_or_default();
        if wide {
            push_wide(piece, &mut pending, out);
        } else if !piece.is_empty() {
            flush_surrogate(&mut pending, out);
            push_chars(piece, false, out);
        }
        count = count.saturating_sub(here);
        pieces.data = pieces.data.get(bytes..).unwrap_or_default();
        if count == 0 {
            flush_surrogate(&mut pending, out);
            return Ok(());
        }
        if !pieces.next_piece() {
            flush_surrogate(&mut pending, out);
            return match at_the_end {
                AtTheEnd::Cut => Ok(()),
                AtTheEnd::Refuse => {
                    Err(damaged("a shared string runs past the end of its records"))
                }
            };
        }
        match pieces.data.split_first() {
            Some((flags, data)) => {
                wide = flags & 1 != 0;
                pieces.data = data;
            }
            None if at_the_end == AtTheEnd::Cut => {
                flush_surrogate(&mut pending, out);
                return Ok(());
            }
            None => {
                return Err(damaged(
                    "a shared string goes on in an empty CONTINUE record",
                ));
            }
        }
    }
}

/// An XLUnicodeString (BIFF8: a 16-bit count, flags, characters) or a
/// string of bytes with a 16-bit length (BIFF5), at the start of
/// `pieces`. Characters past the record's data, and its CONTINUE records',
/// are not there.
fn unicode_string(
    pieces: &mut Pieces<'_>,
    biff: Biff,
    codepage: &'static Encoding,
    what: &str,
) -> Result<String, XlsError> {
    let data = pieces.data;
    let mut out = String::new();
    match biff {
        Biff::Unicode => {
            let (Some(count), Some(flags)) = (u16_at(data, 0), data.get(2)) else {
                // An empty string written as its count alone.
                if data == [0, 0] {
                    return Ok(out);
                }
                return Err(short(what));
            };
            pieces.data = data.get(3..).unwrap_or_default();
            read_chars(
                pieces,
                usize::from(count),
                flags & 1 != 0,
                AtTheEnd::Cut,
                &mut out,
            )?;
        }
        Biff::Bytes => {
            let Some(count) = u16_at(data, 0) else {
                return Err(short(what));
            };
            let bytes = data.get(2..).unwrap_or_default();
            let bytes = bytes.get(..usize::from(count)).unwrap_or(bytes);
            push_bytes(bytes, codepage, &mut out);
        }
    }
    Ok(out)
}

/// A ShortXLUnicodeString (BIFF8: an 8-bit count, flags, characters) or a
/// string of bytes with an 8-bit length (BIFF5).
fn short_string(
    data: &[u8],
    biff: Biff,
    codepage: &'static Encoding,
    what: &str,
) -> Result<String, XlsError> {
    let mut out = String::new();
    match biff {
        Biff::Unicode => {
            let [count, flags, chars @ ..] = data else {
                return Err(short(what));
            };
            let mut pieces = Pieces {
                data: chars,
                rest: &[],
            };
            read_chars(
                &mut pieces,
                usize::from(*count),
                flags & 1 != 0,
                AtTheEnd::Cut,
                &mut out,
            )?;
        }
        Biff::Bytes => {
            let [count, bytes @ ..] = data else {
                return Err(short(what));
            };
            let bytes = bytes.get(..usize::from(*count)).unwrap_or(bytes);
            push_bytes(bytes, codepage, &mut out);
        }
    }
    Ok(out)
}

/// The shared strings, each trimmed: their text, one after another, and where
/// each ends.
#[derive(Clone, Debug, Default)]
pub(super) struct SharedStrings {
    text: String,
    ends: Vec<u32>,
}

impl SharedStrings {
    fn get(&self, index: u32) -> Option<&str> {
        let index = usize::try_from(index).ok()?;
        let end = usize::try_from(*self.ends.get(index)?).ok()?;
        let start = match index.checked_sub(1) {
            Some(before) => usize::try_from(*self.ends.get(before)?).ok()?,
            None => 0,
        };
        self.text.get(start..end)
    }

    /// How many strings the table has room for.
    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize {
        self.ends.capacity()
    }
}

/// The shared strings of an SST record and its CONTINUE records. Each
/// string is an XLUnicodeRichExtendedString: a count, flags, a count of
/// formatting runs and a size of phonetic data when the flags say so, the
/// characters, then the runs and the phonetic data (skipped). The count of
/// strings the record declares is not used: a table holds what it has.
fn read_shared_strings(
    record: &Record<'_>,
    max_text_bytes: u64,
) -> Result<SharedStrings, XlsError> {
    let mut pieces = record.pieces();
    pieces.data = pieces.data.get(8..).ok_or_else(|| short("SST"))?;
    let mut shared = SharedStrings::default();
    let too_large = || {
        XlsError::TooLarge(format!(
            "The workbook's shared strings hold more than {max_text_bytes} bytes of text."
        ))
    };
    let mut piece = String::new();
    while !pieces.data.is_empty() || pieces.next_piece() {
        piece.clear();
        read_rich_string(&mut pieces, &mut piece)?;
        // The table keeps its text trimmed: a string is trimmed here, once,
        // however many cells use it.
        let kept = piece.trim();
        let len = shared
            .text
            .len()
            .checked_add(kept.len())
            .ok_or_else(too_large)?;
        if len as u64 > max_text_bytes {
            return Err(too_large());
        }
        let end = u32::try_from(len).map_err(|_| too_large())?;
        make_room(&mut shared.text, kept.len(), max_text_bytes).map_err(|_| too_large())?;
        shared.text.push_str(kept);
        shared.ends.push(end);
    }
    shared.text.shrink_to_fit();
    shared.ends.shrink_to_fit();
    Ok(shared)
}

/// One XLUnicodeRichExtendedString of the shared strings into `out`.
fn read_rich_string(pieces: &mut Pieces<'_>, out: &mut String) -> Result<(), XlsError> {
    let data = pieces.data;
    if data.is_empty() {
        // A CONTINUE record with nothing in it: an empty string.
        return Ok(());
    }
    let (Some(count), Some(flags)) = (u16_at(data, 0), data.get(2)) else {
        return Err(damaged("a shared string is cut short"));
    };
    let mut at = 3usize;
    let mut runs = 0u64;
    if flags & 0x8 != 0 {
        runs = u64::from(u16_at(data, at).ok_or_else(|| damaged("a shared string is cut short"))?);
        at = at.saturating_add(2);
    }
    let mut phonetic = 0u64;
    if flags & 0x4 != 0 {
        let size = u32_at(data, at).ok_or_else(|| damaged("a shared string is cut short"))?;
        // A negative size cannot be skipped.
        if size > i32::MAX as u32 {
            return Err(damaged(
                "a shared string has a negative size of phonetic data",
            ));
        }
        phonetic = u64::from(size);
        at = at.saturating_add(4);
    }
    pieces.data = data.get(at..).unwrap_or_default();
    read_chars(
        pieces,
        usize::from(count),
        flags & 1 != 0,
        AtTheEnd::Refuse,
        out,
    )?;
    pieces.skip(runs.saturating_mul(4), "a shared string's formatting")?;
    pieces.skip(phonetic, "a shared string's phonetic data")
}

/// What kind of sheet a BOUNDSHEET record names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SheetKind {
    Worksheet,
    Other,
}

/// A sheet as BOUNDSHEET names it.
#[derive(Clone, Debug)]
pub(super) struct BoundSheet {
    pub(super) name: String,
    pub(super) kind: SheetKind,
    /// Where the sheet's BOF is in the stream.
    pub(super) offset: usize,
}

fn read_boundsheet(
    data: &[u8],
    biff: Biff,
    codepage: &'static Encoding,
) -> Result<BoundSheet, XlsError> {
    let (Some(offset), Some(state), Some(kind)) = (u32_at(data, 0), data.get(4), data.get(5))
    else {
        return Err(short("BOUNDSHEET"));
    };
    if state & 0x3F > 2 {
        return Err(damaged(format!(
            "a sheet has an unknown visibility {:#x}",
            state & 0x3F
        )));
    }
    let kind = match kind {
        0 => SheetKind::Worksheet,
        // A macro sheet, a chart, a VBA module.
        1 | 2 | 6 => SheetKind::Other,
        other => return Err(damaged(format!("a sheet has an unknown type {other:#x}"))),
    };
    let mut name = short_string(
        data.get(6..).unwrap_or_default(),
        biff,
        codepage,
        "BOUNDSHEET",
    )?;
    name.retain(|c| c != '\0');
    Ok(BoundSheet {
        name,
        kind,
        offset: usize::try_from(offset)
            .map_err(|_| damaged("a sheet starts past the end of the stream"))?,
    })
}

/// The BIFF version a BOF record gives.
fn read_bof(data: &[u8]) -> Result<Biff, XlsError> {
    let version = u16_at(data, 0).ok_or_else(|| short("BOF"))?;
    Ok(match version {
        0x0200 | 0x0002 | 0x0007 | 0x0300 | 0x0400 | 0x0500 => Biff::Bytes,
        // No version: a BIFF5 workbook says it is one by its type.
        0 if u16_at(data, 2) == Some(0x1000) => Biff::Bytes,
        _ => Biff::Unicode,
    })
}

/// Checks a NAME record (a defined name) as far as its fields go: the
/// name's characters and the formula's first token must be there.
fn check_name(data: &[u8], biff: Biff) -> Result<(), XlsError> {
    let (Some(count), Some(size)) = (data.get(3), u16_at(data, 4)) else {
        return Err(short("NAME"));
    };
    let name = data.get(14..).ok_or_else(|| short("NAME"))?;
    if biff == Biff::Unicode {
        let flags = name.first().ok_or_else(|| short("NAME"))?;
        let width = if flags & 1 != 0 { 2 } else { 1 };
        if name.len() < usize::from(*count).saturating_mul(width).saturating_add(1) {
            return Err(short("NAME"));
        }
    }
    let formula = data
        .len()
        .checked_sub(usize::from(size))
        .and_then(|at| data.get(at..))
        .ok_or_else(|| short("NAME"))?;
    let unicode = biff == Biff::Unicode;
    let needs = match formula.first() {
        // A reference to a cell, a reference to an area, a reference that
        // is an error, on another sheet.
        Some(0x3A | 0x5A | 0x7A) => {
            if unicode {
                7
            } else {
                18
            }
        }
        Some(0x3B | 0x5B | 0x7B) => {
            if unicode {
                11
            } else {
                21
            }
        }
        Some(0x3C | 0x5C | 0x7C | 0x3D | 0x5D | 0x7D) => {
            if unicode {
                3
            } else {
                13
            }
        }
        _ => 0,
    };
    if formula.len() < needs {
        return Err(short("NAME"));
    }
    Ok(())
}

/// The code page a CODEPAGE record names, as an encoding; none for a code
/// page that has none.
pub(super) fn encoding_of(codepage: u16) -> Option<&'static Encoding> {
    use encoding_rs::*;
    Some(match codepage {
        // US-ASCII, and Excel's own name for Windows-1252 on old files.
        367 | 1252 | 28591 | 32769 => WINDOWS_1252,
        65001 => UTF_8,
        1200 | 21010 => UTF_16LE,
        1201 => UTF_16BE,
        1251 => WINDOWS_1251,
        936 | 20936 | 51936 => GBK,
        932 => SHIFT_JIS,
        949 | 20949 | 51949 => EUC_KR,
        1250 => WINDOWS_1250,
        1256 => WINDOWS_1256,
        1254 | 28599 => WINDOWS_1254,
        950 | 951 => BIG5,
        874 | 28601 => WINDOWS_874,
        1255 => WINDOWS_1255,
        1253 => WINDOWS_1253,
        1257 => WINDOWS_1257,
        1258 => WINDOWS_1258,
        20932 => EUC_JP,
        28592 => ISO_8859_2,
        28605 => ISO_8859_15,
        28597 => ISO_8859_7,
        20866 => KOI8_R,
        54936 => GB18030,
        28595 => ISO_8859_5,
        38598 => ISO_8859_8_I,
        28594 => ISO_8859_4,
        28596 => ISO_8859_6,
        50220..=50222 => ISO_2022_JP,
        21866 => KOI8_U,
        28603 => ISO_8859_13,
        28593 => ISO_8859_3,
        866 => IBM866,
        28600 => ISO_8859_10,
        28598 => ISO_8859_8,
        // Excel's own name for Mac Roman on old files.
        10000 | 32768 => MACINTOSH,
        10017 | 10007 => X_MAC_CYRILLIC,
        28604 => ISO_8859_14,
        28606 => ISO_8859_16,
        // ISO-2022-KR, ISO-2022-CN and HZ: decoded as the replacement
        // character, as browsers do.
        50225 | 50227 | 52936 => REPLACEMENT,
        _ => return None,
    })
}

/// Reads the workbook's globals: the records from the start of the stream
/// to the globals' EOF.
pub(super) fn read_globals(stream: &[u8], max_text_bytes: u64) -> Result<Globals, XlsError> {
    let mut globals = Globals {
        biff: Biff::Unicode,
        codepage: UTF_16LE,
        date1904: false,
        formats: Vec::new(),
        custom_formats: BTreeMap::new(),
        sheets: Vec::new(),
        shared: SharedStrings::default(),
    };
    for record in Records::new(stream) {
        let record = record?;
        let data = record.data;
        match record.kind {
            FILEPASS => return Err(XlsError::Encrypted),
            CODEPAGE => {
                let page = u16_at(data, 0).ok_or_else(|| short("CODEPAGE"))?;
                globals.codepage = encoding_of(page)
                    .ok_or_else(|| damaged(format!("the code page {page} is not known")))?;
            }
            DATEMODE => {
                if u16_at(data, 0).ok_or_else(|| short("DATEMODE"))? == 1 {
                    globals.date1904 = true;
                }
            }
            FORMAT => {
                // A format that cannot be read is not used.
                if let Some((id, kind)) = read_format(&record, globals.biff, globals.codepage) {
                    globals.custom_formats.insert(id, kind);
                }
            }
            XF => {
                let format = u16_at(data, 2).ok_or_else(|| short("XF"))?;
                if globals.formats.len() < MAX_CELL_FORMATS {
                    globals.formats.push(format);
                }
            }
            BOUNDSHEET => {
                if globals.sheets.len() >= MAX_SHEETS {
                    return Err(XlsError::TooLarge(too_many_sheets()));
                }
                globals
                    .sheets
                    .push(read_boundsheet(data, globals.biff, globals.codepage)?)
            }
            BOF => globals.biff = read_bof(data)?,
            NAME => check_name(data, globals.biff)?,
            EXTERNSHEET if globals.biff == Biff::Unicode && data.len() < 2 => {
                return Err(short("EXTERNSHEET"));
            }
            SST => globals.shared = read_shared_strings(&record, max_text_bytes)?,
            EOF => break,
            _ => {}
        }
    }
    Ok(globals)
}

/// A FORMAT record: the id it gives a format, and whether the format shows
/// dates.
fn read_format(
    record: &Record<'_>,
    biff: Biff,
    codepage: &'static Encoding,
) -> Option<(u16, bool)> {
    let id = u16_at(record.data, 0)?;
    if !is_custom_format_id(id) {
        return None;
    }
    let code = match biff {
        Biff::Unicode => {
            let mut pieces = record.pieces();
            pieces.data = pieces.data.get(2..)?;
            unicode_string(&mut pieces, biff, codepage, "FORMAT").ok()?
        }
        Biff::Bytes => short_string(record.data.get(2..)?, biff, codepage, "FORMAT").ok()?,
    };
    Some((id, shows_date(&code)))
}

/// What reading a sheet's cells needs of the workbook.
pub(super) struct Context {
    pub(super) stream: Vec<u8>,
    pub(super) biff: Biff,
    pub(super) codepage: &'static Encoding,
    /// The number format of each XF, by index.
    /// Whether the number format of each XF, by index, shows dates.
    pub(super) formats: Vec<bool>,
    pub(super) shared: SharedStrings,
}

impl Context {
    /// The cells of the sheet whose BOF is at `offset` in the stream, with
    /// the records before `end` to read.
    pub(super) fn cells_at<'a>(
        &'a self,
        offset: usize,
        end: usize,
        stop: Option<&'a AtomicBool>,
    ) -> Result<Cells<'a>, XlsError> {
        let records = self
            .stream
            .get(offset..end.min(self.stream.len()))
            .ok_or_else(|| damaged("a sheet starts past the end of the workbook stream"))?;
        Ok(Cells {
            context: self,
            records: Records::watched(records, stop),
            seen_bof: false,
            depth: 0usize,
            formula_at: (0, 0),
            multiple: None,
            done: false,
            ended: false,
        })
    }

    /// A number in the format of XF `xf`.
    fn number(&self, value: f64, xf: u16) -> XlsValue<'static> {
        if self.is_date(xf) {
            XlsValue::DateTime(value)
        } else {
            XlsValue::Number(value)
        }
    }

    /// An integer in the format of XF `xf`.
    fn integer(&self, value: i64, xf: u16) -> XlsValue<'static> {
        if self.is_date(xf) {
            XlsValue::DateTime(value as f64)
        } else {
            XlsValue::Int(value)
        }
    }

    fn is_date(&self, xf: u16) -> bool {
        self.formats.get(usize::from(xf)).copied().unwrap_or(false)
    }

    /// The value of an RK: the XF (2 bytes), then a number in 30 bits, an
    /// integer or the high bits of a float, times 100 when its low bit says
    /// so.
    fn rk(&self, rk: &[u8]) -> Option<XlsValue<'static>> {
        let xf = u16_at(rk, 0)?;
        let raw = u32_at(rk, 2)?;
        let cents = raw & 1 != 0;
        if raw & 2 != 0 {
            let value = i64::from((raw as i32) >> 2);
            Some(if cents && value % 100 != 0 {
                self.number(value as f64 / 100.0, xf)
            } else {
                self.integer(if cents { value / 100 } else { value }, xf)
            })
        } else {
            let value = f64::from_bits(u64::from(raw & !3) << 32);
            Some(self.number(if cents { value / 100.0 } else { value }, xf))
        }
    }

    fn text(&self, pieces: &mut Pieces<'_>, what: &str) -> Result<XlsValue<'static>, XlsError> {
        Ok(XlsValue::Text(Cow::Owned(unicode_string(
            pieces,
            self.biff,
            self.codepage,
            what,
        )?)))
    }
}

/// The name of an error code, as the table writes it after `#`.
fn error_name(code: u8) -> Option<&'static str> {
    Some(match code {
        0x00 => "Null",
        0x07 => "Div0",
        0x0F => "Value",
        0x17 => "Ref",
        0x1D => "Name",
        0x24 => "Num",
        0x2A => "NA",
        0x2B => "GettingData",
        _ => return None,
    })
}

/// An error cell: by its name, or, for an error of another code, the text
/// `#Error(0x..)`.
fn error(code: u8) -> Result<XlsValue<'static>, XlsError> {
    Ok(match error_name(code) {
        Some(name) => XlsValue::Error(name),
        None => XlsValue::Text(Cow::Owned(format!("#Error({code:#04x})"))),
    })
}

/// The cells of a MULRK record still to come.
struct Multiple<'a> {
    row: u32,
    column: u32,
    rks: &'a [u8],
}

/// A sheet's cells, one record at a time.
pub(super) struct Cells<'a> {
    context: &'a Context,
    records: Records<'a>,
    /// Whether the sheet's own BOF has been read; a BOF after it starts an
    /// embedded substream (a chart), whose records are skipped.
    seen_bof: bool,
    depth: usize,
    /// Where the last FORMULA was: a STRING record is its text.
    formula_at: (u32, u32),
    multiple: Option<Multiple<'a>>,
    done: bool,
    /// Whether the sheet's own EOF has been read.
    ended: bool,
}

type Cell<'a> = (u32, u32, XlsValue<'a>);

fn row_col(data: &[u8]) -> Option<(u32, u32)> {
    Some((u32::from(u16_at(data, 0)?), u32::from(u16_at(data, 2)?)))
}

/// A cell, unless it holds a number that is not finite: no workbook does, and
/// a table cannot keep one as a number.
fn finite(cell: Cell<'_>) -> Result<Cell<'_>, XlsError> {
    match cell.2 {
        XlsValue::Number(number) | XlsValue::DateTime(number) if !number.is_finite() => {
            Err(damaged("a number is not finite"))
        }
        _ => Ok(cell),
    }
}

impl<'a> Cells<'a> {
    /// Whether the sheet's own EOF has been read.
    pub(super) fn ended(&self) -> bool {
        self.ended
    }

    /// The cell a record gives, if any.
    fn cell(&mut self, record: Record<'a>) -> Result<Option<Cell<'a>>, XlsError> {
        let context = self.context;
        let data = record.data;
        let cell = match record.kind {
            NUMBER => {
                let (Some((row, col)), Some(xf), Some(value)) =
                    (row_col(data), u16_at(data, 4), f64_at(data, 6))
                else {
                    return Err(short("NUMBER"));
                };
                (row, col, context.number(value, xf))
            }
            RK => {
                let (Some((row, col)), Some(value)) =
                    (row_col(data), data.get(4..10).and_then(|rk| context.rk(rk)))
                else {
                    return Err(short("RK"));
                };
                (row, col, value)
            }
            MULRK => {
                let (Some((row, first)), Some(last)) = (
                    row_col(data),
                    data.len().checked_sub(2).and_then(|at| u16_at(data, at)),
                ) else {
                    return Err(short("MULRK"));
                };
                let Some(count) = last
                    .checked_sub(first as u16)
                    .and_then(|n| n.checked_add(1))
                else {
                    return Err(damaged("a MULRK record ends before it starts"));
                };
                let expected = usize::from(count).saturating_mul(6).saturating_add(6);
                if data.len() != expected {
                    return Err(damaged(
                        "a MULRK record's length does not match its columns",
                    ));
                }
                self.multiple = Some(Multiple {
                    row,
                    column: first,
                    rks: data.get(4..expected.saturating_sub(2)).unwrap_or_default(),
                });
                return Ok(self.next_of_multiple());
            }
            LABELSST => {
                let (Some((row, col)), Some(index)) = (row_col(data), u32_at(data, 6)) else {
                    return Err(short("LABELSST"));
                };
                // An index past the table is no cell.
                match context.shared.get(index) {
                    Some(text) => (row, col, XlsValue::Text(Cow::Borrowed(text))),
                    None => return Ok(None),
                }
            }
            LABEL | RSTRING => {
                let (Some((row, col)), Some(text)) = (row_col(data), data.get(6..)) else {
                    return Err(short("LABEL"));
                };
                let mut pieces = record.pieces();
                pieces.data = text;
                (row, col, context.text(&mut pieces, "LABEL")?)
            }
            BOOLERR => {
                let (Some((row, col)), Some(value), Some(is_error)) =
                    (row_col(data), data.get(6), data.get(7))
                else {
                    return Err(short("BOOLERR"));
                };
                let value = match is_error {
                    0 => XlsValue::Bool(*value != 0),
                    1 => error(*value)?,
                    other => {
                        return Err(damaged(format!(
                            "a BOOLERR record has an unknown kind {other:#x}"
                        )));
                    }
                };
                (row, col, value)
            }
            FORMULA => {
                let (Some((row, col)), Some(xf), Some(result)) =
                    (row_col(data), u16_at(data, 4), data.get(6..14))
                else {
                    return Err(short("FORMULA"));
                };
                if data.len() < 20 {
                    return Err(short("FORMULA"));
                }
                self.formula_at = (row, col);
                match formula_result(context, result, xf)? {
                    Some(value) => (row, col, value),
                    // Text: in the STRING record that follows.
                    None => return Ok(None),
                }
            }
            STRING => {
                let (row, col) = self.formula_at;
                let mut pieces = record.pieces();
                (row, col, context.text(&mut pieces, "STRING")?)
            }
            DIMENSIONS => {
                if !matches!(data.len(), 10 | 14 | 16) {
                    return Err(damaged("a DIMENSIONS record has a length it cannot have"));
                }
                return Ok(None);
            }
            MERGECELLS => {
                let count = u16_at(data, 0).ok_or_else(|| short("MERGECELLS"))?;
                if data.len() < usize::from(count).saturating_mul(8).saturating_add(2) {
                    return Err(short("MERGECELLS"));
                }
                return Ok(None);
            }
            _ => return Ok(None),
        };
        Ok(Some(cell))
    }

    fn next_of_multiple(&mut self) -> Option<Cell<'a>> {
        let multiple = self.multiple.as_mut()?;
        let (Some(rk), Some(rest)) = (multiple.rks.get(..6), multiple.rks.get(6..)) else {
            self.multiple = None;
            return None;
        };
        let value = self.context.rk(rk)?;
        let cell = (multiple.row, multiple.column, value);
        multiple.rks = rest;
        multiple.column = multiple.column.saturating_add(1);
        Some(cell)
    }
}

/// The cached result of a formula (8 bytes): a number, or, when its last
/// two bytes are `FF FF`, text (in a STRING record next, so none here), a
/// boolean, an error or empty text, by its first byte.
fn formula_result(
    context: &Context,
    result: &[u8],
    xf: u16,
) -> Result<Option<XlsValue<'static>>, XlsError> {
    match result {
        [0x00, .., 0xFF, 0xFF] => Ok(None),
        [0x01, _, value, .., 0xFF, 0xFF] => Ok(Some(XlsValue::Bool(*value != 0))),
        [0x02, _, code, .., 0xFF, 0xFF] => error(*code).map(Some),
        [0x03, .., 0xFF, 0xFF] => Ok(Some(XlsValue::Text(Cow::Borrowed("")))),
        [kind, .., 0xFF, 0xFF] => Err(damaged(format!(
            "a formula has a cached result of an unknown type {kind:#x}"
        ))),
        _ => {
            let value = f64_at(result, 0).ok_or_else(|| short("FORMULA"))?;
            Ok(Some(context.number(value, xf)))
        }
    }
}

impl<'a> Iterator for Cells<'a> {
    type Item = Result<Cell<'a>, XlsError>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(cell) = self.next_of_multiple() {
            return Some(finite(cell));
        }
        while !self.done {
            let record = match self.records.next() {
                Some(Ok(record)) => record,
                Some(Err(error)) => {
                    self.done = true;
                    return Some(Err(error));
                }
                None => break,
            };
            if self.depth > 0 {
                match record.kind {
                    BOF => self.depth = self.depth.saturating_add(1),
                    EOF => self.depth = self.depth.saturating_sub(1),
                    _ => {}
                }
                continue;
            }
            match record.kind {
                BOF if self.seen_bof => self.depth = 1,
                BOF => self.seen_bof = true,
                EOF => {
                    self.ended = true;
                    break;
                }
                _ => match self.cell(record) {
                    Ok(Some(cell)) => return Some(finite(cell)),
                    Ok(None) => {}
                    Err(error) => {
                        self.done = true;
                        return Some(Err(error));
                    }
                },
            }
        }
        self.done = true;
        None
    }
}
