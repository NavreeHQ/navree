//! Reading workbooks: that a sheet is tabulated as the range calamine builds
//! would be, without the range.

use std::io::{Cursor, Write};

use super::*;

const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

/// Writes the XML of a sheet.
type SheetXml<'a> = &'a dyn Fn(&mut dyn Write) -> std::io::Result<()>;

/// A workbook from sheet XML written by `write_sheet`, with the shared
/// strings `strings` and the styles the cases below use by index:
/// 0 general, 1 date, 2 date and time, 3 time, 4 a custom date and time,
/// 5 a duration, 6 a percentage, 7 text, 8 scientific.
fn book_of(sheets: &[(&str, SheetXml<'_>)], strings: &[&str], date1904: bool) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    let mut part = |name: &str, body: &str| {
        zip.start_file(name, options).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    };
    let overrides: String = (1..=sheets.len())
        .map(|i| {
            format!(
                r#"<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
            )
        })
        .collect();
    part(
        "[Content_Types].xml",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>{overrides}<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/></Types>"#
        ),
    );
    part(
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
    );
    let listed: String = sheets
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            format!(
                r#"<sheet name="{name}" sheetId="{id}" r:id="rId{id}"/>"#,
                id = i + 1
            )
        })
        .collect();
    let pr = if date1904 {
        r#"<workbookPr date1904="1"/>"#
    } else {
        ""
    };
    part(
        "xl/workbook.xml",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="{NS}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{pr}<sheets>{listed}</sheets></workbook>"#
        ),
    );
    let rels: String = (1..=sheets.len())
        .map(|i| {
            format!(
                r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i}.xml"/>"#
            )
        })
        .collect();
    part(
        "xl/_rels/workbook.xml.rels",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}<Relationship Id="rId100" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId101" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#
        ),
    );
    part(
        "xl/styles.xml",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="{NS}"><numFmts count="3"><numFmt numFmtId="164" formatCode="yyyy\-mm\-dd\ hh:mm"/><numFmt numFmtId="165" formatCode="[h]:mm:ss"/><numFmt numFmtId="166" formatCode="0.00E+00"/></numFmts><cellXfs count="9"><xf numFmtId="0"/><xf numFmtId="14"/><xf numFmtId="22"/><xf numFmtId="21"/><xf numFmtId="164"/><xf numFmtId="165"/><xf numFmtId="9"/><xf numFmtId="49"/><xf numFmtId="166"/></cellXfs></styleSheet>"#
        ),
    );
    let shared: String = strings
        .iter()
        .map(|text| format!(r#"<si><t xml:space="preserve">{text}</t></si>"#))
        .collect();
    part(
        "xl/sharedStrings.xml",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><sst xmlns="{NS}" count="{n}" uniqueCount="{n}">{shared}</sst>"#,
            n = strings.len()
        ),
    );
    for (i, (_, write_sheet)) in sheets.iter().enumerate() {
        zip.start_file(format!("xl/worksheets/sheet{}.xml", i + 1), options)
            .unwrap();
        write_sheet(&mut zip).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// A workbook of one sheet whose `<sheetData>` holds `rows`, followed by
/// `after` (merged cells, say).
fn book(rows: &str, after: &str, strings: &[&str]) -> Vec<u8> {
    book_of(
        &[("Sheet1", &|out| {
            write!(
                out,
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="{NS}"><sheetData>{rows}</sheetData>{after}</worksheet>"#
            )
        })],
        strings,
        false,
    )
}

/// A workbook of one sheet whose XML stops after `rows`, as a file cut short
/// does: calamine reports it unreadable when it gets that far.
fn unfinished_book(rows: &str) -> Vec<u8> {
    book_of(
        &[("Sheet1", &|out| {
            write!(
                out,
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="{NS}"><sheetData>{rows}"#
            )
        })],
        &[],
        false,
    )
}

fn limits(max_rows: u64) -> ReadOptions<'static> {
    ReadOptions {
        max_rows,
        ..ReadOptions::default()
    }
}

/// An inline string cell.
fn text_at(at: &str, text: &str) -> String {
    format!(r#"<c r="{at}" t="inlineStr"><is><t>{text}</t></is></c>"#)
}

fn number_at(at: &str, number: &str) -> String {
    format!(r#"<c r="{at}"><v>{number}</v></c>"#)
}

fn row(number: u32, cells: &[String]) -> String {
    format!(r#"<row r="{number}">{}</row>"#, cells.concat())
}

/// Reading as it was before the cells were streamed: the whole sheet as a
/// range, every row of it after that.
fn read_as_a_range(bytes: &[u8], options: &ReadOptions<'_>) -> Result<Table, SheetsError> {
    let unreadable = |error: calamine::Error| {
        SheetsError::UnsupportedFile(format!("The workbook cannot be read: {error}"))
    };
    let mut workbook = open_workbook_auto_from_rs(Cursor::new(bytes)).map_err(unreadable)?;
    let kind = match &workbook {
        calamine::Sheets::Xls(_) => FileKind::Xls,
        calamine::Sheets::Ods(_) => FileKind::Ods,
        _ => FileKind::Xlsx,
    };
    let range = match options.sheet.map(str::trim).filter(|s| !s.is_empty()) {
        Some(name) => {
            if !workbook.sheet_names().iter().any(|n| n == name) {
                return Err(SheetsError::UnsupportedFile(format!(
                    "The workbook has no sheet named `{name}`."
                )));
            }
            workbook.worksheet_range(name).map_err(unreadable)?
        }
        None => workbook
            .worksheet_range_at(0)
            .ok_or_else(|| SheetsError::UnsupportedFile("The workbook has no sheets.".into()))?
            .map_err(unreadable)?,
    };
    let first_row = range.start().map_or(0, |(row, _)| u64::from(row));
    let first_col = range.start().map_or(0, |(_, col)| col as usize);
    let mut tabulator = Tabulator::new(options);
    for (index, row) in range.rows().enumerate() {
        let mut cells = vec![Cell::Empty; first_col];
        cells.extend(row.iter().map(data_cell));
        let used = cells
            .iter()
            .rposition(|cell| !cell.is_empty())
            .map_or(0, |index| index + 1);
        tabulator.push(first_row + index as u64 + 1, used, cells)?;
    }
    let mut table = tabulator.finish()?;
    table.kind = kind;
    Ok(table)
}

/// What reading came to, for comparing: the table or the message.
fn came_to(result: Result<Table, SheetsError>) -> Result<Table, String> {
    result.map_err(|error| format!("{error:?}"))
}

/// The same limits, tight and loose, that every workbook below is read with.
fn every_limit() -> Vec<ReadOptions<'static>> {
    let mut all = vec![limits(1_000)];
    for rows in [0, 1, 2, 3] {
        all.push(limits(rows));
    }
    for columns in [1, 2, 3, 5] {
        all.push(ReadOptions {
            max_columns: columns,
            ..limits(1_000)
        });
    }
    for cells in [1, 4, 7, 12, 30] {
        all.push(ReadOptions {
            max_cells: cells,
            ..limits(1_000)
        });
    }
    all
}

fn assert_read_as_a_range(label: &str, bytes: &[u8]) {
    for options in every_limit() {
        let expected = came_to(read_as_a_range(bytes, &options));
        let read = came_to(read_table(bytes, "book.xlsx", &options));
        assert_eq!(
            read, expected,
            "{label}: max_rows {}, max_columns {}, max_cells {}",
            options.max_rows, options.max_columns, options.max_cells
        );
    }
}

#[test]
fn hand_written_sheets_are_tabulated_as_their_range_was() {
    let strings = ["Name", "Email", "Ada", "a (2)", " padded ", "x"];
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "the first row starts in column B and a later row in column A",
            book(
                &[
                    row(
                        1,
                        &[
                            r#"<c r="B1" t="s"><v>0</v></c>"#.into(),
                            r#"<c r="C1" t="s"><v>1</v></c>"#.into(),
                        ],
                    ),
                    row(
                        2,
                        &[
                            number_at("A2", "7"),
                            r#"<c r="B2" t="s"><v>2</v></c>"#.into(),
                        ],
                    ),
                ]
                .concat(),
                "",
                &strings,
            ),
        ),
        (
            "every row starts in column C",
            book(
                &[
                    row(2, &[text_at("C2", "a"), text_at("D2", "b")]),
                    row(3, &[number_at("C3", "1"), number_at("D3", "2")]),
                ]
                .concat(),
                "",
                &[],
            ),
        ),
        (
            "typed values",
            book(
                &[
                    row(
                        1,
                        &(0..10)
                            .map(|i| text_at(&format!("{}1", (b'A' + i) as char), &format!("h{i}")))
                            .collect::<Vec<_>>(),
                    ),
                    row(
                        2,
                        &[
                            number_at("A2", "1.5"),
                            number_at("B2", "3"),
                            r#"<c r="C2" t="b"><v>1</v></c>"#.into(),
                            r#"<c r="D2" t="e"><v>#DIV/0!</v></c>"#.into(),
                            r#"<c r="E2" t="str"><f>A1&amp;"x"</f><v>text</v></c>"#.into(),
                            r#"<c r="F2"><f>A2*2</f><v>3</v></c>"#.into(),
                            r#"<c r="G2"><f>A2*2</f></c>"#.into(),
                            r#"<c r="H2" s="1"><v>46082</v></c>"#.into(),
                            r#"<c r="I2" s="2"><v>46082.75</v></c>"#.into(),
                            r#"<c r="J2" s="3"><v>0.25</v></c>"#.into(),
                        ],
                    ),
                    row(
                        3,
                        &[
                            r#"<c r="A3" s="4"><v>46082.5</v></c>"#.into(),
                            r#"<c r="B3" s="5"><v>1.5</v></c>"#.into(),
                            r#"<c r="C3" s="6"><v>0.256</v></c>"#.into(),
                            r#"<c r="D3" s="7"><v>12</v></c>"#.into(),
                            r#"<c r="E3" s="8"><v>12345.678</v></c>"#.into(),
                            r#"<c r="F3" t="d"><v>2026-03-01T10:00:00</v></c>"#.into(),
                            r#"<c r="G3" t="n"><v>1e3</v></c>"#.into(),
                            r#"<c r="H3" t="inlineStr"><is><r><t>rich </t></r><r><t>text</t></r></is></c>"#.into(),
                            r#"<c r="I3" t="e"><v>#N/A</v></c>"#.into(),
                            r#"<c r="J3" t="e"><v>#REF!</v></c>"#.into(),
                        ],
                    ),
                ]
                .concat(),
                "",
                &[],
            ),
        ),
        (
            "shared strings, padded and repeated",
            book(
                &[
                    row(
                        1,
                        &[
                            r#"<c r="A1" t="s"><v>0</v></c>"#.into(),
                            r#"<c r="B1" t="s"><v>0</v></c>"#.into(),
                            r#"<c r="C1" t="s"><v>3</v></c>"#.into(),
                            r#"<c r="D1" t="s"><v>0</v></c>"#.into(),
                        ],
                    ),
                    row(
                        2,
                        &[
                            r#"<c r="A2" t="s"><v>4</v></c>"#.into(),
                            r#"<c r="B2" t="s"><v>5</v></c>"#.into(),
                        ],
                    ),
                ]
                .concat(),
                "",
                &strings,
            ),
        ),
        (
            "empty cells, styled cells and merged cells",
            book(
                &[
                    row(
                        1,
                        &[
                            r#"<c r="A1" s="1"/>"#.into(),
                            text_at("B1", "a"),
                            r#"<c r="C1" s="2"></c>"#.into(),
                            text_at("D1", "d"),
                        ],
                    ),
                    row(2, &[r#"<c r="A2"/>"#.into(), r#"<c r="B2"/>"#.into()]),
                    row(3, &[number_at("B3", "1"), number_at("D3", "2")]),
                ]
                .concat(),
                r#"<mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells>"#,
                &[],
            ),
        ),
        (
            "rows and cells without a position",
            book(
                r#"<row><c t="inlineStr"><is><t>a</t></is></c><c t="inlineStr"><is><t>b</t></is></c></row><row><c><v>1</v></c><c><v>2</v></c></row><row r="5"><c><v>3</v></c></row><row><c><v>4</v></c><c r="C7"><v>5</v></c><c><v>6</v></c></row>"#,
                "",
                &[],
            ),
        ),
        (
            "rows out of order",
            book(
                &[
                    row(5, &[number_at("A5", "5"), number_at("B5", "50")]),
                    row(6, &[number_at("A6", "6")]),
                    row(2, &[text_at("A2", "first"), text_at("B2", "second")]),
                    row(9, &[number_at("A9", "9")]),
                ]
                .concat(),
                "",
                &[],
            ),
        ),
        (
            "the wide row comes first and a narrow one after it",
            book(
                &[
                    row(
                        4,
                        &[
                            text_at("A4", "a"),
                            text_at("B4", "b"),
                            text_at("C4", "c"),
                            text_at("D4", "d"),
                            text_at("E4", "e"),
                            text_at("F4", "f"),
                        ],
                    ),
                    row(5, &[number_at("A5", "1")]),
                    row(1, &[text_at("A1", "only"), text_at("B1", "two")]),
                ]
                .concat(),
                "",
                &[],
            ),
        ),
        (
            "cells out of order within a row",
            book(
                &row(
                    1,
                    &[
                        text_at("C1", "c"),
                        text_at("A1", "a"),
                        text_at("B1", "b"),
                    ],
                ),
                "",
                &[],
            ),
        ),
        (
            "a position given twice",
            book(
                &[
                    row(1, &[text_at("A1", "a"), text_at("B1", "b")]),
                    row(2, &[number_at("A2", "1"), number_at("A2", "2")]),
                    row(3, &[text_at("A3", "kept"), text_at("A3", "   ")]),
                    row(4, &[text_at("A4", "kept"), r#"<c r="A4"/>"#.into()]),
                ]
                .concat(),
                "",
                &[],
            ),
        ),
        (
            "a row given twice",
            book(
                &[
                    row(1, &[text_at("A1", "a"), text_at("B1", "b")]),
                    row(2, &[number_at("A2", "1")]),
                    row(2, &[number_at("B2", "2")]),
                ]
                .concat(),
                "",
                &[],
            ),
        ),
        (
            "blank rows, blank text and trailing empties",
            book(
                &[
                    row(1, &[text_at("A1", "  "), r#"<c r="B1"/>"#.into()]),
                    row(3, &[text_at("B3", " a "), text_at("C3", "b"), text_at("F3", " ")]),
                    row(4, &[]),
                    row(5, &[text_at("A5", " "), text_at("B5", "  ")]),
                    row(7, &[number_at("B7", "1"), number_at("C7", "2"), number_at("D7", "3")]),
                    row(9, &[number_at("Z9", "9")]),
                    row(12, &[text_at("A12", "last")]),
                ]
                .concat(),
                "",
                &[],
            ),
        ),
        (
            "a row that is only a far cell",
            book(
                &[
                    row(1, &[text_at("A1", "a"), text_at("B1", "b")]),
                    row(2, &[number_at("XFD2", "1")]),
                    row(3, &[number_at("A3", "1")]),
                ]
                .concat(),
                "",
                &[],
            ),
        ),
        (
            "a distant row",
            book(
                &[
                    row(1, &[text_at("A1", "a"), text_at("B1", "b")]),
                    row(20_000, &[number_at("A20000", "1"), number_at("B20000", "2")]),
                ]
                .concat(),
                "",
                &[],
            ),
        ),
        (
            "a header repeated and one named like a repeat",
            book(
                &row(
                    1,
                    &[
                        text_at("A1", "a"),
                        text_at("B1", "a"),
                        text_at("C1", "a (2)"),
                        text_at("D1", "a"),
                        number_at("E1", "5"),
                        number_at("F1", "5"),
                    ],
                ),
                "",
                &[],
            ),
        ),
        ("no cells", book("", "", &[])),
        (
            "rows without cells",
            book(&[row(1, &[]), row(2, &[])].concat(), "", &[]),
        ),
        (
            "only empty text",
            book(&row(1, &[text_at("A1", " "), text_at("B1", "")]), "", &[]),
        ),
        (
            "only styled empty cells",
            book(&row(1, &[r#"<c r="A1" s="1"/>"#.into()]), "", &[]),
        ),
    ];
    for (label, bytes) in &cases {
        assert_read_as_a_range(label, bytes);
    }
    // The cases are not all refusals.
    let table = read_table(&cases[0].1, "book.xlsx", &limits(1_000)).unwrap();
    assert_eq!(table.headers, ["Column 1", "Name", "Email"]);
    assert_eq!(table.rows[0].cells[0], Cell::Float(7.0));
}

#[test]
fn a_workbook_with_two_sheets_is_read_by_the_sheet_asked_for() {
    let first = |out: &mut dyn Write| {
        write!(
            out,
            r#"<?xml version="1.0"?><worksheet xmlns="{NS}"><sheetData>{}</sheetData></worksheet>"#,
            [
                row(1, &[text_at("A1", "one")]),
                row(2, &[number_at("A2", "1")])
            ]
            .concat()
        )
    };
    let second = |out: &mut dyn Write| {
        write!(
            out,
            r#"<?xml version="1.0"?><worksheet xmlns="{NS}"><sheetData>{}</sheetData></worksheet>"#,
            [
                row(3, &[text_at("B3", "two"), text_at("C3", "2")]),
                row(4, &[number_at("A4", "1"), number_at("C4", "5")]),
            ]
            .concat()
        )
    };
    let bytes = book_of(&[("First", &first), ("Second", &second)], &[], false);
    for sheet in [
        None,
        Some(""),
        Some("First"),
        Some(" Second "),
        Some("Third"),
    ] {
        for options in every_limit() {
            let options = ReadOptions { sheet, ..options };
            assert_eq!(
                came_to(read_table(&bytes, "book.xlsx", &options)),
                came_to(read_as_a_range(&bytes, &options)),
                "sheet {sheet:?}"
            );
        }
    }
    let table = read_table(
        &bytes,
        "book.xlsx",
        &ReadOptions {
            sheet: Some("Second"),
            ..limits(10)
        },
    )
    .unwrap();
    assert_eq!(table.headers, ["Column 1", "two", "2"]);
    assert_eq!(table.rows[0].number, 4);
}

#[test]
fn a_workbook_with_the_1904_date_system_is_read_as_it_was() {
    let rows = [
        row(1, &[text_at("A1", "day"), text_at("B1", "at")]),
        row(
            2,
            &[
                r#"<c r="A2" s="1"><v>1000</v></c>"#.into(),
                r#"<c r="B2" s="2"><v>1000.5</v></c>"#.into(),
            ],
        ),
    ]
    .concat();
    for date1904 in [false, true] {
        let rows = rows.clone();
        let bytes = book_of(
            &[("Sheet1", &move |out| {
                write!(
                    out,
                    r#"<?xml version="1.0"?><worksheet xmlns="{NS}"><sheetData>{rows}</sheetData></worksheet>"#
                )
            })],
            &[],
            date1904,
        );
        assert_read_as_a_range(&format!("date1904 {date1904}"), &bytes);
    }
}

#[test]
fn workbooks_the_crate_writes_are_tabulated_as_their_range_was() {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let at = date.and_hms_opt(13, 45, 10).unwrap();
    let headers: Vec<String> = [
        "Name", "Age", "Ratio", "Joined", "Seen", "Active", "Note", "",
    ]
    .map(String::from)
    .into();
    let rows: Vec<Vec<Cell>> = vec![
        vec![
            Cell::Text("Ada".into()),
            Cell::Int(36),
            Cell::Float(0.25),
            Cell::Date(date),
            Cell::DateTime(at),
            Cell::Bool(true),
            Cell::Text("  spaced  ".into()),
            Cell::Empty,
        ],
        vec![Cell::Empty; 8],
        vec![
            Cell::Text("Alan".into()),
            Cell::Empty,
            Cell::Float(-1e12),
            Cell::Empty,
            Cell::Empty,
            Cell::Bool(false),
            Cell::Text("é\nnew line".into()),
            Cell::Empty,
        ],
        vec![Cell::Text("Grace".into())],
    ];
    for name in ["People", "A sheet"] {
        let file = crate::write_xlsx(name, &[], &headers, &rows).unwrap();
        assert_read_as_a_range(name, &file);
    }
    let file = crate::write_xlsx("Only headers", &[], &headers, &[]).unwrap();
    assert_read_as_a_range("only headers", &file);
}

/// A tiny generator, to vary the layouts without a dependency.
struct Random(u64);

impl Random {
    fn next(&mut self, below: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % below
    }
}

#[test]
fn generated_sheets_are_tabulated_as_their_range_was() {
    let column = |index: u64| (b'A' + index as u8) as char;
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    for case in 0..250 {
        let rows = 1 + random.next(12);
        let mut cells: Vec<(u64, u64, String)> = Vec::new();
        for r in 1..=rows {
            // Some rows have no cells at all.
            if random.next(6) == 0 {
                continue;
            }
            for c in 0..8 {
                if random.next(3) == 0 {
                    continue;
                }
                let at = format!("{}{}", column(c), r * (1 + random.next(2)));
                let cell = match random.next(9) {
                    0 => format!(
                        r#"<c r="{at}" t="inlineStr"><is><t>w{}</t></is></c>"#,
                        random.next(4)
                    ),
                    1 => format!(r#"<c r="{at}"><v>{}</v></c>"#, random.next(100)),
                    2 => format!(r#"<c r="{at}"><v>{}.5</v></c>"#, random.next(100)),
                    3 => format!(
                        r#"<c r="{at}" s="1"><v>{}</v></c>"#,
                        45_000 + random.next(900)
                    ),
                    4 => format!(r#"<c r="{at}" t="b"><v>{}</v></c>"#, random.next(2)),
                    5 => format!(r#"<c r="{at}" t="inlineStr"><is><t>  </t></is></c>"#),
                    6 => format!(r#"<c r="{at}" s="2"/>"#),
                    7 => format!(r#"<c r="{at}"><f>A1</f><v>{}</v></c>"#, random.next(9)),
                    _ => format!(r#"<c r="{at}" t="e"><v>#N/A</v></c>"#),
                };
                let position = at.trim_start_matches(|c: char| c.is_ascii_alphabetic());
                cells.push((position.parse().unwrap(), c, cell));
            }
        }
        match case % 3 {
            // As Excel writes it: by row, then by column.
            0 => cells.sort_by_key(|(r, c, _)| (*r, *c)),
            // Out of order, now and then a position twice.
            1 => {
                for i in (1..cells.len()).rev() {
                    let j = random.next(i as u64 + 1) as usize;
                    cells.swap(i, j);
                }
            }
            _ => {
                cells.sort_by_key(|(r, c, _)| (*r, *c));
                if cells.len() > 2 {
                    let copy = cells[1].clone();
                    cells.push(copy);
                }
            }
        }
        let xml: String = cells.iter().map(|(_, _, cell)| cell.as_str()).collect();
        // Cells in a row element of their own, so that the layout is the
        // cells' positions alone.
        let rows_xml = format!("<row>{xml}</row>");
        assert_read_as_a_range(
            &format!("generated sheet {case}"),
            &book(&rows_xml, "", &[]),
        );
    }
}

#[test]
fn a_limit_does_not_hide_a_sheet_that_cannot_be_read() {
    // The rows after the limit are cut short: the file is unreadable, as it
    // is whatever the limits.
    let rows = [
        row(1, &[text_at("A1", "a")]),
        row(2, &[number_at("A2", "1")]),
        row(3, &[number_at("A3", "2")]),
        row(4, &[number_at("A4", "3")]),
    ]
    .concat();
    let bytes = unfinished_book(&rows);
    for options in [limits(1), limits(1_000)] {
        let error = read_table(&bytes, "book.xlsx", &options).unwrap_err();
        assert!(
            matches!(&error, SheetsError::UnsupportedFile(m) if m.contains("cannot be read")),
            "{error}"
        );
        assert_eq!(
            came_to(read_table(&bytes, "book.xlsx", &options)),
            came_to(read_as_a_range(&bytes, &options))
        );
    }
}

/// The memory a process has used at most, by asking the operating system:
/// the workspace allows no counting allocator.
#[cfg(target_os = "linux")]
fn kib_of(key: &str) -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("procfs");
    status
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap_or_else(|| panic!("{key} is in /proc/self/status"))
}

/// Eight cells in each of 100,000 rows, a number to a cell. Calamine's range
/// of this sheet holds 800,000 cells: about 80 MB; one row is 8 cells.
#[cfg(target_os = "linux")]
fn long_book() -> Vec<u8> {
    book_of(
        &[("Sheet1", &|out| {
            write!(
                out,
                r#"<?xml version="1.0"?><worksheet xmlns="{NS}"><sheetData>"#
            )?;
            for r in 1..=100_000u32 {
                write!(out, r#"<row r="{r}">"#)?;
                for (c, name) in ["A", "B", "C", "D", "E", "F", "G", "H"].iter().enumerate() {
                    if r == 1 {
                        write!(
                            out,
                            r#"<c r="{name}{r}" t="inlineStr"><is><t>h{c}</t></is></c>"#
                        )?;
                    } else {
                        write!(out, r#"<c r="{name}{r}"><v>{}.25</v></c>"#, r + c as u32)?;
                    }
                }
                write!(out, "</row>")?;
            }
            write!(out, "</sheetData></worksheet>")
        })],
        &[],
        false,
    )
}

/// Runs alone in a process (see the test below), so that nothing else's
/// memory is counted: reads a workbook of 800,000 cells that a limit of ten
/// rows refuses, and checks how much memory that took at most.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "runs in a process of its own, from the test below"]
fn the_peak_memory_of_reading_a_workbook_over_its_limits() {
    let bytes = long_book();
    // Forget the peak of building the workbook.
    std::fs::write("/proc/self/clear_refs", "5").expect("clear_refs");
    let before = kib_of("VmHWM:");
    let result = read_table(&bytes, "book.xlsx", &limits(10));
    let peak = kib_of("VmHWM:");
    assert!(
        matches!(result, Err(SheetsError::TooManyRows { limit: 10 })),
        "{result:?}"
    );
    let used = peak.saturating_sub(before);
    assert!(
        used < 24 * 1024,
        "reading held {used} KiB at most, over a bound of 24 MiB"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_workbook_is_not_held_in_memory_before_its_limits_apply() {
    let this = module_path!().split_once("::").unwrap().1;
    // Runs this test binary again, for one test, so that the peak memory it
    // measures is that test's own and no other test's.
    // nosemgrep: navree-process-outside-tooling
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("{this}::the_peak_memory_of_reading_a_workbook_over_its_limits"),
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
    // That it ran, not that it was filtered out.
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}
