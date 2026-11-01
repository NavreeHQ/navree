//! Reading back the XLSX files the module writes: the first sheet's used
//! area as rows of values, enough to check what was written.

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read};

use chrono::{NaiveDate, NaiveDateTime, TimeDelta};
use quick_xml::events::Event;

/// A cell as it was written.
#[derive(Clone, Debug, PartialEq)]
pub enum Data {
    Empty,
    String(String),
    Float(f64),
    Bool(bool),
    /// A number with a date format.
    DateTime(NaiveDateTime),
}

fn part(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<String> {
    let mut text = String::new();
    zip.by_name(name).ok()?.read_to_string(&mut text).unwrap();
    Some(text)
}

/// The value of `name` in a start tag.
fn attribute(element: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .map(|a| quick_xml::escape::unescape(&a.value).unwrap().into_owned())
}

/// The events of `xml`, each start tag with its attributes and the text up
/// to the next tag.
fn walk(xml: &str, mut each: impl FnMut(&quick_xml::events::BytesStart<'_>, bool, &str)) {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut open: Vec<quick_xml::events::BytesStart<'static>> = Vec::new();
    let mut text = String::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) => {
                each(&e, true, "");
                open.push(e.into_owned());
                text.clear();
            }
            Event::Text(t) => text.push_str(&t),
            Event::GeneralRef(r) => {
                let name: &str = &r;
                text.push_str(match name {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    other => panic!("entity {other}"),
                });
            }
            Event::End(_) => {
                if let Some(e) = open.pop() {
                    each(&e, false, &text);
                }
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
}

/// The sheet names and the first sheet's rows, from its first used row and
/// column to its last.
pub fn read_xlsx(bytes: &[u8]) -> (Vec<String>, Vec<Vec<Data>>) {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut names = Vec::new();
    walk(&part(&mut zip, "xl/workbook.xml").unwrap(), |e, start, _| {
        if start && e.local_name().as_ref() == "sheet" {
            names.push(attribute(e, "name").unwrap());
        }
    });
    let mut strings = Vec::new();
    if let Some(xml) = part(&mut zip, "xl/sharedStrings.xml") {
        let mut current = String::new();
        walk(&xml, |e, start, text| match (e.local_name().as_ref(), start) {
            ("si", true) => current.clear(),
            ("t", false) => current.push_str(text),
            ("si", false) => strings.push(current.clone()),
            _ => {}
        });
    }
    // Which cell formats show dates.
    let mut custom = HashMap::new();
    let mut dates = Vec::new();
    let mut in_cell_formats = false;
    walk(&part(&mut zip, "xl/styles.xml").unwrap(), |e, start, _| {
        match (e.local_name().as_ref(), start) {
            ("numFmt", true) => {
                let code = attribute(e, "formatCode").unwrap().to_lowercase();
                custom.insert(
                    attribute(e, "numFmtId").unwrap(),
                    code.contains('y') || code.contains('d') || code.contains('h'),
                );
            }
            ("cellXfs", flag) => in_cell_formats = flag,
            ("xf", true) if in_cell_formats => {
                let id = attribute(e, "numFmtId").unwrap_or_default();
                let builtin = id.parse::<u16>().is_ok_and(|n| (14..=22).contains(&n));
                dates.push(builtin || custom.get(&id).copied().unwrap_or(false));
            }
            _ => {}
        }
    });
    let mut cells: BTreeMap<(u32, u32), Data> = BTreeMap::new();
    let mut at = (0, 0);
    let mut kind = String::new();
    let mut style = 0usize;
    walk(&part(&mut zip, "xl/worksheets/sheet1.xml").unwrap(), |e, start, text| {
        match (e.local_name().as_ref(), start) {
            ("c", true) => {
                let r = attribute(e, "r").unwrap();
                let letters: String = r.chars().take_while(char::is_ascii_alphabetic).collect();
                let column = letters
                    .bytes()
                    .fold(0u32, |n, b| n * 26 + u32::from(b - b'A') + 1)
                    - 1;
                let row: u32 = r[letters.len()..].parse::<u32>().unwrap() - 1;
                at = (row, column);
                kind = attribute(e, "t").unwrap_or_default();
                style = attribute(e, "s").map_or(0, |s| s.parse().unwrap());
            }
            ("v", false) => {
                let value = match kind.as_str() {
                    "s" => Data::String(strings[text.parse::<usize>().unwrap()].clone()),
                    "b" => Data::Bool(text == "1"),
                    "str" => Data::String(text.to_owned()),
                    _ => {
                        let n: f64 = text.parse().unwrap();
                        if dates.get(style).copied().unwrap_or(false) {
                            let epoch = NaiveDate::from_ymd_opt(1899, 12, 30)
                                .unwrap()
                                .and_hms_opt(0, 0, 0)
                                .unwrap();
                            let ms = (n * 86_400_000.0).round() as i64;
                            Data::DateTime(epoch + TimeDelta::milliseconds(ms))
                        } else {
                            Data::Float(n)
                        }
                    }
                };
                cells.insert(at, value);
            }
            ("t", false) if kind == "inlineStr" => {
                cells.insert(at, Data::String(text.to_owned()));
            }
            _ => {}
        }
    });
    let Some(first_row) = cells.keys().map(|(row, _)| *row).min() else {
        return (names, Vec::new());
    };
    let last_row = cells.keys().map(|(row, _)| *row).max().unwrap();
    let first_col = cells.keys().map(|(_, col)| *col).min().unwrap();
    let last_col = cells.keys().map(|(_, col)| *col).max().unwrap();
    let rows = (first_row..=last_row)
        .map(|row| {
            (first_col..=last_col)
                .map(|col| cells.get(&(row, col)).cloned().unwrap_or(Data::Empty))
                .collect()
        })
        .collect();
    (names, rows)
}
