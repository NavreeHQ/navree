//! Reading workbooks: XLSX, XLSB and ODS files, each opened from its zip
//! file, and XLS files, from their compound file; each read as a
//! [`CellSource`], the cells of a sheet in the order the file stores them,
//! for the table to be made of as they come.
//!
//! A reader holds what every sheet needs (the shared strings, which number
//! formats are dates, the sheet names) and streams the sheet it is asked
//! for: no reader holds a sheet. A part of the zip file stops at the size the
//! file declares for it.
//!
//! Opening a workbook cannot be interrupted, so it is bounded in memory and
//! in work. Everything a zip workbook keeps while it opens is charged to one
//! [`Budget`] of `max_part_bytes`, and what it unpacks to four times that, for
//! all the readers it tries together. Each part is read in one pass, in time
//! in proportion to its bytes; a sheet's part is not read until a sheet is
//! asked for, and that reading can stop at any cell. An XLS workbook holds its
//! stream and its shared strings, each with a limit of its own, and goes
//! through the stream at most twice ([`super::xls`]).

mod archive;
pub(crate) mod dates;
mod ods;
mod package;
mod xlsb;
mod xlsx;
mod xml;

#[cfg(test)]
pub(crate) mod parity_tests;
#[cfg(test)]
pub(crate) mod xlsb_writer;

use std::borrow::Cow;
use std::cell::Cell as Counter;
use std::collections::TryReserveError;
use std::fmt;
use std::marker::PhantomData;
use std::sync::atomic::AtomicBool;

use self::archive::Archive;
use super::read::{FileKind, OLE};
use super::xls::{Xls, XlsError, XlsLimits, XlsValue};
use crate::{Cell, SheetsError};

/// What a workbook reader may hold.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// The most a part of the zip file may unpack to, and the most all that
    /// a zip workbook keeps while it opens may take (the shared strings,
    /// held for the whole reading, the sheet names, the relationships and
    /// the formats); for XLS, the most its workbook stream may hold.
    pub(crate) max_part_bytes: u64,
    /// The most text one row of an ODS sheet may be made of, where a file
    /// can make more text than it holds (ODS repeats a space as many times as
    /// it says); for XLS, the most text its shared strings may hold.
    pub(crate) max_text_bytes: u64,
}

/// The most sheets a workbook may have, in every format: the most Excel can
/// number. A sheet costs a place in several lists, and no workbook needs more.
pub(crate) const MAX_SHEETS: usize = 65_535;

/// That a workbook has more sheets than one may have.
pub(crate) fn too_many_sheets() -> String {
    format!("The workbook has more than {MAX_SHEETS} sheets.")
}

/// What opening a zip workbook may cost: what it keeps and what it reads,
/// counted across every list it builds and every part it unpacks.
#[derive(Debug)]
pub(crate) struct Budget {
    limit: u64,
    held: Counter<u64>,
    max_read: u64,
    read: Counter<u64>,
}

impl Budget {
    fn new(limits: Limits) -> Self {
        Budget {
            limit: limits.max_part_bytes,
            held: Counter::new(0),
            // However a valid workbook shares out its bytes, it is read well
            // within this; a limit that small still reads a small file.
            max_read: limits.max_part_bytes.saturating_mul(4).max(1 << 20),
            read: Counter::new(0),
        }
    }

    /// Counts `bytes` that opening keeps, and refuses the file when that
    /// makes more than the limit.
    fn hold(&self, bytes: usize) -> Result<(), Damaged> {
        let held = self.held.get().saturating_add(bytes as u64);
        self.held.set(held);
        if held > self.limit {
            return Err(damaged(format_args!(
                "what it holds to be opened takes more than {} bytes",
                self.limit
            )));
        }
        Ok(())
    }

    /// Counts `bytes` that opening unpacks; false when that makes more than
    /// it may read.
    fn charge_read(&self, bytes: usize) -> bool {
        let read = self.read.get().saturating_add(bytes as u64);
        self.read.set(read);
        read <= self.max_read
    }

    /// Whether opening has read all it may, so that no reader can read the
    /// file.
    fn read_out(&self) -> bool {
        self.read.get() > self.max_read
    }

    /// The reader that held these has given up, and they are gone with it.
    fn release(&self) {
        self.held.set(0);
    }
}

/// The room a list of `capacity` should grow to, to hold `needed`: double, as
/// a list does, but never past `limit` unless the list itself must be.
fn room_for(capacity: usize, needed: usize, limit: u64) -> usize {
    capacity
        .saturating_mul(2)
        .min(usize::try_from(limit).unwrap_or(usize::MAX))
        .max(needed)
}

/// Makes room in `text` for `more` bytes, within `limit`.
pub(crate) fn make_room(text: &mut String, more: usize, limit: u64) -> Result<(), TryReserveError> {
    let needed = text.len().saturating_add(more);
    if needed <= text.capacity() {
        return Ok(());
    }
    text.try_reserve_exact(room_for(text.capacity(), needed, limit) - text.len())
}

/// A value of a cell.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value<'a> {
    /// Text, trimmed: empty when the cell holds only spaces.
    Text(&'a str),
    /// A number, a date or a boolean.
    Typed(Cell),
}

/// A cell with a value, where it is: row and column from 0.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceCell<'a> {
    pub(crate) row: u32,
    pub(crate) column: u32,
    /// The columns, from `column` on, that hold the value: more than one
    /// for an ODS cell that repeats.
    pub(crate) columns: u32,
    pub(crate) value: Value<'a>,
}

impl SourceCell<'_> {
    /// The position of the last column the cell covers.
    pub(crate) fn last(&self) -> (u32, u32) {
        (
            self.row,
            self.column.saturating_add(self.columns.saturating_sub(1)),
        )
    }
}

/// The cells of a sheet in the order its file stores them, without the ones
/// that hold nothing (a styled empty cell, a formula without a value).
pub(crate) trait CellSource {
    /// The next cell; none at the end of the sheet.
    fn next_cell(&mut self) -> Result<Option<SourceCell<'_>>, SheetsError>;

    /// Says that no cell from column `width` on is kept: of the cells of a
    /// row from there on, a source may give only the last, which tells how
    /// far the row reaches.
    fn cut_at(&mut self, width: usize) {
        let _ = width;
    }

    /// Reads the rest of the sheet, keeping nothing, and says whether its
    /// cells come after `last` and in order, each position once. `running`
    /// is asked along the way whether to go on.
    fn rest_in_order(
        &mut self,
        mut last: Option<(u32, u32)>,
        running: &dyn Fn() -> Result<(), SheetsError>,
    ) -> Result<bool, SheetsError> {
        while let Some(cell) = self.next_cell()? {
            running()?;
            if last.is_some_and(|last| (cell.row, cell.column) <= last) {
                return Ok(false);
            }
            last = Some(cell.last());
        }
        Ok(true)
    }
}

/// What is wrong with a workbook that cannot be read, or that the reading
/// was told to stop (nobody waits for it any more), which is nothing wrong
/// with the workbook.
#[derive(Debug)]
pub(crate) struct Damaged {
    pub(crate) detail: String,
    stopped: bool,
}

pub(crate) fn damaged(detail: impl fmt::Display) -> Damaged {
    Damaged {
        detail: detail.to_string(),
        stopped: false,
    }
}

impl Damaged {
    /// The reading was told to stop.
    fn stopped() -> Self {
        Damaged {
            detail: Stopped.to_string(),
            stopped: true,
        }
    }
}

impl From<Damaged> for SheetsError {
    fn from(damaged: Damaged) -> Self {
        if damaged.stopped {
            return interrupted();
        }
        SheetsError::UnsupportedFile(format!("The workbook cannot be read: {}.", damaged.detail))
    }
}

/// The error of a reading that was told to stop, because nobody waits for the
/// file to be read any more.
pub(crate) fn interrupted() -> SheetsError {
    SheetsError::Io(std::io::Error::new(
        std::io::ErrorKind::Interrupted,
        "nobody is waiting for the file to be read",
    ))
}

/// What a part of the zip file fails with, in the middle of a read, once the
/// reading is told to stop.
#[derive(Debug)]
struct Stopped;

impl fmt::Display for Stopped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the reading was told to stop")
    }
}

impl std::error::Error for Stopped {}

/// Whether `error` is the one a part gives when the reading is told to stop.
fn is_stop(error: &std::io::Error) -> bool {
    error.get_ref().is_some_and(|inner| inner.is::<Stopped>())
}

impl From<quick_xml::Error> for Damaged {
    fn from(error: quick_xml::Error) -> Self {
        match &error {
            quick_xml::Error::Io(io) if is_stop(io) => Damaged::stopped(),
            _ => damaged(format_args!("its XML: {error}")),
        }
    }
}

impl From<quick_xml::events::attributes::AttrError> for Damaged {
    fn from(error: quick_xml::events::attributes::AttrError) -> Self {
        damaged(format_args!("its XML: {error}"))
    }
}

impl From<std::io::Error> for Damaged {
    fn from(error: std::io::Error) -> Self {
        if is_stop(&error) {
            return Damaged::stopped();
        }
        damaged(error)
    }
}

/// A workbook.
pub(crate) enum Workbook<'a> {
    Xlsx(xlsx::Xlsx<'a>),
    Xlsb(xlsb::Xlsb<'a>),
    Ods(ods::Ods<'a>),
    Xls(Xls),
}

/// Why an XLS workbook cannot be read, as the file's refusal.
fn xls_error(error: XlsError) -> SheetsError {
    match error {
        XlsError::TooLarge(what) => SheetsError::UnsupportedFile(what),
        XlsError::Interrupted => interrupted(),
        other => damaged(other).into(),
    }
}

impl<'a> Workbook<'a> {
    /// Opens the workbook in `bytes`. A file that starts as a compound file
    /// does is read as XLS; when it does not read as one, or starts as
    /// anything else, it is read as a zip file (which may come after a
    /// prefix): an XLSX workbook, an XLSB one, or an ODS spreadsheet,
    /// whichever of them it reads as, in that order. When it reads as none,
    /// the reason given is that of the XLS reader for a compound file, and
    /// of the format its parts name for a zip file.
    pub(crate) fn open(bytes: &'a [u8], limits: Limits) -> Result<Self, SheetsError> {
        let xls = if bytes.starts_with(OLE) {
            let xls_limits = XlsLimits {
                max_stream_bytes: limits.max_part_bytes,
                max_text_bytes: limits.max_text_bytes,
            };
            match Xls::open(bytes, xls_limits) {
                Ok(xls) => return Ok(Workbook::Xls(xls)),
                Err(error) => Some(error),
            }
        } else {
            None
        };
        Self::open_zip(bytes, limits).map_err(|zip| xls.map_or(zip, xls_error))
    }

    fn open_zip(bytes: &'a [u8], limits: Limits) -> Result<Self, SheetsError> {
        let archive = Archive::open(bytes)?;
        let budget = Budget::new(limits);
        let xlsx = match xlsx::Xlsx::open(&archive, &budget) {
            Ok(xlsx) => return Ok(Workbook::Xlsx(xlsx)),
            Err(error) => error,
        };
        budget.release();
        let xlsb = match xlsb::Xlsb::open(&archive, &budget) {
            Ok(xlsb) => return Ok(Workbook::Xlsb(xlsb)),
            Err(error) => error,
        };
        budget.release();
        let ods = match ods::Ods::open(&archive, limits, &budget) {
            Ok(ods) => return Ok(Workbook::Ods(ods)),
            Err(error) => error,
        };
        if budget.read_out() {
            return Err(SheetsError::UnsupportedFile(format!(
                "The workbook takes more than {} bytes of reading to open.",
                budget.max_read
            )));
        }
        Err(if archive.has("mimetype") {
            ods
        } else if archive.has("xl/workbook.bin") || xlsb::names_a_binary_workbook(&archive, &budget)
        {
            xlsb
        } else {
            xlsx
        })
    }

    /// The kind of file: XLSB is an Excel workbook as XLSX is.
    pub(crate) fn kind(&self) -> FileKind {
        match self {
            Workbook::Ods(_) => FileKind::Ods,
            Workbook::Xls(_) => FileKind::Xls,
            Workbook::Xlsx(_) | Workbook::Xlsb(_) => FileKind::Xlsx,
        }
    }

    /// The names of the sheets, in the workbook's order: chart and macro
    /// sheets too, but for XLS, whose chart and macro sheets are not listed.
    pub(crate) fn sheet_names(&self) -> &[String] {
        match self {
            Workbook::Xlsx(book) => book.sheet_names(),
            Workbook::Xlsb(book) => book.sheet_names(),
            Workbook::Ods(book) => book.sheet_names(),
            Workbook::Xls(book) => book.sheet_names(),
        }
    }

    /// Reads the cells of the sheet at `sheet` with `read`, given none when
    /// the sheet holds no cells to read (a chart or a macro sheet). `stop`
    /// is raised when nobody waits for the reading any more: the sources
    /// look at it as they read, whether or not a cell comes of what they
    /// read, and end the reading with [`interrupted`]. A zip part's bytes
    /// are looked at every 64 KiB it unpacks, and an XLS sheet's records
    /// every 1,024 records.
    pub(crate) fn read_cells<T>(
        &self,
        sheet: usize,
        stop: Option<&AtomicBool>,
        read: impl FnOnce(Option<&mut dyn CellSource>) -> Result<T, SheetsError>,
    ) -> Result<T, SheetsError> {
        match self {
            Workbook::Xlsx(book) => book.read_cells(sheet, stop, read),
            Workbook::Xlsb(book) => book.read_cells(sheet, stop, read),
            Workbook::Ods(book) => book.read_cells(sheet, stop, read),
            Workbook::Xls(book) => {
                let mut cells = XlsCells {
                    cells: book.cells(sheet, stop).map_err(xls_error)?,
                    date1904: book.date1904(),
                    text: String::new(),
                    lent: PhantomData,
                };
                read(Some(&mut cells))
            }
        }
    }
}

/// The cells of an XLS sheet, as the XLS reader gives them: their shared
/// strings are lent for `'a`.
struct XlsCells<'a, I> {
    cells: I,
    date1904: bool,
    /// The text of the cell last read, when the reader gave it owned.
    text: String,
    lent: PhantomData<&'a str>,
}

impl<'a, I> CellSource for XlsCells<'a, I>
where
    I: Iterator<Item = Result<(u32, u32, XlsValue<'a>), XlsError>>,
{
    fn next_cell(&mut self) -> Result<Option<SourceCell<'_>>, SheetsError> {
        let Some(next) = self.cells.next() else {
            return Ok(None);
        };
        let (row, column, value) = next.map_err(xls_error)?;
        let value = match value {
            // A shared string is measured where it is, not copied, and it is
            // trimmed already.
            XlsValue::Text(Cow::Borrowed(text)) => Value::Text(text),
            XlsValue::Text(Cow::Owned(text)) => {
                self.text = text;
                Value::Text(self.text.trim())
            }
            XlsValue::Number(number) => Value::Typed(Cell::Float(number)),
            XlsValue::Int(number) => Value::Typed(Cell::Int(number)),
            XlsValue::DateTime(serial) => Value::Typed(dates::excel_date(serial, self.date1904)),
            XlsValue::Bool(value) => Value::Typed(Cell::Bool(value)),
            XlsValue::Error(name) => {
                self.text = format!("#{name}");
                Value::Text(&self.text)
            }
        };
        Ok(Some(SourceCell {
            row,
            column,
            columns: 1,
            value,
        }))
    }
}

/// A workbook's shared strings, each trimmed (the table keeps text
/// trimmed), in one text: four bytes for each string besides its text.
#[derive(Debug)]
struct SharedStrings {
    text: String,
    /// Where each string ends in `text`.
    ends: Vec<u32>,
    /// What the strings may take, in bytes.
    limit: u64,
    /// What they take.
    taken: u64,
}

impl SharedStrings {
    fn new(limit: u64) -> Self {
        SharedStrings {
            text: String::new(),
            ends: Vec::new(),
            limit,
            taken: 0,
        }
    }

    /// No strings yet, in what `budget` has left.
    fn within(budget: &Budget) -> Self {
        SharedStrings {
            taken: budget.held.get(),
            ..Self::new(budget.limit)
        }
    }

    /// Adds `text`, trimmed, after counting what it takes.
    fn push(&mut self, text: &str) -> Result<(), SheetsError> {
        let text = text.trim();
        self.taken = self.taken.saturating_add(text.len() as u64 + 4);
        let end = u32::try_from(self.text.len() + text.len());
        let Ok(end) = end.map_err(drop).and_then(|end| {
            let ends = room_for(self.ends.capacity(), self.ends.len() + 1, self.limit / 4);
            if self.taken > self.limit
                || make_room(&mut self.text, text.len(), self.limit).is_err()
                || self.ends.try_reserve_exact(ends - self.ends.len()).is_err()
            {
                Err(())
            } else {
                Ok(end)
            }
        }) else {
            return Err(SheetsError::UnsupportedFile(format!(
                "The workbook's shared strings take more than {} bytes.",
                self.limit.min(u64::from(u32::MAX))
            )));
        };
        self.text.push_str(text);
        self.ends.push(end);
        Ok(())
    }

    /// The strings are all in: they keep no room to grow, and `budget` has
    /// counted what they take.
    fn finish(&mut self, budget: &Budget) {
        self.text.shrink_to_fit();
        self.ends.shrink_to_fit();
        budget.held.set(self.taken);
    }

    fn get(&self, index: usize) -> Option<&str> {
        let end = *self.ends.get(index)? as usize;
        let start = match index.checked_sub(1) {
            Some(before) => *self.ends.get(before)? as usize,
            None => 0,
        };
        self.text.get(start..end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_strings_are_kept_trimmed_and_counted() {
        let mut strings = SharedStrings::new(100);
        strings.push("  a ").unwrap();
        strings.push("").unwrap();
        strings.push("bc").unwrap();
        assert_eq!(strings.get(0), Some("a"));
        assert_eq!(strings.get(1), Some(""));
        assert_eq!(strings.get(2), Some("bc"));
        assert_eq!(strings.get(3), None);
        // 1 + 0 + 2 bytes and 4 for each.
        assert_eq!(strings.taken, 15);
        let mut strings = SharedStrings::new(10);
        strings.push("12345").unwrap();
        let error = strings.push("x").unwrap_err();
        assert!(
            error.to_string().contains("take more than 10 bytes"),
            "{error}"
        );
    }
}
