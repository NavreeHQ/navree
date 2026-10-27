//! XLSX writing on rust_xlsxwriter: a bold, frozen and filtered header
//! row, typed cells, date formats and fitted column widths.

use rust_xlsxwriter::{Format, FormatBorder, Workbook, XlsxError};

use crate::cell::MAX_SAFE_INTEGER;
use crate::{Cell, Column, SheetsError};

/// The most data rows a sheet holds (Excel's 1,048,576 rows, less the
/// header row).
pub const XLSX_MAX_ROWS: u64 = 1_048_575;

/// The longest text a cell holds; longer text is cut.
const MAX_TEXT_CHARS: usize = 32_767;

/// The widest a fitted column gets, in characters.
const MAX_FITTED_WIDTH: f64 = 60.0;

/// The date format of date cells without a column format.
const DATE_FORMAT: &str = "yyyy-mm-dd";

/// The date-time format of date-time cells without a column format.
const DATETIME_FORMAT: &str = "yyyy-mm-dd hh:mm:ss";

impl From<XlsxError> for SheetsError {
    fn from(error: XlsxError) -> Self {
        SheetsError::Xlsx(error.to_string())
    }
}

/// A sheet name Excel accepts: without `[ ] : * ? / \`, at most 31
/// characters, not empty.
pub(crate) fn sheet_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\') && !c.is_control())
        .take(31)
        .collect();
    let cleaned = cleaned.trim().trim_matches('\'').trim().to_owned();
    if cleaned.is_empty() {
        "Sheet1".to_owned()
    } else {
        cleaned
    }
}

/// Column formats, made once.
struct ColumnFormats {
    number: Option<Format>,
    date: Format,
    datetime: Format,
}

/// Builds one XLSX sheet row by row. With `constant_memory`, rows are
/// written to a temporary file as they come, so large exports stay small
/// in memory.
pub(crate) struct XlsxBuilder {
    workbook: Workbook,
    formats: Vec<ColumnFormats>,
    widths: Vec<f64>,
    fixed: Vec<bool>,
    columns: u16,
    row: u32,
    /// Widths are set from the header and the first rows, once.
    sized: bool,
}

impl XlsxBuilder {
    pub(crate) fn new(
        sheet: &str,
        columns: &[Column],
        headers: &[String],
        constant_memory: bool,
    ) -> Result<Self, SheetsError> {
        let count = u16::try_from(columns.len().max(headers.len())).map_err(|_| {
            SheetsError::Invalid("an XLSX sheet holds at most 16,384 columns".into())
        })?;
        if count > 16_384 {
            return Err(SheetsError::Invalid(
                "an XLSX sheet holds at most 16,384 columns".into(),
            ));
        }
        let mut workbook = Workbook::new();
        let header_format = Format::new()
            .set_bold()
            .set_border_bottom(FormatBorder::Thin);
        {
            let worksheet = if constant_memory {
                workbook.add_worksheet_with_constant_memory()?
            } else {
                workbook.add_worksheet()
            };
            worksheet.set_name(sheet_name(sheet))?;
            for (index, header) in headers.iter().enumerate() {
                worksheet.write_string_with_format(
                    0,
                    index as u16,
                    truncate(header),
                    &header_format,
                )?;
            }
            worksheet.set_freeze_panes(1, 0)?;
        }
        let formats = (0..usize::from(count))
            .map(|index| {
                let custom = columns.get(index).and_then(|c| c.format.as_deref());
                ColumnFormats {
                    number: custom.map(|f| Format::new().set_num_format(f)),
                    date: Format::new().set_num_format(custom.unwrap_or(DATE_FORMAT)),
                    datetime: Format::new().set_num_format(custom.unwrap_or(DATETIME_FORMAT)),
                }
            })
            .collect();
        let widths = (0..usize::from(count))
            .map(|index| {
                columns
                    .get(index)
                    .and_then(|c| c.width)
                    .unwrap_or_else(|| text_width(headers.get(index).map_or("", String::as_str)))
            })
            .collect();
        let fixed = (0..usize::from(count))
            .map(|index| columns.get(index).and_then(|c| c.width).is_some())
            .collect();
        Ok(XlsxBuilder {
            workbook,
            formats,
            widths,
            fixed,
            columns: count,
            row: 0,
            sized: false,
        })
    }

    /// Appends `rows` below the ones written so far.
    pub(crate) fn append(&mut self, rows: &[Vec<Cell>]) -> Result<(), SheetsError> {
        if u64::from(self.row) + rows.len() as u64 > XLSX_MAX_ROWS {
            return Err(SheetsError::TooManyRows {
                limit: XLSX_MAX_ROWS,
            });
        }
        if !self.sized {
            for row in rows {
                for (index, cell) in row.iter().enumerate().take(self.widths.len()) {
                    if !self.fixed[index] {
                        let width = cell_width(cell);
                        if width > self.widths[index] {
                            self.widths[index] = width;
                        }
                    }
                }
            }
        }
        let worksheet = self.workbook.worksheet_from_index(0)?;
        if !self.sized {
            for (index, width) in self.widths.iter().enumerate() {
                worksheet.set_column_width(index as u16, width.min(MAX_FITTED_WIDTH))?;
            }
            self.sized = true;
        }
        for row in rows {
            self.row += 1;
            let at = self.row;
            for (index, cell) in row.iter().enumerate().take(usize::from(self.columns)) {
                let col = index as u16;
                let formats = &self.formats[index];
                match cell {
                    Cell::Empty => {}
                    Cell::Text(text) => {
                        worksheet.write_string(at, col, truncate(text))?;
                    }
                    Cell::Int(n) if n.abs() > MAX_SAFE_INTEGER => {
                        worksheet.write_string(at, col, n.to_string())?;
                    }
                    Cell::Int(n) => match &formats.number {
                        Some(format) => {
                            worksheet.write_number_with_format(at, col, *n as f64, format)?;
                        }
                        None => {
                            worksheet.write_number(at, col, *n as f64)?;
                        }
                    },
                    Cell::Float(n) if !n.is_finite() => {
                        worksheet.write_string(at, col, n.to_string())?;
                    }
                    Cell::Float(n) => match &formats.number {
                        Some(format) => {
                            worksheet.write_number_with_format(at, col, *n, format)?;
                        }
                        None => {
                            worksheet.write_number(at, col, *n)?;
                        }
                    },
                    Cell::Bool(b) => {
                        worksheet.write_boolean(at, col, *b)?;
                    }
                    Cell::Date(date) => {
                        worksheet.write_datetime_with_format(at, col, date, &formats.date)?;
                    }
                    Cell::DateTime(at_time) => {
                        worksheet.write_datetime_with_format(
                            at,
                            col,
                            at_time,
                            &formats.datetime,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    /// The finished file.
    pub(crate) fn finish(mut self) -> Result<Vec<u8>, SheetsError> {
        if !self.sized {
            self.append(&[])?;
        }
        let last_column = self.columns.saturating_sub(1);
        let rows = self.row;
        let worksheet = self.workbook.worksheet_from_index(0)?;
        if self.columns > 0 {
            worksheet.autofilter(0, 0, rows, last_column)?;
        }
        Ok(self.workbook.save_to_buffer()?)
    }
}

/// `text`, cut to what a cell holds.
fn truncate(text: &str) -> &str {
    match text.char_indices().nth(MAX_TEXT_CHARS) {
        Some((end, _)) => &text[..end],
        None => text,
    }
}

/// The width a cell's text needs, in characters, plus padding.
fn cell_width(cell: &Cell) -> f64 {
    match cell {
        Cell::Date(_) => 12.0,
        Cell::DateTime(_) => 20.0,
        other => text_width(&other.to_text()),
    }
}

fn text_width(text: &str) -> f64 {
    let longest = text
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    (longest as f64 + 2.0).max(8.0)
}

/// A whole XLSX file with one sheet: bold, frozen and filtered `headers`,
/// then `rows`.
///
/// ```
/// use navree_sheets::{Cell, Column, write_xlsx};
///
/// let file = write_xlsx(
///     "Deals",
///     &[Column::new("title", "Title"), Column::new("amount", "Amount").format("#,##0.00")],
///     &["Title".to_owned(), "Amount".to_owned()],
///     &[vec![Cell::Text("Renewal".into()), Cell::Float(1200.5)]],
/// )
/// .unwrap();
/// assert!(file.starts_with(b"PK"));
/// ```
pub fn write_xlsx(
    sheet: &str,
    columns: &[Column],
    headers: &[String],
    rows: &[Vec<Cell>],
) -> Result<Vec<u8>, SheetsError> {
    let mut builder = XlsxBuilder::new(sheet, columns, headers, false)?;
    builder.append(rows)?;
    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_names_are_made_valid() {
        assert_eq!(sheet_name("Q1/Q2 [draft]"), "Q1Q2 draft");
        assert_eq!(sheet_name(""), "Sheet1");
        assert_eq!(sheet_name("'quoted'"), "quoted");
        assert_eq!(sheet_name(&"x".repeat(40)).len(), 31);
    }

    #[test]
    fn long_text_is_cut_to_what_a_cell_holds() {
        let long = "é".repeat(MAX_TEXT_CHARS + 10);
        assert_eq!(truncate(&long).chars().count(), MAX_TEXT_CHARS);
        assert_eq!(truncate("short"), "short");
    }

    #[test]
    fn too_many_rows_is_an_error() {
        let mut builder = XlsxBuilder::new("S", &[], &["A".into()], true).unwrap();
        builder.row = XLSX_MAX_ROWS as u32;
        let error = builder.append(&[vec![Cell::Int(1)]]).unwrap_err();
        assert!(matches!(error, SheetsError::TooManyRows { .. }));
    }
}
