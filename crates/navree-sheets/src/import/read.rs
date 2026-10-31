//! Reading an uploaded file into a [`Table`]: the header row and the data
//! rows, from CSV, TSV, XLSX, XLS or ODS.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};

use bytes::Bytes;
use calamine::{Data, Reader, open_workbook_auto_from_rs};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::decode::decode;
use crate::{Cell, ImportOptions, SheetsError};

/// The kind of an uploaded file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    /// Comma-, semicolon-, tab- or pipe-separated text.
    Csv,
    /// An Excel workbook (Office Open XML).
    Xlsx,
    /// A legacy Excel workbook (BIFF).
    Xls,
    /// An OpenDocument spreadsheet.
    Ods,
}

impl FileKind {
    /// The stored name (`csv`).
    pub fn as_str(self) -> &'static str {
        match self {
            FileKind::Csv => "csv",
            FileKind::Xlsx => "xlsx",
            FileKind::Xls => "xls",
            FileKind::Ods => "ods",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "csv" => FileKind::Csv,
            "xlsx" => FileKind::Xlsx,
            "xls" => FileKind::Xls,
            "ods" => FileKind::Ods,
            _ => return None,
        })
    }
}

/// One data row: its row number in the file (the header row is the first
/// row a spreadsheet shows, so the first data row is usually 2) and its
/// cells, one per header.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TableRow {
    pub(crate) number: u64,
    pub(crate) cells: Vec<Cell>,
}

/// A file's header row and data rows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Table {
    pub(crate) kind: FileKind,
    /// The encoding of a text file.
    pub(crate) encoding: Option<&'static str>,
    pub(crate) headers: Vec<String>,
    pub(crate) rows: Vec<TableRow>,
}

/// What reading takes besides the bytes.
#[derive(Clone, Debug)]
pub(crate) struct ReadOptions<'a> {
    /// The text encoding (`windows-1256`); detected when `None`.
    pub(crate) encoding: Option<&'a str>,
    /// The workbook sheet to read; the first when `None`.
    pub(crate) sheet: Option<&'a str>,
    /// The most data rows allowed.
    pub(crate) max_rows: u64,
    /// The most columns allowed.
    pub(crate) max_columns: u64,
    /// The most cells allowed, every row counted at the header's width.
    pub(crate) max_cells: u64,
}

impl Default for ReadOptions<'_> {
    /// No encoding and the first sheet, with no data rows allowed and the
    /// default column and cell limits of an import.
    fn default() -> Self {
        let limits = ImportOptions::default();
        ReadOptions {
            encoding: None,
            sheet: None,
            max_rows: 0,
            max_columns: limits.max_columns,
            max_cells: limits.max_cells,
        }
    }
}

const ZIP: &[u8] = b"PK\x03\x04";
const OLE: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";

/// Formats that are certainly not spreadsheets.
const NOT_SPREADSHEETS: &[(&[u8], &str)] = &[
    (b"%PDF", "a PDF document"),
    (b"\x89PNG", "a PNG image"),
    (b"\xFF\xD8\xFF", "a JPEG image"),
    (b"GIF8", "a GIF image"),
];

/// Refuses, before anything else, a file that is empty or plainly not a
/// spreadsheet (a PDF, an image).
pub(crate) fn refuse(bytes: &[u8]) -> Result<(), SheetsError> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err(SheetsError::UnsupportedFile("The file is empty.".into()));
    }
    for (magic, what) in NOT_SPREADSHEETS {
        if bytes.starts_with(magic) {
            return Err(SheetsError::UnsupportedFile(format!(
                "The file is {what}, not a CSV or Excel file."
            )));
        }
    }
    Ok(())
}

/// Reads `bytes` (a file named `filename`) into a table.
pub(crate) fn read_table(
    bytes: &[u8],
    filename: &str,
    options: &ReadOptions<'_>,
) -> Result<Table, SheetsError> {
    refuse(bytes)?;
    let extension = filename
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    if bytes.starts_with(ZIP) || bytes.starts_with(OLE) {
        return read_workbook(bytes, options);
    }
    if matches!(extension.as_str(), "xlsx" | "xlsm" | "xls" | "ods" | "xlsb") {
        return Err(SheetsError::UnsupportedFile(
            "The file is not a valid Excel or OpenDocument workbook.".into(),
        ));
    }
    let tab = extension == "tsv" || extension == "tab";
    read_text(bytes, tab, options)
}

/// [`read_table`] on Tokio's blocking pool, so that a large file does not
/// hold an async worker thread (where a job's timeout cannot fire).
pub(crate) async fn read_table_blocking(
    bytes: Bytes,
    filename: String,
    options: &ReadOptions<'_>,
) -> Result<Table, SheetsError> {
    let encoding = options.encoding.map(str::to_owned);
    let sheet = options.sheet.map(str::to_owned);
    let (max_rows, max_columns, max_cells) =
        (options.max_rows, options.max_columns, options.max_cells);
    let read = tokio::task::spawn_blocking(move || {
        read_table(
            &bytes,
            &filename,
            &ReadOptions {
                encoding: encoding.as_deref(),
                sheet: sheet.as_deref(),
                max_rows,
                max_columns,
                max_cells,
            },
        )
    })
    .await;
    match read {
        Ok(result) => result,
        // A parser that panics on this file: the file is the problem.
        Err(error) if error.is_panic() => {
            tracing::warn!(target: "navree::sheets", %error, "reading an uploaded file panicked");
            Err(SheetsError::UnsupportedFile(
                "The file cannot be read.".into(),
            ))
        }
        Err(error) => Err(SheetsError::app(error)),
    }
}

fn read_text(bytes: &[u8], tab: bool, options: &ReadOptions<'_>) -> Result<Table, SheetsError> {
    let decoded = decode(bytes, options.encoding)?;
    if decoded.text.contains('\0') {
        return Err(SheetsError::UnsupportedFile(
            "The file is not a text file (it contains NUL bytes).".into(),
        ));
    }
    let delimiter = if tab {
        b'\t'
    } else {
        sniff_delimiter(&decoded.text)
    };
    let mut table = read_records(decoded.text.as_bytes(), delimiter, options)?;
    table.encoding = Some(decoded.encoding);
    Ok(table)
}

/// The rows of delimited text from `source`.
fn read_records(
    source: impl Read,
    delimiter: u8,
    options: &ReadOptions<'_>,
) -> Result<Table, SheetsError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(source);
    let mut tabulator = Tabulator::new(options);
    // A spreadsheet shows one row per line, blank lines included (the csv
    // crate skips them), and a quoted cell spanning lines in one row: the
    // record's line as the csv crate counts it (line breaks inside quotes
    // do not count).
    let mut previous = 0u64;
    for record in reader.records() {
        let record = record.map_err(|error| {
            SheetsError::UnsupportedFile(format!("The CSV file cannot be read: {error}"))
        })?;
        let number = record
            .position()
            .map_or(previous + 1, |position| position.line())
            .max(previous + 1);
        previous = number;
        // Up to the last cell with a value; a wider row is cut to the
        // header's width without turning every cell into a `Cell` first.
        let used = record
            .iter()
            .enumerate()
            .filter(|(_, text)| !text.trim().is_empty())
            .last()
            .map_or(0, |(index, _)| index + 1);
        tabulator.push(number, used, record.iter().map(text_cell))?;
    }
    let mut table = tabulator.finish()?;
    table.kind = FileKind::Csv;
    Ok(table)
}

/// A text cell, trimmed; empty text is [`Cell::Empty`].
fn text_cell(text: &str) -> Cell {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        Cell::Empty
    } else {
        Cell::Text(trimmed.to_owned())
    }
}

/// The delimiter of the header line: the most frequent of `,`, `;`, tab and
/// `|` outside quotes (`,` when none appears).
fn sniff_delimiter(text: &str) -> u8 {
    let mut counts = [(b',', 0usize), (b';', 0), (b'\t', 0), (b'|', 0)];
    let mut quoted = false;
    for byte in text.bytes() {
        match byte {
            b'"' => quoted = !quoted,
            b'\n' if !quoted => break,
            _ if !quoted => {
                if let Some(entry) = counts.iter_mut().find(|(d, _)| *d == byte) {
                    entry.1 += 1;
                }
            }
            _ => {}
        }
    }
    counts
        .iter()
        .fold(
            (b',', 0),
            |best, &(d, n)| if n > best.1 { (d, n) } else { best },
        )
        .0
}

fn read_workbook(bytes: &[u8], options: &ReadOptions<'_>) -> Result<Table, SheetsError> {
    let unreadable = |error: calamine::Error| {
        SheetsError::UnsupportedFile(format!("The workbook cannot be read: {error}"))
    };
    let mut workbook = open_workbook_auto_from_rs(Cursor::new(bytes)).map_err(unreadable)?;
    let kind = match &workbook {
        calamine::Sheets::Xls(_) => FileKind::Xls,
        calamine::Sheets::Ods(_) => FileKind::Ods,
        _ => FileKind::Xlsx,
    };
    let range = match options.sheet.map(str::trim).filter(|s| !s.is_empty()) {
        Some(name) => {
            if !workbook.sheet_names().iter().any(|n| n == name) {
                return Err(SheetsError::UnsupportedFile(format!(
                    "The workbook has no sheet named `{name}`."
                )));
            }
            workbook.worksheet_range(name).map_err(unreadable)?
        }
        None => workbook
            .worksheet_range_at(0)
            .ok_or_else(|| SheetsError::UnsupportedFile("The workbook has no sheets.".into()))?
            .map_err(unreadable)?,
    };
    let first_row = range.start().map_or(0, |(row, _)| u64::from(row));
    let first_col = range.start().map_or(0, |(_, col)| col as usize);
    let mut tabulator = Tabulator::new(options);
    for (index, row) in range.rows().enumerate() {
        // Cells left of the range's first column are empty.
        let mut cells = vec![Cell::Empty; first_col];
        cells.extend(row.iter().map(data_cell));
        let used = cells
            .iter()
            .rposition(|cell| !cell.is_empty())
            .map_or(0, |index| index + 1);
        tabulator.push(first_row + index as u64 + 1, used, cells)?;
    }
    let mut table = tabulator.finish()?;
    table.kind = kind;
    Ok(table)
}

/// A workbook cell as a [`Cell`]: text trimmed, dates as dates (or
/// date-times when they have a time), errors as their text.
fn data_cell(data: &Data) -> Cell {
    match data {
        Data::Empty => Cell::Empty,
        Data::String(text) => text_cell(text),
        Data::Int(n) => Cell::Int(*n),
        Data::Float(n) => Cell::Float(*n),
        Data::Bool(b) => Cell::Bool(*b),
        Data::DateTime(at) => match at.as_datetime() {
            Some(at) if at.time() == chrono::NaiveTime::MIN => Cell::Date(at.date()),
            Some(at) => Cell::DateTime(at),
            None => Cell::Float(at.as_f64()),
        },
        Data::DateTimeIso(text) | Data::DurationIso(text) => text_cell(text),
        Data::Error(error) => Cell::Text(format!("#{error:?}")),
    }
}

/// Builds a [`Table`] from records as they are read, so that no limit is
/// checked after the rows are in memory. The header row is the first row
/// with a value and the data rows follow it; empty rows are dropped, short
/// rows padded, cells past the last header dropped.
struct Tabulator {
    max_rows: u64,
    max_columns: u64,
    max_cells: u64,
    headers: Option<Vec<String>>,
    rows: Vec<TableRow>,
}

impl Tabulator {
    fn new(options: &ReadOptions<'_>) -> Self {
        Tabulator {
            max_rows: options.max_rows,
            max_columns: options.max_columns,
            max_cells: options.max_cells,
            headers: None,
            rows: Vec::new(),
        }
    }

    /// Adds the record on row `number`. `used` is how many of its cells
    /// come before the last one with a value (0 for an empty row), and
    /// `cells` yields them all; only the ones the table keeps are taken.
    fn push(
        &mut self,
        number: u64,
        used: usize,
        cells: impl IntoIterator<Item = Cell>,
    ) -> Result<(), SheetsError> {
        if used == 0 {
            return Ok(());
        }
        let cells = cells.into_iter();
        let Some(headers) = &self.headers else {
            if used as u64 > self.max_columns {
                return Err(SheetsError::UnsupportedFile(format!(
                    "The file has more than {} columns.",
                    self.max_columns
                )));
            }
            let header_cells: Vec<Cell> = cells.take(used).collect();
            self.headers = Some(headers_of(&header_cells));
            return Ok(());
        };
        if self.rows.len() as u64 >= self.max_rows {
            return Err(SheetsError::TooManyRows {
                limit: self.max_rows,
            });
        }
        // Every row is padded to the header's width, so the table holds
        // the header and the rows so far, and this row, at that width.
        let width = headers.len();
        let held = (self.rows.len() as u64 + 2).saturating_mul(width as u64);
        if held > self.max_cells {
            return Err(SheetsError::UnsupportedFile(format!(
                "The file has more than {} cells (rows times columns).",
                self.max_cells
            )));
        }
        let mut row = Vec::with_capacity(width);
        row.extend(cells.take(width));
        row.resize(width, Cell::Empty);
        self.rows.push(TableRow { number, cells: row });
        Ok(())
    }

    fn finish(self) -> Result<Table, SheetsError> {
        let Some(headers) = self.headers else {
            return Err(SheetsError::UnsupportedFile(
                "The file has no header row.".into(),
            ));
        };
        Ok(Table {
            kind: FileKind::Csv,
            encoding: None,
            headers,
            rows: self.rows,
        })
    }
}

/// Header texts: trailing empty headers dropped, blank ones named
/// `Column N`, repeats suffixed ` (2)`, ` (3)`.
fn headers_of(cells: &[Cell]) -> Vec<String> {
    let last = cells
        .iter()
        .rposition(|cell| !cell.is_empty())
        .map_or(0, |i| i + 1);
    let mut headers: Vec<String> = Vec::with_capacity(last);
    let mut taken: HashSet<String> = HashSet::with_capacity(last);
    // The suffix each repeated text tries next: every one below it is taken
    // already (names are only ever added), so a repeat never searches again.
    let mut next_suffix: HashMap<String, u64> = HashMap::new();
    for (index, cell) in cells[..last].iter().enumerate() {
        let text = cell.to_text().trim().to_owned();
        let base = if text.is_empty() {
            format!("Column {}", index + 1)
        } else {
            text
        };
        let name = if taken.contains(&base) {
            let n = next_suffix.entry(base.clone()).or_insert(2);
            loop {
                let name = format!("{base} ({n})");
                *n += 1;
                if !taken.contains(&name) {
                    break name;
                }
            }
        } else {
            base
        };
        taken.insert(name.clone());
        headers.push(name);
    }
    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> ReadOptions<'static> {
        ReadOptions {
            max_rows: 1000,
            ..ReadOptions::default()
        }
    }

    fn text(cells: &[Cell]) -> Vec<String> {
        cells.iter().map(|c| c.to_text().into_owned()).collect()
    }

    #[test]
    fn csv_rows_are_numbered_like_a_spreadsheet() {
        let table = read_table(
            b"\n Name , Email,\nAda, ada@example.com\n,,\nAlan,alan@example.com,extra\nGrace\n",
            "people.csv",
            &options(),
        )
        .unwrap();
        assert_eq!(table.kind, FileKind::Csv);
        assert_eq!(table.encoding, Some("UTF-8"));
        assert_eq!(table.headers, ["Name", "Email"]);
        let numbers: Vec<u64> = table.rows.iter().map(|r| r.number).collect();
        assert_eq!(numbers, [3, 5, 6]);
        assert_eq!(text(&table.rows[0].cells), ["Ada", "ada@example.com"]);
        assert_eq!(text(&table.rows[1].cells), ["Alan", "alan@example.com"]);
        assert_eq!(
            table.rows[2].cells,
            [Cell::Text("Grace".into()), Cell::Empty]
        );
    }

    #[test]
    fn a_cell_spanning_lines_is_one_row() {
        let table = read_table(
            b"Name,Note\nAda,\"first\nsecond\"\n\nAlan,x\n",
            "people.csv",
            &options(),
        )
        .unwrap();
        let numbers: Vec<u64> = table.rows.iter().map(|r| r.number).collect();
        assert_eq!(numbers, [2, 4]);
        assert_eq!(table.rows[0].cells[1], Cell::Text("first\nsecond".into()));
    }

    #[test]
    fn the_delimiter_is_sniffed() {
        for (file, name) in [
            ("a;b\n1;2\n", "x.csv"),
            ("a\tb\n1\t2\n", "x.txt"),
            ("a|b\n1|2\n", "x.csv"),
            ("\"x,y\";b\n1;2\n", "x.csv"),
        ] {
            let table = read_table(file.as_bytes(), name, &options()).unwrap();
            assert_eq!(table.headers.len(), 2, "{file:?}");
            assert_eq!(table.rows[0].cells.len(), 2, "{file:?}");
        }
        let table = read_table(b"a,b\t\n1,2\t\n", "x.tsv", &options()).unwrap();
        assert_eq!(table.headers, ["a,b"]);
    }

    #[test]
    fn headers_are_named_and_kept_apart() {
        let table = read_table(b"Email,,Email,Email\n1,2,3,4\n", "x.csv", &options()).unwrap();
        assert_eq!(
            table.headers,
            ["Email", "Column 2", "Email (2)", "Email (3)"]
        );
    }

    #[test]
    fn too_many_rows_is_refused() {
        let file = "a\n1\n2\n3\n";
        let error = read_table(
            file.as_bytes(),
            "x.csv",
            &ReadOptions {
                max_rows: 2,
                ..ReadOptions::default()
            },
        )
        .unwrap_err();
        assert!(matches!(error, SheetsError::TooManyRows { limit: 2 }));
    }

    #[test]
    fn files_that_are_not_spreadsheets_are_refused() {
        for (bytes, name, message) in [
            (&b"%PDF-1.7"[..], "x.csv", "a PDF document"),
            (b"   \n", "x.csv", "empty"),
            (b"plain", "x.xlsx", "not a valid Excel"),
            (b"PK\x03\x04garbage", "x.xlsx", "cannot be read"),
        ] {
            let error = read_table(bytes, name, &options()).unwrap_err();
            assert!(
                matches!(error, SheetsError::UnsupportedFile(ref m) if m.contains(message)),
                "{name}: {error}"
            );
        }
    }

    #[test]
    fn a_workbook_is_read_with_typed_cells() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let file = crate::write_xlsx(
            "People",
            &[],
            &[
                "Name".into(),
                "Age".into(),
                "Joined".into(),
                "Active".into(),
            ],
            &[vec![
                Cell::Text("Ada".into()),
                Cell::Int(36),
                Cell::Date(date),
                Cell::Bool(true),
            ]],
        )
        .unwrap();
        let table = read_table(&file, "people.xlsx", &options()).unwrap();
        assert_eq!(table.kind, FileKind::Xlsx);
        assert_eq!(table.headers, ["Name", "Age", "Joined", "Active"]);
        assert_eq!(table.rows[0].number, 2);
        assert_eq!(
            table.rows[0].cells,
            [
                Cell::Text("Ada".into()),
                Cell::Float(36.0),
                Cell::Date(date),
                Cell::Bool(true)
            ]
        );
        let error = read_table(
            &file,
            "people.xlsx",
            &ReadOptions {
                sheet: Some("Missing"),
                ..options()
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("no sheet named `Missing`"));
        let table = read_table(
            &file,
            "people.xlsx",
            &ReadOptions {
                sheet: Some("People"),
                ..options()
            },
        )
        .unwrap();
        assert_eq!(table.rows.len(), 1);
    }

    /// What reading came to, without printing a table.
    fn outcome(result: &Result<Table, SheetsError>) -> String {
        match result {
            Ok(table) => format!(
                "read {} headers and {} rows",
                table.headers.len(),
                table.rows.len()
            ),
            Err(error) => format!("refused: {error}"),
        }
    }

    fn refused_for(result: &Result<Table, SheetsError>, what: &str) -> bool {
        matches!(result, Err(SheetsError::UnsupportedFile(message)) if message.contains(what))
    }

    /// `width` headers over `rows` rows of one cell each.
    fn wide_and_short(width: usize, rows: usize) -> String {
        let mut file = (0..width)
            .map(|i| format!("h{i}"))
            .collect::<Vec<_>>()
            .join(",");
        file.push('\n');
        for _ in 0..rows {
            file.push_str("x\n");
        }
        file
    }

    #[test]
    fn rows_padded_to_a_wide_header_count_against_the_cell_limit() {
        // Padding makes 200 columns by 201 rows of cells out of 200 header
        // names and 200 short lines.
        let file = wide_and_short(200, 200);
        let result = read_table(
            file.as_bytes(),
            "x.csv",
            &ReadOptions {
                max_rows: 1_000,
                max_cells: 5_000,
                ..ReadOptions::default()
            },
        );
        assert!(refused_for(&result, "cells"), "{}", outcome(&result));
    }

    #[test]
    fn a_file_exactly_at_the_cell_limit_is_read() {
        // The header and three rows, four columns each: 16 cells.
        let file = "a,b,c,d\n1\n2\n3\n";
        let limits = |max_cells| ReadOptions {
            max_rows: 10,
            max_cells,
            ..ReadOptions::default()
        };
        let table = read_table(file.as_bytes(), "x.csv", &limits(16)).unwrap();
        assert_eq!(table.rows.len(), 3);
        assert_eq!(table.rows[2].cells.len(), 4);
        let result = read_table(file.as_bytes(), "x.csv", &limits(15));
        assert!(refused_for(&result, "cells"), "{}", outcome(&result));
    }

    #[test]
    fn a_header_wider_than_the_column_limit_is_refused() {
        let limits = ReadOptions {
            max_rows: 10,
            max_columns: 10,
            ..ReadOptions::default()
        };
        let ten = wide_and_short(10, 1);
        let table = read_table(ten.as_bytes(), "x.csv", &limits).unwrap();
        assert_eq!(table.headers.len(), 10);
        let eleven = wide_and_short(11, 1);
        let result = read_table(eleven.as_bytes(), "x.csv", &limits);
        assert!(refused_for(&result, "columns"), "{}", outcome(&result));
        // Empty header cells after the last name are not columns.
        let trailing = format!("{}{}\n1\n", "a,b", ",".repeat(50));
        let table = read_table(trailing.as_bytes(), "x.csv", &limits).unwrap();
        assert_eq!(table.headers, ["a", "b"]);
    }

    #[test]
    fn a_data_row_wider_than_the_header_is_cut_whatever_its_width() {
        let wide = format!("1,2,3{}\n", ",x".repeat(5_000));
        let file = format!("a,b\n{wide}");
        let table = read_table(file.as_bytes(), "x.csv", &options()).unwrap();
        assert_eq!(table.rows.len(), 1);
        assert_eq!(text(&table.rows[0].cells), ["1", "2"]);
    }

    #[test]
    fn a_workbook_counts_against_the_column_and_cell_limits() {
        let headers: Vec<String> = (0..20).map(|i| format!("h{i}")).collect();
        let rows: Vec<Vec<Cell>> = (0..20).map(|_| (0..20).map(Cell::Int).collect()).collect();
        let file = crate::write_xlsx("Wide", &[], &headers, &rows).unwrap();
        let columns = read_table(
            &file,
            "wide.xlsx",
            &ReadOptions {
                max_rows: 100,
                max_columns: 10,
                ..ReadOptions::default()
            },
        );
        assert!(refused_for(&columns, "columns"), "{}", outcome(&columns));
        let cells = read_table(
            &file,
            "wide.xlsx",
            &ReadOptions {
                max_rows: 100,
                max_cells: 100,
                ..ReadOptions::default()
            },
        );
        assert!(refused_for(&cells, "cells"), "{}", outcome(&cells));
        let table = read_table(
            &file,
            "wide.xlsx",
            &ReadOptions {
                max_rows: 100,
                max_cells: 420,
                ..ReadOptions::default()
            },
        )
        .unwrap();
        assert_eq!(table.rows.len(), 20);
    }

    /// Hands out one line per `read`, counting the reads.
    struct Lines {
        next: std::vec::IntoIter<String>,
        reads: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Read for Lines {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let Some(line) = self.next.next() else {
                return Ok(0);
            };
            self.reads
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            buf[..line.len()].copy_from_slice(line.as_bytes());
            Ok(line.len())
        }
    }

    #[test]
    fn the_row_limit_stops_the_reading() {
        let reads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let lines: Vec<String> = std::iter::once("a\n".to_owned())
            .chain((0..1_000).map(|i| format!("{i}\n")))
            .collect();
        let source = Lines {
            next: lines.into_iter(),
            reads: reads.clone(),
        };
        let result = read_records(
            source,
            b',',
            &ReadOptions {
                max_rows: 10,
                ..ReadOptions::default()
            },
        );
        assert!(
            matches!(result, Err(SheetsError::TooManyRows { limit: 10 })),
            "{}",
            outcome(&result)
        );
        let reads = reads.load(std::sync::atomic::Ordering::Relaxed);
        assert!(reads < 100, "{reads} of 1001 lines were read");
    }

    /// The naming `headers_of` had when it searched the whole list for
    /// every suffix.
    fn searched_headers_of(cells: &[Cell]) -> Vec<String> {
        let last = cells
            .iter()
            .rposition(|cell| !cell.is_empty())
            .map_or(0, |i| i + 1);
        let mut headers: Vec<String> = Vec::with_capacity(last);
        for (index, cell) in cells[..last].iter().enumerate() {
            let text = cell.to_text().trim().to_owned();
            let base = if text.is_empty() {
                format!("Column {}", index + 1)
            } else {
                text
            };
            let mut name = base.clone();
            let mut n = 2;
            while headers.contains(&name) {
                name = format!("{base} ({n})");
                n += 1;
            }
            headers.push(name);
        }
        headers
    }

    fn texts(names: &[&str]) -> Vec<Cell> {
        names
            .iter()
            .map(|name| Cell::Text((*name).into()))
            .collect()
    }

    #[test]
    fn a_header_named_like_a_suffixed_repeat_is_kept_apart() {
        assert_eq!(
            headers_of(&texts(&["a", "a (2)", "a"])),
            ["a", "a (2)", "a (3)"]
        );
        assert_eq!(
            headers_of(&texts(&["a", "a", "a (2)"])),
            ["a", "a (2)", "a (2) (2)"]
        );
        assert_eq!(
            headers_of(&texts(&["a (3)", "a", "a", "a"])),
            ["a (3)", "a", "a (2)", "a (4)"]
        );
        assert_eq!(
            headers_of(&texts(&["Column 2", "", "", "Column 3", "Column 2"])),
            [
                "Column 2",
                "Column 2 (2)",
                "Column 3",
                "Column 3 (2)",
                "Column 2 (3)"
            ]
        );
    }

    #[test]
    fn repeated_headers_are_named_as_they_always_were() {
        let pieces = [
            "a",
            "a",
            "a (2)",
            "a (3)",
            "b",
            "",
            " a ",
            "Column 2",
            "Column 3",
            "a (2) (2)",
            "b (2)",
        ];
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..3_000 {
            let length = (next() % 40) as usize;
            let cells: Vec<Cell> = (0..length)
                .map(|_| Cell::Text(pieces[(next() % pieces.len() as u64) as usize].into()))
                .collect();
            assert_eq!(
                headers_of(&cells),
                searched_headers_of(&cells),
                "{:?}",
                text(&cells)
            );
        }
    }

    #[test]
    fn a_header_repeated_thousands_of_times_is_named_quickly() {
        // Searching the whole list for every suffix takes about n^3 / 6
        // comparisons: 5.6e8 for 1,500 names, seconds. Naming each header
        // once is linear: milliseconds.
        let n = 1_500;
        let mut file = vec!["a"; n].join(",");
        file.push_str("\n1\n");
        let started = std::time::Instant::now();
        let table = read_table(file.as_bytes(), "x.csv", &options()).unwrap();
        let elapsed = started.elapsed();
        assert_eq!(table.headers.len(), n);
        assert_eq!(table.headers[0], "a");
        assert_eq!(table.headers[1], "a (2)");
        assert_eq!(table.headers[n - 1], format!("a ({n})"));
        assert!(
            elapsed < std::time::Duration::from_millis(250),
            "naming {n} repeated headers took {elapsed:?}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reading_leaves_the_async_thread_free() {
        // On a one-thread runtime, a task spawned before the reading only
        // runs if the reading gave the thread up.
        let other = tokio::spawn(async {});
        let file = Bytes::from_static(b"a,b\n1,2\n");
        let table = read_table_blocking(file, "x.csv".to_owned(), &options())
            .await
            .unwrap();
        assert_eq!(table.rows.len(), 1);
        assert!(other.is_finished(), "the reading ran on the async thread");
    }
}
