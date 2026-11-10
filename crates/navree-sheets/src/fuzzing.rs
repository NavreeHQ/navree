//! Entry points for the fuzz targets in `fuzz/` (feature `fuzzing`): an
//! uploaded file read into a table, workbooks built around one part, so
//! that a target reaches each reader's parsing past the zip file, and the
//! XLS reader on its own. Not part of the API.

use std::io::{Cursor, Write};

use crate::Cell;
use crate::import::read::{ReadOptions, read_table};
use crate::import::xls::{Xls, XlsLimits, write};

/// The limits files are read with: small, so that a run stays fast.
fn limits() -> ReadOptions<'static> {
    ReadOptions {
        encoding: None,
        sheet: None,
        max_rows: 1_000,
        max_columns: 256,
        max_cells: 50_000,
        max_inflated_bytes: 8 * 1024 * 1024,
        max_text_bytes: 1024 * 1024,
        stop: None,
    }
}

/// Reads `bytes` as an uploaded file named `name`, and checks the table
/// against the limits: no more rows, columns, cells or text than they
/// allow, every row as wide as the header.
///
/// # Panics
///
/// When the table breaks a limit, or a reader panics.
pub fn read(bytes: &[u8], name: &str) {
    let options = limits();
    let Ok(table) = read_table(bytes, name, &options) else {
        return;
    };
    let width = table.headers.len();
    assert!(width as u64 <= options.max_columns, "{width} columns");
    assert!(
        table.rows.len() as u64 <= options.max_rows,
        "{} rows",
        table.rows.len()
    );
    assert!(
        (table.rows.len() as u64 + 1) * width as u64 <= options.max_cells,
        "{} cells",
        (table.rows.len() + 1) * width
    );
    let mut text = 0u64;
    let mut last = 0;
    for row in &table.rows {
        assert_eq!(row.cells.len(), width, "row {}", row.number);
        assert!(row.number > last, "row {} after {last}", row.number);
        last = row.number;
        for cell in &row.cells {
            if let Cell::Text(value) = cell {
                assert_eq!(value.trim(), value, "untrimmed text");
                text += value.len() as u64;
            }
        }
    }
    assert!(text <= options.max_text_bytes, "{text} bytes of text");
}

/// A zip file of `parts`, stored.
fn zip_of(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in parts {
        if zip.start_file(*name, options).is_err() || zip.write_all(bytes).is_err() {
            return Vec::new();
        }
    }
    zip.finish().map(Cursor::into_inner).unwrap_or_default()
}

const RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
const WORKBOOK: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><workbookPr date1904="0"/><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
const WORKBOOK_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
const STYLES: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="1"><numFmt numFmtId="164" formatCode="yyyy-mm-dd"/></numFmts><cellXfs count="3"><xf numFmtId="0"/><xf numFmtId="14"/><xf numFmtId="164"/></cellXfs></styleSheet>"#;
const STRINGS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2"><si><t>Name</t></si><si><r><t>rich</t></r><r><t xml:space="preserve"> text</t></r></si></sst>"#;
const SHEET: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="inlineStr"><is><t>B</t></is></c></row><row r="2"><c r="A2" s="1"><v>46082</v></c><c r="B2" t="s"><v>1</v></c></row></sheetData></worksheet>"#;

/// An XLSX workbook whose part `which` (0 the sheet, 1 the shared strings,
/// 2 the styles, 3 the workbook, 4 its relationships) is `content`.
pub fn xlsx_with(which: u8, content: &[u8]) -> Vec<u8> {
    let pick = |at: u8, default: &'static [u8]| if which == at { content } else { default };
    zip_of(&[
        ("_rels/.rels", RELS),
        ("xl/workbook.xml", pick(3, WORKBOOK)),
        ("xl/_rels/workbook.xml.rels", pick(4, WORKBOOK_RELS)),
        ("xl/styles.xml", pick(2, STYLES)),
        ("xl/sharedStrings.xml", pick(1, STRINGS)),
        ("xl/worksheets/sheet1.xml", pick(0, SHEET)),
    ])
}

const MANIFEST: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/></manifest:manifest>"#;

/// An ODS spreadsheet whose `content.xml` is `content`.
pub fn ods_with(content: &[u8]) -> Vec<u8> {
    zip_of(&[
        (
            "mimetype",
            b"application/vnd.oasis.opendocument.spreadsheet",
        ),
        ("META-INF/manifest.xml", MANIFEST),
        ("content.xml", content),
    ])
}

const XLSB_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.bin"/></Relationships>"#;

/// The records of a part, each a type and a body.
fn records(all: &[(u16, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (kind, body) in all {
        if *kind < 0x80 {
            out.push(*kind as u8);
        } else {
            out.extend([(*kind & 0x7F) as u8 | 0x80, (*kind >> 7) as u8]);
        }
        out.push(body.len() as u8);
        out.extend_from_slice(body);
    }
    out
}

/// An XLSB workbook whose part `which` (0 the sheet's cell records, after
/// its dimension and before its end, 1 the shared strings, 2 the styles, 3
/// the workbook) is `content`.
pub fn xlsb_with(which: u8, content: &[u8]) -> Vec<u8> {
    // A sheet named `S` whose part is `rId1`.
    let mut sheet_record = vec![0, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0];
    sheet_record.extend("rId1".encode_utf16().flat_map(u16::to_le_bytes));
    sheet_record.extend([1, 0, 0, 0, b'S', 0]);
    let workbook = records(&[
        (0x83, b""),
        (0x99, &[0; 12]),
        (0x9C, &sheet_record),
        (0x90, b""),
    ]);
    let strings = records(&[
        (0x9F, &[1, 0, 0, 0, 1, 0, 0, 0]),
        (0x13, &[0, 1, 0, 0, 0, b'a', 0]),
        (0xA0, b""),
    ]);
    let styles = records(&[
        (0x267, &[0, 0, 0, 0]),
        (0x269, &[2, 0, 0, 0]),
        (0x2F, &[0; 16]),
        (0x2F, &[0, 0, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
    ]);
    let mut sheet = records(&[(0x81, b""), (0x94, &[0; 16]), (0x91, b"")]);
    sheet.extend_from_slice(if which == 0 { content } else { &[] });
    sheet.extend(records(&[(0x92, b""), (0x82, b"")]));
    let pick = |at: u8, default: &[u8]| {
        if which == at {
            content.to_vec()
        } else {
            default.to_vec()
        }
    };
    zip_of(&[
        ("xl/_rels/workbook.bin.rels", XLSB_RELS),
        ("xl/workbook.bin", &pick(3, &workbook)),
        ("xl/sharedStrings.bin", &pick(1, &strings)),
        ("xl/styles.bin", &pick(2, &styles)),
        ("xl/worksheets/sheet1.bin", &sheet),
    ])
}

/// The most a workbook stream handed to [`xls_stream`] may have.
const MAX_STREAM: usize = 1 << 20;

/// Reads `bytes` as an XLS file, under limits of 1 MiB of workbook stream
/// and 1 MiB of shared text, and reads every worksheet's cells.
///
/// # Panics
///
/// When an invariant breaks: a workbook that opens had every sheet read
/// as it opened, so each worksheet's cells read to the end without an
/// error, and its shared text is within the limit.
pub fn xls(bytes: &[u8]) {
    let limits = XlsLimits {
        max_stream_bytes: 1 << 20,
        max_text_bytes: 1 << 20,
    };
    let Ok(xls) = Xls::open(bytes, limits) else {
        return;
    };
    let _ = xls.date1904();
    for (index, name) in xls.sheet_names().iter().enumerate() {
        let cells = xls
            .cells(index, None)
            .unwrap_or_else(|error| panic!("worksheet {name:?} is listed but not there: {error}"));
        for cell in cells {
            if let Err(error) = cell {
                panic!("worksheet {name:?} read as the workbook opened, not after: {error}");
            }
        }
    }
    assert!(xls.cells(xls.sheet_names().len(), None).is_err());
}

/// [`xls`] on `stream` as the workbook stream of a compound file, so that
/// the records are fuzzed without the file around them.
pub fn xls_stream(stream: &[u8]) {
    let stream = stream.get(..MAX_STREAM).unwrap_or(stream);
    xls(&write::compound(&[("Workbook", stream)]).bytes);
}
