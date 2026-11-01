//! Reading XLS workbooks: the BIFF8 (Excel 97 to 2003) or BIFF5 (Excel 5
//! and 95) records of the `Workbook` or `Book` stream of a compound file.
//!
//! [`Xls::open`] reads the compound file ([`cfb`]), copies the workbook
//! stream out of it, and reads the workbook's globals: the sheets, the
//! code page, the date system, the number formats and the shared strings.
//! Each sheet's records are read once to check them, keeping nothing, so a
//! damaged sheet makes the whole workbook unreadable whichever sheet a
//! table is made of. [`Xls::cells`] then reads a sheet's cells one record
//! at a time, in the order the file stores them: what a sheet costs is
//! the stream, the shared strings and one record, never the area its
//! cells span.
//!
//! What the reader holds is bounded by [`XlsLimits`]: the workbook stream
//! by `max_stream_bytes` (measured on the sectors that hold it, before it
//! is copied) and the shared strings' text by `max_text_bytes` (charged as
//! each string is decoded; the count the table declares reserves nothing).
//! Every chain of sectors is followed at most as many steps as the file has
//! sectors, so a chain that loops is refused.
//!
//! # The interface
//!
//! ```text
//! pub(crate) struct XlsLimits { pub max_stream_bytes: u64, pub max_text_bytes: u64 }
//! pub(crate) enum XlsValue<'a> { Text(Cow<'a, str>), Number(f64), Int(i64),
//!                                DateTime(f64), Bool(bool), Error(&'static str) }
//! pub(crate) enum XlsError { NotXls, Encrypted, Damaged(String), TooLarge(String) }
//! impl Xls {
//!     pub(crate) fn open(bytes: &[u8], limits: XlsLimits) -> Result<Xls, XlsError>;
//!     pub(crate) fn sheet_names(&self) -> &[String];
//!     pub(crate) fn date1904(&self) -> bool;
//!     pub(crate) fn cells(&self, sheet: usize)
//!         -> Result<impl Iterator<Item = Result<(u32, u32, XlsValue<'_>), XlsError>> + '_, XlsError>;
//! }
//! ```
//!
//! Why it is shaped as it is:
//!
//! - `XlsValue` has a lifetime, and `Text` is a `Cow<str>`: the text of a
//!   shared string is lent from the table, so a string that a million
//!   cells use is measured (and trimmed, and counted against the text
//!   limit) before it is copied, never copied a million times first.
//!   Labels and formula results are owned.
//! - `XlsValue::Int(i64)`: an RK value stored as an integer is one, as an
//!   integer cell (`Cell::Int`), the way the table has always read it; an
//!   integer in a date format is a `DateTime`.
//! - `Xls::date1904()`: `DateTime` holds the serial number the file stores,
//!   counted from 1900-01-00 or, when this is true, from 1904-01-01 (1,462
//!   days later), the same two systems the XLSX reader converts.
//! - `XlsValue::Error` holds the name the table writes after `#`: `Null`,
//!   `Div0`, `Value`, `Ref`, `Name`, `Num`, `NA`, `GettingData` (so a cell
//!   reads `#Div0`, as it always has).
//! - `cells` also gives cells whose text is empty (a formula that results
//!   in `""`): they are cells, and a later cell at the same position
//!   replaces an earlier one. Blank cells (BLANK, MULBLANK) are skipped.
//!
//! Dates are found as the XLSX and XLSB readers find them, by the same
//! code: a number in a built-in date or time format (ids 14 to 22, 27 to
//! 36, 45 to 47, 50 to 58) or in a custom format whose first section has a
//! date or time token outside quotes, brackets and escapes
//! ([`shows_date`](super::workbook::dates::shows_date)).
#![deny(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

mod biff;
mod cfb;
#[cfg(test)]
mod tests;
#[cfg(any(test, feature = "fuzzing"))]
pub(crate) mod write;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use biff::{Biff, Context, SharedStrings};

use super::workbook::dates::builtin_shows_date;

/// What reading an XLS workbook may hold.
#[derive(Clone, Copy, Debug)]
pub(crate) struct XlsLimits {
    /// The most bytes the workbook stream may have.
    pub(crate) max_stream_bytes: u64,
    /// The most bytes of text the shared strings may hold.
    pub(crate) max_text_bytes: u64,
}

/// The value of a cell.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum XlsValue<'a> {
    /// Text: lent from the shared strings, or owned.
    Text(Cow<'a, str>),
    /// A number.
    Number(f64),
    /// A number stored as an integer.
    Int(i64),
    /// A number in a date or time format: the serial number in the
    /// workbook's date system ([`Xls::date1904`]).
    DateTime(f64),
    /// A boolean.
    Bool(bool),
    /// An error, by the name the table writes after `#` (`Div0`, `NA`).
    Error(&'static str),
}

/// Why a workbook cannot be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum XlsError {
    /// Not a compound file, or one without a workbook stream (a Word
    /// document).
    NotXls,
    /// The workbook is encrypted (FILEPASS).
    Encrypted,
    /// The file breaks the format: what is wrong.
    Damaged(String),
    /// The file holds more than a limit allows: which.
    TooLarge(String),
}

impl fmt::Display for XlsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XlsError::NotXls => f.write_str("the file is not an XLS workbook"),
            XlsError::Encrypted => f.write_str("the workbook is encrypted"),
            XlsError::Damaged(what) => write!(f, "the workbook is damaged: {what}"),
            XlsError::TooLarge(what) => f.write_str(what),
        }
    }
}

impl std::error::Error for XlsError {}

fn damaged(what: impl Into<String>) -> XlsError {
    XlsError::Damaged(what.into())
}

/// The ids a FORMAT record may give a number format of its own; a FORMAT
/// record for another id is not used.
fn is_custom_format_id(id: u16) -> bool {
    matches!(id, 5..=8 | 23..=26 | 41..=44 | 63..=66 | 164..=382)
}

/// A worksheet: its name and where its records start in the stream.
#[derive(Clone, Debug)]
struct SheetAt {
    name: String,
    offset: usize,
}

/// An XLS workbook, open.
pub(crate) struct Xls {
    context: Context,
    names: Vec<String>,
    sheets: Vec<SheetAt>,
    date1904: bool,
}

impl fmt::Debug for Xls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Xls").field("sheets", &self.names).finish()
    }
}

impl Xls {
    /// Opens the workbook in `bytes`, holding no more than `limits` allow.
    pub(crate) fn open(bytes: &[u8], limits: XlsLimits) -> Result<Xls, XlsError> {
        let stream = cfb::workbook_stream(bytes, limits.max_stream_bytes)?;
        let globals = biff::read_globals(&stream, limits.max_text_bytes)?;
        let formats = globals
            .formats
            .iter()
            .map(|id| {
                globals
                    .custom_formats
                    .get(id)
                    .copied()
                    .unwrap_or_else(|| builtin_shows_date(*id))
            })
            .collect();
        let context = Context {
            stream,
            biff: globals.biff,
            codepage: globals.codepage,
            formats,
            shared: globals.shared,
        };
        // Every sheet is read once, so that a damaged one is found now.
        for sheet in &globals.sheets {
            let cells = context.cells_at(sheet.offset)?;
            for cell in cells {
                cell?;
            }
        }
        let sheets: Vec<SheetAt> = globals
            .sheets
            .into_iter()
            .filter(|sheet| sheet.kind == biff::SheetKind::Worksheet)
            .map(|sheet| SheetAt {
                name: sheet.name,
                offset: sheet.offset,
            })
            .collect();
        Ok(Xls {
            names: sheets.iter().map(|sheet| sheet.name.clone()).collect(),
            context,
            sheets,
            date1904: globals.date1904,
        })
    }

    /// The worksheets' names (no chart or macro sheets), in workbook order.
    pub(crate) fn sheet_names(&self) -> &[String] {
        &self.names
    }

    /// How many shared strings the workbook has room for.
    #[cfg(test)]
    fn shared_capacity(&self) -> usize {
        self.context.shared.capacity()
    }

    /// Whether dates count from 1904-01-01 rather than 1900-01-00.
    pub(crate) fn date1904(&self) -> bool {
        self.date1904
    }

    /// The cells of worksheet `sheet` (an index into [`Xls::sheet_names`])
    /// in the order the file stores them, blank cells skipped.
    pub(crate) fn cells(
        &self,
        sheet: usize,
    ) -> Result<impl Iterator<Item = Result<(u32, u32, XlsValue<'_>), XlsError>> + '_, XlsError>
    {
        let sheet = self
            .sheets
            .get(sheet)
            .ok_or_else(|| damaged(format!("the workbook has no worksheet {sheet}")))?;
        self.context.cells_at(sheet.offset)
    }
}

/// What the globals of a workbook say, as they are read.
struct Globals {
    biff: Biff,
    codepage: &'static encoding_rs::Encoding,
    date1904: bool,
    /// The number format of each XF, by index.
    formats: Vec<u16>,
    /// Whether each custom number format shows dates, by its id.
    custom_formats: BTreeMap<u16, bool>,
    sheets: Vec<biff::BoundSheet>,
    shared: SharedStrings,
}
