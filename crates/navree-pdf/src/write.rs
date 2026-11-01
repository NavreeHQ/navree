//! [`Objects`]: a document's objects held to be changed, and the writer
//! that saves them as a PDF.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write as _;

use crate::document::{Document, MAX_HOPS};
use crate::error::PdfError;
use crate::filters;
use crate::object::{Dictionary, Object, ObjectId, Stream, StringFormat};
use crate::parse::{MAX_DEPTH, is_regular};

/// Objects a writer puts in one object stream (100).
const OBJECTS_PER_STREAM: usize = 100;
/// The highest object number kept as it is when a document is read to be
/// edited (8,388,607, the limit of PDF 1.7 on indirect objects); a
/// document with a higher one is renumbered from 1, so new numbers always
/// fit.
const MAX_KEPT_NUMBER: u32 = (1 << 23) - 1;
/// How deep the page tree may nest (256).
const MAX_TREE_DEPTH: usize = 256;

/// A document's objects and trailer, held in memory to be changed and
/// saved: what merging, stamping and other edits work on.
#[derive(Clone, Debug, Default)]
pub struct Objects {
    /// The objects, by id.
    pub objects: BTreeMap<ObjectId, Object>,
    /// The trailer: `Root`, `Info`, `ID`. Saving sets `Size`.
    pub trailer: Dictionary,
    /// The version the header names (`1.7`).
    pub version: String,
    /// The highest object number given out.
    max_number: u32,
}

impl Objects {
    /// No objects, version 1.7.
    pub fn new() -> Objects {
        Objects {
            version: "1.7".into(),
            ..Objects::default()
        }
    }

    /// Every object `document` can read, and its trailer, moved out of it:
    /// an error when a limit runs out while reading them. The objects keep
    /// their numbers, unless one is over 8,388,607: then they are numbered
    /// again from 1.
    pub fn read(document: Document) -> Result<Objects, PdfError> {
        let mut trailer = document.trailer().clone();
        for key in [&b"Size"[..], b"Prev", b"XRefStm", b"Encrypt"] {
            trailer.remove(key);
        }
        let version = if document.version().is_empty() {
            "1.7".into()
        } else {
            document.version().to_owned()
        };
        let listed = document.max_number().min(MAX_KEPT_NUMBER);
        let objects = document.into_objects()?;
        let highest = objects.keys().map(|id| id.0).max().unwrap_or(0);
        let mut read = Objects {
            objects,
            trailer,
            version,
            max_number: highest.max(listed),
        };
        if highest > MAX_KEPT_NUMBER {
            read.renumber(1);
        }
        Ok(read)
    }

    /// The highest object number given out.
    pub fn max_number(&self) -> u32 {
        self.max_number
    }

    /// A new object id, for an object inserted later.
    pub fn reserve(&mut self) -> ObjectId {
        self.max_number = self.max_number.saturating_add(1);
        (self.max_number, 0)
    }

    /// Adds `object` with a new id.
    pub fn add(&mut self, object: impl Into<Object>) -> ObjectId {
        let id = self.reserve();
        self.objects.insert(id, object.into());
        id
    }

    /// Puts `object` at `id`.
    pub fn insert(&mut self, id: ObjectId, object: impl Into<Object>) {
        self.max_number = self.max_number.max(id.0);
        self.objects.insert(id, object.into());
    }

    /// Object `id`.
    pub fn get(&self, id: ObjectId) -> Option<&Object> {
        self.objects.get(&id)
    }

    /// Object `id`, to change.
    pub fn get_mut(&mut self, id: ObjectId) -> Option<&mut Object> {
        self.objects.get_mut(&id)
    }

    /// The dictionary object `id` is.
    pub fn dictionary(&self, id: ObjectId) -> Option<&Dictionary> {
        self.get(id).and_then(Object::as_dict)
    }

    /// The dictionary object `id` is, to change.
    pub fn dictionary_mut(&mut self, id: ObjectId) -> Option<&mut Dictionary> {
        self.get_mut(id).and_then(Object::as_dict_mut)
    }

    /// `object`, or the object it refers to (through up to 128 references).
    pub fn resolve<'a>(&'a self, object: &'a Object) -> Option<&'a Object> {
        let mut object = object;
        for _ in 0..MAX_HOPS {
            match object {
                Object::Reference(id) => object = self.get(*id)?,
                other => return Some(other),
            }
        }
        None
    }

    /// The pages, in order, read as [`Document::pages`] reads them.
    pub fn pages(&self) -> Vec<ObjectId> {
        let mut pages = Vec::new();
        let Some(root) = self
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .and_then(|id| self.dictionary(id))
            .and_then(|catalog| catalog.get(b"Pages"))
            .and_then(Object::as_reference)
        else {
            return pages;
        };
        let kids = |node: ObjectId| -> &[Object] {
            self.dictionary(node)
                .and_then(|node| self.resolve(node.get(b"Kids")?))
                .and_then(Object::as_array)
                .map_or(&[], Vec::as_slice)
        };
        let mut steps = self.objects.len();
        let mut stack: Vec<&[Object]> = Vec::new();
        let mut current = kids(root);
        loop {
            while let Some((kid, rest)) = current.split_first() {
                if steps == 0 {
                    return pages;
                }
                steps -= 1;
                current = rest;
                let Some(id) = kid.as_reference() else {
                    continue;
                };
                let kind = self.dictionary(id).map(|node| {
                    node.type_name()
                        .or_else(|| node.has(b"Linearized").then_some(&b"Linearized"[..]))
                });
                match kind {
                    Some(Some(b"Page")) => pages.push(id),
                    Some(Some(b"Pages")) if stack.len() < MAX_TREE_DEPTH => {
                        if !current.is_empty() {
                            stack.push(current);
                        }
                        current = kids(id);
                    }
                    Some(_) => {}
                    None => pages.push(id),
                }
            }
            match stack.pop() {
                Some(next) => current = next,
                None => return pages,
            }
        }
    }

    /// Gives every object a new number from `first` on, in the order of
    /// their ids, generation 0, and changes every reference to match. A
    /// reference to an object that does not exist becomes `null`. Returns
    /// each old id's new one.
    pub fn renumber(&mut self, first: u32) -> HashMap<ObjectId, ObjectId> {
        let map: HashMap<ObjectId, ObjectId> = self
            .objects
            .keys()
            .zip(first..)
            .map(|(&old, new)| (old, (new, 0)))
            .collect();
        let objects = std::mem::take(&mut self.objects);
        self.objects = objects
            .into_iter()
            .map(|(id, mut object)| {
                renumber_in(&mut object, &map, 0);
                (map.get(&id).copied().unwrap_or(id), object)
            })
            .collect();
        for (_, value) in self.trailer.iter_mut() {
            renumber_in(value, &map, 0);
        }
        self.max_number = self
            .objects
            .keys()
            .map(|id| id.0)
            .max()
            .unwrap_or(0)
            .max(first.saturating_sub(1));
        map
    }

    /// Drops the objects nothing reachable from the trailer refers to.
    pub fn prune(&mut self) {
        let mut reached: HashSet<ObjectId> = HashSet::new();
        let mut queue: Vec<ObjectId> = Vec::new();
        for (_, value) in self.trailer.iter() {
            references(value, &mut queue, 0);
        }
        while let Some(id) = queue.pop() {
            if !reached.insert(id) {
                continue;
            }
            if let Some(object) = self.objects.get(&id) {
                references(object, &mut queue, 0);
            }
        }
        self.objects.retain(|id, _| reached.contains(id));
    }

    /// Compresses (Flate) every stream that has no filter, when that makes
    /// it smaller.
    pub fn compress(&mut self) {
        for object in self.objects.values_mut() {
            if let Object::Stream(stream) = object
                && !stream.dict.has(b"Filter")
            {
                let packed = filters::deflate(&stream.content);
                if packed.len() + 19 < stream.content.len() {
                    stream.dict.set("Filter", "FlateDecode");
                    stream.content = packed.into();
                }
            }
        }
    }

    /// The document as a PDF, with a cross-reference table. An error only
    /// for a document too large for the offsets a cross-reference can hold.
    pub fn save(&self) -> Result<Vec<u8>, PdfError> {
        self.save_with(false)
    }

    /// The document as a PDF; with `object_streams`, the objects other than
    /// streams go into compressed object streams, and the cross-reference
    /// is a stream.
    pub fn save_with(&self, object_streams: bool) -> Result<Vec<u8>, PdfError> {
        let mut out = Vec::new();
        let _ = write!(out, "%PDF-{}\n%\u{e2}\u{e3}\u{cf}\u{d3}\n", self.version);
        // Where each object is: at an offset (with its generation), or in
        // an object stream (with its index there).
        let mut entries: BTreeMap<u32, (u8, u64, u16)> = BTreeMap::new();
        let too_large = || PdfError::TooLarge("a PDF too large for its cross-reference".into());
        let mut next = self.max_number;
        let mut members: Vec<(&ObjectId, &Object)> = Vec::new();
        for (id, object) in &self.objects {
            if object_streams && id.1 == 0 && !matches!(object, Object::Stream(_)) {
                members.push((id, object));
            } else {
                entries.insert(id.0, (1, out.len() as u64, id.1));
                write_indirect(&mut out, *id, object);
            }
        }
        for group in members.chunks(OBJECTS_PER_STREAM) {
            next = next.checked_add(1).ok_or_else(too_large)?;
            let mut header = Vec::new();
            let mut body = Vec::new();
            for (index, (id, object)) in group.iter().enumerate() {
                let _ = write!(header, "{} {} ", id.0, body.len());
                write_object(&mut body, object);
                body.push(b'\n');
                entries.insert(id.0, (2, u64::from(next), index as u16));
            }
            let first = header.len();
            header.extend_from_slice(&body);
            let mut dict = Dictionary::new();
            dict.set("Type", "ObjStm");
            dict.set("N", group.len() as i64);
            dict.set("First", first as i64);
            dict.set("Filter", "FlateDecode");
            let stream = Stream::new(dict, filters::deflate(&header));
            entries.insert(next, (1, out.len() as u64, 0));
            write_indirect(&mut out, (next, 0), &Object::Stream(stream));
        }
        let mut trailer = self.trailer.clone();
        for key in [&b"Prev"[..], b"XRefStm", b"Encrypt"] {
            trailer.remove(key);
        }
        let start = out.len() as u64;
        if object_streams {
            // The cross-reference stream is the last object.
            let own = next.checked_add(1).ok_or_else(too_large)?;
            entries.insert(own, (1, start, 0));
            entries.insert(0, (0, 0, 65535));
            trailer.set("Size", i64::from(own) + 1);
            let mut rows = Vec::with_capacity(entries.len() * 7);
            for &(kind, second, third) in entries.values() {
                rows.push(kind);
                rows.extend_from_slice(
                    &u32::try_from(second)
                        .map_err(|_| too_large())?
                        .to_be_bytes(),
                );
                rows.extend_from_slice(&third.to_be_bytes());
            }
            trailer.set("Type", "XRef");
            trailer.set(
                "W",
                vec![Object::Integer(1), Object::Integer(4), Object::Integer(2)],
            );
            trailer.set("Index", runs(entries.keys()));
            trailer.set("Filter", "FlateDecode");
            let stream = Stream::new(trailer, filters::deflate(&rows));
            write_indirect(&mut out, (own, 0), &Object::Stream(stream));
        } else {
            entries.insert(0, (0, 0, 65535));
            trailer.set("Size", i64::from(next) + 1);
            out.extend_from_slice(b"xref\n");
            let numbers: Vec<u32> = entries.keys().copied().collect();
            for run in numbers.chunk_by(|a, b| a.checked_add(1) == Some(*b)) {
                let _ = writeln!(out, "{} {}", run[0], run.len());
                for number in run {
                    let (kind, offset, generation) =
                        entries.get(number).copied().unwrap_or((0, 0, 0));
                    if offset > 9_999_999_999 {
                        return Err(too_large());
                    }
                    let kind = if kind == 0 { 'f' } else { 'n' };
                    let _ = write!(out, "{offset:010} {generation:05} {kind}\r\n");
                }
            }
            out.extend_from_slice(b"trailer\n");
            write_object(&mut out, &Object::Dictionary(trailer));
            out.push(b'\n');
        }
        let _ = write!(out, "startxref\n{start}\n%%EOF\n");
        Ok(out)
    }
}

/// The runs of consecutive `numbers`, as `/Index` lists them: first number,
/// count.
fn runs<'a>(numbers: impl Iterator<Item = &'a u32>) -> Vec<Object> {
    let numbers: Vec<u32> = numbers.copied().collect();
    numbers
        .chunk_by(|a, b| a.checked_add(1) == Some(*b))
        .flat_map(|run| [Object::from(run[0]), Object::Integer(run.len() as i64)])
        .collect()
}

/// Replaces every reference in `object` through `map`.
fn renumber_in(object: &mut Object, map: &HashMap<ObjectId, ObjectId>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    match object {
        Object::Reference(id) => match map.get(id) {
            Some(new) => *id = *new,
            None => *object = Object::Null,
        },
        Object::Array(items) => {
            for item in items {
                renumber_in(item, map, depth + 1);
            }
        }
        Object::Dictionary(dict) => {
            for (_, value) in dict.iter_mut() {
                renumber_in(value, map, depth + 1);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.iter_mut() {
                renumber_in(value, map, depth + 1);
            }
        }
        _ => {}
    }
}

/// Pushes every reference in `object` onto `queue`.
fn references(object: &Object, queue: &mut Vec<ObjectId>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    match object {
        Object::Reference(id) => queue.push(*id),
        Object::Array(items) => {
            for item in items {
                references(item, queue, depth + 1);
            }
        }
        Object::Dictionary(dict) => {
            for (_, value) in dict.iter() {
                references(value, queue, depth + 1);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.iter() {
                references(value, queue, depth + 1);
            }
        }
        _ => {}
    }
}

fn write_indirect(out: &mut Vec<u8>, (number, generation): ObjectId, object: &Object) {
    let _ = writeln!(out, "{number} {generation} obj");
    write_object(out, object);
    out.extend_from_slice(b"\nendobj\n");
}

/// Writes `object` as PDF syntax. A stream's `Length` is set from its
/// bytes.
pub(crate) fn write_object(out: &mut Vec<u8>, object: &Object) {
    match object {
        Object::Null => out.extend_from_slice(b"null"),
        Object::Boolean(value) => out.extend_from_slice(if *value { b"true" } else { b"false" }),
        Object::Integer(value) => {
            let _ = write!(out, "{value}");
        }
        Object::Real(value) => write_real(out, *value),
        Object::Name(name) => write_name(out, name),
        Object::String(bytes, StringFormat::Literal) => {
            out.push(b'(');
            for &byte in bytes {
                match byte {
                    b'(' | b')' | b'\\' => {
                        out.push(b'\\');
                        out.push(byte);
                    }
                    b'\r' => out.extend_from_slice(b"\\r"),
                    _ => out.push(byte),
                }
            }
            out.push(b')');
        }
        Object::String(bytes, StringFormat::Hexadecimal) => {
            out.push(b'<');
            for byte in bytes {
                let _ = write!(out, "{byte:02X}");
            }
            out.push(b'>');
        }
        Object::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b' ');
                }
                write_object(out, item);
            }
            out.push(b']');
        }
        Object::Dictionary(dict) => write_dictionary(out, dict),
        Object::Stream(stream) => {
            let mut dict = stream.dict.clone();
            dict.set("Length", stream.content.len() as i64);
            write_dictionary(out, &dict);
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(&stream.content);
            out.extend_from_slice(b"\nendstream");
        }
        Object::Reference((number, generation)) => {
            let _ = write!(out, "{number} {generation} R");
        }
    }
}

fn write_dictionary(out: &mut Vec<u8>, dict: &Dictionary) {
    out.extend_from_slice(b"<<");
    for (key, value) in dict.iter() {
        write_name(out, key);
        out.push(b' ');
        write_object(out, value);
    }
    out.extend_from_slice(b">>");
}

/// A name, with `#xx` for every byte that is not a regular printable
/// character.
pub(crate) fn write_name(out: &mut Vec<u8>, name: &[u8]) {
    out.push(b'/');
    for &byte in name {
        if is_regular(byte) && byte != b'#' && (0x21..=0x7E).contains(&byte) {
            out.push(byte);
        } else {
            let _ = write!(out, "#{byte:02X}");
        }
    }
}

/// A real without an exponent, with a decimal point so that it reads back
/// as a real; one that is not a number is 0.
pub(crate) fn write_real(out: &mut Vec<u8>, value: f32) {
    if value.is_finite() {
        let start = out.len();
        let _ = write!(out, "{value}");
        if !out.get(start..).unwrap_or_default().contains(&b'.') {
            out.extend_from_slice(b".0");
        }
    } else {
        out.push(b'0');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary;
    use crate::parse;

    fn written(object: &Object) -> Vec<u8> {
        let mut out = Vec::new();
        write_object(&mut out, object);
        out
    }

    #[test]
    fn objects_written_read_back_the_same() {
        let objects = [
            Object::Null,
            Object::Boolean(true),
            Object::Integer(-42),
            Object::Real(0.5),
            Object::Real(-1234.25),
            Object::name("A B#(c)"),
            Object::string_literal(b"a (b) \\ c\r\nd".to_vec()),
            Object::String(vec![0xFE, 0xFF, 0, 0x41], StringFormat::Hexadecimal),
            Object::Array(vec![1.into(), Object::Reference((3, 1)), "N".into()]),
            Object::Dictionary(
                dictionary! { "Type" => "Page", "Kids" => vec![Object::Reference((2, 0))] },
            ),
        ];
        for object in objects {
            let bytes = written(&object);
            assert_eq!(
                parse::object(&bytes).as_ref(),
                Some(&object),
                "{}",
                String::from_utf8_lossy(&bytes)
            );
        }
        assert_eq!(written(&Object::Real(1e-7)), b"0.0000001");
        assert_eq!(written(&Object::Real(12.0)), b"12.0");
        assert_eq!(written(&Object::Real(f32::NAN)), b"0");
    }

    #[test]
    fn renumbering_moves_every_reference() {
        let mut objects = Objects::new();
        let a = objects.reserve();
        let b = objects.add(dictionary! { "Next" => a, "Gone" => Object::Reference((99, 0)) });
        objects.insert(a, dictionary! { "Back" => b });
        objects.trailer.set("Root", b);
        objects.renumber(10);
        assert_eq!(
            objects.trailer.get(b"Root"),
            Some(&Object::Reference((11, 0)))
        );
        let moved = objects.dictionary((11, 0)).unwrap();
        assert_eq!(moved.get(b"Next"), Some(&Object::Reference((10, 0))));
        assert_eq!(moved.get(b"Gone"), Some(&Object::Null));
        assert_eq!(objects.max_number(), 11);
    }

    #[test]
    fn a_document_with_the_highest_object_numbers_is_renumbered_to_be_edited() {
        let mut objects = Objects::new();
        let (page, pages, catalog) = ((u32::MAX, 0), (u32::MAX - 1, 0), (u32::MAX - 2, 0));
        objects.insert(page, dictionary! { "Type" => "Page", "Parent" => pages });
        objects.insert(
            pages,
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        );
        objects.insert(
            catalog,
            dictionary! { "Type" => "Catalog", "Pages" => pages },
        );
        objects.trailer.set("Root", catalog);
        // With object streams, there is no number left for them.
        assert!(matches!(
            objects.save_with(true),
            Err(PdfError::TooLarge(_))
        ));
        let saved = objects.save().unwrap();
        let document = Document::load(saved, crate::ReadLimits::default()).unwrap();
        assert_eq!(document.pages(), [page]);
        let mut read = Objects::read(document).unwrap();
        assert_eq!(read.max_number(), 3);
        assert_eq!(read.reserve(), (4, 0));
        assert_eq!(read.pages().len(), 1);
        read.save_with(true).unwrap();
    }

    #[test]
    fn pruning_keeps_what_the_trailer_reaches() {
        let mut objects = Objects::new();
        let kept = objects.add(Object::Integer(1));
        let root = objects.add(dictionary! { "K" => kept });
        objects.add(Object::Integer(2));
        objects.trailer.set("Root", root);
        objects.prune();
        assert_eq!(objects.objects.len(), 2);
    }
}
