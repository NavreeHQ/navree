//! CSV writing: a byte-order mark, the delimiter, CRLF line endings and
//! escaping against formula injection.

use std::borrow::Cow;

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::{Cell, SheetsError};

/// The UTF-8 byte-order mark: Excel needs it to open a UTF-8 CSV file
/// (Arabic, accented or CJK text) without garbling it.
pub const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// How CSV is written (configuration section `sheets.csv`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CsvOptions {
    /// Start the file with a UTF-8 byte-order mark, so Excel reads it as
    /// UTF-8 (default `true`).
    pub bom: bool,
    /// The field delimiter, one ASCII character (default `,`; `;` for
    /// spreadsheets set to a European locale, `\t` for TSV).
    pub delimiter: char,
    /// Escape text that a spreadsheet would run as a formula (default
    /// `true`): a cell starting with `=`, `+`, `-`, `@`, a tab or a carriage
    /// return (or their full-width forms) gets a leading `'`. Numbers are
    /// never escaped.
    pub escape_formulas: bool,
    /// End lines with CRLF, as RFC 4180 says (default `true`); `false` uses
    /// LF.
    pub crlf: bool,
}

impl Default for CsvOptions {
    fn default() -> Self {
        CsvOptions {
            bom: true,
            delimiter: ',',
            escape_formulas: true,
            crlf: true,
        }
    }
}

impl CsvOptions {
    /// The delimiter as a byte; an error names `sheets.csv.delimiter` when
    /// it is not one ASCII character.
    pub(crate) fn delimiter_byte(&self) -> Result<u8, SheetsError> {
        if self.delimiter.is_ascii() && !matches!(self.delimiter, '"' | '\n' | '\r') {
            Ok(self.delimiter as u8)
        } else {
            Err(SheetsError::Invalid(format!(
                "sheets.csv.delimiter must be one ASCII character other than a quote or a line break, not {:?}",
                self.delimiter
            )))
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        self.delimiter_byte()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

/// The characters that make a spreadsheet read a cell as a formula.
const FORMULA_STARTS: &[char] = &[
    '=', '+', '-', '@', '\t', '\r', '\u{FF1D}', '\u{FF0B}', '\u{FF0D}', '\u{FF20}',
];

/// `text` with a leading `'` when a spreadsheet would run it as a formula
/// (it starts with `=`, `+`, `-`, `@`, a tab, a carriage return, or a
/// full-width `＝`, `＋`, `－` or `＠`).
///
/// ```
/// use navree_sheets::escape_formula;
///
/// assert_eq!(escape_formula("=HYPERLINK(\"http://x\")"), "'=HYPERLINK(\"http://x\")");
/// assert_eq!(escape_formula("@SUM(A1)"), "'@SUM(A1)");
/// assert_eq!(escape_formula("Ada Lovelace"), "Ada Lovelace");
/// ```
pub fn escape_formula(text: &str) -> Cow<'_, str> {
    match text.chars().next() {
        Some(first) if FORMULA_STARTS.contains(&first) => Cow::Owned(format!("'{text}")),
        _ => Cow::Borrowed(text),
    }
}

/// Encodes rows as CSV, chunk by chunk: the first chunk starts with the
/// byte-order mark and the header row.
pub(crate) struct CsvEncoder {
    options: CsvOptions,
    delimiter: u8,
}

impl CsvEncoder {
    pub(crate) fn new(options: CsvOptions) -> Result<Self, SheetsError> {
        let delimiter = options.delimiter_byte()?;
        Ok(CsvEncoder { options, delimiter })
    }

    fn writer(&self, capacity: usize) -> csv::Writer<Vec<u8>> {
        csv::WriterBuilder::new()
            .delimiter(self.delimiter)
            .terminator(if self.options.crlf {
                csv::Terminator::CRLF
            } else {
                csv::Terminator::Any(b'\n')
            })
            .quote_style(csv::QuoteStyle::Necessary)
            .flexible(true)
            .from_writer(Vec::with_capacity(capacity))
    }

    fn text<'a>(&self, cell: &'a Cell) -> Cow<'a, str> {
        match cell {
            Cell::Text(text) if self.options.escape_formulas => escape_formula(text),
            other => other.to_text(),
        }
    }

    /// The byte-order mark (when on) and the header row.
    pub(crate) fn header(&self, headers: &[String]) -> Result<Bytes, SheetsError> {
        let mut writer = self.writer(256);
        let record: Vec<Cow<'_, str>> = headers
            .iter()
            .map(|header| {
                if self.options.escape_formulas {
                    escape_formula(header)
                } else {
                    Cow::Borrowed(header.as_str())
                }
            })
            .collect();
        writer.write_record(record.iter().map(|text| text.as_bytes()))?;
        let row = finish(writer)?;
        if self.options.bom {
            let mut out = Vec::with_capacity(UTF8_BOM.len() + row.len());
            out.extend_from_slice(UTF8_BOM);
            out.extend_from_slice(&row);
            Ok(Bytes::from(out))
        } else {
            Ok(row)
        }
    }

    /// `rows`, encoded.
    pub(crate) fn rows(&self, rows: &[Vec<Cell>]) -> Result<Bytes, SheetsError> {
        let mut writer = self.writer(rows.len() * 64);
        for row in rows {
            let record: Vec<Cow<'_, str>> = row.iter().map(|cell| self.text(cell)).collect();
            writer.write_record(record.iter().map(|text| text.as_bytes()))?;
        }
        finish(writer)
    }
}

fn finish(writer: csv::Writer<Vec<u8>>) -> Result<Bytes, SheetsError> {
    writer
        .into_inner()
        .map(Bytes::from)
        .map_err(|error| SheetsError::Io(error.into_error()))
}

/// A whole CSV file: the byte-order mark (when on), `headers` and `rows`.
///
/// ```
/// use navree_sheets::{Cell, CsvOptions, write_csv};
///
/// let file = write_csv(
///     &CsvOptions::default(),
///     &["Name".to_owned(), "Total".to_owned()],
///     &[vec![Cell::Text("=1+1".into()), Cell::Int(-5)]],
/// )
/// .unwrap();
/// assert_eq!(file, "\u{FEFF}Name,Total\r\n'=1+1,-5\r\n".as_bytes());
/// ```
pub fn write_csv(
    options: &CsvOptions,
    headers: &[String],
    rows: &[Vec<Cell>],
) -> Result<Vec<u8>, SheetsError> {
    let encoder = CsvEncoder::new(options.clone())?;
    let mut out = encoder.header(headers)?.to_vec();
    out.extend_from_slice(&encoder.rows(rows)?);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csv(options: CsvOptions, rows: &[Vec<Cell>]) -> String {
        let bytes = write_csv(&options, &["A".into(), "B".into()], rows).unwrap();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn every_formula_start_is_escaped() {
        for text in [
            "=cmd", "+1", "-1", "@x", "\tx", "\rx", "＝1", "＋1", "－1", "＠x",
        ] {
            assert_eq!(escape_formula(text), format!("'{text}"), "{text:?}");
        }
        for text in ["", "a=b", "1-2", "'quoted", " =x"] {
            assert_eq!(escape_formula(text), text);
        }
    }

    #[test]
    fn numbers_are_never_escaped() {
        let out = csv(
            CsvOptions::default(),
            &[vec![Cell::Int(-5), Cell::Float(-1.5)]],
        );
        assert!(out.ends_with("A,B\r\n-5,-1.5\r\n"), "{out:?}");
    }

    #[test]
    fn escaping_and_the_bom_can_be_turned_off() {
        let options = CsvOptions {
            bom: false,
            escape_formulas: false,
            crlf: false,
            ..CsvOptions::default()
        };
        let out = csv(options, &[vec![Cell::Text("=1".into()), Cell::Empty]]);
        assert_eq!(out, "A,B\n=1,\n");
    }

    #[test]
    fn fields_are_quoted_when_they_need_it() {
        let options = CsvOptions {
            delimiter: ';',
            ..CsvOptions::default()
        };
        let out = csv(
            options,
            &[vec![
                Cell::Text("a;b".into()),
                Cell::Text("say \"hi\"\nthere".into()),
            ]],
        );
        assert!(
            out.ends_with("A;B\r\n\"a;b\";\"say \"\"hi\"\"\nthere\"\r\n"),
            "{out:?}"
        );
    }

    #[test]
    fn a_bad_delimiter_names_the_key() {
        let options = CsvOptions {
            delimiter: 'é',
            ..CsvOptions::default()
        };
        let error = options.validate().unwrap_err();
        assert!(error.contains("sheets.csv.delimiter"), "{error}");
    }

    #[test]
    fn the_bom_comes_first() {
        let bytes = write_csv(&CsvOptions::default(), &["X".into()], &[]).unwrap();
        assert!(bytes.starts_with(UTF8_BOM));
    }
}
