//! Text extraction from PDF, DOCX, HTML and plain text, for search
//! indexes and retrieval-augmented generation.
//!
//! ```
//! use navree_media::text;
//!
//! # fn main() -> Result<(), navree_media::MediaError> {
//! let html = b"<h1>Policy</h1><p>Lock your <b>screen</b>.</p><script>x()</script>";
//! let out = text::extract(html, "text/html")?;
//! assert_eq!(out.text, "Policy\n\nLock your screen.");
//! # Ok(())
//! # }
//! ```

use std::borrow::Cow;
use std::cell::{Cell, RefCell};

use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::Serialize;

use crate::MediaError;
use crate::archive::{Archive, ArchiveLimits};
use crate::mime;

/// The MIME type of Word documents (DOCX).
pub const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

/// The most bytes a PDF page's content may inflate to.
const PDF_PAGE_LIMIT: usize = 64 * 1024 * 1024;

/// The text of a document.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ExtractedText {
    /// The whole text: paragraphs on their own lines, PDF pages separated
    /// by a blank line, whitespace collapsed, no leading or trailing
    /// whitespace.
    pub text: String,
    /// The text of each PDF page; other documents have one page holding
    /// the whole text.
    pub pages: Vec<String>,
    /// The document's title (`<title>`, the DOCX core properties, the PDF
    /// information dictionary), when it has one.
    pub title: Option<String>,
    /// The type the text was read as.
    pub mime_type: String,
}

impl ExtractedText {
    fn single(text: String, title: Option<String>, mime_type: &str) -> Self {
        Self {
            pages: vec![text.clone()],
            text,
            title,
            mime_type: mime_type.to_owned(),
        }
    }
}

/// Whether [`extract`] reads documents of type `mime_type`: PDF, DOCX,
/// HTML and XHTML, and `text/*` (plain text, Markdown, CSV).
pub fn supports(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "application/pdf" | DOCX | "text/html" | "application/xhtml+xml"
    ) || mime_type.starts_with("text/")
}

/// Extracts the text of a document of type `mime_type`. A type
/// [`extract`] does not read (`application/octet-stream` among them) is
/// sniffed from the content first; one that is still not supported is
/// [`MediaError::Unsupported`], and a damaged document
/// [`MediaError::Text`].
pub fn extract(bytes: &[u8], mime_type: &str) -> Result<ExtractedText, MediaError> {
    let mime_type = if supports(mime_type) {
        Cow::Borrowed(mime_type)
    } else {
        Cow::Owned(mime::sniff(bytes, None))
    };
    match mime_type.as_ref() {
        "application/pdf" => pdf(bytes),
        DOCX => docx(bytes),
        "text/html" | "application/xhtml+xml" => {
            let (text, title) = html(&String::from_utf8_lossy(bytes));
            Ok(ExtractedText::single(text, title, &mime_type))
        }
        other if other.starts_with("text/") => {
            let text = String::from_utf8_lossy(bytes);
            let text = text
                .trim_start_matches('\u{feff}')
                .replace("\r\n", "\n")
                .trim()
                .to_owned();
            Ok(ExtractedText::single(text, None, other))
        }
        other => Err(MediaError::Unsupported {
            operation: "text extraction",
            mime_type: other.to_owned(),
        }),
    }
}

/// PDF text through lopdf, page by page, each page's content inflated no
/// further than 64 MiB.
fn pdf(bytes: &[u8]) -> Result<ExtractedText, MediaError> {
    let options = lopdf::LoadOptions {
        max_decompressed_size: Some(PDF_PAGE_LIMIT),
        ..lopdf::LoadOptions::default()
    };
    let document = lopdf::Document::load_mem_with_options(bytes, options)
        .map_err(|error| MediaError::text(format!("PDF: {error}")))?;
    if document.is_encrypted() {
        return Err(MediaError::text("PDF: the document is encrypted"));
    }
    let mut pages = Vec::new();
    for number in document.get_pages().keys() {
        let page = document
            .extract_text_with_limit(&[*number], PDF_PAGE_LIMIT)
            .map_err(|error| MediaError::text(format!("PDF page {number}: {error}")))?;
        pages.push(tidy(&page));
    }
    let title = pdf_title(&document);
    let text = pages
        .iter()
        .filter(|page| !page.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok(ExtractedText {
        text,
        pages,
        title,
        mime_type: "application/pdf".to_owned(),
    })
}

/// The `Title` of the PDF information dictionary.
fn pdf_title(document: &lopdf::Document) -> Option<String> {
    let info = document.trailer.get(b"Info").ok()?;
    let info = match info {
        lopdf::Object::Reference(id) => document.get_dictionary(*id).ok()?,
        lopdf::Object::Dictionary(dictionary) => dictionary,
        _ => return None,
    };
    let title = info.get(b"Title").ok()?;
    let title = lopdf::decode_text_string(title).ok()?;
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_owned())
}

/// Trims every line and drops runs of blank lines.
fn tidy(text: &str) -> String {
    let mut out = String::new();
    let mut blank = false;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank { "\n\n" } else { "\n" });
        }
        blank = false;
        out.push_str(&line);
    }
    out
}

/// DOCX: the paragraphs of `word/document.xml` (runs, tabs and breaks)
/// and the title in `docProps/core.xml`.
fn docx(bytes: &[u8]) -> Result<ExtractedText, MediaError> {
    let limits = ArchiveLimits {
        max_entry_bytes: 256 * 1024 * 1024,
        ..ArchiveLimits::default()
    };
    let mut archive = Archive::open(bytes.to_vec(), &limits)
        .map_err(|error| MediaError::text(format!("DOCX: {error}")))?;
    let document = archive
        .read("word/document.xml")
        .map_err(|_| MediaError::text("DOCX: the archive has no word/document.xml"))?;
    let text = docx_text(&String::from_utf8_lossy(&document))?;
    let title = archive
        .read("docProps/core.xml")
        .ok()
        .and_then(|core| docx_title(&String::from_utf8_lossy(&core)));
    Ok(ExtractedText::single(text, title, DOCX))
}

fn docx_text(xml: &str) -> Result<String, MediaError> {
    let mut reader = Reader::from_str(xml);
    let mut out = String::new();
    let mut in_text = false;
    loop {
        let event = reader
            .read_event()
            .map_err(|error| MediaError::text(format!("DOCX: {error}")))?;
        match event {
            Event::Start(tag) if tag.local_name().as_ref() == b"t" => in_text = true,
            Event::End(tag) => match tag.local_name().as_ref() {
                b"t" => in_text = false,
                b"p" => out.push('\n'),
                _ => {}
            },
            Event::Empty(tag) => match tag.local_name().as_ref() {
                b"tab" => out.push('\t'),
                b"br" | b"cr" => out.push('\n'),
                b"p" => out.push('\n'),
                _ => {}
            },
            Event::Text(text) if in_text => {
                let text = text
                    .decode()
                    .map_err(|error| MediaError::text(format!("DOCX: {error}")))?;
                out.push_str(&text);
            }
            Event::GeneralRef(reference) if in_text => push_reference(&mut out, &reference),
            Event::Eof => break,
            _ => {}
        }
    }
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    let mut text = lines.join("\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    Ok(text.trim().to_owned())
}

/// An entity or character reference inside XML text.
fn push_reference(out: &mut String, reference: &quick_xml::events::BytesRef<'_>) {
    if let Ok(Some(c)) = reference.resolve_char_ref() {
        out.push(c);
        return;
    }
    let name = reference.decode().unwrap_or_default();
    out.push_str(match name.as_ref() {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        _ => "",
    });
}

fn docx_title(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    let mut in_title = false;
    let mut title = String::new();
    loop {
        match reader.read_event().ok()? {
            Event::Start(tag) if tag.local_name().as_ref() == b"title" => in_title = true,
            Event::End(tag) if tag.local_name().as_ref() == b"title" => break,
            Event::Text(text) if in_title => title.push_str(&text.decode().ok()?),
            Event::GeneralRef(reference) if in_title => push_reference(&mut title, &reference),
            Event::Eof => break,
            _ => {}
        }
    }
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_owned())
}

/// What the HTML sink collects: text with break markers, and the title.
#[derive(Default)]
struct HtmlText {
    raw: RefCell<String>,
    title: RefCell<Option<String>>,
    /// Inside `<title>`.
    in_title: Cell<bool>,
    /// The raw-text element being skipped (`script`, `style`, ...).
    skipping: RefCell<Option<String>>,
    /// Nesting depth inside `<template>` and `<head>`.
    hidden: Cell<u32>,
}

/// A line break between the texts around it.
const LINE: char = '\u{1}';
/// A blank line between the texts around it.
const PARAGRAPH: char = '\u{2}';

fn block_break(name: &str) -> Option<char> {
    Some(match name {
        "br" | "li" | "tr" | "dt" | "dd" | "option" | "caption" | "figcaption" => LINE,
        "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "dl" | "table"
        | "section" | "article" | "header" | "footer" | "nav" | "aside" | "main" | "blockquote"
        | "pre" | "hr" | "form" | "figure" | "address" | "fieldset" | "details" | "summary"
        | "body" => PARAGRAPH,
        _ => return None,
    })
}

impl HtmlText {
    fn tag(&self, tag: &Tag) -> TokenSinkResult<()> {
        let name: &str = &tag.name;
        let start = tag.kind == TagKind::StartTag;
        let skipped = self.skipping.borrow().clone();
        if let Some(skipped) = skipped {
            if !start && skipped == name {
                self.skipping.replace(None);
            }
            return TokenSinkResult::Continue;
        }
        match (name, start) {
            ("title", true) => {
                self.in_title.set(true);
                return TokenSinkResult::RawData(RawKind::Rcdata);
            }
            ("title", false) => self.in_title.set(false),
            ("script", true) => {
                self.skip(name);
                return TokenSinkResult::RawData(RawKind::ScriptData);
            }
            ("style" | "noscript" | "xmp" | "iframe" | "noembed" | "noframes", true) => {
                self.skip(name);
                return TokenSinkResult::RawData(RawKind::Rawtext);
            }
            ("textarea", true) => {
                self.skip(name);
                return TokenSinkResult::RawData(RawKind::Rcdata);
            }
            ("head" | "template", true) if !tag.self_closing => {
                self.hidden.set(self.hidden.get() + 1);
            }
            ("head" | "template", false) => self.hidden.set(self.hidden.get().saturating_sub(1)),
            ("body", true) => self.hidden.set(0),
            ("td" | "th", _) => self.raw.borrow_mut().push(' '),
            _ => {}
        }
        if let Some(marker) = block_break(name) {
            self.raw.borrow_mut().push(marker);
        }
        TokenSinkResult::Continue
    }

    fn skip(&self, name: &str) {
        self.skipping.replace(Some(name.to_owned()));
    }

    fn text(&self, text: &str) {
        if self.in_title.get() {
            let mut title = self.title.borrow_mut();
            title.get_or_insert_with(String::new).push_str(text);
        } else if self.skipping.borrow().is_none() && self.hidden.get() == 0 {
            self.raw
                .borrow_mut()
                .extend(text.chars().filter(|c| *c != LINE && *c != PARAGRAPH));
        }
    }
}

impl TokenSink for HtmlText {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        match token {
            Token::TagToken(tag) => self.tag(&tag),
            Token::CharacterTokens(text) => {
                self.text(&text);
                TokenSinkResult::Continue
            }
            _ => TokenSinkResult::Continue,
        }
    }
}

/// The visible text of an HTML document and its title.
pub(crate) fn html(source: &str) -> (String, Option<String>) {
    let tokenizer = Tokenizer::new(HtmlText::default(), TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(source.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let sink = tokenizer.sink;
    let raw = sink.raw.into_inner();
    let title = sink
        .title
        .into_inner()
        .map(|title| title.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|title| !title.is_empty());
    (collapse(&raw), title)
}

/// Collapses whitespace runs to one space and break markers to one
/// newline (a line) or two (a paragraph), trimming around them.
fn collapse(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut space = false;
    let mut breaks = 0;
    for c in raw.chars() {
        match c {
            LINE => {
                breaks = breaks.max(1);
                space = false;
            }
            PARAGRAPH => {
                breaks = 2;
                space = false;
            }
            c if c.is_ascii_whitespace() => space = true,
            c => {
                if !out.is_empty() {
                    if breaks > 0 {
                        out.push_str(if breaks == 2 { "\n\n" } else { "\n" });
                    } else if space {
                        out.push(' ');
                    }
                }
                breaks = 0;
                space = false;
                out.push(c);
            }
        }
    }
    out
}
