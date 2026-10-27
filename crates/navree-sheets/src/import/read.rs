//! Reading an uploaded file into a [`Table`]: the header row and the data
//! rows, from CSV, TSV, XLSX, XLS or ODS.

use std::io::Cursor;

use calamine::{Data, Reader, open_workbook_auto_from_rs};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::decode::decode;
use crate::{Cell, SheetsError};

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
#[derive(Clone, Debug, Default)]
pub(crate) struct ReadOptions<'a> {
    /// The text encoding (`windows-1256`); detected when `None`.
    pub(crate) encoding: Option<&'a str>,
    /// The workbook sheet to read; the first when `None`.
    pub(crate) sheet: Option<&'a str>,
    /// The most data rows allowed.
    pub(crate) max_rows: u64,
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

/// Reads `bytes` (a file named `filename`) into a table.
pub(crate) fn read_table(
    bytes: &[u8],
    filename: &str,
    options: &ReadOptions<'_>,
) -> Result<Table, SheetsError> {
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

fn read_text(bytes: &[u8], tab: bool, options: &ReadOptions<'_>) -> Result<Table, SheetsError> {
    let decoded = decode(bytes, options.encoding)?;
    if decoded.text.contains('\0') {
        return Err(SheetsError::UnsupportedFile(
            "The file is not a text file (it contains NUL bytes).".into(),
        ));
    }
    let delimiter = if tab { b'\t' } else { sniff_delimiter(&decoded.text) };
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(decoded.text.as_bytes());
    let mut records = Vec::new();
    // A spreadsheet shows one row per line, blank lines included (the csv
    // crate skips them), except that a quoted cell spanning lines is one
    // row: the record's line, less the line breaks inside earlier cells.
    let mut embedded = 0u64;
    let mut previous = 0u64;
    for record in reader.records() {
        let record = record.map_err(|error| {
            SheetsError::UnsupportedFile(format!("The CSV file cannot be read: {error}"))
        })?;
        let line = record
            .position()
            .map_or(previous + 1, |position| position.line());
        let number = line.saturating_sub(embedded).max(previous + 1);
        previous = number;
        embedded += record
            .iter()
            .map(|field| field.matches('
').count() as u64)
            .sum::<u64>();
        records.push((
            number,
            record.iter().map(text_cell).collect::<Vec<_>>(),
        ));
    }
    let mut table = tabulate(records, options.max_rows)?;
    table.kind = FileKind::Csv;
    table.encoding = Some(decoded.encoding);
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
        .fold((b',', 0), |best, &(d, n)| if n > best.1 { (d, n) } else { best })
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
    let records = range
        .rows()
        .enumerate()
        .map(|(index, row)| {
            // Cells left of the range's first column are empty.
            let mut cells = vec![Cell::Empty; first_col];
            cells.extend(row.iter().map(data_cell));
            (first_row + index as u64 + 1, cells)
        })
        .collect();
    let mut table = tabulate(records, options.max_rows)?;
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

/// The header row (the first row with a value) and the data rows after it;
/// empty rows are dropped, short rows padded, cells past the last header
/// dropped.
fn tabulate(records: Vec<(u64, Vec<Cell>)>, max_rows: u64) -> Result<Table, SheetsError> {
    let mut records = records
        .into_iter()
        .filter(|(_, cells)| cells.iter().any(|cell| !cell.is_empty()));
    let Some((_, header_cells)) = records.next() else {
        return Err(SheetsError::UnsupportedFile(
            "The file has no header row.".into(),
        ));
    };
    let headers = headers_of(&header_cells);
    let width = headers.len();
    let mut rows = Vec::new();
    for (number, mut cells) in records {
        if rows.len() as u64 >= max_rows {
            return Err(SheetsError::TooManyRows { limit: max_rows });
        }
        cells.resize(width, Cell::Empty);
        rows.push(TableRow { number, cells });
    }
    Ok(Table {
        kind: FileKind::Csv,
        encoding: None,
        headers,
        rows,
    })
}

/// Header texts: trailing empty headers dropped, blank ones named
/// `Column N`, repeats suffixed ` (2)`, ` (3)`.
fn headers_of(cells: &[Cell]) -> Vec<String> {
    let last = cells.iter().rposition(|cell| !cell.is_empty()).map_or(0, |i| i + 1);
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
        assert_eq!(table.rows[2].cells, [Cell::Text("Grace".into()), Cell::Empty]);
    }

    #[test]
    fn a_cell_spanning_lines_is_one_row() {
        let table = read_table(
            b"Name,Note
Ada,\"first
second\"

Alan,x
",
            "people.csv",
            &options(),
        )
        .unwrap();
        let numbers: Vec<u64> = table.rows.iter().map(|r| r.number).collect();
        assert_eq!(numbers, [2, 4]);
        assert_eq!(table.rows[0].cells[1], Cell::Text("first
second".into()));
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
        assert_eq!(table.headers, ["Email", "Column 2", "Email (2)", "Email (3)"]);
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
            &["Name".into(), "Age".into(), "Joined".into(), "Active".into()],
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
}
