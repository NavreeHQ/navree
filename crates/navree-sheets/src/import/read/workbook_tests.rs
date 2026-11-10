//! Reading workbooks: that a sheet is tabulated as the range of its used
//! area would be, without the range.

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
/// does: it is reported unreadable when the reading gets that far.
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

/// The limits of the workbook readers under `options`.
fn reader_limits(options: &ReadOptions<'_>) -> Limits {
    Limits {
        max_part_bytes: options.max_inflated_bytes,
        max_text_bytes: options.max_text_bytes,
    }
}

/// Every cell the sheet asked for gives, in the order it gives them, one
/// for each column it covers, as the table keeps its value.
fn all_cells(
    bytes: &[u8],
    options: &ReadOptions<'_>,
) -> Result<Vec<(u32, u32, Cell)>, SheetsError> {
    let workbook = Workbook::open(bytes, reader_limits(options))?;
    let sheet = sheet_to_read(workbook.sheet_names(), options)?;
    workbook.read_cells(sheet, None, |cells| {
        let mut all = Vec::new();
        if let Some(cells) = cells {
            while let Some(cell) = cells.next_cell()? {
                for column in cell.column..cell.column + cell.columns {
                    all.push((cell.row, column, cell_of(&cell.value)));
                }
            }
        }
        Ok(all)
    })
}

/// Reading as it was before the cells were streamed: the whole sheet as a
/// range (every position from its first row and column with a cell to its
/// last, a position given twice its last cell), every row of it after that.
fn read_as_a_range(bytes: &[u8], options: &ReadOptions<'_>) -> Result<Table, SheetsError> {
    let kind = Workbook::open(bytes, reader_limits(options))?.kind();
    let cells = all_cells(bytes, options)?;
    let mut tabulator = Tabulator::new(options);
    if let (Some(first_row), Some(last_row), Some(first_col), Some(last_col)) = (
        cells.iter().map(|(row, _, _)| *row).min(),
        cells.iter().map(|(row, _, _)| *row).max(),
        cells.iter().map(|(_, column, _)| *column as usize).min(),
        cells.iter().map(|(_, column, _)| *column as usize).max(),
    ) {
        let width = last_col - first_col + 1;
        let mut range = vec![Cell::Empty; (last_row - first_row + 1) as usize * width];
        for (row, column, cell) in cells {
            range[(row - first_row) as usize * width + column as usize - first_col] = cell;
        }
        for (index, row) in range.chunks(width).enumerate() {
            // Cells left of the range's first column are empty.
            let mut cells = vec![Cell::Empty; first_col];
            cells.extend(row.iter().cloned());
            let used = cells
                .iter()
                .rposition(|cell| !cell.is_empty())
                .map_or(0, |index| index + 1);
            tabulator.push(u64::from(first_row) + index as u64 + 1, used, cells)?;
        }
    }
    let mut table = tabulator.finish()?;
    table.kind = kind;
    Ok(table)
}

/// What reading came to, for comparing: the table or the message.
fn came_to(result: Result<Table, SheetsError>) -> Result<Table, String> {
    result.map_err(|error| format!("{error:?}"))
}

fn refused_for(result: &Result<Table, SheetsError>, what: &str) -> bool {
    matches!(result, Err(SheetsError::UnsupportedFile(message)) if message.contains(what))
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

/// What a workbook's first sheet holds: whether its cells come by row and
/// then by column, how many positions in the first `max_columns` columns
/// have a value, and how many rows have one.
fn shape_of(bytes: &[u8], max_columns: u64) -> (bool, u64, u64) {
    let cells = all_cells(bytes, &limits(1_000)).unwrap();
    let (mut in_order, mut last) = (true, None);
    let mut positions = std::collections::BTreeSet::new();
    for (row, column, _) in cells {
        in_order &= last.is_none_or(|last| (row, column) > last);
        last = Some((row, column));
        if u64::from(column) < max_columns {
            positions.insert((row, column));
        }
    }
    let rows: std::collections::BTreeSet<u32> = positions.iter().map(|(row, _)| *row).collect();
    (in_order, positions.len() as u64, rows.len() as u64)
}

/// Reads `bytes` as the range did, under every limit, and as it is read now.
///
/// A sheet whose cells are not in order is held, until it ends, only up to
/// `max_cells` cells and `max_rows + 1` rows. That is the one difference
/// from the range, which held them all: such a sheet is refused when it has
/// more cells, or rows, with a value than that, though the table made of it
/// would have been within the limits.
fn assert_read_as_a_range(label: &str, bytes: &[u8]) {
    for options in every_limit() {
        let expected = came_to(read_as_a_range(bytes, &options));
        let read = came_to(read_table(bytes, "book.xlsx", &options));
        if read == expected {
            continue;
        }
        let (in_order, cells, rows) = shape_of(bytes, options.max_columns);
        let held_too_much = match &read {
            Err(message) if message.contains("cells (rows times columns)") => {
                cells > options.max_cells
            }
            Err(message) if message.contains("TooManyRows") => rows > options.max_rows + 1,
            _ => false,
        };
        assert!(
            !in_order && held_too_much,
            "{label}: max_rows {}, max_columns {}, max_cells {}\n read {read:?}\n was  {expected:?}",
            options.max_rows,
            options.max_columns,
            options.max_cells
        );
    }
}

#[test]
fn hand_written_sheets_are_tabulated_as_their_range_was() {
    let strings = ["Name", "Email", "Anu", "a (2)", " padded ", "x"];
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
            Cell::Text("Anu".into()),
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
            Cell::Text("Arun".into()),
            Cell::Empty,
            Cell::Float(-1e12),
            Cell::Empty,
            Cell::Empty,
            Cell::Bool(false),
            Cell::Text("é\nnew line".into()),
            Cell::Empty,
        ],
        vec![Cell::Text("Gauri".into())],
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

/// Eight cells in each of 100,000 rows, a number to a cell. A range of the
/// whole sheet would hold 800,000 cells: about 80 MB; one row is 8 cells. With
/// `swap`, the first row lists its first two cells the other way round, so
/// that the sheet is not in order.
#[cfg(target_os = "linux")]
fn long_book(swap: bool) -> Vec<u8> {
    book_of(
        &[("Sheet1", &|out| {
            write!(
                out,
                r#"<?xml version="1.0"?><worksheet xmlns="{NS}"><sheetData>"#
            )?;
            for r in 1..=100_000u32 {
                write!(out, r#"<row r="{r}">"#)?;
                let names = if swap && r == 1 {
                    ["B", "A", "C", "D", "E", "F", "G", "H"]
                } else {
                    ["A", "B", "C", "D", "E", "F", "G", "H"]
                };
                for (c, name) in names.iter().enumerate() {
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

/// Two columns and 4,000 rows, each with a 10 KB text in column K, out of
/// order (the first row lists B before A). 40 MB of text in all.
#[cfg(target_os = "linux")]
fn far_text_book() -> Vec<u8> {
    let text = "x".repeat(10_000);
    book_of(
        &[("Sheet1", &|out| {
            write!(
                out,
                r#"<?xml version="1.0"?><worksheet xmlns="{NS}"><sheetData>"#
            )?;
            write!(
                out,
                r#"<row r="1"><c r="B1" t="inlineStr"><is><t>b</t></is></c><c r="A1" t="inlineStr"><is><t>a</t></is></c></row>"#
            )?;
            for r in 2..=4_000u32 {
                write!(
                    out,
                    r#"<row r="{r}"><c r="A{r}"><v>{r}</v></c><c r="B{r}"><v>1</v></c><c r="K{r}" t="inlineStr"><is><t>{text}</t></is></c></row>"#
                )?;
            }
            write!(out, "</sheetData></worksheet>")
        })],
        &[],
        false,
    )
}

/// Reads `bytes` and says how much memory that took at most, in KiB, after
/// forgetting the peak of everything before.
#[cfg(target_os = "linux")]
fn read_measured(bytes: &[u8], options: &ReadOptions<'_>) -> (Result<Table, SheetsError>, u64) {
    std::fs::write("/proc/self/clear_refs", "5").expect("clear_refs");
    let before = kib_of("VmHWM:");
    let result = read_table(bytes, "book.xlsx", options);
    let peak = kib_of("VmHWM:");
    (result, peak.saturating_sub(before))
}

/// Runs alone in a process (see the test below), so that nothing else's
/// memory is counted: reads a workbook of 800,000 cells that a limit of ten
/// rows refuses, and checks how much memory that took at most.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "runs in a process of its own, from the test below"]
fn the_peak_memory_of_reading_a_workbook_over_its_limits() {
    let bytes = long_book(false);
    let (result, used) = read_measured(&bytes, &limits(10));
    assert!(
        matches!(result, Err(SheetsError::TooManyRows { limit: 10 })),
        "{result:?}"
    );
    assert!(
        used < 24 * 1024,
        "reading held {used} KiB at most, over a bound of 24 MiB"
    );
}

/// The same, with a sheet that is not in order.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "runs in a process of its own, from the test below"]
fn the_peak_memory_of_reading_an_unordered_workbook_over_its_row_limit() {
    let bytes = long_book(true);
    let (result, used) = read_measured(&bytes, &limits(10));
    assert!(
        matches!(result, Err(SheetsError::TooManyRows { limit: 10 })),
        "{result:?}"
    );
    assert!(
        used < 24 * 1024,
        "reading held {used} KiB at most, over a bound of 24 MiB"
    );
}

/// And under a cell limit that the rows do not reach first.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "runs in a process of its own, from the test below"]
fn the_peak_memory_of_reading_an_unordered_workbook_over_its_cell_limit() {
    let bytes = long_book(true);
    let options = ReadOptions {
        max_cells: 100,
        ..limits(1_000_000)
    };
    let (result, used) = read_measured(&bytes, &options);
    assert!(refused_for(&result, "cells"), "{result:?}");
    assert!(
        used < 24 * 1024,
        "reading held {used} KiB at most, over a bound of 24 MiB"
    );
}

/// Text in columns past the column limit is not kept.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "runs in a process of its own, from the test below"]
fn the_peak_memory_of_reading_an_unordered_workbook_with_text_past_the_column_limit() {
    let bytes = far_text_book();
    let options = ReadOptions {
        max_columns: 5,
        ..limits(10_000)
    };
    let (result, used) = read_measured(&bytes, &options);
    let table = result.unwrap();
    assert_eq!(table.headers, ["a", "b"]);
    assert_eq!(table.rows.len(), 3_999);
    assert!(
        used < 12 * 1024,
        "reading held {used} KiB at most, over a bound of 12 MiB; the text is 40,000"
    );
}

/// Runs one of the tests above, alone in a process.
#[cfg(target_os = "linux")]
fn run_alone(test: &str) {
    let this = module_path!().split_once("::").unwrap().1;
    // Runs this test binary again, for one test, so that the peak memory it
    // measures is that test's own and no other test's.
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
    // That it ran, not that it was filtered out.
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[cfg(target_os = "linux")]
#[test]
fn a_workbook_is_not_held_in_memory_before_its_limits_apply() {
    run_alone("the_peak_memory_of_reading_a_workbook_over_its_limits");
}

#[cfg(target_os = "linux")]
#[test]
fn a_workbook_out_of_order_is_not_held_in_memory_before_its_limits_apply() {
    run_alone("the_peak_memory_of_reading_an_unordered_workbook_over_its_row_limit");
    run_alone("the_peak_memory_of_reading_an_unordered_workbook_over_its_cell_limit");
    run_alone("the_peak_memory_of_reading_an_unordered_workbook_with_text_past_the_column_limit");
}

/// `count` rows of `width` numbers, the first row's first two cells the
/// other way round: a sheet that is not in order.
fn unordered_rows(count: u32, width: u8) -> String {
    let column = |c: u8| (b'A' + c) as char;
    let mut rows = row(1, &[text_at("B1", "second"), text_at("A1", "first")]);
    for r in 2..=count {
        let cells: Vec<String> = (0..width)
            .map(|c| number_at(&format!("{}{r}", column(c)), "1"))
            .collect();
        rows.push_str(&row(r, &cells));
    }
    rows
}

#[test]
fn an_unordered_sheet_over_a_limit_is_refused_as_an_ordered_one_is() {
    let rows = unordered_rows(40, 2);
    let bytes = book(&rows, "", &[]);
    let result = read_table(&bytes, "book.xlsx", &limits(5));
    assert!(
        matches!(result, Err(SheetsError::TooManyRows { limit: 5 })),
        "{result:?}"
    );
    let result = read_table(
        &bytes,
        "book.xlsx",
        &ReadOptions {
            max_cells: 30,
            ..limits(100)
        },
    );
    assert!(refused_for(&result, "more than 30 cells"), "{result:?}");
    // Within the limits it is read.
    let table = read_table(&bytes, "book.xlsx", &limits(39)).unwrap();
    assert_eq!(table.rows.len(), 39);
    assert_eq!(table.headers, ["first", "second"]);
}

#[test]
fn an_unordered_sheet_over_a_limit_is_still_unreadable_when_it_is_cut_short() {
    let bytes = unfinished_book(&unordered_rows(40, 2));
    for options in [limits(5), limits(1_000)] {
        let result = read_table(&bytes, "book.xlsx", &options);
        assert!(refused_for(&result, "cannot be read"), "{result:?}");
    }
}

#[test]
fn text_past_the_column_limit_of_an_unordered_sheet_still_makes_its_row_a_row() {
    // Row 3 has only a cell past the column limit: it is a row, empty in
    // every column the table keeps, as it is when the sheet is in order.
    let rows = [
        row(1, &[text_at("B1", "b"), text_at("A1", "a")]),
        row(2, &[number_at("A2", "1"), number_at("B2", "2")]),
        row(3, &[text_at("H3", "far")]),
        row(4, &[number_at("A4", "3")]),
    ]
    .concat();
    let bytes = book(&rows, "", &[]);
    let options = ReadOptions {
        max_columns: 4,
        ..limits(10)
    };
    let table = read_table(&bytes, "book.xlsx", &options).unwrap();
    let numbers: Vec<u64> = table.rows.iter().map(|r| r.number).collect();
    assert_eq!(numbers, [2, 3, 4]);
    assert_eq!(table.rows[1].cells, [Cell::Empty, Cell::Empty]);
    assert_read_as_a_range("far text, unordered", &bytes);
}

/// A workbook whose shared strings are one text of `len` bytes, read by a
/// header cell.
fn book_with_shared_text(len: usize) -> Vec<u8> {
    let text = "x".repeat(len);
    book(
        &row(1, &[r#"<c r="A1" t="s"><v>0</v></c>"#.into()]),
        "",
        &[&text],
    )
}

/// The same bytes with the size the zip file declares for `part` changed.
fn declaring(bytes: &[u8], part: &str, size: u32) -> Vec<u8> {
    let mut bytes = bytes.to_vec();
    let mut at = 0;
    let mut changed = 0;
    while let Some(found) = bytes[at..].windows(4).position(|w| w == b"PK\x01\x02") {
        let header = at + found;
        let name_length = u16::from_le_bytes([bytes[header + 28], bytes[header + 29]]) as usize;
        let name = &bytes[header + 46..header + 46 + name_length];
        if name == part.as_bytes() {
            bytes[header + 24..header + 28].copy_from_slice(&size.to_le_bytes());
            changed += 1;
        }
        at = header + 4;
    }
    assert_eq!(changed, 1, "{part} is in the central directory once");
    bytes
}

#[test]
fn a_part_that_unpacks_past_the_limit_is_refused_before_it_is_read() {
    // 2 MiB of text that is 2 KB in the file.
    let bytes = book_with_shared_text(2 * 1024 * 1024);
    assert!(bytes.len() < 20_000, "{} bytes", bytes.len());
    let options = |max_inflated_bytes| ReadOptions {
        max_inflated_bytes,
        ..limits(10)
    };
    let result = read_table(&bytes, "book.xlsx", &options(1024 * 1024));
    assert!(
        refused_for(&result, "unpacks to more than 1048576 bytes"),
        "{result:?}"
    );
    // Within the limit it is read: a header of that text, and no rows.
    let table = read_table(&bytes, "book.xlsx", &options(4 * 1024 * 1024)).unwrap();
    assert_eq!(table.headers[0].len(), 2 * 1024 * 1024);
}

#[test]
fn the_sheet_part_is_held_to_the_same_limit() {
    let rows = unordered_rows(40_000, 4);
    let bytes = book(&rows, "", &[]);
    let result = read_table(
        &bytes,
        "book.xlsx",
        &ReadOptions {
            max_inflated_bytes: 1024 * 1024,
            ..limits(100_000)
        },
    );
    assert!(refused_for(&result, "unpacks to more than"), "{result:?}");
}

#[test]
fn a_part_that_declares_more_than_it_unpacks_to_is_refused() {
    let bytes = declaring(
        &book_with_shared_text(1000),
        "xl/sharedStrings.xml",
        0x7fff_ffff,
    );
    let result = read_table(&bytes, "book.xlsx", &limits(10));
    assert!(refused_for(&result, "unpacks to more than"), "{result:?}");
}

#[test]
fn a_part_that_declares_less_than_it_unpacks_to_cannot_be_read() {
    // The zip reader stops a part at its declared size, so the declared size
    // bounds what is read whatever the part holds.
    let bytes = declaring(
        &book_with_shared_text(100_000),
        "xl/sharedStrings.xml",
        1000,
    );
    let result = read_table(&bytes, "book.xlsx", &limits(10));
    assert!(refused_for(&result, "cannot be read"), "{result:?}");
}

/// `bytes` behind a 512-byte OLE header whose sector shift is invalid: the
/// XLS reader refuses it at once, and detection goes on to the zip formats,
/// which find the archive after the prefix.
fn behind_an_ole_header(bytes: &[u8]) -> Vec<u8> {
    let mut prefixed = OLE.to_vec();
    prefixed.resize(512, 0);
    prefixed.extend_from_slice(bytes);
    prefixed
}

#[test]
fn a_zip_workbook_behind_an_ole_header_is_held_to_the_unpacked_size_limit() {
    let bytes = book_with_shared_text(2 * 1024 * 1024);
    let options = ReadOptions {
        max_inflated_bytes: 1024 * 1024,
        ..limits(10)
    };
    for file in [bytes.clone(), behind_an_ole_header(&bytes)] {
        let result = read_table(&file, "book.xlsx", &options);
        assert!(
            refused_for(&result, "unpacks to more than 1048576 bytes"),
            "{result:?}"
        );
    }
    // Within the limit the prefixed file is the workbook it holds.
    let table = read_table(
        &behind_an_ole_header(&bytes),
        "book.xlsx",
        &ReadOptions {
            max_inflated_bytes: 4 * 1024 * 1024,
            ..limits(10)
        },
    )
    .unwrap();
    assert_eq!(table.kind, FileKind::Xlsx);
    assert_eq!(table.headers[0].len(), 2 * 1024 * 1024);
}

/// The default limits, but for the text a file may hold.
fn text_limit(max_text_bytes: u64) -> ReadOptions<'static> {
    ReadOptions {
        max_text_bytes,
        ..limits(1_000)
    }
}

/// Headers `alpha` and `beta` (9 bytes) over three rows of `xx` and ` yyy `
/// (15 bytes once trimmed): 24 bytes of text in the table.
const TEXT_IN_THE_TABLE: u64 = 24;

fn assert_text_limit(label: &str, file: &[u8], name: &str) {
    let table = read_table(file, name, &text_limit(TEXT_IN_THE_TABLE)).unwrap();
    assert_eq!(table.rows.len(), 3, "{label}");
    let result = read_table(file, name, &text_limit(TEXT_IN_THE_TABLE - 1));
    assert!(
        refused_for(&result, "The file holds more than 23 bytes of text."),
        "{label}: {result:?}"
    );
}

#[test]
fn the_text_a_table_keeps_is_counted_up_to_the_byte() {
    assert_text_limit(
        "csv",
        b"alpha,beta\nxx, yyy \nxx, yyy \nxx, yyy \n",
        "x.csv",
    );
    let mut rows = row(1, &[text_at("A1", "alpha"), text_at("B1", "beta")]);
    for r in 2..=4 {
        rows.push_str(&row(
            r,
            &[
                text_at(&format!("A{r}"), "xx"),
                text_at(&format!("B{r}"), " yyy "),
            ],
        ));
    }
    assert_text_limit("xlsx in order", &book(&rows, "", &[]), "x.xlsx");
    // The same cells, the first row's the other way round.
    let mut rows = row(1, &[text_at("B1", "beta"), text_at("A1", "alpha")]);
    for r in 2..=4 {
        rows.push_str(&row(
            r,
            &[
                text_at(&format!("A{r}"), "xx"),
                text_at(&format!("B{r}"), " yyy "),
            ],
        ));
    }
    assert_text_limit("xlsx out of order", &book(&rows, "", &[]), "x.xlsx");
}

#[test]
fn text_from_shared_strings_counts_for_every_cell_it_is_in() {
    // One text of 10 bytes, in the header and three rows.
    let mut rows = row(1, &[r#"<c r="A1" t="s"><v>0</v></c>"#.into()]);
    for r in 2..=4 {
        rows.push_str(&row(r, &[format!(r#"<c r="A{r}" t="s"><v>0</v></c>"#)]));
    }
    let bytes = book(&rows, "", &["0123456789"]);
    assert!(read_table(&bytes, "x.xlsx", &text_limit(40)).is_ok());
    let result = read_table(&bytes, "x.xlsx", &text_limit(39));
    assert!(
        refused_for(&result, "more than 39 bytes of text"),
        "{result:?}"
    );
}

#[test]
fn text_past_the_last_header_or_in_numbers_is_not_counted() {
    // Beyond the header's width the cells are cut, and numbers hold no text.
    let rows = [
        row(1, &[text_at("A1", "ab")]),
        row(
            2,
            &[
                number_at("A2", "5"),
                text_at("B2", "a very long text indeed"),
            ],
        ),
    ]
    .concat();
    let bytes = book(&rows, "", &[]);
    assert!(read_table(&bytes, "x.xlsx", &text_limit(2)).is_ok());
}

/// One 64 KiB shared string, referenced by 1,499 rows; each cell would hold
/// its own copy. With `swap`, the header lists its cells the other way round.
#[cfg(target_os = "linux")]
fn shared_text_book(swap: bool) -> Vec<u8> {
    let text = "x".repeat(64 * 1024);
    let mut rows = if swap {
        row(1, &[text_at("B1", "h2"), text_at("A1", "h")])
    } else {
        row(1, &[text_at("A1", "h")])
    };
    for r in 2..=1500u32 {
        rows.push_str(&row(r, &[format!(r#"<c r="A{r}" t="s"><v>0</v></c>"#)]));
    }
    book(&rows, "", &[&text])
}

#[cfg(target_os = "linux")]
fn peak_of_shared_text(swap: bool) {
    let bytes = shared_text_book(swap);
    assert!(bytes.len() < 100_000, "{} bytes", bytes.len());
    let options = ReadOptions {
        max_text_bytes: 1024 * 1024,
        ..limits(100_000)
    };
    let (result, used) = read_measured(&bytes, &options);
    assert!(
        refused_for(&result, "bytes of text"),
        "{:?} after {used} KiB",
        result.as_ref().map(|table| table.rows.len())
    );
    assert!(
        used < 24 * 1024,
        "reading held {used} KiB at most, over a bound of 24 MiB; the copies are 96,000"
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "runs in a process of its own, from the test below"]
fn the_peak_memory_of_reading_one_shared_string_in_many_cells() {
    peak_of_shared_text(false);
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "runs in a process of its own, from the test below"]
fn the_peak_memory_of_reading_one_shared_string_in_many_cells_out_of_order() {
    peak_of_shared_text(true);
}

#[cfg(target_os = "linux")]
#[test]
fn a_shared_string_in_many_cells_is_not_copied_past_the_text_limit() {
    run_alone("the_peak_memory_of_reading_one_shared_string_in_many_cells");
    run_alone("the_peak_memory_of_reading_one_shared_string_in_many_cells_out_of_order");
}

/// A workbook whose one shared string is `text`, used by `cells` cells under
/// a header. With `tail`, a cell that comes before the
/// ones before it ends the sheet, so that it is not in order.
fn book_of_one_shared_string(text: &str, cells: u32, tail: bool) -> Vec<u8> {
    let mut rows = row(1, &[text_at("A1", "h")]);
    for r in 2..=cells + 1 {
        rows.push_str(&row(r, &[format!(r#"<c r="A{r}" t="s"><v>0</v></c>"#)]));
    }
    if tail {
        rows.push_str(&row(1, &[text_at("B1", "b")]));
    }
    book(&rows, "", &[text])
}

#[test]
fn a_padded_shared_string_costs_what_its_text_costs_in_every_cell() {
    // 256 KiB of padding round one character, or round nothing, in 100 cells.
    // Scanning and copying all of it for every cell takes seconds in a debug
    // build (25 MB of text); trimmed once it takes milliseconds.
    let padding = " ".repeat(256 * 1024);
    for (label, text) in [
        ("padded", format!("{padding}x{padding}")),
        ("blank", padding.clone()),
        ("plain", "x".to_owned()),
    ] {
        for tail in [false, true] {
            let bytes = book_of_one_shared_string(&text, 100, tail);
            let started = std::time::Instant::now();
            let result = read_table(&bytes, "x.xlsx", &limits(1_000));
            let took = started.elapsed();
            let table = result.unwrap_or_else(|error| panic!("{label}: {error}"));
            let rows = if label == "blank" { 0 } else { 100 };
            assert_eq!(table.rows.len(), rows, "{label}");
            if rows > 0 {
                assert_eq!(table.rows[0].cells[0], Cell::Text("x".into()), "{label}");
            }
            assert!(
                took < std::time::Duration::from_millis(400),
                "{label} (out of order: {tail}): 101 cells of {} bytes took {took:?}",
                text.len()
            );
        }
    }
}

#[test]
fn error_cells_count_as_the_text_they_are_kept_as() {
    // `#DIV/0!` is kept as `#Div0`: 5 bytes, after the 1 byte header.
    let mut rows = row(1, &[text_at("A1", "h")]);
    for r in 2..=4 {
        rows.push_str(&row(
            r,
            &[format!(r#"<c r="A{r}" t="e"><v>#DIV/0!</v></c>"#)],
        ));
    }
    let in_order = book(&rows, "", &[]);
    rows.push_str(&row(1, &[text_at("B1", "b")]));
    let out_of_order = book(&rows, "", &[]);
    for (label, bytes, header) in [("in order", in_order, 1), ("out of order", out_of_order, 2)] {
        let kept = header + 3 * 5;
        let table = read_table(&bytes, "x.xlsx", &text_limit(kept)).unwrap();
        assert_eq!(table.rows.len(), 3, "{label}");
        assert_eq!(
            table.rows[0].cells[0],
            Cell::Text("#Div0".into()),
            "{label}"
        );
        let result = read_table(&bytes, "x.xlsx", &text_limit(kept - 1));
        assert!(
            refused_for(&result, "bytes of text"),
            "{label}: {:?}",
            result.map(|table| table.rows.len())
        );
    }
}

/// 1,500 cells of one 16 KiB shared string (24 MiB of text in the table),
/// with a cell at the end that is out of order.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "runs in a process of its own, from the test below"]
fn the_peak_memory_of_reading_a_sheet_that_turns_out_of_order() {
    let text = "x".repeat(16 * 1024);
    let bytes = book_of_one_shared_string(&text, 1_499, true);
    let (result, used) = read_measured(&bytes, &limits(100_000));
    let table = result.unwrap();
    assert_eq!(table.rows.len(), 1_499);
    assert!(
        used < 36 * 1024,
        "reading held {used} KiB at most, over a bound of 36 MiB; the table is 24,000 and a second one 24,000 more"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_first_reading_is_given_up_when_the_sheet_is_read_again() {
    run_alone("the_peak_memory_of_reading_a_sheet_that_turns_out_of_order");
}

/// An ODS spreadsheet whose `office:spreadsheet` holds `tables`.
fn ods_of(tables: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/vnd.oasis.opendocument.spreadsheet")
        .unwrap();
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("META-INF/manifest.xml", options).unwrap();
    zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#).unwrap();
    zip.start_file("content.xml", options).unwrap();
    write!(
        zip,
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.3"><office:body><office:spreadsheet>{tables}</office:spreadsheet></office:body></office:document-content>"#
    )
    .unwrap();
    zip.finish().unwrap().into_inner()
}

/// An ODS text cell.
fn ods_text(text: &str) -> String {
    format!(
        r#"<table:table-cell office:value-type="string"><text:p>{text}</text:p></table:table-cell>"#
    )
}

/// An ODS number cell.
fn ods_number(number: &str) -> String {
    format!(r#"<table:table-cell office:value-type="float" office:value="{number}"/>"#)
}

/// `count` empty ODS cells.
fn ods_gap(count: u64) -> String {
    format!(r#"<table:table-cell table:number-columns-repeated="{count}"/>"#)
}

/// An ODS row of `cells`, repeated `times`.
fn ods_row(times: u64, cells: &str) -> String {
    format!(r#"<table:table-row table:number-rows-repeated="{times}">{cells}</table:table-row>"#)
}

fn ods_table(name: &str, rows: &str) -> String {
    format!(r#"<table:table table:name="{name}">{rows}</table:table>"#)
}

/// An XLSB workbook of one sheet whose rows hold `cells`: row, column and
/// value of each, rows in order.
fn xlsb_of(cells: &[(u32, u32, crate::import::workbook::xlsb_writer::Value)]) -> Vec<u8> {
    use crate::import::workbook::xlsb_writer::{cell, row, sheet, workbook};
    let mut records = Vec::new();
    let mut last = None;
    for (r, c, value) in cells {
        if last != Some(*r) {
            records.push(row(*r));
            last = Some(*r);
        }
        records.push(cell(*c, 0, value));
    }
    workbook(&[("Sheet1", Some(sheet(&records)))], &[], &[], &[0], false)
}

/// Cells at A1 and at row 20,001, column 161: a range of the area between
/// them holds 3.2 million cells, about 100 MB. In the ODS file, the empty
/// rows between them are one row element, repeated.
#[cfg(target_os = "linux")]
fn far_apart(ods: bool) -> (Vec<u8>, &'static str) {
    if ods {
        let rows = [
            ods_row(1, &ods_text("h")),
            ods_row(20_000, &ods_gap(161)),
            ods_row(1, &format!("{}{}", ods_gap(160), ods_number("1"))),
        ]
        .concat();
        (ods_of(&ods_table("Sheet1", &rows)), "far.ods")
    } else {
        use crate::import::workbook::xlsb_writer::Value;
        let bytes = xlsb_of(&[
            (0, 0, Value::Text("h".into())),
            (20_001, 160, Value::RkInt(1, false)),
        ]);
        (bytes, "far.xlsb")
    }
}

#[cfg(target_os = "linux")]
fn peak_of_cells_far_apart(ods: bool) {
    let (bytes, name) = far_apart(ods);
    assert!(bytes.len() < 20_000, "{} bytes", bytes.len());
    std::fs::write("/proc/self/clear_refs", "5").expect("clear_refs");
    let before = kib_of("VmHWM:");
    let result = read_table(&bytes, name, &limits(100));
    let used = kib_of("VmHWM:").saturating_sub(before);
    let table = result.unwrap();
    assert_eq!(table.headers, ["h"]);
    // The far row is a row, empty in the header's one column.
    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].number, 20_002);
    assert!(
        used < 24 * 1024,
        "reading held {used} KiB at most, over a bound of 24 MiB; the area is 100,000"
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "runs in a process of its own, from the test below"]
fn the_peak_memory_of_reading_xlsb_cells_far_apart() {
    peak_of_cells_far_apart(false);
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "runs in a process of its own, from the test below"]
fn the_peak_memory_of_reading_ods_cells_far_apart() {
    peak_of_cells_far_apart(true);
}

#[cfg(target_os = "linux")]
#[test]
fn cells_far_apart_take_memory_for_the_cells_not_the_area() {
    run_alone("the_peak_memory_of_reading_xlsb_cells_far_apart");
    run_alone("the_peak_memory_of_reading_ods_cells_far_apart");
}

/// `bytes` with the part `name` holding `content` instead.
fn replacing(bytes: &[u8], name: &str, content: impl AsRef<[u8]>) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for index in 0..archive.len() {
        let mut part = archive.by_index(index).unwrap();
        let part_name = part.name().unwrap().into_owned();
        zip.start_file(part_name.as_str(), options).unwrap();
        if part_name == name {
            zip.write_all(content.as_ref()).unwrap();
        } else {
            std::io::copy(&mut part, &mut zip).unwrap();
        }
    }
    zip.finish().unwrap().into_inner()
}

/// One shared string that its part says is one of twenty million.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "runs in a process of its own, from the test below"]
fn the_peak_memory_of_reading_shared_strings_that_claim_millions() {
    let bytes = replacing(
        &book(
            &row(1, &[r#"<c r="A1" t="s"><v>0</v></c>"#.into()]),
            "",
            &["h"],
        ),
        "xl/sharedStrings.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><sst xmlns="{NS}" count="20000000" uniqueCount="20000000"><si><t>h</t></si></sst>"#
        ),
    );
    // Room reserved is not memory used until it is written: the measure is
    // of the address space, whose peak the process cannot forget.
    let before = kib_of("VmPeak:");
    let result = read_table(&bytes, "book.xlsx", &limits(10));
    let reserved = kib_of("VmPeak:").saturating_sub(before);
    assert_eq!(result.unwrap().headers, ["h"]);
    assert!(
        reserved < 64 * 1024,
        "reading took {reserved} KiB more address space, over a bound of 64 MiB; room for twenty million strings is 469,000"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn the_count_of_shared_strings_a_file_declares_reserves_nothing() {
    run_alone("the_peak_memory_of_reading_shared_strings_that_claim_millions");
}

/// A cell of `a`, `spaces` spaces (one `text:s`) and `then`, under a
/// header.
#[cfg(target_os = "linux")]
fn spaced_ods(spaces: u64, then: &str) -> Vec<u8> {
    let cell = format!(
        r#"<table:table-cell office:value-type="string"><text:p>a<text:s text:c="{spaces}"/>{then}</text:p></table:table-cell>"#
    );
    ods_of(&ods_table(
        "Sheet1",
        &[ods_row(1, &ods_text("h")), ods_row(1, &cell)].concat(),
    ))
}

/// Fifty million spaces between two letters: refused by the text limit
/// before they are made.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "runs in a process of its own, from the test below"]
fn the_peak_memory_of_reading_an_ods_cell_of_many_spaces() {
    let options = ReadOptions {
        max_text_bytes: 1024 * 1024,
        ..limits(10)
    };
    let (result, used) = read_measured(&spaced_ods(50_000_000, "b"), &options);
    assert!(
        refused_for(&result, "more than 1048576 bytes of text"),
        "{result:?}"
    );
    assert!(
        used < 24 * 1024,
        "reading held {used} KiB at most, over a bound of 24 MiB; the spaces are 49,000"
    );
    // Spaces that end a cell are trimmed, so never made.
    let (result, used) = read_measured(&spaced_ods(50_000_000, ""), &options);
    assert_eq!(result.unwrap().rows[0].cells, [Cell::Text("a".into())]);
    assert!(
        used < 24 * 1024,
        "reading held {used} KiB at most, over a bound of 24 MiB; the spaces are 49,000"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn an_ods_cell_that_repeats_a_space_makes_no_more_text_than_the_limit() {
    run_alone("the_peak_memory_of_reading_an_ods_cell_of_many_spaces");
}

#[test]
fn an_ods_spreadsheet_is_read_by_the_sheet_asked_for() {
    let first = ods_table(
        "First",
        &[
            ods_row(2, ""),
            ods_row(1, &[ods_gap(1), ods_text("a"), ods_text("b")].concat()),
            ods_row(
                1,
                &[ods_gap(1), ods_number("1.5"), ods_number("2")].concat(),
            ),
        ]
        .concat(),
    );
    let twice = ods_table("Twice", &ods_row(1, &ods_text("first of two")));
    let again = ods_table("Twice", &ods_row(1, &ods_text("second of two")));
    let bytes = ods_of(&[first, twice, again].concat());
    let table = read_table(&bytes, "book.ods", &limits(10)).unwrap();
    assert_eq!(table.kind, FileKind::Ods);
    assert_eq!(table.headers, ["Column 1", "a", "b"]);
    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].number, 4);
    assert_eq!(
        table.rows[0].cells,
        [Cell::Empty, Cell::Float(1.5), Cell::Float(2.0)]
    );
    // Two tables of one name are one sheet: the last.
    let table = read_table(
        &bytes,
        "book.ods",
        &ReadOptions {
            sheet: Some("Twice"),
            ..limits(10)
        },
    )
    .unwrap();
    assert_eq!(table.headers, ["second of two"]);
    let result = read_table(
        &bytes,
        "book.ods",
        &ReadOptions {
            sheet: Some("Third"),
            ..limits(10)
        },
    );
    assert!(refused_for(&result, "no sheet named `Third`"), "{result:?}");
}

#[test]
fn repeated_ods_rows_and_cells_are_rows_and_cells() {
    let rows = [
        ods_row(1, &[ods_text("a"), ods_text("b"), ods_text("c")].concat()),
        ods_row(3, r#"<table:table-cell table:number-columns-repeated="2" office:value-type="float" office:value="7"/>"#),
        ods_row(1_000, &ods_gap(1_024)),
        ods_row(1, &[ods_gap(2), ods_text("last")].concat()),
        // What spreadsheets write after the last row.
        ods_row(1_047_570, &ods_gap(16_384)),
    ]
    .concat();
    let bytes = ods_of(&ods_table("Sheet1", &rows));
    let table = read_table(&bytes, "book.ods", &limits(10)).unwrap();
    let numbers: Vec<u64> = table.rows.iter().map(|r| r.number).collect();
    assert_eq!(numbers, [2, 3, 4, 1_005]);
    for row in &table.rows[..3] {
        assert_eq!(row.cells, [Cell::Float(7.0), Cell::Float(7.0), Cell::Empty]);
    }
    assert_eq!(
        table.rows[3].cells,
        [Cell::Empty, Cell::Empty, Cell::Text("last".into())]
    );
    assert_read_as_a_range("repeats", &bytes);
}

#[test]
fn an_ods_row_repeated_a_million_times_is_refused_at_the_row_limit_at_once() {
    let rows = [
        ods_row(1, &ods_text("h")),
        ods_row(1_000_000, &ods_number("1")),
    ]
    .concat();
    let bytes = ods_of(&ods_table("Sheet1", &rows));
    let started = std::time::Instant::now();
    let result = read_table(&bytes, "book.ods", &limits(100));
    assert!(
        matches!(result, Err(SheetsError::TooManyRows { limit: 100 })),
        "{result:?}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "took {:?}",
        started.elapsed()
    );
    // Within the limit, every copy is a row.
    let rows = [
        ods_row(1, &ods_text("h")),
        ods_row(50_000, &ods_number("1")),
    ]
    .concat();
    let bytes = ods_of(&ods_table("Sheet1", &rows));
    let table = read_table(&bytes, "book.ods", &limits(50_000)).unwrap();
    assert_eq!(table.rows.len(), 50_000);
    assert_eq!(table.rows[49_999].number, 50_001);
}

#[test]
fn an_ods_cell_repeated_past_the_last_column_stops_there() {
    let cell = r#"<table:table-cell table:number-columns-repeated="1099511627776" office:value-type="string" office:string-value="x"/>"#;
    let bytes = ods_of(&ods_table("Sheet1", &ods_row(1, cell)));
    let started = std::time::Instant::now();
    let table = read_table(&bytes, "book.ods", &limits(10)).unwrap();
    assert_eq!(table.headers.len(), 16_384);
    assert_eq!(table.headers[16_383], "x (16384)");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    let result = read_table(
        &bytes,
        "book.ods",
        &ReadOptions {
            max_columns: 100,
            ..limits(10)
        },
    );
    assert!(refused_for(&result, "more than 100 columns"), "{result:?}");
}

#[test]
fn a_wide_ods_row_repeated_past_a_narrow_header_costs_the_header_s_width() {
    // 16,384 values in a row, 20,000 times, under a header of one column:
    // 300 million cells, of which the table keeps 20,000.
    let wide: String = (0..16_384).map(|i| ods_number(&i.to_string())).collect();
    let rows = [ods_row(1, &ods_text("h")), ods_row(20_000, &wide)].concat();
    let bytes = ods_of(&ods_table("Sheet1", &rows));
    let started = std::time::Instant::now();
    let table = read_table(&bytes, "book.ods", &limits(100_000)).unwrap();
    assert_eq!(table.rows.len(), 20_000);
    assert_eq!(table.rows[19_999].cells, [Cell::Float(0.0)]);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "took {:?}",
        started.elapsed()
    );
}

#[test]
fn ods_text_is_its_paragraphs_and_spaces() {
    let cells = [
        r#"<table:table-cell office:value-type="string"><text:p>first</text:p><text:p>sec<text:span>ond</text:span></text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="string"><text:p>a<text:s text:c="3"/>b<text:s/>c</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="string"><office:annotation><text:p>a note</text:p></office:annotation><text:p>noted</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="string" office:string-value="the attribute"><text:p>not this</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="date" office:date-value="2026-03-01"><text:p>01/03/26</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="boolean" office:boolean-value="true"><text:p>TRUE</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="percentage" office:value="0.25"><text:p>25%</text:p></table:table-cell>"#,
        r#"<table:covered-table-cell office:value-type="string"><text:p>covered</text:p></table:covered-table-cell>"#,
        r#"<table:table-cell office:value-type="string"><text:p>&lt;tag&gt; &amp; &#233;</text:p></table:table-cell>"#,
    ]
    .concat();
    let header: String = (0..9).map(|i| ods_text(&format!("h{i}"))).collect();
    let rows = [ods_row(1, &header), ods_row(1, &cells)].concat();
    let table = read_table(
        &ods_of(&ods_table("Sheet1", &rows)),
        "book.ods",
        &limits(10),
    )
    .unwrap();
    assert_eq!(
        table.rows[0].cells,
        [
            Cell::Text("first\nsecond".into()),
            Cell::Text("a   b c".into()),
            Cell::Text("noted".into()),
            Cell::Text("the attribute".into()),
            Cell::Date(chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()),
            Cell::Bool(true),
            Cell::Float(0.25),
            Cell::Text("covered".into()),
            Cell::Text("<tag> & é".into()),
        ]
    );
}

#[test]
fn an_xlsb_workbook_is_read_as_an_excel_workbook() {
    use crate::import::workbook::xlsb_writer::Value;
    let bytes = xlsb_of(&[
        (0, 0, Value::Text("Name".into())),
        (0, 1, Value::Text("Age".into())),
        (1, 0, Value::Text(" Anu ".into())),
        (1, 1, Value::RkInt(36, false)),
    ]);
    let table = read_table(&bytes, "people.xlsb", &limits(10)).unwrap();
    assert_eq!(table.kind, FileKind::Xlsx);
    assert_eq!(table.headers, ["Name", "Age"]);
    assert_eq!(
        table.rows[0].cells,
        [Cell::Text("Anu".into()), Cell::Int(36)]
    );
}

#[test]
fn damaged_workbooks_are_refused_as_unreadable() {
    use crate::import::workbook::xlsb_writer::{part, sheet, workbook};
    let ods = |content: &str| ods_of(&ods_table("Sheet1", content));
    let xlsb_sheet = |records: &[(u16, Vec<u8>)]| {
        workbook(&[("Sheet1", Some(sheet(records)))], &[], &[], &[0], false)
    };
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("an ODS row of text", ods(&ods_row(1, "<text:p>x</text:p>"))),
        (
            "an ODS count that is not one",
            ods(&ods_row(
                1,
                r#"<table:table-cell table:number-columns-repeated="many"/>"#,
            )),
        ),
        (
            "an ODS number that is not one",
            ods(&ods_row(
                1,
                r#"<table:table-cell office:value-type="float" office:value="one"/>"#,
            )),
        ),
        ("an ODS table not closed", {
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("mimetype", options).unwrap();
            zip.write_all(b"application/vnd.oasis.opendocument.spreadsheet")
                .unwrap();
            zip.start_file("META-INF/manifest.xml", options).unwrap();
            zip.write_all(b"<m/>").unwrap();
            zip.start_file("content.xml", options).unwrap();
            zip.write_all(br#"<a><table:table table:name="S"><table:table-row>"#)
                .unwrap();
            zip.finish().unwrap().into_inner()
        }),
        (
            "an XLSB cell cut short",
            xlsb_sheet(&[(0x00, vec![0; 4]), (0x05, vec![0; 6])]),
        ),
        (
            "an XLSB shared string that is missing",
            xlsb_sheet(&[
                (0x00, vec![0; 4]),
                (
                    0x07,
                    [0u8; 8]
                        .iter()
                        .chain(&9u32.to_le_bytes())
                        .copied()
                        .collect(),
                ),
            ]),
        ),
        ("an XLSB sheet without its end", {
            let mut bytes = xlsb_sheet(&[]);
            bytes = replacing(&bytes, "xl/worksheets/sheet1.bin", "");
            bytes
        }),
        ("an XLSB record longer than its part", {
            let records = part(&[(0x81, vec![]), (0x94, vec![0; 16]), (0x91, vec![])]);
            let mut cut = records.clone();
            cut.extend_from_slice(&[0x05, 0x7F, 1, 2]);
            replacing(&xlsb_sheet(&[]), "xl/worksheets/sheet1.bin", &cut)
        }),
    ];
    for (label, bytes) in cases {
        let result = read_table(&bytes, "book", &limits(10));
        assert!(
            refused_for(&result, "cannot be read"),
            "{label}: {result:?}"
        );
    }
}

#[test]
fn xlsx_errors_of_any_name_are_their_text() {
    let errors = [
        "#DIV/0!",
        "#SPILL!",
        "#CALC!",
        "#FIELD!",
        "#BLOCKED!",
        "#CONNECT!",
        "#BUSY!",
        "#UNKNOWN!",
        "#EXTERNAL!",
        "not an error",
    ];
    let column = |index: usize| (b'A' + index as u8) as char;
    let header: Vec<String> = (0..errors.len())
        .map(|i| text_at(&format!("{}1", column(i)), &format!("h{i}")))
        .collect();
    let cells: Vec<String> = errors
        .iter()
        .enumerate()
        .map(|(i, error)| format!(r#"<c r="{}2" t="e"><v>{error}</v></c>"#, column(i)))
        .collect();
    let bytes = book(&[row(1, &header), row(2, &cells)].concat(), "", &[]);
    let table = read_table(&bytes, "book.xlsx", &limits(10)).unwrap();
    let expected: Vec<Cell> = ["#Div0"]
        .iter()
        .chain(&errors[1..])
        .map(|text| Cell::Text((*text).into()))
        .collect();
    assert_eq!(table.rows[0].cells, expected);
}

#[test]
fn xlsb_errors_of_unknown_codes_are_text() {
    use crate::import::workbook::xlsb_writer::Value;
    let bytes = xlsb_of(&[
        (0, 0, Value::Text("a".into())),
        (0, 1, Value::Text("b".into())),
        (1, 0, Value::Error(0x07)),
        (1, 1, Value::Error(0x99)),
    ]);
    let table = read_table(&bytes, "book.xlsb", &limits(10)).unwrap();
    assert_eq!(
        table.rows[0].cells,
        [
            Cell::Text("#Div0".into()),
            Cell::Text("#Error(0x99)".into())
        ]
    );
}

#[test]
fn ods_dates_and_times_are_dates_and_times() {
    let day = |y, m, d| chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap();
    let cases: [(&str, &str, Cell); 9] = [
        ("date", "2026-03-01", Cell::Date(day(2026, 3, 1))),
        (
            "date",
            "2026-03-01T13:45:10",
            Cell::DateTime(day(2026, 3, 1).and_hms_opt(13, 45, 10).unwrap()),
        ),
        (
            "date",
            "2026-03-01T13:45:10.250",
            Cell::DateTime(day(2026, 3, 1).and_hms_milli_opt(13, 45, 10, 250).unwrap()),
        ),
        ("date", "2026-03-01T00:00:00", Cell::Date(day(2026, 3, 1))),
        ("date", "not a date", Cell::Text("not a date".into())),
        // A time is a time on the first day, as a workbook counts it.
        (
            "time",
            "PT06H30M00S",
            Cell::DateTime(day(1899, 12, 31).and_hms_opt(6, 30, 0).unwrap()),
        ),
        (
            "time",
            "PT30H15M00S",
            Cell::DateTime(day(1900, 1, 1).and_hms_opt(6, 15, 0).unwrap()),
        ),
        (
            "time",
            "PT00H00M01.5S",
            Cell::DateTime(day(1899, 12, 31).and_hms_milli_opt(0, 0, 1, 500).unwrap()),
        ),
        ("time", "P1Y", Cell::Text("P1Y".into())),
    ];
    let header: String = (0..cases.len())
        .map(|i| ods_text(&format!("h{i}")))
        .collect();
    let cells: String = cases
        .iter()
        .map(|(kind, value, _)| {
            format!(
                r#"<table:table-cell office:value-type="{kind}" office:{kind}-value="{value}"><text:p>shown</text:p></table:table-cell>"#
            )
        })
        .collect();
    let rows = [ods_row(1, &header), ods_row(1, &cells)].concat();
    let table = read_table(
        &ods_of(&ods_table("Sheet1", &rows)),
        "book.ods",
        &limits(10),
    )
    .unwrap();
    let expected: Vec<Cell> = cases.iter().map(|(_, _, cell)| cell.clone()).collect();
    assert_eq!(table.rows[0].cells, expected);
}

#[test]
fn ods_tabs_and_line_breaks_are_text() {
    let cell = r#"<table:table-cell office:value-type="string"><text:p>a<text:tab/>b<text:line-break/>c<text:s text:c="2"/>d</text:p></table:table-cell>"#;
    let rows = [ods_row(1, &ods_text("h")), ods_row(1, cell)].concat();
    let table = read_table(
        &ods_of(&ods_table("Sheet1", &rows)),
        "book.ods",
        &limits(10),
    )
    .unwrap();
    assert_eq!(table.rows[0].cells, [Cell::Text("a\tb\nc  d".into())]);
}
