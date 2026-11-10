//! A report's table as an Excel file or a PDF, where Navree UI meets the sheets and the
//! documents engine. navree-ui writes a report's [`ReportTable`] as CSV itself; it cannot
//! write the others, because navree-sheets and navree-documents sit above it, so the facade
//! writes them from the same table.
//!
//! - [`ReportTableSheets`] (the `ui` and `sheets` features): an XLSX file, and its
//!   download.
//! - [`ReportTablePdf`] (the `ui` feature with `documents` or `documents-native`): a PDF,
//!   and its download.

#[cfg(any(
    feature = "sheets",
    feature = "documents",
    feature = "documents-native"
))]
use navree_http::response::Response;
#[cfg(any(
    feature = "sheets",
    feature = "documents",
    feature = "documents-native"
))]
use navree_ui::charts::ReportTable;

/// [`ReportTable`] as an XLSX file: the first column holds the periods as text,
/// each series is a column of numbers (`#,##0.##`), and a gap is an empty cell.
///
/// ```
/// use navree::reports::ReportTableSheets;
/// use navree::ui::charts::{Grain, Report};
/// use navree::ui::dates::DateRange;
///
/// let day = |d| chrono::NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
/// let report = Report::new(DateRange::new(day(2), day(4)), Grain::Day, &chrono::Utc);
/// let values: &[Option<f64>] = &[Some(1_25_000.0), None, Some(80_000.5)];
/// let table = report.table("Day", &[("Collected", values)]);
/// let file = table.xlsx("Collections").unwrap();
/// assert!(file.starts_with(b"PK"));
/// ```
#[cfg(feature = "sheets")]
pub trait ReportTableSheets {
    /// The table as a whole XLSX file with one sheet named `sheet`, its headings
    /// bold, frozen and filtered.
    ///
    /// # Errors
    ///
    /// When the table has more rows than a sheet holds, or the writer fails.
    fn xlsx(&self, sheet: &str) -> Result<Vec<u8>, navree_sheets::SheetsError>;

    /// The workbook as a download named `file`, saved rather than shown, never cached and
    /// never sniffed, as [`ReportTable::csv_response`] answers the CSV.
    ///
    /// # Errors
    ///
    /// As [`xlsx`](Self::xlsx).
    fn xlsx_response(
        &self,
        sheet: &str,
        file: &str,
    ) -> Result<Response, navree_sheets::SheetsError>;
}

#[cfg(feature = "sheets")]
impl ReportTableSheets for ReportTable {
    fn xlsx(&self, sheet: &str) -> Result<Vec<u8>, navree_sheets::SheetsError> {
        use navree_sheets::{Cell, Column, write_xlsx};

        let columns: Vec<Column> = self
            .head()
            .iter()
            .enumerate()
            .map(|(at, name)| {
                let column = Column::new(format!("c{at}"), name.clone());
                if at == 0 {
                    column
                } else {
                    column.format("#,##0.##")
                }
            })
            .collect();
        let rows: Vec<Vec<Cell>> = self
            .periods()
            .iter()
            .enumerate()
            .map(|(row, period)| {
                let mut cells = vec![Cell::Text(period.clone())];
                cells.extend(self.values().iter().map(|series| {
                    series
                        .get(row)
                        .copied()
                        .flatten()
                        .map_or(Cell::Empty, Cell::Float)
                }));
                cells
            })
            .collect();
        write_xlsx(sheet, &columns, self.head(), &rows)
    }

    fn xlsx_response(
        &self,
        sheet: &str,
        file: &str,
    ) -> Result<Response, navree_sheets::SheetsError> {
        Ok(self.file_response(
            self.xlsx(sheet)?,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            file,
        ))
    }
}

/// [`ReportTable`] as a PDF: a title, a heading row and a row for each period, the numbers
/// written as `format` and `locale` write them (`₹1,25,000` under `en-IN` with
/// `NumberFormat::currency("₹")`), a gap left empty, on as many pages as they take with the
/// headings repeated and the pages numbered. A report has at most
/// [`MAX_BUCKETS`](navree_ui::charts::MAX_BUCKETS) periods, so a few pages at most.
///
/// It is laid out by navree-documents' native engine in its bundled Noto fonts, embedded in
/// the file, so the rupee sign and words in Devanagari or Gurmukhi print as they are written.
/// More than six columns turn the pages on their side.
///
/// ```
/// use navree::reports::ReportTablePdf;
/// use navree::ui::charts::{Grain, NumberFormat, NumberLocale, Report};
/// use navree::ui::dates::DateRange;
///
/// let day = |d| chrono::NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
/// let report = Report::new(DateRange::new(day(2), day(4)), Grain::Day, &chrono::Utc);
/// let values: &[Option<f64>] = &[Some(1_25_000.0), None, Some(80_000.5)];
/// let table = report.table("Day", &[("Collected", values)]);
/// let file = table
///     .pdf("Collections", &NumberFormat::currency("₹"), &NumberLocale::of("en-IN"))
///     .unwrap();
/// assert!(file.starts_with(b"%PDF-"));
/// ```
#[cfg(any(feature = "documents", feature = "documents-native"))]
pub trait ReportTablePdf {
    /// The table as a whole PDF file titled `title`.
    ///
    /// # Errors
    ///
    /// The engine's error when it cannot lay the pages out.
    fn pdf(
        &self,
        title: &str,
        format: &navree_ui::charts::NumberFormat,
        locale: &navree_ui::charts::NumberLocale,
    ) -> Result<Vec<u8>, navree_documents::DocumentsError>;

    /// The PDF as a download named `file`, saved rather than shown, never cached and never
    /// sniffed, as [`ReportTable::csv_response`] answers the CSV.
    ///
    /// # Errors
    ///
    /// As [`pdf`](Self::pdf).
    fn pdf_response(
        &self,
        title: &str,
        format: &navree_ui::charts::NumberFormat,
        locale: &navree_ui::charts::NumberLocale,
        file: &str,
    ) -> Result<Response, navree_documents::DocumentsError>;
}

#[cfg(any(feature = "documents", feature = "documents-native"))]
impl ReportTablePdf for ReportTable {
    fn pdf(
        &self,
        title: &str,
        format: &navree_ui::charts::NumberFormat,
        locale: &navree_ui::charts::NumberLocale,
    ) -> Result<Vec<u8>, navree_documents::DocumentsError> {
        use navree_documents::PdfDocument;
        use navree_documents::native::{Align, Cell, Table};

        let columns = self.head().len();
        // The periods read from the start, the numbers line up at the end.
        let align = |at: usize, cell: Cell| {
            if at == 0 {
                cell
            } else {
                cell.align(Align::End)
            }
        };
        let mut table = Table::new().size(9.0).header(
            self.head()
                .iter()
                .enumerate()
                .map(|(at, name)| align(at, Cell::new(name.as_str()))),
        );
        for (row, period) in self.periods().iter().enumerate() {
            let mut cells = vec![Cell::new(period.as_str())];
            cells.extend(self.values().iter().enumerate().map(|(at, series)| {
                let text = series
                    .get(row)
                    .copied()
                    .flatten()
                    .map_or_else(String::new, |value| format.format(value, locale));
                align(at + 1, Cell::new(text))
            }));
            table = table.row(cells);
        }
        let mut document = PdfDocument::new()
            .title(title)
            .heading(1, title)
            .table(table)
            .footer_page_numbers();
        if columns > 6 {
            document = document.landscape();
        }
        Ok(document.build()?.into_bytes().into())
    }

    fn pdf_response(
        &self,
        title: &str,
        format: &navree_ui::charts::NumberFormat,
        locale: &navree_ui::charts::NumberLocale,
        file: &str,
    ) -> Result<Response, navree_documents::DocumentsError> {
        Ok(self.file_response(self.pdf(title, format, locale)?, "application/pdf", file))
    }
}
