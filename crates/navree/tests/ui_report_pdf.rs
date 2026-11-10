//! A report's table downloads as a PDF through the facade, where Navree UI and the
//! documents engine meet.

use chrono::{Days, NaiveDate, Utc};
use navree::pdf::text::page_texts;
use navree::pdf::{Document, ReadLimits};
use navree::reports::ReportTablePdf;
use navree::ui::charts::{Grain, NumberFormat, NumberLocale, Report};
use navree::ui::dates::DateRange;

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).expect("a real day")
}

fn indian() -> (NumberFormat, NumberLocale) {
    (NumberFormat::number(), NumberLocale::of("en-IN"))
}

#[test]
fn a_report_table_is_a_pdf_with_its_numbers_grouped_the_way_the_locale_groups_them() {
    let report = Report::new(DateRange::new(day(2), day(4)), Grain::Day, &Utc);
    let collected: &[Option<f64>] = &[Some(125_000.0), None, Some(80_000.5)];
    let table = report.table("Day", &[("Collected (INR)", collected)]);
    let (format, locale) = indian();
    let file = table
        .pdf("Collections", &format, &locale)
        .expect("the table is written");
    assert!(file.starts_with(b"%PDF-"));
    let document = Document::load(file, ReadLimits::default()).expect("the file is a PDF");
    assert_eq!(document.pages().len(), 1);
    let text = page_texts(&document).expect("the text is read").remove(0);
    for wanted in [
        "Collections",
        "Day",
        "Collected (INR)",
        "2026-11-02",
        "2026-11-04",
        "1,25,000",
        "80,000.5",
    ] {
        assert!(text.contains(wanted), "{wanted} is missing from {text}");
    }
}

#[test]
fn a_report_with_no_series_still_lists_its_periods() {
    let report = Report::new(DateRange::new(day(2), day(3)), Grain::Day, &Utc);
    let (format, locale) = indian();
    let file = report
        .table("Day", &[])
        .pdf("Days", &format, &locale)
        .expect("the table is written");
    let text = page_texts(&Document::load(file, ReadLimits::default()).expect("a PDF"))
        .expect("the text is read")
        .remove(0);
    assert!(
        text.contains("2026-11-02") && text.contains("2026-11-03"),
        "{text}"
    );
}

#[test]
fn a_long_report_takes_more_pages_and_each_repeats_the_headings_and_its_number() {
    let to = day(1).checked_add_days(Days::new(119)).expect("a real day");
    let report = Report::new(DateRange::new(day(1), to), Grain::Day, &Utc);
    let orders: Vec<Option<f64>> = (0..120).map(|at| Some(f64::from(at) * 1_000.0)).collect();
    let table = report.table("Day", &[("Orders", orders.as_slice())]);
    let (format, locale) = indian();
    let file = table
        .pdf("Orders", &format, &locale)
        .expect("the table is written");
    let document = Document::load(file, ReadLimits::default()).expect("the file is a PDF");
    let texts = page_texts(&document).expect("the text is read");
    assert!(texts.len() >= 2, "{} pages", texts.len());
    let pages = texts.len();
    for (at, page) in texts.iter().enumerate() {
        assert!(page.contains("Day") && page.contains("Orders"), "{page}");
        let number = format!("Page {} of {pages}", at + 1);
        assert!(page.contains(&number), "{number} is missing from {page}");
    }
}

#[test]
fn rupee_amounts_and_a_gurmukhi_name_print_as_written() {
    let report = Report::new(DateRange::new(day(2), day(3)), Grain::Day, &Utc);
    let values: &[Option<f64>] = &[Some(100_000.0), Some(250_000.5)];
    let table = report.table("Day", &[("ਗੁਰਪ੍ਰੀਤ ਕੌਰ", values)]);
    let file = table
        .pdf(
            "ਵਸੂਲੀ",
            &NumberFormat::currency("₹"),
            &NumberLocale::of("en-IN"),
        )
        .expect("the table is written");
    let text = page_texts(&Document::load(file, ReadLimits::default()).expect("a PDF"))
        .expect("the text is read")
        .remove(0);
    for wanted in ["ਵਸੂਲੀ", "ਗੁਰਪ੍ਰੀਤ ਕੌਰ", "₹1,00,000", "₹2,50,000.5"]
    {
        assert!(text.contains(wanted), "{wanted} is missing from {text}");
    }
    assert!(
        !text.contains('?'),
        "nothing is written as a question mark: {text}"
    );
}

#[test]
fn the_download_saves_as_a_pdf_with_a_safe_name_and_is_not_cached_or_sniffed() {
    let report = Report::new(DateRange::new(day(2), day(3)), Grain::Day, &Utc);
    let (format, locale) = indian();
    let response = report
        .table("Day", &[])
        .pdf_response("Days", &format, &locale, "../days \"november\".pdf; x")
        .expect("the table is written");
    let headers = response.headers();
    assert_eq!(headers["content-type"], "application/pdf");
    assert_eq!(
        headers["content-disposition"],
        "attachment; filename=\"..daysnovember.pdfx\""
    );
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(headers["x-content-type-options"], "nosniff");
}
