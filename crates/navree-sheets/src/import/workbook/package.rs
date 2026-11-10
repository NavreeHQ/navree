//! The package of an XLSX or XLSB workbook: a part's relationships, which
//! say where the parts it uses are and what each of them is. A target is a
//! relative reference, resolved against the folder of the part that names
//! it, whatever folders and names the file uses.

use std::collections::HashMap;

use percent_encoding::percent_decode_str;
use quick_xml::events::Event;

use super::xml::{self, attributes_of, decode};
use super::{Archive, Budget, Damaged, damaged};

/// The relationships of the package itself, which name the workbook.
pub(super) const PACKAGE: &str = "_rels/.rels";

/// The longest target that can lead to a part: a name in a zip file is at
/// most 65,535 bytes. A longer one is read and leads nowhere, without being
/// resolved, which would list its folders.
const MAX_TARGET: usize = 65_535;

/// What a relationship's type says its target is, by the type's last word:
/// the Transitional, Strict and Microsoft types all end the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Workbook,
    Worksheet,
    Chartsheet,
    Dialogsheet,
    /// An Excel 4 macro sheet: it has no cells to read.
    Macrosheet,
    SharedStrings,
    Styles,
    /// A type that names none of these.
    Other,
    /// No type.
    Unset,
}

impl Kind {
    fn of(kind: Option<&str>) -> Kind {
        let Some(kind) = kind else {
            return Kind::Unset;
        };
        match kind.rsplit('/').next() {
            Some("officeDocument") => Kind::Workbook,
            Some("worksheet") => Kind::Worksheet,
            Some("chartsheet") => Kind::Chartsheet,
            Some("dialogsheet") => Kind::Dialogsheet,
            Some("xlMacrosheet" | "xlIntlMacrosheet") => Kind::Macrosheet,
            Some("sharedStrings") => Kind::SharedStrings,
            Some("styles") => Kind::Styles,
            _ => Kind::Other,
        }
    }
}

/// A relationship: the part it leads to and what that part is.
#[derive(Debug)]
pub(super) struct Relationship {
    pub(super) kind: Kind,
    pub(super) part: String,
}

/// A part's relationships. Many sheets may name one of them: each is held
/// once, and a sheet holds its place.
#[derive(Debug, Default)]
pub(super) struct Relationships {
    all: Vec<Relationship>,
    by_id: HashMap<String, usize>,
}

impl Relationships {
    /// The relationships in the part named `name` (`xl/_rels/workbook.xml.rels`)
    /// of a part in `folder`; none when the file has no such part. What they
    /// take is charged to `budget`.
    pub(super) fn read(
        archive: &Archive<'_>,
        budget: &Budget,
        name: &str,
        folder: &str,
    ) -> Result<Option<Relationships>, Damaged> {
        archive.read_opening(archive.name_of(name), budget, |source| {
            let mut xml = xml::reader(source);
            let mut buf = Vec::new();
            let mut found = Relationships::default();
            loop {
                match xml.next(&mut buf)? {
                    Event::Start(e) if e.local_name().as_ref() == "Relationship" => {
                        let [id, kind, target] = attributes_of(&e, ["Id", "Type", "Target"])?;
                        let Some(id) = id else {
                            continue;
                        };
                        let target = decode(target.unwrap_or_default())?;
                        let part = if target.len() > MAX_TARGET {
                            String::new()
                        } else {
                            existing(archive, resolve(folder, &target))
                        };
                        // The list's entry and the table's, besides the text.
                        budget.hold(id.len() + part.len() + 64)?;
                        found.by_id.insert(id.to_owned(), found.all.len());
                        found.all.push(Relationship {
                            kind: Kind::of(kind),
                            part,
                        });
                    }
                    Event::End(e) if e.local_name().as_ref() == "Relationships" => break,
                    Event::Eof => {
                        return Err(damaged(format_args!("{name} ends before it is complete")));
                    }
                    _ => {}
                }
            }
            Ok(found)
        })
    }

    /// Where the relationship `id` is in the list.
    pub(super) fn place_of(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    pub(super) fn get(&self, place: usize) -> Option<&Relationship> {
        self.all.get(place)
    }

    /// The part of the last relationship of the kind `kind`.
    pub(super) fn part_of(&self, kind: Kind) -> Option<&str> {
        self.all
            .iter()
            .rev()
            .find(|relationship| relationship.kind == kind)
            .map(|relationship| relationship.part.as_str())
    }
}

/// The part `part` as the file has it: percent-encoded in the reference and
/// not in the file's names, a name is decoded.
fn existing(archive: &Archive<'_>, part: String) -> String {
    if archive.has(&part) || !part.contains('%') {
        return part;
    }
    match percent_decode_str(&part).decode_utf8() {
        Ok(decoded) if archive.has(&decoded) => decoded.into_owned(),
        _ => part,
    }
}

/// The part a relationship's `target` leads to from a part in `folder`: from
/// the top of the package when it starts with `/`; `.` is nothing, and `..`
/// the folder above, past the top of the package nothing too.
pub(super) fn resolve(folder: &str, target: &str) -> String {
    let target = target.replace('\\', "/");
    let (from, path) = match target.strip_prefix('/') {
        Some(path) => ("", path),
        None => (folder, target.as_str()),
    };
    let mut segments: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            segment => segments.push(segment),
        }
    }
    segments.join("/")
}

/// The folder of `part`, with its last `/`.
pub(super) fn folder_of(part: &str) -> &str {
    part.rfind('/').map_or("", |at| &part[..=at])
}

/// The part that holds the relationships of `part`.
pub(super) fn relationships_of(part: &str) -> String {
    let folder = folder_of(part);
    let file = &part[folder.len()..];
    format!("{folder}_rels/{file}.rels")
}

/// Whether `part` is a binary one, as an XLSB workbook is.
pub(super) fn is_binary(part: &str) -> bool {
    part.len() >= 4
        && part
            .get(part.len() - 4..)
            .is_some_and(|end| end.eq_ignore_ascii_case(".bin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_resolve_as_relative_references() {
        for (folder, target, part) in [
            ("xl/", "worksheets/s.xml", "xl/worksheets/s.xml"),
            ("xl/", "../xl/s.xml", "xl/s.xml"),
            ("xl/", "./s.xml", "xl/s.xml"),
            ("xl/", "/xl/s.xml", "xl/s.xml"),
            ("xl/", "/s.xml", "s.xml"),
            ("xl/", "../../../s.xml", "s.xml"),
            ("xl/", "a/./b/../s.xml", "xl/a/s.xml"),
            ("xl/", "a\\s.xml", "xl/a/s.xml"),
            ("", "./xl/workbook.xml", "xl/workbook.xml"),
            ("", "xl//workbook.xml", "xl/workbook.xml"),
            ("xl/", "", "xl"),
        ] {
            assert_eq!(resolve(folder, target), part, "{folder} {target}");
        }
    }

    #[test]
    fn a_part_s_relationships_are_beside_it() {
        assert_eq!(relationships_of("xl/book.xml"), "xl/_rels/book.xml.rels");
        assert_eq!(relationships_of("book.bin"), "_rels/book.bin.rels");
        assert_eq!(folder_of("a/b/c.xml"), "a/b/");
        assert_eq!(folder_of("c.xml"), "");
        assert!(is_binary("xl/workbook.BIN"));
        assert!(!is_binary("xl/workbook.xml"));
        assert!(!is_binary("é"));
    }
}
