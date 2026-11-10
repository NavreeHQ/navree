//! Work a small file asks of a reading stays in proportion to what the file
//! holds, however many cells name the same thing.

use std::time::{Duration, Instant};

use super::opening_tests::opts;
use super::*;
use crate::import::xls::write::{self as xw, record};

/// An XLS workbook whose shared strings are the one `string`, named by `cells`
/// cells.
fn xls_of_shared(string: &str, cells: usize) -> Vec<u8> {
    let mut sst = vec![0u8; 8];
    let len = u16::try_from(string.len()).unwrap();
    sst.extend(len.to_le_bytes());
    sst.push(0);
    sst.extend(string.as_bytes());
    let globals = record(0x00FC, &sst);
    let mut records = Vec::with_capacity(cells * 14);
    for at in 0..cells {
        records.extend(xw::label_sst((at >> 8) as u16, (at & 0xFF) as u16, 0, 0));
    }
    xw::Book::new(globals, vec![xw::Sheet::new("S", records)]).file()
}

fn timed(file: &[u8], options: &ReadOptions<'_>) -> (Result<Table, SheetsError>, Duration) {
    let at = Instant::now();
    let result = read_table(file, "x.xls", options);
    (result, at.elapsed())
}

#[test]
fn an_xls_shared_string_is_trimmed_once_however_many_cells_use_it() {
    // A string of 20,000 spaces, which trims to nothing, named by 6,000
    // cells; trimmed each time it is used, that is 120 million characters.
    // The same cells of a string with no spaces to trim are the control.
    let cells = 6_000;
    let hostile = xls_of_shared(&" ".repeat(20_000), cells);
    let control = xls_of_shared(&"a".repeat(20_000), cells);
    let options = ReadOptions {
        max_rows: 10,
        ..opts(1 << 28)
    };
    let (blank, blank_took) = timed(&hostile, &options);
    let (plain, plain_took) = timed(&control, &options);
    // Every cell of the blank string is empty: no header, no rows. The
    // control's cells are the rows the limit refuses.
    assert!(
        matches!(&blank, Err(SheetsError::UnsupportedFile(message)) if message.contains("no header row")),
        "{blank:?}"
    );
    assert!(
        matches!(plain, Err(SheetsError::TooManyRows { limit: 10 })),
        "{plain:?}"
    );
    eprintln!("blank cells {blank_took:?}, plain ones {plain_took:?}");
    assert!(
        blank_took < plain_took * 4 + Duration::from_millis(500),
        "blank cells took {blank_took:?}, plain ones {plain_took:?}"
    );
}
