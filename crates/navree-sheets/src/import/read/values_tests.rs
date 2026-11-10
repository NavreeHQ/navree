//! What a cell's value reads as, the same in every format: the number format
//! of a cell format, times to the millisecond, the largest double, and the
//! text of a cell that has shapes in it.

use chrono::NaiveDate;

use super::opening_tests::{
    ONE_SHEET, cells_of, ods, ods_text_cell, sheet_rel, text, xlsx_sheet, xlsx_with,
};
use super::*;
use crate::import::workbook::xlsb_writer as xb;
use crate::import::xls::write as xw;

macro_rules! fixture {
    ($name:literal) => {
        &include_bytes!(concat!("../../../tests/fixtures/workbooks/", $name))[..]
    };
}

fn table_of(rows: &str) -> String {
    format!(r#"<table:table table:name="S">{rows}</table:table>"#)
}

// The number format of a cell format.

#[test]
fn a_number_format_given_a_built_in_id_is_that_format_in_every_format() {
    use xb::Value::*;
    let records = vec![
        xb::row(0),
        xb::cell(0, 0, &Text("v".into())),
        xb::row(1),
        xb::cell(0, 1, &Real(46082.0)),
    ];
    // Cell format 1 has the number format 14, which the file gives the code
    // `0.00`: a number, not a date.
    let binary = xb::workbook(
        &[("S", Some(xb::sheet(&records)))],
        &[],
        &[(14, "0.00")],
        &[0, 14],
        false,
    );
    let styles = br#"<styleSheet><numFmts><numFmt numFmtId="14" formatCode="0.00"/></numFmts><cellXfs><xf numFmtId="0"/><xf numFmtId="14"/></cellXfs></styleSheet>"#;
    let sheet = br#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>v</t></is></c></row><row r="2"><c r="A2" s="1"><v>46082</v></c></row></sheetData></worksheet>"#;
    let xml = xlsx_with(
        &sheet_rel("s.xml"),
        ONE_SHEET,
        &[("xl/styles.xml", styles), ("xl/s.xml", sheet)],
    );
    let expected = vec![vec![text("v")], vec![Cell::Float(46082.0)]];
    assert_eq!(cells_of(&xml, "x.xlsx").unwrap(), expected);
    assert_eq!(cells_of(&binary, "x.xlsb").unwrap(), expected);
}

// Times.

fn at(day: u32, hour: u32, minute: u32, second: u32, milli: u32) -> Cell {
    Cell::DateTime(
        NaiveDate::from_ymd_opt(2026, 3, day)
            .unwrap()
            .and_hms_milli_opt(hour, minute, second, milli)
            .unwrap(),
    )
}

fn date(day: u32) -> Cell {
    Cell::Date(NaiveDate::from_ymd_opt(2026, 3, day).unwrap())
}

#[test]
fn a_time_a_file_writes_is_read_to_the_millisecond_in_every_format() {
    for (written, expected) in [
        ("2026-03-01T02:57:46.666570", at(1, 2, 57, 46, 667)),
        ("2026-03-01T02:57:46.6664", at(1, 2, 57, 46, 666)),
        ("2026-03-01T02:57:46.5005", at(1, 2, 57, 46, 501)),
        ("2026-03-01T02:57:46.5004", at(1, 2, 57, 46, 500)),
        ("2026-03-01T02:57:46.123456789Z", at(1, 2, 57, 46, 123)),
        ("2026-03-01T02:57:46.25", at(1, 2, 57, 46, 250)),
        ("2026-03-01T02:57:46", at(1, 2, 57, 46, 0)),
        // Rounded up to midnight, it is a date.
        ("2026-03-01T23:59:59.9996", date(2)),
        ("2026-03-01T00:00:00.0004", date(1)),
    ] {
        let ods = ods(&table_of(&format!(
            r#"<table:table-row>{}</table:table-row><table:table-row><table:table-cell office:value-type="date" office:date-value="{written}"/></table:table-row>"#,
            ods_text_cell("h")
        )));
        let xlsx = xlsx_sheet(
            "s.xml",
            "xl/s.xml",
            &format!(
                r#"<row r="1"><c r="A1" t="inlineStr"><is><t>h</t></is></c></row><row r="2"><c r="A2" t="d"><v>{written}</v></c></row>"#
            ),
        );
        assert_eq!(
            cells_of(&ods, "x.ods").unwrap()[1],
            std::slice::from_ref(&expected),
            "{written} in ODS"
        );
        assert_eq!(
            cells_of(&xlsx, "x.xlsx").unwrap()[1],
            [expected],
            "{written} in XLSX"
        );
    }
}

// The largest double.

fn max() -> Cell {
    Cell::Float(f64::MAX)
}

/// The cells of a workbook of the kind `largest.py` writes: a header, the
/// largest double, its negative, the smallest, and one.
fn largest(file: &[u8], name: &str) -> Vec<Vec<Cell>> {
    cells_of(file, name).unwrap()
}

fn check_largest(file: &[u8], name: &str) {
    let got = largest(file, name);
    assert_eq!(got[0], [text("what"), text("value")], "{name}");
    assert_eq!(got[1], [text("big"), max()], "{name}");
    assert_eq!(got[2], [text("negative"), Cell::Float(-f64::MAX)], "{name}");
    assert_eq!(got[3], [text("small"), Cell::Float(5e-324)], "{name}");
    assert_eq!(got[4][0], text("one"), "{name}");
    assert_eq!(got[4][1].to_text(), "1", "{name}");
}

#[test]
fn the_largest_double_is_read_as_every_writer_writes_it() {
    // openpyxl writes it with 16 digits, which round up past the largest
    // double; LibreOffice reads those 16 digits as infinity and writes `INF`
    // (XLSX, ODS) or an infinity (XLS).
    check_largest(fixture!("largest-openpyxl.xlsx"), "x.xlsx");
    check_largest(fixture!("largest-libreoffice.xlsx"), "x.xlsx");
    check_largest(fixture!("largest-libreoffice.ods"), "x.ods");
    check_largest(fixture!("largest-libreoffice.xls"), "x.xls");
}

/// A sheet whose header is `h` and whose one value is `<v>value</v>` in a
/// cell of the attributes `attributes`.
fn valued(attributes: &str, value: &str) -> Vec<u8> {
    xlsx_sheet(
        "s.xml",
        "xl/s.xml",
        &format!(
            r#"<row r="1"><c r="A1" t="inlineStr"><is><t>h</t></is></c></row><row r="2"><c r="A2"{attributes}><v>{value}</v></c></row>"#
        ),
    )
}

fn typed(value: &str) -> Vec<u8> {
    valued(r#" t="n""#, value)
}

fn untyped(value: &str) -> Vec<u8> {
    valued("", value)
}

#[test]
fn a_number_past_the_largest_double_is_the_largest_double_of_its_sign() {
    for (written, expected) in [
        ("1.797693134862316e+308", max()),
        ("1.79769313486232E+308", max()),
        ("-1.797693134862316e+308", Cell::Float(-f64::MAX)),
        ("1e999", max()),
        ("-1e999", Cell::Float(-f64::MAX)),
        ("INF", max()),
        ("inf", max()),
        ("+Infinity", max()),
        ("-INF", Cell::Float(-f64::MAX)),
        ("-infinity", Cell::Float(-f64::MAX)),
        // Not past it.
        ("1.7976931348623157e308", max()),
        ("5e-324", Cell::Float(5e-324)),
    ] {
        let got = cells_of(&typed(written), "x.xlsx").unwrap();
        assert_eq!(got[1], [expected], "{written} typed");
    }
}

#[test]
fn a_value_without_a_type_is_a_number_when_digits_say_so_and_the_text_it_is_otherwise() {
    for (written, expected) in [
        ("1.79769313486232E+308", max()),
        ("-1e999", Cell::Float(-f64::MAX)),
        ("2.5", Cell::Float(2.5)),
    ] {
        let got = cells_of(&untyped(written), "x.xlsx").unwrap();
        assert_eq!(got[1], [expected], "{written} untyped");
    }
    for written in ["NaN", "inf", "-inf", "infinity", "1e", "n/a"] {
        let got = cells_of(&untyped(written), "x.xlsx").unwrap();
        assert_eq!(got[1], [text(written)], "{written} untyped");
    }
}

#[test]
fn a_number_that_is_no_number_makes_the_file_unreadable() {
    for written in ["NaN", "nan", "-NaN", "1e", "x", "0x10", "1_000"] {
        let got = cells_of(&typed(written), "x.xlsx");
        assert!(got.is_err(), "{written}: {got:?}");
    }
    let file = ods(&table_of(
        r#"<table:table-row><table:table-cell office:value-type="float" office:value="NaN"/></table:table-row>"#,
    ));
    assert!(cells_of(&file, "x.ods").is_err());
}

#[test]
fn an_infinity_in_a_binary_workbook_is_the_largest_double_of_its_sign() {
    use xb::Value::*;
    for (value, expected) in [
        (f64::INFINITY, f64::MAX),
        (f64::NEG_INFINITY, -f64::MAX),
        (f64::MAX, f64::MAX),
    ] {
        let records = vec![
            xb::row(0),
            xb::cell(0, 0, &Text("h".into())),
            xb::row(1),
            xb::cell(0, 0, &Real(value)),
        ];
        let file = xb::workbook(&[("S", Some(xb::sheet(&records)))], &[], &[], &[0], false);
        assert_eq!(
            cells_of(&file, "x.xlsb").unwrap()[1],
            [Cell::Float(expected)],
            "XLSB {value}"
        );

        let mut records = xw::label(0, 0, 0, "h", false);
        records.extend(xw::number(1, 0, 0, value));
        let file = xw::Book::new(vec![], vec![xw::Sheet::new("S", records)]).file();
        assert_eq!(
            cells_of(&file, "x.xls").unwrap()[1],
            [Cell::Float(expected)],
            "XLS {value}"
        );
    }
    // And a number that is not one is still refused.
    let mut records = xw::label(0, 0, 0, "h", false);
    records.extend(xw::number(1, 0, 0, f64::NAN));
    let file = xw::Book::new(vec![], vec![xw::Sheet::new("S", records)]).file();
    assert!(cells_of(&file, "x.xls").is_err());
}

#[test]
fn an_infinity_in_a_date_format_is_a_number_like_any_other() {
    let styles =
        br#"<styleSheet><cellXfs><xf numFmtId="0"/><xf numFmtId="14"/></cellXfs></styleSheet>"#;
    let sheet = br#"<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>h</t></is></c></row><row r="2"><c r="A2" s="1" t="n"><v>INF</v></c></row></sheetData></worksheet>"#;
    let file = xlsx_with(
        &sheet_rel("s.xml"),
        ONE_SHEET,
        &[("xl/styles.xml", styles), ("xl/s.xml", sheet)],
    );
    assert_eq!(cells_of(&file, "x.xlsx").unwrap()[1], [max()]);
}

// Shapes in a cell.

#[test]
fn what_a_shape_in_a_cell_says_is_not_the_cell_s_text() {
    let got = cells_of(fixture!("shape-libreoffice.ods"), "x.ods").unwrap();
    assert_eq!(got, [vec![text("head")], vec![text("cell text")]]);

    let row = |cell: &str| {
        format!(
            r#"<table:table-row><table:table-cell office:value-type="string">{cell}</table:table-cell></table:table-row>"#
        )
    };
    let file = ods(&table_of(&format!(
        "{}{}{}{}",
        row("<text:p>head</text:p>"),
        row(
            "<draw:frame><draw:text-box><text:p>framed</text:p></draw:text-box></draw:frame><text:p>one</text:p>"
        ),
        row(
            "<text:p>two</text:p><draw:g><draw:frame><draw:frame><text:p>inner</text:p></draw:frame><text:p>outer</text:p></draw:frame></draw:g>"
        ),
        row(
            "<text:p>three <text:span>four</text:span></text:p><draw:rect><text:p>x</text:p></draw:rect>"
        ),
    )));
    let got = cells_of(&file, "x.ods").unwrap();
    assert_eq!(
        got,
        [
            vec![text("head")],
            vec![text("one")],
            vec![text("two")],
            vec![text("three four")]
        ]
    );
}

#[test]
fn a_cell_whose_only_text_is_a_shape_s_is_empty() {
    let file = ods(&table_of(
        r#"<table:table-row><table:table-cell office:value-type="string"><text:p>head</text:p></table:table-cell></table:table-row><table:table-row><table:table-cell office:value-type="string"><draw:frame><draw:text-box><text:p>framed</text:p></draw:text-box></draw:frame></table:table-cell></table:table-row>"#,
    ));
    assert_eq!(cells_of(&file, "x.ods").unwrap(), [vec![text("head")]]);
}
