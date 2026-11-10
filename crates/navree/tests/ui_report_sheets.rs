//! A report's table downloads as an Excel file through the facade, where
//! Navree UI and the sheets meet.

use chrono::{NaiveDate, Utc};
use navree::reports::ReportTableSheets;
use navree::ui::charts::{Grain, Report};
use navree::ui::dates::DateRange;

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).expect("a real day")
}

#[test]
fn a_report_table_is_an_xlsx_file_with_its_gaps_left_empty() {
    let report = Report::new(DateRange::new(day(2), day(4)), Grain::Day, &Utc);
    let collected: &[Option<f64>] = &[Some(125_000.0), None, Some(80_000.5)];
    let refunded: &[Option<f64>] = &[Some(1_500.0)];
    let table = report.table("Day", &[("Collected", collected), ("Refunded", refunded)]);
    let file = table.xlsx("Collections").expect("the table is written");
    assert!(file.starts_with(b"PK"), "an XLSX file is a zip archive");
    assert!(file.len() > 1_000);
}

#[test]
fn a_report_with_no_series_still_lists_its_periods() {
    let report = Report::new(DateRange::new(day(2), day(3)), Grain::Day, &Utc);
    let file = report
        .table("Day", &[])
        .xlsx("Days")
        .expect("the table is written");
    assert!(file.starts_with(b"PK"));
}

#[test]
fn the_workbook_downloads_with_a_safe_name_and_is_not_cached_or_sniffed() {
    let report = Report::new(DateRange::new(day(2), day(3)), Grain::Day, &Utc);
    let response = report
        .table("Day", &[])
        .xlsx_response("Days", "../days \"november\".xlsx; x")
        .expect("the table is written");
    let headers = response.headers();
    assert_eq!(
        headers["content-type"],
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    );
    assert_eq!(
        headers["content-disposition"],
        "attachment; filename=\"..daysnovember.xlsxx\""
    );
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(headers["x-content-type-options"], "nosniff");
}
