//! HTML (the documented subset) to a [`PdfDocument`]: block elements to
//! blocks, inline elements to formatted runs, a few inline styles, and
//! `@page` from `<style>`.

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use bytes::Bytes;

use super::css::{self, PageRule};
use super::html::{self, Element, Node};
use super::model::{
    Align, Block, Borders, Cell, Direction, Image, List, ListKind, Paragraph, PdfDocument, Row,
    Run, RunKind, Table, TextStyle, VAlign,
};

/// Fetches an image by its `src` (relative to the document): its bytes
/// and the path to name in errors, or `None` when there is none.
pub(crate) type ImageSource<'a> = dyn FnMut(&str) -> Option<(Bytes, String)> + 'a;

/// A converted document and what its stylesheets say about the page.
pub(crate) struct Converted {
    pub(crate) document: PdfDocument,
    pub(crate) page: PageRule,
    /// The `dir` of `<html>` or `<body>`, when given.
    pub(crate) direction: Option<Direction>,
    /// The `lang` of `<html>`, when given.
    pub(crate) lang: Option<String>,
}

/// Converts `html`; `images` fetches what `<img src>` names.
pub(crate) fn convert(html: &str, images: &mut ImageSource<'_>) -> Converted {
    let tree = html::parse(html);
    let mut document = PdfDocument::new();
    let mut stylesheet = String::new();
    collect_head(&tree, &mut document, &mut stylesheet);
    let root = tree.find("html");
    let body = tree.find("body");
    let lang = root
        .and_then(|e| e.attr("lang"))
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_owned());
    let direction = [body, root]
        .into_iter()
        .flatten()
        .find_map(|e| direction_attr(e.attr("dir")?));
    // Styles of <html> and <body> are the document's.
    let mut state = State::default();
    for element in [root, body].into_iter().flatten() {
        state = state.enter(element, true);
    }
    // The styles of <html> and <body> are the document's base, under the
    // engine's defaults for headings, headers and footers.
    document.style = std::mem::take(&mut state.block);
    let mut walker = Walker {
        images,
        out: Vec::new(),
        para: Paragraph::new(),
        para_state: None,
    };
    let content: &[Node] = match body.or(root) {
        Some(element) => &element.children,
        None => &tree.children,
    };
    let mut header = Vec::new();
    let mut footer = Vec::new();
    for node in content {
        match node {
            Node::Element(e) if e.name == "header" || e.name == "footer" => {
                let mut furniture = state.enter(e, true);
                furniture.furniture = true;
                let blocks = walker.collect(&e.children, &furniture);
                if e.name == "header" {
                    header.extend(blocks);
                } else {
                    footer.extend(blocks);
                }
            }
            node => walker.node(node, &state),
        }
    }
    walker.flush();
    document.blocks = walker.out;
    document.header = header;
    document.footer = footer;
    Converted {
        document,
        page: css::page_rules(&stylesheet),
        direction,
        lang,
    }
}

/// `<title>`, `<meta name="author">` and every `<style>`.
fn collect_head(element: &Element, document: &mut PdfDocument, stylesheet: &mut String) {
    for node in &element.children {
        let Node::Element(child) = node else {
            continue;
        };
        match child.name.as_str() {
            "title" => {
                let title = collapse(&child.text());
                if !title.is_empty() && document.title.is_none() {
                    document.title = Some(title);
                }
            }
            "meta" => {
                if child
                    .attr("name")
                    .is_some_and(|n| n.eq_ignore_ascii_case("author"))
                    && let Some(author) = child.attr("content")
                {
                    document.author = Some(author.trim().to_owned());
                }
            }
            "style" => {
                stylesheet.push_str(&child.text());
                stylesheet.push('\n');
            }
            "body" | "svg" | "template" => {}
            _ => collect_head(child, document, stylesheet),
        }
    }
}

/// What an element passes to its content.
#[derive(Clone, Debug, Default)]
struct State {
    /// Style from block elements: a paragraph's own style.
    block: TextStyle,
    /// Style from inline elements since the paragraph's start: its runs'.
    inline: TextStyle,
    /// The computed text size, for relative sizes.
    size: Option<f32>,
    align: Option<Align>,
    direction: Option<Direction>,
    link: Option<String>,
    preformatted: bool,
    /// Inside a header or footer: page-number spans count.
    furniture: bool,
}

impl State {
    fn size(&self) -> f32 {
        self.size.unwrap_or(11.0)
    }

    /// The state inside `element`; `block` for block elements.
    fn enter(&self, element: &Element, block: bool) -> State {
        let mut state = self.clone();
        if block {
            state.block = state.inline.over(&state.block);
            state.inline = TextStyle::default();
        }
        let mut own = TextStyle::default();
        match element.name.as_str() {
            "b" | "strong" => own.bold = Some(true),
            "i" | "em" | "cite" | "var" | "dfn" => own.italic = Some(true),
            "u" | "ins" => own.underline = Some(true),
            "s" | "del" | "strike" => own.strike = Some(true),
            "code" | "kbd" | "samp" | "tt" | "pre" => {
                own.font = Some(Arc::from(vec!["Noto Sans Mono".to_owned()]));
            }
            "small" => own.size = Some(state.size() * 0.85),
            "big" => own.size = Some(state.size() * 1.2),
            "a" => {
                if let Some(href) = element.attr("href").filter(|h| !h.trim().is_empty()) {
                    state.link = Some(href.trim().to_owned());
                }
            }
            "center" => state.align = Some(Align::Center),
            "th" => own.bold = Some(true),
            _ => {}
        }
        if let Some(align) = element.attr("align").and_then(align_value) {
            state.align = Some(align);
        }
        if let Some(direction) = element.attr("dir").and_then(direction_attr) {
            state.direction = Some(direction);
        }
        if let Some(style) = element.attr("style") {
            for (property, value) in css::declarations(style) {
                match property.as_str() {
                    "font-weight" => {
                        own.bold = match value.to_ascii_lowercase().as_str() {
                            "bold" | "bolder" => Some(true),
                            "normal" | "lighter" => Some(false),
                            number => number.parse::<u16>().ok().map(|w| w >= 600),
                        }
                    }
                    "font-style" => {
                        own.italic = Some(matches!(
                            value.to_ascii_lowercase().as_str(),
                            "italic" | "oblique"
                        ));
                    }
                    "color" => {
                        if let Some(color) = css::color(&value) {
                            own.color = Some(color);
                        }
                    }
                    "font-size" => {
                        if let Some(size) = css::font_size(&value, state.size()) {
                            own.size = Some(size);
                        }
                    }
                    "font-family" => {
                        let families = css::families(&value);
                        if !families.is_empty() {
                            own.font = Some(Arc::from(families));
                        }
                    }
                    "text-align" => {
                        if let Some(align) = align_value(&value) {
                            state.align = Some(align);
                        }
                    }
                    "text-decoration" | "text-decoration-line" => {
                        let value = value.to_ascii_lowercase();
                        own.underline = Some(value.contains("underline"));
                        own.strike = Some(value.contains("line-through"));
                    }
                    "line-height" => {
                        own.line_height = match value.trim() {
                            "normal" => Some(1.4),
                            number if number.parse::<f32>().is_ok() => number.parse().ok(),
                            other => css::length(other, state.size(), Some(state.size()))
                                .map(|pt| pt / state.size()),
                        };
                    }
                    "direction" => {
                        if let Some(direction) = direction_attr(&value) {
                            state.direction = Some(direction);
                        }
                    }
                    "white-space" => {
                        state.preformatted =
                            matches!(value.trim(), "pre" | "pre-wrap" | "break-spaces");
                    }
                    _ => {}
                }
            }
        }
        if element.name == "pre" {
            state.preformatted = true;
        }
        if let Some(size) = own.size {
            state.size = Some(size);
        }
        if block {
            state.block = own.over(&state.block);
        } else {
            state.inline = own.over(&state.inline);
        }
        state
    }
}

fn align_value(value: &str) -> Option<Align> {
    Some(match value.trim().to_ascii_lowercase().as_str() {
        "left" => Align::Left,
        "right" => Align::Right,
        "center" | "middle" => Align::Center,
        "justify" => Align::Justify,
        "start" => Align::Start,
        "end" => Align::End,
        _ => return None,
    })
}

fn direction_attr(value: &str) -> Option<Direction> {
    match value.trim().to_ascii_lowercase().as_str() {
        "rtl" => Some(Direction::Rtl),
        "ltr" => Some(Direction::Ltr),
        _ => None,
    }
}

/// Elements that are not printed.
const SKIPPED: [&str; 14] = [
    "head", "style", "script", "template", "noscript", "svg", "canvas", "iframe", "object",
    "video", "audio", "input", "select", "button",
];

/// Inline elements: their content joins the paragraph around them.
const INLINE: [&str; 27] = [
    "a", "abbr", "b", "bdi", "bdo", "big", "cite", "code", "data", "del", "dfn", "em", "font", "i",
    "ins", "kbd", "label", "mark", "q", "s", "samp", "small", "span", "strike", "strong", "time",
    "u",
];

struct Walker<'a, 'i> {
    images: &'a mut ImageSource<'i>,
    out: Vec<Block>,
    /// The paragraph inline content is collected into.
    para: Paragraph,
    /// The state of the element the pending paragraph belongs to.
    para_state: Option<State>,
}

impl Walker<'_, '_> {
    /// The blocks `nodes` make, apart from the main output.
    fn collect(&mut self, nodes: &[Node], state: &State) -> Vec<Block> {
        self.flush();
        let saved = std::mem::take(&mut self.out);
        for node in nodes {
            self.node(node, state);
        }
        self.flush();
        std::mem::replace(&mut self.out, saved)
    }

    fn node(&mut self, node: &Node, state: &State) {
        match node {
            Node::Text(text) => self.text(text, state),
            Node::Element(element) => self.element(element, state),
        }
    }

    fn element(&mut self, element: &Element, state: &State) {
        let name = element.name.as_str();
        if SKIPPED.contains(&name) {
            return;
        }
        let style = element
            .attr("style")
            .map(css::declarations)
            .unwrap_or_default();
        let declared = |properties: &[&str], values: &[&str]| {
            style.iter().any(|(p, v)| {
                properties.contains(&p.as_str())
                    && values.contains(&v.trim().to_ascii_lowercase().as_str())
            })
        };
        let break_before = declared(
            &["break-before", "page-break-before"],
            &["page", "always", "left", "right"],
        );
        let break_after = element.has_class("page-break")
            || declared(
                &["break-after", "page-break-after"],
                &["page", "always", "left", "right"],
            );
        if break_before {
            self.flush();
            self.out.push(Block::PageBreak);
        }
        if INLINE.contains(&name) {
            let inner = state.enter(element, false);
            if inner.furniture
                && let Some(kind) = placeholder(element)
            {
                let mut run = Run::new("");
                run.kind = kind;
                run.style = inner.inline.clone();
                self.start_paragraph(state);
                self.para.runs.push(run);
            } else {
                for child in &element.children {
                    self.node(child, &inner);
                }
            }
        } else {
            match name {
                "br" => {
                    self.start_paragraph(state);
                    let mut run = Run::line_break();
                    run.style = state.inline.clone();
                    self.para.runs.push(run);
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    self.flush();
                    let level = name[1..].parse::<u8>().unwrap_or(1);
                    let mut inner = state.enter(element, true);
                    // The heading's size is its level's unless it sets one.
                    let own_size = element.attr("style").is_some_and(|s| {
                        css::declarations(s).iter().any(|(p, _)| p == "font-size")
                    });
                    if !own_size {
                        inner.block.size = None;
                    }
                    for child in &element.children {
                        self.node(child, &inner);
                    }
                    if let Some(paragraph) = self.take_paragraph() {
                        self.out.push(Block::Heading(level, paragraph));
                    }
                }
                "ul" | "ol" => {
                    self.flush();
                    let inner = state.enter(element, true);
                    let list = self.list(element, &inner);
                    if !list.items.is_empty() {
                        self.out.push(Block::List(list));
                    }
                }
                "table" => {
                    self.flush();
                    let inner = state.enter(element, true);
                    self.table(element, &inner);
                }
                "img" => {
                    self.flush();
                    if let Some(image) = self.image(element, state) {
                        self.out.push(Block::Image(image));
                    }
                }
                "hr" => {
                    self.flush();
                    if !break_after {
                        self.out.push(Block::Rule);
                    }
                }
                _ => {
                    // A block container: p, div, section, li content, td…
                    self.flush();
                    let inner = state.enter(element, true);
                    for child in &element.children {
                        self.node(child, &inner);
                    }
                    self.flush();
                }
            }
        }
        if break_after {
            self.flush();
            self.out.push(Block::PageBreak);
        }
    }

    /// Text, its whitespace collapsed unless preformatted.
    fn text(&mut self, text: &str, state: &State) {
        if text.is_empty() {
            return;
        }
        let collapsed = if state.preformatted {
            text.to_owned()
        } else {
            let mut space = self.ends_in_space();
            let mut out = String::with_capacity(text.len());
            for c in text.chars() {
                if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
                    if !space {
                        out.push(' ');
                        space = true;
                    }
                } else {
                    out.push(c);
                    space = false;
                }
            }
            out
        };
        if collapsed.is_empty() {
            return;
        }
        self.start_paragraph(state);
        let mut run = Run::new(collapsed);
        run.style = state.inline.clone();
        run.link = state.link.clone();
        self.para.runs.push(run);
    }

    /// Whether the pending paragraph is empty or ends in a space or a
    /// line break (so leading whitespace collapses away).
    fn ends_in_space(&self) -> bool {
        for run in self.para.runs.iter().rev() {
            match &run.kind {
                RunKind::Text(text) if text.is_empty() => continue,
                RunKind::Text(text) => return text.ends_with(' '),
                RunKind::LineBreak => return true,
                _ => return false,
            }
        }
        true
    }

    fn start_paragraph(&mut self, state: &State) {
        if self.para_state.is_none() {
            self.para_state = Some(state.clone());
        }
    }

    /// The pending paragraph, trailing spaces trimmed, or `None` when it
    /// prints nothing.
    fn take_paragraph(&mut self) -> Option<Paragraph> {
        let state = self.para_state.take()?;
        let mut paragraph = std::mem::take(&mut self.para);
        if !state.preformatted {
            for run in paragraph.runs.iter_mut().rev() {
                match &mut run.kind {
                    RunKind::Text(text) => {
                        let trimmed = text.trim_end_matches(' ').len();
                        text.truncate(trimmed);
                        if !text.is_empty() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            paragraph
                .runs
                .retain(|run| !matches!(&run.kind, RunKind::Text(t) if t.is_empty()));
        }
        paragraph.style = state.block.clone();
        paragraph.align = state.align;
        paragraph.direction = state.direction;
        paragraph.preformatted = state.preformatted;
        if paragraph.runs.is_empty() || paragraph.is_empty() && !has_placeholder(&paragraph) {
            return None;
        }
        Some(paragraph)
    }

    fn flush(&mut self) {
        if let Some(paragraph) = self.take_paragraph() {
            self.out.push(Block::Paragraph(paragraph));
        }
    }

    fn list(&mut self, element: &Element, state: &State) -> List {
        let kind = if element.name == "ol" {
            match element.attr("type").map(str::trim) {
                Some("a") => ListKind::LowerAlpha,
                Some("A") => ListKind::UpperAlpha,
                Some("i") => ListKind::LowerRoman,
                Some("I") => ListKind::UpperRoman,
                _ => ListKind::Decimal,
            }
        } else {
            ListKind::Bullets
        };
        let mut list = List::new(kind);
        if let Some(start) = element
            .attr("start")
            .and_then(|s| s.trim().parse::<u32>().ok())
        {
            list = list.start(start);
        }
        for node in &element.children {
            match node {
                Node::Element(li) if li.name == "li" => {
                    let inner = state.enter(li, true);
                    let blocks = self.collect(&li.children, &inner);
                    list = list.item_blocks(blocks);
                }
                // Loose content: an item of its own.
                Node::Element(other) => {
                    let blocks = self.collect(std::slice::from_ref(node), state);
                    if !blocks.is_empty() && other.name != "script" {
                        list = list.item_blocks(blocks);
                    }
                }
                Node::Text(text) if !text.trim().is_empty() => {
                    let blocks = self.collect(std::slice::from_ref(node), state);
                    list = list.item_blocks(blocks);
                }
                Node::Text(_) => {}
            }
        }
        list
    }

    fn table(&mut self, element: &Element, state: &State) {
        let mut table = Table::new();
        table.style = TextStyle::default();
        match element.attr("border").map(str::trim) {
            Some("0") => table.borders = Borders::None,
            Some(_) => table.borders = Borders::Grid,
            None => {}
        }
        if element.attr("role") == Some("presentation") {
            table.borders = Borders::None;
            table.header_background = None;
        }
        if let Some(padding) = element
            .attr("cellpadding")
            .and_then(|p| css::length(p, state.size(), None))
        {
            table.padding = (padding, padding);
        }
        let mut widths: Vec<Option<f32>> = Vec::new();
        let mut caption = None;
        for node in &element.children {
            let Node::Element(child) = node else {
                continue;
            };
            match child.name.as_str() {
                "caption" => {
                    let mut inner = state.enter(child, true);
                    inner.align = inner.align.or(Some(Align::Center));
                    caption = Some(self.collect(&child.children, &inner));
                }
                "colgroup" => {
                    for col in child.children.iter().filter_map(|n| match n {
                        Node::Element(e) if e.name == "col" => Some(e),
                        _ => None,
                    }) {
                        widths.push(percent(col));
                    }
                }
                "col" => widths.push(percent(child)),
                "thead" | "tbody" | "tfoot" => {
                    let section = state.enter(child, true);
                    for row in child.children.iter().filter_map(|n| match n {
                        Node::Element(e) if e.name == "tr" => Some(e),
                        _ => None,
                    }) {
                        let laid = self.row(row, &section);
                        table.push_row(laid, child.name == "thead");
                    }
                }
                "tr" => {
                    let row = self.row(child, state);
                    // A first row of header cells is the table's header.
                    let all_th = child.children.iter().all(|n| match n {
                        Node::Element(e) => e.name == "th",
                        Node::Text(t) => t.trim().is_empty(),
                    }) && !row.cells.is_empty();
                    let header = all_th && table.rows.is_empty();
                    table.push_row(row, header);
                }
                _ => {}
            }
        }
        if !widths.is_empty() && widths.iter().all(Option::is_some) {
            table.widths = Some(widths.into_iter().flatten().collect());
        }
        if let Some(caption) = caption {
            self.out.extend(caption);
        }
        if !table.header.is_empty() || !table.rows.is_empty() {
            self.out.push(Block::Table(table));
        }
    }

    fn row(&mut self, element: &Element, state: &State) -> Row {
        let inner = state.enter(element, true);
        let mut row = Row {
            background: background(element),
            ..Row::default()
        };
        for node in &element.children {
            let Node::Element(cell) = node else {
                continue;
            };
            if cell.name != "td" && cell.name != "th" {
                continue;
            }
            let cell_state = inner.enter(cell, true);
            let blocks = self.collect(&cell.children, &cell_state);
            let mut laid = Cell::blocks(blocks);
            laid.colspan = cell
                .attr("colspan")
                .and_then(|c| c.trim().parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 1000);
            laid.align = cell_state.align;
            laid.background = background(cell);
            laid.width_percent = percent(cell);
            laid.valign = cell
                .attr("valign")
                .map(str::to_owned)
                .or_else(|| style_value(cell, "vertical-align"))
                .map_or(VAlign::Top, |v| {
                    match v.trim().to_ascii_lowercase().as_str() {
                        "middle" | "center" => VAlign::Middle,
                        "bottom" => VAlign::Bottom,
                        _ => VAlign::Top,
                    }
                });
            row.cells.push(laid);
        }
        row
    }

    fn image(&mut self, element: &Element, state: &State) -> Option<Image> {
        let src = element.attr("src")?.trim();
        let (bytes, source) = if let Some(data) = src.strip_prefix("data:") {
            let (meta, payload) = data.split_once(',')?;
            if !meta.ends_with(";base64") {
                tracing::debug!(target: "navree_documents::native", "an image data URI is not base64: skipped");
                return None;
            }
            let bytes = BASE64
                .decode(payload.trim().as_bytes())
                .map_err(|error| {
                    tracing::debug!(target: "navree_documents::native", %error, "an image data URI does not decode: skipped");
                })
                .ok()?;
            (Bytes::from(bytes), "data URI".to_owned())
        } else {
            match (self.images)(src) {
                Some(found) => found,
                None => {
                    tracing::debug!(target: "navree_documents::native", src, "an image is not in the template tree (or is on another origin): skipped");
                    return None;
                }
            }
        };
        let mut image = Image::new(bytes);
        image.source = Some(source);
        image.alt = element
            .attr("alt")
            .map(str::to_owned)
            .filter(|a| !a.is_empty());
        let size = state.size();
        let dimension = |attr: &str| {
            style_value(element, attr).or_else(|| element.attr(attr).map(str::to_owned))
        };
        if let Some(width) = dimension("width") {
            if let Some(p) = width.trim().strip_suffix('%') {
                image.width_percent = p.trim().parse().ok();
            } else {
                image.width = css::length(&width, size, None);
            }
        }
        if let Some(height) = dimension("height")
            && !height.trim().ends_with('%')
        {
            image.height = css::length(&height, size, None);
        }
        image.align = element.attr("align").and_then(align_value).or(state.align);
        Some(image)
    }
}

/// The placeholder a header or footer `<span class=..>` stands for.
fn placeholder(element: &Element) -> Option<RunKind> {
    if element.has_class("pageNumber") {
        Some(RunKind::PageNumber)
    } else if element.has_class("totalPages") {
        Some(RunKind::PageCount)
    } else if element.has_class("title") {
        Some(RunKind::Title)
    } else if element.has_class("date") {
        Some(RunKind::Date)
    } else {
        None
    }
}

fn has_placeholder(paragraph: &Paragraph) -> bool {
    paragraph
        .runs
        .iter()
        .any(|run| !matches!(run.kind, RunKind::Text(_)))
}

/// The value of one property of an element's `style`.
fn style_value(element: &Element, property: &str) -> Option<String> {
    css::declarations(element.attr("style")?)
        .into_iter()
        .rev()
        .find(|(p, _)| p == property)
        .map(|(_, v)| v)
}

/// `bgcolor` or `background-color` (or a plain-color `background`).
fn background(element: &Element) -> Option<super::model::Color> {
    style_value(element, "background-color")
        .or_else(|| style_value(element, "background"))
        .and_then(|v| css::color(&v))
        .or_else(|| element.attr("bgcolor").and_then(css::color))
}

/// A `width` (attribute or style) given as a percentage.
fn percent(element: &Element) -> Option<f32> {
    style_value(element, "width")
        .or_else(|| element.attr("width").map(str::to_owned))
        .and_then(|w| {
            w.trim()
                .strip_suffix('%')
                .and_then(|p| p.trim().parse().ok())
        })
}

/// Whitespace collapsed and trimmed.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::model::Color;
    use crate::print::Paper;

    fn convert_html(html: &str) -> Converted {
        convert(html, &mut |src: &str| {
            (src == "images/logo.png").then(|| (Bytes::from_static(b"\x89PNG..."), src.to_owned()))
        })
    }

    fn texts(paragraph: &Paragraph) -> Vec<String> {
        paragraph
            .runs
            .iter()
            .map(|run| match &run.kind {
                RunKind::Text(t) => t.clone(),
                RunKind::LineBreak => "\\n".into(),
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn paragraphs_and_inline_formatting() {
        let converted = convert_html(
            "<html lang=\"en\"><head><title> Invoice\n 42 </title><meta name=author content=\"Agra\"></head>\
             <body><p>  Hello   <b>bold</b> and\n<em>italic <u>under</u></em>  </p><p>Line<br> two</p></body></html>",
        );
        let document = converted.document;
        assert_eq!(document.title.as_deref(), Some("Invoice 42"));
        assert_eq!(document.author.as_deref(), Some("Agra"));
        assert_eq!(converted.lang.as_deref(), Some("en"));
        let [Block::Paragraph(first), Block::Paragraph(second)] = document.blocks.as_slice() else {
            panic!("{:?}", document.blocks);
        };
        assert_eq!(
            texts(first),
            vec!["Hello ", "bold", " and ", "italic ", "under"]
        );
        assert_eq!(first.runs[1].style.bold, Some(true));
        assert_eq!(first.runs[4].style.italic, Some(true));
        assert_eq!(first.runs[4].style.underline, Some(true));
        assert_eq!(texts(second), vec!["Line", "\\n", "two"]);
    }

    #[test]
    fn inline_styles_and_alignment() {
        let converted = convert_html(
            "<body style=\"font-family: 'Noto Serif'; font-size: 10pt\">\
             <p style=\"text-align: right; color: #c00000\">Due <span style=\"font-weight: 700; font-size: 150%\">today</span></p>\
             <div align=center><p>Centered</p></div></body>",
        );
        let document = converted.document;
        assert_eq!(document.style.size, Some(10.0));
        assert_eq!(
            document.style.font.as_deref(),
            Some(&["Noto Serif".to_owned()][..])
        );
        let Block::Paragraph(due) = &document.blocks[0] else {
            panic!()
        };
        assert_eq!(due.align, Some(Align::Right));
        assert_eq!(due.style.color, Some(Color::rgb(0xc0, 0, 0)));
        assert_eq!(due.runs[1].style.bold, Some(true));
        assert_eq!(due.runs[1].style.size, Some(15.0));
        let Block::Paragraph(centered) = &document.blocks[1] else {
            panic!()
        };
        assert_eq!(centered.align, Some(Align::Center));
    }

    #[test]
    fn headings_lists_rules_and_breaks() {
        let document = convert_html(
            "<h1 style=\"color: navy\">Report</h1><ul><li>One<li>Two<ol start=3 type=i><li>Nested</ol></ul>\
             <hr><div class=\"page-break\"></div><p style=\"break-before: page\">After</p>",
        )
        .document;
        let kinds: Vec<&str> = document
            .blocks
            .iter()
            .map(|b| match b {
                Block::Heading(..) => "h",
                Block::List(_) => "list",
                Block::Rule => "hr",
                Block::PageBreak => "break",
                Block::Paragraph(_) => "p",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, vec!["h", "list", "hr", "break", "break", "p"]);
        let Block::Heading(1, heading) = &document.blocks[0] else {
            panic!()
        };
        assert_eq!(heading.style.color, Some(Color::rgb(0, 0, 128)));
        assert_eq!(heading.style.size, None);
        let Block::List(list) = &document.blocks[1] else {
            panic!()
        };
        assert_eq!(list.items.len(), 2);
        let Block::List(nested) = &list.items[1].blocks[1] else {
            panic!("{:?}", list.items[1]);
        };
        assert_eq!((nested.kind, nested.start), (ListKind::LowerRoman, 3));
    }

    #[test]
    fn tables_with_headers_spans_and_alignment() {
        let document = convert_html(
            "<table border=1 cellpadding=8><thead><tr><th width=\"60%\">Item<th>Amount</thead>\
             <tbody><tr style=\"background-color: #f5f5f5\"><td>Consulting<td align=right>1,000.00\
             <tr><td colspan=2 valign=middle style=\"text-align: end\"><b>Total</b></td></tbody></table>",
        )
        .document;
        let Block::Table(table) = &document.blocks[0] else {
            panic!("{:?}", document.blocks)
        };
        assert_eq!(table.borders, Borders::Grid);
        assert_eq!(table.padding, (6.0, 6.0));
        assert_eq!(table.header.len(), 1);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.header[0].cells[0].width_percent, Some(60.0));
        assert_eq!(table.rows[0].background, Some(Color::rgb(0xf5, 0xf5, 0xf5)));
        assert_eq!(table.rows[0].cells[1].align, Some(Align::Right));
        let total = &table.rows[1].cells[0];
        assert_eq!(
            (total.colspan, total.valign, total.align),
            (2, VAlign::Middle, Some(Align::End))
        );
        // A first row of <th> without <thead> is the header too.
        let document = convert_html("<table><tr><th>A<th>B<tr><td>1<td>2</table>").document;
        let Block::Table(table) = &document.blocks[0] else {
            panic!()
        };
        assert_eq!((table.header.len(), table.rows.len()), (1, 1));
        assert_eq!(table.borders, Borders::Rows);
    }

    #[test]
    fn images_come_from_the_source_or_data_uris() {
        let png = BASE64.encode(b"\x89PNG-bytes");
        let html = format!(
            "<p>Logo: <img src=\"images/logo.png\" width=\"120\" alt=\"Agra\"></p>\
             <img src=\"missing.png\"><img src=\"data:image/png;base64,{png}\" style=\"width: 50%\">"
        );
        let document = convert_html(&html).document;
        let images: Vec<&Image> = document
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Image(i) => Some(i),
                _ => None,
            })
            .collect();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].width, Some(90.0));
        assert_eq!(images[0].alt.as_deref(), Some("Agra"));
        assert_eq!(images[0].source.as_deref(), Some("images/logo.png"));
        assert_eq!(images[1].width_percent, Some(50.0));
        assert!(images[1].bytes.starts_with(b"\x89PNG"));
        // The text before the image is its own paragraph.
        assert!(matches!(&document.blocks[0], Block::Paragraph(p) if texts(p) == vec!["Logo:"]));
    }

    #[test]
    fn headers_footers_page_numbers_and_page_rules() {
        let converted = convert_html(
            "<html dir=\"rtl\"><head><style>@page { size: letter landscape; margin: 1in }</style></head>\
             <body><header><p>Agra report</p></header><p>Body</p>\
             <footer><p>Page <span class=\"pageNumber\"></span> of <span class=\"totalPages\"></span></p></footer></body></html>",
        );
        assert_eq!(converted.direction, Some(Direction::Rtl));
        assert_eq!(converted.page.paper, Some(Paper::Letter));
        assert_eq!(converted.page.landscape, Some(true));
        let document = converted.document;
        assert_eq!(document.blocks.len(), 1);
        assert_eq!(document.header.len(), 1);
        let Block::Paragraph(footer) = &document.footer[0] else {
            panic!()
        };
        assert_eq!(
            texts(footer),
            vec!["Page ", "PageNumber", " of ", "PageCount"]
        );
    }

    #[test]
    fn preformatted_text_keeps_its_spaces() {
        let document = convert_html("<pre>let x  = 1;\n    y</pre>").document;
        let Block::Paragraph(pre) = &document.blocks[0] else {
            panic!()
        };
        assert!(pre.preformatted);
        assert_eq!(texts(pre), vec!["let x  = 1;\n    y"]);
        assert_eq!(
            pre.style.font.as_deref(),
            Some(&["Noto Sans Mono".to_owned()][..])
        );
    }
}
