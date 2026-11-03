//! Your own icons: `icon_file!` and `icon_set!`, and the sanitiser they share.
//!
//! An SVG file is read while the app compiles. Only shape elements and safe
//! attributes survive; anything else stops the build with the file's name, the
//! line and the reason. What comes out is shapes and a `viewBox`, the same
//! `Icon` a built-in icon is.

use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::LitStr;

/// Largest file read, in bytes.
const MAX_BYTES: usize = 256 * 1024;
/// Most elements in one file.
const MAX_ELEMENTS: usize = 2048;
/// Deepest nesting.
const MAX_DEPTH: usize = 8;
/// Largest `viewBox` side.
const MAX_BOX: f64 = 4096.0;

/// An icon after sanitising.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Sanitised {
    /// The shapes, as SVG markup with no whitespace between elements.
    pub shapes: String,
    /// The `viewBox`: four numbers.
    pub view_box: String,
}

#[derive(Debug)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
    line: usize,
}

/// Reads an SVG file's bytes into the shapes and box of an icon.
pub(crate) fn sanitize(source: &[u8]) -> Result<Sanitised, String> {
    if source.len() > MAX_BYTES {
        return Err(format!(
            "the file is {} bytes; an icon is at most {MAX_BYTES}",
            source.len()
        ));
    }
    let text = std::str::from_utf8(source).map_err(|_| "the file is not valid UTF-8".to_owned())?;
    let root = Reader::new(text).document()?;
    build(&root)
}

// ------------------------------------------------------------------ reading

struct Reader<'a> {
    text: &'a str,
    at: usize,
    elements: usize,
    /// Whether an XML declaration may still come: only before anything else.
    declaration_allowed: bool,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Self {
        let at = if text.starts_with('\u{feff}') { 3 } else { 0 };
        Self {
            text,
            at,
            elements: 0,
            declaration_allowed: true,
        }
    }

    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    fn line(&self) -> usize {
        self.text[..self.at].bytes().filter(|&b| b == b'\n').count() + 1
    }

    fn fail<T>(&self, message: impl std::fmt::Display) -> Result<T, String> {
        Err(format!("line {}: {message}", self.line()))
    }

    fn skip_space(&mut self) {
        let trimmed = self.rest().trim_start_matches([' ', '\t', '\r', '\n']);
        self.at = self.text.len() - trimmed.len();
    }

    /// Skips a comment, an XML declaration, or white space. `Ok(true)` when it skipped something.
    fn skip_misc(&mut self) -> Result<bool, String> {
        let before = self.at;
        self.skip_space();
        if self.rest().starts_with("<!--") {
            match self.rest().find("-->") {
                Some(end) => self.at += end + 3,
                None => return self.fail("a comment is never closed"),
            }
        } else if self.declaration_allowed && is_xml_declaration(self.rest()) {
            match self.rest().find("?>") {
                Some(end) => self.at += end + 2,
                None => return self.fail("the XML declaration is never closed"),
            }
        } else if self.rest().starts_with("<?") {
            return self.fail("processing instructions are not allowed");
        } else if self.rest().starts_with("<!") {
            return self.fail("a DOCTYPE or CDATA section is not allowed (entities and external files hide in them)");
        }
        if !self.text[before..self.at].trim().is_empty() {
            self.declaration_allowed = false;
        }
        Ok(self.at != before)
    }

    fn document(&mut self) -> Result<Node, String> {
        while self.skip_misc()? {}
        if !self.rest().starts_with('<') {
            return self.fail("the file does not start with an <svg> element");
        }
        let root = self.element(0)?;
        while self.skip_misc()? {}
        if !self.rest().is_empty() {
            return self.fail("there is more after the closing </svg>");
        }
        Ok(root)
    }

    fn name(&mut self) -> Result<String, String> {
        let end = self
            .rest()
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == ':' || c == '_' || c == '.'))
            .unwrap_or(self.rest().len());
        let name = &self.rest()[..end];
        if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_' || c == ':') {
            return self.fail("a name cannot be read here");
        }
        self.at += end;
        Ok(name.to_owned())
    }

    fn element(&mut self, depth: usize) -> Result<Node, String> {
        if depth > MAX_DEPTH {
            return self.fail(format!("elements are nested more than {MAX_DEPTH} deep"));
        }
        self.elements += 1;
        if self.elements > MAX_ELEMENTS {
            return self.fail(format!("more than {MAX_ELEMENTS} elements"));
        }
        let line = self.line();
        self.at += 1; // '<'
        let name = self.name()?;
        let mut node = Node {
            name,
            attrs: Vec::new(),
            children: Vec::new(),
            line,
        };
        loop {
            self.skip_space();
            if self.rest().starts_with("/>") {
                self.at += 2;
                return Ok(node);
            }
            if self.rest().starts_with('>') {
                self.at += 1;
                break;
            }
            let attr = self.name()?;
            self.skip_space();
            if !self.rest().starts_with('=') {
                return self.fail(format!("attribute {attr} has no value"));
            }
            self.at += 1;
            self.skip_space();
            let quote = match self.rest().chars().next() {
                Some(q @ ('"' | '\'')) => q,
                _ => return self.fail(format!("the value of {attr} is not quoted")),
            };
            self.at += 1;
            let Some(end) = self.rest().find(quote) else {
                return self.fail(format!("the value of {attr} is never closed"));
            };
            let raw = &self.rest()[..end];
            if raw.contains('<') {
                return self.fail(format!("the value of {attr} holds a <"));
            }
            let value = decode(raw).map_err(|e| format!("line {}: {e}", self.line()))?;
            self.at += end + 1;
            if node.attrs.iter().any(|(existing, _)| *existing == attr) {
                return self.fail(format!("attribute {attr} twice"));
            }
            node.attrs.push((attr, value));
        }
        // Content.
        let mut text = String::new();
        loop {
            let Some(next) = self.rest().find('<') else {
                return self.fail(format!("<{}> is never closed", node.name));
            };
            text.push_str(&self.rest()[..next]);
            self.at += next;
            if self.rest().starts_with("<!--") {
                match self.rest().find("-->") {
                    Some(end) => self.at += end + 3,
                    None => return self.fail("a comment is never closed"),
                }
            } else if self.rest().starts_with("</") {
                self.at += 2;
                let closing = self.name()?;
                self.skip_space();
                if !self.rest().starts_with('>') || closing != node.name {
                    return self.fail(format!("</{closing}> does not close <{}>", node.name));
                }
                self.at += 1;
                break;
            } else if self.rest().starts_with("<!") || self.rest().starts_with("<?") {
                return self.fail("a DOCTYPE, CDATA section or processing instruction is not allowed");
            } else {
                let child = self.element(depth + 1)?;
                node.children.push(child);
            }
        }
        // Only a title or description may hold text, and it is thrown away.
        if !matches!(node.name.as_str(), "title" | "desc") && !text.trim().is_empty() {
            return self.fail(format!("<{}> holds text, which an icon cannot", node.name));
        }
        Ok(node)
    }
}

/// Whether the text starts with `<?xml` as a declaration (not `<?xml-stylesheet`).
fn is_xml_declaration(text: &str) -> bool {
    text.strip_prefix("<?xml")
        .and_then(|rest| rest.chars().next())
        .is_some_and(|next| next.is_ascii_whitespace() || next == '?')
}

/// Decodes the five named entities and numeric ones. Nothing else.
fn decode(raw: &str) -> Result<String, String> {
    if !raw.contains('&') {
        return Ok(raw.to_owned());
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find(';') else {
            return Err("an & with no ; after it".to_owned());
        };
        let entity = &after[..end];
        let decoded = match entity {
            "quot" => '"',
            "apos" => '\'',
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            _ => {
                let code = if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok()
                } else if let Some(decimal) = entity.strip_prefix('#') {
                    decimal.parse::<u32>().ok()
                } else {
                    None
                };
                // No NUL or other control characters, however they are spelled.
                let allowed = |c: &u32| (*c >= 0x20 || matches!(*c, 9 | 10 | 13)) && !(0x7f..=0x9f).contains(c);
                match code.filter(allowed).and_then(char::from_u32) {
                    Some(c) => c,
                    None => return Err(format!("the entity &{entity}; is not one an icon may use")),
                }
            }
        };
        out.push(decoded);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

// ----------------------------------------------------------------- checking

const SHAPES: [&str; 7] = ["path", "circle", "ellipse", "rect", "line", "polyline", "polygon"];

/// Attributes that are dropped: they cannot change what is drawn or are the page's business.
fn is_dropped(name: &str) -> bool {
    matches!(
        name,
        "class" | "id" | "role" | "focusable" | "tabindex" | "xml:space" | "version" | "xmlns:xlink"
            | "xmlns:svg" | "preserveAspectRatio" | "baseProfile" | "shape-rendering"
            | "enable-background"
    ) || name.starts_with("data-")
        || name.starts_with("aria-")
}

/// Presentation attributes the root may set for everything inside it.
fn is_inherited(name: &str) -> bool {
    matches!(
        name,
        "fill" | "stroke" | "stroke-width" | "stroke-linecap" | "stroke-linejoin" | "stroke-miterlimit"
            | "stroke-dasharray" | "stroke-dashoffset" | "stroke-opacity" | "fill-opacity" | "fill-rule"
            | "clip-rule" | "opacity"
    )
}

fn hint(name: &str) -> &'static str {
    match name {
        "script" => " (scripts cannot run in an icon)",
        "style" => " (the content security policy blocks styles: use attributes such as fill and stroke)",
        "use" | "symbol" | "image" | "a" | "foreignObject" | "iframe" => {
            " (an icon cannot refer to another file or embed one)"
        }
        "defs" | "clipPath" | "mask" | "filter" | "linearGradient" | "radialGradient" | "pattern"
        | "marker" => " (an icon is plain shapes: draw it with currentColor or a hex colour)",
        "text" | "tspan" => " (convert text to outlines first)",
        n if n.starts_with("animate") || n == "set" => " (an icon does not animate)",
        _ => "",
    }
}

fn is_number(text: &str) -> bool {
    let t = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (mantissa, exponent) = match t.find(['e', 'E']) {
        Some(at) => (&t[..at], Some(&t[at + 1..])),
        None => (t, None),
    };
    let mut parts = mantissa.splitn(2, '.');
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next();
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    let ok_mantissa = digits(whole)
        && fraction.is_none_or(digits)
        && (!whole.is_empty() || fraction.is_some_and(|f| !f.is_empty()));
    let ok_exponent = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    ok_mantissa && ok_exponent
}

fn is_length(text: &str) -> bool {
    let t = text.strip_suffix("px").or_else(|| text.strip_suffix('%')).unwrap_or(text);
    is_number(t.trim())
}

fn is_number_list(text: &str) -> bool {
    text.bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b',' | b'+' | b'-' | b'e' | b'E' | b' ' | b'\t' | b'\r' | b'\n'))
}

fn is_colour(text: &str) -> bool {
    match text {
        "none" | "currentColor" => true,
        _ => {
            text.strip_prefix('#')
                .is_some_and(|hex| matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        }
    }
}

fn is_transform(text: &str) -> bool {
    let mut rest = text.trim();
    if rest.is_empty() {
        return false;
    }
    while !rest.is_empty() {
        let Some(open) = rest.find('(') else {
            return false;
        };
        let name = rest[..open].trim();
        if !matches!(name, "translate" | "scale" | "rotate" | "skewX" | "skewY" | "matrix") {
            return false;
        }
        let Some(close) = rest[open..].find(')') else {
            return false;
        };
        if !is_number_list(&rest[open + 1..open + close]) {
            return false;
        }
        rest = rest[open + close + 1..].trim_start_matches([' ', ',', '\t', '\r', '\n']);
    }
    true
}

/// Checks one attribute of a shape or `g`; `Ok(false)` drops it.
fn check_attr(element: &str, name: &str, value: &str) -> Result<bool, String> {
    if is_dropped(name) {
        return Ok(false);
    }
    if name.starts_with("on") {
        return Err(format!("{name} is an event handler; icons cannot run code"));
    }
    if matches!(name, "href" | "xlink:href") {
        return Err(format!("{name} refers to something else; an icon cannot link or include"));
    }
    if name == "style" {
        return Err("style is blocked by the content security policy: use attributes such as fill and stroke".to_owned());
    }
    let ok = match name {
        "d" => {
            value.len() <= MAX_BYTES
                && value.bytes().all(|b| {
                    b.is_ascii_digit()
                        || b.is_ascii_whitespace()
                        || matches!(b, b'M' | b'm' | b'L' | b'l' | b'H' | b'h' | b'V' | b'v' | b'C' | b'c'
                            | b'S' | b's' | b'Q' | b'q' | b'T' | b't' | b'A' | b'a' | b'Z' | b'z' | b'.'
                            | b',' | b'+' | b'-' | b'e' | b'E')
                })
        }
        "points" => is_number_list(value),
        "cx" | "cy" | "r" | "rx" | "ry" | "x" | "y" | "x1" | "y1" | "x2" | "y2" | "width" | "height"
        | "stroke-width" | "stroke-miterlimit" | "stroke-dashoffset" | "pathLength" => is_length(value),
        "opacity" | "fill-opacity" | "stroke-opacity" => is_length(value),
        "fill" | "stroke" => is_colour(value),
        "stroke-linecap" => matches!(value, "butt" | "round" | "square"),
        "stroke-linejoin" => matches!(value, "miter" | "round" | "bevel" | "arcs" | "miter-clip"),
        "fill-rule" | "clip-rule" => matches!(value, "nonzero" | "evenodd"),
        "stroke-dasharray" => value == "none" || is_number_list(value),
        "vector-effect" => matches!(value, "none" | "non-scaling-stroke"),
        "transform" => is_transform(value),
        _ => {
            return Err(format!("the attribute {name} is not allowed on <{element}>"));
        }
    };
    if !ok {
        let shown: String = value.chars().take(40).collect();
        let advice = if matches!(name, "fill" | "stroke") {
            "; use none, currentColor or a hex colour such as #1a73e8"
        } else {
            ""
        };
        return Err(format!("{name}=\"{shown}\" is not a value an icon may have{advice}"));
    }
    Ok(true)
}

fn write_shape(node: &Node, out: &mut String) -> Result<(), String> {
    let name = node.name.as_str();
    if matches!(name, "title" | "desc") {
        if !node.children.is_empty() {
            return Err(format!("line {}: <{name}> holds an element", node.line));
        }
        return Ok(());
    }
    let is_group = name == "g";
    if !is_group && !SHAPES.contains(&name) {
        return Err(format!(
            "line {}: <{name}> is not allowed in an icon: only shapes ({}) and <g>{}",
            node.line,
            SHAPES.join(", "),
            hint(name)
        ));
    }
    if !is_group && !node.children.iter().all(|c| matches!(c.name.as_str(), "title" | "desc")) {
        return Err(format!("line {}: <{name}> has an element inside it", node.line));
    }
    out.push('<');
    out.push_str(name);
    for (attr, value) in &node.attrs {
        if check_attr(name, attr, value).map_err(|e| format!("line {}: <{name}> {e}", node.line))? {
            let _ = write!(out, " {attr}=\"{value}\"");
        }
    }
    if node.children.is_empty() {
        out.push_str("/>");
    } else {
        out.push('>');
        for child in &node.children {
            write_shape(child, out)?;
        }
        let _ = write!(out, "</{name}>");
    }
    Ok(())
}

/// Whether this element is a shape, or a group that holds one.
fn draws(node: &Node) -> bool {
    SHAPES.contains(&node.name.as_str()) || node.children.iter().any(draws)
}

/// Whether any element in the tree has the attribute.
fn mentions(node: &Node, attr: &str) -> bool {
    node.attrs.iter().any(|(name, _)| name == attr) || node.children.iter().any(|c| mentions(c, attr))
}

fn view_box(root: &Node) -> Result<String, String> {
    let attr = |name: &str| root.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str());
    let plain = |s: &str| {
        let t = s.trim_start_matches(['-', '+']);
        !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit() || b == b'.') && t.matches('.').count() <= 1
    };
    let numbers: Vec<String> = if let Some(value) = attr("viewBox") {
        value
            .split(|c: char| c.is_ascii_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    } else if let (Some(width), Some(height)) = (attr("width"), attr("height")) {
        let strip = |s: &str| s.strip_suffix("px").unwrap_or(s).to_owned();
        vec!["0".to_owned(), "0".to_owned(), strip(width), strip(height)]
    } else {
        return Err("the <svg> has no viewBox (or width and height to make one from)".to_owned());
    };
    if numbers.len() != 4 || !numbers.iter().all(|n| plain(n)) {
        return Err(format!("the viewBox is not four plain numbers: {numbers:?}"));
    }
    let size = |s: &str| s.parse::<f64>().unwrap_or(0.0);
    let (width, height) = (size(&numbers[2]), size(&numbers[3]));
    if !(width > 0.0 && height > 0.0 && width <= MAX_BOX && height <= MAX_BOX) {
        return Err(format!("the viewBox must be wider and taller than 0 and at most {MAX_BOX}"));
    }
    Ok(numbers.join(" "))
}

fn build(root: &Node) -> Result<Sanitised, String> {
    if root.name != "svg" {
        return Err(format!("line {}: the root is <{}>, not <svg>", root.line, root.name));
    }
    if let Some((_, ns)) = root.attrs.iter().find(|(n, _)| n == "xmlns")
        && ns != "http://www.w3.org/2000/svg"
    {
        return Err(format!("line {}: xmlns is {ns:?}, not the SVG namespace", root.line));
    }
    let view_box = view_box(root).map_err(|e| format!("line {}: {e}", root.line))?;

    // The root's own presentation attributes apply to everything inside it, so they
    // move to a <g> around the children. Anything else on the root is refused.
    let mut inherited = Vec::new();
    for (name, value) in &root.attrs {
        if matches!(name.as_str(), "viewBox" | "width" | "height" | "xmlns" | "x" | "y") || is_dropped(name) {
            continue;
        }
        if !is_inherited(name) {
            let reason = check_attr("svg", name, value).err().unwrap_or_else(|| {
                format!("the attribute {name} is not allowed on <svg>")
            });
            return Err(format!("line {}: <svg> {reason}", root.line));
        }
        check_attr("svg", name, value).map_err(|e| format!("line {}: <svg> {e}", root.line))?;
        inherited.push((name.clone(), value.clone()));
    }

    // Plain shapes with no paint are filled black in SVG. Here that is the text
    // colour. A drawing that sets only a fill gets no stroke; one that sets only a
    // stroke gets no fill, which is the toolkit's default for icons.
    let stroke = mentions(root, "stroke");
    let fill = mentions(root, "fill");
    if !stroke && !inherited.iter().any(|(n, _)| n == "stroke") {
        inherited.push(("stroke".to_owned(), "none".to_owned()));
    }
    if !fill && !stroke {
        inherited.push(("fill".to_owned(), "currentColor".to_owned()));
    }

    let mut body = String::new();
    for child in &root.children {
        write_shape(child, &mut body)?;
    }
    if !root.children.iter().any(draws) {
        return Err("the file draws nothing".to_owned());
    }
    let shapes = if inherited.is_empty() {
        body
    } else {
        let mut group = String::from("<g");
        for (name, value) in &inherited {
            let _ = write!(group, " {name}=\"{value}\"");
        }
        let _ = write!(group, ">{body}</g>");
        group
    };
    if shapes.len() > MAX_BYTES {
        return Err("the icon is larger than an icon should be".to_owned());
    }
    Ok(Sanitised { shapes, view_box })
}

// ------------------------------------------------------------------- macros

/// The directory the crate being compiled lives in.
pub(crate) fn manifest_dir(span: Span) -> syn::Result<PathBuf> {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| syn::Error::new(span, "CARGO_MANIFEST_DIR is not set; build with cargo"))
}

/// A path inside the crate: relative, with nothing that climbs out of it.
fn inside(relative: &LitStr) -> syn::Result<PathBuf> {
    let path = PathBuf::from(relative.value());
    let safe = !relative.value().is_empty()
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if !safe {
        return Err(syn::Error::new(
            relative.span(),
            "the path is relative to your crate's Cargo.toml and stays inside the crate (no `..`, no absolute path)",
        ));
    }
    Ok(path)
}

/// The tokens for one checked icon, which also make cargo rebuild when the file changes.
fn icon_tokens(ui: &TokenStream, absolute: &Path, source: &[u8], span: Span) -> syn::Result<TokenStream> {
    let icon = sanitize(source).map_err(|reason| {
        syn::Error::new(span, format!("{}: {reason}", absolute.display()))
    })?;
    let file = absolute.to_string_lossy().into_owned();
    let (shapes, view_box) = (icon.shapes, icon.view_box);
    Ok(quote! {
        {
            // Naming the file makes cargo rebuild when it changes.
            const _: &str = ::core::include_str!(#file);
            #ui::icons::Icon::__checked(#shapes, #view_box)
        }
    })
}

pub(crate) fn file(ui: &TokenStream, manifest: &Path, relative: &LitStr) -> syn::Result<TokenStream> {
    let absolute = manifest.join(inside(relative)?);
    let source = std::fs::read(&absolute)
        .map_err(|e| syn::Error::new(relative.span(), format!("{}: {e}", absolute.display())))?;
    icon_tokens(ui, &absolute, &source, relative.span())
}

/// A constant name from a file name: `arrow-left.svg` is `ARROW_LEFT`.
fn const_name(stem: &str) -> Option<String> {
    let name: Option<String> = stem
        .chars()
        .map(|c| match c {
            c if c.is_ascii_alphanumeric() => Some(c.to_ascii_uppercase()),
            '-' | '_' | ' ' | '.' => Some('_'),
            _ => None,
        })
        .collect();
    name.filter(|n| n.starts_with(|c: char| c.is_ascii_uppercase()) && n != "ALL")
}

pub(crate) fn set(ui: &TokenStream, manifest: &Path, relative: &LitStr) -> syn::Result<TokenStream> {
    let dir = manifest.join(inside(relative)?);
    let span = relative.span();
    let entries = std::fs::read_dir(&dir)
        .map_err(|e| syn::Error::new(span, format!("{}: {e}", dir.display())))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(syn::Error::new(span, format!("{} holds no .svg files", dir.display())));
    }

    let mut seen = std::collections::BTreeSet::new();
    let mut items = Vec::new();
    let mut table = Vec::new();
    for path in files {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_owned();
        let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or_default().to_owned();
        let Some(name) = const_name(&stem) else {
            return Err(syn::Error::new(
                span,
                format!(
                    "{}: the file name cannot be a constant name: use letters, digits, `-` and `_`, start with a letter, and avoid `all`",
                    path.display()
                ),
            ));
        };
        if !seen.insert(name.clone()) {
            return Err(syn::Error::new(span, format!("{}: another file gives the constant {name}", path.display())));
        }
        let source = std::fs::read(&path)
            .map_err(|e| syn::Error::new(span, format!("{}: {e}", path.display())))?;
        let value = icon_tokens(ui, &path, &source, span)?;
        let ident = syn::Ident::new(&name, span);
        let doc = format!("`{file_name}`.");
        items.push(quote! {
            #[doc = #doc]
            pub const #ident: #ui::icons::Icon = #value;
        });
        table.push(quote! { (#stem, #ident) });
    }
    Ok(quote! {
        #(#items)*
        /// Every icon of the set with its file name, sorted by file name.
        pub const ALL: &[(&str, #ui::icons::Icon)] = &[#(#table),*];
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARROW: &str = r#"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
  <!-- drawn by hand -->
  <title>Arrow</title>
  <path d="M5 12h14" stroke-linecap="round"/>
  <path d="m12 5 7 7-7 7"/>
</svg>"#;

    fn ok(svg: &str) -> Sanitised {
        sanitize(svg.as_bytes()).unwrap_or_else(|e| panic!("{e}\n{svg}"))
    }

    fn refused(svg: &str) -> String {
        match sanitize(svg.as_bytes()) {
            Ok(icon) => panic!("accepted: {icon:?}\n{svg}"),
            Err(reason) => reason,
        }
    }

    #[test]
    fn a_stroked_icon_keeps_its_shapes_and_root_paint() {
        let icon = ok(ARROW);
        assert_eq!(icon.view_box, "0 0 24 24");
        assert_eq!(
            icon.shapes,
            r#"<g fill="none" stroke="currentColor" stroke-width="2"><path d="M5 12h14" stroke-linecap="round"/><path d="m12 5 7 7-7 7"/></g>"#
        );
    }

    #[test]
    fn a_filled_glyph_takes_the_text_colour_and_no_stroke() {
        let icon = ok(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><path d="M0 0h512v512H0z"/></svg>"#);
        assert_eq!(icon.view_box, "0 0 512 512");
        assert_eq!(
            icon.shapes,
            r#"<g stroke="none" fill="currentColor"><path d="M0 0h512v512H0z"/></g>"#
        );
    }

    #[test]
    fn a_fill_only_drawing_gets_no_stroke() {
        let icon = ok(r##"<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" fill="#1a73e8"/></svg>"##);
        assert!(icon.shapes.starts_with(r#"<g stroke="none">"#), "{}", icon.shapes);
        assert!(!icon.shapes.contains("currentColor"));
    }

    #[test]
    fn other_viewboxes_are_kept_and_width_and_height_make_one() {
        for (svg, want) in [
            (r#"<svg viewBox="0 0 16 16"><path d="M1 1"/></svg>"#, "0 0 16 16"),
            (r#"<svg viewBox="0,0,20,20"><path d="M1 1"/></svg>"#, "0 0 20 20"),
            (r#"<svg viewBox="-2 -2 36 36"><path d="M1 1"/></svg>"#, "-2 -2 36 36"),
            (r#"<svg width="32px" height="32"><path d="M1 1"/></svg>"#, "0 0 32 32"),
        ] {
            assert_eq!(ok(svg).view_box, want);
        }
    }

    #[test]
    fn a_bad_viewbox_is_refused() {
        for svg in [
            r#"<svg><path d="M1 1"/></svg>"#,
            r#"<svg viewBox="0 0 24"><path d="M1 1"/></svg>"#,
            r#"<svg viewBox="0 0 0 0"><path d="M1 1"/></svg>"#,
            r#"<svg viewBox="0 0 99999 99999"><path d="M1 1"/></svg>"#,
            r#"<svg viewBox="0 0 24 24px"><path d="M1 1"/></svg>"#,
            r#"<svg viewBox="a b c d"><path d="M1 1"/></svg>"#,
        ] {
            assert!(refused(svg).contains("viewBox"), "{svg}");
        }
    }

    #[test]
    fn script_in_any_form_is_refused() {
        let wrap = |inner: &str| format!(r#"<svg viewBox="0 0 24 24">{inner}</svg>"#);
        for inner in [
            "<script>alert(1)</script>",
            r#"<script href="x.js"/>"#,
            r#"<path d="M1 1" onload="alert(1)"/>"#,
            r#"<path d="M1 1" onclick="x()"/>"#,
            r#"<path d="M1 1" ONCLICK="x()"/>"#,
            r#"<a href="javascript:alert(1)"><path d="M1 1"/></a>"#,
            r#"<foreignObject><div>hi</div></foreignObject>"#,
            r#"<path d="M1 1" style="fill:red"/>"#,
            "<style>path{fill:red}</style>",
            r#"<use href="other.svg#a"/>"#,
            r##"<use xlink:href="#a"/>"##,
            r#"<image href="https://example.com/x.png"/>"#,
            r#"<path d="M1 1" fill="url(#a)"/>"#,
            r#"<path d="M1 1" fill="javascript:alert(1)"/>"#,
            r#"<animate attributeName="d" to="M0 0"/>"#,
            r#"<set attributeName="fill" to="red"/>"#,
            r#"<text>hi</text>"#,
            r#"<defs><path id="a" d="M1 1"/></defs>"#,
            r#"<svg><path d="M1 1"/></svg>"#,
            r#"<PATH d="M1 1"/>"#,
            r#"<svg:path d="M1 1"/>"#,
            r#"<path d="M1 1" fill="red"/>"#,
            r#"<path d="M1 1" fill="&#106;avascript:x"/>"#,
            r#"<path d="M1 1" transform="translate(1) foo(2)"/>"#,
        ] {
            let reason = refused(&wrap(inner));
            assert!(reason.starts_with("line 1:"), "{inner}: {reason}");
        }
    }

    #[test]
    fn the_root_cannot_carry_what_a_shape_cannot() {
        for svg in [
            r#"<svg viewBox="0 0 24 24" onload="alert(1)"><path d="M1 1"/></svg>"#,
            r#"<svg viewBox="0 0 24 24" style="background:url(x)"><path d="M1 1"/></svg>"#,
            r#"<svg viewBox="0 0 24 24" href="x"><path d="M1 1"/></svg>"#,
            r#"<svg viewBox="0 0 24 24" xmlns="http://evil.example/"><path d="M1 1"/></svg>"#,
            r#"<svg viewBox="0 0 24 24" fill="red"><path d="M1 1"/></svg>"#,
            r#"<svg viewBox="0 0 24 24" transform="scale(9)"><path d="M1 1"/></svg>"#,
        ] {
            refused(svg);
        }
    }

    #[test]
    fn a_doctype_cdata_entities_and_instructions_are_refused() {
        for svg in [
            r#"<!DOCTYPE svg [<!ENTITY x "y">]><svg viewBox="0 0 24 24"><path d="M1 1"/></svg>"#,
            r#"<svg viewBox="0 0 24 24"><![CDATA[<script>]]></svg>"#,
            r#"<?xml-stylesheet href="x.css"?><svg viewBox="0 0 24 24"><path d="M1 1"/></svg>"#,
            r#"<svg viewBox="0 0 24 24"><?php echo 1 ?><path d="M1 1"/></svg>"#,
            r#"<svg viewBox="0 0 24 24"><path d="M1 1" id="&xxe;"/></svg>"#,
            r#"<svg viewBox="0 0 24 24"><path d="M1 1" id="&#0;"/></svg>"#,
            r#"<svg viewBox="0 0 24 24"><path d="M1 1" id="&amp"/></svg>"#,
        ] {
            refused(svg);
        }
    }

    #[test]
    fn markup_hidden_in_a_comment_or_text_is_not_carried() {
        let icon = ok(r#"<svg viewBox="0 0 24 24"><!-- <script>alert(1)</script> --><path d="M1 1"/></svg>"#);
        assert!(!icon.shapes.contains("script"));
        let icon = ok(r#"<svg viewBox="0 0 24 24"><title>&lt;script&gt;</title><desc>x</desc><path d="M1 1"/></svg>"#);
        assert!(!icon.shapes.contains("script"));
        refused(r#"<svg viewBox="0 0 24 24">hello<path d="M1 1"/></svg>"#);
        refused(r#"<svg viewBox="0 0 24 24"><path d="M1 1">text</path></svg>"#);
        refused(r#"<svg viewBox="0 0 24 24"><path d="M1 1"><circle r="1"/></path></svg>"#);
    }

    #[test]
    fn attributes_that_cannot_draw_are_dropped_not_kept() {
        let icon = ok(r##"<svg viewBox="0 0 24 24" class="x" id="y" role="img" aria-label="z" data-a="b"><path class="c" id="d" data-e="f" d="M1 1" fill="#fff" stroke="none"/></svg>"##);
        assert!(!icon.shapes.contains("class"));
        assert!(!icon.shapes.contains("aria"));
        assert!(!icon.shapes.contains("data-"));
        assert!(!icon.shapes.contains(" id="));
    }

    #[test]
    fn broken_markup_is_refused_with_a_line_number() {
        for svg in [
            "",
            "hello",
            "<svg",
            r#"<svg viewBox="0 0 24 24">"#,
            r#"<svg viewBox="0 0 24 24"><path d="M1 1"></svg>"#,
            r#"<svg viewBox="0 0 24 24"><path d="M1 1"/></svg><path d="M2 2"/>"#,
            r#"<svg viewBox="0 0 24 24"><path d=M1/></svg>"#,
            r#"<svg viewBox="0 0 24 24"><path d="M1 1" d="M2 2"/></svg>"#,
            r#"<svg viewBox="0 0 24 24"><path d="M1 <1"/></svg>"#,
            r#"<svg viewBox="0 0 24 24"><!-- never closed </svg>"#,
            r#"<svg viewBox="0 0 24 24"></svg>"#,
            r#"<svg viewBox="0 0 24 24"><g></g></svg>"#,
        ] {
            refused(svg);
        }
        let reason = refused("<svg viewBox=\"0 0 24 24\">\n<path d=\"M1 1\"/>\n<script/>\n</svg>");
        assert!(reason.starts_with("line 3:"), "{reason}");
    }

    #[test]
    fn size_and_depth_are_limited() {
        let big = format!(r#"<svg viewBox="0 0 24 24"><path d="{}"/></svg>"#, "M1 1 ".repeat(100_000));
        assert!(refused(&big).contains("bytes"));
        let deep = format!(
            r#"<svg viewBox="0 0 24 24">{}<path d="M1 1"/>{}</svg>"#,
            "<g>".repeat(20),
            "</g>".repeat(20)
        );
        assert!(refused(&deep).contains("nested"));
        let many = format!(r#"<svg viewBox="0 0 24 24">{}</svg>"#, r#"<path d="M1 1"/>"#.repeat(3000));
        assert!(refused(&many).contains("elements"));
    }

    #[test]
    fn bytes_that_are_not_text_are_refused() {
        assert!(sanitize(&[0xff, 0xfe, 0x00]).unwrap_err().contains("UTF-8"));
        assert!(sanitize(b"\xEF\xBB\xBF<svg viewBox=\"0 0 24 24\"><path d=\"M1 1\"/></svg>").is_ok());
    }

    #[test]
    fn numbers_and_transforms_have_a_grammar() {
        for good in ["1", "-1", "+1.5", ".5", "1.", "1e3", "1E-3", "10px", "50%"] {
            assert!(is_length(good), "{good}");
        }
        for bad in ["", "-", ".", "e3", "1e", "1..2", "1,2", "1 2", "calc(1px)", "NaN", "1em"] {
            assert!(!is_length(bad), "{bad}");
        }
        assert!(is_transform("translate(1 2) rotate(45)"));
        assert!(is_transform("matrix(1,0,0,1,0,0)"));
        assert!(!is_transform("translate(1 2) url(#a)"));
        assert!(!is_transform("translate(javascript:1)"));
        assert!(!is_transform(""));
    }

    #[test]
    fn names_for_constants() {
        assert_eq!(const_name("arrow-left").as_deref(), Some("ARROW_LEFT"));
        assert_eq!(const_name("Brand logo").as_deref(), Some("BRAND_LOGO"));
        assert_eq!(const_name("2fa"), None);
        assert_eq!(const_name("all"), None);
        assert_eq!(const_name("é"), None);
    }

    #[test]
    fn paths_must_stay_inside_the_crate() {
        let lit = |s: &str| LitStr::new(s, Span::call_site());
        assert!(inside(&lit("assets/icons/a.svg")).is_ok());
        assert!(inside(&lit("./a.svg")).is_ok());
        for bad in ["", "../a.svg", "a/../../b.svg", "/etc/passwd", "C:\\x\\a.svg"] {
            assert!(inside(&lit(bad)).is_err(), "{bad}");
        }
    }
}
