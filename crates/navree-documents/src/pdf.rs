//! [`Pdf`]: a rendered PDF, with page count, text extraction, merge,
//! stamps and overlays (on [lopdf](https://docs.rs/lopdf)).

use std::collections::{HashMap, HashSet};
use std::fmt;

use bytes::Bytes;
use http::header::{CONTENT_DISPOSITION, HeaderValue};
use lopdf::content::{Content, Operation};
use lopdf::{
    Dictionary, Document, LoadOptions, Object, ObjectId, Stream, StringFormat, dictionary,
};
use navree_http::{IntoResponse, Response};

use crate::DocumentsError;
use crate::download::{Download, PDF_CONTENT_TYPE};
use crate::stamp::{Pages, Position, Stamp};
use crate::text::decode_text_string;

/// Decompressed content allowed per page while reading a PDF (64 MiB), so
/// a small file cannot inflate without bound.
const MAX_PAGE_CONTENT: usize = 64 * 1024 * 1024;

/// What one object stream or cross-reference stream may inflate to while a
/// PDF loads (8 MiB). The objects inside are parsed as the stream is read,
/// before any page is looked at. A stream over the limit is skipped, with
/// the objects it holds, as a damaged one is.
///
/// Writers commonly put 100 to 200 objects in an object stream, tens of
/// kilobytes; 8 MiB holds tens of thousands of objects, and a
/// cross-reference stream of 8 MiB lists more than a million.
///
/// It bounds the bytes one stream inflates to, not the memory loading
/// takes: lopdf parses every object stream as the file loads, and the
/// objects a stream lists may all point at the same bytes, each parsed
/// again. A PDF of a kilobyte can load as tens of megabytes of objects.
/// Callers cap the size of the PDFs they accept.
const MAX_LOADED_STREAM: usize = 8 * 1024 * 1024;

/// A PDF document: the bytes an engine rendered, or any PDF loaded with
/// [`Pdf::from_bytes`].
///
/// It derefs to `[u8]`, converts into [`Bytes`], and is a response
/// (`application/pdf`, shown inline); [`download`](Pdf::download) and
/// [`inline`](Pdf::inline) name the file.
///
/// Editing returns a new `Pdf`: [`merge`](Pdf::merge) joins documents,
/// [`stamp`](Pdf::stamp) writes a text stamp (a watermark, "PAID", a copy
/// number) and [`overlay`](Pdf::overlay) draws the first page of another
/// PDF over pages of this one (a seal or a signature, in any script, when
/// the overlay is rendered with the native engine).
///
/// ```
/// use navree_documents::{Pdf, Stamp};
///
/// # fn main() -> Result<(), navree_documents::DocumentsError> {
/// let first = Pdf::from_lines(["Invoice 42", "Total: 120.00 EUR"]);
/// let terms = Pdf::from_lines(["Terms and conditions"]);
/// let both = Pdf::merge([&first, &terms])?;
/// assert_eq!(both.page_count()?, 2);
///
/// let paid = both.stamp(&Stamp::text("PAID"))?;
/// assert!(paid.text()?.contains("PAID"));
/// assert!(paid.text()?.contains("Terms and conditions"));
/// # Ok(())
/// # }
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct Pdf(Bytes);

impl fmt::Debug for Pdf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Pdf({} bytes)", self.0.len())
    }
}

impl Pdf {
    /// A PDF from its bytes, checked: an error unless the bytes parse as a
    /// PDF with at least one page.
    pub fn from_bytes(bytes: impl Into<Bytes>) -> Result<Pdf, DocumentsError> {
        let pdf = Pdf(bytes.into());
        if pdf.page_count()? == 0 {
            return Err(DocumentsError::Pdf("the document has no pages".into()));
        }
        Ok(pdf)
    }

    /// Bytes an engine produced, not checked.
    #[cfg(any(feature = "native", feature = "chrome"))]
    pub(crate) fn from_engine(bytes: impl Into<Bytes>) -> Pdf {
        Pdf(bytes.into())
    }

    /// The bytes.
    pub fn bytes(&self) -> &Bytes {
        &self.0
    }

    /// The bytes, by value.
    pub fn into_bytes(self) -> Bytes {
        self.0
    }

    /// The number of pages.
    pub fn page_count(&self) -> Result<usize, DocumentsError> {
        Ok(self.load()?.get_pages().len())
    }

    /// The text of every page, pages separated by a form feed (`\u{c}`):
    /// what the content streams show, in the order they show it, decoded
    /// through each font's encoding. For tests and checks, such as
    /// `assert!(pdf.text()?.contains("Ada Lovelace"))`; layout (columns,
    /// tables) is not reconstructed. Right-to-left text comes back in
    /// reading order.
    pub fn text(&self) -> Result<String, DocumentsError> {
        Ok(crate::text::page_texts(&self.load()?)?.join("\u{c}"))
    }

    /// The text of each page (see [`text`](Pdf::text)).
    pub fn page_texts(&self) -> Result<Vec<String>, DocumentsError> {
        crate::text::page_texts(&self.load()?)
    }

    /// One document with the pages of every PDF in `pdfs`, in order.
    /// Outlines, forms and tagged structure are dropped; each page keeps
    /// its contents, size and resources.
    pub fn merge<'a>(pdfs: impl IntoIterator<Item = &'a Pdf>) -> Result<Pdf, DocumentsError> {
        let mut merged = Document::with_version("1.7");
        let pages_id = merged.new_object_id();
        let mut kids = Vec::new();
        for pdf in pdfs {
            let mut document = pdf.load()?;
            flatten_inherited(&mut document)?;
            document.renumber_objects_with(merged.max_id + 1);
            let page_ids: Vec<ObjectId> = document.get_pages().into_values().collect();
            let page_set: std::collections::BTreeSet<ObjectId> = page_ids.iter().copied().collect();
            for (id, object) in document.objects {
                let keep = match object.type_name() {
                    Ok(b"Catalog" | b"Pages") => false,
                    Ok(b"Page") => page_set.contains(&id),
                    _ => true,
                };
                if keep {
                    merged.objects.insert(id, object);
                }
            }
            merged.max_id = merged.max_id.max(document.max_id);
            for id in page_ids {
                let page = merged
                    .get_object_mut(id)
                    .and_then(Object::as_dict_mut)
                    .map_err(pdf_error)?;
                page.set("Parent", pages_id);
                page.remove(b"StructParents");
                kids.push(Object::Reference(id));
            }
        }
        if kids.is_empty() {
            return Err(DocumentsError::Edit("nothing to merge".into()));
        }
        let count = kids.len() as i64;
        merged.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => count,
            }),
        );
        let catalog_id = merged.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        merged.trailer.set("Root", catalog_id);
        merged.prune_objects();
        save(merged)
    }

    /// This document followed by the pages of `other`.
    pub fn append(&self, other: &Pdf) -> Result<Pdf, DocumentsError> {
        Pdf::merge([self, other])
    }

    /// The document with `stamp` written on the pages it selects.
    pub fn stamp(&self, stamp: &Stamp) -> Result<Pdf, DocumentsError> {
        let encoded = stamp.encoded_text()?;
        let mut document = self.load()?;
        flatten_inherited(&mut document)?;
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica-Bold",
            "Encoding" => "WinAnsiEncoding",
        });
        let opacity = stamp.opacity.clamp(0.0, 1.0);
        let gs_id = document.add_object(dictionary! {
            "Type" => "ExtGState",
            "ca" => opacity,
            "CA" => opacity,
        });
        let mut additions = ResourceAdditions::new(vec![
            (b"Font", b"NavreeStamp", font_id.into()),
            (b"ExtGState", b"NavreeStampGs", gs_id.into()),
        ]);
        for page_id in selected_pages(&document, &stamp.pages) {
            let (width, height) = page_size(&document, page_id)?;
            additions.add_to_page(&mut document, page_id)?;
            let matrix = stamp.matrix(width, height);
            let (r, g, b) = stamp.color;
            let operations = vec![
                Operation::new("q", vec![]),
                Operation::new("gs", vec![Object::Name(b"NavreeStampGs".to_vec())]),
                Operation::new("rg", vec![r.into(), g.into(), b.into()]),
                Operation::new("BT", vec![]),
                Operation::new(
                    "Tf",
                    vec![Object::Name(b"NavreeStamp".to_vec()), stamp.size.into()],
                ),
                Operation::new("Tm", matrix.iter().map(|&v| Object::Real(v)).collect()),
                Operation::new(
                    "Tj",
                    vec![Object::String(encoded.clone(), StringFormat::Literal)],
                ),
                Operation::new("ET", vec![]),
                Operation::new("Q", vec![]),
            ];
            draw_over(&mut document, page_id, operations)?;
        }
        save(document)
    }

    /// The document with the first page of `overlay` drawn over the pages
    /// `pages` selects. An overlay of another size is scaled to fit and
    /// centered.
    pub fn overlay(&self, overlay: &Pdf, pages: Pages) -> Result<Pdf, DocumentsError> {
        let mut document = self.load()?;
        flatten_inherited(&mut document)?;
        let mut source = overlay.load()?;
        flatten_inherited(&mut source)?;
        source.renumber_objects_with(document.max_id + 1);
        let source_page = *source
            .get_pages()
            .get(&1)
            .ok_or_else(|| DocumentsError::Edit("the overlay has no pages".into()))?;
        let (overlay_width, overlay_height) = page_size(&source, source_page)?;
        let content = source
            .get_page_content_with_limit(source_page, MAX_PAGE_CONTENT)
            .map_err(pdf_error)?;
        let page = source.get_dictionary(source_page).map_err(pdf_error)?;
        let resources = page
            .get(b"Resources")
            .cloned()
            .unwrap_or_else(|_| Object::Dictionary(Dictionary::new()));
        let media_box = page.get(b"MediaBox").cloned().map_err(pdf_error)?;
        let (left, bottom) = box_origin(&source, &media_box);
        // Everything the overlay page refers to moves into the document;
        // what nothing references is pruned when saving.
        let source_max = source.max_id;
        for (id, object) in source.objects {
            if !matches!(object.type_name(), Ok(b"Catalog" | b"Pages" | b"Page")) {
                document.objects.insert(id, object);
            }
        }
        document.max_id = document.max_id.max(source_max);
        let form = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "BBox" => media_box,
                "Resources" => resources,
            },
            content,
        );
        let form_id = document.add_object(form);
        let mut additions =
            ResourceAdditions::new(vec![(b"XObject", b"NavreeOverlay", form_id.into())]);
        for page_id in selected_pages(&document, &pages) {
            let (width, height) = page_size(&document, page_id)?;
            additions.add_to_page(&mut document, page_id)?;
            let scale = (width / overlay_width).min(height / overlay_height);
            let dx = (width - overlay_width * scale) / 2.0 - left * scale;
            let dy = (height - overlay_height * scale) / 2.0 - bottom * scale;
            let operations = vec![
                Operation::new("q", vec![]),
                Operation::new(
                    "cm",
                    vec![
                        scale.into(),
                        0.into(),
                        0.into(),
                        scale.into(),
                        dx.into(),
                        dy.into(),
                    ],
                ),
                Operation::new("Do", vec![Object::Name(b"NavreeOverlay".to_vec())]),
                Operation::new("Q", vec![]),
            ];
            draw_over(&mut document, page_id, operations)?;
        }
        save(document)
    }

    /// The document with `title` (and, when given, `author`) in its
    /// metadata.
    pub fn with_metadata(&self, title: &str, author: Option<&str>) -> Result<Pdf, DocumentsError> {
        let mut document = self.load()?;
        let mut info = match document.trailer.get(b"Info").and_then(Object::as_reference) {
            Ok(id) => document.get_dictionary(id).cloned().unwrap_or_default(),
            Err(_) => Dictionary::new(),
        };
        info.set(
            "Title",
            Object::String(utf16_text(title), StringFormat::Hexadecimal),
        );
        if let Some(author) = author {
            info.set(
                "Author",
                Object::String(utf16_text(author), StringFormat::Hexadecimal),
            );
        }
        let info_id = document.add_object(info);
        document.trailer.set("Info", info_id);
        save(document)
    }

    /// The title in the document's metadata, if any.
    pub fn title(&self) -> Result<Option<String>, DocumentsError> {
        let document = self.load()?;
        let Ok(info) = document
            .trailer
            .get(b"Info")
            .and_then(|info| document.dereference(info))
            .and_then(|(_, info)| info.as_dict())
        else {
            return Ok(None);
        };
        Ok(info
            .get(b"Title")
            .and_then(Object::as_str)
            .ok()
            .map(decode_text_string))
    }

    /// A plain document: `lines` of 11-point Helvetica on A4 pages, 60
    /// lines a page. Characters outside Latin-1 print as `?`. For
    /// placeholders and tests (the [`DocumentsFake`](crate::DocumentsFake)
    /// renders with it).
    pub fn from_lines<I, S>(lines: I) -> Pdf
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        const LINES_PER_PAGE: usize = 60;
        let lines: Vec<Vec<u8>> = lines
            .into_iter()
            .flat_map(|line| {
                line.as_ref()
                    .split('\n')
                    .map(latin1_lossy)
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let chunks: Vec<&[Vec<u8>]> = if lines.is_empty() {
            vec![&[]]
        } else {
            lines.chunks(LINES_PER_PAGE).collect()
        };
        let mut kids = Vec::new();
        for chunk in chunks {
            let mut operations = vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 11.into()]),
                Operation::new("TL", vec![13.into()]),
                Operation::new("Td", vec![50.into(), 800.into()]),
            ];
            for (index, line) in chunk.iter().enumerate() {
                if index > 0 {
                    operations.push(Operation::new("T*", vec![]));
                }
                operations.push(Operation::new(
                    "Tj",
                    vec![Object::String(line.clone(), StringFormat::Literal)],
                ));
            }
            operations.push(Operation::new("ET", vec![]));
            let content = Content { operations }.encode().unwrap_or_default();
            let content_id = document.add_object(Stream::new(Dictionary::new(), content));
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Contents" => content_id,
                "Resources" => resources_id,
            });
            kids.push(Object::Reference(page_id));
        }
        let count = kids.len() as i64;
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => count,
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        save(document).expect("a generated document saves")
    }

    fn load(&self) -> Result<Document, DocumentsError> {
        let options = LoadOptions {
            max_decompressed_size: Some(MAX_LOADED_STREAM),
            ..LoadOptions::default()
        };
        Document::load_mem_with_options(&self.0, options).map_err(pdf_error)
    }
}

impl Pdf {
    /// The PDF as a response the browser saves as `filename`
    /// (`Content-Disposition: attachment`).
    pub fn download(self, filename: impl Into<String>) -> Download {
        Download::new(self.0, PDF_CONTENT_TYPE, filename)
    }

    /// The PDF as a response the browser shows, saved as `filename` when
    /// the reader saves it (`Content-Disposition: inline`).
    pub fn inline(self, filename: impl Into<String>) -> Download {
        Download::new(self.0, PDF_CONTENT_TYPE, filename).inline()
    }
}

/// `application/pdf`, shown inline, `Cache-Control: private, no-store`.
impl IntoResponse for Pdf {
    fn into_response(self) -> Response {
        let mut response = self.inline("document.pdf").into_response();
        response
            .headers_mut()
            .insert(CONTENT_DISPOSITION, HeaderValue::from_static("inline"));
        response
    }
}

impl std::ops::Deref for Pdf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for Pdf {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<Pdf> for Bytes {
    fn from(pdf: Pdf) -> Bytes {
        pdf.0
    }
}

impl From<Pdf> for Vec<u8> {
    fn from(pdf: Pdf) -> Vec<u8> {
        pdf.0.to_vec()
    }
}

/// The input cannot be read: the client's error.
fn pdf_error(error: lopdf::Error) -> DocumentsError {
    DocumentsError::Pdf(error.to_string())
}

/// Writing what this crate made failed: the server's error.
fn write_error(what: &str, error: impl fmt::Display) -> DocumentsError {
    DocumentsError::Engine {
        engine: "pdf",
        message: format!("{what}: {error}"),
    }
}

fn save(mut document: Document) -> Result<Pdf, DocumentsError> {
    document.compress();
    let mut out = Vec::new();
    document
        .save_to(&mut out)
        .map_err(|error| write_error("the document could not be written", error))?;
    Ok(Pdf(Bytes::from(out)))
}

/// Copies the attributes a page inherits from its page-tree ancestors
/// (`Resources`, `MediaBox`, `CropBox`, `Rotate`) onto the page, so the
/// page can move to another tree.
///
/// A dictionary or a long array is not copied onto each page: it becomes
/// one object that every page inheriting it refers to, so a tree of many
/// pages and large resources does not grow by their product.
fn flatten_inherited(document: &mut Document) -> Result<(), DocumentsError> {
    const INHERITED: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];
    // The value each page gets, by the ancestor and key it comes from.
    let mut shared: HashMap<(ObjectId, &[u8]), Object> = HashMap::new();
    let page_ids: Vec<ObjectId> = document.get_pages().into_values().collect();
    for page_id in page_ids {
        // The key, and the ancestor that holds it.
        let mut found: Vec<(&[u8], ObjectId)> = Vec::new();
        {
            let page = document.get_dictionary(page_id).map_err(pdf_error)?;
            for key in INHERITED {
                if page.has(key) {
                    continue;
                }
                let mut parent = page.get(b"Parent").and_then(Object::as_reference).ok();
                let mut depth = 0;
                while let Some(id) = parent {
                    depth += 1;
                    if depth > 64 {
                        break;
                    }
                    let Ok(node) = document.get_dictionary(id) else {
                        break;
                    };
                    if node.has(key) {
                        found.push((key, id));
                        break;
                    }
                    parent = node.get(b"Parent").and_then(Object::as_reference).ok();
                }
            }
        }
        let mut values = Vec::with_capacity(found.len());
        for (key, holder) in found {
            let value = match shared.get(&(holder, key)) {
                Some(value) => value.clone(),
                None => {
                    let value = document
                        .get_dictionary(holder)
                        .and_then(|node| node.get(key))
                        .cloned()
                        .map_err(pdf_error)?;
                    let value = match value {
                        Object::Dictionary(_) => Object::Reference(document.add_object(value)),
                        Object::Array(ref items) if items.len() > 4 => {
                            Object::Reference(document.add_object(value))
                        }
                        value => value,
                    };
                    shared.insert((holder, key), value.clone());
                    value
                }
            };
            values.push((key, value));
        }
        let page = document
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .map_err(pdf_error)?;
        for (key, value) in values {
            page.set(key, value);
        }
        if !page.has(b"MediaBox") {
            page.set("MediaBox", vec![0.into(), 0.into(), 612.into(), 792.into()]);
        }
    }
    Ok(())
}

/// The lower-left corner of a box (`[llx lly urx ury]`).
fn box_origin(document: &Document, media_box: &Object) -> (f32, f32) {
    let values = box_values(document, media_box);
    (values[0], values[1])
}

fn box_values(document: &Document, media_box: &Object) -> [f32; 4] {
    let array = document
        .dereference(media_box)
        .and_then(|(_, object)| object.as_array().cloned())
        .unwrap_or_default();
    let mut values = [0.0, 0.0, 612.0, 792.0];
    for (slot, value) in values.iter_mut().zip(array.iter()) {
        if let Ok(number) = document
            .dereference(value)
            .and_then(|(_, object)| object.as_float())
        {
            *slot = number;
        }
    }
    values
}

/// Width and height of a page (its `MediaBox`, after `flatten_inherited`).
fn page_size(document: &Document, page_id: ObjectId) -> Result<(f32, f32), DocumentsError> {
    let page = document.get_dictionary(page_id).map_err(pdf_error)?;
    let media_box = page.get(b"MediaBox").map_err(pdf_error)?;
    let [llx, lly, urx, ury] = box_values(document, media_box);
    Ok(((urx - llx).abs(), (ury - lly).abs()))
}

/// Entries added to the resources of the pages an edit draws on: a font,
/// a graphics state, a form, the same on every page.
///
/// What pages share stays shared: pages whose `Resources` (or whose `Font`
/// dictionary, say) is one object get one new object, a copy with the
/// entries added, so the document grows by the size of the resources once,
/// not once per page. The objects the other pages use are left as they are.
struct ResourceAdditions {
    /// The entries: the kind of resource (`Font`), its name, its value.
    entries: Vec<(&'static [u8], &'static [u8], Object)>,
    /// The copy, with the entries, of each `Resources` object.
    resources: HashMap<ObjectId, ObjectId>,
    /// The copy, with the entries, of each object of one kind (a `Font`
    /// dictionary).
    kinds: HashMap<(&'static [u8], ObjectId), ObjectId>,
}

impl ResourceAdditions {
    fn new(entries: Vec<(&'static [u8], &'static [u8], Object)>) -> Self {
        Self {
            entries,
            resources: HashMap::new(),
            kinds: HashMap::new(),
        }
    }

    /// Adds the entries to the resources of `page_id`.
    fn add_to_page(
        &mut self,
        document: &mut Document,
        page_id: ObjectId,
    ) -> Result<(), DocumentsError> {
        let current = document
            .get_dictionary(page_id)
            .map_err(pdf_error)?
            .get(b"Resources")
            .cloned()
            .unwrap_or_else(|_| Object::Dictionary(Dictionary::new()));
        let resources = match current {
            Object::Reference(id) => match self.resources.get(&id) {
                Some(copy) => Object::Reference(*copy),
                None => {
                    let mut resources = document.get_dictionary(id).cloned().map_err(pdf_error)?;
                    self.add_to(document, &mut resources)?;
                    let copy = document.add_object(resources);
                    self.resources.insert(id, copy);
                    Object::Reference(copy)
                }
            },
            Object::Dictionary(mut resources) => {
                self.add_to(document, &mut resources)?;
                Object::Dictionary(resources)
            }
            _ => {
                let mut resources = Dictionary::new();
                self.add_to(document, &mut resources)?;
                Object::Dictionary(resources)
            }
        };
        document
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .map_err(pdf_error)?
            .set("Resources", resources);
        Ok(())
    }

    /// Adds the entries to `resources`, a dictionary of its own.
    fn add_to(
        &mut self,
        document: &mut Document,
        resources: &mut Dictionary,
    ) -> Result<(), DocumentsError> {
        for index in 0..self.entries.len() {
            let kind = self.entries[index].0;
            if index > 0 && self.entries[..index].iter().any(|(k, ..)| *k == kind) {
                continue;
            }
            let entries = self.entries.iter().filter(|(k, ..)| *k == kind);
            match resources.get(kind) {
                Ok(Object::Reference(id)) => {
                    let id = *id;
                    let copy = match self.kinds.get(&(kind, id)) {
                        Some(copy) => *copy,
                        None => {
                            // One that is not a dictionary is replaced, as
                            // a missing one is.
                            let mut names =
                                document.get_dictionary(id).cloned().unwrap_or_default();
                            for (_, name, value) in entries {
                                names.set(*name, value.clone());
                            }
                            let copy = document.add_object(names);
                            self.kinds.insert((kind, id), copy);
                            copy
                        }
                    };
                    resources.set(kind, copy);
                }
                _ => {
                    let names = sub_dict_mut(resources, kind)?;
                    for (_, name, value) in entries {
                        names.set(*name, value.clone());
                    }
                }
            }
        }
        Ok(())
    }
}

/// The page objects of `document` that `pages` selects, each once: a page
/// object listed twice in the page tree is drawn on once.
fn selected_pages(document: &Document, pages: &Pages) -> Vec<ObjectId> {
    let all = document.get_pages();
    let total = all.len();
    let mut seen = HashSet::new();
    all.into_iter()
        .filter(|&(number, page_id)| pages.includes(number as usize, total) && seen.insert(page_id))
        .map(|(_, page_id)| page_id)
        .collect()
}

/// The sub-dictionary `key` of `resources`, created when missing.
fn sub_dict_mut<'a>(
    resources: &'a mut Dictionary,
    key: &[u8],
) -> Result<&'a mut Dictionary, DocumentsError> {
    if !matches!(resources.get(key), Ok(Object::Dictionary(_))) {
        resources.set(key.to_vec(), Object::Dictionary(Dictionary::new()));
    }
    resources
        .get_mut(key)
        .and_then(Object::as_dict_mut)
        .map_err(pdf_error)
}

/// Appends `operations` to a page after wrapping its existing contents in
/// `q`/`Q`, so whatever state the page leaves does not move the drawing.
fn draw_over(
    document: &mut Document,
    page_id: ObjectId,
    operations: Vec<Operation>,
) -> Result<(), DocumentsError> {
    let existing: Vec<Object> = match document
        .get_dictionary(page_id)
        .map_err(pdf_error)?
        .get(b"Contents")
    {
        Ok(Object::Reference(id)) => vec![Object::Reference(*id)],
        Ok(Object::Array(items)) => items.clone(),
        _ => Vec::new(),
    };
    let open = document.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
    let mut close = b"\nQ\n".to_vec();
    close.extend(
        Content { operations }
            .encode()
            .map_err(|error| write_error("the drawing could not be encoded", error))?,
    );
    let close = document.add_object(Stream::new(Dictionary::new(), close));
    let mut contents = vec![Object::Reference(open)];
    contents.extend(existing);
    contents.push(Object::Reference(close));
    document
        .get_object_mut(page_id)
        .and_then(Object::as_dict_mut)
        .map_err(pdf_error)?
        .set("Contents", contents);
    Ok(())
}

/// `text` in WinAnsi (Latin-1 for the printable range), other characters
/// as `?`.
fn latin1_lossy(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c as u32 {
            0x20..=0x7e | 0xa0..=0xff => c as u32 as u8,
            0x09 => b' ',
            _ => b'?',
        })
        .collect()
}

/// A PDF text string in UTF-16BE with its byte-order mark.
fn utf16_text(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xfe, 0xff];
    for unit in text.encode_utf16() {
        bytes.extend(unit.to_be_bytes());
    }
    bytes
}

impl Stamp {
    /// The text matrix placing the stamp on a page of this size.
    fn matrix(&self, width: f32, height: f32) -> [f32; 6] {
        let text_width = self.text_width();
        let cap_height = self.size * 0.72;
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let margin = self.margin;
        // The point the text is anchored at, and where on the text it is.
        let (anchor, along, across) = match self.position {
            Position::Center => ((width / 2.0, height / 2.0), 0.5, 0.5),
            Position::Top => ((width / 2.0, height - margin), 0.5, 1.0),
            Position::Bottom => ((width / 2.0, margin), 0.5, 0.0),
            Position::TopLeft => ((margin, height - margin), 0.0, 1.0),
            Position::TopRight => ((width - margin, height - margin), 1.0, 1.0),
            Position::BottomLeft => ((margin, margin), 0.0, 0.0),
            Position::BottomRight => ((width - margin, margin), 1.0, 0.0),
        };
        // Start of the baseline: the anchor minus the offsets along the
        // text direction (cos, sin) and across it (-sin, cos).
        let dx = along * text_width;
        let dy = across * cap_height;
        let x = anchor.0 - dx * cos + dy * sin;
        let y = anchor.1 - dx * sin - dy * cos;
        [cos, sin, -sin, cos, x, y]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_documents_have_text_and_pages() {
        let pdf = Pdf::from_lines(["Hello", "Wörld €"]);
        assert!(pdf.starts_with(b"%PDF-"));
        assert_eq!(pdf.page_count().unwrap(), 1);
        let text = pdf.text().unwrap();
        assert!(text.contains("Hello"), "{text:?}");
        assert!(text.contains("Wörld ?"), "{text:?}");
        let long = Pdf::from_lines((0..130).map(|i| format!("line {i}")));
        assert_eq!(long.page_count().unwrap(), 3);
        assert_eq!(
            Pdf::from_lines(Vec::<String>::new()).page_count().unwrap(),
            1
        );
    }

    #[test]
    fn garbage_is_not_a_pdf() {
        assert!(matches!(
            Pdf::from_bytes(&b"not a pdf"[..]),
            Err(DocumentsError::Pdf(_))
        ));
        let pdf = Pdf::from_lines(["x"]);
        assert_eq!(Pdf::from_bytes(pdf.bytes().clone()).unwrap(), pdf);
    }

    #[test]
    fn metadata_titles_round_trip_in_any_script() {
        let pdf = Pdf::from_lines(["x"]);
        assert_eq!(pdf.title().unwrap(), None);
        let titled = pdf.with_metadata("شهادة إتمام", Some("Navree")).unwrap();
        assert_eq!(titled.title().unwrap().as_deref(), Some("شهادة إتمام"));
    }

    #[test]
    fn writing_what_the_crate_made_failing_is_the_server_s_error() {
        let fault = write_error("the drawing could not be encoded", "broken");
        assert!(
            matches!(fault, DocumentsError::Engine { engine: "pdf", .. }),
            "{fault:?}"
        );
        assert_eq!(fault.status(), http::StatusCode::INTERNAL_SERVER_ERROR);
        // Input that cannot be read is the client's.
        let unreadable = pdf_error(lopdf::Error::ObjectNotFound((1, 0)));
        assert_eq!(unreadable.status(), http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn utf16_and_doc_encoded_strings_decode() {
        assert_eq!(decode_text_string(&utf16_text("Ünïcode ✓")), "Ünïcode ✓");
        assert_eq!(decode_text_string(b"Plain"), "Plain");
    }
}
