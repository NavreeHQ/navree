//! Reading workbooks: what opening one may cost, whatever the file says, the
//! valid packages it must read, and the rules for dates, errors and text.

use std::io::{Cursor, Write};
use std::time::{Duration, Instant};

use super::*;
use crate::import::workbook::dates::excel_date;
use crate::import::workbook::xlsb_writer as xb;
use crate::import::xls::write::{self as xw, compound, record};

pub(super) const MIB: usize = 1 << 20;

pub(super) const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";

pub(super) fn zip_parts(parts: &[(&str, &[u8])], stored: bool) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut options = zip::write::SimpleFileOptions::default();
    if stored {
        options = options.compression_method(zip::CompressionMethod::Stored);
    }
    for (name, bytes) in parts {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

pub(super) fn opts(max_text: u64) -> ReadOptions<'static> {
    ReadOptions {
        max_rows: 100_000,
        max_text_bytes: max_text,
        ..ReadOptions::default()
    }
}

/// The table of the first sheet, its header first.
pub(super) fn cells_of(file: &[u8], name: &str) -> Result<Vec<Vec<Cell>>, String> {
    read_table(file, name, &opts(1 << 28))
        .map(|table| {
            let mut all = vec![
                table
                    .headers
                    .iter()
                    .map(|header| Cell::Text(header.clone()))
                    .collect::<Vec<_>>(),
            ];
            all.extend(table.rows.into_iter().map(|row| row.cells));
            all
        })
        .map_err(|error| error.to_string())
}

pub(super) fn text(value: &str) -> Cell {
    Cell::Text(value.into())
}

fn date(year: i32, month: u32, day: u32) -> Cell {
    Cell::Date(chrono::NaiveDate::from_ymd_opt(year, month, day).unwrap())
}

pub(super) fn refused_with(result: &Result<Table, SheetsError>, what: &str) -> bool {
    matches!(result, Err(error) if error.to_string().contains(what))
}

macro_rules! fixture {
    ($name:literal) => {
        &include_bytes!(concat!("../../../tests/fixtures/workbooks/", $name))[..]
    };
}

/// The memory a process has used at most, by asking the operating system.
#[cfg(target_os = "linux")]
fn kib_of(key: &str) -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("procfs");
    status
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap_or_else(|| panic!("{key} is in /proc/self/status"))
}

/// The result of `read` and the bytes it held at most, after forgetting the
/// peak of everything before.
#[cfg(target_os = "linux")]
fn measured<T>(read: impl FnOnce() -> T) -> (T, usize) {
    std::fs::write("/proc/self/clear_refs", "5").expect("clear_refs");
    let before = kib_of("VmHWM:");
    let result = read();
    let peak = kib_of("VmHWM:").saturating_sub(before) as usize * 1024;
    eprintln!("held {peak} bytes at most");
    (result, peak)
}

/// Runs a test of memory alone in a process, so that its peak is its own.
#[cfg(target_os = "linux")]
fn run_alone(test: &str) {
    let this = module_path!().split_once("::").unwrap().1;
    // nosemgrep: navree-process-outside-tooling
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("{this}::{test}"),
            "--ignored",
            "--test-threads=1",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

/// A test of memory: `$body` runs alone in a process, as `$name`.
macro_rules! alone {
    ($name:ident, $inner:ident, $body:block) => {
        #[cfg(target_os = "linux")]
        #[test]
        #[ignore = "runs in a process of its own, from the test beside it"]
        fn $inner() $body

        #[cfg(target_os = "linux")]
        #[test]
        fn $name() {
            run_alone(stringify!($inner));
        }
    };
}

// What an ODS row may make.

pub(super) fn ods(content: &str) -> Vec<u8> {
    let manifest = r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/></manifest:manifest>"#;
    let doc = format!(
        r#"<office:document-content xmlns:office="o" xmlns:table="t" xmlns:text="x"><office:body><office:spreadsheet>{content}</office:spreadsheet></office:body></office:document-content>"#
    );
    zip_parts(
        &[
            (
                "mimetype",
                b"application/vnd.oasis.opendocument.spreadsheet",
            ),
            ("META-INF/manifest.xml", manifest.as_bytes()),
            ("content.xml", doc.as_bytes()),
        ],
        false,
    )
}

fn spaced_cell(spaces: usize) -> String {
    format!(
        r#"<table:table-cell office:value-type="string"><text:p>a<text:s text:c="{spaces}"/>b</text:p></table:table-cell>"#
    )
}

pub(super) fn ods_text_cell(value: &str) -> String {
    format!(
        r#"<table:table-cell office:value-type="string"><text:p>{value}</text:p></table:table-cell>"#
    )
}

alone!(
    the_spaces_of_an_ods_row_count_for_the_row_not_for_each_cell,
    peak_of_ods_spaces_in_one_row,
    {
        // Sixty-four cells of 1 MiB under a limit of 1 MiB would be 64 MiB.
        let file = ods(&format!(
            r#"<table:table table:name="S"><table:table-row>{}</table:table-row></table:table>"#,
            spaced_cell(MIB - 8).repeat(64)
        ));
        let (result, peak) = measured(|| read_table(&file, "x.ods", &opts(MIB as u64)));
        assert!(
            refused_with(&result, "more than 1048576 bytes of text"),
            "{result:?}"
        );
        assert!(peak < 8 * MIB, "held {peak} bytes at most");
    }
);

alone!(
    spaces_in_ods_cells_past_the_last_column_are_not_made,
    peak_of_ods_spaces_past_the_last_column,
    {
        let file = ods(&format!(
            r#"<table:table table:name="S"><table:table-row><table:table-cell office:value-type="float" office:value="1"/><table:table-cell table:number-columns-repeated="20000"/>{}</table:table-row></table:table>"#,
            spaced_cell(MIB - 8).repeat(64)
        ));
        let (result, peak) = measured(|| read_table(&file, "x.ods", &opts(MIB as u64)));
        // The row is its first cell: the rest of it is past every column.
        assert_eq!(result.unwrap().headers, ["1"]);
        assert!(peak < 8 * MIB, "held {peak} bytes at most");
    }
);

alone!(
    spaces_in_ods_cells_past_the_header_are_not_made,
    peak_of_ods_spaces_past_the_header,
    {
        let file = ods(&format!(
            r#"<table:table table:name="S"><table:table-row>{}</table:table-row><table:table-row>{}{}</table:table-row></table:table>"#,
            ods_text_cell("h"),
            ods_text_cell("v"),
            spaced_cell(MIB - 8).repeat(64)
        ));
        let (result, peak) = measured(|| read_table(&file, "x.ods", &opts(MIB as u64)));
        let table = result.unwrap();
        assert_eq!(table.rows[0].cells, [text("v")]);
        assert!(peak < 8 * MIB, "held {peak} bytes at most");
    }
);

#[test]
fn the_spaces_of_a_row_count_together_against_the_limit() {
    // Each cell is within the limit, and the two together are not.
    let row = |cells: usize| {
        ods(&format!(
            r#"<table:table table:name="S"><table:table-row>{}</table:table-row></table:table>"#,
            spaced_cell(600).repeat(cells)
        ))
    };
    assert!(read_table(&row(1), "x.ods", &opts(1024)).is_ok());
    let result = read_table(&row(2), "x.ods", &opts(1024));
    assert!(
        refused_with(&result, "more than 1024 bytes of text"),
        "{result:?}"
    );
}

#[test]
fn a_row_cell_past_the_header_still_makes_a_row_a_row() {
    let file = ods(&format!(
        r#"<table:table table:name="S"><table:table-row>{}</table:table-row><table:table-row><table:table-cell table:number-columns-repeated="3"/>{}</table:table-row></table:table>"#,
        ods_text_cell("h"),
        ods_text_cell("far")
    ));
    let got = cells_of(&file, "x.ods").unwrap();
    assert_eq!(got, [vec![text("h")], vec![Cell::Empty]]);
}

// What opening holds: one relationship is not copied per sheet.

fn xlsx_with(workbook_rels: &str, workbook: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let rels = format!(
        r#"<Relationships><Relationship Id="rId1" Type="{REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let mut parts: Vec<(&str, &[u8])> = vec![
        ("_rels/.rels", rels.as_bytes()),
        ("xl/_rels/workbook.xml.rels", workbook_rels.as_bytes()),
        ("xl/workbook.xml", workbook.as_bytes()),
    ];
    parts.extend_from_slice(extra);
    zip_parts(&parts, false)
}

const ONE_SHEET: &str = r#"<workbook><sheets><sheet name="s" r:id="r"/></sheets></workbook>"#;
const ONE: &str = r#"<row r="1"><c r="A1"><v>7</v></c></row>"#;

fn sheet_rel(target: &str) -> String {
    format!(
        r#"<Relationships><Relationship Id="r" Type="{WS}" Target="{target}"/></Relationships>"#
    )
}

fn xlsx_sheet(rel_target: &str, part: &str, cells: &str) -> Vec<u8> {
    let sheet = format!("<worksheet><sheetData>{cells}</sheetData></worksheet>");
    xlsx_with(
        &sheet_rel(rel_target),
        ONE_SHEET,
        &[(part, sheet.as_bytes())],
    )
}

fn xlsb_root() -> String {
    format!(
        r#"<Relationships><Relationship Id="rId1" Type="{REL}/officeDocument" Target="xl/workbook.bin"/></Relationships>"#
    )
}

/// A `BrtBundleSh` of a visible sheet, its relationship and its name.
fn xlsb_sheet_record(id: &str, name: &str) -> (u16, Vec<u8>) {
    let mut body = 0u32.to_le_bytes().to_vec();
    body.extend_from_slice(&1u32.to_le_bytes());
    body.extend(xb::wide(id));
    body.extend(xb::wide(name));
    (0x9C, body)
}

alone!(
    one_long_relationship_named_by_every_sheet_is_held_once,
    peak_of_one_long_relationship,
    {
        // 256 KiB of target for each of 1,024 sheets would be 256 MiB.
        let rels = sheet_rel(&"a".repeat(256 * 1024));
        let book = format!(
            "<workbook><sheets>{}</sheets></workbook>",
            r#"<sheet name="s" r:id="r"/>"#.repeat(1024)
        );
        let file = xlsx_with(&rels, &book, &[]);
        let (_, peak) = measured(|| read_table(&file, "x.xlsx", &opts(1 << 28)));
        assert!(peak < 16 * MIB, "held {peak} bytes at most");
    }
);

alone!(
    one_long_xlsb_relationship_named_by_every_sheet_is_held_once,
    peak_of_one_long_xlsb_relationship,
    {
        let target = format!("worksheets/{}", "a".repeat(256 * 1024));
        let rels =
            format!(r#"<Relationships><Relationship Id="r" Target="{target}"/></Relationships>"#);
        let mut records = vec![xlsb_sheet_record("r", "s"); 1024];
        records.push((0x90, vec![]));
        let book = xb::part(&records);
        let root = xlsb_root();
        let file = zip_parts(
            &[
                ("_rels/.rels", root.as_bytes()),
                ("xl/_rels/workbook.bin.rels", rels.as_bytes()),
                ("xl/workbook.bin", &book),
            ],
            false,
        );
        let (_, peak) = measured(|| read_table(&file, "x.xlsb", &opts(1 << 28)));
        assert!(peak < 16 * MIB, "held {peak} bytes at most");
    }
);

/// Over the most sheets a workbook has.
const TOO_MANY_SHEETS: usize = 65_536;

#[test]
fn a_workbook_of_too_many_sheets_is_refused_in_every_format() {
    let what = "more than 65535 sheets";
    let xlsx = {
        let book = format!(
            "<workbook><sheets>{}</sheets></workbook>",
            r#"<sheet name="s" r:id="r"/>"#.repeat(TOO_MANY_SHEETS)
        );
        xlsx_with(&sheet_rel("s.xml"), &book, &[])
    };
    let result = read_table(&xlsx, "x.xlsx", &opts(1 << 28));
    assert!(refused_with(&result, what), "XLSX: {result:?}");

    let xlsb = {
        let rels =
            r#"<Relationships><Relationship Id="r" Target="worksheets/s.bin"/></Relationships>"#;
        let mut records = vec![xlsb_sheet_record("r", "s"); TOO_MANY_SHEETS];
        records.push((0x90, vec![]));
        let root = xlsb_root();
        zip_parts(
            &[
                ("_rels/.rels", root.as_bytes()),
                ("xl/_rels/workbook.bin.rels", rels.as_bytes()),
                ("xl/workbook.bin", &xb::part(&records)),
            ],
            false,
        )
    };
    let result = read_table(&xlsb, "x.xlsb", &opts(1 << 28));
    assert!(refused_with(&result, what), "XLSB: {result:?}");

    let file = ods(&r#"<table:table table:name="s"></table:table>"#.repeat(TOO_MANY_SHEETS));
    let result = read_table(&file, "x.ods", &opts(1 << 28));
    assert!(refused_with(&result, what), "ODS: {result:?}");

    // One empty sheet, each listing at an offset of its own.
    let bound = |offset: u32| {
        let mut data = offset.to_le_bytes().to_vec();
        data.extend_from_slice(&[0, 0, 1, 0, b'a']);
        record(0x0085, &data)
    };
    let eof = record(0x000A, &[]);
    let start = xls_bof().len() + TOO_MANY_SHEETS * bound(0).len() + eof.len();
    let mut stream = xls_bof();
    for at in 0..TOO_MANY_SHEETS {
        stream.extend(bound((start + at * eof.len()) as u32));
    }
    stream.extend(&eof);
    stream.extend(eof.repeat(TOO_MANY_SHEETS));
    let file = compound(&[("Workbook", &stream)]).bytes;
    let result = read_table(&file, "x.xls", &opts(1 << 28));
    assert!(refused_with(&result, what), "XLS: {result:?}");
}

/// What the lists opening builds take counts together: each of these parts
/// is within the limit of a part, and all of them are not within it.
#[test]
fn the_lists_opening_builds_share_one_budget() {
    let limit = MIB as u64;
    let options = ReadOptions {
        max_inflated_bytes: limit,
        ..opts(1 << 28)
    };
    let what = "cannot be read";

    // Relationships, and the room their table takes.
    let many = r#"<Relationship Id="a" Target="b"/>"#.repeat(30_000);
    let rels = format!(
        r#"<Relationships>{many}<Relationship Id="r" Type="{WS}" Target="s.xml"/></Relationships>"#
    );
    let sheet = format!("<worksheet><sheetData>{ONE}</sheetData></worksheet>");
    let file = xlsx_with(&rels, ONE_SHEET, &[("xl/s.xml", sheet.as_bytes())]);
    let result = read_table(&file, "x.xlsx", &options);
    assert!(refused_with(&result, what), "relationships: {result:?}");

    // A sheet name and shared strings, each most of the limit.
    let long = "n".repeat(MIB - 1024);
    let book = format!(r#"<workbook><sheets><sheet name="{long}" r:id="r"/></sheets></workbook>"#);
    let strings = format!("<sst><si><t>{long}</t></si></sst>");
    let file = xlsx_with(
        &sheet_rel("s.xml"),
        &book,
        &[
            ("xl/s.xml", sheet.as_bytes()),
            ("xl/sharedStrings.xml", strings.as_bytes()),
        ],
    );
    let result = read_table(&file, "x.xlsx", &options);
    assert!(refused_with(&result, "more than"), "names: {result:?}");

    // Relationships and number formats, each within the budget alone.
    let many = r#"<Relationship Id="a" Target="b"/>"#.repeat(12_000);
    let rels = format!(
        r#"<Relationships>{many}<Relationship Id="r" Type="{WS}" Target="s.xml"/></Relationships>"#
    );
    let styles = format!(
        "<styleSheet><numFmts>{}</numFmts></styleSheet>",
        (0..8_000)
            .map(|id| format!(r#"<numFmt numFmtId="{id}" formatCode="0"/>"#))
            .collect::<String>()
    );
    let file = xlsx_with(
        &rels,
        ONE_SHEET,
        &[
            ("xl/s.xml", sheet.as_bytes()),
            ("xl/styles.xml", styles.as_bytes()),
        ],
    );
    let result = read_table(&file, "x.xlsx", &options);
    assert!(refused_with(&result, what), "formats: {result:?}");
}

fn xlsx_sheet_with_styles(styles: &str) -> Vec<u8> {
    let sheet = format!("<worksheet><sheetData>{ONE}</sheetData></worksheet>");
    xlsx_with(
        &sheet_rel("s.xml"),
        ONE_SHEET,
        &[
            ("xl/s.xml", sheet.as_bytes()),
            ("xl/styles.xml", styles.as_bytes()),
        ],
    )
}

/// Five parts of 1 MiB under a limit of 1 MiB a part: within the limit of a
/// part each, but opening is not to read without end whatever the parts.
#[test]
fn opening_stops_when_it_has_read_four_times_the_limit_of_a_part() {
    let limit = MIB as u64;
    let padding = format!("<!--{}-->", "p".repeat(MIB - 256));
    let root = format!(
        r#"<Relationships>{padding}<Relationship Id="rId1" Type="{REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let rels = format!(
        r#"<Relationships>{padding}<Relationship Id="r" Type="{WS}" Target="s.xml"/></Relationships>"#
    );
    let book =
        format!(r#"<workbook>{padding}<sheets><sheet name="s" r:id="r"/></sheets></workbook>"#);
    let strings = format!("<sst>{padding}</sst>");
    let styles = format!("<styleSheet>{padding}</styleSheet>");
    let sheet = format!("<worksheet><sheetData>{ONE}</sheetData></worksheet>");
    let file = zip_parts(
        &[
            ("_rels/.rels", root.as_bytes()),
            ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
            ("xl/workbook.xml", book.as_bytes()),
            ("xl/sharedStrings.xml", strings.as_bytes()),
            ("xl/styles.xml", styles.as_bytes()),
            ("xl/s.xml", sheet.as_bytes()),
        ],
        false,
    );
    let options = ReadOptions {
        max_inflated_bytes: limit,
        ..opts(1 << 28)
    };
    let result = read_table(&file, "x.xlsx", &options);
    assert!(
        refused_with(&result, "takes more than 4194304 bytes of reading to open"),
        "{result:?}"
    );
    // The same parts open under a limit that lets them.
    let options = ReadOptions {
        max_inflated_bytes: 2 * limit,
        ..opts(1 << 28)
    };
    read_table(&file, "x.xlsx", &options).unwrap();
}

// What opening does: each thing once.

fn styles_book(code: usize, xfs: usize) -> Vec<u8> {
    let long = "0".repeat(code);
    let styles = format!(
        r#"<styleSheet><numFmts><numFmt numFmtId="164" formatCode="{long}"/></numFmts><cellXfs>{}</cellXfs></styleSheet>"#,
        r#"<xf numFmtId="164"/>"#.repeat(xfs)
    );
    xlsx_sheet_with_styles(&styles)
}

#[test]
fn a_long_number_format_is_read_once_not_once_per_cell_format() {
    // A scan of the code for every cell format: 64 KiB x 16,000, 1e9 steps,
    // over two seconds; once, it is under a millisecond and the parts are
    // read in milliseconds.
    let file = styles_book(64 * 1024, 16_000);
    let at = Instant::now();
    let result = read_table(&file, "x.xlsx", &opts(1 << 28));
    let took = at.elapsed();
    eprintln!("took {took:?}");
    assert_eq!(result.unwrap().headers, ["7"]);
    assert!(took < Duration::from_secs(1), "{took:?}");
}

fn xls_bof() -> Vec<u8> {
    let mut data = 0x0600u16.to_le_bytes().to_vec();
    data.extend_from_slice(&0x0005u16.to_le_bytes());
    data.extend_from_slice(&[0; 12]);
    record(0x0809, &data)
}

/// A workbook of `count` listings of sheets, the offset of each given by
/// `offset` from the first record of one sheet with a number and `records`
/// empty records.
fn xls_listed(count: usize, records: usize, offset: impl Fn(usize, usize) -> usize) -> Vec<u8> {
    let eof = record(0x000A, &[]);
    let bound = |at: u32| {
        let mut data = at.to_le_bytes().to_vec();
        data.extend_from_slice(&[0, 0, 1, 0, b'a']);
        record(0x0085, &data)
    };
    let first = xls_bof().len() + count * bound(0).len() + eof.len();
    let mut stream = xls_bof();
    for listing in 0..count {
        stream.extend(bound(offset(first, listing) as u32));
    }
    stream.extend(&eof);
    stream.extend(xls_bof());
    stream.extend(xw::number(0, 0, 0, 1.0));
    stream.extend(record(0x0001, &[]).repeat(records));
    stream.extend(&eof);
    compound(&[("Workbook", &stream)]).bytes
}

#[test]
fn an_xls_sheet_listed_many_times_is_refused_and_not_read_each_time() {
    let file = xls_listed(1_000, 100_000, |first, _| first);
    let at = Instant::now();
    let result = read_table(&file, "x.xls", &opts(1 << 28));
    let took = at.elapsed();
    eprintln!("took {took:?}");
    assert!(refused_with(&result, "same record"), "{result:?}");
    assert!(took < Duration::from_millis(200), "{took:?}");
}

#[test]
fn xls_sheets_that_overlap_are_refused_and_not_each_read_to_the_end() {
    // Every listing starts four bytes after the one before, in one sheet:
    // read from each to its end, 4e8 records.
    let bof = xls_bof().len();
    let number = xw::number(0, 0, 0, 1.0).len();
    let file = xls_listed(2_000, 200_000, |first, listing| {
        first + bof + number + listing * 4
    });
    let at = Instant::now();
    let result = read_table(&file, "x.xls", &opts(1 << 28));
    let took = at.elapsed();
    eprintln!("took {took:?}");
    assert!(
        refused_with(&result, "runs into the sheet after it"),
        "{result:?}"
    );
    assert!(took < Duration::from_millis(500), "{took:?}");
}

#[test]
fn xls_sheets_one_after_the_other_read_as_their_own_cells() {
    let sheets = vec![
        xw::Sheet::new("A", xw::number(0, 0, 0, 1.0)),
        xw::Sheet::new("B", xw::number(0, 0, 0, 2.0)),
    ];
    let file = xw::Book::new(vec![], sheets).file();
    let options = ReadOptions {
        sheet: Some("B"),
        ..opts(1 << 28)
    };
    assert_eq!(read_table(&file, "x.xls", &options).unwrap().headers, ["2"]);
}

#[test]
fn xls_cell_formats_past_the_ones_a_cell_can_name_are_not_kept() {
    // A cell names its format in 16 bits: the rest of the table is no use.
    let mut globals = xw::xf(14).repeat(70_000);
    globals.extend(xw::datemode(false));
    let mut records = xw::number(0, 0, 65_535, 46082.0);
    records.extend(xw::number(0, 1, 0, 46082.0));
    let file = xw::Book::new(globals, vec![xw::Sheet::new("S", records)]).file();
    let table = cells_of(&file, "x.xls").unwrap();
    assert_eq!(table[0], [text("2026-03-01"), text("2026-03-01 (2)")]);
}

// What a surrogate pair is across records.

#[test]
fn a_surrogate_pair_split_across_continue_records_is_one_character() {
    let mut sst = vec![0u8; 8];
    sst.extend([2u8, 0, 1]);
    sst.extend([0x3D, 0xD8]);
    let mut globals = record(0x00FC, &sst);
    globals.extend(record(0x003C, &[1, 0x00, 0xDE]));
    let file = xw::Book::new(
        globals,
        vec![xw::Sheet::new("S", xw::label_sst(0, 0, 0, 0))],
    )
    .file();
    assert_eq!(cells_of(&file, "x.xls").unwrap()[0][0], text("\u{1F600}"));
}

#[test]
fn half_a_pair_at_the_end_of_a_string_or_before_narrow_text_is_a_replacement() {
    // The high half ends the string.
    let mut sst = vec![0u8; 8];
    sst.extend([1u8, 0, 1, 0x3D, 0xD8]);
    let file = xw::Book::new(
        record(0x00FC, &sst),
        vec![xw::Sheet::new("S", xw::label_sst(0, 0, 0, 0))],
    )
    .file();
    assert_eq!(cells_of(&file, "x.xls").unwrap()[0][0], text("\u{FFFD}"));
    // Narrow characters go on after it.
    let mut sst = vec![0u8; 8];
    sst.extend([2u8, 0, 1, 0x3D, 0xD8]);
    let mut globals = record(0x00FC, &sst);
    globals.extend(record(0x003C, &[0, b'x']));
    let file = xw::Book::new(
        globals,
        vec![xw::Sheet::new("S", xw::label_sst(0, 0, 0, 0))],
    )
    .file();
    assert_eq!(cells_of(&file, "x.xls").unwrap()[0][0], text("\u{FFFD}x"));
}

#[test]
fn a_libreoffice_xls_keeps_every_emoji_of_a_long_string() {
    let file = fixture!("emoji-libreoffice.xls");
    let table = cells_of(file, "x.xls").unwrap();
    let Cell::Text(even) = &table[1][1] else {
        panic!("{:?}", table[1][1]);
    };
    assert_eq!(even.matches('\u{1F600}').count(), 4000);
    assert_eq!(even.matches('\u{FFFD}').count(), 0);
    let Cell::Text(odd) = &table[2][1] else {
        panic!("{:?}", table[2][1]);
    };
    assert_eq!(odd.matches('\u{1F600}').count(), 4100);
    assert_eq!(odd.matches('\u{FFFD}').count(), 0);
}

// Dates and numbers.

#[test]
fn a_whole_number_in_a_date_format_of_an_xlsb_is_a_date() {
    use xb::Value::*;
    let records = vec![
        xb::row(0),
        xb::cell(0, 0, &Text("day".into())),
        xb::row(1),
        xb::cell(0, 1, &RkInt(46082, false)),
    ];
    let file = xb::workbook(
        &[("S", Some(xb::sheet(&records)))],
        &[],
        &[],
        &[0, 14],
        false,
    );
    assert_eq!(cells_of(&file, "x.xlsb").unwrap()[1][0], date(2026, 3, 1));
}

#[test]
fn an_xlsx_iso_date_is_a_date() {
    let cells = r#"<row r="1"><c r="A1" t="inlineStr"><is><t>h</t></is></c><c r="B1" t="inlineStr"><is><t>i</t></is></c><c r="C1" t="inlineStr"><is><t>j</t></is></c><c r="D1" t="inlineStr"><is><t>k</t></is></c></row><row r="2"><c r="A2" t="d"><v>2026-03-01T00:00:00</v></c><c r="B2" t="d"><v>2026-03-01T12:30:00Z</v></c><c r="C2" t="d"><v>06:00:00</v></c><c r="D2" t="d"><v>soon</v></c></row>"#;
    let file = xlsx_sheet("s.xml", "xl/s.xml", cells);
    let got = cells_of(&file, "x.xlsx").unwrap();
    let at = |day: u32, hour: u32, minute: u32| {
        Cell::DateTime(
            chrono::NaiveDate::from_ymd_opt(
                if day == 31 { 1899 } else { 2026 },
                if day == 31 { 12 } else { 3 },
                day,
            )
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap(),
        )
    };
    assert_eq!(
        got[1],
        [date(2026, 3, 1), at(1, 12, 30), at(31, 6, 0), text("soon")]
    );
    let got = cells_of(fixture!("iso-openpyxl.xlsx"), "x.xlsx").unwrap();
    assert_eq!(got[1][0], date(2026, 3, 1));
}

#[test]
fn numbers_a_date_cannot_stand_for_stay_numbers() {
    assert!(matches!(excel_date(f64::NAN, false), Cell::Float(n) if n.is_nan()));
    assert!(matches!(excel_date(f64::NAN, true), Cell::Float(n) if n.is_nan()));
    assert!(matches!(excel_date(f64::NEG_INFINITY, true), Cell::Float(n) if n.is_infinite()));
    // The last day a workbook can show is 9999-12-31.
    assert_eq!(excel_date(2_958_465.0, false), date(9999, 12, 31));
    assert_eq!(excel_date(2_958_466.0, false), Cell::Float(2_958_466.0));
    assert_eq!(excel_date(9e7, false), Cell::Float(9e7));
    assert_eq!(excel_date(2_957_003.0, true), date(9999, 12, 31));
    assert_eq!(excel_date(2_957_004.0, true), Cell::Float(2_957_004.0));
}

#[test]
fn the_day_that_never_was_is_only_counted_in_the_1900_system() {
    assert_eq!(excel_date(60.0, false), date(1900, 2, 28));
    assert_eq!(excel_date(59.0, false), date(1900, 2, 28));
    // In the 1904 system days count back one by one.
    assert_eq!(excel_date(-1402.0, true), date(1900, 2, 28));
    assert_eq!(excel_date(-1403.0, true), date(1900, 2, 27));
    assert_eq!(excel_date(-1.0, true), date(1903, 12, 31));
}

#[test]
fn a_number_cell_that_is_not_finite_is_no_number() {
    for word in ["NaN", "inf", "-inf", "infinity", "1e999"] {
        let cells = format!(
            r#"<row r="1"><c r="A1" t="inlineStr"><is><t>h</t></is></c></row><row r="2"><c r="A2" t="n"><v>{word}</v></c></row>"#
        );
        let file = xlsx_sheet("s.xml", "xl/s.xml", &cells);
        let got = cells_of(&file, "x.xlsx");
        assert!(got.is_err(), "{word}: {got:?}");
        // Without a type it is the text it is written as.
        let cells = format!(r#"<row r="1"><c r="A1"><v>{word}</v></c></row>"#);
        let file = xlsx_sheet("s.xml", "xl/s.xml", &cells);
        assert_eq!(cells_of(&file, "x.xlsx").unwrap()[0][0], text(word));
    }
    let file = ods(
        r#"<table:table table:name="S"><table:table-row><table:table-cell office:value-type="float" office:value="NaN"/></table:table-row></table:table>"#,
    );
    let got = cells_of(&file, "x.ods");
    assert!(got.is_err(), "{got:?}");
}

#[test]
fn a_binary_number_that_is_not_finite_is_refused() {
    use xb::Value::*;
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let records = vec![xb::row(0), xb::cell(0, 0, &Real(value))];
        let file = xb::workbook(&[("S", Some(xb::sheet(&records)))], &[], &[], &[0], false);
        let got = cells_of(&file, "x.xlsb");
        assert!(got.is_err(), "XLSB {value}: {got:?}");

        let file = xw::Book::new(
            vec![],
            vec![xw::Sheet::new("S", xw::number(0, 0, 0, value))],
        )
        .file();
        let got = cells_of(&file, "x.xls");
        assert!(got.is_err(), "XLS {value}: {got:?}");
    }
    // In the 30 bits of an RK, one of many in a record.
    let nan = xw::mulrk(0, 0, &[(0, 0x7FF8_0000), (0, 0x7FF8_0000)]);
    let file = xw::Book::new(vec![], vec![xw::Sheet::new("S", nan)]).file();
    let got = cells_of(&file, "x.xls");
    assert!(got.is_err(), "XLS RK: {got:?}");
}

// Error cells.

#[test]
fn an_ods_error_cell_is_its_text() {
    let cell = |shown: &str| {
        format!(
            r#"<table:table-cell table:formula="of:=1/0" office:value-type="string" office:string-value="" calcext:value-type="error"><text:p>{shown}</text:p></table:table-cell>"#
        )
    };
    let file = ods(&format!(
        r#"<table:table table:name="S"><table:table-row>{}</table:table-row><table:table-row>{}{}{}</table:table-row></table:table>"#,
        ods_text_cell("h").repeat(3),
        cell("#DIV/0!"),
        cell("#N/A"),
        cell("Err:502"),
    ));
    let got = cells_of(&file, "x.ods").unwrap();
    assert_eq!(got[1], [text("#Div0"), text("#NA"), text("Err:502")]);
}

#[test]
fn a_libreoffice_ods_gives_the_errors_its_xlsx_gives() {
    let errors = |dump: String| -> Vec<String> {
        dump.lines()
            .filter(|line| line.contains("Text(\"#"))
            .map(str::to_owned)
            .collect()
    };
    let ods = errors(crate::import::workbook::parity_tests::dump(fixture!(
        "types-libreoffice.ods"
    )));
    let xlsx = errors(crate::import::workbook::parity_tests::dump(fixture!(
        "types-libreoffice.xlsx"
    )));
    assert_eq!(ods, xlsx);
    assert!(!ods.is_empty());
}

// Valid packages.

#[test]
fn valid_packages_are_read_wherever_their_parts_are() {
    let expected = [
        vec![text("head"), text("7")],
        vec![text("x"), Cell::Float(8.0)],
    ];
    let files: [(&str, &[u8]); 8] = [
        ("plain", fixture!("valid-plain.xlsx")),
        ("target with dots", fixture!("valid-dots.xlsx")),
        ("target with an entity", fixture!("valid-entity.xlsx")),
        ("cell with extensions", fixture!("valid-extlst.xlsx")),
        ("macro sheet", fixture!("valid-macro.xlsm")),
        (
            "shared strings elsewhere",
            fixture!("valid-sst-elsewhere.xlsx"),
        ),
        ("workbook elsewhere", fixture!("valid-book-elsewhere.xlsx")),
        (
            "workbook target with a dot",
            fixture!("valid-dot-root.xlsx"),
        ),
    ];
    for (label, file) in files {
        let got = cells_of(file, "x.xlsx");
        assert_eq!(got.as_deref(), Ok(&expected[..]), "{label}");
    }
}

#[test]
fn a_cell_with_an_unknown_element_after_its_value_keeps_the_value() {
    let cells = r#"<row r="1"><c r="A1"><v>7</v><extLst><ext uri="x"><v>9</v></ext></extLst></c><c r="B1"><foo/><v>8</v></c></row>"#;
    let file = xlsx_sheet("s.xml", "xl/s.xml", cells);
    assert_eq!(
        cells_of(&file, "x.xlsx").unwrap()[0],
        [text("7"), text("8")]
    );
}

#[test]
fn relationship_targets_are_read_as_relative_references() {
    for (target, part) in [
        ("../xl/s.xml", "xl/s.xml"),
        ("./s.xml", "xl/s.xml"),
        ("/xl/s.xml", "xl/s.xml"),
        ("a/../s.xml", "xl/s.xml"),
        ("../../../s.xml", "s.xml"),
        ("a&amp;b.xml", "xl/a&b.xml"),
        ("sheet%201.xml", "xl/sheet%201.xml"),
        ("sheet%201.xml", "xl/sheet 1.xml"),
        ("A\\S.XML", "xl/a/s.xml"),
    ] {
        let file = xlsx_sheet(target, part, ONE);
        assert_eq!(
            cells_of(&file, "x.xlsx").map(|table| table[0].clone()),
            Ok(vec![text("7")]),
            "{target} for {part}"
        );
    }
}

/// A workbook whose styles live under another name: the date is lost without
/// the relationship that finds them.
#[test]
fn styles_are_found_through_the_workbook_s_relationships() {
    let rels = format!(
        r#"<Relationships><Relationship Id="r" Type="{WS}" Target="s.xml"/><Relationship Id="t" Type="{REL}/styles" Target="theme/looks.xml"/></Relationships>"#
    );
    let styles =
        r#"<styleSheet><cellXfs><xf numFmtId="0"/><xf numFmtId="14"/></cellXfs></styleSheet>"#;
    let sheet = r#"<worksheet><sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="2"><c r="A2" s="1"><v>46082</v></c></row></sheetData></worksheet>"#;
    let file = xlsx_with(
        &rels,
        ONE_SHEET,
        &[
            ("xl/s.xml", sheet.as_bytes()),
            ("xl/theme/looks.xml", styles.as_bytes()),
        ],
    );
    assert_eq!(cells_of(&file, "x.xlsx").unwrap()[1][0], date(2026, 3, 1));
}

#[test]
fn an_excel_macro_sheet_has_no_cells_and_keeps_the_workbook_readable() {
    let rels = format!(
        r#"<Relationships><Relationship Id="m" Type="http://schemas.microsoft.com/office/2006/relationships/xlMacrosheet" Target="macrosheets/m.xml"/><Relationship Id="i" Type="http://schemas.microsoft.com/office/2006/relationships/xlIntlMacrosheet" Target="macrosheets/i.xml"/><Relationship Id="r" Type="{WS}" Target="s.xml"/></Relationships>"#
    );
    let book = r#"<workbook><sheets><sheet name="m" r:id="m"/><sheet name="i" r:id="i"/><sheet name="s" r:id="r"/></sheets></workbook>"#;
    let sheet = format!("<worksheet><sheetData>{ONE}</sheetData></worksheet>");
    let file = xlsx_with(&rels, book, &[("xl/s.xml", sheet.as_bytes())]);
    let options = |sheet| ReadOptions {
        sheet: Some(sheet),
        ..opts(1 << 28)
    };
    let table = read_table(&file, "x.xlsm", &options("s")).unwrap();
    assert_eq!(table.headers, ["7"]);
    // Its part is not read, and there is none here: an empty sheet.
    let result = read_table(&file, "x.xlsm", &options("m"));
    assert!(refused_with(&result, "no header row"), "{result:?}");
}

#[test]
fn an_xlsb_macro_or_chart_sheet_has_no_cells_and_keeps_the_workbook_readable() {
    let rels = format!(
        r#"<Relationships><Relationship Id="m" Type="http://schemas.microsoft.com/office/2006/relationships/xlMacrosheet" Target="macrosheets/m.bin"/><Relationship Id="c" Type="{REL}/chartsheet" Target="chartsheets/c.bin"/><Relationship Id="r" Type="{WS}" Target="worksheets/s.bin"/></Relationships>"#
    );
    let records = vec![
        xlsb_sheet_record("m", "m"),
        xlsb_sheet_record("c", "c"),
        xlsb_sheet_record("r", "s"),
        (0x90, vec![]),
    ];
    let sheet = xb::sheet(&[xb::row(0), xb::cell(0, 0, &xb::Value::RkInt(7, false))]);
    let root = xlsb_root();
    let file = zip_parts(
        &[
            ("_rels/.rels", root.as_bytes()),
            ("xl/_rels/workbook.bin.rels", rels.as_bytes()),
            ("xl/workbook.bin", &xb::part(&records)),
            ("xl/worksheets/s.bin", &sheet),
        ],
        false,
    );
    let options = |sheet| ReadOptions {
        sheet: Some(sheet),
        ..opts(1 << 28)
    };
    let table = read_table(&file, "x.xlsb", &options("s")).unwrap();
    assert_eq!(table.headers, ["7"]);
    for sheet in ["m", "c"] {
        let result = read_table(&file, "x.xlsb", &options(sheet));
        assert!(
            refused_with(&result, "no header row"),
            "{sheet}: {result:?}"
        );
    }
}

#[test]
fn an_xlsb_workbook_is_found_through_the_package_s_relationships() {
    let root = format!(
        r#"<Relationships><Relationship Id="rId1" Type="{REL}/officeDocument" Target="bin/book.bin"/></Relationships>"#
    );
    let rels = format!(
        r#"<Relationships><Relationship Id="r" Type="{WS}" Target="../sheets/s.bin"/><Relationship Id="t" Type="{REL}/sharedStrings" Target="words.bin"/><Relationship Id="y" Type="{REL}/styles" Target="looks.bin"/></Relationships>"#
    );
    let records = vec![xlsb_sheet_record("r", "s"), (0x90, vec![])];
    let strings = xb::part(&[
        (0x9F, vec![1, 0, 0, 0, 1, 0, 0, 0]),
        (0x13, [vec![0], xb::wide("word")].concat()),
        (0xA0, vec![]),
    ]);
    let styles = xb::part(&[
        (0x267, 0u32.to_le_bytes().to_vec()),
        (0x269, 2u32.to_le_bytes().to_vec()),
        (0x2F, vec![0; 16]),
        (0x2F, [vec![0, 0, 14, 0], vec![0; 12]].concat()),
    ]);
    let sheet = xb::sheet(&[
        xb::row(0),
        xb::cell(0, 0, &xb::Value::Shared(0)),
        xb::row(1),
        xb::cell(0, 1, &xb::Value::Real(46082.0)),
    ]);
    let file = zip_parts(
        &[
            ("_rels/.rels", root.as_bytes()),
            ("bin/_rels/book.bin.rels", rels.as_bytes()),
            ("bin/book.bin", &xb::part(&records)),
            ("bin/words.bin", &strings),
            ("bin/looks.bin", &styles),
            ("sheets/s.bin", &sheet),
        ],
        false,
    );
    assert_eq!(
        cells_of(&file, "x.xlsb").unwrap(),
        [vec![text("word")], vec![date(2026, 3, 1)]]
    );
}

// Text that decodes as something else.

#[test]
fn xlsb_text_that_starts_like_a_byte_order_mark_is_utf_16() {
    use xb::Value::*;
    // The bytes FE FF and EF BB BF start other encodings' marks; these are
    // the characters U+FFFE and U+BBEF, then more.
    let sheet = xb::sheet(&[
        xb::row(0),
        xb::cell(0, 0, &Text("\u{BBEF}\u{41BF}".into())),
        xb::cell(1, 0, &Text("\u{FEFF}plain".into())),
    ]);
    let file = xb::workbook(&[("S", Some(sheet))], &[], &[], &[0], false);
    let got = cells_of(&file, "x.xlsb").unwrap();
    assert_eq!(got[0], [text("\u{BBEF}\u{41BF}"), text("plain")]);
    // U+FFFE is no character, but it is not a mark that turns what follows
    // into big-endian text.
    let mut body = vec![0u8; 8];
    let units = [0xFFFEu16, 0x0041];
    body.extend_from_slice(&(units.len() as u32).to_le_bytes());
    body.extend(units.iter().flat_map(|unit| unit.to_le_bytes()));
    let sheet = xb::sheet(&[xb::row(0), (0x0006, body)]);
    let file = xb::workbook(&[("S", Some(sheet))], &[], &[], &[0], false);
    let got = cells_of(&file, "x.xlsb").unwrap();
    assert_eq!(got[0], [text("\u{FFFE}A")]);
}

#[test]
fn xml_comments_in_an_ods_row_or_among_named_expressions_are_comments() {
    let file = ods(&format!(
        r#"<table:named-expressions><!-- names --><table:named-range table:name="n" table:cell-range-address="$S.$A$1"/><?pi x?></table:named-expressions><table:table table:name="S"><table:table-row><!-- a cell -->{}<?pi x?>{}</table:table-row></table:table>"#,
        ods_text_cell("a"),
        ods_text_cell("b")
    ));
    assert_eq!(cells_of(&file, "x.ods").unwrap()[0], [text("a"), text("b")]);
}

// Memory for one element and for the elements still open.

alone!(
    one_very_long_string_is_refused_before_it_is_copied_three_times,
    peak_of_one_very_long_shared_string,
    {
        let length = 32 * MIB;
        let file = {
            let strings = format!("<sst><si><t>{}</t></si></sst>", "x".repeat(length));
            let sheet = format!("<worksheet><sheetData>{ONE}</sheetData></worksheet>");
            xlsx_with(
                &sheet_rel("s.xml"),
                ONE_SHEET,
                &[
                    ("xl/sharedStrings.xml", strings.as_bytes()),
                    ("xl/s.xml", sheet.as_bytes()),
                ],
            )
        };
        let options = ReadOptions {
            max_inflated_bytes: length as u64 + 64,
            ..opts(1 << 28)
        };
        let (result, peak) = measured(|| read_table(&file, "x.xlsx", &options));
        assert!(refused_with(&result, "longer than"), "{result:?}");
        assert!(peak < 2 * length, "held {peak} bytes at most");
    }
);

alone!(
    a_string_of_many_runs_is_held_to_the_length_of_one,
    peak_of_a_string_of_many_runs,
    {
        let run = format!("<r><t>{}</t></r>", "x".repeat(1024));
        let strings = format!("<sst><si>{}</si></sst>", run.repeat(32 * 1024));
        let sheet = format!("<worksheet><sheetData>{ONE}</sheetData></worksheet>");
        let file = xlsx_with(
            &sheet_rel("s.xml"),
            ONE_SHEET,
            &[
                ("xl/sharedStrings.xml", strings.as_bytes()),
                ("xl/s.xml", sheet.as_bytes()),
            ],
        );
        drop(strings);
        let (result, peak) = measured(|| read_table(&file, "x.xlsx", &opts(1 << 28)));
        assert!(refused_with(&result, "longer than"), "{result:?}");
        assert!(peak < 64 * MIB, "held {peak} bytes at most");
    }
);

alone!(
    shared_strings_at_the_limit_take_what_the_limit_says,
    peak_of_shared_strings_at_the_limit,
    {
        // Thirty-two MiB of strings of 1 KiB, within a limit of 34 MiB. The
        // text never has more room than the limit, so the worst is the old
        // room and the new at once while it grows.
        let limit = 34 * MIB;
        let item = format!("<si><t>{}</t></si>", "x".repeat(1020));
        let strings = format!("<sst>{}</sst>", item.repeat(32 * 1024));
        let sheet = format!("<worksheet><sheetData>{ONE}</sheetData></worksheet>");
        let file = xlsx_with(
            &sheet_rel("s.xml"),
            ONE_SHEET,
            &[
                ("xl/sharedStrings.xml", strings.as_bytes()),
                ("xl/s.xml", sheet.as_bytes()),
            ],
        );
        drop(strings);
        let options = ReadOptions {
            max_inflated_bytes: limit as u64,
            ..opts(1 << 28)
        };
        let (result, peak) = measured(|| read_table(&file, "x.xlsx", &options));
        assert_eq!(result.unwrap().headers, ["7"]);
        assert!(peak < limit * 3 / 2, "held {peak} bytes at most");
    }
);

alone!(
    elements_left_open_are_refused_before_they_cost_more_than_their_xml,
    peak_of_elements_left_open,
    {
        let count = 1_000_000;
        let sheet = format!(
            "<worksheet>{}<sheetData></sheetData></worksheet>",
            "<a>".repeat(count)
        );
        let file = xlsx_with(
            &sheet_rel("s.xml"),
            ONE_SHEET,
            &[("xl/s.xml", sheet.as_bytes())],
        );
        let (result, peak) = measured(|| read_table(&file, "x.xlsx", &opts(1 << 28)));
        assert!(refused_with(&result, "nested"), "{result:?}");
        // A little for the elements, the rest for the process around them.
        assert!(peak < 4 * MIB, "held {peak} bytes at most");
    }
);

#[test]
fn elements_nested_as_deep_as_a_real_file_are_read() {
    let open = "<a>".repeat(200);
    let close = "</a>".repeat(200);
    let sheet = format!("<worksheet>{open}{close}<sheetData>{ONE}</sheetData></worksheet>");
    let file = xlsx_with(
        &sheet_rel("s.xml"),
        ONE_SHEET,
        &[("xl/s.xml", sheet.as_bytes())],
    );
    assert_eq!(cells_of(&file, "x.xlsx").unwrap()[0][0], text("7"));
}

#[test]
fn an_element_with_a_name_too_long_to_be_one_is_refused() {
    let sheet = format!(
        "<worksheet><{0}></{0}><sheetData>{ONE}</sheetData></worksheet>",
        "a".repeat(2048)
    );
    let file = xlsx_with(
        &sheet_rel("s.xml"),
        ONE_SHEET,
        &[("xl/s.xml", sheet.as_bytes())],
    );
    let got = cells_of(&file, "x.xlsx");
    assert!(got.is_err(), "{got:?}");
}

#[test]
fn an_xlsb_record_longer_than_one_is_refused() {
    let sheet = xb::part(&[
        (0x81, vec![]),
        (0x94, vec![0; 16]),
        (0x91, vec![]),
        (0x06, vec![0; 16 * MIB + 1]),
    ]);
    let file = xb::workbook(&[("S", Some(sheet))], &[], &[], &[0], false);
    let result = read_table(&file, "x.xlsb", &opts(1 << 28));
    assert!(refused_with(&result, "longer than"), "{result:?}");
}

// Bounds that hold, and that a change must not break.

fn small_xlsx(strings: &str, stored: bool) -> Vec<u8> {
    let root = format!(
        r#"<Relationships><Relationship Id="rId1" Type="{REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let rels = sheet_rel("worksheets/s.xml");
    let sheet = br#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c></row><row r="2"><c r="A2"><v>1</v></c></row></sheetData></worksheet>"#;
    zip_parts(
        &[
            ("_rels/.rels", root.as_bytes()),
            ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
            ("xl/workbook.xml", ONE_SHEET.as_bytes()),
            ("xl/sharedStrings.xml", strings.as_bytes()),
            ("xl/worksheets/s.xml", sheet),
        ],
        stored,
    )
}

fn patch_central(mut bytes: Vec<u8>, name: &str, change: impl Fn(&mut [u8])) -> Vec<u8> {
    let mut at = 0;
    while at + 46 < bytes.len() {
        if bytes[at..at + 4] == *b"PK\x01\x02" {
            let length = u16::from_le_bytes([bytes[at + 28], bytes[at + 29]]) as usize;
            if &bytes[at + 46..at + 46 + length] == name.as_bytes() {
                change(&mut bytes[at..at + 46 + length]);
            }
        }
        at += 1;
    }
    bytes
}

#[test]
fn sizes_a_zip_file_lies_about_cannot_be_read_past() {
    let big = format!("<sst><si><t>{}</t></si></sst>", "x".repeat(1_000_000));
    for stored in [false, true] {
        let file = small_xlsx(&big, stored);
        for size in [0u32, 10, 1000] {
            let lied = patch_central(file.clone(), "xl/sharedStrings.xml", |entry| {
                entry[24..28].copy_from_slice(&size.to_le_bytes());
            });
            let result = read_table(&lied, "x.xlsx", &opts(1 << 28));
            assert!(result.is_err(), "stored {stored}, declared {size}");
        }
    }
}

alone!(
    a_part_named_twice_does_not_get_past_the_limit_of_a_part,
    peak_of_a_part_named_twice,
    {
        let root = format!(
            r#"<Relationships><Relationship Id="rId1" Type="{REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
        );
        let rels = sheet_rel("worksheets/s.xml");
        let sheet = format!("<worksheet><sheetData>{ONE}</sheetData></worksheet>");
        let bomb = format!("<sst><si><t>{}</t></si></sst>", "x".repeat(4 * MIB));
        let small = "<sst></sst>";
        for (first, second) in [(bomb.as_str(), small), (small, bomb.as_str())] {
            let mut file = zip_parts(
                &[
                    ("_rels/.rels", root.as_bytes()),
                    ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
                    ("xl/workbook.xml", ONE_SHEET.as_bytes()),
                    ("xl/worksheets/s.xml", sheet.as_bytes()),
                    ("xl/sharedStrings.xmX", first.as_bytes()),
                    ("xl/sharedStrings.xml", second.as_bytes()),
                ],
                false,
            );
            let needle = b"xl/sharedStrings.xmX";
            for at in 0..file.len() - needle.len() {
                if &file[at..at + needle.len()] == needle {
                    file[at + needle.len() - 1] = b'l';
                }
            }
            let options = ReadOptions {
                max_inflated_bytes: MIB as u64,
                ..opts(1 << 28)
            };
            let (_, peak) = measured(|| read_table(&file, "x.xlsx", &options));
            assert!(peak < 3 * MIB, "held {peak} bytes at most");
        }
    }
);

#[test]
fn many_parts_are_read_in_time_in_proportion_to_them() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for at in 0..80_000 {
        zip.start_file(format!("p/{at}"), options).unwrap();
    }
    zip.start_file("mimetype", options).unwrap();
    zip.write_all(b"nope").unwrap();
    let file = zip.finish().unwrap().into_inner();
    let at = Instant::now();
    let result = read_table(&file, "x.xlsx", &opts(1 << 28));
    assert!(result.is_err());
    assert!(at.elapsed() < Duration::from_secs(10), "{:?}", at.elapsed());
}

#[test]
fn stray_end_records_do_not_make_the_directory_read_again() {
    let count = 20_000u16;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for at in 0..count {
        zip.start_file(format!("p/{at}"), options).unwrap();
    }
    let mut file = zip.finish().unwrap().into_inner();
    let end = file.len() - 22;
    let size = u32::from_le_bytes(file[end + 12..end + 16].try_into().unwrap());
    let offset = u32::from_le_bytes(file[end + 16..end + 20].try_into().unwrap());
    let mut fake = b"PK\x05\x06\0\0\0\0".to_vec();
    fake.extend((count + 1).to_le_bytes());
    fake.extend((count + 1).to_le_bytes());
    fake.extend(size.to_le_bytes());
    fake.extend(offset.to_le_bytes());
    fake.extend([0, 0]);
    let comment = fake.repeat(2_000);
    file[end + 20..end + 22].copy_from_slice(&(comment.len() as u16).to_le_bytes());
    file.extend(&comment);
    let at = Instant::now();
    let _ = read_table(&file, "x.xlsx", &opts(1 << 28));
    assert!(at.elapsed() < Duration::from_secs(10), "{:?}", at.elapsed());
}

#[test]
fn compound_file_tricks_are_refused_quickly() {
    let long = xw::Book::new(
        vec![],
        vec![xw::Sheet::new("S", xw::number(0, 0, 0, 1.0).repeat(500))],
    );
    let mut cases: Vec<(&str, Vec<u8>)> = vec![];
    {
        let mut file = long.compound();
        let first = file.chains[0][0];
        let last = *file.chains[0].last().unwrap();
        file.set_fat(last, first);
        cases.push(("a stream's chain loops", file.bytes));
    }
    {
        let mut file = long.compound();
        let at = file.directory[0];
        file.set_fat(at, at);
        cases.push(("the directory's chain loops", file.bytes));
    }
    {
        let mut file = long.compound();
        file.set_header(48, 0xFFFF_FFF0);
        cases.push(("the directory is nowhere", file.bytes));
    }
    {
        let mut file = long.compound();
        file.set_header(68, 0);
        cases.push(("the DIFAT starts in the FAT", file.bytes));
    }
    {
        let mut bytes = long.compound().bytes;
        bytes.truncate(600);
        cases.push(("the file is cut short", bytes));
    }
    for (label, bytes) in cases {
        let at = Instant::now();
        let _ = read_table(&bytes, "x.xls", &opts(1 << 28));
        assert!(at.elapsed() < Duration::from_secs(2), "{label}");
    }
}

alone!(
    a_table_of_only_empty_shared_strings_costs_about_its_stream,
    peak_of_empty_shared_strings,
    {
        let mut sst = vec![0u8; 8];
        sst.extend([0u8, 0, 0].repeat(2700));
        let mut stream = xls_bof();
        stream.extend(record(0x00FC, &sst));
        for _ in 0..100_000 {
            stream.extend(record(0x003C, &[]));
        }
        stream.extend(record(0x000A, &[]));
        let file = compound(&[("Workbook", &stream)]).bytes;
        let (_, peak) = measured(|| read_table(&file, "x.xls", &opts(0)));
        // The stream, and four bytes for each string, in a list that doubles.
        assert!(peak < file.len() * 8, "held {peak} bytes at most");
    }
);

// Files mutated at random, within the limits of a table.

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, count: usize) -> usize {
        (self.next() % count as u64) as usize
    }
}

const TOKENS: &[&[u8]] = &[
    b"999999999999",
    b"-1",
    b"0",
    b"4294967295",
    b"NaN",
    b"&#0;",
    b"&#x110000;",
    b"\xff\xfe",
    b"\"",
    b"'",
    b"<",
    b">",
    b"/>",
    b"</",
    b"<c>",
    b"</row>",
    b"<row>",
    b"XFD1048576",
    b"AAAAAAAA1",
    b"table:number-columns-repeated=\"18446744073709551615\"",
    b"table:number-rows-repeated=\"18446744073709551615\"",
    b"<text:s text:c=\"100000\"/>",
    b"t=\"s\"",
    b"t=\"d\"",
    b"_x00",
    b"_x00\xc3\xa9_",
    b"<![CDATA[",
    b"]]>",
    b"<!--",
    b"-->",
    b"&",
    b"\xc3",
    b"../",
    b"calcext:value-type=\"error\"",
];

fn mutate(bytes: &mut Vec<u8>, rng: &mut Rng) {
    for _ in 0..1 + rng.below(4) {
        if bytes.is_empty() {
            return;
        }
        let at = rng.below(bytes.len());
        match rng.below(4) {
            0 => bytes[at] = rng.next() as u8,
            1 => {
                let token = TOKENS[rng.below(TOKENS.len())];
                bytes.splice(at..at, token.iter().copied());
            }
            2 => {
                let end = (at + rng.below(64)).min(bytes.len());
                bytes.drain(at..end);
            }
            _ => {
                let token = TOKENS[rng.below(TOKENS.len())];
                let end = (at + token.len()).min(bytes.len());
                bytes.splice(at..end, token.iter().copied());
            }
        }
    }
}

fn parts_of(file: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(file)).unwrap();
    let mut parts = vec![];
    for at in 0..zip.len() {
        let mut part = zip.by_index(at).unwrap();
        let mut bytes = vec![];
        std::io::Read::read_to_end(&mut part, &mut bytes).unwrap();
        parts.push((part.name().unwrap().into_owned(), bytes));
    }
    parts
}

#[test]
fn mutated_files_are_read_within_the_limits_or_refused() {
    let mut seeds: Vec<Vec<(String, Vec<u8>)>> = [
        fixture!("types-openpyxl.xlsx"),
        fixture!("types-libreoffice.xlsx"),
        fixture!("types-libreoffice.ods"),
        fixture!("valid-macro.xlsm"),
        fixture!("valid-sst-elsewhere.xlsx"),
    ]
    .iter()
    .map(|file| parts_of(file))
    .collect();
    seeds.extend(xb::samples().iter().map(|(_, file)| parts_of(file)));
    let records = [
        xw::label(0, 0, 0, "name", false),
        xw::label(0, 1, 0, "n", false),
        xw::rk(1, 0, 1, xw::rk_int(5, false)),
        xw::mulrk(
            1,
            1,
            &[(0, xw::rk_int(5, true)), (1, xw::rk_float(1.5, false))],
        ),
        xw::boolerr(2, 0, 0, 7, true),
        xw::label_sst(2, 1, 0, 0),
        xw::formula(3, 0, 0, xw::formula_text()),
        xw::string_record("s", false),
        xw::number(3, 1, 1, 46082.5),
    ]
    .concat();
    let mut globals = xw::format(164, "yyyy", false);
    globals.extend(xw::xf(0));
    globals.extend(xw::xf(164));
    globals.extend(xw::datemode(true));
    let xls = [
        xw::Book::new(globals.clone(), vec![xw::Sheet::new("S", records.clone())]).file(),
        xw::Book::new(globals, vec![xw::Sheet::new("S", records.repeat(60))]).file(),
    ];
    let options = ReadOptions {
        max_rows: 1_000,
        max_columns: 256,
        max_cells: 50_000,
        max_inflated_bytes: 8 * MIB as u64,
        max_text_bytes: MIB as u64,
        ..ReadOptions::default()
    };
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..4_000 {
        let file = if rng.below(4) == 0 {
            let mut bytes = xls[rng.below(xls.len())].clone();
            mutate(&mut bytes, &mut rng);
            bytes
        } else {
            let seed = &seeds[rng.below(seeds.len())];
            let target = rng.below(seed.len());
            let parts: Vec<(&str, Vec<u8>)> = seed
                .iter()
                .enumerate()
                .map(|(at, (name, bytes))| {
                    let mut bytes = bytes.clone();
                    if at == target || rng.below(8) == 0 {
                        mutate(&mut bytes, &mut rng);
                    }
                    (name.as_str(), bytes)
                })
                .collect();
            let parts: Vec<(&str, &[u8])> = parts
                .iter()
                .map(|(name, bytes)| (*name, bytes.as_slice()))
                .collect();
            zip_parts(&parts, true)
        };
        if let Ok(table) = read_table(&file, "x", &options) {
            assert!(table.headers.len() <= 256);
            assert!(table.rows.len() <= 1_000);
            assert!((table.rows.len() + 1) * table.headers.len() <= 50_000);
        }
    }
}

// What a reading holds for work that gives nothing.

alone!(
    the_spaces_of_an_ods_row_that_repeats_zero_times_are_not_made,
    peak_of_ods_spaces_of_a_row_that_repeats_zero_times,
    {
        // 32 MiB of spaces in a row that gives no cell, under a limit that
        // lets them through.
        let file = ods(&format!(
            r#"<table:table table:name="S"><table:table-row table:number-rows-repeated="0">{}</table:table-row></table:table>"#,
            spaced_cell(32 * MIB)
        ));
        let (result, peak) = measured(|| read_table(&file, "x.ods", &opts(64 * MIB as u64)));
        assert!(refused_with(&result, "no header row"), "{result:?}");
        assert!(peak < 8 * MIB, "held {peak} bytes at most");
    }
);

alone!(
    the_spaces_that_end_an_ods_cell_are_not_made_before_more_whitespace,
    peak_of_ods_spaces_that_end_a_cell,
    {
        let file = ods(&format!(
            r#"<table:table table:name="S"><table:table-row>{}</table:table-row><table:table-row><table:table-cell office:value-type="string"><text:p>a<text:s text:c="{}"/> </text:p></table:table-cell></table:table-row></table:table>"#,
            ods_text_cell("h"),
            32 * MIB
        ));
        let (result, peak) = measured(|| read_table(&file, "x.ods", &opts(64 * MIB as u64)));
        assert_eq!(result.unwrap().rows[0].cells, [text("a")]);
        assert!(peak < 8 * MIB, "held {peak} bytes at most");
    }
);

// What the lists a file's own numbers make take.

alone!(
    the_dimension_of_a_sheet_takes_memory_in_proportion_to_its_bytes,
    peak_of_a_dimension_of_colons,
    {
        // 1 MiB of colons, which are pieces of a range one after the other.
        let sheet = format!(
            r#"<worksheet><dimension ref="{}"/><sheetData/></worksheet>"#,
            ":".repeat(MIB)
        );
        let file = xlsx_with(
            &sheet_rel("s.xml"),
            ONE_SHEET,
            &[("xl/s.xml", sheet.as_bytes())],
        );
        let (result, peak) = measured(|| read_table(&file, "x.xlsx", &opts(1 << 28)));
        assert!(refused_with(&result, "is not a range"), "{result:?}");
        assert!(peak < 6 * MIB, "held {peak} bytes at most");
    }
);

alone!(
    a_relationship_target_of_many_folders_takes_memory_in_proportion_to_its_bytes,
    peak_of_a_target_of_many_folders,
    {
        // A target of 1 MiB, longer than any part's name, of folders one
        // inside the other.
        let rels = format!(
            r#"<Relationships><Relationship Id="r" Type="{WS}" Target="s.xml"/><Relationship Id="x" Type="y" Target="{}"/></Relationships>"#,
            "a/".repeat(MIB / 2)
        );
        let file = xlsx_with(
            &rels,
            ONE_SHEET,
            &[("xl/s.xml", br#"<worksheet><sheetData/></worksheet>"#)],
        );
        let (result, peak) = measured(|| read_table(&file, "x.xlsx", &opts(1 << 28)));
        assert!(refused_with(&result, "no header row"), "{result:?}");
        assert!(peak < 6 * MIB, "held {peak} bytes at most");
    }
);

#[test]
fn a_relationship_target_is_resolved_up_to_the_longest_name_a_part_has() {
    // Folders that are nothing (`./`), then the sheet: exactly as long as the
    // longest name a part has, and one folder more.
    let sheet = format!("<worksheet><sheetData>{ONE}</sheetData></worksheet>");
    for (folders, read) in [(32_765, true), (32_766, false)] {
        let target = format!("{}s.xml", "./".repeat(folders));
        assert_eq!(target.len(), 2 * folders + 5);
        let file = xlsx_with(
            &sheet_rel(&target),
            ONE_SHEET,
            &[("xl/s.xml", sheet.as_bytes())],
        );
        let result = read_table(&file, "x.xlsx", &opts(1 << 28));
        if read {
            assert_eq!(result.unwrap().headers, ["7"]);
        } else {
            assert!(refused_with(&result, "is missing"), "{result:?}");
        }
    }
}

/// The name of the column `at` (from 0) as a sheet writes it.
fn column_name(mut at: usize) -> String {
    let mut name = Vec::new();
    loop {
        name.push(b'A' + (at % 26) as u8);
        if at < 26 {
            break;
        }
        at = at / 26 - 1;
    }
    name.reverse();
    String::from_utf8(name).unwrap()
}

alone!(
    a_header_of_one_shared_string_many_times_takes_two_copies_of_it,
    peak_of_a_header_of_one_shared_string,
    {
        // 1,023 cells that name one string of 16 KiB, under a text limit of
        // 16 MiB: the cells of the header and the names made of them.
        let limit = 16 * MIB;
        let text = "h".repeat(16 * 1024);
        let width = limit / text.len() - 1;
        let strings = format!("<sst><si><t>{text}</t></si></sst>");
        let mut cells = String::new();
        for at in 0..width {
            cells.push_str(&format!(
                r#"<c r="{}1" t="s"><v>0</v></c>"#,
                column_name(at)
            ));
        }
        let sheet =
            format!("<worksheet><sheetData><row r=\"1\">{cells}</row></sheetData></worksheet>");
        let file = xlsx_with(
            &sheet_rel("s.xml"),
            ONE_SHEET,
            &[
                ("xl/sharedStrings.xml", strings.as_bytes()),
                ("xl/s.xml", sheet.as_bytes()),
            ],
        );
        let (result, peak) = measured(|| read_table(&file, "x.xlsx", &opts(limit as u64)));
        assert_eq!(result.unwrap().headers.len(), width);
        assert!(peak < limit * 5 / 2, "held {peak} bytes at most");
    }
);

alone!(
    a_sheet_out_of_order_takes_about_sixty_bytes_for_each_cell,
    peak_of_a_sheet_out_of_order,
    {
        // 1,000,000 cells, the rows last first: kept as sparse cells until the
        // sheet ends.
        let (rows, width) = (10_000, 100);
        let mut sheet = String::from("<worksheet><sheetData>");
        for row in (0..rows).rev() {
            sheet.push_str(&format!(r#"<row r="{}">"#, row + 1));
            for at in 0..width {
                sheet.push_str(&format!(
                    r#"<c r="{}{}"><v>1</v></c>"#,
                    column_name(at),
                    row + 1
                ));
            }
            sheet.push_str("</row>");
        }
        sheet.push_str("</sheetData></worksheet>");
        let file = xlsx_with(
            &sheet_rel("s.xml"),
            ONE_SHEET,
            &[("xl/s.xml", sheet.as_bytes())],
        );
        let (result, peak) = measured(|| read_table(&file, "x.xlsx", &opts(1 << 28)));
        assert_eq!(result.unwrap().rows.len(), rows - 1);
        let each = peak / (rows * width);
        assert!(each < 70, "{each} bytes for each cell");
    }
);

alone!(
    what_opening_holds_is_within_twice_what_it_is_charged,
    peak_of_opening_relationships_and_strings,
    {
        // Half of the budget in relationships, the rest in empty shared
        // strings, the shapes that take the most beyond what they are charged.
        let limit = 32 * MIB;
        let count = limit / 2 / (64 + 8);
        let mut extra = String::new();
        for at in 0..count {
            extra.push_str(&format!(r#"<Relationship Id="{at:08x}" Target=""/>"#));
        }
        let rels = format!(
            r#"<Relationships><Relationship Id="r" Type="{WS}" Target="s.xml"/>{extra}</Relationships>"#
        );
        let strings = format!("<sst>{}</sst>", "<si/>".repeat(limit * 47 / 100 / 4));
        let file = xlsx_with(
            &rels,
            ONE_SHEET,
            &[
                ("xl/sharedStrings.xml", strings.as_bytes()),
                ("xl/s.xml", br#"<worksheet><sheetData/></worksheet>"#),
            ],
        );
        let base = kib_of("VmRSS:") as usize * 1024;
        let (book, peak) = measured(|| {
            Workbook::open(
                &file,
                Limits {
                    max_part_bytes: limit as u64,
                    max_text_bytes: 1 << 28,
                },
            )
        });
        let held = (kib_of("VmRSS:") as usize * 1024).saturating_sub(base);
        assert!(book.is_ok());
        assert!(held < limit * 2 && peak < limit * 5 / 2, "{held} {peak}");
    }
);
