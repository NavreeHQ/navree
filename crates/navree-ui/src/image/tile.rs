//! A tile: several small blocks (a ring with its label, a sparkline with its figure, a
//! figure on its own) laid out in one picture at the size of a home-screen widget.

use crate::charts::{Gradient, Ink, IntoValue, NumberFormat, NumberLocale, Ring, Sparkline};
use crate::components::Tone;
use crate::node::{AttrValue, Element, Node, el};

use super::svg::num;

/// The size of a [`Tile`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TileSize {
    /// 474 by 474 pixels: a small home-screen widget, two blocks.
    Small,
    /// 1014 by 474 pixels: a medium one, four blocks side by side. The default.
    #[default]
    Medium,
    /// 1014 by 1014 pixels: a large one, up to six blocks.
    Large,
    /// Any other size, in pixels, each side 64 to 4096.
    Custom(u32, u32),
}

impl TileSize {
    /// Its width and height in pixels.
    pub const fn pixels(self) -> (u32, u32) {
        match self {
            Self::Small => (474, 474),
            Self::Medium => (1014, 474),
            Self::Large => (1014, 1014),
            Self::Custom(w, h) => (clamp_side(w), clamp_side(h)),
        }
    }

    /// The most blocks it draws: 2 small, 4 medium, 6 large, and for another size as
    /// many as fit at about 200 pixels a side, 1 to 12.
    pub const fn most_blocks(self) -> usize {
        match self {
            Self::Small => 2,
            Self::Medium => 4,
            Self::Large => 6,
            Self::Custom(..) => {
                let (w, h) = self.pixels();
                let n = (w / 200) * (h / 200);
                if n < 1 {
                    1
                } else if n > 12 {
                    12
                } else {
                    n as usize
                }
            }
        }
    }

    /// Its name: `small`, `medium`, `large`, or `custom`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::Custom(..) => "custom",
        }
    }

    /// The size called `name` (`small`, `medium` or `large`), as a URL names it.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "small" => Some(Self::Small),
            "medium" => Some(Self::Medium),
            "large" => Some(Self::Large),
            _ => None,
        }
    }
}

const fn clamp_side(side: u32) -> u32 {
    if side < 64 {
        64
    } else if side > super::MAX_SIDE {
        super::MAX_SIDE
    } else {
        side
    }
}

/// What a [`Block`] draws.
#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Ring {
        value: f64,
        max: f64,
    },
    Line {
        values: Vec<Option<f64>>,
        columns: bool,
    },
    Stat,
}

/// One block of a [`Tile`]: a ring, a sparkline or a figure, with its label, its figure
/// and a quieter note. Its colour is the picture's accent unless it names its own.
///
/// ```
/// use navree_ui::Tone;
/// use navree_ui::charts::Ink;
/// use navree_ui::image::Block;
///
/// let recovery = Block::ring("Recovery", 82, 100).unit("%");
/// let heart = Block::sparkline("Resting HR", [62, 61, 60, 59, 61, 58]).value("58").unit("bpm");
/// let sleep = Block::stat("Sleep", "7h 42m").note("+18 min vs 7-day avg").tone(Tone::Success);
/// let stress = Block::columns("Stress", [22, 35, 48, 30, 26]).ink(Ink::slot(4));
/// # let _ = (recovery, heart, sleep, stress);
/// ```
#[derive(Clone, Debug, PartialEq)]
#[must_use]
pub struct Block {
    kind: Kind,
    label: String,
    value: Option<String>,
    unit: Option<String>,
    note: Option<String>,
    tone: Tone,
    ink: Ink,
}

impl Block {
    fn of(kind: Kind, label: impl Into<String>) -> Self {
        Self {
            kind,
            label: label.into(),
            value: None,
            unit: None,
            note: None,
            tone: Tone::Neutral,
            ink: Ink::accent(),
        }
    }

    /// A ring filled to `value` of `max`, the figure in its middle and the label under it
    /// (beside it in a wide block). A value past `max` laps the ring.
    pub fn ring(label: impl Into<String>, value: impl IntoValue, max: impl IntoValue) -> Self {
        let value = value.into_value().unwrap_or(0.0);
        let max = max.into_value().filter(|m| *m > 0.0).unwrap_or(100.0);
        Self::of(Kind::Ring { value, max }, label)
    }

    /// A line over `values`, with a wash under it, below the label and the figure. With no
    /// [`value`](Self::value), the figure is the last value.
    pub fn sparkline<V: IntoValue>(
        label: impl Into<String>,
        values: impl IntoIterator<Item = V>,
    ) -> Self {
        let values = values.into_iter().map(IntoValue::into_value).collect();
        Self::of(
            Kind::Line {
                values,
                columns: false,
            },
            label,
        )
    }

    /// Columns over `values`, such as stress by the hour, below the label and the figure.
    pub fn columns<V: IntoValue>(
        label: impl Into<String>,
        values: impl IntoIterator<Item = V>,
    ) -> Self {
        let values = values.into_iter().map(IntoValue::into_value).collect();
        Self::of(
            Kind::Line {
                values,
                columns: true,
            },
            label,
        )
    }

    /// A figure on its own: the label above it and the note below.
    pub fn stat(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self::of(Kind::Stat, label).value(value)
    }

    /// The figure, as words: `7h 42m`, `8,432`, `₹1,20,000`. A ring's and a line's
    /// figure is otherwise its value.
    pub fn value(mut self, text: impl Into<String>) -> Self {
        self.value = Some(text.into());
        self
    }

    /// The unit after the figure, smaller: `%`, `bpm`, `kcal`.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// A quieter line under the figure: `+18 min vs 7-day avg`.
    pub fn note(mut self, text: impl Into<String>) -> Self {
        self.note = Some(text.into());
        self
    }

    /// The note's colour: [`Tone::Success`] for good news, [`Tone::Danger`] for bad.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// The colour of the ring, line or columns: a chart colour, a tone. The picture's
    /// accent by default.
    pub fn ink(mut self, ink: Ink) -> Self {
        self.ink = ink;
        self
    }

    /// The figure's words: the value given, or the value formatted.
    fn figure(&self) -> String {
        if let Some(text) = &self.value {
            return text.clone();
        }
        let format = NumberFormat::default();
        let locale = NumberLocale::of("en-IN");
        match &self.kind {
            Kind::Ring { value, .. } => format.format(*value, &locale),
            Kind::Line { values, .. } => values
                .iter()
                .rev()
                .find_map(|v| *v)
                .map(|v| format.format(v, &locale))
                .unwrap_or_default(),
            Kind::Stat => String::new(),
        }
    }
}

/// Several blocks in one picture at the size of a home-screen widget: what
/// [`ChartImage::for_tile`](super::ChartImage::for_tile) draws.
///
/// The blocks are laid out in the grid that keeps each nearest to square (two small
/// ones one above the other, four medium ones side by side, four large ones two by two),
/// on a card, with a title and a note above them when given. Every word is set in
/// SVG, so the whole tile is one drawing.
///
/// ```
/// use navree_ui::image::{Block, ChartImage, Tile, TileSize};
/// use navree_ui::theme::{Mode, Rgb};
///
/// let tile = Tile::new(TileSize::Medium)
///     .title("Today")
///     .note("Updated 07:30")
///     .block(Block::ring("Recovery", 82, 100).unit("%"))
///     .block(Block::sparkline("Resting HR", [62, 61, 60, 59, 61, 58]).unit("bpm"))
///     .block(Block::stat("Sleep", "7h 42m").note("+18 min"));
/// let png = ChartImage::for_tile(&tile)
///     .mode(Mode::Dark)
///     .accent(Rgb::hex(0x22d3ee))
///     .png(&tile.clone().into())
///     .unwrap();
/// assert_eq!((png.width(), png.height()), (1014, 474));
/// ```
#[derive(Clone, Debug, PartialEq)]
#[must_use]
pub struct Tile {
    size: TileSize,
    title: Option<String>,
    note: Option<String>,
    blocks: Vec<Block>,
}

impl Tile {
    /// An empty tile of `size`.
    pub fn new(size: TileSize) -> Self {
        Self {
            size,
            title: None,
            note: None,
            blocks: Vec::new(),
        }
    }

    /// Its size.
    pub fn size(&self) -> TileSize {
        self.size
    }

    /// A title above the blocks: `Today`, a name.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// A quieter note across from the title: `Updated 07:30`.
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Adds a block. Blocks past the size's [`most_blocks`](TileSize::most_blocks) aren't
    /// drawn.
    pub fn block(mut self, block: Block) -> Self {
        self.blocks.push(block);
        self
    }

    /// Adds blocks, in order.
    pub fn blocks(mut self, blocks: impl IntoIterator<Item = Block>) -> Self {
        self.blocks.extend(blocks);
        self
    }

    fn into_node(self) -> Node {
        let (width, height) = self.size.pixels();
        let (w, h) = (f64::from(width), f64::from(height));
        let short = w.min(h);
        let pad = (short * 0.07).round();
        let gap = (short * 0.035).round();
        let radius = short * 0.038;
        let mut children: Vec<Node> = Vec::new();
        let mut top = pad;
        if self.title.is_some() || self.note.is_some() {
            let size = (short * 0.05).clamp(14.0, 34.0);
            let baseline = top + size * 0.8;
            if let Some(title) = &self.title {
                let room = if self.note.is_some() {
                    (w - 2.0 * pad) * 0.6
                } else {
                    w - 2.0 * pad
                };
                children.push(
                    text(
                        pad,
                        baseline,
                        size,
                        600,
                        "fg",
                        "start",
                        fit(title, size, room),
                    )
                    .into(),
                );
            }
            if let Some(note) = &self.note {
                let room = if self.title.is_some() {
                    (w - 2.0 * pad) * 0.38
                } else {
                    w - 2.0 * pad
                };
                children.push(
                    text(
                        w - pad,
                        baseline,
                        size * 0.82,
                        500,
                        "fg-3",
                        "end",
                        fit(note, size * 0.82, room),
                    )
                    .into(),
                );
            }
            top += size + gap;
        }
        let blocks: Vec<&Block> = self.blocks.iter().take(self.size.most_blocks()).collect();
        let area = (pad, top, w - 2.0 * pad, h - top - pad);
        if blocks.is_empty() {
            children.push(
                text(
                    w / 2.0,
                    top + area.3 / 2.0,
                    (short * 0.05).clamp(14.0, 30.0),
                    500,
                    "fg-3",
                    "middle",
                    "No blocks yet".to_owned(),
                )
                .into(),
            );
        }
        let (cols, rows) = grid(blocks.len(), area.2, area.3, gap);
        let cw = (area.2 - gap * (cols as f64 - 1.0)) / cols as f64;
        let ch = (area.3 - gap * (rows as f64 - 1.0)) / rows as f64;
        for (at, block) in blocks.iter().enumerate() {
            let (col, row) = (at % cols, at / cols);
            let x = area.0 + col as f64 * (cw + gap);
            let y = area.1 + row as f64 * (ch + gap);
            children.push(
                el("rect")
                    .attr("x", num(x))
                    .attr("y", num(y))
                    .attr("width", num(cw))
                    .attr("height", num(ch))
                    .attr("rx", num(radius))
                    .attr("fill", "var(--ink-2)")
                    .into(),
            );
            children.extend(draw_block(block, at, (x, y, cw, ch), short));
        }
        el("svg")
            .attr("width", i64::from(width))
            .attr("height", i64::from(height))
            .attr("viewBox", format!("0 0 {width} {height}"))
            .attr("font-family", "Geist")
            .slot("image-tile")
            .attr("data-size", self.size.name())
            .child(children)
            .into()
    }
}

impl From<Tile> for Node {
    fn from(tile: Tile) -> Self {
        tile.into_node()
    }
}

/// The columns and rows for `n` blocks in a `w` by `h` area: the grid whose cells are
/// nearest to square, fewer columns on a tie.
pub(super) fn grid(n: usize, w: f64, h: f64, gap: f64) -> (usize, usize) {
    let n = n.max(1);
    let mut best = (1, n);
    let mut score = f64::INFINITY;
    for cols in 1..=n {
        let rows = n.div_ceil(cols);
        // A grid with a whole empty row or column is never the nearest.
        if (cols - 1) * rows >= n && cols > 1 {
            continue;
        }
        let cw = (w - gap * (cols as f64 - 1.0)) / cols as f64;
        let ch = (h - gap * (rows as f64 - 1.0)) / rows as f64;
        if cw <= 0.0 || ch <= 0.0 {
            continue;
        }
        let off = (cw / ch).ln().abs();
        if off < score - 1e-9 {
            score = off;
            best = (cols, rows);
        }
    }
    best
}

/// How wide `text` is at `size`, about: Geist's figures and lower case run near 0.56 em.
fn width_of(text: &str, size: f64) -> f64 {
    text.chars()
        .map(|c| match c {
            'i' | 'l' | 'j' | '.' | ',' | ':' | ';' | '\'' | '|' | '!' | ' ' => 0.3,
            'm' | 'w' | 'M' | 'W' | '%' | '@' => 0.85,
            c if c.is_ascii_uppercase() => 0.68,
            _ => 0.53,
        })
        .sum::<f64>()
        * size
}

/// `text` cut with an ellipsis to fit `room` at `size`.
fn fit(text: &str, size: f64, room: f64) -> String {
    if width_of(text, size) <= room {
        return text.to_owned();
    }
    let mut out = String::new();
    for c in text.chars() {
        let next = format!("{out}{c}\u{2026}");
        if width_of(&next, size) > room {
            break;
        }
        out.push(c);
    }
    format!("{}\u{2026}", out.trim_end())
}

/// A line of words at its baseline, in a token's colour.
fn text(
    x: f64,
    baseline: f64,
    size: f64,
    weight: u16,
    token: &str,
    anchor: &'static str,
    words: String,
) -> Element {
    el("text")
        .attr("x", num(x))
        .attr("y", num(baseline))
        .attr("font-size", num(size))
        .attr("font-weight", i64::from(weight))
        .attr("fill", format!("var(--{token})"))
        .attr("text-anchor", anchor)
        .child(words)
}

/// A figure with its unit after it, smaller and quieter, sized down to fit `room`.
fn figure(
    x: f64,
    baseline: f64,
    size: f64,
    anchor: &'static str,
    value: &str,
    unit: Option<&str>,
    room: f64,
) -> (Element, f64) {
    let unit_text = unit.map(|u| format!("\u{2009}{u}")).unwrap_or_default();
    let wide = width_of(value, size) + width_of(&unit_text, size * 0.48);
    let size = if wide > room && wide > 0.0 {
        size * room / wide
    } else {
        size
    };
    let mut words = text(x, baseline, size, 700, "fg", anchor, value.to_owned())
        .attr("letter-spacing", num(-size * 0.02));
    if let Some(unit) = unit {
        words = words.child(
            el("tspan")
                .attr("font-size", num(size * 0.48))
                .attr("font-weight", "600")
                .attr("fill", "var(--fg-2)")
                .attr("letter-spacing", "0")
                .child(format!("\u{2009}{unit}")),
        );
    }
    (words, size)
}

/// A small label in capitals, spaced as the rings' labels are on a page.
fn label(
    x: f64,
    baseline: f64,
    size: f64,
    anchor: &'static str,
    words: &str,
    room: f64,
) -> Element {
    let upper = words.to_uppercase();
    text(
        x,
        baseline,
        size,
        600,
        "fg-2",
        anchor,
        fit(&upper, size * 1.1, room),
    )
    .attr("letter-spacing", num(size * 0.08))
}

/// The note under a figure, in its tone's colour.
fn note(
    x: f64,
    baseline: f64,
    size: f64,
    anchor: &'static str,
    block: &Block,
    room: f64,
) -> Option<Element> {
    let words = block.note.as_deref()?;
    let token = match block.tone.value() {
        Some("success" | "up") => "success",
        Some("danger" | "down") => "danger",
        Some("warning") => "amber",
        Some("info") => "info",
        Some("violet") => "violet",
        Some("cyan") => "cyan",
        Some("rose") => "rose",
        _ => "fg-3",
    };
    Some(text(
        x,
        baseline,
        size,
        500,
        token,
        anchor,
        fit(words, size, room),
    ))
}

/// The first drawing in `node`, with the colour the HTML around it names, placed at
/// `x`, `y`, `w` by `h`.
fn drawing_of(node: &Node, place: (f64, f64, f64, f64)) -> Option<Node> {
    fn find(node: &Node, color: Option<(&'static str, AttrValue)>) -> Option<Element> {
        match node {
            Node::Element(element) => {
                let own = element
                    .attrs
                    .iter()
                    .find(|a| a.name == "data-color" || a.name == "data-tone")
                    .map(|a| (a.name, a.value.clone()));
                let color = own.or(color);
                if element.tag == "svg" && !element.children.is_empty() {
                    let mut svg = element.clone();
                    if let Some((name, value)) = color
                        && !svg
                            .attrs
                            .iter()
                            .any(|a| a.name == "data-color" || a.name == "data-tone")
                    {
                        svg.attrs.push(crate::node::Attr { name, value });
                    }
                    return Some(svg);
                }
                element
                    .children
                    .iter()
                    .find_map(|child| find(child, color.clone()))
            }
            Node::Fragment(nodes) => nodes.iter().find_map(|child| find(child, color.clone())),
            _ => None,
        }
    }
    let mut svg = find(node, None)?;
    let (x, y, w, h) = place;
    svg.attrs
        .retain(|a| !matches!(a.name, "x" | "y" | "width" | "height"));
    for (name, value) in [("x", x), ("y", y), ("width", w), ("height", h)] {
        svg.attrs.push(crate::node::Attr {
            name,
            value: AttrValue::Text(num(value).into_boxed_str()),
        });
    }
    Some(Node::Element(svg))
}

/// What one block draws in its cell.
fn draw_block(block: &Block, at: usize, cell: (f64, f64, f64, f64), short: f64) -> Vec<Node> {
    let (x, y, w, h) = cell;
    let inset = (w.min(h) * 0.09).clamp(10.0, 40.0);
    let small = (short * 0.034).clamp(11.0, 26.0);
    let mut out: Vec<Node> = Vec::new();
    let value = block.figure();
    let unit = block.unit.as_deref();
    let id = format!("tile-block-{at}");
    match &block.kind {
        Kind::Ring { value: v, max } => {
            let wide = w >= h * 1.3;
            let d = if wide {
                (h - 2.0 * inset).min(w * 0.45)
            } else {
                (w - 2.0 * inset).min(h * 0.6)
            }
            .max(24.0);
            let size = d.round().max(24.0);
            let ring = Ring::new(block.label.clone(), *v)
                .id(id)
                .max(*max)
                .size(size as u32)
                .thickness((size * 0.09).max(3.0))
                .ink(block.ink)
                .fill(Gradient::sheen(block.ink));
            let (rx, ry) = if wide {
                (x + inset, y + (h - size) / 2.0)
            } else {
                (x + (w - size) / 2.0, y + inset)
            };
            if let Some(drawing) = drawing_of(&ring.into(), (rx, ry, size, size)) {
                out.push(drawing);
            }
            if wide {
                let tx = rx + size + inset * 0.8;
                let room = x + w - inset - tx;
                let label_size = small;
                let (fig, fig_size) = figure(tx, 0.0, size * 0.34, "start", &value, unit, room);
                let total = label_size * 1.5
                    + fig_size * 1.05
                    + block.note.as_ref().map_or(0.0, |_| small * 1.6);
                let mut line = y + (h - total) / 2.0 + label_size;
                out.push(label(tx, line, label_size, "start", &block.label, room).into());
                line += fig_size * 1.05;
                out.push(set_y(fig, line).into());
                if let Some(n) = note(tx, line + small * 1.5, small, "start", block, room) {
                    out.push(n.into());
                }
            } else {
                let cx = x + w / 2.0;
                let room = size * 0.72;
                let (fig, fig_size) = figure(cx, 0.0, size * 0.26, "middle", &value, unit, room);
                out.push(set_y(fig, ry + size / 2.0 + fig_size * 0.36).into());
                let mut line = ry + size + inset * 0.6 + small;
                out.push(label(cx, line, small, "middle", &block.label, w - 2.0 * inset).into());
                line += small * 1.55;
                if let Some(n) = note(cx, line, small * 0.95, "middle", block, w - 2.0 * inset) {
                    out.push(n.into());
                }
            }
        }
        Kind::Line { values, columns } => {
            let room = w - 2.0 * inset;
            let mut line = y + inset + small;
            out.push(label(x + inset, line, small, "start", &block.label, room).into());
            let (fig, fig_size) = figure(
                x + inset,
                0.0,
                (h * 0.2).min(short * 0.11),
                "start",
                &value,
                unit,
                room,
            );
            line += fig_size * 1.1;
            out.push(set_y(fig, line).into());
            if let Some(n) = note(
                x + inset,
                line + small * 1.45,
                small * 0.95,
                "start",
                block,
                room,
            ) {
                out.push(n.into());
                line += small * 1.45;
            }
            let top = line + inset * 0.6;
            let bottom = y + h - inset;
            if bottom - top >= 12.0 {
                let plot_h = (bottom - top).round();
                let mut spark = Sparkline::new(block.label.clone(), values.clone())
                    .id(id)
                    .height(plot_h as u32)
                    .ink(block.ink);
                spark = if *columns {
                    spark.columns()
                } else {
                    spark.smooth().fade()
                };
                if let Some(drawing) = drawing_of(&spark.into(), (x + inset, top, room, plot_h)) {
                    out.push(drawing);
                }
            }
        }
        Kind::Stat => {
            let room = w - 2.0 * inset;
            let fig_size = (h * 0.28).min(short * 0.13);
            let total =
                small * 1.6 + fig_size * 1.05 + block.note.as_ref().map_or(0.0, |_| small * 1.6);
            let mut line = y + ((h - total) / 2.0).max(inset) + small;
            out.push(label(x + inset, line, small, "start", &block.label, room).into());
            let (fig, fig_size) = figure(x + inset, 0.0, fig_size, "start", &value, unit, room);
            line += fig_size * 1.1;
            out.push(set_y(fig, line).into());
            if let Some(n) = note(x + inset, line + small * 1.6, small, "start", block, room) {
                out.push(n.into());
            }
        }
    }
    out
}

/// `text` with its baseline moved to `y`.
fn set_y(mut text: Element, y: f64) -> Element {
    for a in &mut text.attrs {
        if a.name == "y" {
            a.value = AttrValue::Text(num(y).into_boxed_str());
        }
    }
    text
}
