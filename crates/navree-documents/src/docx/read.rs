//! Reading a DOCX file: the package (a zip archive of parts) and the text
//! of its main document part.
//!
//! Every figure that comes from the file is checked against a limit before
//! anything is sized from it: the archive's entry count, the size each part
//! declares, the bytes a part actually unpacks to (counted as they are
//! produced), the depth of the XML and the length of the text. The document
//! part is read as a stream of XML events, so what the reader holds does not
//! grow with the file except for the text it returns and the largest single
//! element or text of the part.

use std::cell::Cell;
use std::io::{self, BufReader, Cursor, Read};

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::DocumentsError;

/// What reading a DOCX file may take, so that a small file cannot make a
/// call run for long or fill the memory. A limit that is passed is
/// [`DocumentsError::TooLarge`] (a 413), and the message names it.
///
/// ```
/// use navree_documents::docx::DocxLimits;
///
/// let limits = DocxLimits {
///     max_text_bytes: 1024 * 1024,
///     ..DocxLimits::default()
/// };
/// assert_eq!(limits.max_entries, 10_000);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DocxLimits {
    /// The most entries the archive may have, directories included (default
    /// 10,000).
    pub max_entries: usize,
    /// The most bytes one part may unpack to (default 64 MiB). Checked
    /// against the size the archive declares for the part before it is read,
    /// and against the bytes actually produced while it is read. The part is
    /// read as a stream; the most it holds at once is its largest single
    /// element or text, so this bounds that memory too.
    pub max_part_bytes: u64,
    /// The most bytes all the parts read for one call may unpack to together
    /// (default 128 MiB): the package's content types and relationships and
    /// the main document.
    pub max_inflated_bytes: u64,
    /// The most bytes of text a call may return (default 64 MiB). The text is
    /// held whole, so this is memory as well.
    pub max_text_bytes: usize,
    /// How deep elements of the document may nest (default 256). Real
    /// documents stay below 50, with tables inside tables inside content
    /// controls.
    pub max_depth: usize,
}

impl Default for DocxLimits {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_part_bytes: 64 * 1024 * 1024,
            max_inflated_bytes: 128 * 1024 * 1024,
            max_text_bytes: 64 * 1024 * 1024,
            max_depth: 256,
        }
    }
}

/// The namespaces of WordprocessingML: the transitional one Word writes and
/// the strict one of ISO/IEC 29500.
const WORD_NAMESPACES: [&str; 2] = [
    "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    "http://purl.oclc.org/ooxml/wordprocessingml/main",
];
/// The namespace of markup compatibility (`mc:AlternateContent`).
const MARKUP_COMPATIBILITY: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
/// The relationship type of the main document, transitional and strict.
const DOCUMENT_RELATIONSHIPS: [&str; 2] = [
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument",
    "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument",
];
/// The content types of a Word document's main part: a document and a
/// template, with and without macros.
const DOCUMENT_CONTENT_TYPES: [&str; 4] = [
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml",
    "application/vnd.ms-word.document.macroEnabled.main+xml",
    "application/vnd.ms-word.template.macroEnabledTemplate.main+xml",
];
/// Where the relationships of the package itself are.
const PACKAGE_RELATIONSHIPS: &str = "_rels/.rels";
/// The part that gives the content type of every other part.
const CONTENT_TYPES: &str = "[Content_Types].xml";
/// The main part when the relationships do not name one.
const DEFAULT_DOCUMENT: &str = "word/document.xml";

/// The text of the main document part of `bytes`, or, with `keep_text` off,
/// nothing: the file is read and checked to the end all the same.
pub(super) fn read_text(
    bytes: &[u8],
    limits: &DocxLimits,
    keep_text: bool,
) -> Result<String, DocumentsError> {
    let archive = ZipArchive::new(Cursor::new(bytes)).map_err(damaged)?;
    if archive.has_duplicate_names() {
        return Err(damaged("two entries have the same name"));
    }
    if archive.len() > limits.max_entries {
        return Err(DocumentsError::TooLarge(format!(
            "the DOCX file has more than {} entries",
            limits.max_entries
        )));
    }
    let mut package = Package {
        archive,
        limits,
        used: Cell::new(0),
    };
    let document = package.main_part()?;
    let index = package
        .find(&document)?
        .ok_or_else(|| damaged("the package has no main document"))?;
    package.part(index, |xml| document_text(xml, limits, keep_text))
}

/// The error for a file that is not a Word document.
fn damaged(reason: impl std::fmt::Display) -> DocumentsError {
    DocumentsError::Docx(format!("not a Word document: {reason}"))
}

/// A zip archive of parts and how much of them has been read.
struct Package<'a> {
    archive: ZipArchive<Cursor<&'a [u8]>>,
    limits: &'a DocxLimits,
    /// The bytes all the parts read so far unpacked to.
    used: Cell<u64>,
}

impl Package<'_> {
    /// The index of the entry named `name`, by the names the central
    /// directory gives (a name in an extra field does not count): without
    /// regard to case, with `\` as `/` and without a leading `/`. `None`
    /// when there is none; an error when two have the name.
    fn find(&self, name: &str) -> Result<Option<usize>, DocumentsError> {
        let wanted = canonical(name);
        let mut found = None;
        for (index, raw) in self.archive.file_names_raw().enumerate() {
            let Ok(entry) = std::str::from_utf8(raw) else {
                continue;
            };
            if canonical(entry) != wanted {
                continue;
            }
            if found.replace(index).is_some() {
                return Err(damaged(format!("two entries are named `{name}`")));
            }
        }
        Ok(found)
    }

    /// Runs `read` over the XML of entry `index`, as it unpacks. The entry
    /// is refused before it is read when the archive declares it larger than
    /// what the limits leave, and the unpacked bytes are counted as they are
    /// produced.
    fn part<T>(
        &mut self,
        index: usize,
        read: impl FnOnce(&mut Xml<'_>) -> Result<T, DocumentsError>,
    ) -> Result<T, DocumentsError> {
        let total_left = self
            .limits
            .max_inflated_bytes
            .saturating_sub(self.used.get());
        let by_total = total_left < self.limits.max_part_bytes;
        let cap = self.limits.max_part_bytes.min(total_left);
        let limits = self.limits;
        let refused = move || {
            DocumentsError::TooLarge(if by_total {
                format!(
                    "the DOCX file unpacks to more than {} bytes in all",
                    limits.max_inflated_bytes
                )
            } else {
                format!(
                    "a part of the DOCX file unpacks to more than {} bytes",
                    limits.max_part_bytes
                )
            })
        };
        let file = self.archive.by_index(index).map_err(damaged)?;
        if file.is_dir() {
            return Err(damaged("a part is a directory"));
        }
        if file.size() > cap {
            return Err(refused());
        }
        let part_used = Cell::new(0);
        let tripped = Cell::new(false);
        let inner: Box<dyn Read + '_> = Box::new(file);
        let metered = Metered {
            inner,
            left: cap,
            used: &part_used,
            tripped: &tripped,
        };
        let mut reader = NsReader::from_reader(BufReader::new(metered));
        let config = reader.config_mut();
        config.check_end_names = true;
        config.expand_empty_elements = false;
        config.trim_text(false);
        let mut xml = Xml {
            reader,
            tripped: &tripped,
        };
        let result = read(&mut xml);
        drop(xml);
        self.used.set(self.used.get() + part_used.get());
        match result {
            Err(_) if tripped.get() => Err(refused()),
            result => result,
        }
    }

    /// The name of the main document part: the target of the package's
    /// `officeDocument` relationship. The package must declare a content
    /// type for it that is a Word document's.
    fn main_part(&mut self) -> Result<String, DocumentsError> {
        let max_depth = self.limits.max_depth;
        let types = self
            .find(CONTENT_TYPES)?
            .ok_or_else(|| damaged("the package has no [Content_Types].xml"))?;
        let rels = self
            .find(PACKAGE_RELATIONSHIPS)?
            .ok_or_else(|| damaged("the package has no _rels/.rels"))?;
        let document = self.part(rels, |xml| {
            let mut found = None;
            xml.each_element(max_depth, |_, element| {
                if found.is_some() || element.local_name().into_inner() != "Relationship" {
                    return;
                }
                let is_document = attribute(element, "Type")
                    .is_some_and(|kind| DOCUMENT_RELATIONSHIPS.contains(&kind.as_str()));
                let external = attribute(element, "TargetMode").is_some_and(|m| m == "External");
                if is_document && !external {
                    found =
                        Some(attribute(element, "Target").and_then(|target| part_name(&target)));
                }
            })?;
            Ok(found)
        })?;
        let document = match document {
            Some(Some(name)) => name,
            Some(None) => return Err(damaged("the main document is not a part of the package")),
            None => DEFAULT_DOCUMENT.to_owned(),
        };
        let content_type = self.part(types, |xml| {
            let wanted = canonical(&document);
            let extension = wanted
                .rsplit_once('.')
                .map(|(_, extension)| extension.to_owned());
            let mut by_name = None;
            let mut by_extension = None;
            xml.each_element(max_depth, |_, element| {
                match element.local_name().into_inner() {
                    "Override" => {
                        let named = attribute(element, "PartName")
                            .is_some_and(|name| canonical(&name) == wanted);
                        if named {
                            by_name = attribute(element, "ContentType");
                        }
                    }
                    "Default" => {
                        let matches = attribute(element, "Extension").is_some_and(|candidate| {
                            extension
                                .as_deref()
                                .is_some_and(|extension| candidate.eq_ignore_ascii_case(extension))
                        });
                        if matches {
                            by_extension = attribute(element, "ContentType");
                        }
                    }
                    _ => {}
                }
            })?;
            Ok(by_name.or(by_extension))
        })?;
        match content_type {
            Some(kind)
                if DOCUMENT_CONTENT_TYPES
                    .iter()
                    .any(|word| kind.eq_ignore_ascii_case(word)) =>
            {
                Ok(document)
            }
            Some(kind) => Err(damaged(format!(
                "the main part is `{kind}`, not a Word document"
            ))),
            None => Err(damaged("the main part has no content type")),
        }
    }
}

/// A part name as compared: lower case, `/`-separated, without a leading `/`.
fn canonical(name: &str) -> String {
    name.replace('\\', "/")
        .trim_start_matches('/')
        .to_ascii_lowercase()
}

/// The part a relationship target names, from the package's root: without
/// `.` and empty segments. `None` for a target that climbs out of the
/// package.
fn part_name(target: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for segment in target.trim_start_matches(['/', '\\']).split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            segment => parts.push(segment),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The value of the attribute of `element` whose local name is `name`.
fn attribute(element: &BytesStart<'_>, name: &str) -> Option<String> {
    let mut attributes = element.attributes();
    // Attributes are not checked for duplicates: that is quadratic in their
    // number.
    attributes.with_checks(false);
    attributes
        .flatten()
        .find(|attribute| attribute.key.local_name().into_inner() == name)
        .and_then(|attribute| {
            attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .ok()
                .map(|value| value.into_owned())
        })
}

/// Counts the bytes read through it and fails once more than `left` come.
struct Metered<'a, R> {
    inner: R,
    left: u64,
    used: &'a Cell<u64>,
    tripped: &'a Cell<bool>,
}

impl<R: Read> Read for Metered<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // One byte more than is left, so that a stream past its limit shows.
        let room = usize::try_from(self.left.saturating_add(1)).unwrap_or(usize::MAX);
        let take = buf.len().min(room);
        let n = self.inner.read(&mut buf[..take])?;
        let produced = n as u64;
        if produced > self.left {
            self.tripped.set(true);
            return Err(io::Error::other("the part unpacks to more than its limit"));
        }
        self.left -= produced;
        self.used.set(self.used.get() + produced);
        Ok(n)
    }
}

/// The XML of one part, read event by event.
struct Xml<'a> {
    reader: NsReader<BufReader<Metered<'a, Box<dyn Read + 'a>>>>,
    /// Whether the part's limit stopped the stream.
    tripped: &'a Cell<bool>,
}

impl Xml<'_> {
    /// Calls `visit` for every element start and empty element, with the
    /// element's namespace, until the end of the part. A part that is not
    /// well-formed is an error.
    fn each_element(
        &mut self,
        max_depth: usize,
        mut visit: impl FnMut(&ResolveResult<'_>, &BytesStart<'_>),
    ) -> Result<(), DocumentsError> {
        let tripped = self.tripped;
        let mut buf = Vec::new();
        let mut depth = 0usize;
        loop {
            buf.clear();
            let (namespace, event) = self
                .reader
                .read_resolved_event_into(&mut buf)
                .map_err(|error| xml_error(&error, tripped))?;
            match event {
                Event::Start(element) => {
                    if depth >= max_depth {
                        return Err(too_deep(max_depth));
                    }
                    visit(&namespace, &element);
                    depth += 1;
                }
                Event::Empty(element) => visit(&namespace, &element),
                Event::End(_) => depth = depth.saturating_sub(1),
                Event::DocType(_) => return Err(damaged("a part has a DTD")),
                Event::Eof if depth == 0 => return Ok(()),
                Event::Eof => return Err(damaged("a part ends inside an element")),
                _ => {}
            }
        }
    }
}

/// An XML error; when the part's limit is what stopped the stream, an error
/// that `Package::part` turns into the limit that was passed.
fn xml_error(error: &quick_xml::Error, tripped: &Cell<bool>) -> DocumentsError {
    if tripped.get() {
        return DocumentsError::TooLarge(String::new());
    }
    damaged(error)
}

/// What an element of the main part means to the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    Document,
    Paragraph,
    Run,
    Text,
    Tab,
    Break,
    CarriageReturn,
    NoBreakHyphen,
    Drawing,
    Table,
    Row,
    Cell,
    /// Properties, deleted text, legacy pictures and fallbacks: the element
    /// and everything in it is ignored.
    Skip,
    /// Anything else: read through.
    Other,
}

fn tag(namespace: &ResolveResult<'_>, local: &str) -> Tag {
    let in_word = match namespace {
        ResolveResult::Bound(name) => WORD_NAMESPACES.contains(&name.0),
        // A prefix nothing declares is the usual one.
        ResolveResult::Unknown(prefix) => prefix == "w",
        ResolveResult::Unbound => false,
    };
    if in_word {
        return match local {
            "document" => Tag::Document,
            "p" => Tag::Paragraph,
            "r" => Tag::Run,
            "t" => Tag::Text,
            "tab" => Tag::Tab,
            "br" => Tag::Break,
            "cr" => Tag::CarriageReturn,
            "noBreakHyphen" => Tag::NoBreakHyphen,
            "drawing" => Tag::Drawing,
            "tbl" => Tag::Table,
            "tr" => Tag::Row,
            "tc" => Tag::Cell,
            "pPr" | "rPr" | "tblPr" | "trPr" | "tcPr" | "tblGrid" | "sectPr" | "sdtPr"
            | "sdtEndPr" | "del" | "moveFrom" | "pict" | "object" | "txbxContent" => Tag::Skip,
            _ => Tag::Other,
        };
    }
    let in_alternative = match namespace {
        ResolveResult::Bound(name) => name.0 == MARKUP_COMPATIBILITY,
        ResolveResult::Unknown(prefix) => prefix == "mc",
        ResolveResult::Unbound => false,
    };
    if in_alternative && local == "Fallback" {
        return Tag::Skip;
    }
    Tag::Other
}

/// The text being collected, within a limit.
struct Sink {
    text: Option<String>,
    len: usize,
    max: usize,
}

impl Sink {
    fn push(&mut self, piece: &str) -> Result<(), DocumentsError> {
        self.len = self.len.saturating_add(piece.len());
        if self.len > self.max {
            return Err(DocumentsError::TooLarge(format!(
                "the DOCX text is over {} bytes",
                self.max
            )));
        }
        if let Some(text) = &mut self.text {
            text.push_str(piece);
        }
        Ok(())
    }

    fn push_char(&mut self, c: char) -> Result<(), DocumentsError> {
        let mut buf = [0u8; 4];
        self.push(c.encode_utf8(&mut buf))
    }
}

/// Where the text of the body stands: which paragraph it is in, which cell.
#[derive(Default)]
struct Position {
    /// Paragraphs open (a paragraph inside a paragraph is part of it).
    paragraphs: usize,
    /// Whether the open paragraph's text is part of the text.
    counted: bool,
    runs: usize,
    texts: usize,
    /// Tables open: only the outermost one has rows and cells in the text.
    tables: usize,
    /// Whether a row of the outermost table is open.
    row: bool,
    /// Cells in the open row so far.
    cells: usize,
    /// Whether a cell of the outermost table is open.
    in_cell: bool,
    /// Paragraphs in the open cell so far.
    cell_paragraphs: usize,
    /// Whether a line of text has been started.
    lines: bool,
}

impl Position {
    /// Whether characters here are text: inside a `w:t` of a run of a
    /// paragraph that counts.
    fn in_text(&self) -> bool {
        self.in_run() && self.texts > 0
    }

    /// Whether the position is in a run of a paragraph that counts.
    fn in_run(&self) -> bool {
        self.counted && self.runs > 0
    }
}

/// The text of the document part `xml`: a line for each paragraph and each
/// row, the cells of a row separated by tabs.
fn document_text(
    xml: &mut Xml<'_>,
    limits: &DocxLimits,
    keep_text: bool,
) -> Result<String, DocumentsError> {
    let tripped = xml.tripped;
    let mut sink = Sink {
        text: keep_text.then(String::new),
        len: 0,
        max: limits.max_text_bytes,
    };
    let mut at = Position::default();
    let mut buf = Vec::new();
    let mut depth = 0usize;
    let mut roots = 0usize;
    // Levels left to close in an ignored element; 0 outside one.
    let mut skipping = 0usize;
    loop {
        buf.clear();
        let (namespace, event) = xml
            .reader
            .read_resolved_event_into(&mut buf)
            .map_err(|error| xml_error(&error, tripped))?;
        let (element, empty) = match &event {
            Event::Start(element) => (Some(element), false),
            Event::Empty(element) => (Some(element), true),
            _ => (None, false),
        };
        if let Some(element) = element {
            if depth >= limits.max_depth {
                return Err(too_deep(limits.max_depth));
            }
            if skipping > 0 {
                if !empty {
                    skipping += 1;
                    depth += 1;
                }
                continue;
            }
            let kind = tag(&namespace, element.local_name().into_inner());
            if depth == 0 {
                roots += 1;
                if roots > 1 {
                    return Err(damaged("the document has more than one root"));
                }
                if kind != Tag::Document {
                    return Err(damaged("the main part is not a document"));
                }
            }
            open(kind, element, &mut at, &mut sink)?;
            if empty {
                close(kind, &mut at);
            } else {
                depth += 1;
                if matches!(kind, Tag::Skip | Tag::Drawing) {
                    skipping = 1;
                }
            }
            continue;
        }
        match event {
            Event::End(element) => {
                depth = depth.saturating_sub(1);
                if skipping > 0 {
                    skipping -= 1;
                    continue;
                }
                close(tag(&namespace, element.local_name().into_inner()), &mut at);
            }
            Event::Text(text) if skipping == 0 && at.in_text() => {
                sink.push(&text.xml10_content())?;
            }
            Event::CData(data) if skipping == 0 && at.in_text() => {
                sink.push(&data.xml10_content())?;
            }
            Event::GeneralRef(reference) => {
                let c = match reference.resolve_char_ref() {
                    Ok(Some(c)) => c,
                    Ok(None) => match &*reference {
                        "lt" => '<',
                        "gt" => '>',
                        "amp" => '&',
                        "apos" => '\'',
                        "quot" => '"',
                        other => return Err(damaged(format!("`&{other};` is not defined"))),
                    },
                    Err(error) => return Err(damaged(error)),
                };
                if skipping == 0 && at.in_text() {
                    sink.push_char(c)?;
                }
            }
            Event::DocType(_) => return Err(damaged("the document has a DTD")),
            Event::Eof if depth == 0 && roots == 1 => break,
            Event::Eof => {
                return Err(damaged(if roots == 0 {
                    "the document is empty"
                } else {
                    "the document ends inside an element"
                }));
            }
            _ => {}
        }
    }
    Ok(sink.text.unwrap_or_default())
}

fn too_deep(max_depth: usize) -> DocumentsError {
    DocumentsError::TooLarge(format!(
        "the DOCX document nests deeper than {max_depth} levels"
    ))
}

/// An element opens.
fn open(
    tag: Tag,
    element: &BytesStart<'_>,
    at: &mut Position,
    sink: &mut Sink,
) -> Result<(), DocumentsError> {
    match tag {
        Tag::Paragraph => {
            at.paragraphs += 1;
            if at.paragraphs > 1 {
                return Ok(());
            }
            at.counted = at.tables == 0 || at.in_cell;
            if !at.counted {
                return Ok(());
            }
            if at.tables == 0 {
                if at.lines {
                    sink.push("\n")?;
                }
                at.lines = true;
            } else {
                if at.cell_paragraphs > 0 {
                    sink.push(" ")?;
                }
                at.cell_paragraphs += 1;
            }
        }
        Tag::Run => at.runs += 1,
        Tag::Text => at.texts += 1,
        Tag::Tab if at.in_run() => sink.push("\t")?,
        Tag::CarriageReturn if at.in_run() => sink.push("\n")?,
        Tag::NoBreakHyphen if at.in_run() => sink.push("-")?,
        Tag::Break if at.in_run() => {
            let page = attribute(element, "type").is_some_and(|kind| kind == "page");
            sink.push(if page { "\u{c}" } else { "\n" })?;
        }
        Tag::Drawing if at.in_run() => sink.push("[image]")?,
        Tag::Table => at.tables += 1,
        Tag::Row if at.tables == 1 => {
            if at.lines {
                sink.push("\n")?;
            }
            at.lines = true;
            at.row = true;
            at.cells = 0;
        }
        Tag::Cell if at.tables == 1 && at.row => {
            if at.cells > 0 {
                sink.push("\t")?;
            }
            at.cells += 1;
            at.in_cell = true;
            at.cell_paragraphs = 0;
        }
        _ => {}
    }
    Ok(())
}

/// An element closes.
fn close(tag: Tag, at: &mut Position) {
    match tag {
        Tag::Paragraph => {
            at.paragraphs = at.paragraphs.saturating_sub(1);
            if at.paragraphs == 0 {
                at.counted = false;
            }
        }
        Tag::Run => at.runs = at.runs.saturating_sub(1),
        Tag::Text => at.texts = at.texts.saturating_sub(1),
        Tag::Table => {
            at.tables = at.tables.saturating_sub(1);
            if at.tables == 0 {
                at.row = false;
                at.in_cell = false;
            }
        }
        Tag::Row if at.tables == 1 => {
            at.row = false;
            at.in_cell = false;
        }
        Tag::Cell if at.tables == 1 => at.in_cell = false,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::CompressionMethod;
    use zip::write::SimpleFileOptions;

    use super::*;

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const MAIN: &str =
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

    fn zip_of(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in parts {
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn content_types(main: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="{main}"/></Types>"#
        )
    }

    const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

    /// A package with `document` as `word/document.xml`.
    fn package(document: &str) -> Vec<u8> {
        zip_of(&[
            ("[Content_Types].xml", content_types(MAIN).as_bytes()),
            ("_rels/.rels", RELS.as_bytes()),
            ("word/document.xml", document.as_bytes()),
        ])
    }

    /// A package whose document has `body` in its body.
    fn docx(body: &str) -> Vec<u8> {
        package(&format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body>{body}</w:body></w:document>"#
        ))
    }

    fn text_of(body: &str) -> String {
        read_text(&docx(body), &DocxLimits::default(), true).unwrap()
    }

    fn run(text: &str) -> String {
        format!("<w:r><w:t>{text}</w:t></w:r>")
    }

    fn para(inner: &str) -> String {
        format!("<w:p>{inner}</w:p>")
    }

    fn cell(inner: &str) -> String {
        format!("<w:tc><w:tcPr/>{inner}</w:tc>")
    }

    fn row(cells: &[String]) -> String {
        format!("<w:tr>{}</w:tr>", cells.concat())
    }

    fn table(rows: &[String]) -> String {
        format!("<w:tbl><w:tblPr/><w:tblGrid/>{}</w:tbl>", rows.concat())
    }

    fn too_large(result: Result<String, DocumentsError>) -> String {
        match result {
            Err(DocumentsError::TooLarge(message)) => message,
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    fn damaged(result: Result<String, DocumentsError>) -> String {
        match result {
            Err(DocumentsError::Docx(message)) => message,
            other => panic!("expected Docx, got {other:?}"),
        }
    }

    #[test]
    fn a_paragraph_is_a_line_and_runs_join() {
        let body = [
            para(&format!("{}{}", run("Prepared for "), run("Acme"))),
            para(""),
            para(&run("Last")),
        ]
        .concat();
        assert_eq!(text_of(&body), "Prepared for Acme\n\nLast");
        assert_eq!(text_of(""), "");
        assert_eq!(text_of("<w:p/>"), "");
        assert_eq!(text_of("<w:p/><w:p/>"), "\n");
    }

    #[test]
    fn tabs_breaks_and_images_have_a_mark() {
        let body = para(
            r#"<w:r><w:t>a</w:t><w:tab/><w:t>b</w:t><w:br/><w:t>c</w:t><w:br w:type="textWrapping"/><w:t>d</w:t><w:br w:type="page"/><w:t>e</w:t><w:cr/><w:t>f</w:t><w:noBreakHyphen/><w:t>g</w:t><w:br w:type="column"/></w:r><w:r><w:drawing><wp:inline><a:graphic><a:graphicData><w:p><w:r><w:t>not text</w:t></w:r></w:p></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#,
        );
        assert_eq!(text_of(&body), "a\tb\nc\nd\u{c}e\nf-g\n[image]");
    }

    #[test]
    fn an_alternative_reads_its_choice_and_not_its_fallback() {
        let body = para(
            r#"<w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing/></mc:Choice><mc:Fallback><w:pict><w:t>old</w:t></w:pict><w:r><w:t>fallback text</w:t></w:r></mc:Fallback></mc:AlternateContent></w:r><w:r><w:t>after</w:t></w:r>"#,
        );
        assert_eq!(text_of(&body), "[image]after");
    }

    #[test]
    fn a_table_is_a_line_for_each_row_with_tabs_between_its_cells() {
        let body = [
            para(&run("Before")),
            table(&[
                row(&[cell(&para(&run("Control"))), cell(&para(&run("Status")))]),
                row(&[
                    cell(&para(&run("Sign-in"))),
                    cell(&[para(&run("Passed")), para(""), para(&run("twice"))].concat()),
                ]),
                row(&[]),
                row(&[cell(&para(""))]),
            ]),
            para(&run("After")),
        ]
        .concat();
        assert_eq!(
            text_of(&body),
            "Before\nControl\tStatus\nSign-in\tPassed  twice\n\n\nAfter"
        );
    }

    #[test]
    fn a_table_inside_a_cell_is_part_of_the_cell() {
        let inner = table(&[
            row(&[cell(&para(&run("in1"))), cell(&para(&run("in2")))]),
            row(&[cell(&para(&run("in3")))]),
        ]);
        let body = table(&[row(&[
            cell(&[para(&run("Nested:")), inner, para(&run("after"))].concat()),
            cell(&para(&run("Last"))),
        ])]);
        assert_eq!(text_of(&body), "Nested: in1 in2 in3 after\tLast");
    }

    #[test]
    fn paragraphs_outside_a_cell_of_a_table_are_not_text() {
        let body = format!(
            "<w:tbl>{}<w:tr>{}{}</w:tr></w:tbl>",
            para(&run("loose")),
            para(&run("loose in a row")),
            cell(&para(&run("kept")))
        );
        assert_eq!(text_of(&body), "kept");
    }

    #[test]
    fn text_in_links_insertions_and_content_controls_is_text() {
        let body = [
            para(&format!(
                r#"{}<w:hyperlink r:id="rId2" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{}</w:hyperlink>{}"#,
                run("See "),
                run("the link"),
                run(".")
            )),
            para(&format!(
                r#"{}<w:ins w:id="0" w:author="A">{}</w:ins><w:smartTag>{}</w:smartTag><w:fldSimple w:instr=" PAGE ">{}</w:fldSimple>"#,
                run("Kept "),
                run("inserted "),
                run("tagged "),
                run("3")
            )),
            format!(
                "<w:sdt><w:sdtPr><w:alias w:val=\"x\"/><w:t>not text</w:t></w:sdtPr><w:sdtContent>{}</w:sdtContent></w:sdt>",
                para(&run("in a control"))
            ),
            para(&format!(
                "<w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt>",
                run("inline control")
            )),
        ]
        .concat();
        assert_eq!(
            text_of(&body),
            "See the link.\nKept inserted tagged 3\nin a control\ninline control"
        );
    }

    #[test]
    fn deleted_text_properties_and_field_codes_are_not_text() {
        let body = para(
            r#"<w:pPr><w:rPr><w:t>no</w:t></w:rPr></w:pPr><w:r><w:rPr><w:b/><w:t>no</w:t></w:rPr><w:t>kept</w:t></w:r><w:del><w:r><w:delText>gone</w:delText><w:t>gone too</w:t></w:r></w:del><w:moveFrom><w:r><w:t>moved</w:t></w:r></w:moveFrom><w:r><w:instrText> PAGE </w:instrText><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:delText>hidden</w:delText></w:r>"#,
        );
        assert_eq!(text_of(&body), "kept");
    }

    #[test]
    fn text_outside_runs_and_other_namespaces_are_not_text() {
        let body =
            para(r#"stray<w:t>no run</w:t><w:r>stray<a:t>drawing text</a:t><w:t>kept</w:t></w:r>"#);
        assert_eq!(text_of(&body), "kept");
    }

    #[test]
    fn references_are_resolved_once() {
        let body = para(&run(
            "a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos; &#65;&#x42; &amp;lt;tag&amp;gt; &amp;amp;",
        ));
        assert_eq!(text_of(&body), "a & b <c> \"d\" 'e' AB &lt;tag&gt; &amp;");
        let body = para("<w:r><w:t><![CDATA[<raw> & text]]></w:t></w:r>");
        assert_eq!(text_of(&body), "<raw> & text");
        let body = para("<w:r><w:t xml:space=\"preserve\">  two  \r\n  lines </w:t></w:r>");
        assert_eq!(text_of(&body), "  two  \n  lines ");
    }

    #[test]
    fn an_undefined_reference_is_a_damaged_file() {
        damaged(read_text(
            &docx(&para(&run("a &custom; b"))),
            &DocxLimits::default(),
            true,
        ));
    }

    #[test]
    fn elements_are_found_by_namespace_and_not_by_prefix() {
        let document = format!(
            r#"<x:document xmlns:x="{W}"><x:body><x:p><x:r><x:t>other prefix</x:t></x:r></x:p></x:body></x:document>"#
        );
        assert_eq!(
            read_text(&package(&document), &DocxLimits::default(), true).unwrap(),
            "other prefix"
        );
        let document = format!(
            r#"<document xmlns="{W}"><body><p><r><t>default namespace</t></r></p></body></document>"#
        );
        assert_eq!(
            read_text(&package(&document), &DocxLimits::default(), true).unwrap(),
            "default namespace"
        );
        let strict = "http://purl.oclc.org/ooxml/wordprocessingml/main";
        let document = format!(
            r#"<w:document xmlns:w="{strict}"><w:body><w:p><w:r><w:t>strict</w:t></w:r></w:p></w:body></w:document>"#
        );
        assert_eq!(
            read_text(&package(&document), &DocxLimits::default(), true).unwrap(),
            "strict"
        );
        // A prefix that is never declared is the usual `w`.
        let document =
            "<w:document><w:body><w:p><w:r><w:t>undeclared</w:t></w:r></w:p></w:body></w:document>";
        assert_eq!(
            read_text(&package(document), &DocxLimits::default(), true).unwrap(),
            "undeclared"
        );
        // Another namespace's `p` and `t` are not paragraphs and text.
        let document = r#"<w:document xmlns:w="urn:other"><w:body><w:p><w:r><w:t>no</w:t></w:r></w:p></w:body></w:document>"#;
        damaged(read_text(&package(document), &DocxLimits::default(), true));
    }

    #[test]
    fn the_main_part_comes_from_the_relationships() {
        let rels = RELS.replace("word/document.xml", "/office/main.xml");
        let types = content_types(MAIN).replace("/word/document.xml", "/office/main.xml");
        let document = format!(
            r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>moved</w:t></w:r></w:p></w:body></w:document>"#
        );
        let bytes = zip_of(&[
            ("[Content_Types].xml", types.as_bytes()),
            ("_rels/.rels", rels.as_bytes()),
            ("office/main.xml", document.as_bytes()),
            (
                "word/document.xml",
                b"<w:document xmlns:w=\"urn:other\"/>".as_slice(),
            ),
        ]);
        assert_eq!(
            read_text(&bytes, &DocxLimits::default(), true).unwrap(),
            "moved"
        );
        // A target that climbs out of the package finds nothing.
        let rels = RELS.replace("word/document.xml", "../word/document.xml");
        let bytes = zip_of(&[
            ("[Content_Types].xml", content_types(MAIN).as_bytes()),
            ("_rels/.rels", rels.as_bytes()),
            ("word/document.xml", b"<w:document/>".as_slice()),
        ]);
        damaged(read_text(&bytes, &DocxLimits::default(), true));
    }

    #[test]
    fn macro_enabled_documents_and_templates_are_word_documents() {
        for main in [
            "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml",
            "application/vnd.ms-word.document.macroEnabled.main+xml",
            "application/vnd.ms-word.template.macroEnabledTemplate.main+xml",
        ] {
            let document = format!(
                r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>ok</w:t></w:r></w:p></w:body></w:document>"#
            );
            let bytes = zip_of(&[
                ("[Content_Types].xml", content_types(main).as_bytes()),
                ("_rels/.rels", RELS.as_bytes()),
                ("word/document.xml", document.as_bytes()),
            ]);
            assert_eq!(
                read_text(&bytes, &DocxLimits::default(), true).unwrap(),
                "ok"
            );
        }
    }

    #[test]
    fn a_package_that_is_not_a_word_document_is_refused() {
        let document = format!(r#"<w:document xmlns:w="{W}"><w:body/></w:document>"#);
        // A spreadsheet's content type for the main part.
        let sheet = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
        let bytes = zip_of(&[
            ("[Content_Types].xml", content_types(sheet).as_bytes()),
            ("_rels/.rels", RELS.as_bytes()),
            ("word/document.xml", document.as_bytes()),
        ]);
        damaged(read_text(&bytes, &DocxLimits::default(), true));
        // No content type for the main part at all.
        let types = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/></Types>"#;
        let bytes = zip_of(&[
            ("[Content_Types].xml", types.as_bytes()),
            ("_rels/.rels", RELS.as_bytes()),
            ("word/document.xml", document.as_bytes()),
        ]);
        damaged(read_text(&bytes, &DocxLimits::default(), true));
        // The root is not a document.
        let bytes = package(&format!(r#"<w:styles xmlns:w="{W}"/>"#));
        damaged(read_text(&bytes, &DocxLimits::default(), true));
    }

    #[test]
    fn missing_parts_are_a_damaged_file() {
        let document = format!(r#"<w:document xmlns:w="{W}"><w:body/></w:document>"#);
        let types = content_types(MAIN);
        for parts in [
            vec![
                ("_rels/.rels", RELS.as_bytes()),
                ("word/document.xml", document.as_bytes()),
            ],
            vec![
                ("[Content_Types].xml", types.as_bytes()),
                ("word/document.xml", document.as_bytes()),
            ],
            vec![
                ("[Content_Types].xml", types.as_bytes()),
                ("_rels/.rels", RELS.as_bytes()),
            ],
            vec![],
        ] {
            damaged(read_text(&zip_of(&parts), &DocxLimits::default(), true));
        }
    }

    #[test]
    fn bytes_that_are_not_a_zip_archive_are_a_damaged_file() {
        for bytes in [
            &b""[..],
            b"not a zip",
            b"PK\x03\x04",
            b"PK\x05\x06\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
            &[0u8; 100],
        ] {
            damaged(read_text(bytes, &DocxLimits::default(), true));
        }
    }

    #[test]
    fn xml_that_is_not_well_formed_is_a_damaged_file() {
        for document in [
            // Cut off.
            format!(r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>cut"#),
            format!(r#"<w:document xmlns:w="{W}"><w:body><w:p></w:body></w:document>"#),
            // Not closed.
            format!(r#"<w:document xmlns:w="{W}"><w:body><w:p>"#),
            // Two roots.
            format!(r#"<w:document xmlns:w="{W}"/><w:document xmlns:w="{W}"/>"#),
            // A DTD.
            format!(
                r#"<!DOCTYPE w:document [<!ENTITY a "b">]><w:document xmlns:w="{W}"><w:body/></w:document>"#
            ),
            String::new(),
            "not xml".to_owned(),
        ] {
            damaged(read_text(&package(&document), &DocxLimits::default(), true));
        }
        // Not UTF-8.
        let mut document =
            format!(r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>"#).into_bytes();
        document.extend_from_slice(&[0xff, 0xfe, 0xfd]);
        document.extend_from_slice(b"</w:t></w:r></w:p></w:body></w:document>");
        let bytes = zip_of(&[
            ("[Content_Types].xml", content_types(MAIN).as_bytes()),
            ("_rels/.rels", RELS.as_bytes()),
            ("word/document.xml", &document),
        ]);
        damaged(read_text(&bytes, &DocxLimits::default(), true));
    }

    #[test]
    fn a_damaged_deflate_stream_is_an_error_and_not_a_panic() {
        let bytes = docx(&para(&run(&"text ".repeat(200))));
        let mut archive = ZipArchive::new(Cursor::new(&bytes[..])).unwrap();
        let file = archive.by_name("word/document.xml").unwrap();
        let start = file.data_start().unwrap() as usize;
        let length = file.compressed_size() as usize;
        let mut broken = bytes.clone();
        for byte in &mut broken[start + 2..start + length.min(40)] {
            *byte ^= 0xff;
        }
        damaged(read_text(&broken, &DocxLimits::default(), true));
    }

    #[test]
    fn every_prefix_and_every_flipped_byte_of_a_small_file_is_handled() {
        let body = [
            para(&format!("{}<w:tab/>{}", run("one"), run("two"))),
            table(&[row(&[cell(&para(&run("c")))])]),
        ]
        .concat();
        let bytes = docx(&body);
        let limits = DocxLimits::default();
        for end in 0..bytes.len() {
            let _ = read_text(&bytes[..end], &limits, true);
        }
        for at in 0..bytes.len() {
            let mut flipped = bytes.clone();
            flipped[at] ^= 0x5a;
            let _ = read_text(&flipped, &limits, true);
        }
    }

    #[test]
    fn nothing_is_kept_when_only_checking() {
        let bytes = docx(&para(&run("text")));
        assert_eq!(
            read_text(&bytes, &DocxLimits::default(), false).unwrap(),
            ""
        );
        damaged(read_text(
            &package("<w:document"),
            &DocxLimits::default(),
            false,
        ));
        let tight = DocxLimits {
            max_text_bytes: 3,
            ..DocxLimits::default()
        };
        too_large(read_text(&bytes, &tight, false));
    }

    #[test]
    fn more_entries_than_the_limit_are_refused() {
        let bytes = docx(&para(&run("x")));
        let roomy = DocxLimits {
            max_entries: 3,
            ..DocxLimits::default()
        };
        assert_eq!(read_text(&bytes, &roomy, true).unwrap(), "x");
        let tight = DocxLimits {
            max_entries: 2,
            ..DocxLimits::default()
        };
        let message = too_large(read_text(&bytes, &tight, true));
        assert!(message.contains("2 entries"), "{message}");
    }

    #[test]
    fn a_part_that_declares_more_than_the_limit_is_refused_before_it_is_read() {
        let big = format!(
            r#"<w:document xmlns:w="{W}"><w:body>{}</w:body></w:document>"#,
            para(&run(&"x".repeat(5000)))
        );
        let bytes = package(&big);
        let tight = DocxLimits {
            max_part_bytes: 4096,
            ..DocxLimits::default()
        };
        let message = too_large(read_text(&bytes, &tight, true));
        assert!(message.contains("4096 bytes"), "{message}");
        let roomy = DocxLimits {
            max_part_bytes: big.len() as u64,
            ..DocxLimits::default()
        };
        assert_eq!(read_text(&bytes, &roomy, true).unwrap().len(), 5000);
        let one_short = DocxLimits {
            max_part_bytes: big.len() as u64 - 1,
            ..DocxLimits::default()
        };
        too_large(read_text(&bytes, &one_short, true));
    }

    #[test]
    fn all_the_parts_together_stay_within_the_total() {
        let document = format!(
            r#"<w:document xmlns:w="{W}"><w:body>{}</w:body></w:document>"#,
            para(&run(&"y".repeat(3000)))
        );
        let bytes = package(&document);
        let parts = (content_types(MAIN).len() + RELS.len() + document.len()) as u64;
        let fits = DocxLimits {
            max_inflated_bytes: parts,
            ..DocxLimits::default()
        };
        assert_eq!(read_text(&bytes, &fits, true).unwrap().len(), 3000);
        let short = DocxLimits {
            max_inflated_bytes: parts - 1,
            ..DocxLimits::default()
        };
        let message = too_large(read_text(&bytes, &short, true));
        assert!(message.contains("in all"), "{message}");
    }

    /// `bytes` with the size the archive declares for the part `name` set to
    /// `declared`, in the part's local header and in the central directory.
    fn forge_declared_size(mut bytes: Vec<u8>, name: &str, declared: u32) -> Vec<u8> {
        let mut at = 0;
        while let Some(found) = bytes[at..].windows(4).position(|w| w == b"PK") {
            let entry = at + found;
            let length = |offset: usize| {
                usize::from(u16::from_le_bytes([
                    bytes[entry + offset],
                    bytes[entry + offset + 1],
                ]))
            };
            let name_length = length(28);
            if &bytes[entry + 46..entry + 46 + name_length] == name.as_bytes() {
                let local = u32::from_le_bytes(bytes[entry + 42..entry + 46].try_into().unwrap());
                let local = local as usize;
                bytes[entry + 24..entry + 28].copy_from_slice(&declared.to_le_bytes());
                bytes[local + 22..local + 26].copy_from_slice(&declared.to_le_bytes());
                return bytes;
            }
            at = entry + 4;
        }
        panic!("no entry named {name}");
    }

    #[test]
    fn a_declared_size_reserves_nothing() {
        let bytes = docx(&para(&run("x")));
        // A part of a few hundred bytes that declares 300 MB: refused for
        // what it declares, quickly, without a buffer of that size.
        let forged = forge_declared_size(bytes.clone(), "word/document.xml", 300_000_000);
        let message = too_large(read_text(&forged, &DocxLimits::default(), true));
        assert!(message.contains("67108864 bytes"), "{message}");
        // Under the limit, the reader goes by the bytes the part really has.
        let forged = forge_declared_size(bytes, "word/document.xml", 1_000_000);
        let _ = read_text(&forged, &DocxLimits::default(), true);
    }

    #[test]
    fn a_part_that_unpacks_to_far_more_than_it_weighs_is_refused() {
        // Eight MiB of one letter deflates to some kilobytes.
        let document = format!(
            r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:body></w:document>"#,
            "a".repeat(8 * 1024 * 1024)
        );
        let bytes = package(&document);
        assert!(bytes.len() < 64 * 1024, "{} bytes", bytes.len());
        let tight = DocxLimits {
            max_part_bytes: 1024 * 1024,
            ..DocxLimits::default()
        };
        too_large(read_text(&bytes, &tight, true));
    }

    #[test]
    fn a_meter_stops_a_stream_that_runs_past_its_limit() {
        let used = Cell::new(0);
        let tripped = Cell::new(false);
        let mut meter = Metered {
            inner: &[7u8; 10][..],
            left: 4,
            used: &used,
            tripped: &tripped,
        };
        let mut out = Vec::new();
        let error = meter.read_to_end(&mut out).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(tripped.get());
        let used = Cell::new(0);
        let tripped = Cell::new(false);
        let mut meter = Metered {
            inner: &[7u8; 4][..],
            left: 4,
            used: &used,
            tripped: &tripped,
        };
        let mut out = Vec::new();
        meter.read_to_end(&mut out).unwrap();
        assert_eq!(out.len(), 4);
        assert_eq!(used.get(), 4);
        assert!(!tripped.get());
    }

    #[test]
    fn text_over_the_limit_is_refused() {
        let body = (0..50)
            .map(|n| para(&run(&format!("line {n:02}"))))
            .collect::<String>();
        let bytes = docx(&body);
        // 50 lines of 7 bytes and 49 breaks.
        let exact = DocxLimits {
            max_text_bytes: 50 * 7 + 49,
            ..DocxLimits::default()
        };
        assert_eq!(read_text(&bytes, &exact, true).unwrap().len(), 399);
        let short = DocxLimits {
            max_text_bytes: 50 * 7 + 48,
            ..DocxLimits::default()
        };
        let message = too_large(read_text(&bytes, &short, true));
        assert!(message.contains("398 bytes"), "{message}");
    }

    #[test]
    fn nesting_over_the_limit_is_refused() {
        fn nested(levels: usize) -> Vec<u8> {
            docx(&format!(
                "{}{}{}",
                "<w:sdt><w:sdtContent>".repeat(levels),
                para(&run("deep")),
                "</w:sdtContent></w:sdt>".repeat(levels)
            ))
        }
        let limits = DocxLimits {
            max_depth: 16,
            ..DocxLimits::default()
        };
        // document and body, a paragraph, a run and a text: 5 levels, and 2
        // for each content control.
        assert_eq!(read_text(&nested(5), &limits, true).unwrap(), "deep");
        let message = too_large(read_text(&nested(6), &limits, true));
        assert!(message.contains("16 levels"), "{message}");
        // The same inside a part that is skipped.
        let skipped = docx(&format!(
            "<w:p><w:pPr>{}{}</w:pPr></w:p>",
            "<w:x>".repeat(20),
            "</w:x>".repeat(20)
        ));
        too_large(read_text(&skipped, &limits, true));
        // Without a limit of its own it stops at 256, however deep the file.
        let deep = docx(&format!(
            "<w:p>{}{}</w:p>",
            "<w:x>".repeat(1000),
            "</w:x>".repeat(1000)
        ));
        too_large(read_text(&deep, &DocxLimits::default(), true));
    }

    #[test]
    fn many_attributes_on_one_element_take_linear_time() {
        let attributes = (0..20_000)
            .map(|n| format!(r#"w:a{n}="v""#))
            .collect::<Vec<_>>()
            .join(" ");
        let body = para(&format!(
            r#"<w:r><w:br {attributes} w:type="page"/><w:t>x</w:t></w:r>"#
        ));
        let started = std::time::Instant::now();
        assert_eq!(text_of(&body), "\u{c}x");
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn the_same_part_twice_is_refused() {
        let document = format!(r#"<w:document xmlns:w="{W}"><w:body/></w:document>"#);
        // The archive writer refuses two entries of one name: the second is
        // written under a name one letter off, and renamed in both headers.
        let bytes = zip_of(&[
            ("[Content_Types].xml", content_types(MAIN).as_bytes()),
            ("_rels/.rels", RELS.as_bytes()),
            ("word/document.xml", document.as_bytes()),
            ("word/document.xmX", document.as_bytes()),
        ]);
        let from = b"word/document.xmX";
        let mut forged = bytes.clone();
        while let Some(at) = forged.windows(from.len()).position(|w| w == from) {
            forged[at..at + from.len()].copy_from_slice(b"word/document.xml");
        }
        damaged(read_text(&forged, &DocxLimits::default(), true));
        // The same name with other letters in capitals is the same part.
        let from = b"word/Document.xml";
        let bytes = zip_of(&[
            ("[Content_Types].xml", content_types(MAIN).as_bytes()),
            ("_rels/.rels", RELS.as_bytes()),
            ("word/document.xml", document.as_bytes()),
            ("word/Document.xml", document.as_bytes()),
        ]);
        assert!(bytes.windows(from.len()).any(|w| w == from));
        damaged(read_text(&bytes, &DocxLimits::default(), true));
    }

    #[test]
    fn part_names_are_matched_without_regard_to_case_or_slash_style() {
        let document = format!(
            r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>found</w:t></w:r></w:p></w:body></w:document>"#
        );
        let bytes = zip_of(&[
            ("[content_types].xml", content_types(MAIN).as_bytes()),
            ("_rels\\.rels", RELS.as_bytes()),
            ("/Word/Document.xml", document.as_bytes()),
        ]);
        assert_eq!(
            read_text(&bytes, &DocxLimits::default(), true).unwrap(),
            "found"
        );
    }

    #[test]
    fn the_limits_have_the_documented_defaults() {
        let limits = DocxLimits::default();
        assert_eq!(limits.max_entries, 10_000);
        assert_eq!(limits.max_part_bytes, 64 * 1024 * 1024);
        assert_eq!(limits.max_inflated_bytes, 128 * 1024 * 1024);
        assert_eq!(limits.max_text_bytes, 64 * 1024 * 1024);
        assert_eq!(limits.max_depth, 256);
        let read: DocxLimits = serde_json::from_str(r#"{"max_depth": 8}"#).unwrap();
        assert_eq!(
            read,
            DocxLimits {
                max_depth: 8,
                ..DocxLimits::default()
            }
        );
    }
}
