//! Reading a workbook is stoppable and bounded in every loop, whatever comes
//! out of it: a file that makes work and gives no cell (rows that repeat zero
//! times, cells in no column, empty records) neither makes the text it
//! describes nor keeps the reading from stopping when nobody waits for it.
//!
//! Whether a reading was stopped does not depend on how fast the machine is: the flag is
//! raised by another thread, and a reading counts only if the flag was up while it ran
//! (see [`stopped`]). The times that remain are wall-clock times with wide margins: what a
//! reading that ignores the stop costs here is seconds, what one that heeds it costs is
//! milliseconds.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::opening_tests::{MIB, REL, ods, ods_text_cell, opts, refused_with, text, zip_parts};
use super::*;
use crate::import::workbook::xlsb_writer as xb;
use crate::import::xls::write::{self as xw, record};

/// `read_table`, and how long it took.
fn timed(
    file: &[u8],
    name: &str,
    options: ReadOptions<'_>,
) -> (Result<Table, SheetsError>, Duration) {
    let at = Instant::now();
    let result = read_table(file, name, &options);
    (result, at.elapsed())
}

/// How many times a sheet is read to see it stopped, when the flag only went up after the
/// reading had ended: a thread that waits its turn on a busy machine may be late.
const TRIES: usize = 50;

/// How many readings that ended with the flag up and no stop (they need not be in a row) are
/// the failure. One can be the flag going up between the reading's last look at it and its
/// end; three are a reading that ignores it.
const ENDS_WITH_FLAG_UP: usize = 3;

/// The first sheet of the workbook `file` read into a table, with the stop flag raised by
/// another thread `after` it is running, and how long the reading took to end once the flag
/// was up. Opening a workbook cannot be interrupted, so the clock starts after it: what is
/// measured is the sheet.
///
/// A reading is only told to stop if it is still running, so one that ended before the thread
/// raised the flag has nothing to say about stopping, whatever the load on the machine did to
/// the thread: it is read again, at most [`TRIES`] times. What the caller sees is a reading
/// that was stopped, or the [`ENDS_WITH_FLAG_UP`]th one that ended with the flag up and no
/// stop, which is the reading ignoring it. Its time is how long it ran after the flag went up.
fn stopped(
    file: &[u8],
    options: ReadOptions<'_>,
    after: Duration,
) -> (Result<Table, SheetsError>, Duration) {
    let book = Workbook::open(
        file,
        Limits {
            max_part_bytes: options.max_inflated_bytes,
            max_text_bytes: options.max_text_bytes,
        },
    )
    .unwrap();
    let mut ended_with_the_flag_up = 0;
    for _ in 0..TRIES {
        let flag = AtomicBool::new(false);
        let raised_at = OnceLock::new();
        let (running, hears_it) = mpsc::channel();
        let (result, ended_at, flag_up) = std::thread::scope(|scope| {
            scope.spawn(|| {
                running.send(()).unwrap();
                std::thread::sleep(after);
                flag.store(true, Ordering::Relaxed);
                raised_at.set(Instant::now()).unwrap();
            });
            // The raising thread runs before the reading starts, so the wait is the one asked
            // for and not the time the thread took to start.
            hears_it.recv().unwrap();
            let options = ReadOptions {
                stop: Some(&flag),
                ..options.clone()
            };
            let result = tabulate_cells(&book, 0, &options).and_then(Tabulator::finish);
            let ended_at = Instant::now();
            (result, ended_at, flag.load(Ordering::Relaxed))
        });
        // How long the reading took to end once the flag was up (nothing, if it was not).
        let heeded_in = || {
            raised_at.get().map_or(Duration::ZERO, |raised| {
                ended_at.saturating_duration_since(*raised)
            })
        };
        if interrupted(&result) {
            return (result, heeded_in());
        }
        if flag_up {
            ended_with_the_flag_up += 1;
            if ended_with_the_flag_up == ENDS_WITH_FLAG_UP {
                return (result, heeded_in());
            }
        }
    }
    panic!("the flag never went up while the sheet was being read, in {TRIES} readings");
}

fn interrupted(result: &Result<Table, SheetsError>) -> bool {
    matches!(result, Err(SheetsError::Io(error)) if error.kind() == std::io::ErrorKind::Interrupted)
}

/// The raise comes soon after the reading starts, and what a file this
/// size holds takes far longer to read, even in a release build.
const SOON: Duration = Duration::from_millis(5);

/// However long the reading takes to heed the stop: the margin is wide
/// because the machine is busy, and the floods take many times it when the
/// stop is ignored.
const PROMPT: Duration = Duration::from_secs(3);

fn ods_stored(content: &str) -> Vec<u8> {
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
        true,
    )
}

fn table_of(rows: &str) -> String {
    format!(r#"<table:table table:name="S">{rows}</table:table>"#)
}

fn xlsx_stored(sheet_data: &str) -> Vec<u8> {
    let root = format!(
        r#"<Relationships><Relationship Id="rId1" Type="{REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let rels = format!(
        r#"<Relationships><Relationship Id="r" Type="{REL}/worksheet" Target="s.xml"/></Relationships>"#
    );
    let book = r#"<workbook><sheets><sheet name="s" r:id="r"/></sheets></workbook>"#;
    let sheet = format!("<worksheet><sheetData>{sheet_data}</sheetData></worksheet>");
    zip_parts(
        &[
            ("_rels/.rels", root.as_bytes()),
            ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
            ("xl/workbook.xml", book.as_bytes()),
            ("xl/s.xml", sheet.as_bytes()),
        ],
        true,
    )
}

// A reading that is told to stop, in each format, in the middle of a sheet
// that gives it no cell to stop at.

const FLOOD: usize = 24 * MIB;

#[test]
fn an_xlsx_sheet_of_cells_without_values_stops_when_told_to() {
    let file = xlsx_stored(&format!("<row>{}</row>", "<c/>".repeat(FLOOD / 4)));
    let (result, took) = stopped(&file, opts(1 << 28), SOON);
    assert!(
        interrupted(&result),
        "{result:?} {took:?} after the flag went up"
    );
    eprintln!("ended {took:?} after the flag went up");
    assert!(took < PROMPT, "{took:?}");
}

#[test]
fn an_xlsx_cell_with_a_flood_inside_it_stops_when_told_to() {
    let cell = format!(
        r#"<row r="1"><c r="A1"><extLst>{}</extLst></c></row>"#,
        "<a/>".repeat(FLOOD / 4)
    );
    let file = xlsx_stored(&cell);
    let (result, took) = stopped(&file, opts(1 << 28), SOON);
    assert!(
        interrupted(&result),
        "{result:?} {took:?} after the flag went up"
    );
    eprintln!("ended {took:?} after the flag went up");
    assert!(took < PROMPT, "{took:?}");
}

#[test]
fn an_xlsb_sheet_of_records_without_cells_stops_when_told_to() {
    let mut sheet = xb::part(&[
        (0x81, vec![]),
        (0x93, vec![0; 19]),
        (0x94, vec![0; 16]),
        (0x91, vec![]),
    ]);
    // A record of a type no cell has, without a body.
    sheet.extend(std::iter::repeat_n([0x7Fu8, 0], FLOOD / 3).flatten());
    sheet.extend(xb::part(&[(0x92, vec![]), (0x82, vec![])]));
    let file = xb::workbook(&[("S", Some(sheet))], &[], &[], &[], false);
    let (result, took) = stopped(&file, opts(1 << 28), SOON);
    assert!(
        interrupted(&result),
        "{result:?} {took:?} after the flag went up"
    );
    eprintln!("ended {took:?} after the flag went up");
    assert!(took < PROMPT, "{took:?}");
}

#[test]
fn an_ods_sheet_of_rows_without_values_stops_when_told_to() {
    let row = "<table:table-row><table:table-cell/></table:table-row>";
    let file = ods_stored(&table_of(&row.repeat(FLOOD / row.len())));
    let (result, took) = stopped(&file, opts(1 << 28), SOON);
    assert!(
        interrupted(&result),
        "{result:?} {took:?} after the flag went up"
    );
    eprintln!("ended {took:?} after the flag went up");
    assert!(took < PROMPT, "{took:?}");
}

#[test]
fn the_rest_of_an_ods_sheet_after_a_limit_stops_when_told_to() {
    let row = r#"<table:table-row><table:table-cell office:value-type="float" office:value="1"/></table:table-row>"#;
    let file = ods_stored(&table_of(&row.repeat(FLOOD / row.len())));
    let options = ReadOptions {
        max_rows: 2,
        ..opts(1 << 28)
    };
    let (result, took) = stopped(&file, options, SOON);
    assert!(
        interrupted(&result),
        "{result:?} {took:?} after the flag went up"
    );
    eprintln!("ended {took:?} after the flag went up");
    assert!(took < PROMPT, "{took:?}");
}

#[test]
fn the_rest_of_an_xlsx_sheet_after_a_limit_stops_when_told_to() {
    let row = r#"<row><c><v>1</v></c></row>"#;
    let file = xlsx_stored(&row.repeat(FLOOD / row.len()));
    let options = ReadOptions {
        max_rows: 2,
        ..opts(1 << 28)
    };
    let (result, took) = stopped(&file, options, SOON);
    assert!(
        interrupted(&result),
        "{result:?} {took:?} after the flag went up"
    );
    eprintln!("ended {took:?} after the flag went up");
    assert!(took < PROMPT, "{took:?}");
}

#[test]
fn an_xls_sheet_of_records_without_cells_stops_when_told_to() {
    // 1.5 million records of a type no cell has: the most a compound file the
    // test writer makes can hold.
    let records = record(0x007F, &[]).repeat(1_500_000);
    let file = xw::Book::new(vec![], vec![xw::Sheet::new("S", records)]).file();
    let (result, took) = stopped(&file, opts(1 << 28), Duration::ZERO);
    assert!(
        interrupted(&result),
        "{result:?} {took:?} after the flag went up"
    );
    eprintln!("ended {took:?} after the flag went up");
    assert!(took < PROMPT, "{took:?}");
}

#[test]
fn an_xls_sheet_of_continue_records_stops_when_told_to() {
    // One record followed by 1.5 million CONTINUE records: one step of the
    // records' iterator takes them all.
    let mut records = record(0x007F, &[]);
    records.extend(record(0x003C, &[]).repeat(1_500_000));
    let file = xw::Book::new(vec![], vec![xw::Sheet::new("S", records)]).file();
    let (result, took) = stopped(&file, opts(1 << 28), Duration::ZERO);
    assert!(
        interrupted(&result),
        "{result:?} {took:?} after the flag went up"
    );
    eprintln!("ended {took:?} after the flag went up");
    assert!(took < PROMPT, "{took:?}");
}

// A timeout that drops the reading leaves nothing running.

#[test]
fn a_timeout_that_drops_a_reading_leaves_the_blocking_pool_free() {
    let file = bytes::Bytes::from(xlsx_stored(&format!(
        "<row>{}</row>",
        "<c/>".repeat(FLOOD / 4)
    )));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap();
    let outcome = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_millis(50),
            read_table_blocking(file, "x.xlsx".to_owned(), &opts(1 << 28)),
        )
        .await
    });
    assert!(outcome.is_err(), "the reading ended before the timeout");
    // Dropping a runtime waits for the tasks on its blocking pool: this is
    // what a graceful shutdown waits for.
    let at = Instant::now();
    drop(runtime);
    let waited = at.elapsed();
    eprintln!("shutdown waited {waited:?} for the reading");
    assert!(
        waited < PROMPT,
        "shutdown waited {waited:?} for the reading"
    );
}

// Work that gives nothing costs nothing.

/// A cell of text with `spaces` spaces in it.
fn spaced(attributes: &str, spaces: usize) -> String {
    format!(
        r#"<table:table-cell office:value-type="string"{attributes}><text:p>a<text:s text:c="{spaces}"/>b</text:p></table:table-cell>"#
    )
}

/// `rows` rows that each describe a cell of `spaces` spaces and give no cell,
/// in a file that is read with a text limit that lets one such cell through:
/// making each of them is the cost, 1.2 GiB of spaces in all.
fn ghosts_cost_nothing(row: impl Fn(usize) -> String, rows: usize) {
    let spaces = 4 * MIB - 64;
    let file = ods(&table_of(&row(spaces).repeat(rows)));
    let (result, took) = timed(&file, "x.ods", opts(4 * MIB as u64));
    assert!(
        refused_with(&result, "no header row"),
        "{result:?} after {took:?}"
    );
    eprintln!("read in {took:?}");
    assert!(took < Duration::from_secs(2), "{took:?}");
}

#[test]
fn rows_that_repeat_zero_times_make_no_text() {
    ghosts_cost_nothing(
        |spaces| {
            format!(
                r#"<table:table-row table:number-rows-repeated="0">{}</table:table-row>"#,
                spaced("", spaces)
            )
        },
        300,
    );
}

#[test]
fn cells_that_repeat_zero_times_make_no_text() {
    ghosts_cost_nothing(
        |spaces| {
            format!(
                "<table:table-row>{}</table:table-row>",
                spaced(r#" table:number-columns-repeated="0""#, spaces)
            )
        },
        300,
    );
}

#[test]
fn rows_past_the_last_row_of_a_sheet_make_no_text() {
    let spaces = 4 * MIB - 64;
    let row = format!("<table:table-row>{}</table:table-row>", spaced("", spaces));
    let first = r#"<table:table-row table:number-rows-repeated="1048576"><table:table-cell/></table:table-row>"#;
    let file = ods(&table_of(&format!("{first}{}", row.repeat(300))));
    let (result, took) = timed(&file, "x.ods", opts(4 * MIB as u64));
    assert!(
        refused_with(&result, "no header row"),
        "{result:?} after {took:?}"
    );
    eprintln!("read in {took:?}");
    assert!(took < Duration::from_secs(2), "{took:?}");
}

#[test]
fn cells_past_the_last_column_of_a_row_make_no_text() {
    let spaces = 4 * MIB - 64;
    let past = r#"<table:table-cell table:number-columns-repeated="16384"/>"#;
    let row = format!(
        "<table:table-row>{past}{}</table:table-row>",
        spaced("", spaces)
    );
    let file = ods(&table_of(&row.repeat(300)));
    let (result, took) = timed(&file, "x.ods", opts(4 * MIB as u64));
    assert!(
        refused_with(&result, "no header row"),
        "{result:?} after {took:?}"
    );
    eprintln!("read in {took:?}");
    assert!(took < Duration::from_secs(2), "{took:?}");
}

#[test]
fn a_row_that_repeats_zero_times_is_not_the_header() {
    let ghost = format!(
        r#"<table:table-row table:number-rows-repeated="0">{}</table:table-row>"#,
        ods_text_cell("ghost")
    );
    let row = |cells: [&str; 3]| {
        format!(
            "<table:table-row>{}</table:table-row>",
            cells.map(ods_text_cell).concat()
        )
    };
    let file = ods(&table_of(&format!(
        "{ghost}{}{}",
        row(["a", "b", "c"]),
        row(["1", "2", "3"])
    )));
    let table = read_table(&file, "x.ods", &opts(1 << 28)).unwrap();
    assert_eq!(table.headers, ["a", "b", "c"]);
    assert_eq!(table.rows[0].cells, [text("1"), text("2"), text("3")]);
}
