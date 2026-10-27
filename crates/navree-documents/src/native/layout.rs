//! Blocks to pages: every block becomes pieces that are not split (a line,
//! an image, a table row), pieces fill pages top to bottom, table headers
//! repeat, and headers and footers are laid out per page once the page
//! count is known.

use std::collections::HashMap;
use std::sync::Arc;

use krilla::image::Image as PdfImage;

use super::fonts::FontBook;
use super::model::{
    Align, Block, Borders, Cell, Color, Direction, Image, List, ListKind, Paragraph, PdfDocument,
    Row, RunKind, Table, TextStyle, VAlign, mm_to_pt,
};
use super::tags::{Furniture, Mark, NodeId, NodeKind, SlotId, StructTree};
use super::text::{GlyphRun, LINE_BREAK, Para, Shaper, Style};

/// Links, unless the run sets its own color.
pub(crate) const LINK_COLOR: Color = Color::rgb(0x1A, 0x5F, 0xB4);
/// Horizontal rules.
const RULE_COLOR: Color = Color::rgb(0xC8, 0xCC, 0xD2);
/// Table lines.
const BORDER_COLOR: Color = Color::rgb(0xD0, 0xD5, 0xDD);
/// The line under a table's header rows.
const HEADER_RULE_COLOR: Color = Color::rgb(0x98, 0xA2, 0xB3);
/// Header and footer text.
const FURNITURE_COLOR: Color = Color::rgb(0x55, 0x5B, 0x66);
/// Heading sizes as multiples of the body size, h1 to h6.
const HEADING_SCALE: [f32; 6] = [2.0, 1.55, 1.27, 1.09, 1.0, 0.91];
/// The space between the header (or footer) and the body, in points.
const FURNITURE_GAP: f32 = 4.0 * super::model::PT_PER_MM;

/// One thing drawn on a page, in points from the top-left corner.
#[derive(Clone, Debug)]
pub(crate) enum Draw {
    Text {
        x: f32,
        baseline: f32,
        run: Arc<GlyphRun>,
        mark: Mark,
    },
    Fill {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Color,
    },
    Stroke {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        width: f32,
        color: Color,
    },
    Image {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        image: PdfImage,
        mark: Mark,
    },
    Link {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        url: Arc<str>,
        slot: Option<SlotId>,
    },
}

impl Draw {
    fn translate(&mut self, dx: f32, dy: f32) {
        match self {
            Draw::Text { x, baseline, .. } => {
                *x += dx;
                *baseline += dy;
            }
            Draw::Fill { x, y, .. } | Draw::Image { x, y, .. } | Draw::Link { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Draw::Stroke { x1, y1, x2, y2, .. } => {
                *x1 += dx;
                *x2 += dx;
                *y1 += dy;
                *y2 += dy;
            }
        }
    }

    /// The same drawing as page furniture: no structure, no link target
    /// in the tree.
    fn into_artifact(self, furniture: Furniture) -> Draw {
        match self {
            Draw::Text { x, baseline, run, .. } => Draw::Text {
                x,
                baseline,
                run,
                mark: Mark::Artifact(furniture),
            },
            Draw::Image {
                x, y, w, h, image, ..
            } => Draw::Image {
                x,
                y,
                w,
                h,
                image,
                mark: Mark::Artifact(furniture),
            },
            Draw::Link {
                x, y, w, h, url, ..
            } => Draw::Link {
                x,
                y,
                w,
                h,
                url,
                slot: None,
            },
            other => other,
        }
    }
}

/// A heading's place in the outline.
#[derive(Clone, Debug)]
pub(crate) struct Anchor {
    pub(crate) level: u8,
    pub(crate) title: String,
}

/// Something that is not split across pages.
#[derive(Clone, Debug, Default)]
pub(crate) struct Piece {
    /// Space above it, dropped at the top of a page.
    pub(crate) gap: f32,
    pub(crate) height: f32,
    /// Relative to the piece's top-left corner.
    pub(crate) draws: Vec<Draw>,
    pub(crate) anchor: Option<Anchor>,
    /// Must share a page with the piece after it.
    pub(crate) keep_with_next: bool,
}

/// A laid-out block: its pieces and the space after it (which collapses
/// with the next block's space before).
#[derive(Clone, Debug, Default)]
pub(crate) struct Laid {
    pub(crate) pieces: Vec<Piece>,
    pub(crate) after: f32,
}

/// A cell's content, placed in its row.
#[derive(Clone, Debug)]
struct CellLaid {
    x: f32,
    width: f32,
    pieces: Vec<Piece>,
    valign: VAlign,
    background: Option<Color>,
}

impl CellLaid {
    fn content_height(&self) -> f32 {
        stack_height(&self.pieces)
    }
}

/// A table row: its cells, whose content the row's height holds.
#[derive(Clone, Debug)]
struct RowLaid {
    cells: Vec<CellLaid>,
    height: f32,
    background: Option<Color>,
}

/// A laid-out table.
#[derive(Clone, Debug)]
pub(crate) struct TableLaid {
    gap: f32,
    after: f32,
    width: f32,
    header: Vec<RowLaid>,
    rows: Vec<RowLaid>,
    borders: Borders,
    padding: (f32, f32),
    header_background: Option<Color>,
}

/// What the page flow is made of.
#[derive(Clone, Debug)]
pub(crate) enum Flow {
    Block(Laid),
    Table(Box<TableLaid>),
    PageBreak,
}

/// One laid-out page.
#[derive(Clone, Debug, Default)]
pub(crate) struct Page {
    pub(crate) draws: Vec<Draw>,
    /// Headings on the page: their outline entry and their top.
    pub(crate) anchors: Vec<(Anchor, f32)>,
}

/// A document laid out: pages, the structure tree, and what goes into the
/// metadata.
#[derive(Debug)]
pub(crate) struct LaidDocument {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) pages: Vec<Page>,
    pub(crate) tree: StructTree,
}

/// Why a document could not be laid out.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LayoutError {
    /// An image that does not decode.
    Image {
        source: Option<String>,
        message: String,
    },
    /// No font at all (bundled fonts off and no font directory).
    NoFonts,
}

/// What the placeholders of headers and footers print.
#[derive(Clone, Debug, Default)]
pub(crate) struct Fields {
    pub(crate) page: Option<usize>,
    pub(crate) pages: Option<usize>,
    pub(crate) title: Option<String>,
    pub(crate) date: String,
}

/// Where a block is laid out.
#[derive(Clone, Debug)]
struct Context {
    width: f32,
    /// The structure node the block's nodes go under.
    parent: Option<NodeId>,
    style: TextStyle,
    /// Set for headers and footers: everything is an artifact.
    furniture: Option<Furniture>,
    list_depth: usize,
    /// The tallest a single piece may be (a page's body).
    max_height: f32,
}

/// Lays out a document with `fonts`; `date` is what `Run::date` prints.
pub(crate) fn lay_out(
    document: &PdfDocument,
    fonts: &FontBook,
    date: &str,
) -> Result<LaidDocument, LayoutError> {
    if fonts.is_empty() {
        return Err(LayoutError::NoFonts);
    }
    let mut layout = Layout::new(document, fonts, date);
    layout.run(document)
}

struct Layout<'f> {
    shaper: Shaper<'f>,
    tree: StructTree,
    base: TextStyle,
    default_families: Arc<[String]>,
    direction: Option<Direction>,
    lang: Option<Arc<str>>,
    fields: Fields,
    images: HashMap<(usize, usize), PdfImage>,
}

impl<'f> Layout<'f> {
    fn new(document: &PdfDocument, fonts: &'f FontBook, date: &str) -> Self {
        let default_families: Arc<[String]> = Arc::from(vec!["Noto Sans".to_owned()]);
        let defaults = TextStyle {
            font: Some(default_families.clone()),
            size: Some(11.0),
            bold: Some(false),
            italic: Some(false),
            color: Some(Color::BLACK),
            underline: Some(false),
            strike: Some(false),
            line_height: Some(1.4),
        };
        Self {
            shaper: Shaper::new(fonts),
            tree: StructTree::default(),
            base: document.style.over(&defaults),
            default_families,
            direction: document.direction,
            lang: document.lang.as_deref().map(Arc::from),
            fields: Fields {
                title: document.title.clone(),
                date: date.to_owned(),
                ..Fields::default()
            },
            images: HashMap::new(),
        }
    }

    fn run(&mut self, document: &PdfDocument) -> Result<LaidDocument, LayoutError> {
        let (width, height) = document.page.size();
        let margins = document.page.margins;
        let (top, right, bottom, left) = (
            mm_to_pt(margins.top as f32),
            mm_to_pt(margins.right as f32),
            mm_to_pt(margins.bottom as f32),
            mm_to_pt(margins.left as f32),
        );
        let body_width = (width - left - right).max(36.0);
        // Headers and footers are measured with wide numbers, then laid
        // out again on every page.
        self.fields.page = Some(888);
        self.fields.pages = Some(888);
        let header_height = self.furniture_height(&document.header, body_width, height)?;
        let footer_height = self.furniture_height(&document.footer, body_width, height)?;
        let header_y = if header_height + 2.0 * FURNITURE_GAP <= top {
            (top - header_height) / 2.0
        } else {
            FURNITURE_GAP
        };
        let body_top = if document.header.is_empty() {
            top
        } else {
            top.max(header_y + header_height + FURNITURE_GAP)
        };
        let footer_y = if footer_height + 2.0 * FURNITURE_GAP <= bottom {
            height - bottom + (bottom - footer_height) / 2.0
        } else {
            height - FURNITURE_GAP - footer_height
        };
        let body_bottom = if document.footer.is_empty() {
            height - bottom
        } else {
            (height - bottom).min(footer_y - FURNITURE_GAP)
        };
        let body_height = (body_bottom - body_top).max(36.0);
        self.fields.page = None;
        self.fields.pages = None;

        let cx = Context {
            width: body_width,
            parent: None,
            style: self.base.clone(),
            furniture: None,
            list_depth: 0,
            max_height: body_height,
        };
        let mut flow = Vec::new();
        for block in &document.blocks {
            flow.push(self.block(block, &cx)?);
        }
        let mut pages = paginate(flow, body_height);
        for page in &mut pages {
            for draw in &mut page.draws {
                draw.translate(left, body_top);
            }
            for (_, y) in &mut page.anchors {
                *y += body_top;
            }
        }
        let count = pages.len();
        for (index, page) in pages.iter_mut().enumerate() {
            self.fields.page = Some(index + 1);
            self.fields.pages = Some(count);
            for (blocks, y, furniture) in [
                (&document.header, header_y, Furniture::Header),
                (&document.footer, footer_y, Furniture::Footer),
            ] {
                let pieces = self.furniture(blocks, body_width, height, furniture)?;
                let mut top = y;
                for (i, piece) in pieces.into_iter().enumerate() {
                    if i > 0 {
                        top += piece.gap;
                    }
                    for mut draw in piece.draws {
                        draw.translate(left, top);
                        page.draws.push(draw);
                    }
                    top += piece.height;
                }
            }
        }
        Ok(LaidDocument {
            width,
            height,
            pages,
            tree: std::mem::take(&mut self.tree),
        })
    }

    fn furniture_height(
        &mut self,
        blocks: &[Block],
        width: f32,
        page_height: f32,
    ) -> Result<f32, LayoutError> {
        let pieces = self.furniture(blocks, width, page_height, Furniture::Header)?;
        Ok(stack_height(&pieces))
    }

    /// A header's or footer's pieces, in small gray text unless styled.
    fn furniture(
        &mut self,
        blocks: &[Block],
        width: f32,
        page_height: f32,
        furniture: Furniture,
    ) -> Result<Vec<Piece>, LayoutError> {
        let style = TextStyle {
            size: Some(9.0),
            color: Some(FURNITURE_COLOR),
            line_height: Some(1.3),
            ..TextStyle::default()
        }
        .over(&self.base);
        let cx = Context {
            width,
            parent: None,
            style,
            furniture: Some(furniture),
            list_depth: 0,
            max_height: page_height / 3.0,
        };
        let mut pieces = Vec::new();
        for block in blocks {
            pieces.extend(flatten(self.block(block, &cx)?));
        }
        Ok(pieces)
    }

    /// One block at `cx`.
    fn block(&mut self, block: &Block, cx: &Context) -> Result<Flow, LayoutError> {
        Ok(match block {
            Block::Paragraph(paragraph) => {
                let size = paragraph.style.over(&cx.style).size.unwrap_or(11.0);
                let node = self.node(cx, NodeKind::P);
                let pieces = self.paragraph(paragraph, &cx.style, Align::Start, cx, node);
                Flow::Block(Laid {
                    pieces,
                    after: 0.65 * size,
                })
            }
            Block::Heading(level, paragraph) => {
                let level = (*level).clamp(1, 6);
                let base_size = cx.style.size.unwrap_or(11.0);
                let size = base_size * HEADING_SCALE[usize::from(level - 1)];
                let style = TextStyle {
                    size: Some(size),
                    bold: Some(true),
                    line_height: Some(1.25),
                    ..TextStyle::default()
                }
                .over(&cx.style);
                let size = paragraph.style.over(&style).size.unwrap_or(size);
                let node = self.node(cx, NodeKind::H(level));
                let mut pieces = self.paragraph(paragraph, &style, Align::Start, cx, node);
                if let Some(first) = pieces.first_mut() {
                    first.gap = 0.9 * size;
                    if cx.furniture.is_none() {
                        first.anchor = Some(Anchor {
                            level,
                            title: plain_text(paragraph),
                        });
                    }
                }
                for piece in &mut pieces {
                    piece.keep_with_next = true;
                }
                Flow::Block(Laid {
                    pieces,
                    after: 0.35 * size,
                })
            }
            Block::List(list) => {
                let size = cx.style.size.unwrap_or(11.0);
                let mut laid = self.list(list, cx)?;
                if let Some(first) = laid.pieces.first_mut() {
                    first.gap = first.gap.max(0.2 * size);
                }
                laid.after = 0.65 * size;
                Flow::Block(laid)
            }
            Block::Table(table) => {
                let size = table.style.over(&cx.style).size.unwrap_or(11.0);
                let mut laid = self.table(table, cx)?;
                laid.gap = 0.35 * size;
                laid.after = 0.9 * size;
                Flow::Table(Box::new(laid))
            }
            Block::Image(image) => {
                let size = cx.style.size.unwrap_or(11.0);
                let piece = self.image(image, cx)?;
                Flow::Block(Laid {
                    pieces: vec![piece],
                    after: 0.65 * size,
                })
            }
            Block::Rule => {
                let size = cx.style.size.unwrap_or(11.0);
                Flow::Block(Laid {
                    pieces: vec![Piece {
                        gap: 0.5 * size,
                        height: 0.75,
                        draws: vec![Draw::Stroke {
                            x1: 0.0,
                            y1: 0.375,
                            x2: cx.width,
                            y2: 0.375,
                            width: 0.75,
                            color: RULE_COLOR,
                        }],
                        ..Piece::default()
                    }],
                    after: 0.65 * size,
                })
            }
            Block::Space(height) => Flow::Block(Laid {
                pieces: vec![Piece {
                    height: height.max(0.0),
                    ..Piece::default()
                }],
                after: 0.0,
            }),
            Block::PageBreak => Flow::PageBreak,
        })
    }

    /// A structure node under the context's parent, unless the context
    /// is page furniture.
    fn node(&mut self, cx: &Context, kind: NodeKind) -> Option<NodeId> {
        if cx.furniture.is_some() {
            return None;
        }
        Some(self.tree.node(cx.parent, kind))
    }

    /// A paragraph's lines as pieces, tagged in `node`.
    fn paragraph(
        &mut self,
        paragraph: &Paragraph,
        inherited: &TextStyle,
        align: Align,
        cx: &Context,
        node: Option<NodeId>,
    ) -> Vec<Piece> {
        let para = self.para(paragraph, inherited, align);
        let lines = self.shaper.lines(&para, cx.width);
        let count = lines.len();
        let mut pieces = Vec::with_capacity(count);
        for (index, line) in lines.into_iter().enumerate() {
            let baseline = line.ascent;
            let height = line.height();
            let mut draws = Vec::new();
            // Slots in reading order, drawing in visual order.
            let mut order: Vec<usize> = (0..line.runs.len()).collect();
            order.sort_by_key(|&i| line.runs[i].start);
            let mut marks = vec![(Mark::Artifact(Furniture::Layout), None); line.runs.len()];
            for &i in &order {
                let run = &line.runs[i];
                marks[i] = match (cx.furniture, node) {
                    (Some(furniture), _) => (Mark::Artifact(furniture), None),
                    (None, Some(node)) => match &run.link {
                        Some(_) => {
                            let link = self.tree.node(Some(node), NodeKind::Link);
                            let text = self.tree.slot(link);
                            let annotation = self.tree.slot(link);
                            (Mark::Content(text), Some(annotation))
                        }
                        None => (Mark::Content(self.tree.slot(node)), None),
                    },
                    (None, None) => (Mark::Artifact(Furniture::Layout), None),
                };
            }
            for (run, (mark, link_slot)) in line.runs.into_iter().zip(marks) {
                let face = self.shaper.fonts().face(run.face);
                let size = run.style.size;
                for (on, (position, thickness)) in [
                    (run.style.underline, face.underline),
                    (run.style.strike, face.strike),
                ] {
                    if on && run.width > 0.0 {
                        let y = baseline - position * size + thickness * size / 2.0;
                        draws.push(Draw::Stroke {
                            x1: run.x,
                            y1: y,
                            x2: run.x + run.width,
                            y2: y,
                            width: (thickness * size).max(0.5),
                            color: run.style.color,
                        });
                    }
                }
                if let Some(url) = &run.link {
                    draws.push(Draw::Link {
                        x: run.x,
                        y: 0.0,
                        w: run.width,
                        h: height,
                        url: url.clone(),
                        slot: link_slot,
                    });
                }
                draws.push(Draw::Text {
                    x: run.x,
                    baseline,
                    run: Arc::new(run),
                    mark,
                });
            }
            pieces.push(Piece {
                gap: 0.0,
                height,
                draws,
                anchor: None,
                // Orphans and widows: the first two lines and the last two
                // share a page.
                keep_with_next: count >= 2 && (index == 0 || index + 2 == count),
            });
        }
        pieces
    }

    /// A model paragraph as text spans with resolved styles.
    fn para(&mut self, paragraph: &Paragraph, inherited: &TextStyle, align: Align) -> Para {
        let block = paragraph.style.over(inherited);
        let base = Arc::new(self.resolve(&block));
        let mut para = Para::new(base);
        para.align = paragraph.align.unwrap_or(align);
        para.direction = paragraph.direction.or(self.direction);
        para.lang = self.lang.clone();
        para.preformatted = paragraph.preformatted;
        for run in &paragraph.runs {
            let mut own = run.style.clone();
            if run.link.is_some() {
                own.color = own.color.or(Some(LINK_COLOR));
                own.underline = own.underline.or(Some(true));
            }
            let style = Arc::new(self.resolve(&own.over(&block)));
            let text = match &run.kind {
                RunKind::Text(text) => text.clone(),
                RunKind::LineBreak => LINE_BREAK.to_string(),
                RunKind::PageNumber => self.fields.page.map(|p| p.to_string()).unwrap_or_default(),
                RunKind::PageCount => self.fields.pages.map(|p| p.to_string()).unwrap_or_default(),
                RunKind::Title => self.fields.title.clone().unwrap_or_default(),
                RunKind::Date => self.fields.date.clone(),
            };
            para.push(&text, style, run.link.as_deref().map(Arc::from));
        }
        para
    }

    fn resolve(&self, style: &TextStyle) -> Style {
        Style {
            families: style
                .font
                .clone()
                .unwrap_or_else(|| self.default_families.clone()),
            size: style.size.unwrap_or(11.0).clamp(1.0, 400.0),
            bold: style.bold.unwrap_or(false),
            italic: style.italic.unwrap_or(false),
            color: style.color.unwrap_or(Color::BLACK),
            underline: style.underline.unwrap_or(false),
            strike: style.strike.unwrap_or(false),
            line_height: style.line_height.unwrap_or(1.4).clamp(0.5, 5.0),
        }
    }

    /// A list: markers in the indent (on the right in right-to-left
    /// documents), items' blocks beside them.
    fn list(&mut self, list: &List, cx: &Context) -> Result<Laid, LayoutError> {
        let size = cx.style.size.unwrap_or(11.0);
        let indent = 1.6 * size;
        let marker_gap = 0.45 * size;
        let rtl = self.direction == Some(Direction::Rtl);
        let ordered = list.kind != ListKind::Bullets;
        let node = self.node(cx, NodeKind::L(ordered));
        let mut pieces = Vec::new();
        for (index, item) in list.items.iter().enumerate() {
            let li = node.map(|node| self.tree.node(Some(node), NodeKind::Li));
            let label = li.map(|li| self.tree.node(Some(li), NodeKind::Lbl));
            let body = li.map(|li| self.tree.node(Some(li), NodeKind::LBody));
            let item_cx = Context {
                width: (cx.width - indent).max(size),
                parent: body,
                list_depth: cx.list_depth + 1,
                ..cx.clone()
            };
            let mut item_pieces = Vec::new();
            for block in &item.blocks {
                let flow = self.block(block, &item_cx)?;
                let laid = flatten(flow);
                item_pieces.extend(laid);
            }
            if item_pieces.is_empty() {
                item_pieces.push(Piece {
                    height: self.shaper.empty_line(&self.resolve(&cx.style)).0
                        + self.shaper.empty_line(&self.resolve(&cx.style)).1,
                    ..Piece::default()
                });
            }
            let dx = if rtl { 0.0 } else { indent };
            for piece in &mut item_pieces {
                for draw in &mut piece.draws {
                    draw.translate(dx, 0.0);
                }
            }
            // The marker, on the first line's baseline.
            let marker = self.marker(list, cx.list_depth, index);
            let marker_paragraph = Paragraph::new().text(marker);
            let mut marker_cx = cx.clone();
            marker_cx.width = indent * 4.0;
            let marker_pieces = self.paragraph(
                &marker_paragraph,
                &cx.style,
                Align::Start,
                &Context {
                    furniture: cx.furniture,
                    ..marker_cx
                },
                label,
            );
            let first_baseline = item_pieces[0]
                .draws
                .iter()
                .find_map(|draw| match draw {
                    Draw::Text { baseline, .. } => Some(*baseline),
                    _ => None,
                })
                .unwrap_or(size);
            if let Some(marker_piece) = marker_pieces.into_iter().next() {
                let width: f32 = marker_piece
                    .draws
                    .iter()
                    .filter_map(|draw| match draw {
                        Draw::Text { run, .. } => Some(run.width),
                        _ => None,
                    })
                    .sum();
                let marker_baseline = marker_piece
                    .draws
                    .iter()
                    .find_map(|draw| match draw {
                        Draw::Text { baseline, .. } => Some(*baseline),
                        _ => None,
                    })
                    .unwrap_or(0.0);
                let x = if rtl {
                    cx.width - indent + marker_gap
                } else {
                    indent - marker_gap - width
                };
                for mut draw in marker_piece.draws {
                    draw.translate(x, first_baseline - marker_baseline);
                    item_pieces[0].draws.push(draw);
                }
            }
            item_pieces[0].gap = if index == 0 { 0.0 } else { 0.25 * size };
            pieces.extend(item_pieces);
        }
        Ok(Laid { pieces, after: 0.0 })
    }

    /// The marker of item `index` at nesting `depth`.
    fn marker(&mut self, list: &List, depth: usize, index: usize) -> String {
        let number = list.start as usize + index;
        match list.kind {
            ListKind::Bullets => {
                let wanted = ['•', '◦', '▪'][depth % 3];
                let style = self.resolve(&self.base.clone());
                let face = self
                    .shaper
                    .fonts()
                    .select(&style.families, false, false, wanted);
                let covered = face.is_some_and(|face| self.shaper.fonts().face(face).covers(wanted));
                if covered { wanted } else { '•' }.to_string()
            }
            ListKind::Decimal => format!("{number}."),
            ListKind::LowerAlpha => format!("{}.", alpha(number)),
            ListKind::UpperAlpha => format!("{}.", alpha(number).to_uppercase()),
            ListKind::LowerRoman => format!("{}.", roman(number)),
            ListKind::UpperRoman => format!("{}.", roman(number).to_uppercase()),
        }
    }

    /// An image block: sized, aligned, tagged as a figure.
    fn image(&mut self, image: &Image, cx: &Context) -> Result<Piece, LayoutError> {
        let decoded = self.decode(image)?;
        let (pixels_w, pixels_h) = decoded.size();
        let (natural_w, natural_h) = (pixels_w as f32 * 0.75, pixels_h as f32 * 0.75);
        let ratio = if natural_w > 0.0 {
            natural_h / natural_w
        } else {
            1.0
        };
        let wanted_w = image
            .width
            .or(image.width_percent.map(|p| cx.width * p / 100.0));
        let (mut w, mut h) = match (wanted_w, image.height) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * ratio),
            (None, Some(h)) => (if ratio > 0.0 { h / ratio } else { h }, h),
            (None, None) => (natural_w, natural_h),
        };
        if w > cx.width {
            h *= cx.width / w;
            w = cx.width;
        }
        if h > cx.max_height {
            w *= cx.max_height / h;
            h = cx.max_height;
        }
        let rtl = self.direction == Some(Direction::Rtl);
        let x = match (image.align.unwrap_or(Align::Start), rtl) {
            (Align::Center, _) => (cx.width - w) / 2.0,
            (Align::Right, _) | (Align::End, false) | (Align::Start | Align::Justify, true) => {
                cx.width - w
            }
            _ => 0.0,
        };
        let mark = match cx.furniture {
            Some(furniture) => Mark::Artifact(furniture),
            None => match self.node(cx, NodeKind::Figure(image.alt.clone())) {
                Some(node) => Mark::Content(self.tree.slot(node)),
                None => Mark::Artifact(Furniture::Layout),
            },
        };
        Ok(Piece {
            gap: 0.0,
            height: h,
            draws: vec![Draw::Image {
                x,
                y: 0.0,
                w,
                h,
                image: decoded,
                mark,
            }],
            ..Piece::default()
        })
    }

    fn decode(&mut self, image: &Image) -> Result<PdfImage, LayoutError> {
        let key = (image.bytes.as_ptr() as usize, image.bytes.len());
        if let Some(decoded) = self.images.get(&key) {
            return Ok(decoded.clone());
        }
        let bytes = image.bytes.clone();
        let data: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(bytes.clone());
        let data = krilla::Data::from(data);
        let decoded = if bytes.starts_with(b"\x89PNG") {
            PdfImage::from_png(data, true)
        } else if bytes.starts_with(&[0xFF, 0xD8]) {
            PdfImage::from_jpeg(data, true)
        } else if bytes.starts_with(b"GIF8") {
            PdfImage::from_gif(data, true)
        } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            PdfImage::from_webp(data, true)
        } else {
            Err("not a PNG, JPEG, GIF or WebP image".to_owned())
        }
        .map_err(|message| LayoutError::Image {
            source: image.source.clone(),
            message,
        })?;
        self.images.insert(key, decoded.clone());
        Ok(decoded)
    }

    /// A table: columns sized, cells laid out, rows measured.
    fn table(&mut self, table: &Table, cx: &Context) -> Result<TableLaid, LayoutError> {
        let style = table.style.over(&cx.style);
        let columns = table
            .header
            .iter()
            .chain(&table.rows)
            .map(|row| row.cells.iter().map(|c| c.colspan.max(1)).sum::<usize>())
            .max()
            .unwrap_or(0)
            .max(1);
        let (pad_v, pad_h) = table.padding;
        let widths = self.column_widths(table, columns, cx.width, &style);
        let mut edges = vec![0.0f32];
        for w in &widths {
            edges.push(edges.last().copied().unwrap_or(0.0) + w);
        }
        let rtl = self.direction == Some(Direction::Rtl);
        let node = self.node(cx, NodeKind::Table);
        let mut laid = TableLaid {
            gap: 0.0,
            after: 0.0,
            width: cx.width,
            header: Vec::new(),
            rows: Vec::new(),
            borders: table.borders,
            padding: table.padding,
            header_background: table.header_background,
        };
        for (group, header) in [(&table.header, true), (&table.rows, false)] {
            if group.is_empty() {
                continue;
            }
            let group_node = node.map(|node| {
                self.tree.node(
                    Some(node),
                    if header {
                        NodeKind::THead
                    } else {
                        NodeKind::TBody
                    },
                )
            });
            for row in group {
                let laid_row = self.row(
                    row,
                    header,
                    &RowSetup {
                        edges: &edges,
                        columns,
                        pad: (pad_v, pad_h),
                        rtl,
                        width: cx.width,
                        style: &style,
                    },
                    cx,
                    group_node,
                )?;
                if header {
                    laid.header.push(laid_row);
                } else {
                    laid.rows.push(laid_row);
                }
            }
        }
        Ok(laid)
    }

    /// The columns' widths: as the table asks, or shared by content.
    fn column_widths(
        &mut self,
        table: &Table,
        columns: usize,
        width: f32,
        style: &TextStyle,
    ) -> Vec<f32> {
        if let Some(weights) = &table.widths {
            let mut weights: Vec<f32> = weights.iter().copied().take(columns).collect();
            let fill = if weights.is_empty() {
                1.0
            } else {
                weights.iter().sum::<f32>() / weights.len() as f32
            };
            weights.resize(columns, fill.max(0.01));
            let total: f32 = weights.iter().sum::<f32>().max(0.01);
            return weights.iter().map(|w| width * w / total).collect();
        }
        let (_, pad_h) = table.padding;
        let mut min = vec![0.0f32; columns];
        let mut max = vec![0.0f32; columns];
        let mut percent: Vec<Option<f32>> = vec![None; columns];
        for (row, header) in table
            .header
            .iter()
            .map(|r| (r, true))
            .chain(table.rows.iter().map(|r| (r, false)))
        {
            let mut column = 0;
            for cell in &row.cells {
                let span = cell.colspan.max(1);
                if column >= columns {
                    break;
                }
                let (lo, hi) = self.measure_cell(cell, row, header, style);
                let (lo, hi) = (lo + 2.0 * pad_h, hi + 2.0 * pad_h);
                if span == 1 {
                    min[column] = min[column].max(lo);
                    max[column] = max[column].max(hi);
                    if let Some(p) = cell.width_percent
                        && percent[column].is_none()
                    {
                        percent[column] = Some(p);
                    }
                }
                column += span;
            }
        }
        // Columns with a percentage get it; the others share the rest.
        let fixed: f32 = percent.iter().flatten().map(|p| width * p / 100.0).sum();
        let rest = (width - fixed).max(0.0);
        let free: Vec<usize> = (0..columns).filter(|&c| percent[c].is_none()).collect();
        let sum_max: f32 = free.iter().map(|&c| max[c]).sum();
        let sum_min: f32 = free.iter().map(|&c| min[c]).sum();
        let mut widths = vec![0.0f32; columns];
        for c in 0..columns {
            if let Some(p) = percent[c] {
                widths[c] = width * p / 100.0;
            }
        }
        if !free.is_empty() {
            if sum_max <= rest && sum_max > 0.0 {
                for &c in &free {
                    widths[c] = max[c] + (rest - sum_max) * max[c] / sum_max;
                }
            } else if sum_min <= rest {
                let spread: f32 = free.iter().map(|&c| max[c] - min[c]).sum::<f32>().max(0.01);
                for &c in &free {
                    widths[c] = min[c] + (rest - sum_min) * (max[c] - min[c]) / spread;
                }
            } else {
                let total = sum_min.max(0.01);
                for &c in &free {
                    widths[c] = rest * min[c] / total;
                }
            }
            if sum_max <= 0.0 {
                for &c in &free {
                    widths[c] = rest / free.len() as f32;
                }
            }
        }
        widths
    }

    /// A cell's narrowest and widest content widths.
    fn measure_cell(&mut self, cell: &Cell, row: &Row, header: bool, style: &TextStyle) -> (f32, f32) {
        let style = self.cell_style(cell, row, header, style);
        let (mut lo, mut hi) = (0.0f32, 0.0f32);
        for block in &cell.blocks {
            let (a, b) = match block {
                Block::Paragraph(paragraph) | Block::Heading(_, paragraph) => {
                    let para = self.para(paragraph, &style, Align::Start);
                    self.shaper.measure(&para)
                }
                Block::Image(image) => match self.decode(image) {
                    Ok(decoded) => {
                        let natural = decoded.size().0 as f32 * 0.75;
                        let w = image.width.unwrap_or(natural);
                        (w.min(120.0), w)
                    }
                    Err(_) => (0.0, 0.0),
                },
                Block::List(list) => {
                    let size = style.size.unwrap_or(11.0);
                    let mut widest = (0.0f32, 0.0f32);
                    for item in &list.items {
                        for block in &item.blocks {
                            if let Block::Paragraph(paragraph) = block {
                                let para = self.para(paragraph, &style, Align::Start);
                                let (a, b) = self.shaper.measure(&para);
                                widest = (widest.0.max(a), widest.1.max(b));
                            }
                        }
                    }
                    (widest.0 + 1.6 * size, widest.1 + 1.6 * size)
                }
                _ => (0.0, 0.0),
            };
            lo = lo.max(a);
            hi = hi.max(b);
        }
        (lo, hi)
    }

    fn cell_style(&self, cell: &Cell, row: &Row, header: bool, table: &TextStyle) -> TextStyle {
        let header_style = TextStyle {
            bold: if header { Some(true) } else { None },
            ..TextStyle::default()
        };
        cell.style.over(&row.style.over(&header_style.over(table)))
    }

    /// One row: each cell's blocks laid out in its columns.
    fn row(
        &mut self,
        row: &Row,
        header: bool,
        setup: &RowSetup<'_>,
        cx: &Context,
        group: Option<NodeId>,
    ) -> Result<RowLaid, LayoutError> {
        let tr = group.map(|group| self.tree.node(Some(group), NodeKind::Tr));
        let (pad_v, pad_h) = setup.pad;
        let mut cells = Vec::new();
        let mut column = 0;
        for cell in &row.cells {
            if column >= setup.columns {
                break;
            }
            let span = cell.colspan.max(1).min(setup.columns - column);
            let x = setup.edges[column];
            let width = setup.edges[column + span] - x;
            column += span;
            let style = self.cell_style(cell, row, header, setup.style);
            let td = tr.map(|tr| {
                self.tree
                    .node(Some(tr), if header { NodeKind::Th } else { NodeKind::Td })
            });
            let cell_cx = Context {
                width: (width - 2.0 * pad_h).max(1.0),
                parent: td,
                style: style.clone(),
                furniture: cx.furniture,
                list_depth: cx.list_depth,
                max_height: cx.max_height,
            };
            let mut pieces = Vec::new();
            for block in &cell.blocks {
                match block {
                    // A cell's paragraphs are the cell's content.
                    Block::Paragraph(paragraph) => {
                        let align = cell.align.unwrap_or(Align::Start);
                        let size = paragraph.style.over(&style).size.unwrap_or(11.0);
                        let mut lines = self.paragraph(paragraph, &style, align, &cell_cx, td);
                        if let Some(first) = lines.first_mut()
                            && !pieces.is_empty()
                        {
                            first.gap = 0.4 * size;
                        }
                        pieces.extend(lines);
                    }
                    other => {
                        let flow = self.block(other, &cell_cx)?;
                        let mut laid = flatten(flow);
                        if let Some(first) = laid.first_mut()
                            && pieces.is_empty()
                        {
                            first.gap = 0.0;
                        }
                        pieces.extend(laid);
                    }
                }
            }
            for piece in &mut pieces {
                for draw in &mut piece.draws {
                    draw.translate(pad_h, 0.0);
                }
            }
            let x = if setup.rtl {
                setup.width - x - width
            } else {
                x
            };
            cells.push(CellLaid {
                x,
                width,
                pieces,
                valign: cell.valign,
                background: cell.background,
            });
        }
        let content = cells
            .iter()
            .map(CellLaid::content_height)
            .fold(0.0f32, f32::max);
        let minimum = {
            let resolved = self.resolve(setup.style);
            let (a, d) = self.shaper.empty_line(&resolved);
            a + d
        };
        Ok(RowLaid {
            cells,
            height: content.max(minimum) + 2.0 * pad_v,
            background: row.background,
        })
    }
}

/// What every row of one table shares.
struct RowSetup<'a> {
    edges: &'a [f32],
    columns: usize,
    pad: (f32, f32),
    rtl: bool,
    width: f32,
    style: &'a TextStyle,
}

/// The height of pieces stacked, their gaps included (but the first's).
fn stack_height(pieces: &[Piece]) -> f32 {
    pieces
        .iter()
        .enumerate()
        .map(|(i, p)| if i == 0 { p.height } else { p.gap + p.height })
        .sum()
}

/// A flow item as plain pieces (inside a cell, a list item or a header):
/// a table becomes one piece per row.
fn flatten(flow: Flow) -> Vec<Piece> {
    match flow {
        Flow::Block(laid) => laid.pieces,
        Flow::Table(table) => {
            let mut pieces = Vec::new();
            let header_rows = table.header.len();
            for (index, row) in table.header.iter().chain(&table.rows).enumerate() {
                let draws = row_draws(
                    &table,
                    row,
                    index < header_rows,
                    index + 1 == header_rows,
                    row.height,
                );
                pieces.push(Piece {
                    gap: if index == 0 { table.gap } else { 0.0 },
                    height: row.height,
                    draws,
                    ..Piece::default()
                });
            }
            pieces
        }
        Flow::PageBreak => Vec::new(),
    }
}

/// A row's drawings, relative to its top: fills, then content, then lines.
fn row_draws(
    table: &TableLaid,
    row: &RowLaid,
    header: bool,
    last_header: bool,
    height: f32,
) -> Vec<Draw> {
    let (pad_v, _) = table.padding;
    let mut draws = Vec::new();
    let fill = if header {
        row.background.or(table.header_background)
    } else {
        row.background
    };
    // Each cell is filled once, with its own color or the row's, and
    // neighbours of one color as one rectangle, so no edge shows between
    // fills.
    let mut fills: Vec<(f32, f32, Color)> = row
        .cells
        .iter()
        .filter_map(|cell| {
            let color = cell.background.or(fill)?;
            Some((cell.x, cell.x + cell.width, color))
        })
        .collect();
    fills.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f32, f32, Color)> = Vec::new();
    for (x0, x1, color) in fills {
        match merged.last_mut() {
            Some(last) if last.2 == color && (last.1 - x0).abs() < 0.01 => last.1 = x1,
            _ => merged.push((x0, x1, color)),
        }
    }
    for (x0, x1, color) in merged {
        draws.push(Draw::Fill {
            x: x0,
            y: 0.0,
            w: x1 - x0,
            h: height,
            color,
        });
    }
    for cell in &row.cells {
        let content = cell.content_height();
        let room = (height - 2.0 * pad_v - content).max(0.0);
        let offset = match cell.valign {
            VAlign::Top => 0.0,
            VAlign::Middle => room / 2.0,
            VAlign::Bottom => room,
        };
        let mut y = pad_v + offset;
        for (i, piece) in cell.pieces.iter().enumerate() {
            if i > 0 {
                y += piece.gap;
            }
            for draw in &piece.draws {
                let mut draw = draw.clone();
                draw.translate(cell.x, y);
                draws.push(draw);
            }
            y += piece.height;
        }
    }
    match table.borders {
        Borders::None => {}
        Borders::Rows => {
            let (width, color) = if last_header {
                (0.8, HEADER_RULE_COLOR)
            } else {
                (0.5, BORDER_COLOR)
            };
            draws.push(Draw::Stroke {
                x1: 0.0,
                y1: height,
                x2: table.width,
                y2: height,
                width,
                color,
            });
        }
        Borders::Grid => {
            for cell in &row.cells {
                let (x0, x1) = (cell.x, cell.x + cell.width);
                for (a, b, c, d) in [
                    (x0, 0.0, x1, 0.0),
                    (x0, height, x1, height),
                    (x0, 0.0, x0, height),
                    (x1, 0.0, x1, height),
                ] {
                    draws.push(Draw::Stroke {
                        x1: a,
                        y1: b,
                        x2: c,
                        y2: d,
                        width: 0.5,
                        color: BORDER_COLOR,
                    });
                }
            }
        }
    }
    draws
}

/// A row cut to `limit` points: what fits (at least one piece of each
/// cell that has any) and the rest, `None` when nothing is left.
fn split_row(row: &RowLaid, limit: f32, pad_v: f32) -> (RowLaid, Option<RowLaid>) {
    let room = (limit - 2.0 * pad_v).max(0.0);
    let mut head = row.clone();
    let mut tail = row.clone();
    let mut rest = false;
    for (index, cell) in row.cells.iter().enumerate() {
        let mut used = 0.0;
        let mut take = 0;
        for (i, piece) in cell.pieces.iter().enumerate() {
            let need = if i == 0 { piece.height } else { piece.gap + piece.height };
            if used + need > room && take > 0 {
                break;
            }
            used += need;
            take += 1;
        }
        head.cells[index].pieces = cell.pieces[..take].to_vec();
        let mut remaining = cell.pieces[take..].to_vec();
        if let Some(first) = remaining.first_mut() {
            first.gap = 0.0;
            rest = true;
        }
        tail.cells[index].pieces = remaining;
    }
    head.height = head
        .cells
        .iter()
        .map(CellLaid::content_height)
        .fold(0.0f32, f32::max)
        + 2.0 * pad_v;
    if !rest {
        return (head, None);
    }
    tail.height = tail
        .cells
        .iter()
        .map(CellLaid::content_height)
        .fold(0.0f32, f32::max)
        + 2.0 * pad_v;
    (head, Some(tail))
}

/// What pagination places, in order.
enum Unit {
    Piece {
        piece: Piece,
        gap: f32,
    },
    /// A table row (`None`: a table of header rows only).
    Row {
        table: usize,
        row: Option<RowLaid>,
        gap: f32,
    },
    Break,
}

/// Fills pages of `height` points with the flow.
pub(crate) fn paginate(flow: Vec<Flow>, height: f32) -> Vec<Page> {
    // Flatten: collapse each block's space before with the space after
    // the block before it.
    let mut units = Vec::new();
    let mut tables: Vec<TableLaid> = Vec::new();
    let mut after = 0.0f32;
    for item in flow {
        match item {
            Flow::Block(laid) => {
                for (i, piece) in laid.pieces.into_iter().enumerate() {
                    let gap = if i == 0 { piece.gap.max(after) } else { piece.gap };
                    units.push(Unit::Piece { piece, gap });
                }
                after = laid.after;
            }
            Flow::Table(table) => {
                let id = tables.len();
                let mut table = *table;
                let rows = std::mem::take(&mut table.rows);
                let gap = table.gap.max(after);
                after = table.after;
                if rows.is_empty() {
                    if !table.header.is_empty() {
                        units.push(Unit::Row {
                            table: id,
                            row: None,
                            gap,
                        });
                    }
                } else {
                    for (i, row) in rows.into_iter().enumerate() {
                        units.push(Unit::Row {
                            table: id,
                            row: Some(row),
                            gap: if i == 0 { gap } else { 0.0 },
                        });
                    }
                }
                tables.push(table);
            }
            Flow::PageBreak => {
                units.push(Unit::Break);
                after = 0.0;
            }
        }
    }
    let header_height = |table: &TableLaid| table.header.iter().map(|r| r.height).sum::<f32>();

    let mut pages = vec![Page::default()];
    let mut y = 0.0f32;
    let mut empty = true;
    // The table whose header rows are on the current page.
    let mut table_on_page: Option<usize> = None;
    let mut headers_tagged = vec![false; tables.len()];
    let mut queue: std::collections::VecDeque<Unit> = units.into();
    while let Some(unit) = queue.pop_front() {
        match unit {
            Unit::Break => {
                if !empty {
                    pages.push(Page::default());
                    y = 0.0;
                    empty = true;
                    table_on_page = None;
                }
            }
            Unit::Piece { piece, gap } => {
                let gap = if empty { 0.0 } else { gap };
                let mut need = gap + piece.height;
                if piece.keep_with_next {
                    need += keep_need(&queue, &tables, height);
                }
                if !empty && y + need > height + 0.01 {
                    pages.push(Page::default());
                    y = 0.0;
                    empty = true;
                    table_on_page = None;
                    queue.push_front(Unit::Piece { piece, gap });
                    continue;
                }
                y += gap;
                let page = pages.last_mut().expect("a page");
                if let Some(anchor) = &piece.anchor {
                    page.anchors.push((anchor.clone(), y));
                }
                for mut draw in piece.draws {
                    draw.translate(0.0, y);
                    page.draws.push(draw);
                }
                y += piece.height;
                empty = false;
                table_on_page = None;
            }
            Unit::Row { table, row, gap } => {
                let laid = &tables[table];
                let gap = if empty { 0.0 } else { gap };
                let headers = if table_on_page == Some(table) {
                    0.0
                } else {
                    header_height(laid)
                };
                let row_height = row.as_ref().map_or(0.0, |r| r.height);
                if !empty && y + gap + headers + row_height > height + 0.01 {
                    pages.push(Page::default());
                    y = 0.0;
                    empty = true;
                    table_on_page = None;
                    queue.push_front(Unit::Row { table, row, gap });
                    continue;
                }
                let mut y_here = y + gap;
                let page = pages.last_mut().expect("a page");
                if table_on_page != Some(table) {
                    let tagged = !headers_tagged[table];
                    headers_tagged[table] = true;
                    let count = laid.header.len();
                    for (i, header) in laid.header.iter().enumerate() {
                        for mut draw in row_draws(laid, header, true, i + 1 == count, header.height) {
                            if !tagged {
                                draw = draw.into_artifact(Furniture::Pagination);
                            }
                            draw.translate(0.0, y_here);
                            page.draws.push(draw);
                        }
                        y_here += header.height;
                    }
                    table_on_page = Some(table);
                }
                y = y_here;
                empty = false;
                let Some(row) = row else {
                    continue;
                };
                // Only a row taller than what an empty page leaves splits.
                let remaining = height - y_here;
                let (row, rest) = if row.height > remaining + 0.01 {
                    split_row(&row, remaining, laid.padding.0)
                } else {
                    (row, None)
                };
                for mut draw in row_draws(laid, &row, false, false, row.height) {
                    draw.translate(0.0, y_here);
                    page.draws.push(draw);
                }
                y = y_here + row.height;
                if let Some(rest) = rest {
                    pages.push(Page::default());
                    y = 0.0;
                    empty = true;
                    table_on_page = None;
                    queue.push_front(Unit::Row {
                        table,
                        row: Some(rest),
                        gap: 0.0,
                    });
                }
            }
        }
    }
    if pages.len() > 1 && pages.last().is_some_and(|p| p.draws.is_empty()) {
        pages.pop();
    }
    pages
}

/// The height the pieces kept with a piece need after it: the next
/// unit's, and the one after that while they keep too (at most a third
/// of a page, so keeping never pushes forever).
fn keep_need(queue: &std::collections::VecDeque<Unit>, tables: &[TableLaid], height: f32) -> f32 {
    let mut need = 0.0;
    for unit in queue.iter().take(3) {
        match unit {
            Unit::Piece { piece, gap } => {
                need += gap + piece.height;
                if !piece.keep_with_next {
                    break;
                }
            }
            Unit::Row { table, row, gap } => {
                let headers: f32 = tables[*table].header.iter().map(|r| r.height).sum();
                need += gap + headers + row.as_ref().map_or(0.0, |r| r.height);
                break;
            }
            Unit::Break => break,
        }
    }
    need.min(height / 3.0)
}

/// A paragraph's text without formatting (for the outline).
fn plain_text(paragraph: &Paragraph) -> String {
    let mut text = String::new();
    for run in &paragraph.runs {
        match &run.kind {
            RunKind::Text(t) => text.push_str(t),
            RunKind::LineBreak => text.push(' '),
            _ => {}
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 1 → a, 26 → z, 27 → aa.
fn alpha(mut number: usize) -> String {
    let mut letters = Vec::new();
    while number > 0 {
        number -= 1;
        letters.push((b'a' + (number % 26) as u8) as char);
        number /= 26;
    }
    letters.iter().rev().collect()
}

/// 1 → i, 4 → iv, 1994 → mcmxciv.
fn roman(mut number: usize) -> String {
    const TABLE: [(usize, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (value, letters) in TABLE {
        while number >= value {
            out.push_str(letters);
            number -= value;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::model::Paragraph;
    use crate::print::Margins;

    fn book() -> &'static FontBook {
        Box::leak(Box::new(FontBook::load(true, &[])))
    }

    fn texts(page: &Page) -> Vec<String> {
        let mut lines: Vec<(f32, f32, String)> = page
            .draws
            .iter()
            .filter_map(|draw| match draw {
                Draw::Text { x, baseline, run, .. } => Some((*baseline, *x, run.text.clone())),
                _ => None,
            })
            .collect();
        lines.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        lines.into_iter().map(|(_, _, t)| t).collect()
    }

    fn all_text(page: &Page) -> String {
        texts(page).join(" ")
    }

    #[test]
    fn markers_count_in_every_style() {
        assert_eq!(alpha(1), "a");
        assert_eq!(alpha(26), "z");
        assert_eq!(alpha(27), "aa");
        assert_eq!(roman(4), "iv");
        assert_eq!(roman(1994), "mcmxciv");
    }

    #[test]
    fn long_documents_flow_onto_more_pages() {
        let mut document = PdfDocument::new().margins(Margins::all(20.0));
        for i in 0..120 {
            document = document.paragraph(format!("Paragraph number {i} of the report."));
        }
        let laid = lay_out(&document, book(), "2026-10-27").unwrap();
        assert!(laid.pages.len() >= 3, "{}", laid.pages.len());
        // Nothing below the bottom margin, nothing lost.
        let bottom = laid.height - mm_to_pt(20.0);
        let mut seen = 0;
        for page in &laid.pages {
            for draw in &page.draws {
                if let Draw::Text { baseline, .. } = draw {
                    assert!(*baseline <= bottom, "{baseline} > {bottom}");
                    seen += 1;
                }
            }
        }
        assert_eq!(seen, 120, "every paragraph, one run each");
        assert!(all_text(&laid.pages[0]).starts_with("Paragraph number 0 "));
    }

    #[test]
    fn page_breaks_start_pages_and_headings_keep_with_what_follows() {
        let document = PdfDocument::new()
            .paragraph("First page")
            .page_break()
            .paragraph("Second page");
        let laid = lay_out(&document, book(), "").unwrap();
        assert_eq!(laid.pages.len(), 2);
        assert_eq!(all_text(&laid.pages[1]), "Second page");

        // Wherever the page ends, a heading is never its last line: it
        // shares a page with the paragraph after it.
        let mut moved = false;
        for count in 25..40 {
            let mut document = PdfDocument::new();
            for i in 0..count {
                document = document.paragraph(format!("Line {i}"));
            }
            let document = document
                .heading(2, "Findings")
                .paragraph("The first finding.");
            let laid = lay_out(&document, book(), "").unwrap();
            for page in &laid.pages {
                assert_ne!(texts(page).last().map(String::as_str), Some("Findings"));
            }
            let page = laid
                .pages
                .iter()
                .position(|p| texts(p).iter().any(|t| t == "Findings"))
                .unwrap();
            assert!(texts(&laid.pages[page]).iter().any(|t| t == "The first finding."));
            moved |= page > 0 && texts(&laid.pages[page])[0] == "Findings";
        }
        assert!(moved, "some count pushes the heading to the next page");
    }

    #[test]
    fn table_headers_repeat_on_every_page_and_rows_are_not_split() {
        let mut table = Table::new().header(["Control", "Status"]);
        for i in 0..90 {
            table = table.row([format!("Control {i}"), "Passed".to_owned()]);
        }
        let laid = lay_out(&PdfDocument::new().table(table), book(), "").unwrap();
        assert!(laid.pages.len() >= 2, "{}", laid.pages.len());
        for page in &laid.pages {
            let texts = texts(page);
            assert_eq!(texts[0], "Control", "{texts:?}");
            assert_eq!(texts[1], "Status");
        }
        // Each row once, in order.
        let rows: Vec<String> = laid
            .pages
            .iter()
            .flat_map(texts)
            .filter(|t| t.starts_with("Control "))
            .collect();
        assert_eq!(rows.len(), 90);
        assert_eq!(rows[89], "Control 89");
        // Only the first header is tagged content; repeats are artifacts.
        let marks: Vec<Mark> = laid
            .pages
            .iter()
            .map(|page| {
                page.draws
                    .iter()
                    .find_map(|d| match d {
                        Draw::Text { run, mark, .. } if run.text == "Control" => Some(*mark),
                        _ => None,
                    })
                    .unwrap()
            })
            .collect();
        assert!(matches!(marks[0], Mark::Content(_)));
        assert!(marks[1..].iter().all(|m| *m == Mark::Artifact(Furniture::Pagination)));
    }

    #[test]
    fn a_row_taller_than_a_page_splits_between_lines() {
        let tall: String = (0..140).map(|i| format!("Line {i}\n")).collect();
        let table = Table::new().header(["Notes"]).row([tall.as_str()]);
        let laid = lay_out(&PdfDocument::new().table(table), book(), "").unwrap();
        assert!(laid.pages.len() >= 2, "{}", laid.pages.len());
        let lines: Vec<String> = laid
            .pages
            .iter()
            .flat_map(texts)
            .filter(|t| t.starts_with("Line "))
            .collect();
        assert_eq!(lines.len(), 140);
        assert_eq!(texts(&laid.pages[1])[0], "Notes", "the header repeats");
    }

    #[test]
    fn headers_and_footers_sit_in_the_margins_with_page_numbers() {
        let mut document = PdfDocument::new()
            .title("Audit")
            .margins(Margins::all(20.0))
            .header(Paragraph::new().run(super::super::model::Run::title()).align(Align::End))
            .footer_page_numbers();
        for i in 0..100 {
            document = document.paragraph(format!("Paragraph {i}"));
        }
        let laid = lay_out(&document, book(), "").unwrap();
        let count = laid.pages.len();
        assert!(count >= 2);
        let top = mm_to_pt(20.0);
        let bottom = laid.height - mm_to_pt(20.0);
        for (index, page) in laid.pages.iter().enumerate() {
            let furniture: Vec<(f32, String, Mark)> = page
                .draws
                .iter()
                .filter_map(|draw| match draw {
                    Draw::Text { baseline, run, mark, .. }
                        if matches!(mark, Mark::Artifact(Furniture::Header | Furniture::Footer)) =>
                    {
                        Some((*baseline, run.text.clone(), *mark))
                    }
                    _ => None,
                })
                .collect();
            let header: Vec<&(f32, String, Mark)> = furniture
                .iter()
                .filter(|f| f.2 == Mark::Artifact(Furniture::Header))
                .collect();
            let footer: String = furniture
                .iter()
                .filter(|f| f.2 == Mark::Artifact(Furniture::Footer))
                .map(|f| f.1.as_str())
                .collect();
            assert_eq!(header[0].1, "Audit");
            assert!(header[0].0 < top, "the header is in the top margin");
            assert_eq!(footer, format!("Page {} of {count}", index + 1));
            for f in &furniture {
                if f.2 == Mark::Artifact(Furniture::Footer) {
                    assert!(f.0 > bottom, "the footer is in the bottom margin");
                }
            }
        }
    }

    #[test]
    fn a_tall_header_pushes_the_body_down() {
        let header = Paragraph::new().text("One\nTwo\nThree\nFour\nFive");
        let laid = lay_out(
            &PdfDocument::new()
                .margins(Margins::all(10.0))
                .header(header)
                .paragraph("Body"),
            book(),
            "",
        )
        .unwrap();
        let page = &laid.pages[0];
        let five = page
            .draws
            .iter()
            .find_map(|d| match d {
                Draw::Text { baseline, run, .. } if run.text == "Five" => Some(*baseline),
                _ => None,
            })
            .unwrap();
        let body = page
            .draws
            .iter()
            .find_map(|d| match d {
                Draw::Text { baseline, run, .. } if run.text == "Body" => Some(*baseline),
                _ => None,
            })
            .unwrap();
        assert!(body > five + FURNITURE_GAP, "{body} > {five}");
    }

    #[test]
    fn right_to_left_tables_start_on_the_right() {
        let document = PdfDocument::new()
            .locale("ar")
            .table(Table::new().header(["الضابط", "الحالة"]).row(["المصادقة", "ناجح"]));
        let laid = lay_out(&document, book(), "").unwrap();
        let x_of = |text: &str| {
            laid.pages[0]
                .draws
                .iter()
                .find_map(|d| match d {
                    Draw::Text { x, run, .. } if run.text == text => Some(*x),
                    _ => None,
                })
                .unwrap()
        };
        assert!(x_of("الضابط") > x_of("الحالة"));
    }

    #[test]
    fn lists_put_markers_before_their_items() {
        let document = PdfDocument::new().list(
            List::numbered()
                .item("Rotate the keys")
                .item_with("Restore a backup", List::bullets().item("Database")),
        );
        let laid = lay_out(&document, book(), "").unwrap();
        let page = &laid.pages[0];
        let find = |text: &str| {
            page.draws
                .iter()
                .find_map(|d| match d {
                    Draw::Text { x, baseline, run, .. } if run.text == text => Some((*x, *baseline)),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{text}: {:?}", texts(page)))
        };
        let (marker_x, marker_y) = find("1.");
        let (item_x, item_y) = find("Rotate the keys");
        assert!(marker_x < item_x && (marker_y - item_y).abs() < 0.01);
        let (bullet_x, _) = find("◦");
        let (nested_x, _) = find("Database");
        assert!(bullet_x > marker_x && nested_x > item_x);
        find("2.");
    }
}
