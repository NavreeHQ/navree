//! `navree_sheets`: an uploaded spreadsheet read into a table, whole (CSV,
//! XLSX, XLSB, XLS, ODS) or as one part of a workbook built around it, so
//! that every reader is reached past the zip file.
//!
//! Input: a byte that says what the rest is, then the rest. 0 a file, 1 a
//! CSV file; 2 to 6 an XLSX workbook's sheet, shared strings, styles,
//! workbook or relationships; 7 an ODS spreadsheet's `content.xml`; 8 to 11
//! an XLSB workbook's cell records, shared strings, styles or workbook.
//!
//! Invariants: reading returns a table or an error; a table keeps to its
//! limits of rows, columns, cells and text, every row as wide as the header
//! and in order, text trimmed.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_sheets::fuzzing;

fuzz_target!(|data: &[u8]| {
    let Some((&kind, rest)) = data.split_first() else {
        return;
    };
    match kind % 12 {
        0 => fuzzing::read(rest, "upload.xlsx"),
        1 => fuzzing::read(rest, "upload.csv"),
        part @ 2..=6 => fuzzing::read(&fuzzing::xlsx_with(part - 2, rest), "book.xlsx"),
        7 => fuzzing::read(&fuzzing::ods_with(rest), "book.ods"),
        part => fuzzing::read(&fuzzing::xlsb_with(part - 8, rest), "book.xlsb"),
    }
});
