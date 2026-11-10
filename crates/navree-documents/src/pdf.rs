//! [`Pdf`]: a rendered PDF, with page count, text extraction, merge,
//! stamps and overlays (on navree-pdf).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

use bytes::Bytes;
use http::header::{CONTENT_DISPOSITION, HeaderValue};
use navree_http::{IntoResponse, Response};
use navree_pdf::content::{self, Operation};
use navree_pdf::text::decode_text_string;
use navree_pdf::{
    ContentError, Dictionary, Document, Object, ObjectId, Objects, PdfError, ReadLimits, Stream,
    StringFormat, dictionary, memory_of,
};

use crate::DocumentsError;
use crate::download::{Download, PDF_CONTENT_TYPE};
use crate::stamp::{Pages, Position, Stamp};

/// What an edit counts for each page it changes, besides the objects it adds:
/// the entries it sets on the page, the room the page's dictionary and the
/// edit's own lists keep for them.
const PAGE_EDIT_MEMORY: usize = 512;
/// What an edit counts for the resources of a page that are its own, when it
/// adds its entries to them: a dictionary for each kind, and the entries.
const RESOURCES_EDIT_MEMORY: usize = 1024;

/// Decompressed content allowed per page while reading a PDF (64 MiB), so
/// a small file cannot inflate without bound.
const MAX_PAGE_CONTENT: usize = 64 * 1024 * 1024;

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
    ///
    /// Reading a PDF is held to a budget (see the documents guide): the
    /// objects it reads may be counted as holding 256 MiB, and its reader may
    /// look at 1 GiB, and a file that needs more is refused with
    /// [`DocumentsError::TooLarge`], however small it is.
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
        let document = self.load()?;
        let count = document.pages().len();
        document.check().map_err(pdf_error)?;
        Ok(count)
    }

    /// The text of every page, pages separated by a form feed (`\u{c}`):
    /// what the content streams show, in the order they show it, decoded
    /// through each font's encoding. For tests and checks, such as
    /// `assert!(pdf.text()?.contains("Anu Lakhotia"))`; layout (columns,
    /// tables) is not reconstructed. Right-to-left text comes back in
    /// reading order.
    pub fn text(&self) -> Result<String, DocumentsError> {
        Ok(self.page_texts()?.join("\u{c}"))
    }

    /// The text of each page (see [`text`](Pdf::text)).
    pub fn page_texts(&self) -> Result<Vec<String>, DocumentsError> {
        navree_pdf::text::page_texts(&self.load()?).map_err(pdf_error)
    }

    /// One document with the pages of every PDF in `pdfs`, in order.
    /// Outlines, forms and tagged structure are dropped; each page keeps
    /// its contents, size and resources.
    ///
    /// Like every edit, this refuses a document it would lose part of (an
    /// object its page tree, contents or resources lead to cannot be read:
    /// [`DocumentsError::Pdf`]), and one it would write more than the sizes of
    /// the files plus 256 MiB for, or take more than 256 MiB of memory and 1
    /// GiB of work for ([`DocumentsError::TooLarge`]). The limits are those of
    /// one document, for all the inputs together: the memory an input is
    /// counted as is taken from the limit only for the pages it keeps and what
    /// they lead to (not its outlines, forms or tagged structure, which are
    /// let go of before the next input is read), and never for the bytes of a
    /// stream, which are the inputs' own: so scanned documents merge whatever
    /// their images come to, many files of ordinary size merge, and a few that
    /// each hold objects near the limit do not.
    ///
    /// The output is the size of the inputs, and copies of one input (the same
    /// [`Pdf`] many times, as a list of file ids a client sent can ask for) are
    /// the caller's memory only once: what the copies after the first write is
    /// held to 1 GiB. An app that merges a list a client controls bounds the list
    /// too. An output there is no memory for is [`DocumentsError::TooLarge`].
    pub fn merge<'a>(pdfs: impl IntoIterator<Item = &'a Pdf>) -> Result<Pdf, DocumentsError> {
        merge_within(pdfs, ReadLimits::default())
    }

    /// This document followed by the pages of `other`.
    pub fn append(&self, other: &Pdf) -> Result<Pdf, DocumentsError> {
        Pdf::merge([self, other])
    }

    /// The document with `stamp` written on the pages it selects, placed on
    /// what each page shows: its `MediaBox`, cut to its `CropBox`, wherever
    /// that box starts.
    pub fn stamp(&self, stamp: &Stamp) -> Result<Pdf, DocumentsError> {
        stamp.encoded_text()?;
        stamped(self.objects()?, stamp)
    }
    /// The document with the first page of `overlay` drawn over the pages
    /// `pages` selects. An overlay of another size is scaled to fit and
    /// centered on what the page shows. The document and the overlay are held
    /// together to the limits of one document.
    pub fn overlay(&self, overlay: &Pdf, pages: Pages) -> Result<Pdf, DocumentsError> {
        let mut document = self.objects()?;
        let drawn = overlaid(&mut document, overlay, &pages);
        finish(document, drawn)
    }

    /// The document with `title` (and, when given, `author`) in its
    /// metadata.
    pub fn with_metadata(&self, title: &str, author: Option<&str>) -> Result<Pdf, DocumentsError> {
        let mut document = self.objects()?;
        let mut info = match document.trailer.get(b"Info").and_then(Object::as_reference) {
            Some(id) => document.dictionary(id).cloned().unwrap_or_default(),
            None => Dictionary::new(),
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
        let info_id = document.add(info);
        document.trailer.set("Info", info_id);
        save(document)
    }

    /// The title in the document's metadata, if any.
    pub fn title(&self) -> Result<Option<String>, DocumentsError> {
        let document = self.load()?;
        let title = document
            .info()
            .and_then(|info| info.get(b"Title"))
            .and_then(Object::as_str)
            .map(decode_text_string);
        document.check().map_err(pdf_error)?;
        Ok(title)
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
        let mut document = Objects::new();
        let pages_id = document.reserve();
        let font_id = document.add(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
            "Encoding" => "WinAnsiEncoding",
        });
        let resources_id = document.add(dictionary! {
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
            let content_id =
                document.add(Stream::new(Dictionary::new(), content::encode(&operations)));
            let page_id = document.add(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                "Contents" => content_id,
                "Resources" => resources_id,
            });
            kids.push(Object::Reference(page_id));
        }
        let count = kids.len() as i64;
        document.insert(
            pages_id,
            dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => count,
            },
        );
        let catalog_id = document.add(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        save(document).expect("a generated document saves")
    }

    /// The document, read within the default [`ReadLimits`].
    fn load(&self) -> Result<Document, DocumentsError> {
        Document::load(self.0.clone(), ReadLimits::default()).map_err(pdf_error)
    }

    /// Every object of the document, to edit.
    fn objects(&self) -> Result<Objects, DocumentsError> {
        Objects::read(self.load()?).map_err(pdf_error)
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

/// The input cannot be read, or reading it would go over a limit: the
/// client's error.
fn pdf_error(error: PdfError) -> DocumentsError {
    match error {
        PdfError::TooLarge(message) => DocumentsError::TooLarge(message),
        other => DocumentsError::Pdf(other.to_string()),
    }
}

/// An object the input should have and does not: the client's error.
fn missing(what: &str) -> DocumentsError {
    DocumentsError::Pdf(format!("{what} is missing"))
}

/// Writing what this crate made failed: the server's error.
fn write_error(what: &str, error: impl fmt::Display) -> DocumentsError {
    DocumentsError::Engine {
        engine: "pdf",
        message: format!("{what}: {error}"),
    }
}

/// One document with the pages of every PDF in `pdfs`, all read within
/// `limits` together: what the pages and what they lead to are counted as, taken
/// off the memory left for the inputs after them, and the work of reading and
/// editing each, taken off the work. An input is read with what is left, so
/// that many documents together are held to what one may be; what is not kept
/// of an input (its outlines, its forms, its tagged structure) is counted only
/// while it is read.
fn merge_within<'a>(
    pdfs: impl IntoIterator<Item = &'a Pdf>,
    limits: ReadLimits,
) -> Result<Pdf, DocumentsError> {
    let mut merged = Objects::new();
    let pages_id = merged.reserve();
    let mut kids = Vec::new();
    // What is written is held to what one document may write past the size
    // of the inputs.
    let mut input_bytes = 0usize;
    // What the inputs after this one, and the pages list, may still take.
    let mut left = limits;
    // The caller holds a file once, however many times it is listed: what the
    // copies after the first write is all new memory, and is held to the work
    // one document may take, since none of it is counted as it is read.
    let mut seen = HashSet::new();
    let mut copied = 0usize;
    for pdf in pdfs {
        if left.memory == 0 || left.work == 0 {
            return Err(limits_used_up(limits));
        }
        if !seen.insert((pdf.0.as_ptr() as usize, pdf.0.len())) {
            copied = copied.saturating_add(pdf.0.len());
            if copied > limits.work {
                return Err(DocumentsError::TooLarge(format!(
                    "the copies of a PDF listed more than once would write more than {} MiB",
                    limits.work >> 20
                )));
            }
        }
        input_bytes = input_bytes.saturating_add(pdf.0.len());
        let shared = |error| shared_error(error, limits);
        let mut document =
            Objects::read(Document::load(pdf.0.clone(), left).map_err(shared)?).map_err(shared)?;
        document.share_limits();
        flatten_checked(&mut document).map_err(|error| shared_edit_error(error, limits))?;
        document.renumber(merged.max_number().saturating_add(1));
        let mut page_ids: Vec<ObjectId> = document.pages();
        // The work may have run out on the way: the pages after that point
        // are not listed, and are not lost without a word.
        document.check().map_err(shared)?;
        // A page the page tree lists many times is one page, as it is when it
        // is stamped: the output lists as many pages as it has objects for.
        let mut listed = HashSet::new();
        page_ids.retain(|id| listed.insert(*id));
        for &id in &page_ids {
            let Some(page) = document.dictionary_mut(id) else {
                // No object is found once the work has run out.
                document.check().map_err(shared)?;
                return Err(missing("a page of the document"));
            };
            page.set("Parent", pages_id);
            page.remove(b"StructParents");
        }
        document.check().map_err(shared)?;
        // The pages and what they lead to are kept; the rest of the input is
        // let go of here.
        let reached = document.reachable_from(page_ids.iter().copied());
        let page_set: BTreeSet<ObjectId> = page_ids.iter().copied().collect();
        let mut kept = 0usize;
        for (id, object) in std::mem::take(&mut document.objects) {
            let keep = reached.contains(&id)
                && match object.type_name() {
                    Some(b"Catalog" | b"Pages") => false,
                    Some(b"Page") => page_set.contains(&id),
                    _ => true,
                };
            if keep {
                // Counted when its document was read, and now again as kept,
                // but for the bytes of a stream: those are the input's.
                kept = kept.saturating_add(document.memory_kept(&object));
                merged.adopt(id, object);
            }
        }
        left = ReadLimits {
            memory: left.memory.saturating_sub(kept),
            work: left.work.saturating_sub(document.work_counted()),
            ..left
        };
        kids.extend(page_ids.into_iter().map(Object::Reference));
    }
    if kids.is_empty() {
        return Err(DocumentsError::Edit("nothing to merge".into()));
    }
    if left.memory == 0 || left.work == 0 {
        return Err(limits_used_up(limits));
    }
    // What is left is for the pages list, and for compressing what is written.
    merged.set_work_limit(left.work);
    merged.set_added_limit(left.memory);
    let output_limit = input_bytes.saturating_add(limits.memory);
    let count = kids.len() as i64;
    merged.insert(
        pages_id,
        dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => count,
        },
    );
    let catalog_id = merged.add(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    merged.trailer.set("Root", catalog_id);
    merged.prune();
    merged.set_output_limit(output_limit);
    merged
        .check()
        .map_err(|error| shared_error(error, limits))?;
    save(merged).map_err(|error| match error {
        DocumentsError::TooLarge(message) => {
            DocumentsError::TooLarge(format!("{message}; {}", shared_limits(limits)))
        }
        other => other,
    })
}

/// That the PDFs of one edit share the limits of one document, and what those
/// are.
fn shared_limits(limits: ReadLimits) -> String {
    format!(
        "the PDFs of one edit share the limits of one document ({} MiB of memory, {} MiB of \
         bytes scanned)",
        limits.memory >> 20,
        limits.work >> 20,
    )
}

/// The inputs of an edit have used all of `limits`, with more to do.
fn limits_used_up(limits: ReadLimits) -> DocumentsError {
    DocumentsError::TooLarge(format!(
        "{}, and the PDFs before this point have used them up",
        shared_limits(limits)
    ))
}

/// What an input read within what is left of `limits` ran out of: the
/// limit named is what was left, so the error says what the whole is.
fn shared_error(error: PdfError, limits: ReadLimits) -> DocumentsError {
    match error {
        PdfError::TooLarge(message) => {
            DocumentsError::TooLarge(format!("{message}; {}", shared_limits(limits)))
        }
        other => pdf_error(other),
    }
}

/// [`shared_error`] for an error that may be one of another kind.
fn shared_edit_error(error: DocumentsError, limits: ReadLimits) -> DocumentsError {
    match error {
        DocumentsError::TooLarge(message) => shared_error(PdfError::TooLarge(message), limits),
        other => other,
    }
}

/// `document` with the first page of `overlay` drawn over the pages `pages`
/// selects.
fn overlaid(document: &mut Objects, overlay: &Pdf, pages: &Pages) -> Result<(), DocumentsError> {
    flatten_checked(document)?;
    // The overlay is read within what is left of the limits the document was
    // read within, so that the two together are held to those of one.
    let limits = document.limits();
    let loaded = Document::load(overlay.0.clone(), document.limits_left())
        .map_err(|error| shared_error(error, limits))?;
    let first = loaded
        .pages()
        .first()
        .copied()
        .ok_or_else(|| DocumentsError::Edit("the overlay has no pages".into()))?;
    let content = loaded.page_content(first, MAX_PAGE_CONTENT);
    // A limit of the document, when one ran out on the way.
    loaded
        .check()
        .map_err(|error| shared_error(error, limits))?;
    let content = content.map_err(|error| match error {
        ContentError::Over => DocumentsError::TooLarge(format!(
            "the overlay's page content inflates to more than {} MiB",
            MAX_PAGE_CONTENT >> 20
        )),
        _ => DocumentsError::Pdf("the overlay's page content cannot be read".into()),
    })?;
    let mut source = Objects::read(loaded).map_err(|error| shared_error(error, limits))?;
    source.share_limits();
    flatten_checked(&mut source).map_err(|error| shared_edit_error(error, limits))?;
    document
        .spend_work(source.work_counted())
        .map_err(|error| shared_error(error, limits))?;
    let numbers = source.renumber(document.max_number().saturating_add(1));
    document.set_output_limit(document.output_limit().saturating_add(overlay.0.len()));
    let source_page = *numbers
        .get(&first)
        .ok_or_else(|| missing("the overlay's page"))?;
    let (overlay_width, overlay_height) = page_size(&source, source_page)?;
    let page = source
        .dictionary(source_page)
        .ok_or_else(|| missing("the overlay's page"))?;
    let resources = page
        .get(b"Resources")
        .cloned()
        .unwrap_or_else(|| Object::Dictionary(Dictionary::new()));
    let media_box = page
        .get(b"MediaBox")
        .ok_or_else(|| missing("the overlay's MediaBox"))?;
    // Four numbers, or the box of a page that has none: not what else the
    // file gives.
    let shown_box = box_values(&source, media_box);
    let media_box = match four_numbers(&source, media_box) {
        Some(_) => media_box.clone(),
        None => Object::Array(shown_box.iter().map(|&value| Object::Real(value)).collect()),
    };
    let (left, bottom) = (
        PageBox::from_corners(shown_box).left,
        PageBox::from_corners(shown_box).bottom,
    );
    // Everything the overlay page refers to moves into the document;
    // what nothing refers to is pruned below, before saving.
    let mut adopted = 0usize;
    for (id, object) in std::mem::take(&mut source.objects) {
        if !matches!(object.type_name(), Some(b"Catalog" | b"Pages" | b"Page")) {
            adopted = adopted.saturating_add(source.memory_kept(&object));
            document.adopt(id, object);
        }
    }
    // Counted with the document's own from here on.
    document
        .hold(adopted)
        .map_err(|error| shared_error(error, limits))?;
    let form = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => media_box,
            "Resources" => resources,
        },
        content,
    );
    let form_id = document.add(form);
    let mut additions =
        ResourceAdditions::new(vec![(b"XObject", b"NavreeOverlay", form_id.into())]);
    for page_id in selected_pages(document, pages) {
        let shown = page_box(document, page_id)?;
        let (width, height) = (shown.width(), shown.height());
        additions.add_to_page(document, page_id)?;
        let scale = (width / overlay_width).min(height / overlay_height);
        let dx = shown.left + (width - overlay_width * scale) / 2.0 - left * scale;
        let dy = shown.bottom + (height - overlay_height * scale) / 2.0 - bottom * scale;
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
            Operation::new("Do", vec![Object::name("NavreeOverlay")]),
            Operation::new("Q", vec![]),
        ];
        draw_over(document, page_id, &operations)?;
    }
    document.prune();
    Ok(())
}

/// `document` with `stamp` written on the pages it selects.
fn stamped(mut document: Objects, stamp: &Stamp) -> Result<Pdf, DocumentsError> {
    let drawn = draw_stamp(&mut document, stamp);
    finish(document, drawn)
}

fn draw_stamp(document: &mut Objects, stamp: &Stamp) -> Result<(), DocumentsError> {
    let encoded = stamp.encoded_text()?;
    flatten_checked(document)?;
    let font_id = document.add(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica-Bold",
        "Encoding" => "WinAnsiEncoding",
    });
    let opacity = stamp.opacity.clamp(0.0, 1.0);
    let gs_id = document.add(dictionary! {
        "Type" => "ExtGState",
        "ca" => opacity,
        "CA" => opacity,
    });
    let mut additions = ResourceAdditions::new(vec![
        (b"Font", b"NavreeStamp", font_id.into()),
        (b"ExtGState", b"NavreeStampGs", gs_id.into()),
    ]);
    for page_id in selected_pages(document, &stamp.pages) {
        let shown = page_box(document, page_id)?;
        additions.add_to_page(document, page_id)?;
        let mut matrix = stamp.matrix(shown.width(), shown.height());
        matrix[4] += shown.left;
        matrix[5] += shown.bottom;
        let (r, g, b) = stamp.color;
        let operations = vec![
            Operation::new("q", vec![]),
            Operation::new("gs", vec![Object::name("NavreeStampGs")]),
            Operation::new("rg", vec![r.into(), g.into(), b.into()]),
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec![Object::name("NavreeStamp"), stamp.size.into()]),
            Operation::new("Tm", matrix.iter().map(|&v| Object::Real(v)).collect()),
            Operation::new(
                "Tj",
                vec![Object::String(encoded.clone(), StringFormat::Literal)],
            ),
            Operation::new("ET", vec![]),
            Operation::new("Q", vec![]),
        ];
        draw_over(document, page_id, &operations)?;
    }
    Ok(())
}

/// What an edit that has run comes to: when a limit of the edit ran out, that
/// is the error, whatever it looked like on the way (no object is found once
/// the work has run out); otherwise what it returned, and the document saved.
fn finish(document: Objects, drawn: Result<(), DocumentsError>) -> Result<Pdf, DocumentsError> {
    document.check().map_err(pdf_error)?;
    drawn?;
    save(document)
}

/// [`flatten_inherited`], with a limit that ran out as the error.
fn flatten_checked(document: &mut Objects) -> Result<(), DocumentsError> {
    let flattened = flatten_inherited(document);
    document.check().map_err(pdf_error)?;
    flattened
}

/// Writes `document`, within its limit: a file that would write more is the
/// client's.
fn save(mut document: Objects) -> Result<Pdf, DocumentsError> {
    document.compress();
    let out = document.save().map_err(|error| match error {
        PdfError::TooLarge(message) => DocumentsError::TooLarge(message),
        other => write_error("the document could not be written", other),
    })?;
    Ok(Pdf(Bytes::from(out)))
}

/// Whether a value a page inherits is small enough to write on each page: a
/// number, a short name, a reference, or a short array of those. A box is
/// four numbers; anything else becomes one object the pages refer to.
fn small_enough_to_copy(value: &Object) -> bool {
    const LONGEST_NAME: usize = 32;
    const LONGEST_ARRAY: usize = 4;
    let scalar = |value: &Object| match value {
        Object::Null
        | Object::Boolean(_)
        | Object::Integer(_)
        | Object::Real(_)
        | Object::Reference(_) => true,
        Object::Name(name) => name.len() <= LONGEST_NAME,
        _ => false,
    };
    match value {
        Object::Array(items) => items.len() <= LONGEST_ARRAY && items.iter().all(scalar),
        other => scalar(other),
    }
}

/// Copies the attributes a page inherits from its page-tree ancestors
/// (`Resources`, `MediaBox`, `CropBox`, `Rotate`) onto the page, so the
/// page can move to another tree.
///
/// A value larger than a box is not copied onto each page: it becomes one
/// object that every page inheriting it refers to, so a tree of many pages
/// and large resources does not grow by their product. A `Rotate` that is not
/// a number is not a rotation, and is not inherited.
fn flatten_inherited(document: &mut Objects) -> Result<(), DocumentsError> {
    const INHERITED: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];
    // The value each page gets, by the ancestor and key it comes from.
    let mut shared: HashMap<(ObjectId, &[u8]), Object> = HashMap::new();
    let page_ids: Vec<ObjectId> = document.pages();
    for page_id in page_ids {
        document.hold(PAGE_EDIT_MEMORY).map_err(pdf_error)?;
        // The key, and the ancestor that holds it.
        let mut found: Vec<(&[u8], ObjectId)> = Vec::new();
        {
            let page = document
                .dictionary(page_id)
                .ok_or_else(|| missing("a page of the document"))?;
            for key in INHERITED {
                if page.has(key) {
                    continue;
                }
                let mut parent = page.get(b"Parent");
                let mut depth = 0;
                while let Some((id, Some(node))) = parent.and_then(|up| document.followed(up)) {
                    depth += 1;
                    if depth > 64 {
                        break;
                    }
                    let Some(node) = node.as_dict() else {
                        break;
                    };
                    if let Some(value) = node.get(key) {
                        let usable = key != b"Rotate"
                            || document
                                .resolve(value)
                                .is_some_and(|value| value.as_float().is_some());
                        if usable {
                            found.push((key, id));
                        }
                        break;
                    }
                    parent = node.get(b"Parent");
                }
            }
        }
        let mut values = Vec::with_capacity(found.len());
        for (key, holder) in found {
            let value = match shared.get(&(holder, key)) {
                Some(value) => value.clone(),
                None => {
                    let value = document
                        .dictionary(holder)
                        .and_then(|node| node.get(key))
                        .ok_or_else(|| missing("an inherited page attribute"))?;
                    document.hold(memory_of(value)).map_err(pdf_error)?;
                    let value = value.clone();
                    let value = if small_enough_to_copy(&value) {
                        value
                    } else {
                        let id = document.reserve();
                        document.adopt(id, value);
                        Object::Reference(id)
                    };
                    shared.insert((holder, key), value.clone());
                    value
                }
            };
            values.push((key, value));
        }
        let page = document
            .dictionary_mut(page_id)
            .ok_or_else(|| missing("a page of the document"))?;
        for (key, value) in values {
            page.set(key, value);
        }
        if !page.has(b"MediaBox") {
            page.set("MediaBox", vec![0.into(), 0.into(), 612.into(), 792.into()]);
        }
    }
    Ok(())
}

/// A rectangle on a page, in the page's own coordinates.
#[derive(Clone, Copy)]
struct PageBox {
    left: f32,
    bottom: f32,
    right: f32,
    top: f32,
}

impl PageBox {
    /// The box a file writes as two opposite corners, in either order.
    fn from_corners([x0, y0, x1, y1]: [f32; 4]) -> PageBox {
        PageBox {
            left: x0.min(x1),
            bottom: y0.min(y1),
            right: x0.max(x1),
            top: y0.max(y1),
        }
    }

    fn width(&self) -> f32 {
        self.right - self.left
    }

    fn height(&self) -> f32 {
        self.top - self.bottom
    }

    /// Where this box and `other` overlap, when they do.
    fn within(&self, other: PageBox) -> Option<PageBox> {
        let shared = PageBox {
            left: self.left.max(other.left),
            bottom: self.bottom.max(other.bottom),
            right: self.right.min(other.right),
            top: self.top.min(other.top),
        };
        (shared.width() > 0.0 && shared.height() > 0.0).then_some(shared)
    }
}

/// What a page shows (its `MediaBox` cut to its `CropBox`, after
/// `flatten_inherited`): where an edit draws.
fn page_box(document: &Objects, page_id: ObjectId) -> Result<PageBox, DocumentsError> {
    let page = document
        .dictionary(page_id)
        .ok_or_else(|| missing("a page of the document"))?;
    let media_box = page
        .get(b"MediaBox")
        .ok_or_else(|| missing("a page's MediaBox"))?;
    let media_box = PageBox::from_corners(box_values(document, media_box));
    let crop_box = page
        .get(b"CropBox")
        .and_then(|value| four_numbers(document, value))
        .map(PageBox::from_corners);
    Ok(crop_box
        .and_then(|crop_box| crop_box.within(media_box))
        .unwrap_or(media_box))
}

/// The four numbers a box is, read where they are; anything else is no box.
fn four_numbers(document: &Objects, value: &Object) -> Option<[f32; 4]> {
    let [a, b, c, d] = document.resolve(value)?.as_array()?.as_slice() else {
        return None;
    };
    let number = |value: &Object| document.resolve(value)?.as_float();
    Some([number(a)?, number(b)?, number(c)?, number(d)?])
}

/// The four numbers of a box, or those of a page that has none.
fn box_values(document: &Objects, value: &Object) -> [f32; 4] {
    four_numbers(document, value).unwrap_or([0.0, 0.0, 612.0, 792.0])
}

/// Width and height of a page (its `MediaBox`, after `flatten_inherited`).
fn page_size(document: &Objects, page_id: ObjectId) -> Result<(f32, f32), DocumentsError> {
    let page = document
        .dictionary(page_id)
        .ok_or_else(|| missing("a page of the document"))?;
    let media_box = page
        .get(b"MediaBox")
        .ok_or_else(|| missing("a page's MediaBox"))?;
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

/// A copy of the dictionary that `value` leads to, counted before it is made:
/// an empty one when it is none.
fn dictionary_copy(document: &Objects, id: ObjectId) -> Result<Dictionary, DocumentsError> {
    let Some(dictionary) = document
        .followed(&Object::Reference(id))
        .and_then(|(_, object)| object?.as_dict())
    else {
        return Ok(Dictionary::new());
    };
    document
        .hold(
            dictionary
                .iter()
                .map(|(key, value)| key.len() + memory_of(value))
                .sum(),
        )
        .map_err(pdf_error)?;
    Ok(dictionary.clone())
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
        document: &mut Objects,
        page_id: ObjectId,
    ) -> Result<(), DocumentsError> {
        // Moved out of the page and back: what the page has is not copied.
        let current = document
            .dictionary_mut(page_id)
            .ok_or_else(|| missing("a page of the document"))?
            .remove(b"Resources")
            .unwrap_or_else(|| Object::Dictionary(Dictionary::new()));
        let resources = match current {
            Object::Reference(id) => match self.resources.get(&id) {
                Some(copy) => Object::Reference(*copy),
                None => {
                    let mut resources = dictionary_copy(document, id)?;
                    self.add_to(document, &mut resources)?;
                    let copy = document.add(resources);
                    self.resources.insert(id, copy);
                    Object::Reference(copy)
                }
            },
            Object::Dictionary(mut resources) => {
                document.hold(RESOURCES_EDIT_MEMORY).map_err(pdf_error)?;
                self.add_to(document, &mut resources)?;
                Object::Dictionary(resources)
            }
            _ => {
                document.hold(RESOURCES_EDIT_MEMORY).map_err(pdf_error)?;
                let mut resources = Dictionary::new();
                self.add_to(document, &mut resources)?;
                Object::Dictionary(resources)
            }
        };
        document
            .dictionary_mut(page_id)
            .ok_or_else(|| missing("a page of the document"))?
            .set("Resources", resources);
        Ok(())
    }

    /// Adds the entries to `resources`, a dictionary of its own.
    fn add_to(
        &mut self,
        document: &mut Objects,
        resources: &mut Dictionary,
    ) -> Result<(), DocumentsError> {
        for index in 0..self.entries.len() {
            let kind = self.entries[index].0;
            if index > 0 && self.entries[..index].iter().any(|(k, ..)| *k == kind) {
                continue;
            }
            let entries = self.entries.iter().filter(|(k, ..)| *k == kind);
            match resources.get(kind) {
                Some(Object::Reference(id)) => {
                    let id = *id;
                    let copy = match self.kinds.get(&(kind, id)) {
                        Some(copy) => *copy,
                        None => {
                            // One that is not a dictionary is replaced, as
                            // a missing one is.
                            let mut names = dictionary_copy(document, id)?;
                            for (_, name, value) in entries {
                                names.set(*name, value.clone());
                            }
                            let copy = document.add(names);
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
fn selected_pages(document: &Objects, pages: &Pages) -> Vec<ObjectId> {
    let all = document.pages();
    let total = all.len();
    let mut seen = HashSet::new();
    all.into_iter()
        .enumerate()
        .filter(|&(index, page_id)| pages.includes(index + 1, total) && seen.insert(page_id))
        .map(|(_, page_id)| page_id)
        .collect()
}

/// The sub-dictionary `key` of `resources`, created when missing.
fn sub_dict_mut<'a>(
    resources: &'a mut Dictionary,
    key: &[u8],
) -> Result<&'a mut Dictionary, DocumentsError> {
    if !matches!(resources.get(key), Some(Object::Dictionary(_))) {
        resources.set(key.to_vec(), Object::Dictionary(Dictionary::new()));
    }
    resources
        .get_mut(key)
        .and_then(Object::as_dict_mut)
        .ok_or_else(|| missing("a resource dictionary"))
}

/// The streams a page's `Contents` lists: `Contents` may hold nothing else.
/// Counted before they are copied, as pages can share one long array.
fn contents_streams(
    document: &Objects,
    contents: Option<&Object>,
) -> Result<Vec<Object>, DocumentsError> {
    let items = match contents {
        Some(reference @ Object::Reference(_)) => match document.followed(reference) {
            Some((id, Some(Object::Stream(_)))) => return Ok(vec![Object::Reference(id)]),
            Some((_, Some(Object::Array(items)))) => items,
            _ => return Ok(Vec::new()),
        },
        Some(Object::Array(items)) => items,
        _ => return Ok(Vec::new()),
    };
    document
        .hold(items.len().saturating_mul(std::mem::size_of::<Object>()))
        .map_err(pdf_error)?;
    // Room for the most there can be, so that the list is not grown by
    // doubling, which holds up to twice what it is counted as.
    let mut streams = Vec::with_capacity(items.len());
    streams.extend(
        items
            .iter()
            .filter_map(|item| match document.followed(item)? {
                (id, Some(Object::Stream(_))) => Some(Object::Reference(id)),
                _ => None,
            }),
    );
    Ok(streams)
}

/// Appends `operations` to a page after wrapping its existing contents in
/// `q`/`Q`, so whatever state the page leaves does not move the drawing. Stops
/// here when a limit of the edit has run out.
fn draw_over(
    document: &mut Objects,
    page_id: ObjectId,
    operations: &[Operation],
) -> Result<(), DocumentsError> {
    document.hold(PAGE_EDIT_MEMORY).map_err(pdf_error)?;
    let contents = document
        .dictionary(page_id)
        .ok_or_else(|| missing("a page of the document"))?
        .get(b"Contents");
    let existing = contents_streams(document, contents)?;
    let open = document.add(Stream::new(Dictionary::new(), b"q\n".to_vec()));
    let mut close = b"\nQ\n".to_vec();
    close.extend(content::encode(operations));
    let close = document.add(Stream::new(Dictionary::new(), close));
    let mut contents = Vec::with_capacity(existing.len() + 2);
    contents.push(Object::Reference(open));
    contents.extend(existing);
    contents.push(Object::Reference(close));
    document
        .dictionary_mut(page_id)
        .ok_or_else(|| missing("a page of the document"))?
        .set("Contents", contents);
    document.check().map_err(pdf_error)
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
        let unreadable = pdf_error(PdfError::Damaged("object 1 0 is missing".into()));
        assert_eq!(unreadable.status(), http::StatusCode::BAD_REQUEST);
        // So is input that would take more than the limits to read.
        let over = pdf_error(PdfError::TooLarge("over the limit".into()));
        assert_eq!(over.status(), http::StatusCode::PAYLOAD_TOO_LARGE);
    }

    /// `pages` pages with nothing on them.
    fn empty_pages(pages: usize) -> Objects {
        let mut document = Objects::new();
        let tree = document.reserve();
        let kids: Vec<Object> = (0..pages)
            .map(|_| {
                document
                    .add(dictionary! { "Type" => "Page", "Parent" => tree })
                    .into()
            })
            .collect();
        document.insert(
            tree,
            dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => pages as i64 },
        );
        let catalog = document.add(dictionary! { "Type" => "Catalog", "Pages" => tree });
        document.trailer.set("Root", catalog);
        document
    }

    #[test]
    fn what_a_stamp_adds_is_refused_before_it_is_held() {
        let read = || {
            let bytes = empty_pages(4_000).save().unwrap();
            Objects::read(Document::load(bytes, ReadLimits::default()).unwrap()).unwrap()
        };
        // About 3 KiB a page at the most.
        let mut held = read();
        held.set_added_limit(32 * 1024 * 1024);
        let stamp = Stamp::text("X");
        stamped(held, &stamp).unwrap();
        let mut counted = read();
        draw_stamp(&mut counted, &stamp).unwrap();
        let page = counted.memory_added() / 4_000;
        assert!((2 * 1024..3 * 1024).contains(&page), "{page} a page");
        let mut small = read();
        small.set_added_limit(1024 * 1024);
        let error = stamped(small, &stamp).unwrap_err();
        assert!(matches!(error, DocumentsError::TooLarge(_)), "{error:?}");
        assert!(error.to_string().contains("1 MiB"), "{error}");
    }

    #[test]
    fn a_page_is_counted_for_what_it_adds() {
        let bytes = empty_pages(1_000).save().unwrap();
        let mut document =
            Objects::read(Document::load(bytes, ReadLimits::default()).unwrap()).unwrap();
        document.set_added_limit(usize::MAX);
        flatten_inherited(&mut document).unwrap();
        let after_flatten = document.memory_added();
        assert!(after_flatten >= 1_000 * PAGE_EDIT_MEMORY, "{after_flatten}");
    }

    /// A file of a few kilobytes whose page refers to an array of `items`
    /// zeros (in an object stream, which inflates to a few hundred kilobytes):
    /// with `kept`, from the page, which a merge keeps; without, from the
    /// document's information, which it does not.
    fn pdf_with_an_array(items: usize, kept: bool) -> Pdf {
        let mut objects = Objects::new();
        let pages = objects.reserve();
        let array = Object::Array(vec![Object::Integer(0); items]);
        let mut page = dictionary! { "Type" => "Page", "Parent" => pages };
        if kept {
            page.set("Thing", array.clone());
        } else {
            let info = objects.add(dictionary! { "Junk" => array });
            objects.trailer.set("Info", info);
        }
        let page = objects.add(page);
        objects.insert(
            pages,
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        );
        let catalog = objects.add(dictionary! { "Type" => "Catalog", "Pages" => pages });
        objects.trailer.set("Root", catalog);
        objects.compress();
        let bytes = objects.save_with(true).unwrap();
        assert!(bytes.len() < 16 * 1024, "{} bytes", bytes.len());
        Pdf::from_bytes(bytes).unwrap()
    }

    #[test]
    fn a_merge_is_held_to_the_memory_of_one_document_for_all_its_inputs() {
        // About 3 MiB each: two fit in 8 MiB, three do not.
        let pdf = pdf_with_an_array(100_000, true);
        let limits = ReadLimits {
            memory: 8 * 1024 * 1024,
            ..ReadLimits::default()
        };
        let merged = merge_within([&pdf, &pdf], limits).unwrap();
        assert_eq!(merged.page_count().unwrap(), 2);
        let error = merge_within([&pdf, &pdf, &pdf], limits).unwrap_err();
        assert!(matches!(error, DocumentsError::TooLarge(_)), "{error:?}");
        assert!(error.to_string().contains("8 MiB"), "{error}");
    }

    #[test]
    fn a_merge_is_held_to_the_work_of_one_document_for_all_its_inputs() {
        let pdf = pdf_with_an_array(100_000, true);
        let one = Objects::read(Document::load(pdf.0.clone(), ReadLimits::default()).unwrap())
            .unwrap()
            .work_counted();
        // Room for two inputs and half of a third.
        let limits = ReadLimits {
            work: one * 5 / 2,
            ..ReadLimits::default()
        };
        merge_within([&pdf, &pdf], limits).unwrap();
        let error = merge_within([&pdf, &pdf, &pdf], limits).unwrap_err();
        assert!(matches!(error, DocumentsError::TooLarge(_)), "{error:?}");
    }

    #[test]
    fn an_overlay_is_held_with_its_document_to_the_limits_of_one() {
        let base = pdf_with_an_array(170_000, true);
        let limits = ReadLimits {
            memory: 8 * 1024 * 1024,
            ..ReadLimits::default()
        };
        let draw = |overlay: &Pdf| {
            let mut document =
                Objects::read(Document::load(base.0.clone(), limits).unwrap()).unwrap();
            overlaid(&mut document, overlay, &Pages::All)
        };
        // Each is within 8 MiB, and together they are not.
        draw(&Pdf::from_lines(["SEAL"])).unwrap();
        let error = draw(&pdf_with_an_array(110_000, true)).unwrap_err();
        assert!(matches!(error, DocumentsError::TooLarge(_)), "{error:?}");
        assert!(error.to_string().contains("8 MiB"), "{error}");
    }

    #[test]
    fn what_a_merge_does_not_keep_of_an_input_is_counted_only_while_it_is_read() {
        // Ten inputs of 3 MiB each, none of which is kept, in 8 MiB.
        let pdf = pdf_with_an_array(100_000, false);
        let limits = ReadLimits {
            memory: 8 * 1024 * 1024,
            ..ReadLimits::default()
        };
        let merged = merge_within(std::iter::repeat_n(&pdf, 10), limits).unwrap();
        assert_eq!(merged.page_count().unwrap(), 10);
        assert!(merged.len() < 16 * 1024, "{} bytes", merged.len());
    }

    #[test]
    fn a_merge_whose_work_runs_out_is_an_error_and_never_loses_pages() {
        let pdf = Pdf::from_lines((0..130).map(|i| format!("line {i}")));
        assert_eq!(pdf.page_count().unwrap(), 3);
        let mut refused = 0;
        for work in (1024..96 * 1024).step_by(512) {
            let limits = ReadLimits {
                work,
                ..ReadLimits::default()
            };
            match merge_within([&pdf, &pdf], limits) {
                Ok(merged) => assert_eq!(merged.page_count().unwrap(), 6, "work {work}"),
                Err(DocumentsError::TooLarge(_)) => refused += 1,
                Err(other) => panic!("work {work}: {other:?}"),
            }
        }
        assert!(refused > 0);
    }

    #[test]
    fn the_contents_a_page_gets_hold_what_they_are_counted_as() {
        // The list a stamp makes of a page's contents is counted for its length
        // once. A list that grew by doubling would hold up to twice that.
        let mut objects = Objects::new();
        let stream = objects.add(Stream::new(Dictionary::new(), b"n".to_vec()));
        let shared = objects.add(vec![Object::Reference(stream); 2_000]);
        let page = objects.add(dictionary! { "Type" => "Page", "Contents" => shared });
        let operations = [Operation::new("q", vec![])];
        draw_over(&mut objects, page, &operations).unwrap();
        let contents = objects
            .dictionary(page)
            .and_then(|page| page.get(b"Contents"))
            .and_then(Object::as_array)
            .unwrap();
        assert_eq!(contents.len(), 2_002);
        assert_eq!(contents.capacity(), contents.len());
    }

    #[test]
    fn utf16_and_doc_encoded_strings_decode() {
        assert_eq!(decode_text_string(&utf16_text("Ünïcode ✓")), "Ünïcode ✓");
        assert_eq!(decode_text_string(b"Plain"), "Plain");
    }
}
