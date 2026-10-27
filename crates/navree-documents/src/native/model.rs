//! The document model of the native engine: [`PdfDocument`] and its
//! blocks, built in code or converted from an HTML template.

use std::sync::Arc;

use bytes::Bytes;

use crate::print::{Margins, Paper, PrintOptions};
use crate::render::is_rtl;

/// An sRGB color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red, 0 to 255.
    pub r: u8,
    /// Green, 0 to 255.
    pub g: u8,
    /// Blue, 0 to 255.
    pub b: u8,
}

impl Color {
    /// Black.
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    /// White.
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    /// The color with these components.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// A hexadecimal color (`#1a5fb4`, `1A5FB4`, `#fff`), or `None` when
    /// `hex` is not one.
    pub fn hex(hex: &str) -> Option<Color> {
        let hex = hex.trim().trim_start_matches('#');
        let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
        match hex.len() {
            3 => Some(Color::rgb(
                digit(0)? * 17,
                digit(1)? * 17,
                digit(2)? * 17,
            )),
            6 => {
                let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
                Some(Color::rgb(byte(0)?, byte(2)?, byte(4)?))
            }
            _ => None,
        }
    }
}

/// A hexadecimal color (`"C00000"`, `"#1a5fb4"`), as [`Color::hex`];
/// black when the text is not one.
impl From<&str> for Color {
    fn from(hex: &str) -> Self {
        Color::hex(hex).unwrap_or(Color::BLACK)
    }
}

impl From<(u8, u8, u8)> for Color {
    fn from((r, g, b): (u8, u8, u8)) -> Self {
        Color::rgb(r, g, b)
    }
}

/// Where a paragraph's lines sit. `Start` and `End` follow the text
/// direction (right and left in right-to-left text); `Left` and `Right`
/// do not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// The start of the line (left in left-to-right text).
    #[default]
    Start,
    /// Centered.
    Center,
    /// The end of the line (right in left-to-right text).
    End,
    /// Both edges: spaces stretch, except on a paragraph's last line.
    Justify,
    /// The left edge, whatever the direction.
    Left,
    /// The right edge, whatever the direction.
    Right,
}

/// The direction text reads in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Left to right.
    Ltr,
    /// Right to left (Arabic, Hebrew, Persian, Urdu).
    Rtl,
}

/// Where a cell's content sits vertically.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VAlign {
    /// At the top (the default).
    #[default]
    Top,
    /// In the middle.
    Middle,
    /// At the bottom.
    Bottom,
}

/// The text style a run, a paragraph or a document sets; what it leaves
/// out comes from around it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TextStyle {
    /// Font families, in order of preference.
    pub(crate) font: Option<Arc<[String]>>,
    /// Size in points.
    pub(crate) size: Option<f32>,
    pub(crate) bold: Option<bool>,
    pub(crate) italic: Option<bool>,
    pub(crate) color: Option<Color>,
    pub(crate) underline: Option<bool>,
    pub(crate) strike: Option<bool>,
    /// Line height as a multiple of the size.
    pub(crate) line_height: Option<f32>,
}

impl TextStyle {
    /// `self` over `base`: what `self` sets wins.
    pub(crate) fn over(&self, base: &TextStyle) -> TextStyle {
        TextStyle {
            font: self.font.clone().or_else(|| base.font.clone()),
            size: self.size.or(base.size),
            bold: self.bold.or(base.bold),
            italic: self.italic.or(base.italic),
            color: self.color.or(base.color),
            underline: self.underline.or(base.underline),
            strike: self.strike.or(base.strike),
            line_height: self.line_height.or(base.line_height),
        }
    }
}

/// What a run holds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RunKind {
    Text(String),
    /// A forced line break (`<br>`).
    LineBreak,
    /// The page's number (in a header or footer).
    PageNumber,
    /// The number of pages (in a header or footer).
    PageCount,
    /// The document's title (in a header or footer).
    Title,
    /// The render's date, `YYYY-MM-DD` (in a header or footer).
    Date,
}

/// A run of text with one format, inside a [`Paragraph`].
///
/// ```
/// use navree_documents::native::{Color, Paragraph, Run};
///
/// let total = Paragraph::new()
///     .text("Total: ")
///     .run(Run::new("1,234.56 EUR").bold().color(Color::rgb(0x1a, 0x5f, 0xb4)));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub(crate) kind: RunKind,
    pub(crate) style: TextStyle,
    pub(crate) link: Option<String>,
}

impl Run {
    /// A run of `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self::of(RunKind::Text(text.into()))
    }

    fn of(kind: RunKind) -> Self {
        Self {
            kind,
            style: TextStyle::default(),
            link: None,
        }
    }

    /// A line break.
    pub fn line_break() -> Self {
        Self::of(RunKind::LineBreak)
    }

    /// The number of the page it is printed on: for headers and footers
    /// (elsewhere it prints nothing).
    pub fn page_number() -> Self {
        Self::of(RunKind::PageNumber)
    }

    /// The number of pages: for headers and footers.
    pub fn page_count() -> Self {
        Self::of(RunKind::PageCount)
    }

    /// The document's title: for headers and footers.
    pub fn title() -> Self {
        Self::of(RunKind::Title)
    }

    /// The date of the render (`2026-10-27`): for headers and footers.
    pub fn date() -> Self {
        Self::of(RunKind::Date)
    }

    /// Bold.
    pub fn bold(mut self) -> Self {
        self.style.bold = Some(true);
        self
    }

    /// Italic.
    pub fn italic(mut self) -> Self {
        self.style.italic = Some(true);
        self
    }

    /// Underlined.
    pub fn underline(mut self) -> Self {
        self.style.underline = Some(true);
        self
    }

    /// Struck through.
    pub fn strike(mut self) -> Self {
        self.style.strike = Some(true);
        self
    }

    /// The size in points.
    pub fn size(mut self, points: f32) -> Self {
        self.style.size = Some(points);
        self
    }

    /// The color: a [`Color`] or a hexadecimal string (`"C00000"`).
    pub fn color(mut self, color: impl Into<Color>) -> Self {
        self.style.color = Some(color.into());
        self
    }

    /// The font family (`"Noto Serif"`); characters it lacks fall back
    /// through the bundled families.
    pub fn font(mut self, family: impl Into<String>) -> Self {
        self.style.font = Some(Arc::from(vec![family.into()]));
        self
    }

    /// Makes the run a link to `url`, printed in the link color and
    /// underlined unless the run sets its own.
    pub fn link(mut self, url: impl Into<String>) -> Self {
        self.link = Some(url.into());
        self
    }
}

/// A paragraph: runs of text, aligned.
///
/// ```
/// use navree_documents::native::{Align, Paragraph};
///
/// let signed = Paragraph::new()
///     .text("Signed off by ")
///     .bold("the audit team")
///     .text(".")
///     .align(Align::End);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paragraph {
    pub(crate) runs: Vec<Run>,
    pub(crate) align: Option<Align>,
    pub(crate) direction: Option<Direction>,
    pub(crate) style: TextStyle,
    /// Keep spaces and line breaks as written (`<pre>`).
    pub(crate) preformatted: bool,
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

    /// Adds `text` as a link to `url`.
    pub fn link(self, text: impl Into<String>, url: impl Into<String>) -> Self {
        self.run(Run::new(text).link(url))
    }

    /// Adds a line break.
    pub fn line_break(self) -> Self {
        self.run(Run::line_break())
    }

    /// Adds the page's number (headers and footers).
    pub fn page_number(self) -> Self {
        self.run(Run::page_number())
    }

    /// Adds the number of pages (headers and footers).
    pub fn page_count(self) -> Self {
        self.run(Run::page_count())
    }

    /// Adds a run.
    pub fn run(mut self, run: Run) -> Self {
        self.runs.push(run);
        self
    }

    /// Aligns the lines.
    pub fn align(mut self, align: Align) -> Self {
        self.align = Some(align);
        self
    }

    /// The direction the paragraph reads in; by default the document's.
    pub fn direction(mut self, direction: Direction) -> Self {
        self.direction = Some(direction);
        self
    }

    /// The size of its text in points.
    pub fn size(mut self, points: f32) -> Self {
        self.style.size = Some(points);
        self
    }

    /// The color of its text.
    pub fn color(mut self, color: impl Into<Color>) -> Self {
        self.style.color = Some(color.into());
        self
    }

    /// The font family of its text.
    pub fn font(mut self, family: impl Into<String>) -> Self {
        self.style.font = Some(Arc::from(vec![family.into()]));
        self
    }

    /// Bold text throughout.
    pub fn strong(mut self) -> Self {
        self.style.bold = Some(true);
        self
    }

    /// The line height as a multiple of the text size (default 1.4).
    pub fn line_height(mut self, factor: f32) -> Self {
        self.style.line_height = Some(factor);
        self
    }

    /// The runs.
    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    /// Whether the paragraph has nothing to print.
    pub(crate) fn is_empty(&self) -> bool {
        self.runs.iter().all(|run| match &run.kind {
            RunKind::Text(text) => {
                if self.preformatted {
                    text.is_empty()
                } else {
                    text.trim().is_empty()
                }
            }
            _ => false,
        })
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

/// A bullet or numbered list; items can hold lists of their own.
///
/// ```
/// use navree_documents::native::List;
///
/// let steps = List::numbered()
///     .item("Rotate the keys")
///     .item_with("Restore a backup", List::bullets().item("Database").item("Files"));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub(crate) kind: ListKind,
    pub(crate) start: u32,
    pub(crate) items: Vec<ListItem>,
}

/// How list items are marked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    /// Bullets (•, then ◦ and ▪ when nested).
    Bullets,
    /// 1. 2. 3.
    Decimal,
    /// a. b. c.
    LowerAlpha,
    /// A. B. C.
    UpperAlpha,
    /// i. ii. iii.
    LowerRoman,
    /// I. II. III.
    UpperRoman,
}

/// One item: its blocks (paragraphs, nested lists).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ListItem {
    pub(crate) blocks: Vec<Block>,
}

impl List {
    /// A bullet list.
    pub fn bullets() -> Self {
        Self::new(ListKind::Bullets)
    }

    /// A numbered list (1. 2. 3.).
    pub fn numbered() -> Self {
        Self::new(ListKind::Decimal)
    }

    /// A list marked by `kind`.
    pub fn new(kind: ListKind) -> Self {
        Self {
            kind,
            start: 1,
            items: Vec::new(),
        }
    }

    /// The number of the first item (default 1).
    pub fn start(mut self, start: u32) -> Self {
        self.start = start;
        self
    }

    /// Adds an item.
    pub fn item(mut self, item: impl Into<Paragraph>) -> Self {
        self.items.push(ListItem {
            blocks: vec![Block::Paragraph(item.into())],
        });
        self
    }

    /// Adds an item followed by a nested list.
    pub fn item_with(mut self, item: impl Into<Paragraph>, nested: List) -> Self {
        self.items.push(ListItem {
            blocks: vec![Block::Paragraph(item.into()), Block::List(nested)],
        });
        self
    }

    pub(crate) fn item_blocks(mut self, blocks: Vec<Block>) -> Self {
        self.items.push(ListItem { blocks });
        self
    }
}

/// Which lines a table draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Borders {
    /// A line under the header and between rows (the default).
    #[default]
    Rows,
    /// Every cell's edges.
    Grid,
    /// None.
    None,
}

/// A table cell: paragraphs (and images), aligned.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    pub(crate) blocks: Vec<Block>,
    pub(crate) colspan: usize,
    pub(crate) align: Option<Align>,
    pub(crate) valign: VAlign,
    pub(crate) background: Option<Color>,
    pub(crate) style: TextStyle,
    /// The column's width as a percentage of the table, when the cell
    /// sets one (`width="30%"`).
    pub(crate) width_percent: Option<f32>,
}

impl Cell {
    /// A cell holding one paragraph.
    pub fn new(content: impl Into<Paragraph>) -> Self {
        Self::blocks(vec![Block::Paragraph(content.into())])
    }

    /// A cell holding several blocks (paragraphs, images, lists).
    pub fn blocks(blocks: Vec<Block>) -> Self {
        Self {
            blocks,
            colspan: 1,
            ..Self::default()
        }
    }

    /// Spans `columns` columns.
    pub fn colspan(mut self, columns: usize) -> Self {
        self.colspan = columns.max(1);
        self
    }

    /// Aligns its lines.
    pub fn align(mut self, align: Align) -> Self {
        self.align = Some(align);
        self
    }

    /// Places its content vertically.
    pub fn valign(mut self, valign: VAlign) -> Self {
        self.valign = valign;
        self
    }

    /// Fills it with `color`.
    pub fn background(mut self, color: impl Into<Color>) -> Self {
        self.background = Some(color.into());
        self
    }

    /// Bold text.
    pub fn bold(mut self) -> Self {
        self.style.bold = Some(true);
        self
    }
}

impl<P: Into<Paragraph>> From<P> for Cell {
    fn from(content: P) -> Self {
        Cell::new(content)
    }
}

/// A table row.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Row {
    pub(crate) cells: Vec<Cell>,
    pub(crate) background: Option<Color>,
    pub(crate) style: TextStyle,
}

/// A table: header rows (bold, repeated on every page the table continues
/// on) and body rows, with columns sized to their content or by
/// [`widths`](Table::widths).
///
/// ```
/// use navree_documents::native::{Align, Cell, Table};
///
/// let lines = Table::new()
///     .header(["Item", "Qty", "Amount"])
///     .row(["Consulting", "10", "1,000.00"])
///     .row([Cell::new("Total").colspan(2).bold(), Cell::new("1,000.00").align(Align::End)])
///     .widths([60.0, 15.0, 25.0]);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub(crate) header: Vec<Row>,
    pub(crate) rows: Vec<Row>,
    /// Relative column widths.
    pub(crate) widths: Option<Vec<f32>>,
    pub(crate) borders: Borders,
    /// Cell padding in points: vertical, horizontal.
    pub(crate) padding: (f32, f32),
    pub(crate) header_background: Option<Color>,
    pub(crate) style: TextStyle,
}

impl Default for Table {
    fn default() -> Self {
        Self {
            header: Vec::new(),
            rows: Vec::new(),
            widths: None,
            borders: Borders::Rows,
            padding: (4.0, 6.0),
            header_background: Some(Color::rgb(0xF2, 0xF4, 0xF7)),
            style: TextStyle::default(),
        }
    }
}

impl Table {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a header row.
    pub fn header<I, C>(mut self, cells: I) -> Self
    where
        I: IntoIterator<Item = C>,
        C: Into<Cell>,
    {
        self.header.push(Row {
            cells: cells.into_iter().map(Into::into).collect(),
            ..Row::default()
        });
        self
    }

    /// Adds a body row.
    pub fn row<I, C>(mut self, cells: I) -> Self
    where
        I: IntoIterator<Item = C>,
        C: Into<Cell>,
    {
        self.rows.push(Row {
            cells: cells.into_iter().map(Into::into).collect(),
            ..Row::default()
        });
        self
    }

    /// The columns' relative widths (`[2.0, 1.0]`: the first column twice
    /// as wide as the second). By default the columns share the width by
    /// their content.
    pub fn widths(mut self, widths: impl IntoIterator<Item = f32>) -> Self {
        self.widths = Some(widths.into_iter().map(|w| w.max(0.0)).collect());
        self
    }

    /// Which lines to draw (default [`Borders::Rows`]).
    pub fn borders(mut self, borders: Borders) -> Self {
        self.borders = borders;
        self
    }

    /// The space between a cell's edges and its text, in millimeters
    /// (default 1.4 mm above and below, 2.1 mm left and right).
    pub fn padding_mm(mut self, vertical: f32, horizontal: f32) -> Self {
        self.padding = (mm_to_pt(vertical), mm_to_pt(horizontal));
        self
    }

    /// The header rows' fill (default a light gray), or `None`.
    pub fn header_background(mut self, color: Option<Color>) -> Self {
        self.header_background = color;
        self
    }

    /// The size of the table's text in points.
    pub fn size(mut self, points: f32) -> Self {
        self.style.size = Some(points);
        self
    }

    pub(crate) fn push_row(&mut self, row: Row, header: bool) {
        if header {
            self.header.push(row);
        } else {
            self.rows.push(row);
        }
    }
}

/// A PNG, JPEG, GIF or WebP image.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub(crate) bytes: Bytes,
    /// Width and height in points.
    pub(crate) width: Option<f32>,
    pub(crate) height: Option<f32>,
    /// A width as a percentage of the available width.
    pub(crate) width_percent: Option<f32>,
    pub(crate) align: Option<Align>,
    pub(crate) alt: Option<String>,
    /// Where it came from, for error messages (`images/logo.png`).
    pub(crate) source: Option<String>,
}

impl Image {
    /// The image in `bytes`, at 96 dots per inch unless
    /// [`width_mm`](Image::width_mm) or [`height_mm`](Image::height_mm)
    /// size it, and never wider than the space it is in.
    pub fn new(bytes: impl Into<Bytes>) -> Self {
        Self {
            bytes: bytes.into(),
            width: None,
            height: None,
            width_percent: None,
            align: None,
            alt: None,
            source: None,
        }
    }

    /// The printed width in millimeters; without a height, the height
    /// keeps the image's proportions.
    pub fn width_mm(mut self, width: f32) -> Self {
        self.width = Some(mm_to_pt(width));
        self
    }

    /// The printed height in millimeters.
    pub fn height_mm(mut self, height: f32) -> Self {
        self.height = Some(mm_to_pt(height));
        self
    }

    /// Where it sits on its line (default the start).
    pub fn align(mut self, align: Align) -> Self {
        self.align = Some(align);
        self
    }

    /// The text that describes it to screen readers.
    pub fn alt(mut self, alt: impl Into<String>) -> Self {
        self.alt = Some(alt.into());
        self
    }
}

/// One block of a document.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// A paragraph.
    Paragraph(Paragraph),
    /// A heading, level 1 to 6.
    Heading(u8, Paragraph),
    /// A list.
    List(List),
    /// A table.
    Table(Table),
    /// An image on its own line.
    Image(Image),
    /// A horizontal rule.
    Rule,
    /// Vertical space, in points.
    Space(f32),
    /// The next block starts a page.
    PageBreak,
}

impl From<Paragraph> for Block {
    fn from(paragraph: Paragraph) -> Self {
        Block::Paragraph(paragraph)
    }
}

impl From<Table> for Block {
    fn from(table: Table) -> Self {
        Block::Table(table)
    }
}

impl From<Image> for Block {
    fn from(image: Image) -> Self {
        Block::Image(image)
    }
}

impl From<List> for Block {
    fn from(list: List) -> Self {
        Block::List(list)
    }
}

impl From<&str> for Block {
    fn from(text: &str) -> Self {
        Block::Paragraph(text.into())
    }
}

/// Page size, orientation and margins, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PageSetup {
    pub(crate) paper: Paper,
    pub(crate) landscape: bool,
    pub(crate) margins: Margins,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self {
            paper: Paper::A4,
            landscape: false,
            margins: Margins::all(20.0),
        }
    }
}

impl PageSetup {
    /// Width and height in points.
    pub(crate) fn size(&self) -> (f32, f32) {
        let (w, h) = self.paper.size_mm();
        let (w, h) = (mm_to_pt(w as f32), mm_to_pt(h as f32));
        if self.landscape { (h, w) } else { (w, h) }
    }
}

/// A PDF document laid out by the native engine, block by block:
/// headings, paragraphs with formatted runs and links, bullet and numbered
/// lists, tables whose header rows repeat on every page, images, rules and
/// page breaks, with a header and a footer that can number pages.
///
/// ```
/// use navree_documents::PdfDocument;
/// use navree_documents::native::{Align, Paragraph, Table};
///
/// # fn main() -> Result<(), navree_documents::DocumentsError> {
/// let pdf = PdfDocument::new()
///     .title("Quarterly audit report")
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
/// let text = pdf.text()?;
/// assert!(text.contains("Prepared for Acme Ltd."));
/// assert!(text.contains("Multi-factor sign-in Passed"));
/// assert!(text.contains("Page 1 of 1"));
/// # Ok(())
/// # }
/// ```
///
/// **Languages.** [`locale`](PdfDocument::locale) sets the language (in
/// the metadata, and for shaping) and, for Arabic, Hebrew, Persian and
/// Urdu, makes paragraphs read right to left. Text in any script the
/// bundled fonts cover prints without setting a font: each character
/// falls back to a family that has it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PdfDocument {
    pub(crate) title: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) lang: Option<String>,
    pub(crate) direction: Option<Direction>,
    pub(crate) page: PageSetup,
    pub(crate) style: TextStyle,
    pub(crate) header: Vec<Block>,
    pub(crate) footer: Vec<Block>,
    pub(crate) blocks: Vec<Block>,
}

impl PdfDocument {
    /// An empty A4 document with 20 mm margins, in 11-point Noto Sans.
    pub fn new() -> Self {
        Self::default()
    }

    /// The title, in the metadata (and for `Run::title`).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The author, in the metadata.
    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// The document's language (`ar`, `en-GB`); right-to-left languages
    /// make paragraphs read right to left.
    pub fn locale(mut self, locale: impl Into<String>) -> Self {
        let locale = locale.into();
        self.direction = Some(if is_rtl(&locale) {
            Direction::Rtl
        } else {
            Direction::Ltr
        });
        self.lang = Some(locale);
        self
    }

    /// The direction paragraphs read in, over the locale's.
    pub fn direction(mut self, direction: Direction) -> Self {
        self.direction = Some(direction);
        self
    }

    /// The font family of the text (default `Noto Sans`).
    pub fn font(mut self, family: impl Into<String>) -> Self {
        self.style.font = Some(Arc::from(vec![family.into()]));
        self
    }

    /// The size of body text in points (default 11).
    pub fn font_size(mut self, points: f32) -> Self {
        self.style.size = Some(points);
        self
    }

    /// The paper (default A4).
    pub fn paper(mut self, paper: Paper) -> Self {
        self.page.paper = paper;
        self
    }

    /// Landscape pages.
    pub fn landscape(mut self) -> Self {
        self.page.landscape = true;
        self
    }

    /// The margins in millimeters (default 20 mm).
    pub fn margins(mut self, margins: Margins) -> Self {
        self.page.margins = margins;
        self
    }

    /// Paper, orientation and margins from `print`.
    pub fn print(mut self, print: &PrintOptions) -> Self {
        self.page = PageSetup {
            paper: print.paper,
            landscape: print.landscape,
            margins: print.margin,
        };
        self
    }

    /// Adds a block to the header printed at the top of every page.
    pub fn header(mut self, block: impl Into<Block>) -> Self {
        self.header.push(block.into());
        self
    }

    /// Adds a block to the footer printed at the bottom of every page.
    pub fn footer(mut self, block: impl Into<Block>) -> Self {
        self.footer.push(block.into());
        self
    }

    /// A centered "Page 2 of 5" footer.
    pub fn footer_page_numbers(self) -> Self {
        self.footer(
            Paragraph::new()
                .text("Page ")
                .page_number()
                .text(" of ")
                .page_count()
                .align(Align::Center),
        )
    }

    /// Adds a heading, level 1 to 6.
    pub fn heading(mut self, level: u8, text: impl Into<Paragraph>) -> Self {
        self.blocks.push(Block::Heading(level.clamp(1, 6), text.into()));
        self
    }

    /// Adds a paragraph.
    pub fn paragraph(mut self, paragraph: impl Into<Paragraph>) -> Self {
        self.blocks.push(Block::Paragraph(paragraph.into()));
        self
    }

    /// Adds a bullet list.
    pub fn bullets<I, P>(self, items: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<Paragraph>,
    {
        let list = items.into_iter().fold(List::bullets(), List::item);
        self.list(list)
    }

    /// Adds a numbered list.
    pub fn numbered<I, P>(self, items: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<Paragraph>,
    {
        let list = items.into_iter().fold(List::numbered(), List::item);
        self.list(list)
    }

    /// Adds a list.
    pub fn list(mut self, list: List) -> Self {
        self.blocks.push(Block::List(list));
        self
    }

    /// Adds a table.
    pub fn table(mut self, table: Table) -> Self {
        self.blocks.push(Block::Table(table));
        self
    }

    /// Adds an image on its own line.
    pub fn image(mut self, image: Image) -> Self {
        self.blocks.push(Block::Image(image));
        self
    }

    /// Adds a horizontal rule.
    pub fn rule(mut self) -> Self {
        self.blocks.push(Block::Rule);
        self
    }

    /// Adds vertical space, in millimeters.
    pub fn space_mm(mut self, height: f32) -> Self {
        self.blocks.push(Block::Space(mm_to_pt(height)));
        self
    }

    /// Starts a new page.
    pub fn page_break(mut self) -> Self {
        self.blocks.push(Block::PageBreak);
        self
    }

    /// Adds any block.
    pub fn block(mut self, block: impl Into<Block>) -> Self {
        self.blocks.push(block.into());
        self
    }

    /// The blocks, in order.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }
}

/// Points per millimeter.
pub(crate) const PT_PER_MM: f32 = 72.0 / 25.4;

pub(crate) fn mm_to_pt(mm: f32) -> f32 {
    mm * PT_PER_MM
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_parse_from_hex() {
        assert_eq!(Color::hex("#1a5fb4"), Some(Color::rgb(0x1a, 0x5f, 0xb4)));
        assert_eq!(Color::hex("C00000"), Some(Color::rgb(0xc0, 0, 0)));
        assert_eq!(Color::hex("#fff"), Some(Color::WHITE));
        assert_eq!(Color::hex("#ggg"), None);
        assert_eq!(Color::hex("12345"), None);
        assert_eq!(Color::from("nonsense"), Color::BLACK);
    }

    #[test]
    fn styles_inherit_what_they_leave_out() {
        let base = TextStyle {
            size: Some(11.0),
            bold: Some(false),
            color: Some(Color::BLACK),
            ..TextStyle::default()
        };
        let own = TextStyle {
            bold: Some(true),
            ..TextStyle::default()
        };
        let style = own.over(&base);
        assert_eq!(style.size, Some(11.0));
        assert_eq!(style.bold, Some(true));
        assert_eq!(style.color, Some(Color::BLACK));
    }

    #[test]
    fn the_locale_sets_the_direction() {
        assert_eq!(
            PdfDocument::new().locale("ar-EG").direction,
            Some(Direction::Rtl)
        );
        assert_eq!(
            PdfDocument::new().locale("hi").direction,
            Some(Direction::Ltr)
        );
        let landscape = PdfDocument::new().paper(Paper::Letter).landscape();
        let (w, h) = landscape.page.size();
        assert!((w - 792.0).abs() < 0.1 && (h - 612.0).abs() < 0.1, "{w} {h}");
    }

    #[test]
    fn empty_paragraphs_are_detected() {
        assert!(Paragraph::new().text("  ").is_empty());
        assert!(!Paragraph::new().line_break().is_empty());
        assert!(!Paragraph::from("x").is_empty());
    }
}
