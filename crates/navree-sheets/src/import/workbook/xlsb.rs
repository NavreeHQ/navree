//! XLSB workbooks: binary records ([MS-XLSB]) in a zip file.
//!
//! A part is a run of records, each a type and a size (7 bits to a byte,
//! the high bit saying another byte follows: two bytes of type at most and
//! four of size) and that many bytes of body. Opening finds the workbook
//! through the package's relationships (`xl/workbook.bin` when they name
//! none), and reads the workbook's relationships, the shared strings
//! (`sharedStrings.bin`), the cell formats (`styles.bin`) and the workbook
//! (its sheets and date system). A sheet is then read as a stream of its
//! cell records.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{BufRead, Read};
use std::sync::atomic::AtomicBool;

use encoding_rs::UTF_16LE;

use super::dates::{builtin_shows_date, excel_date, shows_date};
use super::package::{self, Kind as PartKind, Relationships};
use super::xml::{MAX_LENGTH, too_long};
use super::{
    Archive, Budget, CellSource, Damaged, MAX_SHEETS, SharedStrings, SourceCell, Value, damaged,
    is_stop, too_many_sheets,
};
use crate::{Cell, SheetsError};

// Record types.
const ROW: u16 = 0x0000;
const CELL_RK: u16 = 0x0002;
const CELL_ERROR: u16 = 0x0003;
const CELL_BOOL: u16 = 0x0004;
const CELL_REAL: u16 = 0x0005;
const CELL_STRING: u16 = 0x0006;
const CELL_SHARED: u16 = 0x0007;
const FORMULA_STRING: u16 = 0x0008;
const FORMULA_NUMBER: u16 = 0x0009;
const FORMULA_BOOL: u16 = 0x000A;
const SHARED_STRING: u16 = 0x0013;
const FORMAT: u16 = 0x002C;
const CELL_FORMAT: u16 = 0x002F;
const END_SHEETS: u16 = 0x0090;
const BEGIN_SHEET_DATA: u16 = 0x0091;
const END_SHEET_DATA: u16 = 0x0092;
const SHEET_DIMENSION: u16 = 0x0094;
const WORKBOOK_PROPERTIES: u16 = 0x0099;
const SHEET: u16 = 0x009C;
const BEGIN_SHARED_STRINGS: u16 = 0x009F;
const BEGIN_FORMATS: u16 = 0x0267;
const BEGIN_CELL_FORMATS: u16 = 0x0269;

/// The last row a sheet may have, from 0; a row header past it ends the
/// sheet.
const LAST_ROW: u32 = 0x0010_0000;

/// An XLSB workbook, opened.
pub(crate) struct Xlsb<'a> {
    archive: Archive<'a>,
    strings: SharedStrings,
    /// Whether each cell format shows a number as a date.
    dates: Vec<bool>,
    /// Whether dates count from 1904.
    is_1904: bool,
    names: Vec<String>,
    /// The workbook's relationships.
    relationships: Relationships,
    /// The relationship of each sheet, by its place.
    sheets: Vec<usize>,
}

/// Whether the package's relationships name a binary workbook part.
pub(super) fn names_a_binary_workbook(archive: &Archive<'_>, budget: &Budget) -> bool {
    named_workbook(archive, budget).is_ok_and(|part| part.is_some())
}

/// The binary workbook part the package's relationships name.
fn named_workbook(archive: &Archive<'_>, budget: &Budget) -> Result<Option<String>, Damaged> {
    let package = Relationships::read(archive, budget, package::PACKAGE, "")?;
    Ok(package
        .as_ref()
        .and_then(|package| package.part_of(PartKind::Workbook))
        .filter(|part| package::is_binary(part))
        .map(str::to_owned))
}

impl<'a> Xlsb<'a> {
    pub(super) fn open(archive: &Archive<'a>, budget: &Budget) -> Result<Self, SheetsError> {
        let workbook =
            named_workbook(archive, budget)?.unwrap_or_else(|| "xl/workbook.bin".to_owned());
        let folder = package::folder_of(&workbook);
        let relationships = Relationships::read(
            archive,
            budget,
            &package::relationships_of(&workbook),
            folder,
        )?
        .unwrap_or_default();
        // Parts the relationships do not name are looked for where they
        // usually are.
        let named = |kind, name: &str| {
            relationships
                .part_of(kind)
                .map_or_else(|| format!("{folder}{name}"), str::to_owned)
        };
        let strings = shared_strings(
            archive,
            &named(PartKind::SharedStrings, "sharedStrings.bin"),
            budget,
        )?;
        let dates = date_formats(archive, &named(PartKind::Styles, "styles.bin"), budget)?;
        let (names, sheets, is_1904) = sheets(archive, &workbook, &relationships, budget)?;
        Ok(Xlsb {
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
        // These hold no cells, and no dimension to find.
        if is_cell_less(sheet) {
            return read(None);
        }
        let path = &sheet.part;
        let read = self
            .archive
            .read(self.archive.name_of(path), stop, |source| {
                let mut records = Records::new(source);
                // The sheet's dimension, then its cells, past the views and
                // the column widths.
                let dimension = records.find(SHEET_DIMENSION, &[])?;
                if dimension.len() < 16 {
                    return Err(short("the sheet's dimension").into());
                }
                records.find(
                    BEGIN_SHEET_DATA,
                    &[(0x0085, 0x0086), (0x0025, 0x0026), (0x0186, 0x0187)],
                )?;
                let mut cells = XlsbCells {
                    records,
                    strings: &self.strings,
                    dates: &self.dates,
                    is_1904: self.is_1904,
                    row: 0,
                    done: false,
                    text: String::new(),
                };
                read(Some(&mut cells))
            })?;
        read.ok_or_else(|| damaged(format_args!("the sheet part {path} is missing")).into())
    }
}

/// What kind of sheet a relationship leads to: for one that does not say, by
/// the folder its part is in.
fn kind_of_sheet(relationship: &package::Relationship) -> PartKind {
    if relationship.kind != PartKind::Unset {
        return relationship.kind;
    }
    let mut folders = relationship.part.rsplit('/').skip(1);
    match folders.next() {
        Some("worksheets") => PartKind::Worksheet,
        Some("chartsheets") => PartKind::Chartsheet,
        Some("dialogsheets") => PartKind::Dialogsheet,
        Some("macrosheets") => PartKind::Macrosheet,
        _ => PartKind::Other,
    }
}

/// Whether a sheet is of a kind that holds no cells.
fn is_cell_less(relationship: &package::Relationship) -> bool {
    matches!(
        kind_of_sheet(relationship),
        PartKind::Chartsheet | PartKind::Macrosheet
    )
}

fn short(what: &str) -> Damaged {
    damaged(format_args!("{what} is cut short"))
}

/// The records of a binary part.
struct Records<'r> {
    source: &'r mut dyn BufRead,
    body: Vec<u8>,
}

impl<'r> Records<'r> {
    fn new(source: &'r mut dyn BufRead) -> Self {
        Records {
            source,
            body: Vec::new(),
        }
    }

    fn byte(&mut self) -> Result<u8, Damaged> {
        let mut byte = [0u8];
        self.source.read_exact(&mut byte).map_err(|error| {
            // A part that ends is damaged; one that was told to stop is not.
            if is_stop(&error) {
                Damaged::from(error)
            } else {
                damaged("a part ends before its last record")
            }
        })?;
        Ok(byte[0])
    }

    /// The next record's type and body. A part ends with a record that
    /// says so, so its end is an error.
    fn next(&mut self) -> Result<(u16, &[u8]), Damaged> {
        let first = self.byte()?;
        let kind = if first & 0x80 == 0 {
            u16::from(first)
        } else {
            u16::from(first & 0x7F) | (u16::from(self.byte()? & 0x7F) << 7)
        };
        let mut size = 0u64;
        for at in 0..4 {
            let byte = self.byte()?;
            size |= u64::from(byte & 0x7F) << (7 * at);
            if byte & 0x80 == 0 {
                break;
            }
        }
        if size > MAX_LENGTH as u64 {
            return Err(too_long());
        }
        // Read as it comes: the body is held only once the part has it.
        self.body.clear();
        let read = (&mut self.source).take(size).read_to_end(&mut self.body)?;
        if read as u64 != size {
            return Err(damaged("a part ends inside a record"));
        }
        Ok((kind, &self.body))
    }

    /// The body of the next record of type `kind`. The records between
    /// are passed over, and a record that starts one of the `blocks` is
    /// passed over with every record up to the end of its block.
    fn find(&mut self, kind: u16, blocks: &[(u16, u16)]) -> Result<&[u8], Damaged> {
        loop {
            let (found, _) = self.next()?;
            if found == kind {
                return Ok(&self.body);
            }
            if let Some((_, end)) = blocks.iter().find(|(start, _)| *start == found) {
                while self.next()?.0 != *end {}
            }
        }
    }
}

fn u16_at(body: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(body.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(body: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(body.get(at..at + 4)?.try_into().ok()?))
}

/// A string as XLSB writes it (`XLWideString`): its length in UTF-16 code
/// units, then the units; and the bytes it takes. The units are UTF-16
/// whatever they start with, and a byte-order mark that starts the string is
/// not part of its text.
fn wide_string(body: &[u8]) -> Result<(Cow<'_, str>, usize), Damaged> {
    let units = u32_at(body, 0).ok_or_else(|| short("a string"))? as usize;
    let end = units
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(4))
        .ok_or_else(|| short("a string"))?;
    let text = body.get(4..end).ok_or_else(|| short("a string"))?;
    let text = text.strip_prefix(&[0xFF, 0xFE]).unwrap_or(text);
    Ok((UTF_16LE.decode_without_bom_handling(text).0, end))
}

/// The shared strings in the part `path`; none when the workbook has no such
/// part.
fn shared_strings(
    archive: &Archive<'_>,
    path: &str,
    budget: &Budget,
) -> Result<SharedStrings, SheetsError> {
    let strings = archive.read_opening(archive.name_of(path), budget, |source| {
        let mut records = Records::new(source);
        let begin = records.find(BEGIN_SHARED_STRINGS, &[])?;
        // The count of unique strings, which is read as the count of
        // strings to come, one record at a time.
        let mut count = u32_at(begin, 4).ok_or_else(|| short("the shared strings"))?;
        let mut strings = SharedStrings::within(budget);
        while count > 0 {
            let item = records.find(SHARED_STRING, &[(0x0023, 0x0024)])?;
            // A flags byte, then the text (its runs and phonetic text after).
            let (text, _) = wide_string(item.get(1..).unwrap_or_default())?;
            strings.push(&text)?;
            count -= 1;
        }
        strings.finish(budget);
        Ok::<_, SheetsError>(strings)
    })?;
    Ok(strings.unwrap_or_else(|| SharedStrings::within(budget)))
}

/// Whether each cell format in the part `path` shows a number as a date: by
/// its custom number format when it has one, by the built-in format's id when
/// not.
fn date_formats(archive: &Archive<'_>, path: &str, budget: &Budget) -> Result<Vec<bool>, Damaged> {
    let dates = archive.read_opening(archive.name_of(path), budget, |source| {
        let mut records = Records::new(source);
        // An id is 16 bits, so the table has a size of its own.
        let mut custom: HashMap<u16, bool> = HashMap::new();
        loop {
            let (kind, body) = records.next()?;
            match kind {
                BEGIN_FORMATS => {
                    let mut count = u32_at(body, 0).ok_or_else(|| short("the formats"))?;
                    while count > 0 {
                        let format = records.find(FORMAT, &[])?;
                        let id = u16_at(format, 0).ok_or_else(|| short("a format"))?;
                        let (code, _) = wide_string(format.get(2..).unwrap_or_default())?;
                        custom.insert(id, shows_date(&code));
                        count -= 1;
                    }
                }
                BEGIN_CELL_FORMATS => {
                    let mut count = u32_at(body, 0).ok_or_else(|| short("the cell formats"))?;
                    let mut dates = Vec::new();
                    while count > 0 {
                        let format = records.find(CELL_FORMAT, &[])?;
                        let id = u16_at(format, 2).ok_or_else(|| short("a cell format"))?;
                        budget.hold(1)?;
                        dates.push(
                            custom
                                .get(&id)
                                .copied()
                                .unwrap_or_else(|| builtin_shows_date(id)),
                        );
                        count -= 1;
                    }
                    return Ok::<_, Damaged>(dates);
                }
                _ => {}
            }
        }
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
        let mut records = Records::new(source);
        let (mut names, mut sheets, mut is_1904) = (Vec::new(), Vec::new(), false);
        loop {
            let (kind, body) = records.next()?;
            match kind {
                WORKBOOK_PROPERTIES => {
                    is_1904 = body
                        .first()
                        .ok_or_else(|| short("the workbook's properties"))?
                        & 1
                        != 0;
                }
                SHEET => {
                    let state = u32_at(body, 0).ok_or_else(|| short("a sheet"))?;
                    if state > 2 {
                        return Err(damaged(format_args!("the sheet state {state} is unknown")));
                    }
                    let units = u32_at(body, 8).ok_or_else(|| short("a sheet"))?;
                    // A sheet without a part is not one.
                    if units == u32::MAX {
                        continue;
                    }
                    let (id, used) = wide_string(body.get(8..).unwrap_or_default())?;
                    let place = relationships
                        .place_of(id.as_ref())
                        .ok_or_else(|| damaged(format_args!("the sheet part {id} is not named")))?;
                    let sheet = relationships
                        .get(place)
                        .ok_or_else(|| damaged(format_args!("the sheet part {id} is not named")))?;
                    if !matches!(
                        kind_of_sheet(sheet),
                        PartKind::Worksheet
                            | PartKind::Chartsheet
                            | PartKind::Dialogsheet
                            | PartKind::Macrosheet
                    ) {
                        return Err(damaged(format_args!(
                            "the sheet {} is of an unknown type",
                            sheet.part
                        )));
                    }
                    if names.len() >= MAX_SHEETS {
                        return Err(damaged(too_many_sheets()));
                    }
                    let (name, _) = wide_string(body.get(8 + used..).unwrap_or_default())?;
                    // A name, and its place in two lists.
                    budget.hold(name.len() + 32)?;
                    names.push(name.into_owned());
                    sheets.push(place);
                }
                END_SHEETS => return Ok::<_, Damaged>((names, sheets, is_1904)),
                _ => {}
            }
        }
    })?;
    found.ok_or_else(|| damaged(format_args!("the file has no {path} part")))
}

/// The cells of a sheet, as its part lists them.
struct XlsbCells<'w, 'r> {
    records: Records<'r>,
    strings: &'w SharedStrings,
    dates: &'w [bool],
    is_1904: bool,
    row: u32,
    done: bool,
    /// The text of the cell last read.
    text: String,
}

/// What a cell record holds.
enum Found {
    /// The text in `XlsbCells::text`.
    Text,
    Shared(usize),
    Fixed(&'static str),
    Typed(Cell),
}

impl CellSource for XlsbCells<'_, '_> {
    fn next_cell(&mut self) -> Result<Option<SourceCell<'_>>, SheetsError> {
        if self.done {
            return Ok(None);
        }
        let (row, column, found) = loop {
            let (kind, body) = self.records.next()?;
            let cell = || short("a cell");
            // A cell record starts with its column, its cell format (three
            // bytes and a flags byte) and the first byte of its value.
            let head: [u8; 9] = match body.get(..9).map(<[u8; 9]>::try_from) {
                Some(Ok(head)) => head,
                _ if matches!(kind, CELL_RK..=FORMULA_BOOL) => return Err(cell().into()),
                _ => [0; 9],
            };
            let date = {
                let style = u32::from_le_bytes([head[4], head[5], head[6], 0]) as usize;
                self.dates.get(style).copied().unwrap_or(false)
            };
            let number = |value: f64| -> Result<Cell, Damaged> {
                if value.is_nan() {
                    return Err(damaged("a cell holds NaN"));
                }
                // An infinity is the largest double of its sign.
                let value = value.clamp(-f64::MAX, f64::MAX);
                Ok(if date {
                    excel_date(value, self.is_1904)
                } else {
                    Cell::Float(value)
                })
            };
            let found = match kind {
                ROW => {
                    self.row = u32_at(body, 0).ok_or_else(|| short("a row"))?;
                    if self.row > LAST_ROW {
                        self.done = true;
                        return Ok(None);
                    }
                    continue;
                }
                END_SHEET_DATA => {
                    self.done = true;
                    return Ok(None);
                }
                CELL_RK => {
                    let rk = u32_at(body, 8).ok_or_else(cell)?;
                    let hundredths = rk & 1 != 0;
                    if rk & 2 != 0 {
                        let whole = i64::from((rk & !3) as i32 >> 2);
                        if hundredths {
                            Found::Typed(number(whole as f64 / 100.0)?)
                        } else if date {
                            // A whole number in a date format is a date.
                            Found::Typed(number(whole as f64)?)
                        } else {
                            Found::Typed(Cell::Int(whole))
                        }
                    } else {
                        let value = f64::from_bits(u64::from(rk & !3) << 32);
                        let value = if hundredths { value / 100.0 } else { value };
                        Found::Typed(number(value)?)
                    }
                }
                CELL_ERROR => Found::Fixed(match head[8] {
                    0x00 => "#Null",
                    0x07 => "#Div0",
                    0x0F => "#Value",
                    0x17 => "#Ref",
                    0x1D => "#Name",
                    0x24 => "#Num",
                    0x2A => "#NA",
                    0x2B => "#GettingData",
                    // An error of another code is kept by its code.
                    code => {
                        self.text = format!("#Error({code:#04x})");
                        break (self.row, u32_at(body, 0).ok_or_else(cell)?, Found::Text);
                    }
                }),
                CELL_BOOL | FORMULA_BOOL => Found::Typed(Cell::Bool(head[8] != 0)),
                CELL_REAL | FORMULA_NUMBER => {
                    let bits = body.get(8..16).ok_or_else(cell)?;
                    let value = f64::from_le_bytes(bits.try_into().map_err(|_| cell())?);
                    Found::Typed(number(value)?)
                }
                CELL_STRING | FORMULA_STRING => {
                    let (text, _) = wide_string(body.get(8..).unwrap_or_default())?;
                    self.text.clear();
                    self.text.push_str(&text);
                    Found::Text
                }
                CELL_SHARED => {
                    let index = u32_at(body, 8).ok_or_else(cell)? as usize;
                    if self.strings.get(index).is_none() {
                        return Err(
                            damaged(format_args!("the shared string {index} is missing")).into(),
                        );
                    }
                    Found::Shared(index)
                }
                _ => continue,
            };
            let column = u32_at(body, 0).ok_or_else(cell)?;
            break (self.row, column, found);
        };
        let value = match found {
            Found::Text => Value::Text(self.text.trim()),
            Found::Shared(index) => Value::Text(self.strings.get(index).unwrap_or_default()),
            Found::Fixed(text) => Value::Text(text),
            Found::Typed(cell) => Value::Typed(cell),
        };
        Ok(Some(SourceCell {
            row,
            column,
            columns: 1,
            value,
        }))
    }
}
