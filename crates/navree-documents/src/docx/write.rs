//! Writing a DOCX file: the WordprocessingML parts of a [`Docx`] and the
//! zip archive that holds them.
//!
//! The package has the parts Word needs and no more: the content types and
//! relationships, the document, its styles and settings, the numbering of
//! the lists, a header and a footer when the document has them, the images
//! as media parts, and the document properties. Elements come in the order
//! the schema gives, which Word and LibreOffice both rely on.

use std::fmt::Write as _;
use std::io::{Cursor, Write as _};

use bytes::Bytes;
use chrono::{SecondsFormat, Utc};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use super::{Align, Block, Docx, FooterContent, Image, Paragraph, Run, Table, emu, twips};
use crate::DocumentsError;
use crate::print::Margins;

const XML_DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;

const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";

const REL_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const REL_CORE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const REL_APP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const REL_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const REL_SETTINGS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";
const REL_NUMBERING: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering";
const REL_HEADER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
const REL_FOOTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

const TYPE_RELATIONSHIPS: &str = "application/vnd.openxmlformats-package.relationships+xml";
const TYPE_CORE: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const TYPE_APP: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
const TYPE_DOCUMENT: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const TYPE_STYLES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const TYPE_SETTINGS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
const TYPE_NUMBERING: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const TYPE_HEADER: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const TYPE_FOOTER: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";

/// Pixels are 1/96 inch, an inch is 914,400 EMU.
const EMU_PER_PIXEL: u64 = 9525;
/// The largest size in half-points Word accepts for a run (819 pt).
const MAX_HALF_POINTS: u32 = 1638;

/// One file of the archive.
struct Part {
    name: String,
    data: Vec<u8>,
    /// Kept as it is (images are compressed already).
    stored: bool,
}

impl Part {
    fn xml(name: &str, xml: String) -> Self {
        Self {
            name: name.to_owned(),
            data: xml.into_bytes(),
            stored: false,
        }
    }
}

/// An image of the document, as a part of the package.
struct Media {
    /// `image1.png`.
    name: String,
    bytes: Bytes,
    content_type: &'static str,
    extension: &'static str,
}

/// The bytes of the package `docx` builds.
pub(super) fn build(docx: &Docx) -> Result<Vec<u8>, DocumentsError> {
    let page = Page::of(docx);
    let mut body = Body {
        rtl: docx.rtl(),
        text_width: page.text_width(),
        xml: String::new(),
        lists: Vec::new(),
        media: Vec::new(),
    };
    for block in &docx.blocks {
        body.block(block)?;
    }
    let rtl = body.rtl;
    let has_numbering = !body.lists.is_empty();
    let has_header = docx.header.is_some();
    let has_footer = docx.footer.is_some();

    let mut document = String::new();
    let _ = write!(
        document,
        "{XML_DECLARATION}<w:document xmlns:w=\"{NS_W}\" xmlns:r=\"{NS_R}\" xmlns:wp=\"{NS_WP}\"><w:body>"
    );
    document.push_str(&body.xml);
    page.section(&mut document, has_header, has_footer);
    document.push_str("</w:body></w:document>");

    let mut parts = vec![
        Part::xml(
            "[Content_Types].xml",
            content_types(has_numbering, has_header, has_footer, &body.media),
        ),
        Part::xml("_rels/.rels", package_relationships()),
        Part::xml("docProps/core.xml", core_properties(docx)),
        Part::xml("docProps/app.xml", app_properties()),
        Part::xml("word/document.xml", document),
        Part::xml(
            "word/_rels/document.xml.rels",
            document_relationships(has_numbering, has_header, has_footer, &body.media),
        ),
        Part::xml("word/styles.xml", styles(docx)),
        Part::xml("word/settings.xml", settings()),
    ];
    if has_numbering {
        parts.push(Part::xml("word/numbering.xml", numbering(&body.lists)));
    }
    if let Some(text) = &docx.header {
        parts.push(Part::xml(
            "word/header1.xml",
            header_or_footer("hdr", rtl, |body| {
                body.paragraph(&Paragraph::new().text(text.as_str()), None, None, false);
            }),
        ));
    }
    match &docx.footer {
        Some(FooterContent::Text(text)) => parts.push(Part::xml(
            "word/footer1.xml",
            header_or_footer("ftr", rtl, |body| {
                body.paragraph(&Paragraph::new().text(text.as_str()), None, None, false);
            }),
        )),
        Some(FooterContent::PageNumbers) => parts.push(Part::xml(
            "word/footer1.xml",
            header_or_footer("ftr", rtl, |body| body.page_numbers()),
        )),
        None => {}
    }
    for media in body.media {
        parts.push(Part {
            name: format!("word/media/{}", media.name),
            data: media.bytes.to_vec(),
            stored: true,
        });
    }
    zip_of(parts)
}

fn zip_of(parts: Vec<Part>) -> Result<Vec<u8>, DocumentsError> {
    let failed = |error: &dyn std::fmt::Display| DocumentsError::Docx(error.to_string());
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for part in parts {
        // Every entry has the same fixed date, so the archive's bytes depend
        // on the document and not on when it was zipped.
        let options = SimpleFileOptions::default()
            .compression_method(if part.stored {
                CompressionMethod::Stored
            } else {
                CompressionMethod::Deflated
            })
            .last_modified_time(DateTime::default());
        zip.start_file(part.name, options).map_err(|e| failed(&e))?;
        zip.write_all(&part.data).map_err(|e| failed(&e))?;
    }
    let cursor = zip.finish().map_err(|e| failed(&e))?;
    Ok(cursor.into_inner())
}

/// The page: its size and margins, in twips.
struct Page {
    width: u32,
    height: u32,
    landscape: bool,
    top: u32,
    right: u32,
    bottom: u32,
    left: u32,
}

impl Page {
    fn of(docx: &Docx) -> Self {
        let (mut width, mut height) = docx.paper.size_mm();
        if docx.landscape {
            std::mem::swap(&mut width, &mut height);
        }
        let margins = docx.margins.unwrap_or(Margins::all(25.4));
        let to_twips = |mm: f64| u32::try_from(twips(mm).max(0)).unwrap_or(0);
        Self {
            width: to_twips(width).max(1),
            height: to_twips(height).max(1),
            landscape: docx.landscape,
            top: to_twips(margins.top),
            right: to_twips(margins.right),
            bottom: to_twips(margins.bottom),
            left: to_twips(margins.left),
        }
    }

    /// The width between the margins.
    fn text_width(&self) -> u32 {
        self.width
            .saturating_sub(self.left)
            .saturating_sub(self.right)
            .max(1)
    }

    /// The section properties: the last element of the body.
    fn section(&self, out: &mut String, header: bool, footer: bool) {
        out.push_str("<w:sectPr>");
        if header {
            out.push_str(r#"<w:headerReference w:type="default" r:id="rIdHeader1" />"#);
        }
        if footer {
            out.push_str(r#"<w:footerReference w:type="default" r:id="rIdFooter1" />"#);
        }
        let orient = if self.landscape {
            r#" w:orient="landscape""#
        } else {
            ""
        };
        let _ = write!(
            out,
            r#"<w:pgSz w:w="{}" w:h="{}"{orient} /><w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="720" w:footer="720" w:gutter="0" /></w:sectPr>"#,
            self.width, self.height, self.top, self.right, self.bottom, self.left
        );
    }
}

/// The body of the document being written.
struct Body {
    rtl: bool,
    /// The width a table spans, in twips.
    text_width: u32,
    xml: String,
    /// Whether each list is ordered; the list numbered `n` is at `n - 1`.
    lists: Vec<bool>,
    media: Vec<Media>,
}

impl Body {
    fn block(&mut self, block: &Block) -> Result<(), DocumentsError> {
        match block {
            Block::Heading(level, paragraph) => {
                self.paragraph(paragraph, Some(&format!("Heading{level}")), None, false);
            }
            Block::Paragraph(paragraph) => self.paragraph(paragraph, None, None, false),
            Block::List { ordered, items } => {
                self.lists.push(*ordered);
                let id = self.lists.len();
                for item in items {
                    self.paragraph(item, None, Some(id), false);
                }
            }
            Block::Table(table) => self.table(table),
            Block::Image(image) => self.image(image)?,
            Block::PageBreak => {
                self.xml
                    .push_str(r#"<w:p><w:r><w:br w:type="page" /></w:r></w:p>"#);
            }
        }
        Ok(())
    }

    /// `<w:p>` with its properties (style, list, direction, alignment) and
    /// runs; `bold` makes every run bold.
    fn paragraph(
        &mut self,
        paragraph: &Paragraph,
        style: Option<&str>,
        list: Option<usize>,
        bold: bool,
    ) {
        let rtl = self.rtl;
        write_paragraph(&mut self.xml, paragraph, style, list, rtl, bold);
    }

    fn table(&mut self, table: &Table) {
        let rows = table.header.iter().chain(table.rows.iter());
        let columns = rows.clone().map(Vec::len).max();
        let Some(columns) = columns else {
            // Word does not open a table without rows.
            return;
        };
        let columns = columns.max(1);
        let width = (self.text_width / u32::try_from(columns).unwrap_or(u32::MAX)).max(1);
        let out = &mut self.xml;
        out.push_str("<w:tbl><w:tblPr>");
        if self.rtl {
            out.push_str("<w:bidiVisual />");
        }
        out.push_str(
            r#"<w:tblW w:w="5000" w:type="pct" /><w:tblBorders><w:top w:val="single" w:sz="4" w:space="0" w:color="auto" /><w:left w:val="single" w:sz="4" w:space="0" w:color="auto" /><w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto" /><w:right w:val="single" w:sz="4" w:space="0" w:color="auto" /><w:insideH w:val="single" w:sz="4" w:space="0" w:color="auto" /><w:insideV w:val="single" w:sz="4" w:space="0" w:color="auto" /></w:tblBorders></w:tblPr><w:tblGrid>"#,
        );
        for _ in 0..columns {
            let _ = write!(out, r#"<w:gridCol w:w="{width}" />"#);
        }
        out.push_str("</w:tblGrid>");
        let rtl = self.rtl;
        for (index, row) in rows.enumerate() {
            let header = index == 0 && table.header.is_some();
            out.push_str("<w:tr>");
            if header {
                out.push_str("<w:trPr><w:cantSplit /><w:tblHeader /></w:trPr>");
            }
            // A row without cells would not open in Word.
            let empty = [Paragraph::new()];
            let cells = if row.is_empty() {
                &empty[..]
            } else {
                row.as_slice()
            };
            for cell in cells {
                let _ = write!(
                    out,
                    r#"<w:tc><w:tcPr><w:tcW w:w="{width}" w:type="dxa" /></w:tcPr>"#
                );
                write_paragraph(out, cell, None, None, rtl, header);
                out.push_str("</w:tc>");
            }
            out.push_str("</w:tr>");
        }
        out.push_str("</w:tbl>");
    }

    fn image(&mut self, image: &Image) -> Result<(), DocumentsError> {
        let (width_px, height_px) = image.size()?;
        let (content_type, extension) = if image.bytes.starts_with(b"\x89PNG") {
            ("image/png", "png")
        } else {
            ("image/jpeg", "jpeg")
        };
        let number = self.media.len() + 1;
        let file = format!("image{number}.{extension}");
        let (cx, cy) = match image.width_mm {
            Some(width_mm) => {
                let height_mm = width_mm * f64::from(height_px) / f64::from(width_px);
                (u64::from(emu(width_mm)), u64::from(emu(height_mm)))
            }
            None => (
                u64::from(width_px) * EMU_PER_PIXEL,
                u64::from(height_px) * EMU_PER_PIXEL,
            ),
        };
        self.media.push(Media {
            name: file.clone(),
            bytes: image.bytes.clone(),
            content_type,
            extension,
        });
        let out = &mut self.xml;
        out.push_str("<w:p>");
        write_paragraph_properties(out, None, None, self.rtl, image.align);
        let _ = write!(
            out,
            concat!(
                r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0">"#,
                r#"<wp:extent cx="{cx}" cy="{cy}" /><wp:effectExtent l="0" t="0" r="0" b="0" />"#,
                r#"<wp:docPr id="{number}" name="Picture {number}" />"#,
                r#"<wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a="{a}" noChangeAspect="1" /></wp:cNvGraphicFramePr>"#,
                r#"<a:graphic xmlns:a="{a}"><a:graphicData uri="{pic}">"#,
                r#"<pic:pic xmlns:pic="{pic}"><pic:nvPicPr><pic:cNvPr id="{number}" name="{file}" /><pic:cNvPicPr /></pic:nvPicPr>"#,
                r#"<pic:blipFill><a:blip r:embed="rIdImage{number}" /><a:stretch><a:fillRect /></a:stretch></pic:blipFill>"#,
                r#"<pic:spPr><a:xfrm><a:off x="0" y="0" /><a:ext cx="{cx}" cy="{cy}" /></a:xfrm>"#,
                r#"<a:prstGeom prst="rect"><a:avLst /></a:prstGeom></pic:spPr></pic:pic>"#,
                r#"</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#,
            ),
            a = NS_A,
            pic = NS_PIC,
            cx = cx,
            cy = cy,
            number = number,
            file = file,
        );
        Ok(())
    }

    /// "Page N of M", centered, with Word's fields for N and M.
    fn page_numbers(&mut self) {
        let out = &mut self.xml;
        out.push_str("<w:p>");
        write_paragraph_properties(out, None, None, self.rtl, Some(Align::Center));
        out.push_str(r#"<w:r><w:t xml:space="preserve">Page </w:t></w:r>"#);
        field(out, "PAGE");
        out.push_str(r#"<w:r><w:t xml:space="preserve"> of </w:t></w:r>"#);
        field(out, "NUMPAGES");
        out.push_str("</w:p>");
    }
}

/// A field: its code, and `1` as the text Word shows until it updates.
fn field(out: &mut String, code: &str) {
    let _ = write!(
        out,
        concat!(
            r#"<w:r><w:fldChar w:fldCharType="begin" /></w:r>"#,
            r#"<w:r><w:instrText xml:space="preserve"> {code} </w:instrText></w:r>"#,
            r#"<w:r><w:fldChar w:fldCharType="separate" /></w:r>"#,
            r#"<w:r><w:t>1</w:t></w:r>"#,
            r#"<w:r><w:fldChar w:fldCharType="end" /></w:r>"#,
        ),
        code = code
    );
}

/// `<w:p>` for `paragraph`.
fn write_paragraph(
    out: &mut String,
    paragraph: &Paragraph,
    style: Option<&str>,
    list: Option<usize>,
    rtl: bool,
    bold: bool,
) {
    out.push_str("<w:p>");
    write_paragraph_properties(out, style, list, rtl, paragraph.align);
    for run in &paragraph.runs {
        write_run(out, run, bold);
    }
    out.push_str("</w:p>");
}

/// `<w:pPr>`, in the order of the schema, or nothing when there is none.
fn write_paragraph_properties(
    out: &mut String,
    style: Option<&str>,
    list: Option<usize>,
    rtl: bool,
    align: Option<Align>,
) {
    if style.is_none() && list.is_none() && !rtl && align.is_none() {
        return;
    }
    out.push_str("<w:pPr>");
    if let Some(style) = style {
        out.push_str(r#"<w:pStyle w:val=""#);
        escape(out, style);
        out.push_str(r#"" />"#);
    }
    if let Some(list) = list {
        let _ = write!(
            out,
            r#"<w:numPr><w:ilvl w:val="0" /><w:numId w:val="{list}" /></w:numPr>"#
        );
    }
    if rtl {
        out.push_str("<w:bidi />");
    }
    if let Some(align) = align {
        let value = match align {
            Align::Start => "start",
            Align::Center => "center",
            Align::End => "end",
            Align::Justify => "both",
        };
        let _ = write!(out, r#"<w:jc w:val="{value}" />"#);
    }
    out.push_str("</w:pPr>");
}

/// `<w:r>` for `run`: its properties in the order of the schema, then its
/// text, with a tab or a break element for each tab and new line.
fn write_run(out: &mut String, run: &Run, bold: bool) {
    out.push_str("<w:r>");
    let bold = bold || run.bold;
    let color = run.color.as_deref().and_then(hex_color);
    let size = run.size.map(half_points);
    if bold || run.italic || run.underline || size.is_some() || color.is_some() {
        out.push_str("<w:rPr>");
        if bold {
            out.push_str("<w:b /><w:bCs />");
        }
        if run.italic {
            out.push_str("<w:i /><w:iCs />");
        }
        if let Some(color) = color {
            let _ = write!(out, r#"<w:color w:val="{color}" />"#);
        }
        if let Some(size) = size {
            let _ = write!(out, r#"<w:sz w:val="{size}" /><w:szCs w:val="{size}" />"#);
        }
        if run.underline {
            out.push_str(r#"<w:u w:val="single" />"#);
        }
        out.push_str("</w:rPr>");
    }
    write_text(out, &run.text);
    out.push_str("</w:r>");
}

/// A size in points as half-points, from 1 to what Word accepts.
fn half_points(points: f32) -> u32 {
    let half = (points * 2.0).round();
    if half.is_nan() {
        1
    } else {
        half.clamp(1.0, MAX_HALF_POINTS as f32) as u32
    }
}

/// Text as `w:t`, `w:tab` and `w:br` elements.
fn write_text(out: &mut String, text: &str) {
    fn flush(out: &mut String, pending: &mut String) {
        if !pending.is_empty() {
            out.push_str(r#"<w:t xml:space="preserve">"#);
            escape(out, pending);
            out.push_str("</w:t>");
            pending.clear();
        }
    }
    let mut pending = String::new();
    for c in text.chars() {
        match c {
            '\n' => {
                flush(out, &mut pending);
                out.push_str("<w:br />");
            }
            '\t' => {
                flush(out, &mut pending);
                out.push_str("<w:tab />");
            }
            '\r' => {}
            c => pending.push(c),
        }
    }
    flush(out, &mut pending);
}

/// `color` as six upper-case hex digits (`C00000`, `#c00` or `c00`), or
/// `None` when it is not a color Word accepts.
fn hex_color(color: &str) -> Option<String> {
    let digits = color.trim().trim_start_matches('#');
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match digits.len() {
        6 => Some(digits.to_ascii_uppercase()),
        3 => Some(
            digits
                .chars()
                .flat_map(|c| [c, c])
                .collect::<String>()
                .to_ascii_uppercase(),
        ),
        _ => None,
    }
}

/// `text` escaped for XML text and attribute values. Characters XML 1.0
/// cannot hold, and control characters, are left out.
fn escape(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' => out.push(c),
            c if c.is_control() => {}
            '\u{fffe}' | '\u{ffff}' => {}
            c => out.push(c),
        }
    }
}

fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    escape(&mut out, text);
    out
}

/// A header or footer part (`hdr` or `ftr`) whose paragraphs `fill` writes.
fn header_or_footer(root: &str, rtl: bool, fill: impl FnOnce(&mut Body)) -> String {
    let mut body = Body {
        rtl,
        text_width: 0,
        xml: String::new(),
        lists: Vec::new(),
        media: Vec::new(),
    };
    fill(&mut body);
    format!(
        "{XML_DECLARATION}<w:{root} xmlns:w=\"{NS_W}\" xmlns:r=\"{NS_R}\">{}</w:{root}>",
        body.xml
    )
}

fn content_types(numbering: bool, header: bool, footer: bool, media: &[Media]) -> String {
    let mut out = format!(
        r#"{XML_DECLARATION}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="{TYPE_RELATIONSHIPS}" /><Default Extension="xml" ContentType="application/xml" />"#
    );
    let mut seen: Vec<&str> = Vec::new();
    for item in media {
        if !seen.contains(&item.extension) {
            seen.push(item.extension);
            let _ = write!(
                out,
                r#"<Default Extension="{}" ContentType="{}" />"#,
                item.extension, item.content_type
            );
        }
    }
    let mut overrides = vec![
        ("/word/document.xml", TYPE_DOCUMENT),
        ("/word/styles.xml", TYPE_STYLES),
        ("/word/settings.xml", TYPE_SETTINGS),
    ];
    if numbering {
        overrides.push(("/word/numbering.xml", TYPE_NUMBERING));
    }
    if header {
        overrides.push(("/word/header1.xml", TYPE_HEADER));
    }
    if footer {
        overrides.push(("/word/footer1.xml", TYPE_FOOTER));
    }
    overrides.push(("/docProps/core.xml", TYPE_CORE));
    overrides.push(("/docProps/app.xml", TYPE_APP));
    for (part, content_type) in overrides {
        let _ = write!(
            out,
            r#"<Override PartName="{part}" ContentType="{content_type}" />"#
        );
    }
    out.push_str("</Types>");
    out
}

/// A relationships part: `(id, type, target)` for each relationship.
fn relationships(items: &[(String, &str, String)]) -> String {
    let mut out = format!(
        r#"{XML_DECLARATION}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#
    );
    for (id, kind, target) in items {
        let _ = write!(
            out,
            r#"<Relationship Id="{id}" Type="{kind}" Target="{target}" />"#
        );
    }
    out.push_str("</Relationships>");
    out
}

fn package_relationships() -> String {
    relationships(&[
        (
            "rId1".to_owned(),
            REL_DOCUMENT,
            "word/document.xml".to_owned(),
        ),
        ("rId2".to_owned(), REL_CORE, "docProps/core.xml".to_owned()),
        ("rId3".to_owned(), REL_APP, "docProps/app.xml".to_owned()),
    ])
}

fn document_relationships(numbering: bool, header: bool, footer: bool, media: &[Media]) -> String {
    let mut items = vec![
        ("rIdStyles".to_owned(), REL_STYLES, "styles.xml".to_owned()),
        (
            "rIdSettings".to_owned(),
            REL_SETTINGS,
            "settings.xml".to_owned(),
        ),
    ];
    if numbering {
        items.push((
            "rIdNumbering".to_owned(),
            REL_NUMBERING,
            "numbering.xml".to_owned(),
        ));
    }
    if header {
        items.push((
            "rIdHeader1".to_owned(),
            REL_HEADER,
            "header1.xml".to_owned(),
        ));
    }
    if footer {
        items.push((
            "rIdFooter1".to_owned(),
            REL_FOOTER,
            "footer1.xml".to_owned(),
        ));
    }
    for (index, item) in media.iter().enumerate() {
        items.push((
            format!("rIdImage{}", index + 1),
            REL_IMAGE,
            format!("media/{}", item.name),
        ));
    }
    relationships(&items)
}

/// The title and author (File > Info in Word) and the dates.
fn core_properties(docx: &Docx) -> String {
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let mut out = format!(
        r#"{XML_DECLARATION}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">"#
    );
    if let Some(title) = &docx.title {
        let _ = write!(out, "<dc:title>{}</dc:title>", escaped(title));
    }
    if let Some(author) = &docx.author {
        let author = escaped(author);
        let _ = write!(
            out,
            "<dc:creator>{author}</dc:creator><cp:lastModifiedBy>{author}</cp:lastModifiedBy>"
        );
    }
    let _ = write!(
        out,
        r#"<dcterms:created xsi:type="dcterms:W3CDTF">{now}</dcterms:created><dcterms:modified xsi:type="dcterms:W3CDTF">{now}</dcterms:modified></cp:coreProperties>"#
    );
    out
}

fn app_properties() -> String {
    format!(
        r#"{XML_DECLARATION}<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Navree</Application></Properties>"#
    )
}

fn settings() -> String {
    format!(
        r#"{XML_DECLARATION}<w:settings xmlns:w="{NS_W}"><w:zoom w:percent="100" /><w:defaultTabStop w:val="720" /><w:compat><w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15" /></w:compat></w:settings>"#
    )
}

/// The default text properties, the built-in styles Word expects, and the
/// three heading styles.
fn styles(docx: &Docx) -> String {
    let mut out = format!(
        r#"{XML_DECLARATION}<w:styles xmlns:w="{NS_W}"><w:docDefaults><w:rPrDefault><w:rPr>"#
    );
    if let Some(font) = &docx.font {
        let font = escaped(font);
        let _ = write!(
            out,
            r#"<w:rFonts w:ascii="{font}" w:hAnsi="{font}" w:eastAsia="{font}" w:cs="{font}" />"#
        );
    }
    if let Some(locale) = &docx.locale {
        let tag = escaped(&locale.replace('_', "-"));
        let attribute = if docx.rtl() { "bidi" } else { "val" };
        let _ = write!(out, r#"<w:lang w:{attribute}="{tag}" />"#);
    }
    out.push_str(concat!(
        "</w:rPr></w:rPrDefault></w:docDefaults>",
        r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal" /><w:qFormat /></w:style>"#,
        r#"<w:style w:type="character" w:default="1" w:styleId="DefaultParagraphFont"><w:name w:val="Default Paragraph Font" /><w:uiPriority w:val="1" /><w:semiHidden /><w:unhideWhenUsed /></w:style>"#,
        r#"<w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table" /><w:uiPriority w:val="99" /><w:semiHidden /><w:unhideWhenUsed /><w:tblPr><w:tblInd w:w="0" w:type="dxa" /><w:tblCellMar><w:top w:w="0" w:type="dxa" /><w:left w:w="108" w:type="dxa" /><w:bottom w:w="0" w:type="dxa" /><w:right w:w="108" w:type="dxa" /></w:tblCellMar></w:tblPr></w:style>"#,
        r#"<w:style w:type="numbering" w:default="1" w:styleId="NoList"><w:name w:val="No List" /><w:uiPriority w:val="99" /><w:semiHidden /><w:unhideWhenUsed /></w:style>"#,
    ));
    for (level, size) in [(1, 32), (2, 28), (3, 24)] {
        let _ = write!(
            out,
            concat!(
                r#"<w:style w:type="paragraph" w:styleId="Heading{level}"><w:name w:val="heading {level}" />"#,
                r#"<w:basedOn w:val="Normal" /><w:next w:val="Normal" /><w:uiPriority w:val="9" /><w:qFormat />"#,
                r#"<w:pPr><w:keepNext /><w:keepLines /><w:spacing w:before="240" w:after="120" /><w:outlineLvl w:val="{outline}" /></w:pPr>"#,
                r#"<w:rPr><w:b /><w:bCs /><w:sz w:val="{size}" /><w:szCs w:val="{size}" /></w:rPr></w:style>"#,
            ),
            level = level,
            outline = level - 1,
            size = size,
        );
    }
    out.push_str("</w:styles>");
    out
}

/// One abstract numbering and one numbering for each list, so that every
/// numbered list starts from 1.
fn numbering(lists: &[bool]) -> String {
    let mut out = format!(r#"{XML_DECLARATION}<w:numbering xmlns:w="{NS_W}">"#);
    for (index, ordered) in lists.iter().enumerate() {
        let (format, text) = if *ordered {
            ("decimal", "%1.")
        } else {
            ("bullet", "\u{2022}")
        };
        let _ = write!(
            out,
            concat!(
                r#"<w:abstractNum w:abstractNumId="{index}"><w:multiLevelType w:val="singleLevel" />"#,
                r#"<w:lvl w:ilvl="0"><w:start w:val="1" /><w:numFmt w:val="{format}" /><w:lvlText w:val="{text}" />"#,
                r#"<w:lvlJc w:val="left" /><w:pPr><w:ind w:left="720" w:hanging="360" /></w:pPr></w:lvl></w:abstractNum>"#,
            ),
            index = index,
            format = format,
            text = text,
        );
    }
    for index in 0..lists.len() {
        let _ = write!(
            out,
            r#"<w:num w:numId="{}"><w:abstractNumId w:val="{index}" /></w:num>"#,
            index + 1
        );
    }
    out.push_str("</w:numbering>");
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Read;

    use quick_xml::events::Event;
    use quick_xml::name::ResolveResult;
    use quick_xml::reader::NsReader;

    use super::*;
    use crate::docx::{Align, DocxFile, Image, Paragraph, Run, Table};
    use crate::print::{Margins, Paper};

    fn logo() -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/templates/images/logo.png"),
        )
        .unwrap()
    }

    /// The parts of a built package, in archive order.
    fn parts(docx: &Docx) -> Vec<(String, Vec<u8>)> {
        let bytes = build(docx).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut parts = Vec::new();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).unwrap();
            let mut data = Vec::new();
            file.read_to_end(&mut data).unwrap();
            parts.push((file.name().unwrap().into_owned(), data));
        }
        parts
    }

    fn named(parts: &[(String, Vec<u8>)]) -> BTreeMap<&str, String> {
        parts
            .iter()
            .map(|(name, data)| (name.as_str(), String::from_utf8_lossy(data).into_owned()))
            .collect()
    }

    fn part(docx: &Docx, name: &str) -> String {
        let parts = parts(docx);
        named(&parts)
            .remove(name)
            .unwrap_or_else(|| panic!("no part {name}"))
    }

    fn document(docx: &Docx) -> String {
        part(docx, "word/document.xml")
    }

    /// Reads `xml` to its end: every part must be well-formed.
    fn well_formed(name: &str, xml: &str) {
        let mut reader = NsReader::from_str(xml);
        loop {
            match reader.read_resolved_event() {
                Ok((ResolveResult::Unknown(prefix), _)) => {
                    panic!("{name}: the prefix `{prefix}` is not declared")
                }
                Ok((_, Event::Eof)) => break,
                Ok(_) => {}
                Err(error) => panic!("{name}: {error}\n{xml}"),
            }
        }
    }

    /// The attributes `name` of the elements with local name `element`.
    fn attributes(xml: &str, element: &str, name: &str) -> Vec<String> {
        let mut reader = NsReader::from_str(xml);
        let mut found = Vec::new();
        loop {
            match reader.read_event().unwrap() {
                Event::Start(e) | Event::Empty(e) => {
                    if e.local_name().into_inner() == element {
                        for attribute in e.attributes().flatten() {
                            if attribute.key.local_name().into_inner() == name {
                                found.push(
                                    attribute
                                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                        .unwrap()
                                        .into_owned(),
                                );
                            }
                        }
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
        found
    }

    /// Checks that the package holds together: every part is well-formed XML
    /// with a content type, every override names a part that exists, every
    /// relationship leads to a part, and every relationship id the document
    /// uses is defined.
    fn consistent(docx: &Docx) {
        let parts = parts(docx);
        let by_name = named(&parts);
        assert_eq!(
            parts[0].0, "[Content_Types].xml",
            "the content types come first"
        );
        for (name, xml) in &by_name {
            if name.ends_with(".xml") || name.ends_with(".rels") {
                well_formed(name, xml);
            }
        }
        let types = &by_name["[Content_Types].xml"];
        let overrides = attributes(types, "Override", "PartName");
        let defaults = attributes(types, "Default", "Extension");
        for name in by_name.keys().filter(|n| **n != "[Content_Types].xml") {
            let extension = name.rsplit('.').next().unwrap();
            assert!(
                overrides.contains(&format!("/{name}")) || defaults.iter().any(|d| d == extension),
                "{name} has no content type"
            );
        }
        for name in &overrides {
            assert!(
                by_name.contains_key(name.trim_start_matches('/')),
                "{name} is declared and missing"
            );
        }
        for (rels_name, base) in [
            ("_rels/.rels", ""),
            ("word/_rels/document.xml.rels", "word/"),
        ] {
            let rels = &by_name[rels_name];
            let ids = attributes(rels, "Relationship", "Id");
            let mut unique = ids.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(ids.len(), unique.len(), "{rels_name} repeats an id");
            for target in attributes(rels, "Relationship", "Target") {
                assert!(
                    by_name.contains_key(format!("{base}{target}").as_str()),
                    "{rels_name} leads to {target}, which is missing"
                );
            }
        }
        let document = &by_name["word/document.xml"];
        let ids = attributes(
            &by_name["word/_rels/document.xml.rels"],
            "Relationship",
            "Id",
        );
        for attribute in ["embed", "id"] {
            for used in attributes(document, "blip", attribute)
                .into_iter()
                .chain(attributes(document, "headerReference", attribute))
                .chain(attributes(document, "footerReference", attribute))
            {
                assert!(ids.contains(&used), "{used} is used and not defined");
            }
        }
    }

    fn report() -> Docx {
        Docx::new()
            .title("Quarterly audit report")
            .author("Acme Ltd")
            .heading(1, "Quarterly audit report")
            .heading(2, "Summary")
            .paragraph(
                Paragraph::new()
                    .text("Prepared for ")
                    .bold("Acme Ltd")
                    .text(" by ")
                    .italic("the audit team")
                    .text("."),
            )
            .bullets(["Every control was tested", "Two findings are open"])
            .numbered(["Rotate the keys", "Restore a backup"])
            .table(
                Table::new()
                    .header(["Control", "Status"])
                    .row(["Multi-factor sign-in", "Passed"])
                    .row(["Backups restored", "Open"]),
            )
            .image(Image::new(logo()).width_mm(30.0).align(Align::Center))
            .page_break()
            .paragraph(Paragraph::from("Appendix").align(Align::End))
            .paragraph(Run::new("Line one\nLine two").size(9.0).color("#555555"))
            .header("Acme Ltd - confidential")
            .footer_page_numbers()
    }

    #[test]
    fn an_empty_document_is_a_complete_package() {
        let docx = Docx::new();
        let parts = parts(&docx);
        let names: Vec<&str> = parts.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "[Content_Types].xml",
                "_rels/.rels",
                "docProps/core.xml",
                "docProps/app.xml",
                "word/document.xml",
                "word/_rels/document.xml.rels",
                "word/styles.xml",
                "word/settings.xml",
            ]
        );
        consistent(&docx);
        let file = docx.build().unwrap();
        assert_eq!(file.text().unwrap(), "");
    }

    #[test]
    fn a_full_document_is_a_complete_package() {
        let docx = report();
        let parts = parts(&docx);
        let names: Vec<&str> = parts.iter().map(|(name, _)| name.as_str()).collect();
        for expected in [
            "word/numbering.xml",
            "word/header1.xml",
            "word/footer1.xml",
            "word/media/image1.png",
        ] {
            assert!(names.contains(&expected), "{names:?}");
        }
        consistent(&docx);
        let media = parts
            .iter()
            .find(|(n, _)| n == "word/media/image1.png")
            .unwrap();
        assert_eq!(media.1, logo());
    }

    #[test]
    fn the_package_reads_back_as_the_text_that_went_in() {
        let file = report().build().unwrap();
        assert_eq!(
            file.text().unwrap(),
            "Quarterly audit report\nSummary\nPrepared for Acme Ltd by the audit team.\n\
             Every control was tested\nTwo findings are open\nRotate the keys\nRestore a backup\n\
             Control\tStatus\nMulti-factor sign-in\tPassed\nBackups restored\tOpen\n[image]\n\u{c}\n\
             Appendix\nLine one\nLine two"
        );
    }

    #[test]
    fn text_is_escaped_and_tabs_and_breaks_are_elements() {
        let docx = Docx::new().paragraph("a<b>&\"'\tc\nd\r\ne\u{1}f\u{7f}g\u{fffe}h");
        let xml = document(&docx);
        assert!(xml.contains("a&lt;b&gt;&amp;&quot;&apos;"), "{xml}");
        assert!(xml.contains("<w:tab />"), "{xml}");
        assert!(xml.contains("<w:br />"), "{xml}");
        assert!(!xml.contains('\u{1}') && !xml.contains('\r'), "{xml:?}");
        consistent(&docx);
        assert_eq!(
            docx.build().unwrap().text().unwrap(),
            "a<b>&\"'\tc\nd\nefgh"
        );
    }

    #[test]
    fn spaces_and_empty_text_are_kept() {
        let docx = Docx::new()
            .paragraph("  two  spaces  ")
            .paragraph(Run::new(""))
            .paragraph("");
        let xml = document(&docx);
        assert!(
            xml.contains(r#"<w:t xml:space="preserve">  two  spaces  </w:t>"#),
            "{xml}"
        );
        consistent(&docx);
        assert_eq!(docx.build().unwrap().text().unwrap(), "  two  spaces  \n\n");
    }

    #[test]
    fn runs_carry_their_format_in_schema_order() {
        let docx = Docx::new().paragraph(
            Paragraph::new().run(
                Run::new("x")
                    .bold()
                    .italic()
                    .underline()
                    .size(10.5)
                    .color("c00000"),
            ),
        );
        let xml = document(&docx);
        assert!(
            xml.contains(
                r#"<w:rPr><w:b /><w:bCs /><w:i /><w:iCs /><w:color w:val="C00000" /><w:sz w:val="21" /><w:szCs w:val="21" /><w:u w:val="single" /></w:rPr>"#
            ),
            "{xml}"
        );
        let plain = document(&Docx::new().paragraph("x"));
        assert!(!plain.contains("<w:rPr"), "{plain}");
    }

    #[test]
    fn a_color_that_is_not_hex_is_left_out() {
        for (given, written) in [
            ("#c00", Some("CC0000")),
            ("00ff7F", Some("00FF7F")),
            ("red", None),
            ("C00000\" /><w:x", None),
            ("12345", None),
            ("", None),
        ] {
            let xml = document(&Docx::new().paragraph(Run::new("x").color(given)));
            match written {
                Some(hex) => assert!(
                    xml.contains(&format!(r#"<w:color w:val="{hex}" />"#)),
                    "{given}: {xml}"
                ),
                None => assert!(!xml.contains("<w:color"), "{given}: {xml}"),
            }
        }
    }

    #[test]
    fn sizes_stay_inside_what_word_accepts() {
        for (points, half_points) in [
            (0.0, 1),
            (-3.0, 1),
            (9.0, 18),
            (5000.0, 1638),
            (f32::NAN, 1),
        ] {
            let xml = document(&Docx::new().paragraph(Run::new("x").size(points)));
            assert!(
                xml.contains(&format!(r#"<w:sz w:val="{half_points}" />"#)),
                "{points}: {xml}"
            );
        }
    }

    #[test]
    fn alignment_follows_the_text_direction() {
        for (align, value) in [
            (Align::Start, "start"),
            (Align::Center, "center"),
            (Align::End, "end"),
            (Align::Justify, "both"),
        ] {
            let xml = document(&Docx::new().paragraph(Paragraph::from("x").align(align)));
            assert!(
                xml.contains(&format!(r#"<w:pPr><w:jc w:val="{value}" /></w:pPr>"#)),
                "{xml}"
            );
        }
    }

    #[test]
    fn paragraph_properties_come_in_schema_order() {
        let docx = Docx::new()
            .locale("ar")
            .heading(1, Paragraph::from("h").align(Align::Center))
            .bullets(["item"]);
        let xml = document(&docx);
        assert!(
            xml.contains(
                r#"<w:pPr><w:pStyle w:val="Heading1" /><w:bidi /><w:jc w:val="center" /></w:pPr>"#
            ),
            "{xml}"
        );
        assert!(
            xml.contains(r#"<w:pPr><w:numPr><w:ilvl w:val="0" /><w:numId w:val="1" /></w:numPr><w:bidi /></w:pPr>"#),
            "{xml}"
        );
    }

    #[test]
    fn headings_use_the_heading_styles_and_levels_are_clamped() {
        let docx = Docx::new()
            .heading(0, "zero")
            .heading(1, "one")
            .heading(2, "two")
            .heading(3, "three")
            .heading(9, "nine");
        let xml = document(&docx);
        assert_eq!(
            xml.matches(r#"<w:pStyle w:val="Heading1" />"#).count(),
            2,
            "{xml}"
        );
        assert_eq!(
            xml.matches(r#"<w:pStyle w:val="Heading2" />"#).count(),
            1,
            "{xml}"
        );
        assert_eq!(
            xml.matches(r#"<w:pStyle w:val="Heading3" />"#).count(),
            2,
            "{xml}"
        );
        let styles = part(&docx, "word/styles.xml");
        for (level, size) in [(1, 32), (2, 28), (3, 24)] {
            assert!(
                styles.contains(&format!(r#"w:styleId="Heading{level}""#)),
                "{styles}"
            );
            assert!(
                styles.contains(&format!(r#"<w:outlineLvl w:val="{}" />"#, level - 1)),
                "{styles}"
            );
            assert!(
                styles.contains(&format!(r#"<w:sz w:val="{size}" />"#)),
                "{styles}"
            );
        }
        assert!(styles.contains(r#"w:styleId="Normal""#), "{styles}");
        consistent(&docx);
    }

    #[test]
    fn every_list_is_numbered_from_its_own_start() {
        let docx = Docx::new()
            .numbered(["a", "b"])
            .bullets(["c"])
            .numbered(["d"]);
        let xml = document(&docx);
        assert_eq!(attributes(&xml, "numId", "val"), ["1", "1", "2", "3"]);
        let numbering = part(&docx, "word/numbering.xml");
        assert_eq!(attributes(&numbering, "num", "numId"), ["1", "2", "3"]);
        assert_eq!(
            attributes(&numbering, "abstractNum", "abstractNumId"),
            ["0", "1", "2"]
        );
        assert_eq!(
            attributes(&numbering, "abstractNumId", "val"),
            ["0", "1", "2"]
        );
        assert_eq!(
            attributes(&numbering, "numFmt", "val"),
            ["decimal", "bullet", "decimal"]
        );
        assert_eq!(
            attributes(&numbering, "lvlText", "val"),
            ["%1.", "\u{2022}", "%1."]
        );
        assert_eq!(attributes(&numbering, "start", "val"), ["1", "1", "1"]);
        // All the abstract numberings come before all the numberings.
        assert!(numbering.rfind("<w:abstractNum ").unwrap() < numbering.find("<w:num ").unwrap());
        consistent(&docx);
        let none = Docx::new().paragraph("x");
        assert!(
            !parts(&none)
                .iter()
                .any(|(name, _)| name == "word/numbering.xml")
        );
    }

    #[test]
    fn a_table_has_a_grid_a_repeating_header_and_cells_of_equal_width() {
        let docx = Docx::new().table(
            Table::new()
                .header(["a", "b", "c"])
                .row(["1", "2"])
                .row(["3", "4", "5", "6"]),
        );
        let xml = document(&docx);
        // A4 with margins of 25.4 mm leaves 11906 - 2 * 1440 twips; 4 columns.
        assert_eq!(attributes(&xml, "gridCol", "w"), ["2256"; 4], "{xml}");
        assert_eq!(attributes(&xml, "tcW", "w"), ["2256"; 9], "{xml}");
        assert_eq!(xml.matches("<w:tblHeader />").count(), 1, "{xml}");
        assert_eq!(xml.matches("<w:tr>").count(), 3, "{xml}");
        // The header's runs are bold, the others' are not.
        assert_eq!(xml.matches("<w:b />").count(), 3, "{xml}");
        assert!(
            xml.contains(r#"<w:tblW w:w="5000" w:type="pct" />"#),
            "{xml}"
        );
        assert!(!xml.contains("bidiVisual"), "{xml}");
        consistent(&docx);
        assert_eq!(
            docx.build().unwrap().text().unwrap(),
            "a\tb\tc\n1\t2\n3\t4\t5\t6"
        );
    }

    #[test]
    fn a_table_with_nothing_in_it_is_left_out_and_a_row_has_a_cell() {
        let docx = Docx::new()
            .table(Table::new())
            .paragraph("between")
            .table(Table::new().row(Vec::<String>::new()));
        let xml = document(&docx);
        assert_eq!(xml.matches("<w:tbl>").count(), 1, "{xml}");
        assert_eq!(xml.matches("<w:tc>").count(), 1, "{xml}");
        consistent(&docx);
        assert_eq!(docx.build().unwrap().text().unwrap(), "between\n");
    }

    #[test]
    fn right_to_left_documents_flip_paragraphs_and_tables_but_not_page_breaks() {
        let docx = Docx::new()
            .locale("ar-EG")
            .heading(1, "h")
            .page_break()
            .table(Table::new().row(["a"]));
        let xml = document(&docx);
        assert_eq!(xml.matches("<w:bidi />").count(), 2, "{xml}");
        assert!(xml.contains("<w:tblPr><w:bidiVisual /><w:tblW"), "{xml}");
        assert!(
            xml.contains(r#"<w:p><w:r><w:br w:type="page" /></w:r></w:p>"#),
            "{xml}"
        );
        let styles = part(&docx, "word/styles.xml");
        assert!(styles.contains(r#"<w:lang w:bidi="ar-EG" />"#), "{styles}");
        let ltr = part(&Docx::new().locale("en_GB"), "word/styles.xml");
        assert!(ltr.contains(r#"<w:lang w:val="en-GB" />"#), "{ltr}");
        let none = part(&Docx::new(), "word/styles.xml");
        assert!(!none.contains("<w:lang"), "{none}");
        consistent(&docx);
    }

    #[test]
    fn a_font_is_set_for_every_script() {
        let styles = part(&Docx::new().font("Noto \"Sans\" Arabic"), "word/styles.xml");
        assert!(
            styles.contains(
                r#"<w:rFonts w:ascii="Noto &quot;Sans&quot; Arabic" w:hAnsi="Noto &quot;Sans&quot; Arabic" w:eastAsia="Noto &quot;Sans&quot; Arabic" w:cs="Noto &quot;Sans&quot; Arabic" />"#
            ),
            "{styles}"
        );
        well_formed("styles", &styles);
    }

    #[test]
    fn images_are_media_parts_with_their_size() {
        let jpeg = [
            0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00,
            0x20, 0x00, 0x30, 0x03,
        ];
        let docx = Docx::new()
            .image(Image::new(jpeg.to_vec()))
            .image(Image::new(logo()).width_mm(30.0))
            .image(Image::new(logo()).align(Align::End));
        let parts = parts(&docx);
        let names: Vec<&str> = parts.iter().map(|(name, _)| name.as_str()).collect();
        assert!(names.contains(&"word/media/image1.jpeg"), "{names:?}");
        assert!(names.contains(&"word/media/image2.png"), "{names:?}");
        assert!(names.contains(&"word/media/image3.png"), "{names:?}");
        let xml = document(&docx);
        // 48 by 32 pixels at 96 dots per inch: 9525 EMU a pixel.
        assert_eq!(attributes(&xml, "extent", "cx")[0], "457200", "{xml}");
        assert_eq!(attributes(&xml, "extent", "cy")[0], "304800", "{xml}");
        // 30 mm wide keeps the proportions of the logo.
        assert_eq!(attributes(&xml, "extent", "cx")[1], "1080000", "{xml}");
        assert_eq!(attributes(&xml, "docPr", "id"), ["1", "2", "3"]);
        assert_eq!(attributes(&xml, "blip", "embed").len(), 3);
        let types = named(&parts)["[Content_Types].xml"].clone();
        assert!(
            types.contains(r#"Extension="jpeg""#) && types.contains(r#"Extension="png""#),
            "{types}"
        );
        assert!(xml.contains(r#"<w:jc w:val="end" />"#), "{xml}");
        consistent(&docx);
    }

    #[test]
    fn an_image_that_is_not_png_or_jpeg_is_refused() {
        let error = build(&Docx::new().image(Image::new(&b"GIF89a"[..]))).unwrap_err();
        assert!(
            matches!(&error, DocumentsError::Docx(message) if message.contains("PNG")),
            "{error:?}"
        );
    }

    #[test]
    fn the_header_and_footer_are_parts_the_section_refers_to() {
        let docx = Docx::new()
            .header("Top <secret>")
            .footer_page_numbers()
            .locale("he");
        let header = part(&docx, "word/header1.xml");
        assert!(
            header.contains("Top &lt;secret&gt;") && header.contains("<w:bidi />"),
            "{header}"
        );
        let footer = part(&docx, "word/footer1.xml");
        assert!(
            footer.contains(r#"<w:t xml:space="preserve">Page </w:t>"#),
            "{footer}"
        );
        assert!(
            footer.contains(r#"<w:t xml:space="preserve"> of </w:t>"#),
            "{footer}"
        );
        assert!(
            footer.contains("<w:instrText xml:space=\"preserve\"> PAGE </w:instrText>"),
            "{footer}"
        );
        assert!(
            footer.contains("<w:instrText xml:space=\"preserve\"> NUMPAGES </w:instrText>"),
            "{footer}"
        );
        assert_eq!(
            footer.matches(r#"w:fldCharType="begin""#).count(),
            2,
            "{footer}"
        );
        assert_eq!(
            footer.matches(r#"w:fldCharType="separate""#).count(),
            2,
            "{footer}"
        );
        assert_eq!(
            footer.matches(r#"w:fldCharType="end""#).count(),
            2,
            "{footer}"
        );
        assert!(footer.contains(r#"<w:jc w:val="center" />"#), "{footer}");
        let xml = document(&docx);
        assert_eq!(attributes(&xml, "headerReference", "type"), ["default"]);
        assert_eq!(attributes(&xml, "footerReference", "type"), ["default"]);
        consistent(&docx);
        let text = part(&Docx::new().footer("Page one\nof two"), "word/footer1.xml");
        assert!(text.contains("<w:br />"), "{text}");
        let neither = document(&Docx::new());
        assert!(
            !neither.contains("headerReference") && !neither.contains("footerReference"),
            "{neither}"
        );
    }

    #[test]
    fn the_section_holds_the_page_size_orientation_and_margins() {
        let xml = document(&Docx::new());
        assert!(
            xml.contains(r#"<w:pgSz w:w="11906" w:h="16838" />"#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0" />"#),
            "{xml}"
        );
        let wide = Docx::new()
            .paper(Paper::Letter)
            .landscape()
            .margins(Margins {
                top: 10.0,
                right: 20.0,
                bottom: 30.0,
                left: 40.0,
            });
        let xml = document(&wide);
        assert!(
            xml.contains(r#"<w:pgSz w:w="15840" w:h="12240" w:orient="landscape" />"#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"<w:pgMar w:top="567" w:right="1134" w:bottom="1701" w:left="2268""#),
            "{xml}"
        );
        // The section is the last child of the body.
        assert!(xml.contains("</w:sectPr></w:body></w:document>"), "{xml}");
    }

    #[test]
    fn the_properties_hold_the_title_the_author_and_the_dates() {
        let docx = Docx::new().title("Audit <Q3> & more").author("Navree");
        let core = part(&docx, "docProps/core.xml");
        assert!(
            core.contains("<dc:title>Audit &lt;Q3&gt; &amp; more</dc:title>"),
            "{core}"
        );
        assert!(core.contains("<dc:creator>Navree</dc:creator>"), "{core}");
        assert!(
            core.contains("<cp:lastModifiedBy>Navree</cp:lastModifiedBy>"),
            "{core}"
        );
        for tag in ["created", "modified"] {
            let start = core
                .find(&format!("<dcterms:{tag} xsi:type=\"dcterms:W3CDTF\">"))
                .unwrap();
            let value = &core[start..];
            let value = value.split('>').nth(1).unwrap().split('<').next().unwrap();
            assert!(
                chrono::DateTime::parse_from_rfc3339(value).is_ok(),
                "{value}"
            );
            assert!(value.ends_with('Z'), "{value}");
        }
        let bare = part(&Docx::new(), "docProps/core.xml");
        assert!(
            !bare.contains("dc:title") && !bare.contains("dc:creator"),
            "{bare}"
        );
        let app = part(&docx, "docProps/app.xml");
        assert!(app.contains("<Application>Navree</Application>"), "{app}");
    }

    #[test]
    fn the_archive_is_deflated_and_its_dates_are_fixed() {
        let bytes = build(&report()).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        for index in 0..archive.len() {
            let file = archive.by_index(index).unwrap();
            let name = file.name().unwrap().into_owned();
            let png = name.ends_with(".png");
            assert_eq!(
                file.compression(),
                if png {
                    zip::CompressionMethod::Stored
                } else {
                    zip::CompressionMethod::Deflated
                },
                "{name}"
            );
            assert_eq!(file.last_modified().unwrap().year(), 1980);
        }
    }

    #[test]
    fn a_large_document_builds_and_reads_back() {
        let mut docx = Docx::new();
        for n in 0..20_000 {
            docx = docx.paragraph(format!("paragraph {n}"));
        }
        let file = docx.build().unwrap();
        let text = file.text().unwrap();
        assert_eq!(text.lines().count(), 20_000);
        assert!(text.ends_with("paragraph 19999"));
        let _: DocxFile = DocxFile::from_bytes(file.into_bytes()).unwrap();
    }

    /// A small deterministic generator (an LCG), so the test needs no crate.
    struct Random(u64);

    impl Random {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) as usize) % n
        }

        fn text(&mut self) -> String {
            const PIECES: [&str; 20] = [
                "a",
                "word",
                " ",
                "  ",
                "<",
                ">",
                "&",
                "\"",
                "'",
                "\t",
                "\n",
                "\r",
                "\u{1}",
                "\u{fffe}",
                "\u{e9}",
                "\u{4e2d}",
                "\u{1f600}",
                "\u{627}\u{644}",
                "&amp;",
                "]]>",
            ];
            (0..self.below(6))
                .map(|_| PIECES[self.below(PIECES.len())])
                .collect()
        }
    }

    /// What reading gives back for text that went in: carriage returns and
    /// control characters are left out, tabs and new lines stay.
    fn kept(text: &str) -> String {
        text.chars()
            .filter(|c| matches!(c, '\t' | '\n') || !(c.is_control() || *c == '\u{fffe}'))
            .collect()
    }

    fn cells(random: &mut Random) -> Vec<String> {
        (0..random.below(4)).map(|_| random.text()).collect()
    }

    fn row_line(cells: &[String]) -> String {
        cells
            .iter()
            .map(|cell| kept(cell))
            .collect::<Vec<_>>()
            .join("\t")
    }

    #[test]
    fn whatever_text_goes_in_comes_back_out() {
        let mut random = Random(0x6e61_7672_6565);
        for round in 0..300 {
            let mut docx = Docx::new();
            let mut lines: Vec<String> = Vec::new();
            for _ in 0..random.below(8) {
                match random.below(7) {
                    0 | 1 => {
                        let text = random.text();
                        lines.push(kept(&text));
                        docx = docx.paragraph(text);
                    }
                    2 => {
                        let text = random.text();
                        lines.push(kept(&text));
                        docx = docx.heading(random.below(5) as u8, text);
                    }
                    3 => {
                        let items: Vec<String> =
                            (0..random.below(4)).map(|_| random.text()).collect();
                        lines.extend(items.iter().map(|item| kept(item)));
                        docx = if random.below(2) == 0 {
                            docx.bullets(items)
                        } else {
                            docx.numbered(items)
                        };
                    }
                    4 => {
                        let mut table = Table::new();
                        if random.below(2) == 0 {
                            let header = cells(&mut random);
                            lines.push(row_line(&header));
                            table = table.header(header);
                        }
                        for _ in 0..random.below(4) {
                            let row = cells(&mut random);
                            lines.push(row_line(&row));
                            table = table.row(row);
                        }
                        docx = docx.table(table);
                    }
                    5 => {
                        lines.push("[image]".to_owned());
                        docx = docx.image(Image::new(logo()));
                    }
                    _ => {
                        lines.push("\u{c}".to_owned());
                        docx = docx.page_break();
                    }
                }
            }
            let file = docx
                .build()
                .unwrap_or_else(|error| panic!("round {round}: {error:?}"));
            assert_eq!(file.text().unwrap(), lines.join("\n"), "round {round}");
            consistent(&docx);
        }
    }
}
