#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use super::write::*;
use super::*;

fn limits() -> XlsLimits {
    XlsLimits {
        max_stream_bytes: 64 * 1024 * 1024,
        max_text_bytes: 64 * 1024 * 1024,
    }
}

/// The cells of sheet `index`, as the file gives them.
fn cells_of(xls: &Xls, index: usize) -> Vec<(u32, u32, XlsValue<'_>)> {
    xls.cells(index, None)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn text(value: &str) -> XlsValue<'static> {
    XlsValue::Text(value.to_owned().into())
}

fn open(bytes: &[u8]) -> Xls {
    match Xls::open(bytes, limits()) {
        Ok(xls) => xls,
        Err(error) => panic!("{error}"),
    }
}

fn refused(bytes: &[u8]) -> XlsError {
    match Xls::open(bytes, limits()) {
        Ok(_) => panic!("the file was read"),
        Err(error) => error,
    }
}

fn damaged(bytes: &[u8], what: &str) {
    match refused(bytes) {
        XlsError::Damaged(message) => assert!(message.contains(what), "{message}"),
        other => panic!("{other:?}"),
    }
}

/// A workbook of one sheet with `records`, with `strings` shared.
fn one_sheet(strings: &[&str], records: Vec<u8>) -> Vec<u8> {
    let shared: Vec<Shared<'_>> = strings.iter().map(|s| Shared::from(*s)).collect();
    let mut globals = codepage(1200);
    globals.extend(sst(&shared, strings.len() as u32, 8224));
    Book::new(globals, vec![Sheet::new("Sheet1", records)]).file()
}

#[test]
fn shared_strings_are_held_and_given_trimmed() {
    // The table keeps its text trimmed: a shared string is trimmed when it is
    // decoded, once, and counted trimmed against the text limit.
    let strings = [
        " 	 padded
 ",
        "   ",
        "inner  space",
        "",
    ];
    let mut records = Vec::new();
    for at in 0..4 {
        records.extend(label_sst(0, at, 0, u32::from(at)));
    }
    let bytes = one_sheet(&strings, records);
    let xls = open(&bytes);
    let cells = cells_of(&xls, 0);
    let read: Vec<&XlsValue<'_>> = cells.iter().map(|cell| &cell.2).collect();
    assert_eq!(
        read,
        [&text("padded"), &text(""), &text("inner  space"), &text("")]
    );
    // 6 + 0 + 12 + 0 bytes of text: a limit of that is enough.
    let enough = XlsLimits {
        max_text_bytes: 18,
        ..limits()
    };
    assert!(Xls::open(&bytes, enough).is_ok());
    let short = XlsLimits {
        max_text_bytes: 17,
        ..limits()
    };
    assert!(matches!(
        Xls::open(&bytes, short),
        Err(XlsError::TooLarge(_))
    ));
}

#[test]
fn a_workbook_is_read_cell_by_cell_in_the_order_of_the_file() {
    let mut records = label_sst(0, 0, 0, 0);
    records.extend(label_sst(0, 1, 0, 1));
    records.extend(number(1, 1, 0, 2.5));
    records.extend(rk(1, 0, 0, rk_int(-7, false)));
    records.extend(boolerr(2, 0, 0, 1, false));
    records.extend(boolerr(2, 1, 0, 0x07, true));
    records.extend(blank(2, 2, 0));
    records.extend(mulrk(
        3,
        0,
        &[
            (0, rk_int(1, false)),
            (0, rk_int(250, true)),
            (0, rk_float(0.5, false)),
        ],
    ));
    let bytes = one_sheet(&["Name", "Age"], records);
    let xls = open(&bytes);
    assert_eq!(xls.sheet_names(), ["Sheet1"]);
    assert!(!xls.date1904());
    assert_eq!(
        cells_of(&xls, 0),
        [
            (0, 0, text("Name")),
            (0, 1, text("Age")),
            (1, 1, XlsValue::Number(2.5)),
            (1, 0, XlsValue::Int(-7)),
            (2, 0, XlsValue::Bool(true)),
            (2, 1, XlsValue::Error("Div0")),
            (3, 0, XlsValue::Int(1)),
            (3, 1, XlsValue::Number(2.5)),
            (3, 2, XlsValue::Number(0.5)),
        ]
    );
}

#[test]
fn rk_values_are_integers_or_floats_as_their_flags_say() {
    let records = mulrk(
        0,
        0,
        &[
            (0, rk_int(300, true)),
            (0, rk_int(-12345, true)),
            (0, rk_int(536_870_911, false)),
            (0, rk_int(-536_870_912, false)),
            (0, rk_float(1234.5, false)),
            (0, rk_float(1234.5, true)),
        ],
    );
    let xls = open(&one_sheet(&[], records));
    let values: Vec<XlsValue<'_>> = cells_of(&xls, 0).into_iter().map(|(_, _, v)| v).collect();
    assert_eq!(
        values,
        [
            XlsValue::Int(3),
            XlsValue::Number(-123.45),
            XlsValue::Int(536_870_911),
            XlsValue::Int(-536_870_912),
            XlsValue::Number(1234.5),
            XlsValue::Number(12.345),
        ]
    );
}

#[test]
fn every_error_code_has_its_name_and_an_unknown_one_its_code() {
    for (code, name) in [
        (0x00, "Null"),
        (0x07, "Div0"),
        (0x0F, "Value"),
        (0x17, "Ref"),
        (0x1D, "Name"),
        (0x24, "Num"),
        (0x2A, "NA"),
        (0x2B, "GettingData"),
    ] {
        let xls = open(&one_sheet(&[], boolerr(0, 0, 0, code, true)));
        assert_eq!(cells_of(&xls, 0), [(0, 0, XlsValue::Error(name))]);
    }
    let xls = open(&one_sheet(&[], boolerr(0, 0, 0, 0x55, true)));
    assert_eq!(
        cells_of(&xls, 0),
        [(0, 0, XlsValue::Text("#Error(0x55)".into()))]
    );
}

#[test]
fn formulas_give_their_cached_results() {
    let mut records = formula(0, 0, 0, 42.5f64.to_le_bytes());
    records.extend(formula(0, 1, 0, formula_text()));
    records.extend(string_record("from a formula", false));
    records.extend(formula(0, 2, 0, formula_bool(true)));
    records.extend(formula(0, 3, 0, formula_error(0x2A)));
    records.extend(formula(0, 4, 0, formula_empty()));
    // Text whose STRING record is missing gives no cell.
    records.extend(formula(1, 0, 0, formula_text()));
    records.extend(number(1, 1, 0, 1.0));
    let xls = open(&one_sheet(&[], records));
    assert_eq!(
        cells_of(&xls, 0),
        [
            (0, 0, XlsValue::Number(42.5)),
            (0, 1, text("from a formula")),
            (0, 2, XlsValue::Bool(true)),
            (0, 3, XlsValue::Error("NA")),
            (0, 4, text("")),
            (1, 1, XlsValue::Number(1.0)),
        ]
    );
    let mut bad = formula(0, 0, 0, [9, 0, 0, 0, 0, 0, 0xFF, 0xFF]);
    damaged(&one_sheet(&[], bad.clone()), "formula");
    bad.truncate(4 + 19);
    bad[2..4].copy_from_slice(&19u16.to_le_bytes());
    damaged(&one_sheet(&[], bad), "FORMULA");
}

#[test]
fn labels_and_rich_strings_are_text() {
    let mut records = label(0, 0, 0, "plain", false);
    records.extend(label(0, 1, 0, "مرحبا", false));
    records.extend(rstring(0, 2, 0, "rich"));
    let xls = open(&one_sheet(&[], records));
    assert_eq!(
        cells_of(&xls, 0),
        [
            (0, 0, text("plain")),
            (0, 1, text("مرحبا")),
            (0, 2, text("rich"))
        ]
    );
}

#[test]
fn a_shared_string_index_past_the_table_is_no_cell() {
    let mut records = label_sst(0, 0, 0, 0);
    records.extend(label_sst(0, 1, 0, 5));
    let xls = open(&one_sheet(&["only"], records));
    assert_eq!(cells_of(&xls, 0), [(0, 0, text("only"))]);
}

#[test]
fn a_shared_string_is_lent_not_copied_for_each_cell() {
    let mut records = label_sst(0, 0, 0, 0);
    records.extend(label_sst(1, 0, 0, 0));
    let xls = open(&one_sheet(&["the same text"], records));
    let cells = cells_of(&xls, 0);
    let pointers: Vec<*const u8> = cells
        .iter()
        .map(|(_, _, value)| match value {
            XlsValue::Text(std::borrow::Cow::Borrowed(text)) => text.as_ptr(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(pointers[0], pointers[1]);
}

#[test]
fn shared_strings_split_across_continue_records_are_joined() {
    let latin: String = "abcdefghij".repeat(30);
    let wide: String = "αβγδεζηθ".repeat(20);
    // Latin-1 first, then characters that need two bytes: the part in the
    // next record restarts with a flag of its own.
    let mixed = format!("{}{}{}", "ab".repeat(40), "ж".repeat(50), "xy".repeat(30));
    let strings = [
        Shared::from(latin.as_str()),
        Shared::from(wide.as_str()),
        Shared::from(mixed.as_str()),
        Shared {
            text: "with runs and phonetic data",
            runs: 30,
            phonetic: 150,
        },
        Shared::from("after"),
    ];
    let mut globals = codepage(1200);
    globals.extend(sst(&strings, 5, 64));
    let mut records = Vec::new();
    for i in 0..5 {
        records.extend(label_sst(i, 0, 0, u32::from(i)));
    }
    let bytes = Book::new(globals, vec![Sheet::new("S", records)]).file();
    let xls = open(&bytes);
    let texts: Vec<String> = cells_of(&xls, 0)
        .into_iter()
        .map(|(_, _, v)| match v {
            XlsValue::Text(t) => t.into_owned(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        texts,
        [
            latin.as_str(),
            wide.as_str(),
            mixed.as_str(),
            "with runs and phonetic data",
            "after"
        ]
    );
}

#[test]
fn a_string_cut_short_by_the_end_of_the_records_is_damage() {
    let mut data = 1u32.to_le_bytes().to_vec();
    data.extend_from_slice(&1u32.to_le_bytes());
    // 10 characters declared, 3 given, no CONTINUE.
    data.extend_from_slice(&10u16.to_le_bytes());
    data.extend_from_slice(&[0, b'a', b'b', b'c']);
    let mut globals = codepage(1200);
    globals.extend(record(0x00FC, &data));
    let bytes = Book::new(globals, vec![Sheet::new("S", Vec::new())]).file();
    damaged(&bytes, "shared string");
}

#[test]
fn the_unique_count_of_the_shared_strings_reserves_nothing() {
    // A table that says it has 2^31 strings and has three.
    let strings = [Shared::from("a"), Shared::from("b"), Shared::from("c")];
    let mut globals = codepage(1200);
    globals.extend(sst(&strings, 1 << 31, 8224));
    // The total count too.
    let at = globals.len() - (3 * 4 + 8);
    globals[at..at + 4].copy_from_slice(&(1u32 << 31).to_le_bytes());
    let bytes = Book::new(globals, vec![Sheet::new("S", label_sst(0, 0, 0, 2))]).file();
    let xls = open(&bytes);
    assert_eq!(cells_of(&xls, 0), [(0, 0, text("c"))]);
    assert!(
        xls.shared_capacity() < 1024,
        "the shared strings hold room for {} strings",
        xls.shared_capacity()
    );
}

#[test]
fn shared_strings_past_the_text_limit_are_refused() {
    let long = "x".repeat(1000);
    let strings = [Shared::from(long.as_str()), Shared::from(long.as_str())];
    let mut globals = codepage(1200);
    globals.extend(sst(&strings, 2, 8224));
    let bytes = Book::new(globals, vec![Sheet::new("S", Vec::new())]).file();
    let limited = |max_text_bytes| {
        Xls::open(
            &bytes,
            XlsLimits {
                max_text_bytes,
                ..limits()
            },
        )
    };
    assert!(limited(2000).is_ok());
    assert!(matches!(limited(1999), Err(XlsError::TooLarge(_))));
}

#[test]
fn sheets_are_worksheets_in_workbook_order() {
    let sheets = vec![
        Sheet {
            kind: 2,
            ..Sheet::new("Chart", Vec::new())
        },
        Sheet::new("First", label(0, 0, 0, "one", false)),
        Sheet {
            kind: 1,
            ..Sheet::new("Macros", Vec::new())
        },
        Sheet {
            state: 1,
            ..Sheet::new("Hidden", label(0, 0, 0, "two", false))
        },
    ];
    let bytes = Book::new(codepage(1200), sheets).file();
    let xls = open(&bytes);
    assert_eq!(xls.sheet_names(), ["First", "Hidden"]);
    assert_eq!(cells_of(&xls, 0), [(0, 0, text("one"))]);
    assert_eq!(cells_of(&xls, 1), [(0, 0, text("two"))]);
    assert!(xls.cells(2, None).is_err());
}

#[test]
fn an_embedded_chart_in_a_sheet_is_skipped() {
    // A chart's substream inside the sheet, with records that look like
    // cells.
    let mut records = label(0, 0, 0, "before", false);
    let mut chart = record(BOF, &[0, 6, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    chart.extend(number(5, 5, 0, 1.0));
    chart.extend(record(EOF, &[]));
    records.extend(chart);
    records.extend(label(1, 0, 0, "after", false));
    let xls = open(&Book::new(codepage(1200), vec![Sheet::new("S", records)]).file());
    assert_eq!(
        cells_of(&xls, 0),
        [(0, 0, text("before")), (1, 0, text("after"))]
    );
}

#[test]
fn a_sheet_with_cells_far_apart_is_two_cells() {
    // The corners of a BIFF8 sheet: 65,536 rows by 256 columns, 16.7
    // million cells between them, two in the file. DIMENSIONS says so too.
    let xls = open(&corners());
    assert_eq!(
        cells_of(&xls, 0),
        [(0, 0, text("first")), (65_535, 255, XlsValue::Number(2.0))]
    );
}

#[test]
fn dates_are_found_by_format_as_the_xlsx_reader_finds_them() {
    // XF 0 general, 1 built-in date (14), 2 built-in duration (46), 3 a
    // custom date, 4 a custom number with quoted text, 5 the built-in date
    // 14 again, which a FORMAT record may not set (its code for 14 is not
    // used).
    let mut globals = codepage(1200);
    globals.extend(format(164, "yyyy-mm-dd", false));
    globals.extend(format(165, "\"Day \"0", false));
    globals.extend(format(14, "0.00", false));
    for id in [0, 14, 46, 164, 165, 14] {
        globals.extend(xf(id));
    }
    let mut records = Vec::new();
    for (col, xf) in (0..6).zip(0..6) {
        records.extend(number(0, col, xf, 45000.5));
    }
    records.extend(rk(1, 0, 1, rk_int(45000, false)));
    records.extend(rk(1, 1, 99, rk_int(45000, false)));
    let xls = open(&Book::new(globals, vec![Sheet::new("S", records)]).file());
    let values: Vec<XlsValue<'_>> = cells_of(&xls, 0).into_iter().map(|(_, _, v)| v).collect();
    assert_eq!(
        values,
        [
            XlsValue::Number(45000.5),
            XlsValue::DateTime(45000.5),
            XlsValue::DateTime(45000.5),
            XlsValue::DateTime(45000.5),
            XlsValue::Number(45000.5),
            XlsValue::DateTime(45000.5),
            XlsValue::DateTime(45000.0),
            XlsValue::Int(45000),
        ]
    );
}

#[test]
fn the_1904_date_system_is_reported() {
    let mut globals = codepage(1200);
    globals.extend(datemode(true));
    let xls = open(&Book::new(globals, vec![Sheet::new("S", Vec::new())]).file());
    assert!(xls.date1904());
}

#[test]
fn an_encrypted_workbook_is_refused_as_encrypted() {
    let mut globals = filepass();
    globals.extend(codepage(1200));
    let bytes = Book::new(globals, vec![Sheet::new("S", Vec::new())]).file();
    assert!(matches!(refused(&bytes), XlsError::Encrypted));
}

#[test]
fn files_that_are_not_compound_files_are_not_xls() {
    for bytes in [&b""[..], b"PK\x03\x04", b"a,b\n1,2\n", &[0u8; 600]] {
        assert!(matches!(refused(bytes), XlsError::NotXls), "{bytes:?}");
    }
}

#[test]
fn a_compound_file_without_a_workbook_is_not_xls() {
    let file = compound(&[("WordDocument", &[1, 2, 3][..])]);
    assert!(matches!(refused(&file.bytes), XlsError::NotXls));
}

#[test]
fn a_biff5_book_is_read() {
    let mut globals = codepage(1252);
    globals.extend(format(164, "yyyy-mm-dd", true));
    globals.extend(xf(0));
    globals.extend(xf(164));
    let mut records = label(0, 0, 0, "Café \u{20AC}", true);
    records.extend(number(0, 1, 1, 45000.0));
    records.extend(formula(1, 0, 0, formula_text()));
    records.extend(string_record("text result", true));
    let book = Book {
        biff5: true,
        ..Book::new(globals, vec![Sheet::new("Feuille", records)])
    };
    let xls = open(&book.file());
    assert_eq!(xls.sheet_names(), ["Feuille"]);
    assert_eq!(
        cells_of(&xls, 0),
        [
            // Windows-1252: 0x80 is the euro sign.
            (0, 0, text("Café \u{20AC}")),
            (0, 1, XlsValue::DateTime(45000.0)),
            (1, 0, text("text result")),
        ]
    );
}

#[test]
fn a_workbook_in_the_mini_stream_or_in_sectors_is_read_the_same() {
    // A small workbook lives in the mini stream; a large one in sectors of
    // its own.
    for count in [3u16, 600] {
        let mut records = Vec::new();
        for row in 0..count {
            records.extend(number(row, 0, 0, f64::from(row)));
        }
        let book = Book::new(codepage(1200), vec![Sheet::new("S", records)]);
        let compound = book.compound();
        assert_eq!(compound.mini[0], count == 3);
        let xls = open(&compound.bytes);
        let cells = cells_of(&xls, 0);
        assert_eq!(cells.len(), usize::from(count));
        assert_eq!(
            cells.last().unwrap().2,
            XlsValue::Number(f64::from(count - 1))
        );
    }
}

fn big_book() -> Compound {
    let mut records = Vec::new();
    for row in 0..600 {
        records.extend(number(row, 0, 0, f64::from(row)));
    }
    Book::new(codepage(1200), vec![Sheet::new("S", records)]).compound()
}

#[test]
fn a_sector_chain_that_loops_is_refused() {
    let mut file = big_book();
    let chain = file.chains[0].clone();
    // The workbook's last sector leads back to its first.
    file.set_fat(*chain.last().unwrap(), chain[0]);
    // And the directory says it is far longer than the file, so a reader
    // that follows the chain to the size would go round for ever.
    file.set_size(0, u32::MAX);
    damaged(&file.bytes, "loop");

    let mut file = big_book();
    let directory = file.directory[0];
    file.set_fat(directory, directory);
    damaged(&file.bytes, "loop");
}

#[test]
fn a_mini_sector_chain_that_loops_is_refused() {
    let book = Book::new(codepage(1200), vec![Sheet::new("S", number(0, 0, 0, 1.0))]);
    let mut file = book.compound();
    assert!(file.mini[0]);
    let chain = file.chains[0].clone();
    file.set_mini_fat(*chain.last().unwrap(), chain[0]);
    file.set_size(0, 4095);
    damaged(&file.bytes, "loop");
}

#[test]
fn a_difat_chain_that_loops_is_refused() {
    let mut file = big_book();
    // A DIFAT sector after the file's last that points at itself.
    let id = (file.bytes.len() / 512 - 1) as u32;
    let mut sector = vec![0xFFu8; 512];
    sector[508..512].copy_from_slice(&id.to_le_bytes());
    file.bytes.extend(sector);
    file.set_header(68, id);
    file.set_header(72, 1);
    damaged(&file.bytes, "loop");
}

#[test]
fn a_sector_past_the_end_of_the_file_is_refused() {
    let mut file = big_book();
    let chain = file.chains[0].clone();
    file.set_fat(chain[1], 1_000_000);
    damaged(&file.bytes, "past the end");
}

#[test]
fn a_chain_that_ends_before_the_size_gives_what_it_holds() {
    // The size says more than the chain holds: the stream is the chain.
    let mut file = big_book();
    let len = file.chains[0].len() as u32 * 512;
    file.set_size(0, len + 100_000);
    let xls = open(&file.bytes);
    assert_eq!(cells_of(&xls, 0).len(), 600);
}

#[test]
fn a_workbook_stream_past_its_limit_is_refused_before_it_is_read() {
    let file = big_book();
    let size = file.chains[0].len() as u64 * 512;
    let open_with = |max_stream_bytes| {
        Xls::open(
            &file.bytes,
            XlsLimits {
                max_stream_bytes,
                ..limits()
            },
        )
    };
    assert!(open_with(size).is_ok());
    assert!(matches!(open_with(4096), Err(XlsError::TooLarge(_))));
}

#[test]
fn damaged_records_are_refused() {
    // A record that says it is longer than the stream.
    let mut records = number(0, 0, 0, 1.0);
    records[2..4].copy_from_slice(&5000u16.to_le_bytes());
    let bytes = Book::new(codepage(1200), vec![Sheet::new("S", records)]).file();
    damaged(&bytes, "record");
    // A cell record too short for its fields.
    for short in [
        record(0x0203, &[0; 13]),
        record(0x027E, &[0; 9]),
        record(0x00FD, &[0; 9]),
        record(0x0204, &[0; 5]),
        record(0x0205, &[0; 7]),
        record(0x00BD, &[0; 5]),
        record(0x0200, &[0; 12]),
        record(0x00E5, &[3, 0, 0, 0]),
    ] {
        let bytes = Book::new(codepage(1200), vec![Sheet::new("S", short.clone())]).file();
        assert!(matches!(refused(&bytes), XlsError::Damaged(_)), "{short:?}");
    }
    // MULRK whose last column comes before its first.
    let mut bad = mulrk(0, 5, &[(0, rk_int(1, false))]);
    let n = bad.len();
    bad[n - 2..].copy_from_slice(&1u16.to_le_bytes());
    let bytes = Book::new(codepage(1200), vec![Sheet::new("S", bad)]).file();
    damaged(&bytes, "MULRK");
    // A sheet that starts past the end of the stream.
    let mut stream = Book::new(codepage(1200), vec![Sheet::new("S", Vec::new())]).stream();
    let at = stream.windows(2).position(|w| w == [0x85, 0x00]).unwrap() + 4;
    stream[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    damaged(&compound(&[("Workbook", &stream)]).bytes, "sheet");
}

#[test]
fn an_unknown_codepage_is_refused() {
    let bytes = Book::new(codepage(4), vec![Sheet::new("S", Vec::new())]).file();
    damaged(&bytes, "code page");
}

#[test]
fn a_damaged_sheet_the_table_does_not_read_still_makes_the_workbook_damaged() {
    let sheets = vec![
        Sheet::new("Good", label(0, 0, 0, "fine", false)),
        Sheet::new("Bad", record(0x0203, &[0; 3])),
    ];
    let bytes = Book::new(codepage(1200), sheets).file();
    damaged(&bytes, "NUMBER");
}

#[test]
fn a_byte_order_mark_that_starts_a_string_is_dropped() {
    let mut records = label(0, 0, 0, "\u{FEFF}Email", false);
    records.extend(label(0, 1, 0, "a\u{FEFF}b", false));
    records.extend(label_sst(0, 2, 0, 0));
    let mut globals = codepage(1200);
    globals.extend(sst(&[Shared::from("\u{FEFF}shared")], 1, 8224));
    let xls = open(&Book::new(globals, vec![Sheet::new("S", records)]).file());
    assert_eq!(
        cells_of(&xls, 0),
        [
            (0, 0, text("Email")),
            (0, 1, text("a\u{FEFF}b")),
            (0, 2, text("shared"))
        ]
    );
}

/// A cell's value as the fixtures' expected cells write it.
fn written(value: &XlsValue<'_>) -> String {
    use sha2::Digest;
    match value {
        XlsValue::Text(text) if text.chars().count() > 64 => {
            let head: String = text.chars().take(16).collect();
            let digest = hex::encode(sha2::Sha256::digest(text.as_bytes()));
            format!(
                "Text(long(chars={}, bytes={}, sha256={}, head={head:?}))",
                text.chars().count(),
                text.len(),
                &digest[..16]
            )
        }
        XlsValue::Text(text) => format!("Text({text:?})"),
        XlsValue::Number(n) => format!("Number({n:?})"),
        XlsValue::Int(n) => format!("Int({n})"),
        XlsValue::DateTime(n) => format!("DateTime({n:?})"),
        XlsValue::Bool(b) => format!("Bool({b})"),
        XlsValue::Error(name) => format!("Error({name})"),
    }
}

/// The date system, then each worksheet and its cells by position, the
/// last cell the file gives at a position kept.
fn written_workbook(bytes: &[u8]) -> String {
    let xls = open(bytes);
    let mut out = format!("date1904: {}\n", xls.date1904());
    for (index, name) in xls.sheet_names().iter().enumerate() {
        out.push_str(&format!("sheet {name:?}\n"));
        let mut cells = std::collections::BTreeMap::new();
        for (row, col, value) in cells_of(&xls, index) {
            cells.insert((row, col), written(&value));
        }
        for ((row, col), value) in cells {
            out.push_str(&format!("  {row} {col} {value}\n"));
        }
    }
    out
}

#[test]
fn workbooks_are_read_cell_for_cell_as_recorded() {
    // Files LibreOffice wrote (types, its 1904 twin, many rows; a chart
    // sheet, an embedded chart, long shared strings over CONTINUE records,
    // dates, errors, formulas), and two written here: a BIFF5 book and a
    // BIFF8 one with the records LibreOffice does not write. Each `.txt` is
    // the cells recorded for the file.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/xls/");
    for name in ["types", "types1904", "many", "biff5", "biff8"] {
        let bytes = std::fs::read(format!("{dir}{name}.xls")).unwrap();
        let expected = std::fs::read_to_string(format!("{dir}{name}.xls.txt")).unwrap();
        let read = written_workbook(&bytes);
        assert_eq!(read, expected.replace("\r\n", "\n"), "{name}.xls");
    }
}

/// The corners of a BIFF8 sheet, and DIMENSIONS saying it is all used.
fn corners() -> Vec<u8> {
    let mut records = dimensions(0, 65_536, 0, 256);
    records.extend(label(0, 0, 0, "first", false));
    records.extend(number(65_535, 255, 0, 2.0));
    Book::new(codepage(1200), vec![Sheet::new("S", records)]).file()
}

#[cfg(target_os = "linux")]
fn kib_of(key: &str) -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    status
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .and_then(|rest| rest.trim().trim_end_matches(" kB").trim().parse().ok())
        .unwrap()
}

/// Runs alone in a process (see the test below): reads the corners of a
/// sheet of 16.7 million cells and checks the most memory that took.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "runs in a process of its own, from the test below"]
fn the_peak_memory_of_reading_cells_far_apart() {
    let bytes = corners();
    std::fs::write("/proc/self/clear_refs", "5").unwrap();
    let before = kib_of("VmHWM:");
    let xls = open(&bytes);
    let cells = cells_of(&xls, 0);
    let used = kib_of("VmHWM:").saturating_sub(before);
    assert_eq!(cells.len(), 2);
    // A cell of a dense grid is at least 24 bytes: 400 MB for this area.
    assert!(
        used < 4 * 1024,
        "reading held {used} KiB at most, over 4 MiB"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn cells_far_apart_hold_memory_for_the_cells_not_the_area() {
    let this = module_path!().split_once("::").unwrap().1;
    // nosemgrep: navree-process-outside-tooling
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("{this}::the_peak_memory_of_reading_cells_far_apart"),
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

#[test]
fn a_formula_result_longer_than_a_record_goes_on_in_continue_records() {
    let long = "ab".repeat(5000);
    let mut data = (long.len() as u16).to_le_bytes().to_vec();
    data.push(0);
    data.extend_from_slice(long.as_bytes());
    let mut records = formula(0, 0, 0, formula_text());
    records.extend(record(0x0207, &data[..3 + 8000]));
    // The rest, after a flags byte of its own: two bytes a character.
    let rest: Vec<u8> = std::iter::once(1)
        .chain(long[8000..].encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    records.extend(record(CONTINUE, &rest));
    let xls = open(&Book::new(codepage(1200), vec![Sheet::new("S", records)]).file());
    assert_eq!(cells_of(&xls, 0), [(0, 0, text(&long))]);
}

#[test]
fn a_biff5_format_code_is_a_string_with_an_8_bit_length() {
    for (code, date) in [
        ("\\d0", false),
        ("\"d\"0", false),
        ("d", true),
        ("yyyy-mm-dd", true),
        ("0.00", false),
    ] {
        let mut globals = codepage(1252);
        globals.extend(format(164, code, true));
        globals.extend(xf(164));
        let book = Book {
            biff5: true,
            ..Book::new(globals, vec![Sheet::new("S", number(0, 0, 0, 45000.0))])
        };
        let xls = open(&book.file());
        let expected = if date {
            XlsValue::DateTime(45000.0)
        } else {
            XlsValue::Number(45000.0)
        };
        assert_eq!(cells_of(&xls, 0), [(0, 0, expected)], "{code:?}");
    }
}

#[test]
fn biff8_strings_are_unicode_whatever_the_code_page() {
    let mut records = label(0, 0, 0, "Café", false);
    records.extend(label(0, 1, 0, "日本", false));
    let xls = open(&Book::new(codepage(1252), vec![Sheet::new("S", records)]).file());
    assert_eq!(
        cells_of(&xls, 0),
        [(0, 0, text("Café")), (0, 1, text("日本"))]
    );
}

#[test]
fn biff5_strings_are_read_in_the_workbook_code_page() {
    let (bytes, _, _) = encoding_rs::SHIFT_JIS.encode("日本語");
    let mut data = vec![0, 0, 0, 0, 0, 0];
    data.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    data.extend_from_slice(&bytes);
    let mut cases = vec![(932, record(0x0204, &data), "日本語")];
    // The code pages old versions of Excel write for US-ASCII, Mac Roman
    // and Windows-1252.
    for page in [367, 32768, 32769] {
        cases.push((page, label(0, 0, 0, "plain", true), "plain"));
    }
    for (page, records, expected) in cases {
        let book = Book {
            biff5: true,
            ..Book::new(codepage(page), vec![Sheet::new("S", records)])
        };
        let xls = open(&book.file());
        assert_eq!(cells_of(&xls, 0), [(0, 0, text(expected))], "{page}");
    }
}

#[test]
fn a_workbook_obfuscated_with_xor_is_refused_as_encrypted() {
    let mut globals = record(0x002F, &[0, 0, 0x12, 0x34, 0x56, 0x78]);
    globals.extend(codepage(1200));
    let bytes = Book::new(globals, vec![Sheet::new("S", label(0, 0, 0, "x", false))]).file();
    assert!(matches!(refused(&bytes), XlsError::Encrypted));
}

#[test]
fn sheets_with_the_same_name_keep_their_own_cells() {
    let sheets = vec![
        Sheet::new("Same", label(0, 0, 0, "first", false)),
        Sheet::new("Same", label(0, 0, 0, "second", false)),
    ];
    let xls = open(&Book::new(codepage(1200), sheets).file());
    assert_eq!(xls.sheet_names(), ["Same", "Same"]);
    assert_eq!(cells_of(&xls, 0), [(0, 0, text("first"))]);
    assert_eq!(cells_of(&xls, 1), [(0, 0, text("second"))]);
}

#[test]
fn a_formula_is_read_by_its_cached_result_whatever_its_tokens() {
    // A formula whose token count runs past its record: the formula is not
    // read, its result is.
    let mut data = vec![0, 0, 0, 0, 0, 0];
    data.extend_from_slice(&1.5f64.to_le_bytes());
    data.extend_from_slice(&[0; 6]);
    data.extend_from_slice(&200u16.to_le_bytes());
    data.extend_from_slice(&[0x1E, 1, 0]);
    let xls = open(&one_sheet(&[], record(0x0006, &data)));
    assert_eq!(cells_of(&xls, 0), [(0, 0, XlsValue::Number(1.5))]);
}

#[test]
fn a_damaged_defined_name_is_refused() {
    // NAME: options, a key, a name of 4 characters, a formula of 7 bytes,
    // and so on: here it stops after 10 bytes.
    let mut globals = codepage(1200);
    globals.extend(record(0x0018, &[0, 0, 0, 4, 7, 0, 0, 0, 0, 0]));
    let bytes = Book::new(globals, vec![Sheet::new("S", Vec::new())]).file();
    damaged(&bytes, "NAME");
    // A whole name whose formula is a reference to another sheet's cell,
    // cut short.
    let mut data = vec![0, 0, 0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, b'x'];
    data.extend_from_slice(&[0x3A, 0, 0]);
    let mut globals = codepage(1200);
    globals.extend(record(0x0018, &data));
    let bytes = Book::new(globals, vec![Sheet::new("S", Vec::new())]).file();
    damaged(&bytes, "NAME");
    // And whole.
    let mut data = vec![0, 0, 0, 1, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, b'x'];
    data.extend_from_slice(&[0x3A, 0, 0, 1, 0, 2, 0]);
    let mut globals = codepage(1200);
    globals.extend(record(0x0018, &data));
    let bytes = Book::new(globals, vec![Sheet::new("S", label(0, 0, 0, "ok", false))]).file();
    assert_eq!(cells_of(&open(&bytes), 0), [(0, 0, text("ok"))]);
}

#[test]
fn a_string_of_its_count_alone_is_empty_only_when_the_count_is_zero() {
    let mut records = formula(0, 0, 0, formula_text());
    records.extend(record(0x0207, &[0, 0]));
    let xls = open(&one_sheet(&[], records));
    assert_eq!(cells_of(&xls, 0), [(0, 0, text(""))]);
    let mut records = formula(0, 0, 0, formula_text());
    records.extend(record(0x0207, &[3, 0]));
    damaged(&one_sheet(&[], records), "STRING");
}
