//! A small XLSB writer for tests: binary records ([MS-XLSB]) in a zip file,
//! with only what a reader needs (the workbook, its sheets, the shared
//! strings and the cell formats).
//!
//! It depends on nothing but `std` and `zip`.

use std::io::{Cursor, Write};

/// The records of a part: type and body.
pub(crate) type Records = Vec<(u16, Vec<u8>)>;

/// The bytes of `records`: each a type of one or two bytes and a size of
/// one to four, 7 bits to a byte.
pub(crate) fn part(records: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (kind, body) in records {
        if *kind < 0x80 {
            out.push(*kind as u8);
        } else {
            out.push((*kind & 0x7F) as u8 | 0x80);
            out.push((*kind >> 7) as u8);
        }
        let mut size = body.len();
        loop {
            let byte = (size & 0x7F) as u8;
            size >>= 7;
            if size == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
        out.extend_from_slice(body);
    }
    out
}

/// A string as XLSB writes it: its length in UTF-16 units, then the units.
pub(crate) fn wide(text: &str) -> Vec<u8> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut out = (units.len() as u32).to_le_bytes().to_vec();
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

/// A zip file of `parts`, named.
pub(crate) fn zip_of(parts: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in parts {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// A cell's value, as one of the cell records.
#[derive(Clone, Debug)]
pub(crate) enum Value {
    /// `BrtCellRk`: a number as 30 bits (an integer, or the high bits of a
    /// float), and whether it is in hundredths.
    RkInt(i32, bool),
    RkFloat(f64, bool),
    Real(f64),
    Text(String),
    Shared(u32),
    Bool(bool),
    Error(u8),
    Blank,
    FormulaNumber(f64),
    FormulaText(String),
    FormulaBool(bool),
    FormulaError(u8),
}

/// A cell record at `column`, with the cell format `style`.
pub(crate) fn cell(column: u32, style: u32, value: &Value) -> (u16, Vec<u8>) {
    let mut body = column.to_le_bytes().to_vec();
    body.extend_from_slice(&style.to_le_bytes()[..3]);
    body.push(0);
    let kind = match value {
        Value::RkInt(n, hundredths) => {
            let rk = ((*n as u32) << 2) | 2 | u32::from(*hundredths);
            body.extend_from_slice(&rk.to_le_bytes());
            0x02
        }
        Value::RkFloat(n, hundredths) => {
            let high = (n.to_bits() >> 32) as u32 & !3;
            body.extend_from_slice(&(high | u32::from(*hundredths)).to_le_bytes());
            0x02
        }
        Value::Error(code) => {
            body.push(*code);
            0x03
        }
        Value::Bool(b) => {
            body.push(u8::from(*b));
            0x04
        }
        Value::Real(n) => {
            body.extend_from_slice(&n.to_le_bytes());
            0x05
        }
        Value::Text(text) => {
            body.extend_from_slice(&wide(text));
            0x06
        }
        Value::Shared(index) => {
            body.extend_from_slice(&index.to_le_bytes());
            0x07
        }
        Value::Blank => 0x01,
        // A formula's value, then its flags and an empty formula.
        Value::FormulaText(text) => {
            body.extend_from_slice(&wide(text));
            body.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
            0x08
        }
        Value::FormulaNumber(n) => {
            body.extend_from_slice(&n.to_le_bytes());
            body.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
            0x09
        }
        Value::FormulaBool(b) => {
            body.push(u8::from(*b));
            body.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
            0x0A
        }
        Value::FormulaError(code) => {
            body.push(*code);
            body.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
            0x0B
        }
    };
    (kind, body)
}

/// A row header record.
pub(crate) fn row(row: u32) -> (u16, Vec<u8>) {
    let mut body = row.to_le_bytes().to_vec();
    body.extend_from_slice(&[0; 13]);
    (0x00, body)
}

/// A sheet part of `records` (rows and cells).
pub(crate) fn sheet(records: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut all: Records = vec![(0x81, vec![]), (0x93, vec![0; 19])];
    // The dimension: first and last row, first and last column.
    all.push((
        0x94,
        [0u32, 0, 0, 0]
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect(),
    ));
    // Views, which a reader passes over.
    all.push((0x85, vec![]));
    all.push((0x89, vec![0; 30]));
    all.push((0x8A, vec![]));
    all.push((0x86, vec![]));
    all.push((0x91, vec![]));
    all.extend(records.iter().cloned());
    all.push((0x92, vec![]));
    all.push((0x82, vec![]));
    part(&all)
}

/// A workbook: its sheets (name, and part or none for a sheet without
/// one), shared strings, custom number formats (id and code), cell formats
/// (the number format of each) and date system.
pub(crate) fn workbook(
    sheets: &[(&str, Option<Vec<u8>>)],
    strings: &[&str],
    formats: &[(u16, &str)],
    cell_formats: &[u16],
    date1904: bool,
) -> Vec<u8> {
    let mut parts: Vec<(&str, Vec<u8>)> = Vec::new();
    parts.push((
        "[Content_Types].xml",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="bin" ContentType="application/vnd.ms-excel.sheet.binary.macroEnabled.main"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/></Types>"#.to_vec(),
    ));
    parts.push((
        "_rels/.rels",
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.bin"/></Relationships>"#.to_vec(),
    ));
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    let mut book: Records = vec![(0x83, vec![])];
    let mut properties = vec![u8::from(date1904), 0, 0, 0];
    properties.extend_from_slice(&[0; 8]);
    properties.extend_from_slice(&wide(""));
    book.push((0x99, properties));
    book.push((0x8F, vec![]));
    let mut names = Vec::new();
    for (index, (name, part)) in sheets.iter().enumerate() {
        let id = format!("rId{}", index + 1);
        let mut body = 0u32.to_le_bytes().to_vec();
        body.extend_from_slice(&(index as u32 + 1).to_le_bytes());
        match part {
            Some(_) => body.extend_from_slice(&wide(&id)),
            None => body.extend_from_slice(&u32::MAX.to_le_bytes()),
        }
        body.extend_from_slice(&wide(name));
        book.push((0x9C, body));
        if let Some(part) = part {
            rels.push_str(&format!(
                r#"<Relationship Id="{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{}.bin"/>"#,
                index + 1
            ));
            names.push((
                format!("xl/worksheets/sheet{}.bin", index + 1),
                part.clone(),
            ));
        }
    }
    book.push((0x90, vec![]));
    book.push((0x9D, vec![0; 30]));
    book.push((0x84, vec![]));
    rels.push_str("</Relationships>");
    parts.push(("xl/_rels/workbook.bin.rels", rels.into_bytes()));
    parts.push(("xl/workbook.bin", part(&book)));
    let mut sst: Records = vec![(0x9F, {
        let mut body = (strings.len() as u32).to_le_bytes().to_vec();
        body.extend_from_slice(&(strings.len() as u32).to_le_bytes());
        body
    })];
    for text in strings {
        let mut body = vec![0];
        body.extend_from_slice(&wide(text));
        sst.push((0x13, body));
    }
    sst.push((0xA0, vec![]));
    parts.push(("xl/sharedStrings.bin", part(&sst)));
    let mut styles: Records = vec![(0x116, vec![])];
    styles.push((0x267, (formats.len() as u32).to_le_bytes().to_vec()));
    for (id, code) in formats {
        let mut body = id.to_le_bytes().to_vec();
        body.extend_from_slice(&wide(code));
        styles.push((0x2C, body));
    }
    styles.push((0x268, vec![]));
    styles.push((0x269, (cell_formats.len() as u32).to_le_bytes().to_vec()));
    for format in cell_formats {
        let mut body = 0xFFFFu16.to_le_bytes().to_vec();
        body.extend_from_slice(&format.to_le_bytes());
        body.extend_from_slice(&[0; 12]);
        styles.push((0x2F, body));
    }
    styles.push((0x26A, vec![]));
    styles.push((0x117, vec![]));
    parts.push(("xl/styles.bin", part(&styles)));
    let named: Vec<(String, Vec<u8>)> = names;
    let mut all: Vec<(&str, Vec<u8>)> = parts;
    for (name, bytes) in &named {
        all.push((name.as_str(), bytes.clone()));
    }
    zip_of(&all)
}

/// The workbooks the XLSB tests read, by name.
pub(crate) fn samples() -> Vec<(&'static str, Vec<u8>)> {
    use Value::*;
    let strings = ["Name", "  padded  ", "shared", "", "Ünïcødé 名前"];
    let formats = [
        (164, "yyyy\\-mm\\-dd\\ hh:mm"),
        (165, "[h]:mm:ss"),
        (166, "0.00%"),
    ];
    // 0 general, 1 a date (14), 2 a date and time (22), 3 custom date and
    // time, 4 a duration, 5 a percentage, 6 a time (21).
    let cell_formats = [0, 14, 22, 164, 165, 166, 21];
    let header = |names: &[&str]| -> Records {
        let mut records = vec![row(0)];
        for (column, name) in names.iter().enumerate() {
            records.push(cell(column as u32, 0, &Text((*name).to_owned())));
        }
        records
    };
    let mut types = header(&["a", "b", "c", "d", "e", "f", "g", "h"]);
    types.push(row(1));
    for (column, value) in [
        RkInt(36, false),
        RkInt(-7, false),
        RkInt(12345, true),
        RkFloat(1.5, false),
        RkFloat(250.0, true),
        Real(0.1 + 0.2),
        Real(-1e15),
        Bool(true),
    ]
    .iter()
    .enumerate()
    {
        types.push(cell(column as u32, 0, value));
    }
    types.push(row(2));
    for (column, value) in [
        Shared(0),
        Shared(1),
        Shared(3),
        Shared(4),
        Text("  inline  ".into()),
        Text(String::new()),
        Blank,
        Bool(false),
    ]
    .iter()
    .enumerate()
    {
        types.push(cell(column as u32, 0, value));
    }
    types.push(row(3));
    for (column, code) in [0x00u8, 0x07, 0x0F, 0x17, 0x1D, 0x24, 0x2A, 0x2B]
        .iter()
        .enumerate()
    {
        types.push(cell(column as u32, 0, &Error(*code)));
    }
    types.push(row(4));
    for (column, value) in [
        FormulaNumber(72.0),
        FormulaText(" Ada! ".into()),
        FormulaBool(true),
        FormulaError(0x07),
        FormulaNumber(46082.0),
    ]
    .iter()
    .enumerate()
    {
        let style = if column == 4 { 1 } else { 0 };
        types.push(cell(column as u32, style, value));
    }
    types.push(row(5));
    for (column, (style, value)) in [
        (1, Real(46082.0)),
        (2, Real(46082.75)),
        (3, Real(46082.5)),
        (4, Real(1.25)),
        (5, Real(0.256)),
        (6, RkFloat(0.25, false)),
        // An integer stays one whatever its format.
        (1, RkInt(46082, false)),
        (1, RkInt(4608200, true)),
    ]
    .iter()
    .enumerate()
    {
        types.push(cell(column as u32, *style, value));
    }
    // A cell format that does not exist is no date.
    types.push(row(6));
    types.push(cell(0, 99, &Real(46082.0)));
    let types = workbook(
        &[("Types", Some(sheet(&types)))],
        &strings,
        &formats,
        &cell_formats,
        false,
    );

    let mut dates = header(&["day", "at"]);
    dates.push(row(1));
    dates.push(cell(0, 1, &Real(1000.0)));
    dates.push(cell(1, 2, &Real(1000.5)));
    let date1904 = workbook(
        &[("Dates", Some(sheet(&dates)))],
        &[],
        &[],
        &cell_formats,
        true,
    );

    let mut first = header(&["one"]);
    first.extend([row(1), cell(0, 0, &RkInt(1, false))]);
    let mut second = vec![
        row(2),
        cell(1, 0, &Text("two".into())),
        cell(2, 0, &Text("2".into())),
    ];
    second.extend([
        row(3),
        cell(0, 0, &RkInt(1, false)),
        cell(2, 0, &RkInt(5, false)),
    ]);
    let sheets = workbook(
        &[
            ("First", Some(sheet(&first))),
            ("No part", None),
            ("Second & <more>", Some(sheet(&second))),
        ],
        &[],
        &[],
        &[0],
        false,
    );

    // Cells out of order, a position given twice, a row given twice.
    let unordered = vec![
        row(4),
        cell(0, 0, &Text("late".into())),
        row(0),
        cell(1, 0, &Text("b".into())),
        cell(0, 0, &Text("a".into())),
        row(1),
        cell(0, 0, &RkInt(1, false)),
        cell(0, 0, &RkInt(2, false)),
        row(1),
        cell(1, 0, &Text("   ".into())),
        row(2),
        cell(0, 0, &Text("kept".into())),
        cell(0, 0, &Text(" ".into())),
    ];
    let unordered = workbook(
        &[("Unordered", Some(sheet(&unordered)))],
        &[],
        &[],
        &[0],
        false,
    );

    // The last column of a sheet.
    let columns = vec![
        row(0),
        cell(0, 0, &Text("a".into())),
        cell(16_383, 0, &Text("far".into())),
        row(1),
        cell(16_383, 0, &RkInt(1, false)),
    ];
    let columns = workbook(&[("Columns", Some(sheet(&columns)))], &[], &[], &[0], false);
    // The last row a sheet may have, and one past it, which ends the sheet.
    let rows = vec![
        row(0),
        cell(0, 0, &Text("a".into())),
        row(0x0010_0000),
        cell(0, 0, &RkInt(1, false)),
        row(0x0010_0001),
        cell(0, 0, &RkInt(2, false)),
    ];
    let rows = workbook(&[("Rows", Some(sheet(&rows)))], &[], &[], &[0], false);

    vec![
        ("xlsb-types", types),
        ("xlsb-1904", date1904),
        ("xlsb-sheets", sheets),
        ("xlsb-unordered", unordered),
        ("xlsb-columns", columns),
        ("xlsb-rows", rows),
    ]
}
