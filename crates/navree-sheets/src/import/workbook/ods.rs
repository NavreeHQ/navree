//! ODS spreadsheets: OpenDocument XML (`content.xml`) in a zip file.
//!
//! Opening checks the file's media type and that it is not encrypted, then
//! reads all of `content.xml` once, keeping only the tables' names, so that
//! a file that cannot be read is refused whichever sheet is asked for. A
//! sheet is then read again from `content.xml` as a stream of its rows.
//!
//! A row or a cell may say it repeats (`table:number-rows-repeated`,
//! `table:number-columns-repeated`). A repeated cell is given once with the
//! columns it covers, and repeated rows without a value are only counted;
//! a row with values is given once for each time it repeats, so the table's
//! limits count every copy. As spreadsheets do, a row stops at 16,384
//! columns and a sheet at 1,048,576 rows.
//!
//! A row's text is made as its cells are read, spaces and all: the text of
//! the whole row counts against `max_text_bytes` before it is made, and the
//! text of a cell in a column the table does not keep is only counted.

use std::collections::HashMap;
use std::io::BufRead;
use std::ops::Range;

use quick_xml::events::{BytesStart, Event};

use super::dates::{finite_number, iso_date, iso_duration};
use super::xlsx::error_text;
use super::xml::{self, Xml, attribute, attributes, decode, push_entity};
use super::{
    Archive, Budget, CellSource, Damaged, Limits, MAX_SHEETS, SourceCell, Value, damaged,
    too_many_sheets,
};
use crate::{Cell, SheetsError};

const MEDIA_TYPE: &[u8] = b"application/vnd.oasis.opendocument.spreadsheet";

/// The columns of a row, and the rows of a sheet, that are read.
const MAX_COLUMNS: u64 = 16_384;
const MAX_ROWS: u64 = 1_048_576;

/// An ODS spreadsheet, opened.
pub(crate) struct Ods<'a> {
    archive: Archive<'a>,
    limits: Limits,
    /// Whether times count from 1904, by the file's null date.
    is_1904: bool,
    names: Vec<String>,
    /// The table each sheet is read from, by its place among the named
    /// tables: the last of the tables with the sheet's name.
    tables: Vec<usize>,
}

impl<'a> Ods<'a> {
    pub(super) fn open(
        archive: &Archive<'a>,
        limits: Limits,
        budget: &Budget,
    ) -> Result<Self, SheetsError> {
        let media_type = archive.read_opening("mimetype", budget, |source| {
            let mut start = [0u8; MEDIA_TYPE.len()];
            source.read_exact(&mut start).map_err(Damaged::from)?;
            Ok::<_, Damaged>(start == MEDIA_TYPE)
        })?;
        match media_type {
            None => return Err(damaged("the file has no mimetype part").into()),
            Some(false) => return Err(damaged("the file is not an ODS spreadsheet").into()),
            Some(true) => {}
        }
        let encrypted =
            archive.read_opening("META-INF/manifest.xml", budget, |source| encrypted(source))?;
        match encrypted {
            None => return Err(damaged("the file has no META-INF/manifest.xml part").into()),
            Some(true) => return Err(damaged("the file is encrypted").into()),
            Some(false) => {}
        }
        let content = archive.read_opening("content.xml", budget, |source| {
            let mut content = OdsCells::new(source, None, limits, false, Some(budget));
            content.rest_in_order(None, &|| Ok(()))?;
            Ok::<_, SheetsError>((content.names, content.is_1904))
        })?;
        let (names, is_1904) =
            content.ok_or_else(|| damaged("the file has no content.xml part"))?;
        let last: HashMap<&str, usize> = names
            .iter()
            .enumerate()
            .map(|(at, name)| (name.as_str(), at))
            .collect();
        let tables = names
            .iter()
            .map(|name| last.get(name.as_str()).copied().unwrap_or(0))
            .collect();
        Ok(Ods {
            archive: archive.clone(),
            limits,
            is_1904,
            names,
            tables,
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
        let table = *self
            .tables
            .get(sheet)
            .ok_or_else(|| damaged("the workbook has no such sheet"))?;
        let read = self.archive.read("content.xml", |source| {
            let mut cells = OdsCells::new(source, Some(table), self.limits, self.is_1904, None);
            read(Some(&mut cells))
        })?;
        read.ok_or_else(|| damaged("the file has no content.xml part").into())
    }
}

/// Whether the manifest describes how a part is encrypted, after the first
/// part it lists.
fn encrypted(source: &mut dyn BufRead) -> Result<bool, Damaged> {
    let mut xml = xml::reader(source);
    let mut buf = Vec::new();
    let mut listed = false;
    loop {
        match xml.next(&mut buf)? {
            Event::Start(e) if e.name().as_ref() == "manifest:file-entry" => listed = true,
            Event::Start(e) if listed && e.name().as_ref() == "manifest:encryption-data" => {
                return Ok(true);
            }
            Event::Eof => return Ok(false),
            _ => {}
        }
    }
}

fn is_cell(name: &str) -> bool {
    matches!(name, "table:table-cell" | "table:covered-table-cell")
}

fn whitespace(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

/// A value of a cell in the row being read.
#[derive(Clone, Debug)]
enum Held {
    /// Text, trimmed, in the row's text.
    Text(Range<usize>),
    Typed(Cell),
}

/// A cell of the row being read, with a value.
#[derive(Clone, Debug)]
struct HeldCell {
    column: u32,
    columns: u32,
    value: Held,
}

/// Where the reading of `content.xml` is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// Outside the named tables.
    Outside,
    /// In a named table, whose cells are given or not; with `last`, the
    /// reading ends with it.
    Table { given: bool, last: bool },
    /// Past the table that is read, or at the end of the part.
    Done,
}

/// The cells of a table of `content.xml`; with no table to give, a reading
/// of every table that gives no cell and keeps their names.
struct OdsCells<'r> {
    xml: Xml<'r>,
    limits: Limits,
    /// The named table whose cells are given, by its place among them.
    target: Option<usize>,
    /// The named tables so far, and their names when no table is given,
    /// charged to what opening may hold.
    tables: usize,
    names: Vec<String>,
    budget: Option<&'r Budget>,
    place: Place,
    /// Whether times count from 1904, by the null date the file states.
    is_1904: bool,
    /// Whether the last style seen has a name, so that its table
    /// properties are read.
    named_style: bool,
    /// The row of the next row of the table.
    next_row: u64,
    /// The row being given: its cells, their text, the row it is given as
    /// now and how many times more it is given.
    row: Vec<HeldCell>,
    row_text: String,
    row_number: u32,
    rows_left: u64,
    /// The next cell of the row to give.
    at: usize,
    /// The columns from which a row's cells are not kept: what the table
    /// says, and once there is a first row with a value, which is the header,
    /// no further than it.
    cut: usize,
    header_end: Option<usize>,
    buf: Vec<u8>,
    row_buf: Vec<u8>,
    cell_buf: Vec<u8>,
}

impl<'r> OdsCells<'r> {
    fn new(
        source: &'r mut dyn BufRead,
        target: Option<usize>,
        limits: Limits,
        is_1904: bool,
        budget: Option<&'r Budget>,
    ) -> Self {
        OdsCells {
            xml: xml::reader(source),
            limits,
            is_1904,
            target,
            tables: 0,
            names: Vec::new(),
            budget,
            place: Place::Outside,
            named_style: false,
            next_row: 0,
            row: Vec::new(),
            row_text: String::new(),
            row_number: 0,
            rows_left: 0,
            at: 0,
            cut: usize::MAX,
            header_end: None,
            buf: Vec::new(),
            row_buf: Vec::new(),
            cell_buf: Vec::new(),
        }
    }

    /// The columns of a row that are kept, once the reader of the cells has
    /// said it cuts them.
    fn kept_columns(&self) -> usize {
        match self.cut {
            usize::MAX => usize::MAX,
            cut => cut.min(self.header_end.unwrap_or(usize::MAX)),
        }
    }

    /// Reads on to the next row of the table to give (`rows_left` is then
    /// more than 0), or to the end.
    fn advance(&mut self) -> Result<(), SheetsError> {
        loop {
            match self.place {
                Place::Done => return Ok(()),
                Place::Outside => match self.xml.next(&mut self.buf)? {
                    Event::Start(e) if e.name().as_ref() == "style:style" => {
                        self.named_style = match attribute(&e, "style:name")? {
                            Some(name) => decode(name).map(|_| true)?,
                            None => false,
                        };
                    }
                    Event::Start(e)
                        if self.named_style && e.name().as_ref() == "style:table-properties" =>
                    {
                        if let Some(display) = attribute(&e, "table:display")?
                            && !matches!(&*decode(display)?, "true" | "false")
                        {
                            return Err(damaged("a table's display is not a boolean").into());
                        }
                    }
                    // The date times count from: 1899-12-30 unless it says
                    // 1904-01-01.
                    Event::Start(e) if e.name().as_ref() == "table:null-date" => {
                        if let Some(day) = attribute(&e, "table:date-value")? {
                            self.is_1904 = decode(day)? == "1904-01-01";
                        }
                    }
                    Event::Start(e) if e.name().as_ref() == "table:table" => {
                        if let Some(style) = attribute(&e, "table:style-name")? {
                            decode(style)?;
                        }
                        if let Some(name) = attribute(&e, "table:name")? {
                            let name = decode(name)?;
                            let given = self.target == Some(self.tables);
                            if self.target.is_none() {
                                if self.names.len() >= MAX_SHEETS {
                                    return Err(SheetsError::UnsupportedFile(too_many_sheets()));
                                }
                                if let Some(budget) = self.budget {
                                    // A name, and its place in two lists.
                                    budget.hold(name.len() + 32)?;
                                }
                                self.names.push(name.into_owned());
                            }
                            self.tables += 1;
                            self.next_row = 0;
                            self.place = Place::Table { given, last: given };
                        }
                    }
                    Event::Start(e) if e.name().as_ref() == "table:named-expressions" => {
                        named_expressions(&mut self.xml, &mut self.row_buf)?;
                    }
                    Event::Eof => self.place = Place::Done,
                    _ => {}
                },
                Place::Table { given, last } => match self.xml.next(&mut self.buf)? {
                    Event::Start(e) if e.name().as_ref() == "table:table-row" => {
                        let repeats = match attribute(&e, "table:number-rows-repeated")? {
                            Some(repeats) => count(&decode(repeats)?)?,
                            None => 1,
                        };
                        let kept = self.kept_columns();
                        read_row(
                            &mut self.xml,
                            given.then_some((&mut self.row, &mut self.row_text)),
                            [&mut self.row_buf, &mut self.cell_buf],
                            self.limits,
                            self.is_1904,
                            kept,
                        )?;
                        if given && self.header_end.is_none() {
                            self.header_end = self.row.last().map(|cell| {
                                (cell.column as usize).saturating_add(cell.columns as usize)
                            });
                        }
                        let first = self.next_row;
                        let rows = repeats.min(MAX_ROWS.saturating_sub(first));
                        self.next_row = first + rows;
                        if given && rows > 0 && !self.row.is_empty() {
                            self.row_number = u32::try_from(first).unwrap_or(u32::MAX);
                            self.rows_left = rows;
                            self.at = 0;
                            return Ok(());
                        }
                    }
                    Event::End(e) if e.name().as_ref() == "table:table" => {
                        self.place = if last { Place::Done } else { Place::Outside };
                    }
                    Event::Eof => return Err(damaged("a table is not closed").into()),
                    _ => {}
                },
            }
        }
    }
}

impl CellSource for OdsCells<'_> {
    fn next_cell(&mut self) -> Result<Option<SourceCell<'_>>, SheetsError> {
        loop {
            if self.rows_left > 0 {
                if let Some(cell) = self.row.get(self.at) {
                    // Past the columns kept, only the last cell of the row
                    // counts: it tells how far the row reaches.
                    let cell = if cell.column as usize >= self.cut {
                        self.at = self.row.len();
                        self.row.last().unwrap_or(cell)
                    } else {
                        self.at += 1;
                        cell
                    };
                    let value = match &cell.value {
                        Held::Text(range) => {
                            Value::Text(self.row_text.get(range.clone()).unwrap_or_default())
                        }
                        Held::Typed(typed) => Value::Typed(typed.clone()),
                    };
                    return Ok(Some(SourceCell {
                        row: self.row_number,
                        column: cell.column,
                        columns: cell.columns,
                        value,
                    }));
                }
                self.rows_left -= 1;
                self.row_number = self.row_number.saturating_add(1);
                self.at = 0;
                continue;
            }
            if self.place == Place::Done {
                return Ok(None);
            }
            self.advance()?;
        }
    }

    fn cut_at(&mut self, width: usize) {
        self.cut = width;
    }

    /// The cells of a spreadsheet always come in order: the rest is read
    /// for whether it can be, without giving a cell.
    fn rest_in_order(
        &mut self,
        _last: Option<(u32, u32)>,
        running: &dyn Fn() -> Result<(), SheetsError>,
    ) -> Result<bool, SheetsError> {
        if let Place::Table { last, .. } = self.place {
            self.place = Place::Table { given: false, last };
        }
        self.rows_left = 0;
        self.row.clear();
        while self.place != Place::Done {
            running()?;
            self.advance()?;
        }
        Ok(true)
    }
}

/// A count of repeats.
fn count(text: &str) -> Result<u64, Damaged> {
    text.parse::<u64>()
        .map_err(|_| damaged(format_args!("`{text}` is not a count")))
}

/// Reads the named expressions of the spreadsheet, keeping nothing.
fn named_expressions(xml: &mut Xml<'_>, buf: &mut Vec<u8>) -> Result<(), Damaged> {
    loop {
        match xml.next(buf)? {
            Event::Start(e)
                if matches!(
                    e.name().as_ref(),
                    "table:named-range" | "table:named-expression"
                ) =>
            {
                for attribute in attributes(&e) {
                    let (key, value) = attribute?;
                    if matches!(
                        key,
                        "table:name" | "table:cell-range-address" | "table:expression"
                    ) {
                        decode(value)?;
                    }
                }
            }
            Event::End(e)
                if matches!(
                    e.name().as_ref(),
                    "table:named-range" | "table:named-expression"
                ) => {}
            Event::End(e) if e.name().as_ref() == "table:named-expressions" => return Ok(()),
            Event::Text(text) if whitespace(&text) => {}
            Event::Comment(_) | Event::PI(_) => {}
            _ => return Err(damaged("the named expressions hold something else")),
        }
    }
}

/// What is done with the text of a cell.
enum Keep<'t> {
    /// Nothing: the file is only being read through.
    Nothing,
    /// It is counted but not made: the cell is in a column the table does
    /// not keep, so all it says is how far its row reaches.
    Count,
    /// It is added to its row's text.
    Text(&'t mut String),
}

/// A cell the table does not keep: that it has a value is all it tells.
fn past_the_columns_kept() -> Held {
    Held::Typed(Cell::Bool(true))
}

/// Reads a row up to its end, into `held` (its cells with a value and their
/// text) when it is given. The cells from column `cut` on are not made.
fn read_row(
    xml: &mut Xml<'_>,
    mut held: Option<(&mut Vec<HeldCell>, &mut String)>,
    [buf, cell_buf]: [&mut Vec<u8>; 2],
    limits: Limits,
    is_1904: bool,
    cut: usize,
) -> Result<(), SheetsError> {
    if let Some((cells, text)) = held.as_mut() {
        cells.clear();
        text.clear();
    }
    let kept = (cut as u64).min(MAX_COLUMNS);
    let mut column = 0u64;
    loop {
        match xml.next(buf)? {
            Event::Start(e) if is_cell(e.name().as_ref()) => {
                let repeats = match attribute(&e, "table:number-columns-repeated")? {
                    Some(repeats) => count(repeats)?,
                    None => 1,
                };
                let start = column;
                column = column.saturating_add(repeats);
                let end = column.min(MAX_COLUMNS);
                let keep = match held.as_mut() {
                    None => Keep::Nothing,
                    Some(_) if start >= kept => Keep::Count,
                    Some((_, text)) => Keep::Text(text),
                };
                let (value, closed) = cell_value(xml, &e, keep, cell_buf, limits, is_1904)?;
                if !closed {
                    xml.skip(e.name(), cell_buf)?;
                }
                if let (Some((cells, _)), Some(value), true) = (held.as_mut(), value, start < end) {
                    cells.push(HeldCell {
                        column: start as u32,
                        columns: (end - start) as u32,
                        value,
                    });
                }
            }
            Event::End(e) if e.name().as_ref() == "table:table-row" => return Ok(()),
            Event::Text(text) if whitespace(&text) => {}
            Event::Comment(_) | Event::PI(_) => {}
            _ => return Err(damaged("a row holds something other than cells").into()),
        }
    }
}

/// The value of a cell (none when it has none), and whether its element was
/// read to its end. The first of the value attributes counts; text is the
/// cell's paragraphs when it has no `office:string-value`. An error cell
/// (`calcext:value-type="error"`) is the text of its paragraphs, whatever the
/// attributes say, as an error is in the other formats. Text is treated as
/// `keep` says, and its place in its row's text returned.
fn cell_value(
    xml: &mut Xml<'_>,
    cell: &BytesStart<'_>,
    mut keep: Keep<'_>,
    buf: &mut Vec<u8>,
    limits: Limits,
    is_1904: bool,
) -> Result<(Option<Held>, bool), SheetsError> {
    let start = match &keep {
        Keep::Text(text) => text.len(),
        _ => 0,
    };
    let mut value = None;
    let mut set = false;
    let mut string = false;
    let mut error = false;
    for attribute in attributes(cell) {
        let (key, raw) = attribute?;
        match key {
            "office:value" if !set => {
                let number = finite_number(raw)
                    .ok_or_else(|| damaged(format_args!("`{raw}` is not a number")))?;
                value = Some(Held::Typed(Cell::Float(number)));
                set = true;
            }
            "office:string-value" | "office:date-value" | "office:time-value" if !set => {
                let decoded = decode(raw)?;
                // A date or a time is one when it reads as one, and text
                // when it does not.
                let typed = match key {
                    "office:date-value" => iso_date(&decoded),
                    "office:time-value" => iso_duration(&decoded, is_1904),
                    _ => None,
                };
                value = match typed {
                    Some(cell) => Some(Held::Typed(cell)),
                    None => held_text(&mut keep, &decoded, limits)?,
                };
                set = true;
            }
            "office:boolean-value" if !set => {
                value = Some(Held::Typed(Cell::Bool(raw == "TRUE" || raw == "true")));
                set = true;
            }
            "office:value-type" if !set => string = raw == "string",
            "calcext:value-type" => error = raw == "error",
            "table:formula" => {
                decode(raw)?;
            }
            _ => {}
        }
    }
    if !error && (set || !string) {
        return Ok((value, false));
    }
    if let Keep::Text(text) = &mut keep {
        text.truncate(start);
    }
    let mut content = Content {
        start,
        keep,
        spaces: 0,
        started: false,
        paragraphs: 0,
        counted: 0,
        limit: limits.max_text_bytes,
    };
    loop {
        match xml.next(buf)? {
            Event::Text(more) => content.add(&more.xml10_content())?,
            Event::GeneralRef(entity) => {
                let mut character = String::new();
                push_entity(&entity, &mut character)?;
                content.add(&character)?;
            }
            Event::End(e) if is_cell(e.name().as_ref()) => break,
            Event::Start(e) if e.name().as_ref() == "office:annotation" => {
                xml.skip(e.name(), &mut Vec::new())?;
            }
            Event::Start(e) if e.name().as_ref() == "text:p" => {
                if content.paragraphs > 0 {
                    content.add(
                        "
",
                    )?;
                }
                content.paragraphs += 1;
            }
            Event::Start(e) if e.name().as_ref() == "text:tab" => content.add("\t")?,
            Event::Start(e) if e.name().as_ref() == "text:line-break" => content.add("\n")?,
            Event::Start(e) if e.name().as_ref() == "text:s" => {
                let repeats = match attribute(&e, "text:c")? {
                    Some(repeats) => count(&decode(repeats)?)?,
                    None => 1,
                };
                content.spaces = content.spaces.saturating_add(repeats);
            }
            Event::Eof => return Err(damaged("a cell is not closed").into()),
            _ => {}
        }
    }
    Ok((content.finish(error), true))
}

/// That a file's text comes to more than the limit.
fn too_much_text(limit: u64) -> SheetsError {
    SheetsError::UnsupportedFile(format!("The file holds more than {limit} bytes of text."))
}

/// The text of a cell's paragraphs, while it is read.
struct Content<'t> {
    keep: Keep<'t>,
    /// Where the cell's text starts in its row's text, when it is kept.
    start: usize,
    /// Spaces of `text:s` not yet added: they are added when text follows
    /// them, so spaces that end the cell are never made.
    spaces: u64,
    /// Whether the cell has text other than spaces.
    started: bool,
    paragraphs: usize,
    /// The text of a cell that is not made, so far.
    counted: u64,
    /// The most text a row may be made of.
    limit: u64,
}

impl Content<'_> {
    fn add(&mut self, more: &str) -> Result<(), SheetsError> {
        // Spaces at the start are trimmed with the rest, so not made.
        let spaces = if self.started { self.spaces } else { 0 };
        match &mut self.keep {
            Keep::Nothing => return Ok(()),
            Keep::Count => {
                self.counted = self.counted.saturating_add(spaces);
                if spaces > 0 && self.counted > self.limit {
                    return Err(too_much_text(self.limit));
                }
                self.counted = self.counted.saturating_add(more.len() as u64);
            }
            Keep::Text(text) => {
                let length = (text.len() as u64)
                    .saturating_add(spaces)
                    .saturating_add(more.len() as u64);
                if length > self.limit {
                    return Err(too_much_text(self.limit));
                }
                // Within the limit, so a number that fits.
                let spaces = spaces as usize;
                text.try_reserve(spaces + more.len())
                    .map_err(|_| too_much_text(self.limit))?;
                text.extend(std::iter::repeat_n(' ', spaces));
                text.push_str(more);
            }
        }
        self.spaces = 0;
        self.started |= !whitespace(more);
        Ok(())
    }

    /// The cell's text, trimmed, in the row's text; none when it is only
    /// spaces, and then taken out again. An error of a name the formats share
    /// is kept by the name they keep it as.
    fn finish(self, error: bool) -> Option<Held> {
        let text = match self.keep {
            Keep::Nothing => return None,
            Keep::Count => return self.started.then(past_the_columns_kept),
            Keep::Text(text) => text,
        };
        let cell = text.get(self.start..).unwrap_or_default();
        let trimmed = cell.trim();
        if trimmed.is_empty() {
            text.truncate(self.start);
            return None;
        }
        if error && let Some(fixed) = error_text(trimmed) {
            text.truncate(self.start);
            text.push_str(fixed);
            return Some(Held::Text(self.start..text.len()));
        }
        let from = self.start + (trimmed.as_ptr() as usize - cell.as_ptr() as usize);
        Some(Held::Text(from..from + trimmed.len()))
    }
}

/// `cell`, trimmed, as `keep` says: none when it is only spaces or is not
/// kept at all.
fn held_text(keep: &mut Keep<'_>, cell: &str, limits: Limits) -> Result<Option<Held>, SheetsError> {
    let trimmed = cell.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match keep {
        Keep::Nothing => Ok(None),
        Keep::Count => Ok(Some(past_the_columns_kept())),
        Keep::Text(text) => {
            let start = text.len();
            if (start as u64).saturating_add(trimmed.len() as u64) > limits.max_text_bytes {
                return Err(too_much_text(limits.max_text_bytes));
            }
            text.push_str(trimmed);
            Ok(Some(Held::Text(start..text.len())))
        }
    }
}
