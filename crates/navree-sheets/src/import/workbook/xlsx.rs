//! XLSX workbooks (and XLSM): SpreadsheetML parts in a zip file.
//!
//! Opening reads the package's relationships (where the workbook is), the
//! shared strings, the cell formats (which show dates), the workbook's
//! relationships and the workbook (its sheets and date system). A sheet is
//! then read as a stream of its cells.

use std::collections::HashMap;
use std::io::BufRead;

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;

use super::dates::{builtin_id_shows_date, excel_date, shows_date};
use super::xml::{self, attribute, attributes, attributes_of, decode, push_entity, unescape_excel};
use super::{Archive, CellSource, Damaged, Limits, SharedStrings, SourceCell, Value, damaged};
use crate::{Cell, SheetsError};

type Xml<'r> = quick_xml::Reader<&'r mut dyn BufRead>;

/// An XLSX workbook, opened.
pub(crate) struct Xlsx<'a> {
    archive: Archive<'a>,
    strings: SharedStrings,
    /// Whether each cell format (`cellXfs`) shows a number as a date.
    dates: Vec<bool>,
    /// Whether dates count from 1904.
    is_1904: bool,
    names: Vec<String>,
    /// The part of each sheet.
    paths: Vec<String>,
}

impl<'a> Xlsx<'a> {
    pub(super) fn open(archive: &Archive<'a>, limits: Limits) -> Result<Self, SheetsError> {
        let folder = workbook_folder(archive)?;
        let strings = shared_strings(archive, &folder, limits)?;
        let dates = date_formats(archive, &folder)?;
        let relationships = relationships(archive, &folder)?;
        let (names, paths, is_1904) = sheets(archive, &folder, &relationships)?;
        Ok(Xlsx {
            archive: archive.clone(),
            strings,
            dates,
            is_1904,
            names,
            paths,
        })
    }

    pub(super) fn sheet_names(&self) -> &[String] {
        &self.names
    }

    pub(super) fn read_cells<T>(
        &self,
        sheet: usize,
        read: impl FnOnce(Option<&mut dyn CellSource>) -> Result<T, SheetsError>,
    ) -> Result<T, SheetsError> {
        let path = self
            .paths
            .get(sheet)
            .ok_or_else(|| damaged("the workbook has no such sheet"))?;
        let read = self.archive.read(self.archive.name_of(path), |source| {
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

/// The next event of `xml`, read into `buf`.
fn next<'b>(xml: &mut Xml<'_>, buf: &'b mut Vec<u8>) -> Result<Event<'b>, Damaged> {
    buf.clear();
    Ok(xml.read_event_into(buf)?)
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

/// The folder of the workbook part, which the package's relationships name
/// (`xl/`).
fn workbook_folder(archive: &Archive<'_>) -> Result<String, Damaged> {
    let target = archive.read(archive.name_of("_rels/.rels"), |source| {
        let mut xml = xml::reader(source);
        let mut buf = Vec::new();
        loop {
            match next(&mut xml, &mut buf)? {
                Event::Start(e) if is(&e, "Relationships") => break,
                Event::Eof => return Err(missing("_rels/.rels")),
                _ => {}
            }
        }
        let mut target = None;
        loop {
            match next(&mut xml, &mut buf)? {
                Event::Start(e) if is(&e, "Relationship") => {
                    let [kind, to] = attributes_of(&e, ["Type", "Target"])?;
                    if let (true, Some(to)) = (
                        kind.is_some_and(|kind| kind.ends_with("/relationships/officeDocument")),
                        to,
                    ) {
                        target = Some(decode(to)?.into_owned());
                    }
                }
                Event::End(e) if ends(&e, "Relationships") => break,
                Event::Eof => return Err(missing("_rels/.rels")),
                _ => {}
            }
        }
        Ok(target)
    })?;
    let target = target
        .ok_or_else(|| damaged("the file has no _rels/.rels part"))?
        .ok_or_else(|| damaged("the file names no workbook part"))?;
    let folder = match target.rfind('/') {
        Some(at) => &target[..=at],
        None => "",
    };
    Ok(folder.strip_prefix('/').unwrap_or(folder).to_owned())
}

/// The shared strings, rich text joined; none when the workbook has no
/// part for them. The count the part declares is not used: the strings are
/// counted as they come.
fn shared_strings(
    archive: &Archive<'_>,
    folder: &str,
    limits: Limits,
) -> Result<SharedStrings, SheetsError> {
    let path = format!("{folder}sharedStrings.xml");
    let strings = archive.read(archive.name_of(&path), |source| {
        let mut strings = SharedStrings::new(limits.max_part_bytes);
        let mut xml = xml::reader(source);
        let (mut buf, mut inner, mut text_buf) = (Vec::new(), Vec::new(), Vec::new());
        loop {
            match next(&mut xml, &mut buf)? {
                Event::Start(e) if is(&e, "sst") => {
                    attribute(&e, "uniqueCount")?;
                    break;
                }
                Event::Eof => return Err(missing("the shared strings").into()),
                _ => {}
            }
        }
        loop {
            match next(&mut xml, &mut buf)? {
                Event::Start(e) if is(&e, "si") => {
                    let text = read_string(&mut xml, e.name(), &mut inner, &mut text_buf)?;
                    strings.push(&text)?;
                }
                Event::End(e) if ends(&e, "sst") => break,
                Event::Eof => return Err(missing("the shared strings").into()),
                _ => {}
            }
        }
        Ok::<_, SheetsError>(strings)
    })?;
    Ok(strings.unwrap_or_else(|| SharedStrings::new(limits.max_part_bytes)))
}

/// The text of a string element (`<si>`, `<is>`) up to its end `closing`:
/// its `<t>` texts joined, without the phonetic runs (`<rPh>`). A `<t>`
/// that does not preserve its spaces loses the spaces, tabs and line breaks
/// round it.
fn read_string(
    xml: &mut Xml<'_>,
    closing: QName<'_>,
    buf: &mut Vec<u8>,
    text_buf: &mut Vec<u8>,
) -> Result<String, Damaged> {
    let mut text = String::new();
    let mut phonetic = false;
    loop {
        match next(xml, buf)? {
            Event::Start(e) if is(&e, "rPh") => phonetic = true,
            Event::End(e) if e.name() == closing => return Ok(text),
            Event::End(e) if ends(&e, "rPh") => phonetic = false,
            Event::Start(e) if is(&e, "t") && !phonetic => {
                let preserve = attribute(&e, "xml:space")? == Some("preserve");
                let mut value = String::new();
                loop {
                    match next(xml, text_buf)? {
                        Event::Text(t) => value.push_str(&t.xml10_content()),
                        Event::CData(t) => value.push_str(&t.xml10_content()),
                        Event::GeneralRef(entity) => push_entity(&entity, &mut value)?,
                        Event::End(end) if end.name() == e.name() => break,
                        Event::Eof => return Err(missing("a string")),
                        _ => {}
                    }
                }
                let value = if preserve {
                    &value
                } else {
                    value.trim_matches([' ', '\t', '\r', '\n'])
                };
                text.push_str(&unescape_excel(value));
            }
            Event::Eof => return Err(missing("a string")),
            _ => {}
        }
    }
}

/// Whether each cell format shows a number as a date: by its custom number
/// format (`numFmts`) when it has one, by the built-in format's id when not.
fn date_formats(archive: &Archive<'_>, folder: &str) -> Result<Vec<bool>, Damaged> {
    let path = format!("{folder}styles.xml");
    let dates = archive.read(archive.name_of(&path), |source| {
        let mut xml = xml::reader(source);
        let (mut buf, mut inner) = (Vec::new(), Vec::new());
        let mut custom: HashMap<String, String> = HashMap::new();
        let mut dates = Vec::new();
        loop {
            match next(&mut xml, &mut buf)? {
                Event::Start(e) if is(&e, "numFmts") => loop {
                    match next(&mut xml, &mut inner)? {
                        Event::Start(e) if is(&e, "numFmt") => {
                            if let [Some(id), Some(code)] =
                                attributes_of(&e, ["numFmtId", "formatCode"])?
                            {
                                custom.insert(id.to_owned(), decode(code)?.into_owned());
                            }
                        }
                        Event::End(e) if ends(&e, "numFmts") => break,
                        Event::Eof => return Err(missing("the styles")),
                        _ => {}
                    }
                },
                Event::Start(e) if is(&e, "cellXfs") => loop {
                    match next(&mut xml, &mut inner)? {
                        Event::Start(e) if is(&e, "xf") => {
                            dates.push(attribute(&e, "numFmtId")?.is_some_and(|id| {
                                custom.get(id).map_or_else(
                                    || builtin_id_shows_date(id),
                                    |code| shows_date(code),
                                )
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

/// The workbook's relationships: target and type by id.
fn relationships(
    archive: &Archive<'_>,
    folder: &str,
) -> Result<HashMap<String, (String, String)>, Damaged> {
    let path = format!("{folder}_rels/workbook.xml.rels");
    let found = archive.read(archive.name_of(&path), |source| {
        let mut xml = xml::reader(source);
        let mut buf = Vec::new();
        let mut relationships = HashMap::new();
        loop {
            match next(&mut xml, &mut buf)? {
                Event::Start(e) if is(&e, "Relationship") => {
                    if let [Some(id), kind, target] = attributes_of(&e, ["Id", "Type", "Target"])? {
                        relationships.insert(
                            id.to_owned(),
                            (
                                target.unwrap_or_default().to_owned(),
                                kind.unwrap_or_default().to_owned(),
                            ),
                        );
                    }
                }
                Event::End(e) if ends(&e, "Relationships") => break,
                Event::Eof => return Err(missing("the workbook's relationships")),
                _ => {}
            }
        }
        Ok(relationships)
    })?;
    found.ok_or_else(|| damaged(format_args!("the file has no {path} part")))
}

/// The sheets' names and parts, and whether dates count from 1904.
fn sheets(
    archive: &Archive<'_>,
    folder: &str,
    relationships: &HashMap<String, (String, String)>,
) -> Result<(Vec<String>, Vec<String>, bool), Damaged> {
    let path = format!("{folder}workbook.xml");
    let found = archive.read(archive.name_of(&path), |source| {
        let mut xml = xml::reader(source);
        let (mut buf, mut inner) = (Vec::new(), Vec::new());
        let (mut names, mut paths, mut is_1904) = (Vec::new(), Vec::new(), false);
        loop {
            match next(&mut xml, &mut buf)? {
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
                    let (target, kind) = relationships
                        .get(id)
                        .ok_or_else(|| damaged(format_args!("the sheet `{name}` has no part")))?;
                    if !matches!(
                        kind.rsplit('/').next(),
                        Some("worksheet" | "chartsheet" | "dialogsheet")
                    ) {
                        return Err(damaged(format_args!(
                            "the sheet `{name}` is of an unknown type"
                        )));
                    }
                    paths.push(match target.strip_prefix('/') {
                        Some(absolute) => absolute.to_owned(),
                        None => format!("{folder}{target}"),
                    });
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
                        let mut value = String::new();
                        loop {
                            match next(&mut xml, &mut inner)? {
                                Event::GeneralRef(entity) => push_entity(&entity, &mut value)?,
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
        Ok((names, paths, is_1904))
    })?;
    Ok(found.unwrap_or_default())
}

/// Reads a sheet part up to its cells (`<sheetData>`): false when the part
/// has none to read, as a chart sheet does.
fn find_sheet_data(xml: &mut Xml<'_>) -> Result<bool, Damaged> {
    let mut buf = Vec::new();
    let mut other = false;
    loop {
        match next(xml, &mut buf)? {
            Event::Start(e) if is(&e, "dimension") => {
                let range = attribute(&e, "ref")?
                    .ok_or_else(|| damaged("the sheet's dimension has no range"))?;
                let corners: Vec<&str> = range.split(':').collect();
                if corners.len() > 2 {
                    return Err(damaged(format_args!("the range {range} is not a range")));
                }
                for corner in corners {
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
            match next(&mut self.xml, &mut self.buf)? {
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
                        match next(&mut self.xml, &mut self.cell_buf)? {
                            Event::Start(value) => {
                                found = read_value(
                                    &mut self.xml,
                                    &value,
                                    &kind,
                                    date,
                                    self.is_1904,
                                    self.strings,
                                    [&mut self.value_buf, &mut self.text_buf],
                                    &mut self.text,
                                )?;
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
/// (`<is>`) or its formula (`<f>`, which holds no value).
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
) -> Result<Found, Damaged> {
    let name = element.name();
    match element.local_name().as_ref() {
        "is" => {
            *text = read_string(xml, name, buf, text_buf)?;
            Ok(Found::Text)
        }
        "v" if *kind == Kind::Inline => {
            xml.read_to_end_into(name, buf)?;
            Ok(Found::Empty)
        }
        "v" if matches!(
            kind,
            Kind::Unset | Kind::Number | Kind::Shared | Kind::Bool | Kind::Error
        ) =>
        {
            // These values are numbers or names: the first text is the value.
            let found = match next(xml, buf)? {
                Event::Text(value) => plain_value(&value, kind, date, is_1904, strings, text)?,
                Event::End(end) if end.name() == name => return Ok(Found::Empty),
                Event::Eof => return Err(missing("a value")),
                _ => Found::Empty,
            };
            xml.read_to_end_into(name, buf)?;
            Ok(found)
        }
        "v" => {
            text.clear();
            loop {
                match next(xml, buf)? {
                    Event::Text(value) => text.push_str(&value.xml10_content()),
                    Event::GeneralRef(entity) => push_entity(&entity, text)?,
                    Event::End(end) if end.name() == name => break,
                    Event::Eof => return Err(missing("a value")),
                    _ => {}
                }
            }
            match kind {
                Kind::Date => Ok(Found::Text),
                Kind::Formula => {
                    if let std::borrow::Cow::Owned(unescaped) = unescape_excel(text) {
                        *text = unescaped;
                    }
                    Ok(Found::Text)
                }
                _ => Err(damaged(format_args!(
                    "a cell has the unknown type {kind:?}"
                ))),
            }
        }
        "f" => {
            xml.read_to_end_into(name, buf)?;
            Ok(Found::Empty)
        }
        _ => Err(damaged("a cell holds an element that is not a value")),
    }
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
        Kind::Error => Found::Fixed(error_text(value)?),
        _ if value.is_empty() => Found::Empty,
        _ => match value.parse::<f64>() {
            Ok(number) if date => Found::Typed(excel_date(number, is_1904)),
            Ok(number) => Found::Typed(Cell::Float(number)),
            // A value without a type that is not a number is text.
            Err(_) if *kind == Kind::Unset => {
                text.clear();
                text.push_str(value);
                Found::Text
            }
            Err(_) => return Err(damaged(format_args!("`{value}` is not a number"))),
        },
    })
}

/// The text an error cell is kept as, by the error a sheet writes (`#N/A`).
pub(super) fn error_text(error: &str) -> Result<&'static str, Damaged> {
    Ok(match error {
        "#DIV/0!" => "#Div0",
        "#N/A" => "#NA",
        "#NAME?" => "#Name",
        "#NULL!" => "#Null",
        "#NUM!" => "#Num",
        "#REF!" => "#Ref",
        "#VALUE!" => "#Value",
        other => return Err(damaged(format_args!("`{other}` is not an error"))),
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
