//! Word documents (DOCX): [`Docx`], built block by block, and
//! [`DocxFile`], the result. A file is read back (`DocxFile::from_bytes`,
//! `DocxFile::text`) within [`DocxLimits`].

use bytes::Bytes;
use navree_http::{IntoResponse, Response};

use crate::DocumentsError;
use crate::download::{DOCX_CONTENT_TYPE, Download};
use crate::print::{Margins, Paper};
use crate::render::is_rtl;

mod read;
mod write;

pub use read::DocxLimits;

/// Twips (1/20 of a point) per millimeter.
const TWIPS_PER_MM: f64 = 1440.0 / 25.4;
/// EMUs per millimeter (images are sized in EMUs).
const EMU_PER_MM: f64 = 36_000.0;

/// A Word document (DOCX), built block by block: headings, paragraphs
/// with bold and italic runs, bullet and numbered lists, tables, images
/// and page breaks, on A4 or any paper, with a header and a footer that
/// can number pages.
///
/// ```
/// use navree_documents::Docx;
/// use navree_documents::docx::{Align, Paragraph, Table};
///
/// # fn main() -> Result<(), navree_documents::DocumentsError> {
/// let file = Docx::new()
///     .heading(1, "Quarterly audit report")
///     .paragraph(Paragraph::new().text("Prepared for ").bold("Acme Ltd").text("."))
///     .bullets(["Every control was tested", "Two findings are open"])
///     .table(
///         Table::new()
///             .header(["Control", "Status"])
///             .row(["Multi-factor sign-in", "Passed"])
///             .row(["Backups restored", "Open"]),
///     )
///     .paragraph(Paragraph::from("Signed off by the audit team.").align(Align::End))
///     .footer_page_numbers()
///     .build()?;
/// let text = file.text()?;
/// assert!(text.contains("Prepared for Acme Ltd."));
/// assert!(text.contains("Multi-factor sign-in\tPassed"));
/// # Ok(())
/// # }
/// ```
///
/// **Languages.** With [`locale`](Docx::locale) set to a right-to-left
/// language (Arabic, Hebrew, Persian, Urdu), every paragraph and table
/// reads right to left. [`font`](Docx::font) sets the font for Latin and
/// for complex scripts; Word substitutes a font the reader's computer has
/// when it is missing.
#[derive(Clone, Debug, Default)]
pub struct Docx {
    locale: Option<String>,
    font: Option<String>,
    paper: Paper,
    landscape: bool,
    margins: Option<Margins>,
    title: Option<String>,
    author: Option<String>,
    header: Option<String>,
    footer: Option<FooterContent>,
    blocks: Vec<Block>,
}

/// What the footer holds.
#[derive(Clone, Debug)]
enum FooterContent {
    Text(String),
    PageNumbers,
}

#[derive(Clone, Debug)]
enum Block {
    Heading(u8, Paragraph),
    Paragraph(Paragraph),
    List {
        ordered: bool,
        items: Vec<Paragraph>,
    },
    Table(Table),
    Image(Image),
    PageBreak,
}

impl Docx {
    /// An empty A4 document with 25.4 mm margins.
    pub fn new() -> Self {
        Self::default()
    }

    /// The document's language (`ar`, `en-GB`): right-to-left languages
    /// make every paragraph and table read right to left.
    pub fn locale(mut self, locale: impl Into<String>) -> Self {
        self.locale = Some(locale.into());
        self
    }

    /// The font family of all text (Latin and complex scripts).
    pub fn font(mut self, family: impl Into<String>) -> Self {
        self.font = Some(family.into());
        self
    }

    /// The paper size (default A4).
    pub fn paper(mut self, paper: Paper) -> Self {
        self.paper = paper;
        self
    }

    /// Landscape pages.
    pub fn landscape(mut self) -> Self {
        self.landscape = true;
        self
    }

    /// The page margins in millimeters (default 25.4 on every side).
    pub fn margins(mut self, margins: Margins) -> Self {
        self.margins = Some(margins);
        self
    }

    /// The title in the document's properties (File > Info in Word).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The author in the document's properties.
    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Text at the top of every page.
    pub fn header(mut self, text: impl Into<String>) -> Self {
        self.header = Some(text.into());
        self
    }

    /// Text at the bottom of every page.
    pub fn footer(mut self, text: impl Into<String>) -> Self {
        self.footer = Some(FooterContent::Text(text.into()));
        self
    }

    /// "Page N of M", centered at the bottom of every page (Word fills the
    /// numbers in).
    pub fn footer_page_numbers(mut self) -> Self {
        self.footer = Some(FooterContent::PageNumbers);
        self
    }

    /// A heading of `level` 1 to 3 (Word's "Heading 1" to "Heading 3"
    /// styles, so they appear in its navigation pane and tables of
    /// contents). Levels outside 1..=3 are clamped.
    pub fn heading(mut self, level: u8, text: impl Into<Paragraph>) -> Self {
        self.blocks
            .push(Block::Heading(level.clamp(1, 3), text.into()));
        self
    }

    /// A paragraph: a string, or a [`Paragraph`] with formatted runs.
    pub fn paragraph(mut self, paragraph: impl Into<Paragraph>) -> Self {
        self.blocks.push(Block::Paragraph(paragraph.into()));
        self
    }

    /// A bulleted list.
    pub fn bullets<I, P>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<Paragraph>,
    {
        self.blocks.push(Block::List {
            ordered: false,
            items: items.into_iter().map(Into::into).collect(),
        });
        self
    }

    /// A numbered list (1., 2., ...), numbered from 1.
    pub fn numbered<I, P>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<Paragraph>,
    {
        self.blocks.push(Block::List {
            ordered: true,
            items: items.into_iter().map(Into::into).collect(),
        });
        self
    }

    /// A table.
    pub fn table(mut self, table: Table) -> Self {
        self.blocks.push(Block::Table(table));
        self
    }

    /// An image (PNG or JPEG) in its own paragraph.
    pub fn image(mut self, image: Image) -> Self {
        self.blocks.push(Block::Image(image));
        self
    }

    /// Starts a new page.
    pub fn page_break(mut self) -> Self {
        self.blocks.push(Block::PageBreak);
        self
    }

    fn rtl(&self) -> bool {
        self.locale.as_deref().is_some_and(is_rtl)
    }

    /// Writes the document.
    pub fn build(&self) -> Result<DocxFile, DocumentsError> {
        Ok(DocxFile(Bytes::from(write::build(self)?)))
    }
}

fn twips(mm: f64) -> i32 {
    (mm * TWIPS_PER_MM).round() as i32
}

fn emu(mm: f64) -> u32 {
    (mm * EMU_PER_MM).round().max(1.0) as u32
}

/// Where a paragraph's lines sit. `Start` and `End` follow the text
/// direction (right and left in a right-to-left document).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// The start of the line (left in left-to-right text).
    Start,
    /// Centered.
    Center,
    /// The end of the line (right in left-to-right text).
    End,
    /// Both edges.
    Justify,
}

/// A run of text with one format.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    /// The text.
    pub text: String,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
    /// The size in points (default: the style's).
    pub size: Option<f32>,
    /// The color as hex RGB (`C00000`).
    pub color: Option<String>,
}

impl Run {
    /// Plain text.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// Bold.
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// Italic.
    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    /// Underlined.
    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    /// The size in points.
    pub fn size(mut self, points: f32) -> Self {
        self.size = Some(points);
        self
    }

    /// The color as hex RGB (`C00000`, with or without `#`).
    pub fn color(mut self, hex: impl Into<String>) -> Self {
        self.color = Some(hex.into().trim_start_matches('#').to_owned());
        self
    }
}

/// A paragraph: runs of text, aligned.
///
/// ```
/// use navree_documents::docx::{Align, Paragraph, Run};
///
/// let paragraph = Paragraph::new()
///     .text("Status: ")
///     .run(Run::new("Open").bold().color("C00000"))
///     .align(Align::Center);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paragraph {
    runs: Vec<Run>,
    align: Option<Align>,
}

impl Paragraph {
    /// An empty paragraph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds plain text.
    pub fn text(self, text: impl Into<String>) -> Self {
        self.run(Run::new(text))
    }

    /// Adds bold text.
    pub fn bold(self, text: impl Into<String>) -> Self {
        self.run(Run::new(text).bold())
    }

    /// Adds italic text.
    pub fn italic(self, text: impl Into<String>) -> Self {
        self.run(Run::new(text).italic())
    }

    /// Adds a run.
    pub fn run(mut self, run: Run) -> Self {
        self.runs.push(run);
        self
    }

    /// Aligns the paragraph.
    pub fn align(mut self, align: Align) -> Self {
        self.align = Some(align);
        self
    }

    /// The runs.
    pub fn runs(&self) -> &[Run] {
        &self.runs
    }
}

impl From<&str> for Paragraph {
    fn from(text: &str) -> Self {
        Paragraph::new().text(text)
    }
}

impl From<String> for Paragraph {
    fn from(text: String) -> Self {
        Paragraph::new().text(text)
    }
}

impl From<&String> for Paragraph {
    fn from(text: &String) -> Self {
        Paragraph::new().text(text.clone())
    }
}

impl From<Run> for Paragraph {
    fn from(run: Run) -> Self {
        Paragraph::new().run(run)
    }
}

/// A table: an optional header row (bold, repeated on every page) and rows
/// of cells.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    header: Option<Vec<Paragraph>>,
    rows: Vec<Vec<Paragraph>>,
}

impl Table {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// The header row.
    pub fn header<I, P>(mut self, cells: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<Paragraph>,
    {
        self.header = Some(cells.into_iter().map(Into::into).collect());
        self
    }

    /// Adds a row.
    pub fn row<I, P>(mut self, cells: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<Paragraph>,
    {
        self.rows.push(cells.into_iter().map(Into::into).collect());
        self
    }
}

/// A PNG or JPEG image for a [`Docx`].
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    bytes: Bytes,
    width_mm: Option<f64>,
    align: Option<Align>,
}

impl Image {
    /// The image in `bytes` (PNG or JPEG), at 96 dots per inch unless
    /// [`width_mm`](Image::width_mm) sets its width.
    pub fn new(bytes: impl Into<Bytes>) -> Self {
        Self {
            bytes: bytes.into(),
            width_mm: None,
            align: None,
        }
    }

    /// The printed width in millimeters; the height keeps the image's
    /// proportions.
    pub fn width_mm(mut self, width: f64) -> Self {
        self.width_mm = Some(width);
        self
    }

    /// Aligns the image's paragraph.
    pub fn align(mut self, align: Align) -> Self {
        self.align = Some(align);
        self
    }

    /// Width and height in pixels, read from the PNG or JPEG header.
    fn size(&self) -> Result<(u32, u32), DocumentsError> {
        image_size(&self.bytes)
            .ok_or_else(|| DocumentsError::Docx("an image must be a PNG or a JPEG file".into()))
    }
}

/// Width and height of a PNG or JPEG image.
pub(crate) fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 && &bytes[12..16] == b"IHDR" {
        let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return (width > 0 && height > 0).then_some((width, height));
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        let mut i = 2;
        while i + 4 <= bytes.len() {
            if bytes[i] != 0xff {
                return None;
            }
            let marker = bytes[i + 1];
            if marker == 0xff {
                i += 1;
                continue;
            }
            let length = usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
            // Start-of-frame markers carry the size (not DHT, JPG or DAC).
            if matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
                if i + 9 > bytes.len() {
                    return None;
                }
                let height = u32::from(u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]));
                let width = u32::from(u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]));
                return (width > 0 && height > 0).then_some((width, height));
            }
            i += 2 + length;
        }
    }
    None
}

/// A built Word document: its bytes, readable back as text, and a
/// response (`DocxFile::download`).
#[derive(Clone, PartialEq, Eq)]
pub struct DocxFile(Bytes);

impl std::fmt::Debug for DocxFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DocxFile({} bytes)", self.0.len())
    }
}

impl DocxFile {
    /// A DOCX file from its bytes, checked: an error unless they read as a
    /// Word document within the default [`DocxLimits`].
    pub fn from_bytes(bytes: impl Into<Bytes>) -> Result<DocxFile, DocumentsError> {
        Self::from_bytes_within(bytes, &DocxLimits::default())
    }

    /// A DOCX file from its bytes, checked within `limits`: the same as
    /// [`from_bytes`](Self::from_bytes) with limits of your own.
    ///
    /// Bytes that are not a Word document that can be read (a damaged
    /// archive or XML, another kind of package, a DTD, two entries of one
    /// name) are [`DocumentsError::InvalidDocx`], a 400; a file that is over a
    /// limit is [`DocumentsError::TooLarge`], a 413.
    pub fn from_bytes_within(
        bytes: impl Into<Bytes>,
        limits: &DocxLimits,
    ) -> Result<DocxFile, DocumentsError> {
        let file = DocxFile(bytes.into());
        read::read_text(&file.0, limits, false)?;
        Ok(file)
    }

    /// The bytes.
    pub fn bytes(&self) -> &Bytes {
        &self.0
    }

    /// The bytes, by value.
    pub fn into_bytes(self) -> Bytes {
        self.0
    }

    /// The text of the document body: one line per paragraph, table cells
    /// separated by tabs, a row per line, `[image]` for an image and a form
    /// feed (`\u{c}`) for a page break. For tests and checks.
    ///
    /// The text of a hyperlink, of an inserted run and of a content control
    /// is part of its paragraph; deleted text, headers, footers and notes
    /// are not. The file is read within the default [`DocxLimits`].
    pub fn text(&self) -> Result<String, DocumentsError> {
        self.text_within(&DocxLimits::default())
    }

    /// [`text`](Self::text) within `limits`.
    pub fn text_within(&self, limits: &DocxLimits) -> Result<String, DocumentsError> {
        read::read_text(&self.0, limits, true)
    }

    /// The file as a response the browser saves as `filename`.
    pub fn download(self, filename: impl Into<String>) -> Download {
        Download::new(self.0, DOCX_CONTENT_TYPE, filename)
    }
}

/// The DOCX content type, saved as `document.docx`.
impl IntoResponse for DocxFile {
    fn into_response(self) -> Response {
        self.download("document.docx").into_response()
    }
}

impl From<DocxFile> for Bytes {
    fn from(file: DocxFile) -> Bytes {
        file.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_sizes_come_from_png_and_jpeg_headers() {
        let png = [
            0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13, b'I', b'H', b'D', b'R',
            0, 0, 1, 0, 0, 0, 0, 64, 8, 6, 0, 0, 0,
        ];
        assert_eq!(image_size(&png), Some((256, 64)));
        // SOI, an APP0 segment, then SOF0 with height 32 and width 48.
        let jpeg = [
            0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00,
            0x20, 0x00, 0x30, 0x03,
        ];
        assert_eq!(image_size(&jpeg), Some((48, 32)));
        assert_eq!(image_size(b"GIF89a"), None);
        assert_eq!(image_size(&[0xff, 0xd8, 0x00]), None);
    }

    #[test]
    fn units() {
        assert_eq!(twips(25.4), 1440);
        assert_eq!(emu(10.0), 360_000);
    }
}
