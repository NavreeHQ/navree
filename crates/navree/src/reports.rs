//! A report's table as an Excel file, where Navree UI and the sheets meet
//! (the `ui` and `sheets` features). navree-ui writes a report's
//! [`ReportTable`] as CSV itself; it cannot write XLSX, because navree-sheets
//! sits above it.
//!
//! ```
//! use navree::reports::ReportTableSheets;
//! use navree::ui::charts::{Grain, Report};
//! use navree::ui::dates::DateRange;
//!
//! let day = |d| chrono::NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
//! let report = Report::new(DateRange::new(day(2), day(4)), Grain::Day, &chrono::Utc);
//! let values: &[Option<f64>] = &[Some(1_25_000.0), None, Some(80_000.5)];
//! let table = report.table("Day", &[("Collected", values)]);
//! let file = table.xlsx("Collections").unwrap();
//! assert!(file.starts_with(b"PK"));
//! ```

use navree_sheets::{Cell, Column, SheetsError, write_xlsx};
use navree_ui::charts::ReportTable;

/// [`ReportTable`] as an XLSX file: the first column holds the periods as text,
/// each series is a column of numbers (`#,##0.##`), and a gap is an empty cell.
pub trait ReportTableSheets {
    /// The table as a whole XLSX file with one sheet named `sheet`, its headings
    /// bold, frozen and filtered.
    ///
    /// # Errors
    ///
    /// When the table has more rows than a sheet holds, or the writer fails.
    fn xlsx(&self, sheet: &str) -> Result<Vec<u8>, SheetsError>;
}

impl ReportTableSheets for ReportTable {
    fn xlsx(&self, sheet: &str) -> Result<Vec<u8>, SheetsError> {
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
}
