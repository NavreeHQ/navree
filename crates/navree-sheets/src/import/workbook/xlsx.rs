//! XLSX workbooks (and XLSM): SpreadsheetML parts in a zip file.
//!
//! Opening reads the package's relationships (where the workbook is), the
//! workbook's relationships (where its parts are), the shared strings, the
//! cell formats (which show dates) and the workbook (its sheets and date
//! system). A sheet is then read as a stream of its cells.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;

use super::dates::{
    builtin_id_shows_date, excel_date, iso_date, iso_time, number, number_or_text, shows_date,
};
use super::package::{self, Kind as PartKind, Relationships};
use super::xml::{
    self, MAX_LENGTH, Xml, attribute, attributes, attributes_of, decode, push_entity, too_long,
    unescape_excel,
};
use super::{
    Archive, Budget, CellSource, Damaged, MAX_SHEETS, SharedStrings, SourceCell, Value, damaged,
    too_many_sheets,
};
use crate::{Cell, SheetsError};

/// An XLSX workbook, opened.
pub(crate) struct Xlsx<'a> {
    archive: Archive<'a>,
    strings: SharedStrings,
    /// Whether each cell format (`cellXfs`) shows a number as a date.
    dates: Vec<bool>,
    /// Whether dates count from 1904.
    is_1904: bool,
    names: Vec<String>,
    /// The workbook's relationships.
    relationships: Relationships,
    /// The relationship of each sheet, by its place.
    sheets: Vec<usize>,
}

impl<'a> Xlsx<'a> {
    pub(super) fn open(archive: &Archive<'a>, budget: &Budget) -> Result<Self, SheetsError> {
        let workbook = workbook_part(archive, budget)?;
        let folder = package::folder_of(&workbook);
        let rels = package::relationships_of(&workbook);
        let relationships = Relationships::read(archive, budget, &rels, folder)?
            .ok_or_else(|| damaged(format_args!("the file has no {rels} part")))?;
        // Parts the relationships do not name are looked for where they
        // usually are.
        let named = |kind, name: &str| {
            relationships
                .part_of(kind)
                .map_or_else(|| format!("{folder}{name}"), str::to_owned)
        };
        let strings = shared_strings(
            archive,
            &named(PartKind::SharedStrings, "sharedStrings.xml"),
            budget,
        )?;
        let dates = date_formats(archive, &named(PartKind::Styles, "styles.xml"), budget)?;
        let (names, sheets, is_1904) = sheets(archive, &workbook, &relationships, budget)?;
        Ok(Xlsx {
            archive: archive.clone(),
            strings,
            dates,
            is_1904,
            names,
            relationships,
            sheets,
        })
    }

    pub(super) fn sheet_names(&self) -> &[String] {
        &self.names
    }

    pub(super) fn read_cells<T>(
        &self,
        sheet: usize,
        stop: Option<&AtomicBool>,
        read: impl FnOnce(Option<&mut dyn CellSource>) -> Result<T, SheetsError>,
    ) -> Result<T, SheetsError> {
        let sheet = self
            .sheets
            .get(sheet)
            .and_then(|place| self.relationships.get(*place))
            .ok_or_else(|| damaged("the workbook has no such sheet"))?;
        if sheet.kind == PartKind::Macrosheet {
            return read(None);
        }
        let path = &sheet.part;
        let read = self
            .archive
            .read(self.archive.name_of(path), stop, |source| {
                let mut xml = xml::reader(source);
                if !find_sheet_data(&mut xml)? {
                    // A chart sheet: its part has no cells.
                    return read(None);
                }
                let mut cells = XlsxCells {
                    xml,
                    strings: &self.strings,
                    dates: &self.dates,
                    is_1904: self.is_1904,
                    row: 0,
                    column: 0,
                    done: false,
                    buf: Vec::new(),
                    cell_buf: Vec::new(),
                    value_buf: Vec::new(),
                    text_buf: Vec::new(),
                    text: String::new(),
                };
                read(Some(&mut cells))
            })?;
        read.ok_or_else(|| damaged(format_args!("the sheet part {path} is missing")).into())
    }
}

fn is(element: &BytesStart<'_>, name: &str) -> bool {
    element.local_name().as_ref() == name
}

fn ends(end: &quick_xml::events::BytesEnd<'_>, name: &str) -> bool {
    end.local_name().as_ref() == name
}

fn missing(what: &str) -> Damaged {
    damaged(format_args!("{what} ends before it is complete"))
}

/// The workbook part, which the package's relationships name. A binary one
/// is an XLSB workbook's.
fn workbook_part(archive: &Archive<'_>, budget: &Budget) -> Result<String, Damaged> {
    let relationships = Relationships::read(archive, budget, package::PACKAGE, "")?
        .ok_or_else(|| damaged("the file has no _rels/.rels part"))?;
    let part = relationships
        .part_of(PartKind::Workbook)
        .ok_or_else(|| damaged("the file names no workbook part"))?;
    if package::is_binary(part) {
        return Err(damaged("the workbook part is a binary one"));
    }
    Ok(part.to_owned())
}

/// The shared strings in the part `path`, rich text joined; none when the
/// workbook has no such part. The count the part declares is not used: the
/// strings are counted as they come.
fn shared_strings(
    archive: &Archive<'_>,
    path: &str,
    budget: &Budget,
) -> Result<SharedStrings, SheetsError> {
    let strings = archive.read_opening(archive.name_of(path), budget, |source| {
        let mut strings = SharedStrings::within(budget);
        let mut xml = xml::reader(source);
        let (mut buf, mut inner, mut text_buf) = (Vec::new(), Vec::new(), Vec::new());
        loop {
            match xml.next(&mut buf)? {
                Event::Start(e) if is(&e, "sst") => {
                    attribute(&e, "uniqueCount")?;
                    break;
                }
                Event::Eof => return Err(missing("the shared strings").into()),
                _ => {}
            }
        }
        loop {
            match xml.next(&mut buf)? {
                Event::Start(e) if is(&e, "si") => {
                    let text = read_string(&mut xml, e.name(), &mut inner, &mut text_buf)?;
                    strings.push(&text)?;
                }
                Event::End(e) if ends(&e, "sst") => break,
                Event::Eof => return Err(missing("the shared strings").into()),
                _ => {}
            }
        }
        strings.finish(budget);
        Ok::<_, SheetsError>(strings)
    })?;
    Ok(strings.unwrap_or_else(|| SharedStrings::within(budget)))
}

/// The text of a string element (`<si>`, `<is>`) up to its end `closing`:
/// its `<t>` texts joined, without the phonetic runs (`<rPh>`). A `<t>`
/// that does not preserve its spaces loses the spaces, tabs and line breaks
/// round it. Made in place, so one string is never held twice over here.
fn read_string(
    xml: &mut Xml<'_>,
    closing: QName<'_>,
    buf: &mut Vec<u8>,
    text_buf: &mut Vec<u8>,
) -> Result<String, Damaged> {
    let mut text = String::new();
    let mut phonetic = false;
    loop {
        match xml.next(buf)? {
            Event::Start(e) if is(&e, "rPh") => phonetic = true,
            Event::End(e) if e.name() == closing => return Ok(text),
            Event::End(e) if ends(&e, "rPh") => phonetic = false,
            Event::Start(e) if is(&e, "t") && !phonetic => {
                let preserve = attribute(&e, "xml:space")? == Some("preserve");
                let start = text.len();
                loop {
                    match xml.next(text_buf)? {
                        Event::Text(t) => text.push_str(&t.xml10_content()),
                        Event::CData(t) => text.push_str(&t.xml10_content()),
                        Event::GeneralRef(entity) => push_entity(&entity, &mut text)?,
                        Event::End(end) if end.name() == e.name() => break,
                        Event::Eof => return Err(missing("a string")),
                        _ => {}
                    }
                    if text.len() > MAX_LENGTH {
                        return Err(too_long());
                    }
                }
                if !preserve {
                    let spaces = [' ', '\t', '\r', '\n'];
                    let value = &text[start..];
                    let kept = value.trim_end_matches(spaces).len();
                    let before = kept - value[..kept].trim_start_matches(spaces).len();
                    text.truncate(start + kept);
                    text.drain(start..start + before);
                }
                if let Cow::Owned(unescaped) = unescape_excel(&text[start..]) {
                    text.truncate(start);
                    text.push_str(&unescaped);
                }
            }
            Event::Eof => return Err(missing("a string")),
            _ => {}
        }
    }
}

/// Whether each cell format in the part `path` shows a number as a date: by
/// its custom number format (`numFmts`) when it has one, by the built-in
/// format's id when not. A custom format is looked at once, when it is read,
/// however many cell formats use it.
fn date_formats(archive: &Archive<'_>, path: &str, budget: &Budget) -> Result<Vec<bool>, Damaged> {
    let dates = archive.read_opening(archive.name_of(path), budget, |source| {
        let mut xml = xml::reader(source);
        let (mut buf, mut inner) = (Vec::new(), Vec::new());
        let mut custom: HashMap<String, bool> = HashMap::new();
        let mut dates = Vec::new();
        loop {
            match xml.next(&mut buf)? {
                Event::Start(e) if is(&e, "numFmts") => loop {
                    match xml.next(&mut inner)? {
                        Event::Start(e) if is(&e, "numFmt") => {
                            if let [Some(id), Some(code)] =
                                attributes_of(&e, ["numFmtId", "formatCode"])?
                            {
                                // The entry in the table, besides the id.
                                budget.hold(id.len() + 32)?;
                                custom.insert(id.to_owned(), shows_date(&decode(code)?));
                            }
                        }
                        Event::End(e) if ends(&e, "numFmts") => break,
                        Event::Eof => return Err(missing("the styles")),
                        _ => {}
                    }
                },
                Event::Start(e) if is(&e, "cellXfs") => loop {
                    match xml.next(&mut inner)? {
                        Event::Start(e) if is(&e, "xf") => {
                            budget.hold(1)?;
                            dates.push(attribute(&e, "numFmtId")?.is_some_and(|id| {
                                custom
                                    .get(id)
                                    .copied()
                                    .unwrap_or_else(|| builtin_id_shows_date(id))
                            }));
                        }
                        Event::End(e) if ends(&e, "cellXfs") => break,
                        Event::Eof => return Err(missing("the styles")),
                        _ => {}
                    }
                },
                Event::End(e) if ends(&e, "styleSheet") => break,
                Event::Eof => return Err(missing("the styles")),
                _ => {}
            }
        }
        Ok(dates)
    })?;
    Ok(dates.unwrap_or_default())
}

/// The sheets of the workbook part `path`: their names, the place of the
/// relationship of each, and whether dates count from 1904.
fn sheets(
    archive: &Archive<'_>,
    path: &str,
    relationships: &Relationships,
    budget: &Budget,
) -> Result<(Vec<String>, Vec<usize>, bool), Damaged> {
    let found = archive.read_opening(archive.name_of(path), budget, |source| {
        let mut xml = xml::reader(source);
        let (mut buf, mut inner) = (Vec::new(), Vec::new());
        let (mut names, mut sheets, mut is_1904) = (Vec::new(), Vec::new(), false);
        let mut entity = String::new();
        loop {
            match xml.next(&mut buf)? {
                Event::Start(e) if is(&e, "sheet") => {
                    let mut name = String::new();
                    let mut id = "";
                    for attribute in attributes(&e) {
                        let (key, value) = attribute?;
                        match key {
                            "name" => name = decode(value)?.into_owned(),
                            "state" if !matches!(value, "visible" | "hidden" | "veryHidden") => {
                                return Err(damaged(format_args!(
                                    "the sheet state `{value}` is unknown"
                                )));
                            }
                            _ if key == "id"
                                || key.strip_suffix("id").is_some_and(|p| p.ends_with(':')) =>
                            {
                                id = value;
                            }
                            _ => {}
                        }
                    }
                    let place = relationships
                        .place_of(id)
                        .ok_or_else(|| damaged(format_args!("the sheet `{name}` has no part")))?;
                    let kind = relationships.get(place).map(|sheet| sheet.kind);
                    if !matches!(
                        kind,
                        Some(
                            PartKind::Worksheet
                                | PartKind::Chartsheet
                                | PartKind::Dialogsheet
                                | PartKind::Macrosheet
                        )
                    ) {
                        return Err(damaged(format_args!(
                            "the sheet `{name}` is of an unknown type"
                        )));
                    }
                    if names.len() >= MAX_SHEETS {
                        return Err(damaged(too_many_sheets()));
                    }
                    // A name, and its place in two lists.
                    budget.hold(name.len() + 32)?;
                    sheets.push(place);
                    names.push(name);
                }
                Event::Start(e) if is(&e, "workbookPr") => {
                    if let Some(value) = attribute(&e, "date1904")? {
                        is_1904 = value == "1" || value == "true";
                    }
                }
                Event::Start(e) if is(&e, "definedName") => {
                    if let Some(name) = attribute(&e, "name")? {
                        decode(name)?;
                        // Checked as it is read, and not kept.
                        loop {
                            entity.clear();
                            match xml.next(&mut inner)? {
                                Event::GeneralRef(reference) => {
                                    push_entity(&reference, &mut entity)?;
                                }
                                Event::End(end) if end.name() == e.name() => break,
                                Event::Eof => return Err(missing("the workbook")),
                                _ => {}
                            }
                        }
                    }
                }
                Event::End(e) if ends(&e, "workbook") => break,
                Event::Eof => return Err(missing("the workbook")),
                _ => {}
            }
        }
        Ok((names, sheets, is_1904))
    })?;
    Ok(found.unwrap_or_default())
}

/// Reads a sheet part up to its cells (`<sheetData>`): false when the part
/// has none to read, as a chart sheet does.
fn find_sheet_data(xml: &mut Xml<'_>) -> Result<bool, Damaged> {
    let mut buf = Vec::new();
    let mut other = false;
    loop {
        match xml.next(&mut buf)? {
            Event::Start(e) if is(&e, "dimension") => {
                let range = attribute(&e, "ref")?
                    .ok_or_else(|| damaged("the sheet's dimension has no range"))?;
                // Two corners at most: the pieces are not listed, which
                // would take memory in proportion to the colons.
                let mut corners = range.split(':');
                let (first, second, third) = (corners.next(), corners.next(), corners.next());
                if third.is_some() {
                    return Err(damaged(format_args!("the range {range} is not a range")));
                }
                for corner in [first, second].into_iter().flatten() {
                    cell_position(corner)?;
                }
            }
            Event::Start(e) if is(&e, "sheetData") => return Ok(true),
            Event::Start(_) => other = true,
            Event::Eof if other => return Ok(false),
            Event::Eof => return Err(missing("the sheet")),
            _ => {}
        }
    }
}

/// A position as a sheet writes it (`B12`): row and column from 0, the
/// column none when there is no letter.
fn position(at: &str) -> Result<(u32, Option<u32>), Damaged> {
    let wrong = || damaged(format_args!("`{at}` is not a cell position"));
    let mut column = 0u32;
    let mut row = 0u32;
    let mut digits = false;
    for byte in at.bytes() {
        let letter = match byte {
            b'A'..=b'Z' if !digits => byte - b'A',
            b'a'..=b'z' if !digits => byte - b'a',
            b'0'..=b'9' => {
                digits = true;
                row = row
                    .checked_mul(10)
                    .and_then(|row| row.checked_add(u32::from(byte - b'0')))
                    .ok_or_else(wrong)?;
                continue;
            }
            _ => return Err(wrong()),
        };
        column = column
            .checked_mul(26)
            .and_then(|column| column.checked_add(u32::from(letter) + 1))
            .ok_or_else(wrong)?;
    }
    let row = row.checked_sub(1).ok_or_else(wrong)?;
    Ok((row, column.checked_sub(1)))
}

fn cell_position(at: &str) -> Result<(u32, u32), Damaged> {
    match position(at)? {
        (row, Some(column)) => Ok((row, column)),
        (_, None) => Err(damaged(format_args!("`{at}` is not a cell position"))),
    }
}

/// The type a cell gives its value (`t`).
#[derive(Clone, Debug, PartialEq)]
enum Kind {
    /// No type: a number, or text when it is not one.
    Unset,
    Number,
    Shared,
    Bool,
    Error,
    Inline,
    Formula,
    Date,
    Other(String),
}

impl Kind {
    fn of(attribute: Option<&str>) -> Kind {
        match attribute {
            None => Kind::Unset,
            Some("n") => Kind::Number,
            Some("s") => Kind::Shared,
            Some("b") => Kind::Bool,
            Some("e") => Kind::Error,
            Some("inlineStr" | "is") => Kind::Inline,
            Some("str") => Kind::Formula,
            Some("d") => Kind::Date,
            Some(other) => Kind::Other(other.to_owned()),
        }
    }
}

/// What a cell holds, while it is read.
enum Found {
    Empty,
    /// The text in `XlsxCells::text`.
    Text,
    Shared(usize),
    Fixed(&'static str),
    Typed(Cell),
}

/// The cells of a sheet, as its part lists them.
struct XlsxCells<'w, 'r> {
    xml: Xml<'r>,
    strings: &'w SharedStrings,
    dates: &'w [bool],
    is_1904: bool,
    /// The row of the next cell without a position, and its column.
    row: u32,
    column: u32,
    /// Whether the sheet's cells have ended.
    done: bool,
    buf: Vec<u8>,
    cell_buf: Vec<u8>,
    value_buf: Vec<u8>,
    text_buf: Vec<u8>,
    /// The text of the cell last read.
    text: String,
}

impl CellSource for XlsxCells<'_, '_> {
    fn next_cell(&mut self) -> Result<Option<SourceCell<'_>>, SheetsError> {
        if self.done {
            return Ok(None);
        }
        loop {
            match self.xml.next(&mut self.buf)? {
                Event::Start(e) if is(&e, "row") => {
                    if let Some(at) = attribute(&e, "r")? {
                        self.row = position(at)?.0;
                    }
                }
                Event::End(e) if ends(&e, "row") => {
                    self.row = self
                        .row
                        .checked_add(1)
                        .ok_or_else(|| damaged("too many rows"))?;
                    self.column = 0;
                }
                Event::Start(e) if is(&e, "c") => {
                    let [at, style, kind] = attributes_of(&e, ["r", "s", "t"])?;
                    let (row, column) = match at {
                        Some(at) => {
                            let (row, column) = cell_position(at)?;
                            self.column = column;
                            (row, column)
                        }
                        None => (self.row, self.column),
                    };
                    // A cell's number format, by the index of its cell
                    // format; an index that is not a number is the first.
                    let date = match style {
                        Some(style) => self
                            .dates
                            .get(style.parse::<usize>().unwrap_or(0))
                            .copied()
                            .unwrap_or(false),
                        None => false,
                    };
                    let kind = Kind::of(kind);
                    let mut found = Found::Empty;
                    loop {
                        match self.xml.next(&mut self.cell_buf)? {
                            Event::Start(value) => {
                                let read = read_value(
                                    &mut self.xml,
                                    &value,
                                    &kind,
                                    date,
                                    self.is_1904,
                                    self.strings,
                                    [&mut self.value_buf, &mut self.text_buf],
                                    &mut self.text,
                                )?;
                                if let Some(read) = read {
                                    found = read;
                                }
                            }
                            Event::End(end) if ends(&end, "c") => break,
                            Event::Eof => return Err(missing("a cell").into()),
                            _ => {}
                        }
                    }
                    self.column = self
                        .column
                        .checked_add(1)
                        .ok_or_else(|| damaged("too many columns"))?;
                    let value = match found {
                        Found::Empty => continue,
                        Found::Text => Value::Text(self.text.trim()),
                        Found::Shared(index) => {
                            Value::Text(self.strings.get(index).unwrap_or_default())
                        }
                        Found::Fixed(text) => Value::Text(text),
                        Found::Typed(cell) => Value::Typed(cell),
                    };
                    return Ok(Some(SourceCell {
                        row,
                        column,
                        columns: 1,
                        value,
                    }));
                }
                Event::End(e) if ends(&e, "sheetData") => {
                    self.done = true;
                    return Ok(None);
                }
                Event::Eof => return Err(missing("the sheet").into()),
                _ => {}
            }
        }
    }
}

/// The value of an element of a cell: its value (`<v>`), its inline string
/// (`<is>`) or its formula (`<f>`, which holds no value). Any other element
/// (`<extLst>`) says nothing about the value: none.
#[allow(clippy::too_many_arguments)]
fn read_value(
    xml: &mut Xml<'_>,
    element: &BytesStart<'_>,
    kind: &Kind,
    date: bool,
    is_1904: bool,
    strings: &SharedStrings,
    [buf, text_buf]: [&mut Vec<u8>; 2],
    text: &mut String,
) -> Result<Option<Found>, Damaged> {
    let name = element.name();
    Ok(Some(match element.local_name().as_ref() {
        "is" => {
            *text = read_string(xml, name, buf, text_buf)?;
            Found::Text
        }
        "v" if *kind == Kind::Inline => {
            xml.skip(name, buf)?;
            Found::Empty
        }
        "v" if matches!(
            kind,
            Kind::Unset | Kind::Number | Kind::Shared | Kind::Bool | Kind::Error
        ) =>
        {
            // These values are numbers or names: the first text is the value.
            let found = match xml.next(buf)? {
                Event::Text(value) => plain_value(&value, kind, date, is_1904, strings, text)?,
                Event::End(end) if end.name() == name => return Ok(Some(Found::Empty)),
                Event::Eof => return Err(missing("a value")),
                _ => Found::Empty,
            };
            xml.skip(name, buf)?;
            found
        }
        "v" => {
            text.clear();
            loop {
                match xml.next(buf)? {
                    Event::Text(value) => text.push_str(&value.xml10_content()),
                    Event::GeneralRef(entity) => push_entity(&entity, text)?,
                    Event::End(end) if end.name() == name => break,
                    Event::Eof => return Err(missing("a value")),
                    _ => {}
                }
                if text.len() > MAX_LENGTH {
                    return Err(too_long());
                }
            }
            match kind {
                // A date in ISO 8601; text that is not one stays text.
                Kind::Date => {
                    match iso_date(text.trim()).or_else(|| iso_time(text.trim(), is_1904)) {
                        Some(cell) => Found::Typed(cell),
                        None => Found::Text,
                    }
                }
                Kind::Formula => {
                    if let Cow::Owned(unescaped) = unescape_excel(text) {
                        *text = unescaped;
                    }
                    Found::Text
                }
                _ => {
                    return Err(damaged(format_args!(
                        "a cell has the unknown type {kind:?}"
                    )));
                }
            }
        }
        "f" => {
            xml.skip(name, buf)?;
            Found::Empty
        }
        _ => {
            xml.skip(name, buf)?;
            return Ok(None);
        }
    }))
}

/// A value written as a number, a shared string's index, a boolean or an
/// error's name.
fn plain_value(
    value: &str,
    kind: &Kind,
    date: bool,
    is_1904: bool,
    strings: &SharedStrings,
    text: &mut String,
) -> Result<Found, Damaged> {
    Ok(match kind {
        Kind::Shared if value.is_empty() => Found::Empty,
        Kind::Shared => {
            // An index that is not a number is the first.
            let index = value.parse::<usize>().unwrap_or(0);
            if strings.get(index).is_none() {
                return Err(damaged(format_args!(
                    "the shared string {index} is missing"
                )));
            }
            Found::Shared(index)
        }
        Kind::Bool => match value.trim() {
            "" => Found::Empty,
            "0" => Found::Typed(Cell::Bool(false)),
            "1" => Found::Typed(Cell::Bool(true)),
            word if word.eq_ignore_ascii_case("false") => Found::Typed(Cell::Bool(false)),
            word if word.eq_ignore_ascii_case("true") => Found::Typed(Cell::Bool(true)),
            other => return Err(damaged(format_args!("`{other}` is not a boolean"))),
        },
        Kind::Error if value.is_empty() => Found::Empty,
        // An error of another name (`#SPILL!`) is the text it is written as.
        Kind::Error => match error_text(value) {
            Some(fixed) => Found::Fixed(fixed),
            None => {
                text.clear();
                text.push_str(value);
                Found::Text
            }
        },
        _ if value.is_empty() => Found::Empty,
        _ => match number_of(kind, value) {
            Some(number) if date => Found::Typed(excel_date(number, is_1904)),
            Some(number) => Found::Typed(Cell::Float(number)),
            // A value without a type that is not a number is text.
            None if *kind == Kind::Unset => {
                text.clear();
                text.push_str(value);
                Found::Text
            }
            None => return Err(damaged(format_args!("`{value}` is not a number"))),
        },
    })
}

/// The number a value is, by the type of its cell: a cell that does not say
/// what it holds keeps the words `inf` and `nan` as text.
fn number_of(kind: &Kind, value: &str) -> Option<f64> {
    if *kind == Kind::Unset {
        number_or_text(value)
    } else {
        number(value)
    }
}

/// The text an error cell is kept as, by the error a sheet writes (`#N/A`
/// is kept as `#NA`); none for an error of another name.
pub(super) fn error_text(error: &str) -> Option<&'static str> {
    Some(match error {
        "#DIV/0!" => "#Div0",
        "#N/A" => "#NA",
        "#NAME?" => "#Name",
        "#NULL!" => "#Null",
        "#NUM!" => "#Num",
        "#REF!" => "#Ref",
        "#VALUE!" => "#Value",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_are_read_as_written() {
        assert_eq!(cell_position("A1").unwrap(), (0, 0));
        assert_eq!(cell_position("b12").unwrap(), (11, 1));
        assert_eq!(cell_position("XFD1048576").unwrap(), (1_048_575, 16_383));
        assert_eq!(position("7").unwrap(), (6, None));
        for wrong in ["", "A", "A0", "1A", "A-1", "A1:B2", "é1", "A99999999999"] {
            assert!(position(wrong).is_err(), "{wrong}");
        }
        assert!(cell_position("12").is_err());
    }
}
