//! Reading an uploaded file into a [`Table`]: the header row and the data
//! rows, from CSV, TSV, XLSX, XLSB, XLS or ODS.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Cursor, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::Bytes;
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};

use super::decode::decode;
use super::workbook::{CellSource, Limits, Value, Workbook, interrupted};
use crate::{Cell, ImportOptions, SheetsError};

/// The kind of an uploaded file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
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
    /// The most a part of a zipped workbook may unpack to.
    pub(crate) max_inflated_bytes: u64,
    /// The most text the cells the table keeps may hold, in bytes.
    pub(crate) max_text_bytes: u64,
    /// Raised when nobody waits for the reading any more; the reading then
    /// stops: a workbook's sheet within a few hundred microseconds of
    /// reading, and a text file at its next record. Opening a workbook is not
    /// interrupted.
    pub(crate) stop: Option<&'a AtomicBool>,
}

impl ReadOptions<'_> {
    /// An error once the reading has been told to stop.
    fn ensure_running(&self) -> Result<(), SheetsError> {
        if self.stop.is_some_and(|stop| stop.load(Ordering::Relaxed)) {
            return Err(interrupted());
        }
        Ok(())
    }
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
            max_inflated_bytes: limits.max_inflated_bytes,
            max_text_bytes: limits.max_text_bytes,
            stop: None,
        }
    }
}

const ZIP: &[u8] = b"PK\x03\x04";
pub(crate) const OLE: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";

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
    options.ensure_running()?;
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
    let (max_rows, max_columns, max_cells, max_inflated_bytes, max_text_bytes) = (
        options.max_rows,
        options.max_columns,
        options.max_cells,
        options.max_inflated_bytes,
        options.max_text_bytes,
    );
    let read = blocking(move |stop| {
        read_table(
            &bytes,
            &filename,
            &ReadOptions {
                encoding: encoding.as_deref(),
                sheet: sheet.as_deref(),
                max_rows,
                max_columns,
                max_cells,
                max_inflated_bytes,
                max_text_bytes,
                stop: Some(stop),
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

/// Runs `job` on Tokio's blocking pool and waits for it. `job` is given a
/// flag that is raised when nobody waits for it any more (the future is
/// dropped, as a timeout does), so that it can stop instead of carrying on
/// with a file and a table nobody will use.
async fn blocking<T: Send + 'static>(
    job: impl FnOnce(&AtomicBool) -> T + Send + 'static,
) -> Result<T, tokio::task::JoinError> {
    let stop = Arc::new(AtomicBool::new(false));
    let _raised_on_drop = Raise(stop.clone());
    tokio::task::spawn_blocking(move || job(&stop)).await
}

/// Raises its flag when dropped.
struct Raise(Arc<AtomicBool>);

impl Drop for Raise {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
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
        options.ensure_running()?;
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
    // Whatever the file starts with: a zip workbook may come after a
    // prefix, and a file the XLS reader refuses is read as a zip workbook.
    check_unpacked_sizes(bytes, options.max_inflated_bytes)?;
    let workbook = Workbook::open(
        bytes,
        Limits {
            max_part_bytes: options.max_inflated_bytes,
            max_text_bytes: options.max_text_bytes,
        },
    )?;
    let sheet = sheet_to_read(workbook.sheet_names(), options)?;
    let mut table = tabulate_cells(&workbook, sheet, options)?.finish()?;
    table.kind = workbook.kind();
    Ok(table)
}

/// Where the sheet asked for is among `names`: the first sheet when none
/// is asked for.
fn sheet_to_read(names: &[String], options: &ReadOptions<'_>) -> Result<usize, SheetsError> {
    match options.sheet.map(str::trim).filter(|s| !s.is_empty()) {
        Some(name) => names.iter().position(|n| n == name).ok_or_else(|| {
            SheetsError::UnsupportedFile(format!("The workbook has no sheet named `{name}`."))
        }),
        None if names.is_empty() => Err(SheetsError::UnsupportedFile(
            "The workbook has no sheets.".into(),
        )),
        None => Ok(0),
    }
}

/// Refuses a zipped workbook with a part that unpacks to more than `limit`
/// bytes, by the size the zip file declares for it, before any part is read:
/// the shared strings are held for the whole reading and the text of a cell
/// is read whole, and a small file can unpack to a thousand times its size.
/// The zip reader stops a part at its declared size, so a file that
/// declares less than a part holds cannot be read, and one that declares
/// more is refused here.
fn check_unpacked_sizes(bytes: &[u8], limit: u64) -> Result<(), SheetsError> {
    // A file that is not a zip archive is for the readers to say so.
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return Ok(());
    };
    for index in 0..archive.len() {
        let Ok(part) = archive.by_index_raw(index) else {
            continue;
        };
        if part.size() > limit {
            return Err(SheetsError::UnsupportedFile(format!(
                "The workbook has a part that unpacks to more than {limit} bytes."
            )));
        }
    }
    Ok(())
}

/// The cells of a worksheet row, given with their columns in increasing
/// order, as the row they stand for: `used` cells, empty where none was
/// given. Produced as it is taken, so a row of a million columns costs
/// what is taken from it.
fn dense(
    cells: impl IntoIterator<Item = (usize, Cell)>,
    used: usize,
) -> impl Iterator<Item = Cell> {
    let mut cells = cells.into_iter().peekable();
    (0..used).map(move |at| match cells.next_if(|(column, _)| *column == at) {
        Some((_, cell)) => cell,
        None => Cell::Empty,
    })
}

/// Whether a sheet's cells came in the order a spreadsheet writes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    /// By row, and by column within a row, each position once.
    Kept,
    /// Some cell came at or before the one before it.
    Broken,
}

/// A workbook's sheet as a table, read cell by cell.
///
/// A worksheet lists its cells by row and, within a row, by column, so the
/// rows are taken one at a time and the limits apply to each as it is
/// read; no cell is kept past its row. A sheet that lists cells in another
/// order (Excel never writes one) is read whole, as sparse cells, and
/// tabulated from there: the same table either way.
fn tabulate_cells(
    workbook: &Workbook<'_>,
    sheet: usize,
    options: &ReadOptions<'_>,
) -> Result<Tabulator, SheetsError> {
    let mut tabulator = Tabulator::new(options);
    let order = workbook.read_cells(sheet, options.stop, |cells| {
        stream_rows(cells, &mut tabulator, options)
    })?;
    if order == Order::Kept {
        return Ok(tabulator);
    }
    // The rows of the first reading are dropped, not held while the sheet
    // is read again.
    tabulator = Tabulator::new(options);
    workbook.read_cells(sheet, options.stop, |cells| {
        tabulate_unordered(cells, &mut tabulator, options)
    })?;
    Ok(tabulator)
}

/// One row of a worksheet, while its cells come: the ones with a value that
/// the table could keep, and how far the row reaches.
struct OpenRow {
    row: u32,
    /// One past the column of the last cell with a value.
    used: usize,
    cells: Vec<(usize, Cell)>,
}

fn stream_rows(
    cells: Option<&mut dyn CellSource>,
    tabulator: &mut Tabulator,
    options: &ReadOptions<'_>,
) -> Result<Order, SheetsError> {
    let Some(source) = cells else {
        return Ok(Order::Kept);
    };
    let mut last = None;
    let mut open: Option<OpenRow> = None;
    // The first limit the rows break. The cells after it are still read,
    // keeping none, because a cell out of order would change the answer:
    // the rows would not be the ones read so far.
    let mut refused = None;
    loop {
        // Of the cells past the header's width, a source need only give
        // the last of a row.
        source.cut_at(tabulator.width());
        let Some(cell) = source.next_cell()? else {
            break;
        };
        options.ensure_running()?;
        if last.is_some_and(|last| (cell.row, cell.column) <= last) {
            return Ok(Order::Broken);
        }
        last = Some(cell.last());
        // Text is measured where it is, and counted, before it is copied:
        // a shared string is one text for any number of cells.
        if cell.value == Value::Text("") {
            continue;
        }
        if open.as_ref().is_some_and(|open| open.row != cell.row) {
            refused = open
                .take()
                .and_then(|finished| push_row(tabulator, finished).err());
            if refused.is_some() {
                break;
            }
        }
        let open = open.get_or_insert_with(|| OpenRow {
            row: cell.row,
            used: 0,
            cells: Vec::new(),
        });
        let column = cell.column as usize;
        let end = column.saturating_add(cell.columns as usize);
        open.used = end;
        // Cells past the header's width are cut, so they are not kept.
        for column in column..end.min(tabulator.width()) {
            if let Err(error) = tabulator.charge(kept_bytes(&cell.value)) {
                refused = Some(error);
                break;
            }
            open.cells.push((column, cell_of(&cell.value)));
        }
        if refused.is_some() {
            break;
        }
    }
    if let Some(error) = refused {
        return match source.rest_in_order(last, &|| options.ensure_running())? {
            true => Err(error),
            false => Ok(Order::Broken),
        };
    }
    if let Some(finished) = open.take() {
        push_row(tabulator, finished)?;
    }
    Ok(Order::Kept)
}

fn push_row(tabulator: &mut Tabulator, row: OpenRow) -> Result<(), SheetsError> {
    tabulator.push_counted(u64::from(row.row) + 1, row.used, dense(row.cells, row.used))
}

/// The bytes of text a cell value is kept as: its text, trimmed already.
fn kept_bytes(value: &Value<'_>) -> usize {
    match value {
        Value::Text(text) => text.len(),
        Value::Typed(_) => 0,
    }
}

/// The cell a value is kept as.
fn cell_of(value: &Value<'_>) -> Cell {
    match value {
        Value::Text("") => Cell::Empty,
        Value::Text(text) => Cell::Text((*text).to_owned()),
        Value::Typed(cell) => cell.clone(),
    }
}

/// The rows of a sheet whose cells are not in order, by row and column.
///
/// The cells with a value are kept, by position, until the sheet ends; a
/// position given twice keeps its last cell. What is kept is bounded by the
/// limits: a cell in a column past `max_columns` is kept as a mark that the
/// row has something there (the table cuts every row at its header, which
/// is no wider than `max_columns`), and the sheet is refused as soon as it
/// holds more than `max_rows + 1` rows (the header and the data rows) or
/// more than `max_cells` cells. The rest of the sheet is then still read,
/// keeping nothing, so that a sheet that cannot be read is reported as
/// that.
fn tabulate_unordered(
    cells: Option<&mut dyn CellSource>,
    tabulator: &mut Tabulator,
    options: &ReadOptions<'_>,
) -> Result<(), SheetsError> {
    let Some(source) = cells else {
        return Ok(());
    };
    let mut sheet: BTreeMap<u32, BTreeMap<u32, Cell>> = BTreeMap::new();
    let mut held = 0u64;
    let mut refused = None;
    while let Some(cell) = source.next_cell()? {
        options.ensure_running()?;
        if refused.is_some() {
            continue;
        }
        let row = cell.row;
        // A cell that covers columns past `max_columns` is kept up to there,
        // and once more as a mark after.
        let start = u64::from(cell.column);
        let end = start.saturating_add(u64::from(cell.columns));
        let kept_end = end.min(options.max_columns).max(start);
        let marks = u64::from(end > kept_end);
        for column in start..kept_end.saturating_add(marks) {
            let column = u32::try_from(column).unwrap_or(u32::MAX);
            let kept = if cell.value == Value::Text("") {
                Cell::Empty
            } else if u64::from(column) >= options.max_columns {
                Cell::Bool(true)
            } else if let Err(error) = tabulator.charge(kept_bytes(&cell.value)) {
                refused = Some(error);
                break;
            } else {
                cell_of(&cell.value)
            };
            match sheet.get_mut(&row).and_then(|cells| cells.get_mut(&column)) {
                Some(slot) => *slot = kept,
                // Nothing and an empty cell are the same.
                None if kept.is_empty() => {}
                None => {
                    sheet.entry(row).or_default().insert(column, kept);
                    held += 1;
                }
            }
            refused = if held > options.max_cells {
                Some(too_many_cells(options.max_cells))
            } else if sheet.len() as u64 > options.max_rows.saturating_add(1) {
                Some(SheetsError::TooManyRows {
                    limit: options.max_rows,
                })
            } else {
                None
            };
            if refused.is_some() {
                break;
            }
        }
        if refused.is_some() {
            sheet = BTreeMap::new();
        }
    }
    if let Some(error) = refused {
        return Err(error);
    }
    for (row, cells) in sheet {
        let used = cells
            .iter()
            .rev()
            .find(|(_, cell)| !cell.is_empty())
            .map_or(0, |(column, _)| *column as usize + 1);
        let cells = cells
            .into_iter()
            .map(|(column, cell)| (column as usize, cell));
        tabulator.push_counted(u64::from(row) + 1, used, dense(cells, used))?;
    }
    Ok(())
}

/// Builds a [`Table`] from records as they are read, so that no limit is
/// checked after the rows are in memory. The header row is the first row
/// with a value and the data rows follow it; empty rows are dropped, short
/// rows padded, cells past the last header dropped.
struct Tabulator {
    max_rows: u64,
    max_columns: u64,
    max_cells: u64,
    max_text_bytes: u64,
    /// The text of the cells taken so far.
    text_bytes: u64,
    headers: Option<Vec<String>>,
    rows: Vec<TableRow>,
}

impl Tabulator {
    fn new(options: &ReadOptions<'_>) -> Self {
        Tabulator {
            max_rows: options.max_rows,
            max_columns: options.max_columns,
            max_cells: options.max_cells,
            max_text_bytes: options.max_text_bytes,
            text_bytes: 0,
            headers: None,
            rows: Vec::new(),
        }
    }

    /// How many cells of a row the table keeps: the header's width, and
    /// before there is a header the most columns it may have.
    fn width(&self) -> usize {
        let width = self
            .headers
            .as_ref()
            .map_or(self.max_columns, |headers| headers.len() as u64);
        usize::try_from(width).unwrap_or(usize::MAX)
    }

    /// Counts `bytes` of text that the table is about to keep, and refuses
    /// the file when that makes more than the limit.
    fn charge(&mut self, bytes: usize) -> Result<(), SheetsError> {
        self.text_bytes = self.text_bytes.saturating_add(bytes as u64);
        if self.text_bytes > self.max_text_bytes {
            return Err(SheetsError::UnsupportedFile(format!(
                "The file holds more than {} bytes of text.",
                self.max_text_bytes
            )));
        }
        Ok(())
    }

    /// Adds the record on row `number`. `used` is how many of its cells
    /// come before the last one with a value (0 for an empty row), and
    /// `cells` yields them all; only the ones the table keeps are taken, and
    /// their text is counted.
    fn push(
        &mut self,
        number: u64,
        used: usize,
        cells: impl IntoIterator<Item = Cell>,
    ) -> Result<(), SheetsError> {
        self.push_cells(number, used, cells, true)
    }

    /// [`Tabulator::push`] for cells whose text was counted before they were
    /// made.
    fn push_counted(
        &mut self,
        number: u64,
        used: usize,
        cells: impl IntoIterator<Item = Cell>,
    ) -> Result<(), SheetsError> {
        self.push_cells(number, used, cells, false)
    }

    fn push_cells(
        &mut self,
        number: u64,
        used: usize,
        cells: impl IntoIterator<Item = Cell>,
        count_text: bool,
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
            if count_text {
                for cell in &header_cells {
                    if let Cell::Text(text) = cell {
                        self.charge(text.len())?;
                    }
                }
            }
            self.headers = Some(headers_of(header_cells));
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
            return Err(too_many_cells(self.max_cells));
        }
        let mut row = Vec::with_capacity(width);
        for cell in cells.take(width) {
            if let (true, Cell::Text(text)) = (count_text, &cell) {
                self.charge(text.len())?;
            }
            row.push(cell);
        }
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

fn too_many_cells(limit: u64) -> SheetsError {
    SheetsError::UnsupportedFile(format!(
        "The file has more than {limit} cells (rows times columns)."
    ))
}

/// Header texts: trailing empty headers dropped, blank ones named
/// `Column N`, repeats suffixed ` (2)`, ` (3)`. The cells' text is moved into
/// the names, not copied, so the header takes the room of its names and of
/// the set that keeps them apart, and not that of its cells besides.
fn headers_of(mut cells: Vec<Cell>) -> Vec<String> {
    let last = cells
        .iter()
        .rposition(|cell| !cell.is_empty())
        .map_or(0, |i| i + 1);
    cells.truncate(last);
    let mut headers: Vec<String> = Vec::with_capacity(last);
    let mut taken: HashSet<String> = HashSet::with_capacity(last);
    // The suffix each repeated text tries next: every one below it is taken
    // already (names are only ever added), so a repeat never searches again.
    let mut next_suffix: HashMap<String, u64> = HashMap::new();
    for (index, cell) in cells.into_iter().enumerate() {
        let text = match cell {
            Cell::Text(text) => text,
            other => other.to_text().into_owned(),
        };
        let text = if text.trim().len() == text.len() {
            text
        } else {
            text.trim().to_owned()
        };
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
            b"\n Name , Email,\nAnu, anu@example.com\n,,\nArun,arun@example.com,extra\nGauri\n",
            "people.csv",
            &options(),
        )
        .unwrap();
        assert_eq!(table.kind, FileKind::Csv);
        assert_eq!(table.encoding, Some("UTF-8"));
        assert_eq!(table.headers, ["Name", "Email"]);
        let numbers: Vec<u64> = table.rows.iter().map(|r| r.number).collect();
        assert_eq!(numbers, [3, 5, 6]);
        assert_eq!(text(&table.rows[0].cells), ["Anu", "anu@example.com"]);
        assert_eq!(text(&table.rows[1].cells), ["Arun", "arun@example.com"]);
        assert_eq!(
            table.rows[2].cells,
            [Cell::Text("Gauri".into()), Cell::Empty]
        );
    }

    #[test]
    fn a_cell_spanning_lines_is_one_row() {
        let table = read_table(
            b"Name,Note\nAnu,\"first\nsecond\"\n\nArun,x\n",
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
                Cell::Text("Anu".into()),
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
                Cell::Text("Anu".into()),
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

    /// Hands out one line per `read`, and raises `flag` at the `at`th.
    struct Interrupting {
        next: std::vec::IntoIter<String>,
        reads: usize,
        at: usize,
        flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl Read for Interrupting {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let Some(line) = self.next.next() else {
                return Ok(0);
            };
            self.reads += 1;
            if self.reads == self.at {
                self.flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            buf[..line.len()].copy_from_slice(line.as_bytes());
            Ok(line.len())
        }
    }

    #[test]
    fn a_reading_nobody_waits_for_stops_at_the_next_record() {
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let lines: Vec<String> = std::iter::once(
            "a
"
            .to_owned(),
        )
        .chain((0..1_000).map(|i| {
            format!(
                "{i}
"
            )
        }))
        .collect();
        let mut source = Interrupting {
            next: lines.into_iter(),
            reads: 0,
            at: 10,
            flag: flag.clone(),
        };
        let result = read_records(
            &mut source,
            b',',
            &ReadOptions {
                max_rows: 10_000,
                stop: Some(&flag),
                ..ReadOptions::default()
            },
        );
        assert!(
            matches!(result, Err(SheetsError::Io(ref e)) if e.kind() == std::io::ErrorKind::Interrupted),
            "{}",
            outcome(&result)
        );
        // The csv reader reads ahead a little, not through the file.
        assert!(
            source.reads < 100,
            "{} of 1001 lines were read",
            source.reads
        );
    }

    #[test]
    fn a_reading_nobody_waits_for_does_not_start_a_file() {
        let flag = std::sync::atomic::AtomicBool::new(true);
        let workbook = crate::write_xlsx("S", &[], &["a".into()], &[vec![Cell::Int(1)]]).unwrap();
        for (file, name) in [
            (
                &b"a
1
"[..],
                "x.csv",
            ),
            (&workbook[..], "x.xlsx"),
        ] {
            let result = read_table(
                file,
                name,
                &ReadOptions {
                    stop: Some(&flag),
                    ..options()
                },
            );
            assert!(
                matches!(result, Err(SheetsError::Io(ref e)) if e.kind() == std::io::ErrorKind::Interrupted),
                "{name}: {}",
                outcome(&result)
            );
        }
        flag.store(false, std::sync::atomic::Ordering::Relaxed);
        let result = read_table(
            b"a
1
",
            "x.csv",
            &ReadOptions {
                stop: Some(&flag),
                ..options()
            },
        );
        assert_eq!(result.unwrap().rows.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_blocking_job_is_told_to_stop_when_nobody_waits_for_it() {
        use futures_util::FutureExt;
        use std::sync::atomic::Ordering;

        let (stopped, seen) = std::sync::mpsc::channel();
        let job = move |stop: &std::sync::atomic::AtomicBool| {
            // Runs until it is told to stop; gives up after a while so that a
            // failure ends.
            let give_up = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while !stop.load(Ordering::Relaxed) && std::time::Instant::now() < give_up {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            stopped.send(stop.load(Ordering::Relaxed)).unwrap();
        };
        // Polled once, so the job is on the blocking pool, and dropped, as a
        // timeout drops it.
        assert!(blocking(job).now_or_never().is_none());
        let was_told = seen
            .recv_timeout(std::time::Duration::from_secs(60))
            .unwrap();
        assert!(was_told, "the job ran until it gave up");
    }

    #[tokio::test]
    async fn a_blocking_job_that_is_waited_for_returns_what_it_returns() {
        assert_eq!(blocking(|_| 7).await.unwrap(), 7);
        let panicked = blocking(|_| -> u8 { panic!("a parser panics") }).await;
        assert!(panicked.unwrap_err().is_panic());
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
            headers_of(texts(&["a", "a (2)", "a"])),
            ["a", "a (2)", "a (3)"]
        );
        assert_eq!(
            headers_of(texts(&["a", "a", "a (2)"])),
            ["a", "a (2)", "a (2) (2)"]
        );
        assert_eq!(
            headers_of(texts(&["a (3)", "a", "a", "a"])),
            ["a (3)", "a", "a (2)", "a (4)"]
        );
        assert_eq!(
            headers_of(texts(&["Column 2", "", "", "Column 3", "Column 2"])),
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
                headers_of(cells.clone()),
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
        // runs if the reading gave the thread up. The file is long enough
        // that the reading is still going when the runtime first asks for it
        // (a two-line file can be read before that, and then nothing waits).
        let other = tokio::spawn(async {});
        let file = Bytes::from(format!("a,b\n{}", "1,2\n".repeat(20_000)));
        let many = ReadOptions {
            max_rows: 50_000,
            ..options()
        };
        let table = read_table_blocking(file, "x.csv".to_owned(), &many)
            .await
            .unwrap();
        assert_eq!(table.rows.len(), 20_000);
        assert!(other.is_finished(), "the reading ran on the async thread");
    }
}

#[cfg(test)]
mod amplification_tests;
#[cfg(test)]
mod ods_text_tests;
#[cfg(test)]
mod opening_tests;
#[cfg(test)]
mod stop_tests;
#[cfg(test)]
mod values_tests;
#[cfg(test)]
mod workbook_tests;
