//! A drawing of the toolkit's, written out as a standalone SVG document with every colour
//! resolved: what an image renderer reads. A page colours its charts with CSS (a mark
//! names its colour with `data-color` or `data-tone` and its part with `data-slot`, and
//! the sheets turn those into theme tokens); a renderer has no stylesheet and no custom
//! properties, so the same rules are applied here, from the palette of one theme in one
//! mode, as presentation attributes.
//!
//! Only what draws is written: `data-*`, `aria-*` and the other attributes of a page
//! are left out, an element of HTML inside the drawing goes with its children, and so
//! do `image` and `foreignObject`. A reference (`href`) that isn't to an element of the
//! drawing itself is left out too, so nothing outside the document is ever read.

use crate::html::escape_into;
use crate::node::{AttrValue, Element, Node};
use crate::theme::{Palette, Rgb, Side, Token};

/// One theme's colours in one mode, with the surface the drawing sits on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Inks {
    pub palette: Palette,
    pub side: Side,
    pub surface: Rgb,
}

impl Inks {
    fn get(&self, token: Token) -> Rgb {
        self.palette.get(token)
    }

    /// A token by its custom property's name (`fg-2`, `chart-3`, `chart-3-vivid`).
    fn named(&self, name: &str) -> Option<Rgb> {
        if name == "surface" {
            return Some(self.surface);
        }
        if let Some(base) = name.strip_suffix("-vivid") {
            let token = Token::from_name(base)?;
            return Token::CHARTS
                .contains(&token)
                .then(|| self.get(token).vivid(self.side));
        }
        Token::from_name(name).map(|token| self.get(token))
    }

    fn tone(&self, name: &str) -> Rgb {
        match name {
            "info" => self.get(Token::Info),
            "success" | "up" => self.get(Token::Success),
            "warning" => self.get(Token::Amber),
            "danger" | "down" => self.get(Token::Danger),
            "violet" => self.get(Token::Violet),
            "cyan" => self.get(Token::Cyan),
            "rose" => self.get(Token::Rose),
            "orange" => self.get(Token::Amber).mix(self.get(Token::Danger), 0.4),
            _ => self.get(Token::Fg3),
        }
    }

    /// The colour a mark's `data-color` names, and its vivid end.
    fn color(&self, value: &str) -> Option<(Rgb, Rgb)> {
        let fg = self.get(Token::Fg);
        let toward = |c: Rgb| (c, c.mix(fg, 0.38));
        match value {
            "accent" => Some(toward(self.get(Token::Accent))),
            "other" => Some(toward(self.get(Token::Fg3))),
            slot => {
                let n: usize = slot.parse().ok().filter(|n| (1..=8).contains(n))?;
                let token = Token::CHARTS[n - 1];
                Some((self.get(token), self.get(token).vivid(self.side)))
            }
        }
    }
}

/// What an element inherits: the mark's colour and its vivid end, the surface, the
/// text colour (`currentColor`), and the part it is drawn in.
#[derive(Clone, Copy, Debug)]
struct Ctx {
    c: Rgb,
    vivid: Rgb,
    color: Rgb,
    /// Inside the bars of a bar list, where a coloured mark fills with its colour.
    bars: bool,
    /// Inside a sleep chart's stages.
    stages: bool,
    /// Inside a reference line or a zone, whose words sit on the surface.
    marked: bool,
    /// Inside a target or an event, whose lines and words are quieter.
    target: bool,
    event: bool,
    band: bool,
    zone: bool,
    reference: bool,
    solid: bool,
}

/// The size of the box a length's percentage is of, in the current user units.
#[derive(Clone, Copy, Debug)]
struct Viewport {
    w: f64,
    h: f64,
}

/// Elements that draw nothing an image can show, or that would read from outside.
fn dropped(tag: &str) -> bool {
    matches!(
        tag,
        "image" | "foreignObject" | "script" | "style" | "iframe" | "video" | "audio" | "a"
    ) || !svg_tag(tag)
}

/// The elements of SVG the writer keeps; anything else (HTML) goes.
fn svg_tag(tag: &str) -> bool {
    matches!(
        tag,
        "svg"
            | "g"
            | "defs"
            | "path"
            | "rect"
            | "circle"
            | "ellipse"
            | "line"
            | "polyline"
            | "polygon"
            | "text"
            | "tspan"
            | "linearGradient"
            | "radialGradient"
            | "stop"
            | "clipPath"
            | "mask"
            | "pattern"
            | "use"
            | "symbol"
            | "title"
            | "filter"
            | "feGaussianBlur"
            | "feOffset"
            | "feFlood"
            | "feComposite"
            | "feMerge"
            | "feMergeNode"
            | "feDropShadow"
    )
}

/// Attributes of a page, not of a drawing.
fn page_attr(name: &str) -> bool {
    name.starts_with("data-")
        || name.starts_with("aria-")
        || matches!(
            name,
            "role" | "focusable" | "tabindex" | "hidden" | "class" | "lang" | "dir" | "title"
        )
}

const PAINT: [&str; 5] = ["fill", "stroke", "stop-color", "color", "flood-color"];

/// A number as SVG writes it: at most three decimals, no trailing zeros.
pub(crate) fn num(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_owned();
    }
    let mut text = format!("{value:.3}");
    while text.contains('.') && (text.ends_with('0') || text.ends_with('.')) {
        text.pop();
    }
    if text == "-0" { "0".to_owned() } else { text }
}

fn alpha(value: f64) -> String {
    num(value)
}

/// A length in the current user units: a number, `px`, or a percentage of `of`.
fn length(text: &str, of: f64) -> Option<f64> {
    let text = text.trim();
    if let Some(pct) = text.strip_suffix('%') {
        return pct.trim().parse::<f64>().ok().map(|p| p / 100.0 * of);
    }
    text.strip_suffix("px")
        .unwrap_or(text)
        .trim()
        .parse::<f64>()
        .ok()
}

/// The writer: one pass over the tree, out to `out`.
pub(crate) struct Writer<'a> {
    inks: &'a Inks,
    out: String,
    /// What every id of the drawing starts with, and every reference to one: two
    /// drawings in one page never share a gradient.
    ids: Option<String>,
}

impl<'a> Writer<'a> {
    pub(crate) fn new(inks: &'a Inks) -> Self {
        Self {
            inks,
            out: String::with_capacity(4096),
            ids: None,
        }
    }

    /// Starts every id with `prefix`, and every reference to one.
    pub(crate) fn ids(mut self, prefix: String) -> Self {
        self.ids = Some(prefix);
        self
    }

    /// `value` with its references to ids of the drawing given the prefix.
    fn own_ids(&self, name: &str, value: String) -> String {
        let Some(prefix) = &self.ids else {
            return value;
        };
        match name {
            "id" => format!("{prefix}-{value}"),
            "href" | "xlink:href" => value.replacen('#', &format!("#{prefix}-"), 1),
            _ if value.contains("url(#") => value.replace("url(#", &format!("url(#{prefix}-")),
            _ => value,
        }
    }

    /// The whole document: a `width` × `height` image with `background` (none for a
    /// clear one) under the first drawing in `node`. `None` when `node` holds no
    /// `<svg>` with anything in it.
    pub(crate) fn document(
        mut self,
        node: &Node,
        width: u32,
        height: u32,
        background: Option<Rgb>,
    ) -> Option<String> {
        let fg = self.inks.get(Token::Fg);
        let first = self.inks.get(Token::Chart1);
        let ctx = Ctx {
            c: first,
            vivid: first.vivid(self.inks.side),
            color: fg,
            bars: false,
            stages: false,
            marked: false,
            target: false,
            event: false,
            band: false,
            zone: false,
            reference: false,
            solid: false,
        };
        let (drawing, ctx) = self.find(node, ctx)?;
        let (w, h) = (f64::from(width), f64::from(height));
        self.out
            .push_str(r#"<svg xmlns="http://www.w3.org/2000/svg" width=""#);
        self.out.push_str(&width.to_string());
        self.out.push_str(r#"" height=""#);
        self.out.push_str(&height.to_string());
        self.out.push_str(r#"" viewBox="0 0 "#);
        self.out.push_str(&format!("{width} {height}"));
        self.out.push_str(r#"" font-family="Geist">"#);
        if let Some(back) = background {
            self.out
                .push_str(r#"<rect x="0" y="0" width="100%" height="100%" fill=""#);
            self.out.push_str(&back.to_hex());
            self.out.push_str(r#""/>"#);
        }
        self.element(drawing, ctx, Viewport { w, h }, true);
        self.out.push_str("</svg>");
        Some(self.out)
    }

    /// The first `<svg>` in `node` that holds something, with what the HTML around it
    /// passes down: the colour its chart names.
    fn find<'n>(&self, node: &'n Node, ctx: Ctx) -> Option<(&'n Element, Ctx)> {
        match node {
            Node::Element(element) => {
                let ctx = self.inherit(element, ctx);
                if element.tag == "svg" && !element.children.is_empty() {
                    return Some((element, ctx));
                }
                element
                    .children
                    .iter()
                    .find_map(|child| self.find(child, ctx))
            }
            Node::Fragment(nodes) => nodes.iter().find_map(|child| self.find(child, ctx)),
            _ => None,
        }
    }

    /// The context `element` passes to itself and its children.
    fn inherit(&self, element: &Element, mut ctx: Ctx) -> Ctx {
        if let Some(value) = attr(element, "data-color")
            && let Some((c, vivid)) = self.inks.color(&value)
        {
            ctx.c = c;
            ctx.vivid = vivid;
        } else if let Some(tone) = attr(element, "data-tone") {
            ctx.c = self.inks.tone(&tone);
            ctx.vivid = ctx.c.mix(self.inks.get(Token::Fg), 0.38);
        }
        match attr(element, "data-slot").as_deref() {
            Some("bars") => ctx.bars = true,
            Some("chart-stages") => ctx.stages = true,
            Some("chart-target") => ctx.target = true,
            Some("chart-event") => ctx.event = true,
            Some("chart-band") => ctx.band = true,
            Some("chart-zone") => {
                ctx.zone = true;
                ctx.marked = true;
            }
            Some("chart-ref") => {
                ctx.reference = true;
                ctx.marked = true;
                ctx.solid = has(element, "data-solid");
            }
            _ => {}
        }
        if let Some(value) = attr(element, "color")
            && let Some(colour) = self.paint_value(&value, ctx)
        {
            ctx.color = colour;
        }
        ctx
    }

    /// A paint attribute's value with tokens resolved: `var(--fg-2)`, `currentColor`.
    /// `None` leaves the value as it is.
    fn paint_value(&self, value: &str, ctx: Ctx) -> Option<Rgb> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("currentColor") {
            return Some(ctx.color);
        }
        let inner = value.strip_prefix("var(--")?.strip_suffix(')')?;
        let (name, fallback) = match inner.split_once(',') {
            Some((name, fallback)) => (name.trim(), Some(fallback.trim())),
            None => (inner.trim(), None),
        };
        match name {
            "c" => Some(ctx.c),
            "c-vivid" => Some(ctx.vivid),
            _ => self
                .inks
                .named(name)
                .or_else(|| fallback.and_then(|f| self.paint_value(f, ctx)))
                .or_else(|| fallback.and_then(|f| Rgb::parse(f).ok())),
        }
    }

    fn element(&mut self, element: &Element, ctx: Ctx, vp: Viewport, outermost: bool) {
        if dropped(element.tag) {
            return;
        }
        let ctx = self.inherit(element, ctx);
        let tag = element.tag;
        // Attributes, the first of each name, as a browser keeps them.
        let mut attrs: Vec<(&'static str, String)> = Vec::with_capacity(element.attrs.len());
        for a in &element.attrs {
            if page_attr(a.name) || attrs.iter().any(|(name, _)| *name == a.name) {
                continue;
            }
            let Some(value) = a.value.as_text().map(|v| v.into_owned()) else {
                if matches!(a.value, AttrValue::Flag) {
                    continue;
                }
                continue;
            };
            let value = match a.name {
                "href" | "xlink:href" if !value.starts_with('#') => continue,
                "clip-path" | "mask" | "filter" if !value.trim_start().starts_with("url(") => {
                    continue;
                }
                "vector-effect" => continue,
                name if PAINT.contains(&name) => match self.paint_value(&value, ctx) {
                    Some(colour) => colour.to_hex(),
                    None => value,
                },
                _ => value,
            };
            attrs.push((a.name, self.own_ids(a.name, value)));
        }
        let mut add = |name: &'static str, value: String| {
            if !attrs.iter().any(|(n, _)| *n == name) {
                attrs.push((name, value));
            }
        };
        let slot = attr(element, "data-slot");
        for (name, value) in self.rules(element, slot.as_deref(), ctx) {
            add(name, value);
        }
        // A column with rounded ends: the clip a page draws, as the rectangle's own shape.
        if tag == "rect"
            && let Some(clip) = attr(element, "clip-path")
            && let Some((inset, round)) = column_inset(&clip)
        {
            let x = attrs
                .iter()
                .find(|(n, _)| *n == "x")
                .map(|(_, v)| v.clone());
            let w = attrs
                .iter()
                .find(|(n, _)| *n == "width")
                .map(|(_, v)| v.clone());
            if let (Some(x), Some(w)) = (x, w) {
                let pct = |v: &str| {
                    v.trim()
                        .strip_suffix('%')
                        .and_then(|p| p.parse::<f64>().ok())
                };
                let (x, w, unit) = match (pct(&x), pct(&w)) {
                    (Some(x), Some(w)) => (x, w, "%"),
                    _ => (
                        length(&x, vp.w).unwrap_or(0.0),
                        length(&w, vp.w).unwrap_or(0.0),
                        "",
                    ),
                };
                for (name, value) in &mut attrs {
                    if *name == "x" {
                        *value = format!("{}{unit}", num(x + w * inset));
                    } else if *name == "width" {
                        *value = format!("{}{unit}", num(w * (1.0 - 2.0 * inset)));
                    }
                }
                // A page rounds the corners as one radius on both sides, held to half the
                // narrower side: a pill. SVG holds each radius to its own side, so both
                // are given, worked out in pixels.
                let wide = if unit == "%" { w / 100.0 * vp.w } else { w } * (1.0 - 2.0 * inset);
                let tall = attrs
                    .iter()
                    .find(|(n, _)| *n == "height")
                    .and_then(|(_, v)| length(v, vp.h))
                    .unwrap_or(wide);
                let radius = round.min(wide.min(tall) / 2.0).max(0.0);
                attrs.push(("rx", num(radius)));
                attrs.push(("ry", num(radius)));
            }
        }

        // A nested drawing: what its percentages are of, and, when it stretches its
        // drawing to fit (`preserveAspectRatio="none"`), the lines that keep their
        // width in pixels drawn at their place in its box rather than stretched.
        let mut inner = vp;
        let mut stretch: Option<(f64, f64)> = None;
        if tag == "svg" {
            if outermost {
                for (name, default) in [("width", "100%"), ("height", "100%")] {
                    if !attrs.iter().any(|(n, _)| *n == name) {
                        attrs.push((name, default.to_owned()));
                    }
                }
            }
            let get = |name: &str| {
                attrs
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| v.clone())
            };
            let w = get("width").and_then(|v| length(&v, vp.w)).unwrap_or(vp.w);
            let h = get("height").and_then(|v| length(&v, vp.h)).unwrap_or(vp.h);
            inner = Viewport { w, h };
            if let Some(view) = get("viewBox") {
                let parts: Vec<f64> = view
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|p| !p.is_empty())
                    .filter_map(|p| p.parse().ok())
                    .collect();
                if let [_, _, vw, vh] = parts[..]
                    && vw > 0.0
                    && vh > 0.0
                {
                    inner = Viewport { w: vw, h: vh };
                    let none = get("preserveAspectRatio").is_some_and(|v| v.trim() == "none");
                    let (sx, sy) = (w / vw, h / vh);
                    if none && (sx - sy).abs() > 1e-9 && element.children.iter().any(keeps_width) {
                        stretch = Some((sx, sy));
                        attrs.retain(|(n, _)| *n != "viewBox" && *n != "preserveAspectRatio");
                        // The box is the nested drawing's own size now, in its parent's units.
                        for (name, value) in &mut attrs {
                            if *name == "width" {
                                *value = num(w);
                            } else if *name == "height" {
                                *value = num(h);
                            }
                        }
                    }
                }
            }
        }

        self.out.push('<');
        self.out.push_str(tag);
        for (name, value) in &attrs {
            self.out.push(' ');
            self.out.push_str(name);
            self.out.push_str("=\"");
            escape_into(&mut self.out, value);
            self.out.push('"');
        }
        if element.children.is_empty() {
            self.out.push_str("/>");
            return;
        }
        self.out.push('>');
        match stretch {
            Some((sx, sy)) => {
                // Runs of plain marks are stretched as a group; a line that keeps its
                // width is drawn with its points moved instead, in order.
                let mut open = false;
                for child in &element.children {
                    let scaled = match child {
                        Node::Element(e) if keeps_width_el(e) => scaled_path(e, sx, sy),
                        _ => None,
                    };
                    match scaled {
                        Some(path) => {
                            if open {
                                self.out.push_str("</g>");
                                open = false;
                            }
                            self.element(&path, ctx, Viewport { w: vp.w, h: vp.h }, false);
                        }
                        None => {
                            if !open {
                                self.out.push_str(&format!(
                                    r#"<g transform="scale({} {})">"#,
                                    num(sx),
                                    num(sy)
                                ));
                                open = true;
                            }
                            self.node(child, ctx, inner);
                        }
                    }
                }
                if open {
                    self.out.push_str("</g>");
                }
            }
            None => {
                for child in &element.children {
                    self.node(child, ctx, inner);
                }
            }
        }
        self.out.push_str("</");
        self.out.push_str(tag);
        self.out.push('>');
    }

    fn node(&mut self, node: &Node, ctx: Ctx, vp: Viewport) {
        match node {
            Node::Element(element) => self.element(element, ctx, vp, false),
            Node::Fragment(nodes) => {
                for child in nodes {
                    self.node(child, ctx, vp);
                }
            }
            Node::Text(text) => escape_into(&mut self.out, text),
            Node::Int(value) => self.out.push_str(&value.to_string()),
            // Markup of the toolkit's own (an icon's shapes): drawn as it is.
            Node::Static(markup) => self.out.push_str(markup),
            Node::Raw(html) => self.out.push_str(html.as_str()),
            Node::Empty => {}
        }
    }

    /// What the stylesheets give `element`, for each attribute it doesn't set itself.
    fn rules(
        &self,
        element: &Element,
        slot: Option<&str>,
        ctx: Ctx,
    ) -> Vec<(&'static str, String)> {
        let inks = self.inks;
        let hex = Rgb::to_hex;
        let tok = |t: Token| inks.get(t).to_hex();
        let mut out: Vec<(&'static str, String)> = Vec::new();
        let mut put = |name: &'static str, value: String| out.push((name, value));
        let light = matches!(inks.side, Side::Light);
        match slot {
            Some("radial-track") => {
                put("fill", "none".into());
                if attr(element, "data-track").as_deref() == Some("tint") {
                    put("stroke", hex(ctx.c));
                    put("stroke-opacity", alpha(0.18));
                } else {
                    put("stroke", tok(Token::Fg));
                    put("stroke-opacity", alpha(0.09));
                }
            }
            Some("radial-zone") => {
                put("fill", "none".into());
                put("stroke", hex(ctx.c));
                put("stroke-opacity", alpha(if light { 0.34 } else { 0.44 }));
            }
            Some("radial-arc") => {
                put("fill", "none".into());
                put("stroke", hex(ctx.c));
                put("stroke-linecap", "round".into());
            }
            Some("radial-lap") => {
                put("fill", "none".into());
                put("stroke", hex(ctx.vivid));
                put("stroke-linecap", "round".into());
            }
            Some("radial-halo") => {
                put("fill", "none".into());
                put("stroke", hex(ctx.c));
                put("stroke-opacity", alpha(0.35));
            }
            Some("radial-icon" | "icon") => {
                put("fill", "none".into());
                let ink = if slot == Some("icon") {
                    ctx.color
                } else {
                    ctx.c.mix(inks.get(Token::Fg), 0.22)
                };
                put("stroke", hex(ink));
                put("stroke-width", "2".into());
                put("stroke-linecap", "round".into());
                put("stroke-linejoin", "round".into());
            }
            Some("chart-line") => {
                put("fill", "none".into());
                put("stroke", hex(ctx.c));
                put("stroke-width", "2".into());
                put("stroke-linejoin", "round".into());
                put("stroke-linecap", "round".into());
            }
            Some("chart-area") => {
                put("stroke", "none".into());
                if attr(element, "fill").is_none() {
                    put("fill", hex(ctx.c));
                    put("fill-opacity", alpha(0.14));
                }
            }
            Some("chart-end") => {
                put("fill", hex(ctx.c));
                put("stroke", hex(inks.surface));
                put("stroke-width", "2".into());
            }
            Some("chart-point" | "chart-dot") => {
                put("fill", hex(ctx.c));
                put("stroke", hex(inks.surface));
                put("stroke-width", "2".into());
            }
            Some(
                "chart-column" | "chart-bar" | "seg" | "cols-part" | "strip-cell" | "clock-wedge",
            ) => {
                put("fill", hex(ctx.c));
                if slot == Some("seg") || slot == Some("cols-part") {
                    put("stroke", hex(inks.surface));
                    put(
                        "stroke-width",
                        if slot == Some("seg") { "1.5" } else { "1" }.into(),
                    );
                }
            }
            Some("clock-band") => {
                put("fill", hex(ctx.c));
                put("fill-opacity", alpha(0.72));
            }
            Some("chart-slice") => {
                put("fill", hex(ctx.c));
                put("stroke", hex(inks.surface));
                put("stroke-width", "2".into());
                put("stroke-linejoin", "round".into());
            }
            Some("chart-grid") => {
                put("stroke", tok(Token::Line));
                put("stroke-width", "1".into());
            }
            Some("chart-zero" | "chart-axis-line" | "chart-tick") => {
                put("stroke", tok(Token::Line3));
                put("stroke-width", "1".into());
            }
            Some("chart-cursor") => {
                put("stroke", tok(Token::Fg3));
                put("stroke-width", "1".into());
            }
            Some("chart-target-line") => {
                put("stroke", tok(Token::Fg2));
                put("stroke-width", "1".into());
                put("stroke-dasharray", "4 3".into());
            }
            Some("chart-track-bg" | "meter-track" | "seg-track" | "cols-track" | "scale-track") => {
                put("fill", tok(Token::Ink3));
            }
            Some("meter-fill") => put("fill", hex(ctx.c)),
            Some("meter-goal") => {
                put("stroke", tok(Token::Fg1));
                put("stroke-width", "2".into());
                put("stroke-linecap", "round".into());
            }
            Some("clock-ring") => {
                put("fill", "none".into());
                put("stroke", tok(Token::Line3));
                put("stroke-width", "1".into());
                put("stroke-dasharray", "2 4".into());
            }
            _ => {}
        }
        // What a part's own elements take from it.
        match element.tag {
            "stop" => {
                let colour = match attr(element, "data-mix").as_deref() {
                    Some("vivid") => ctx.vivid,
                    Some("soft") => ctx.c.mix(inks.surface, 0.7),
                    _ => ctx.c,
                };
                put("stop-color", hex(colour));
            }
            "line" if ctx.reference => {
                put("stroke", hex(ctx.c));
                put("stroke-width", "1".into());
                if !ctx.solid {
                    put("stroke-dasharray", "4 3".into());
                }
            }
            "line" if ctx.target => {
                put("stroke", tok(Token::Fg2));
                put("stroke-width", "1".into());
                put("stroke-dasharray", "4 3".into());
            }
            "line" if ctx.event => {
                put("stroke", tok(Token::Fg3));
                put("stroke-width", "1".into());
            }
            "rect" if ctx.zone => {
                put("fill", hex(ctx.c));
                put("fill-opacity", alpha(0.08));
            }
            "rect" if ctx.band => {
                put("fill", tok(Token::Fg));
                put("fill-opacity", alpha(0.05));
            }
            "rect" | "path" | "circle"
                if (ctx.bars || ctx.stages)
                    && (has(element, "data-color") || has(element, "data-tone")) =>
            {
                put("fill", hex(ctx.c));
            }
            "text" | "tspan" => {
                let quiet = if ctx.marked || ctx.target || ctx.event {
                    Token::Fg2
                } else {
                    Token::Fg3
                };
                put("fill", tok(quiet));
                if element.tag == "text" {
                    put("font-size", "11".into());
                }
            }
            _ => {}
        }
        if has(element, "data-dashed") {
            put("stroke-dasharray", "3 3".into());
        }
        out
    }
}

/// The value of attribute `name`, the first one.
fn attr(element: &Element, name: &str) -> Option<String> {
    element
        .attrs
        .iter()
        .find(|a| a.name == name)
        .and_then(|a| a.value.as_text().map(|v| v.into_owned()))
}

fn has(element: &Element, name: &str) -> bool {
    element.attrs.iter().any(|a| a.name == name)
}

/// Whether `node` is a line that keeps its width when its drawing stretches.
fn keeps_width(node: &Node) -> bool {
    matches!(node, Node::Element(e) if keeps_width_el(e))
}

fn keeps_width_el(element: &Element) -> bool {
    element.tag == "path"
        && element.attrs.iter().any(|a| {
            a.name == "vector-effect" && a.value.as_text().as_deref() == Some("non-scaling-stroke")
        })
}

/// `element`, a path, with its points stretched by `sx` and `sy`: `None` when its data
/// can't be read.
fn scaled_path(element: &Element, sx: f64, sy: f64) -> Option<Element> {
    let d = attr(element, "d")?;
    let scaled = scale_path(&d, sx, sy)?;
    let mut out = element.clone();
    for a in &mut out.attrs {
        if a.name == "d" {
            a.value = AttrValue::Text(scaled.clone().into_boxed_str());
        }
    }
    Some(out)
}

/// Path data with every point stretched by `sx` and `sy`. Arcs aren't stretched (their
/// radii and angles would change shape): `None` for a path with one.
pub(crate) fn scale_path(d: &str, sx: f64, sy: f64) -> Option<String> {
    let mut out = String::with_capacity(d.len());
    let mut command = ' ';
    let mut index = 0usize;
    let mut rest = d.trim_start();
    while !rest.is_empty() {
        let first = rest.chars().next()?;
        if first.is_ascii_alphabetic() {
            command = first;
            if matches!(command, 'A' | 'a') {
                return None;
            }
            if !matches!(
                command,
                'M' | 'm'
                    | 'L'
                    | 'l'
                    | 'H'
                    | 'h'
                    | 'V'
                    | 'v'
                    | 'C'
                    | 'c'
                    | 'S'
                    | 's'
                    | 'Q'
                    | 'q'
                    | 'T'
                    | 't'
                    | 'Z'
                    | 'z'
            ) {
                return None;
            }
            out.push(command);
            index = 0;
            rest = rest[1..].trim_start_matches(|c: char| c.is_whitespace() || c == ',');
            continue;
        }
        let (value, len) = number(rest)?;
        let along_x = match command.to_ascii_uppercase() {
            'H' => true,
            'V' => false,
            'Z' => return None,
            _ => index.is_multiple_of(2),
        };
        let value = value * if along_x { sx } else { sy };
        if !out.ends_with(|c: char| c.is_ascii_alphabetic()) {
            out.push(' ');
        }
        out.push_str(&num(value));
        index += 1;
        rest = rest[len..].trim_start_matches(|c: char| c.is_whitespace() || c == ',');
    }
    Some(out)
}

/// The number at the start of `text`, and how many bytes it takes.
fn number(text: &str) -> Option<(f64, usize)> {
    let bytes = text.as_bytes();
    let mut at = 0;
    if at < bytes.len() && (bytes[at] == b'-' || bytes[at] == b'+') {
        at += 1;
    }
    let mut dot = false;
    let mut digits = false;
    while at < bytes.len() {
        match bytes[at] {
            b'0'..=b'9' => digits = true,
            b'.' if !dot => dot = true,
            _ => break,
        }
        at += 1;
    }
    if !digits {
        return None;
    }
    if at < bytes.len() && (bytes[at] == b'e' || bytes[at] == b'E') {
        let mut end = at + 1;
        if end < bytes.len() && (bytes[end] == b'-' || bytes[end] == b'+') {
            end += 1;
        }
        let start = end;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end > start {
            at = end;
        }
    }
    text[..at].parse().ok().map(|v| (v, at))
}

/// A sparkline column's clip, `inset(0 max(1px, 22%) round 99px)`: the share of its width
/// cut from each side, and the corner radius.
fn column_inset(clip: &str) -> Option<(f64, f64)> {
    let inner = clip.trim().strip_prefix("inset(")?.strip_suffix(')')?;
    let (sides, round) = inner.split_once("round")?;
    let pct = sides
        .split([' ', ',', '(', ')'])
        .filter_map(|p| p.strip_suffix('%'))
        .find_map(|p| p.parse::<f64>().ok())?;
    let radius = length(round, 0.0)?;
    Some(((pct / 100.0).clamp(0.0, 0.49), radius))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_short_and_never_negative_zero() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(1.23456), "1.235");
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(f64::NAN), "0");
        assert_eq!(num(120.5), "120.5");
    }

    #[test]
    fn a_path_is_stretched_point_by_point() {
        assert_eq!(
            scale_path("M0 4L200 10L1000 28", 0.5, 2.0).unwrap(),
            "M0 8L100 20L500 56"
        );
        assert_eq!(
            scale_path("M0 32L0 4C66.7 6 133.3 8 200 10L1000 32Z", 0.1, 1.0).unwrap(),
            "M0 32L0 4C6.67 6 13.33 8 20 10L100 32Z"
        );
        assert_eq!(scale_path("M1,2 h10 v-4", 2.0, 3.0).unwrap(), "M2 6h20v-12");
        assert_eq!(scale_path("M1e2 .5", 1.0, 2.0).unwrap(), "M100 1");
        assert!(scale_path("M0 0A5 5 0 0 1 10 10", 2.0, 2.0).is_none());
        assert!(scale_path("M0 0 X", 2.0, 2.0).is_none());
    }

    #[test]
    fn a_columns_clip_is_its_share_and_radius() {
        assert_eq!(
            column_inset("inset(0 max(1px, 22%) round 99px)"),
            Some((0.22, 99.0))
        );
        assert_eq!(column_inset("url(#c)"), None);
    }

    #[test]
    fn lengths_are_numbers_pixels_or_shares() {
        assert_eq!(length("100%", 320.0), Some(320.0));
        assert_eq!(length("32", 0.0), Some(32.0));
        assert_eq!(length("12px", 0.0), Some(12.0));
        assert_eq!(length("auto", 1.0), None);
    }
}
