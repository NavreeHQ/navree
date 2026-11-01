//! Reading workbooks: XLSX, XLSB and ODS files, each opened from its zip
//! file and read as a [`CellSource`], the cells of a sheet in the order the
//! file stores them, for the table to be made of as they come.
//!
//! A reader holds what every sheet needs (the shared strings, which number
//! formats are dates, the sheet names) and streams the sheet it is asked
//! for: no reader holds a sheet. What a reader keeps is bounded by
//! [`Limits`], and a part of the zip file stops at the size the file
//! declares for it.

mod archive;
pub(crate) mod dates;
mod ods;
mod xlsb;
mod xlsx;
mod xml;

#[cfg(test)]
mod parity_tests;

use std::fmt;

use self::archive::Archive;
use super::read::FileKind;
use crate::{Cell, SheetsError};

/// What a workbook reader may hold.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// The most a part of the zip file may unpack to, and the most the
    /// shared strings, held for the whole reading, may take.
    pub(crate) max_part_bytes: u64,
    /// The most text one cell may be made of, where a file can make more
    /// text than it holds (ODS repeats a space as many times as it says).
    pub(crate) max_text_bytes: u64,
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

/// What is wrong with a workbook that cannot be read.
#[derive(Debug)]
pub(crate) struct Damaged(pub(crate) String);

pub(crate) fn damaged(detail: impl fmt::Display) -> Damaged {
    Damaged(detail.to_string())
}

impl From<Damaged> for SheetsError {
    fn from(damaged: Damaged) -> Self {
        SheetsError::UnsupportedFile(format!("The workbook cannot be read: {}.", damaged.0))
    }
}

impl From<quick_xml::Error> for Damaged {
    fn from(error: quick_xml::Error) -> Self {
        damaged(format_args!("its XML: {error}"))
    }
}

impl From<quick_xml::events::attributes::AttrError> for Damaged {
    fn from(error: quick_xml::events::attributes::AttrError) -> Self {
        damaged(format_args!("its XML: {error}"))
    }
}

impl From<std::io::Error> for Damaged {
    fn from(error: std::io::Error) -> Self {
        damaged(error)
    }
}

/// A workbook in a zip file.
pub(crate) enum Workbook<'a> {
    Xlsx(xlsx::Xlsx<'a>),
    Xlsb(xlsb::Xlsb<'a>),
    Ods(ods::Ods<'a>),
}

impl<'a> Workbook<'a> {
    /// Opens the workbook in the zip file `bytes`: an XLSX workbook, an XLSB
    /// one, or an ODS spreadsheet, whichever of them it reads as, in that
    /// order. When it reads as none, the reason given is that of the format
    /// its parts name.
    pub(crate) fn open(bytes: &'a [u8], limits: Limits) -> Result<Self, SheetsError> {
        let archive = Archive::open(bytes)?;
        let xlsx = match xlsx::Xlsx::open(&archive, limits) {
            Ok(xlsx) => return Ok(Workbook::Xlsx(xlsx)),
            Err(error) => error,
        };
        let xlsb = match xlsb::Xlsb::open(&archive, limits) {
            Ok(xlsb) => return Ok(Workbook::Xlsb(xlsb)),
            Err(error) => error,
        };
        let ods = match ods::Ods::open(&archive, limits) {
            Ok(ods) => return Ok(Workbook::Ods(ods)),
            Err(error) => error,
        };
        Err(if archive.has("mimetype") {
            ods
        } else if archive.has("xl/workbook.bin") {
            xlsb
        } else {
            xlsx
        })
    }

    /// The kind of file: XLSB is an Excel workbook as XLSX is.
    pub(crate) fn kind(&self) -> FileKind {
        match self {
            Workbook::Ods(_) => FileKind::Ods,
            Workbook::Xlsx(_) | Workbook::Xlsb(_) => FileKind::Xlsx,
        }
    }

    /// The names of the sheets, in the workbook's order, chart sheets too.
    pub(crate) fn sheet_names(&self) -> &[String] {
        match self {
            Workbook::Xlsx(book) => book.sheet_names(),
            Workbook::Xlsb(book) => book.sheet_names(),
            Workbook::Ods(book) => book.sheet_names(),
        }
    }

    /// Reads the cells of the sheet at `sheet` with `read`, given none when
    /// the sheet holds no cells to read (a chart sheet).
    pub(crate) fn read_cells<T>(
        &self,
        sheet: usize,
        read: impl FnOnce(Option<&mut dyn CellSource>) -> Result<T, SheetsError>,
    ) -> Result<T, SheetsError> {
        match self {
            Workbook::Xlsx(book) => book.read_cells(sheet, read),
            Workbook::Xlsb(book) => book.read_cells(sheet, read),
            Workbook::Ods(book) => book.read_cells(sheet, read),
        }
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

    /// Adds `text`, trimmed, after counting what it takes.
    fn push(&mut self, text: &str) -> Result<(), SheetsError> {
        let text = text.trim();
        self.taken = self.taken.saturating_add(text.len() as u64 + 4);
        let end = u32::try_from(self.text.len() + text.len());
        let Ok(end) = end.map_err(drop).and_then(|end| {
            if self.taken > self.limit {
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
