//! [`lower`]: a page's finished tree read into native parts.
//!
//! The tree a page builds already says what each part is: HTML is semantic
//! where it counts (a link, a form, a field, a table, a heading, a dialog),
//! and every component marks its parts with `data-slot` and its states with
//! attributes. Two readers of that contract exist already, the stylesheet and
//! the browser runtime. This is the third.
//!
//! - The **generic reader** here reads plain HTML: text and its runs,
//!   headings, links, buttons, forms and their controls, lists, tables,
//!   pictures, `details` and `dialog`.
//! - A **slot reader** ([`slots`](super::slots)) reads one component by its
//!   `data-slot`.
//!
//! Reading is strict. A reader that meets a structure it does not know says
//! so and names what it met; it never guesses. What could not be read is
//! listed in [`Lowered::misses`], and the page it stands on is shown in the
//! client's web view instead.

use std::borrow::Cow;

use navree_native::action::{Action, Command, Pair, Present, State};
use navree_native::input as i;
use navree_native::node as n;
use serde::Serialize;

use crate::node::{AttrValue, Element, Node};

pub(crate) type N = n::Node;

/// One thing a reader could not turn into a native part.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Miss {
    /// What it was, as `kind:name`: `slot:calendar`, `tag:canvas`,
    /// `hook:data-ui-sortable`, `cmd:fill`, `raw`.
    pub what: String,
    /// Why it could not be read.
    pub why: String,
}

/// A tree read into native parts, with what could not be read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Lowered {
    /// The native tree. Where a part could not be read stands a `web` node:
    /// empty from [`lower`], holding the part's markup from
    /// [`lower_keeping_markup`].
    pub node: N,
    /// What could not be read. When this is empty the whole tree is native.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub misses: Vec<Miss>,
    /// Decoration that was left out because nothing native draws it: a
    /// picture drawn in SVG that the page hides from assistive technology.
    /// The tree is still whole without it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<String>,
}

impl Lowered {
    /// Whether every part was read.
    pub fn is_native(&self) -> bool {
        self.misses.is_empty()
    }

    /// What could not be read, each thing once, in the order it was met: the
    /// `why` of a page-level fallback.
    pub fn why(&self) -> Vec<String> {
        let mut why: Vec<String> = Vec::new();
        for miss in &self.misses {
            if !why.contains(&miss.what) {
                why.push(miss.what.clone());
            }
        }
        why
    }
}

/// Reads `node` into native parts.
///
/// ```
/// use navree_native::node::{Button, Node, Prominence, Role};
/// use navree_native::Action;
/// use navree_ui::Button as UiButton;
/// use navree_ui::native::lower;
///
/// let lowered = lower(&UiButton::primary("Edit").href("/deals/1/edit").into());
/// assert!(lowered.is_native());
/// assert_eq!(
///     lowered.node,
///     Node::Button(Button {
///         label: "Edit".to_owned(),
///         role: Role::Primary,
///         prominence: Prominence::High,
///         action: Some(Action::navigate("/deals/1/edit")),
///         ..Button::default()
///     })
/// );
/// ```
pub fn lower(node: &Node) -> Lowered {
    lowered(node, false)
}

/// Reads `node` into native parts, as [`lower`] does, and keeps the markup of
/// each part that could not be read in the `web` node that stands for it.
pub fn lower_keeping_markup(node: &Node) -> Lowered {
    lowered(node, true)
}

fn lowered(node: &Node, keep_markup: bool) -> Lowered {
    let mut cx = Cx::new(keep_markup);
    let nodes = cx.sequence(std::slice::from_ref(node));
    cx.finish(nodes)
}

/// Why a reader gave up.
#[derive(Debug)]
pub(crate) struct Fail {
    /// What it met, when that is not the part it was reading: `hook:…`, `cmd:…`.
    pub(crate) what: Option<String>,
    /// Why it could not go on.
    pub(crate) why: String,
}

/// What a reader answers: the parts it read, or why not.
pub(crate) type Read<T = Vec<N>> = Result<T, Fail>;

/// A reader's "I do not know this structure".
pub(crate) fn fail<T>(why: impl Into<String>) -> Read<T> {
    Err(Fail {
        what: None,
        why: why.into(),
    })
}

/// A reader's "this other thing is in the way".
pub(crate) fn fail_on<T>(what: impl Into<String>, why: impl Into<String>) -> Read<T> {
    Err(Fail {
        what: Some(what.into()),
        why: why.into(),
    })
}

/// The deepest a tree is read: a page nests a few dozen levels, and a tree
/// built from data without end is not a page. A tree is measured before it is
/// read ([`web_depth`]) and one deeper than this is not read at all.
pub(crate) const DEEPEST: usize = 256;

/// The most stack, in bytes, one reading uses below where it began. The readers of
/// a page hold about two bands of levels at once (see [`BAND`]); this is the
/// bound under which that stays whatever a reader's frames come to, and under
/// which a thread of the default 2 MiB has the rest of its stack for the handler
/// around the reading, the envelope and the bytes written. A reading that would
/// pass it stops at the element it was about to read, as a part it could not
/// read (`depth`): a request can end a page in the web view, never a thread.
pub(crate) const STACK_BUDGET: usize = 1 << 20;

/// How deep `nodes` nest, in elements and fragments, counted with a list of its
/// own: a tree of any depth is measured without using the stack. Stops once it is
/// past `limit`, so a tree built without end costs `limit` levels, not its size.
pub(crate) fn web_depth<'a>(nodes: impl IntoIterator<Item = &'a Node>, limit: usize) -> usize {
    let mut deepest = 0;
    let mut walk: Vec<(&Node, usize)> = nodes.into_iter().map(|node| (node, 1)).collect();
    while let Some((node, level)) = walk.pop() {
        match node {
            Node::Element(element) => {
                deepest = deepest.max(level);
                if deepest > limit {
                    return deepest;
                }
                walk.extend(element.child_nodes().iter().map(|child| (child, level + 1)));
            }
            Node::Fragment(nodes) => {
                deepest = deepest.max(level);
                if deepest > limit {
                    return deepest;
                }
                walk.extend(nodes.iter().map(|child| (child, level + 1)));
            }
            _ => {}
        }
    }
    deepest
}

/// Where the stack stands: the address of a local, which is lower the deeper the
/// call.
#[inline(always)]
fn stack_at() -> usize {
    let marker = 0_u8;
    std::ptr::addr_of!(marker) as usize
}

/// How many levels one reading goes down before the levels below it are read
/// first, from the bottom up. However deep a tree, the stack holds about two
/// bands of readers at once.
const BAND: u16 = 16;

/// Parts read ahead of their place, from the bottom of a deep tree up.
struct Ahead {
    nodes: Vec<N>,
    misses: Vec<Miss>,
    dropped: Vec<String>,
}

/// One reading of a tree.
pub(crate) struct Cx {
    pub(crate) misses: Vec<Miss>,
    pub(crate) dropped: Vec<String>,
    keep_markup: bool,
    /// How many elements are being read around the current one, in this band.
    depth: u16,
    /// How deep the band starts.
    base: usize,
    /// How many forms are being read around the current element.
    forms: u16,
    /// The elements of a deep tree read ahead, by address, with the forms
    /// they stand in.
    ahead: std::collections::HashMap<(usize, bool), Ahead>,
    /// The page's token against forged posts, which the page's one post form
    /// carries and a post item of a menu names.
    pub(crate) csrf: Option<String>,
    /// The words of the page's dialog of a question, while its buttons that ask
    /// first are read.
    pub(crate) asking: Option<Asking>,
    /// Where the stack stood when the first element was read: what
    /// [`STACK_BUDGET`] is counted from. Zero until then.
    started: usize,
    /// The most stack the reading used, for the tests that measure it.
    #[cfg(test)]
    pub(crate) most: usize,
}

/// The dialog a page asks a question in: the words that stand over the
/// question a button names and the buttons that answer it.
pub(crate) struct Asking {
    pub(crate) title: String,
    pub(crate) confirm: String,
    pub(crate) cancel: Option<String>,
}

impl Cx {
    pub(crate) fn new(keep_markup: bool) -> Self {
        Self {
            misses: Vec::new(),
            dropped: Vec::new(),
            keep_markup,
            depth: 0,
            base: 0,
            forms: 0,
            ahead: std::collections::HashMap::new(),
            csrf: None,
            asking: None,
            started: 0,
            #[cfg(test)]
            most: 0,
        }
    }

    /// Whether `nodes`, read from the top, nest deeper than a page does. A reading
    /// inside a reading never asks: what it reads was measured with the rest.
    fn too_deep<'a>(&self, nodes: impl IntoIterator<Item = &'a Node>) -> bool {
        self.depth == 0 && self.base == 0 && web_depth(nodes, DEEPEST) > DEEPEST
    }

    /// What stands in place of a tree that is not read because it is too deep.
    #[inline(never)]
    fn depth_miss(&mut self) -> Vec<N> {
        self.misses.push(Miss {
            what: "depth".to_owned(),
            why: "the tree is deeper than a page is".to_owned(),
        });
        vec![N::Web(n::WebRegion {
            id: None,
            html: String::new(),
        })]
    }

    /// This reading, for a page whose posts carry `token`.
    pub(crate) fn with_csrf(mut self, token: Option<&str>) -> Self {
        self.csrf = token.map(str::to_owned);
        self
    }

    /// Reads ahead the elements under `top` that stand a whole number of
    /// bands below it, the deepest first, each from a stack of its own size:
    /// the reading of `top` then stops a band down, where each is waiting.
    #[inline(never)]
    fn read_ahead(&mut self, top: &Element) {
        // Every element under `top`, with how far below it, and whether a
        // form stands between them.
        let mut found: Vec<(&Element, usize, bool)> = Vec::new();
        let mut walk: Vec<(&Node, usize, bool)> = top
            .child_nodes()
            .iter()
            .map(|node| (node, 1, top.tag() == "form"))
            .collect();
        while let Some((node, below, in_form)) = walk.pop() {
            match node {
                Node::Element(element) => {
                    if below % usize::from(BAND) == 0 {
                        found.push((element, below, in_form));
                    }
                    let in_form = in_form || element.tag() == "form";
                    walk.extend(
                        element
                            .child_nodes()
                            .iter()
                            .map(|child| (child, below + 1, in_form)),
                    );
                }
                Node::Fragment(nodes) => {
                    walk.extend(nodes.iter().map(|child| (child, below, in_form)));
                }
                _ => {}
            }
        }
        if found.is_empty() {
            return;
        }
        found.sort_by_key(|found| std::cmp::Reverse(found.1));
        let (depth, base, forms) = (self.depth, self.base, self.forms);
        let level = base + usize::from(depth);
        for (element, below, in_form) in found {
            let (misses, dropped) = (
                std::mem::take(&mut self.misses),
                std::mem::take(&mut self.dropped),
            );
            self.depth = 0;
            self.base = level + below;
            self.forms = forms + u16::from(in_form);
            let mut nodes = Vec::new();
            self.read_one(element, &mut nodes);
            let read = Ahead {
                nodes,
                misses: std::mem::replace(&mut self.misses, misses),
                dropped: std::mem::replace(&mut self.dropped, dropped),
            };
            self.ahead
                .insert((std::ptr::from_ref(element) as usize, self.forms > 0), read);
        }
        (self.depth, self.base, self.forms) = (depth, base, forms);
    }

    /// The reading's result, with `nodes` as one node.
    pub(crate) fn finish(self, nodes: Vec<N>) -> Lowered {
        Lowered {
            node: one(nodes),
            misses: self.misses,
            dropped: self.dropped,
        }
    }

    /// Records that `element` could not be read and answers what stands in
    /// its place.
    #[inline(never)]
    pub(crate) fn miss(&mut self, what: String, why: String, element: &Element) -> N {
        self.misses.push(Miss { what, why });
        N::Web(n::WebRegion {
            id: id(element),
            html: if self.keep_markup {
                crate::render::render(&Node::Element(element.clone()))
            } else {
                String::new()
            },
        })
    }

    /// Records a miss that has no element of its own.
    #[inline(never)]
    pub(crate) fn miss_text(&mut self, what: &str, why: &str, html: &str) -> N {
        self.misses.push(Miss {
            what: what.to_owned(),
            why: why.to_owned(),
        });
        N::Web(n::WebRegion {
            id: None,
            html: if self.keep_markup {
                html.to_owned()
            } else {
                String::new()
            },
        })
    }

    /// The children of `element`, read.
    pub(crate) fn of(&mut self, element: &Element) -> Vec<N> {
        self.sequence(element.child_nodes())
    }

    /// Nodes in a row, read: text and inline elements join into one text
    /// node, and each other element is read by its slot or its tag.
    pub(crate) fn sequence(&mut self, nodes: &[Node]) -> Vec<N> {
        // Measured before anything else reads it: `flat` and the readers' own walks
        // go down a tree of fragments and inline elements on the stack.
        if self.too_deep(nodes) {
            return self.depth_miss();
        }
        let nodes = flat(nodes);
        self.flat_sequence(&nodes)
    }

    /// [`sequence`](Self::sequence) for nodes a reader picked out of an
    /// element's children.
    pub(crate) fn flat_sequence(&mut self, nodes: &[&Node]) -> Vec<N> {
        if self.too_deep(nodes.iter().copied()) {
            return self.depth_miss();
        }
        let mut out = Vec::new();
        let mut runs: Vec<n::Run> = Vec::new();
        // Labels that name a control elsewhere: where each stands, the
        // control's id, and its words.
        let mut labels: Vec<(usize, String, String)> = Vec::new();
        // The popups of menus that were read with their buttons.
        let mut paired: Vec<*const Element> = Vec::new();
        for (at, &node) in nodes.iter().enumerate() {
            if is_inline(node) {
                inline(node, Style::default(), None, &mut runs);
                continue;
            }
            flush(&mut runs, &mut out);
            match node {
                Node::Element(element) => {
                    if paired.contains(&std::ptr::from_ref(element)) {
                        continue;
                    }
                    if let Some((popup, read)) =
                        super::slots::menu_of(self, element, &nodes[at + 1..])
                    {
                        paired.push(std::ptr::from_ref(popup));
                        match read {
                            Ok(menu) => out.push(menu),
                            Err(fail) => {
                                let web = self.miss(
                                    fail.what.unwrap_or_else(|| "slot:dropdown-menu".to_owned()),
                                    fail.why,
                                    element,
                                );
                                out.push(web);
                            }
                        }
                        continue;
                    }
                    if let Some(named) = label_for(element) {
                        labels.push((out.len(), named, text_of(element)));
                        out.push(N::text(text_of(element)));
                        continue;
                    }
                    self.element(element, &mut out);
                }
                Node::Raw(html) if !html.as_str().trim().is_empty() => {
                    let web = self.miss_text("raw", "markup written as it is", html.as_str());
                    out.push(web);
                }
                Node::Static(html) if !html.trim().is_empty() => {
                    let web = self.miss_text("raw", "markup written as it is", html);
                    out.push(web);
                }
                _ => {}
            }
        }
        flush(&mut runs, &mut out);
        // A label beside its control becomes the control's label.
        let mut taken = Vec::new();
        for (at, named, words) in labels {
            let found = out
                .iter_mut()
                .enumerate()
                .filter(|(other, _)| *other != at)
                .find_map(|(_, node)| field_mut(node, &named));
            if let Some(field) = found {
                if field.label.is_none() {
                    field.label = Some(words);
                }
                taken.push(at);
            }
        }
        for at in taken.into_iter().rev() {
            out.remove(at);
        }
        out
    }

    /// One element, read by its slot's reader or, with no slot, by its tag.
    pub(crate) fn element(&mut self, element: &Element, out: &mut Vec<N>) {
        if !self.ahead.is_empty()
            && let Some(read) = self
                .ahead
                .remove(&(std::ptr::from_ref(element) as usize, self.forms > 0))
        {
            out.extend(read.nodes);
            self.misses.extend(read.misses);
            self.dropped.extend(read.dropped);
            return;
        }
        if self.depth == BAND {
            self.read_ahead(element);
        }
        self.read_one(element, out);
    }

    /// [`element`](Self::element), read where it stands.
    fn read_one(&mut self, element: &Element, out: &mut Vec<N>) {
        let here = stack_at();
        if self.started == 0 {
            self.started = here;
        }
        let used = self.started.abs_diff(here);
        #[cfg(test)]
        {
            self.most = self.most.max(used);
        }
        if self.base + usize::from(self.depth) >= DEEPEST || used >= STACK_BUDGET {
            let web = self.miss(
                "depth".to_owned(),
                "the tree is deeper than a page is".to_owned(),
                element,
            );
            out.push(web);
            return;
        }
        self.depth += 1;
        let read = match slot(element) {
            Some(slot) => match super::slots::read(self, &slot, element) {
                Some(read) => read.map_err(|fail| (format!("slot:{slot}"), fail)),
                None => Err((
                    format!("slot:{slot}"),
                    Fail {
                        what: None,
                        why: "no reader".to_owned(),
                    },
                )),
            },
            None => self
                .generic(element)
                .map_err(|fail| (format!("tag:{}", element.tag()), fail)),
        };
        self.depth -= 1;
        match read {
            Ok(nodes) => out.extend(nodes),
            Err((what, fail)) => {
                let web = self.miss(fail.what.unwrap_or(what), fail.why, element);
                out.push(web);
            }
        }
    }

    /// An element with no slot, read by its tag.
    ///
    /// This and what it calls for a container are on the stack once for every
    /// level of a page, so the work of each tag is in a function of its own
    /// and this one holds nearly nothing.
    pub(crate) fn generic(&mut self, element: &Element) -> Read {
        hooks(element, GENERIC_HOOKS)?;
        if is_special(element) {
            return self.special(element);
        }
        match element.tag() {
            "div" | "section" | "article" | "main" | "header" | "footer" | "aside" | "nav"
            | "figure" | "figcaption" | "address" | "blockquote" | "span" | "p" | "bdi" | "li"
            | "fieldset" | "legend" | "small" | "strong" | "em" | "b" | "time" | "dt" | "dd"
            | "caption" | "summary" | "hgroup" | "search" | "menu" | "center" => {
                self.container(element)
            }
            "label" => self.label(element),
            "ul" | "ol" => self.list(element),
            "dl" => self.facts(element).map(|facts| vec![N::Facts(facts)]),
            "table" => self.table(element).map(|table| vec![N::Table(table)]),
            "form" => self.form(element).map(|form| vec![N::Form(form)]),
            "a" => self.link(element),
            "details" => self.details(element),
            "dialog" => self.plain_dialog(element),
            _ => self.leaf(element),
        }
    }

    /// An element whose attributes say more than its tag: words for a screen
    /// reader, a region, something hidden, a popover, a click that runs commands.
    #[inline(never)]
    fn special(&mut self, element: &Element) -> Read {
        // Words for a screen reader only are not drawn.
        if has(element, "data-ui-sr") {
            return Ok(Vec::new());
        }
        // What a split of panes shows only while its panes are stacked, or only
        // while they are side by side: a client moves between panes its own way.
        if has(element, "data-split-only") {
            self.dropped.push("split-only".to_owned());
            return Ok(Vec::new());
        }
        if has(element, "data-ui-region") {
            return self.region(element).map(|region| vec![region]);
        }
        if has(element, "hidden") {
            // Something hidden that nothing can name is never shown.
            let Some(id) = id(element) else {
                return Ok(Vec::new());
            };
            let children = self.of(element);
            return Ok(vec![N::Stack(n::Stack {
                id: Some(id),
                hidden: true,
                children,
                ..n::Stack::default()
            })]);
        }
        if has(element, "popover") {
            let Some(id) = id(element) else {
                return fail("a popover with no id");
            };
            return Ok(vec![N::Popover(n::Popover {
                id,
                children: self.of(element),
            })]);
        }
        if has(element, "contenteditable") {
            return fail("editable text");
        }
        // Anything that runs commands when it is clicked is a link around what it holds.
        let Some(action) = self.action_of(element)? else {
            return fail("a click that does nothing");
        };
        Ok(vec![N::Link(n::Link {
            id: id(element),
            label: text_of(element),
            action,
            children: self.of(element),
        })])
    }

    /// A `<ul>` or an `<ol>`: one part an item.
    #[inline(never)]
    fn list(&mut self, element: &Element) -> Read {
        let mut children = Vec::new();
        for child in flat(element.child_nodes()) {
            match child {
                Node::Element(item) if item.tag() == "li" && slot(item).is_none() => {
                    hooks(item, &[])?;
                    let read = self.of(item);
                    children.push(with_id(one(read), id(item)));
                }
                Node::Element(item) if item.tag() == "li" => {
                    let mut read = Vec::new();
                    self.element(item, &mut read);
                    children.push(one(read));
                }
                Node::Element(item) => {
                    return fail(format!("a <{}> among the items of a list", item.tag()));
                }
                Node::Text(text) if text.trim().is_empty() => {}
                _ => return fail("text among the items of a list"),
            }
        }
        Ok(vec![N::List(n::List {
            id: id(element),
            children,
            ordered: element.tag() == "ol",
            ..n::List::default()
        })])
    }

    /// An `<a>`: a link, with what it holds when that is more than words. A
    /// link that leads nowhere is what it holds.
    #[inline(never)]
    fn link(&mut self, element: &Element) -> Read {
        let Some(action) = self.action_of(element)? else {
            return self.container(element);
        };
        let plain = element.child_nodes().iter().all(is_inline);
        Ok(vec![N::Link(n::Link {
            id: id(element),
            label: label_of(element),
            action,
            children: if plain { Vec::new() } else { self.of(element) },
        })])
    }

    /// A `<details>`: what opens under its summary.
    #[inline(never)]
    fn details(&mut self, element: &Element) -> Read {
        let mut label = None;
        let mut rest = Vec::new();
        for child in flat(element.child_nodes()) {
            match child {
                Node::Element(summary) if summary.tag() == "summary" && label.is_none() => {
                    label = Some(text_of(summary));
                }
                other => rest.push(other),
            }
        }
        let Some(label) = label else {
            return fail("no summary");
        };
        Ok(vec![N::Disclosure(n::Disclosure {
            id: id(element),
            label,
            open: has(element, "open"),
            children: self.flat_sequence(&rest),
        })])
    }

    /// A `<dialog>` that is no component's.
    #[inline(never)]
    fn plain_dialog(&mut self, element: &Element) -> Read {
        let Some(id) = id(element) else {
            return fail("a dialog with no id");
        };
        Ok(vec![N::Dialog(n::Dialog {
            id,
            title: attr(element, "aria-label").map(Cow::into_owned),
            open: has(element, "open"),
            children: self.of(element),
            ..n::Dialog::default()
        })])
    }

    /// An element that holds no parts to read: a heading, a control, a
    /// picture, a rule.
    #[inline(never)]
    fn leaf(&mut self, element: &Element) -> Read {
        match element.tag() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = element.tag().as_bytes()[1] - b'0';
                Ok(vec![N::Text(n::Text {
                    id: id(element),
                    value: text_of(element),
                    style: if level <= 2 {
                        n::TextStyle::Title
                    } else {
                        n::TextStyle::Headline
                    },
                    heading: Some(level),
                    ..n::Text::default()
                })])
            }
            "button" => self.button(element).map(|button| vec![N::Button(button)]),
            "input" | "select" | "textarea" => match self.control(element)? {
                Control::Field(field) => Ok(vec![N::Field(*field)]),
                Control::Button(button) => Ok(vec![N::Button(*button)]),
                Control::Hidden(_) if self.forms > 0 => Ok(Vec::new()),
                Control::Hidden(_) => fail("a hidden value outside any form"),
            },
            "img" => {
                let Some(src) = attr(element, "src") else {
                    return fail("a picture with no source");
                };
                Ok(vec![N::Image(n::Image {
                    id: id(element),
                    src: src.into_owned(),
                    alt: attr(element, "alt")
                        .map(Cow::into_owned)
                        .unwrap_or_default(),
                    width: number(element, "width"),
                    height: number(element, "height"),
                    fit: None,
                })])
            }
            "svg" => {
                // A drawing the page hides from assistive technology is decoration.
                if attr(element, "aria-hidden").as_deref() == Some("true") {
                    self.dropped.push("svg".to_owned());
                    Ok(Vec::new())
                } else {
                    fail("a drawing with a meaning nothing here reads")
                }
            }
            "hr" => Ok(vec![N::Divider(n::Divider { id: id(element) })]),
            "br" | "wbr" | "noscript" | "colgroup" | "col" => Ok(Vec::new()),
            "pre" => Ok(vec![N::Code(n::Code {
                id: id(element),
                text: raw_text(element),
                lang: None,
            })]),
            "code" | "kbd" | "samp" => Ok(vec![N::Text(n::Text {
                id: id(element),
                value: text_of(element),
                style: n::TextStyle::Mono,
                ..n::Text::default()
            })]),
            // A value within a range, read at a glance: a level, not how far along.
            "meter" => {
                let number = |name: &str, otherwise: f64| {
                    attr(element, name)
                        .and_then(|text| text.trim().parse::<f64>().ok())
                        .filter(|value| value.is_finite())
                        .unwrap_or(otherwise)
                };
                let (min, max) = (number("min", 0.0), number("max", 1.0));
                if max <= min {
                    return fail("a meter whose range is empty");
                }
                let value = number("value", min).clamp(min, max);
                let shown = text_of(element);
                Ok(vec![N::Gauge(n::Gauge {
                    id: id(element),
                    value: json_number(value),
                    min: json_number(min),
                    max: json_number(max),
                    label: attr(element, "aria-label").map(Cow::into_owned),
                    shown: Some(shown).filter(|shown| !shown.is_empty()),
                    tone: None,
                    ..n::Gauge::default()
                })])
            }
            "progress" => {
                let max = attr(element, "max")
                    .and_then(|max| max.parse::<f64>().ok())
                    .filter(|max| *max > 0.0)
                    .unwrap_or(1.0);
                let value = attr(element, "value")
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(|value| ((value / max).clamp(0.0, 1.0) * 1000.0).round() as u16);
                Ok(vec![N::Progress(n::Progress {
                    id: id(element),
                    value,
                    label: attr(element, "aria-label").map(Cow::into_owned),
                    ..n::Progress::default()
                })])
            }
            other => fail(format!("a <{other}> has no native meaning")),
        }
    }

    /// A `<label>` around one control is that control's field, named by the
    /// label's words; any other label is what it holds.
    #[inline(never)]
    fn label(&mut self, element: &Element) -> Read {
        let children = flat(element.child_nodes());
        let mut controls = children.iter().filter_map(|child| match child {
            Node::Element(child) if matches!(child.tag(), "input" | "select" | "textarea") => {
                Some(child)
            }
            _ => None,
        });
        let (Some(inner), None) = (controls.next(), controls.next()) else {
            return self.container(element);
        };
        let words_only = children.iter().all(|child| match child {
            Node::Element(child) if std::ptr::eq(child, inner) => true,
            other => is_inline(other),
        });
        if !words_only {
            return self.container(element);
        }
        let mut words = String::new();
        for child in &children {
            if !matches!(child, Node::Element(child) if std::ptr::eq(child, inner)) {
                collect_text(child, &mut words);
                words.push(' ');
            }
        }
        match self.control(inner)? {
            Control::Field(mut field) => {
                let words = tidy(&words);
                if !words.is_empty() {
                    field.label = Some(words);
                }
                Ok(vec![N::Field(*field)])
            }
            _ => self.container(element),
        }
    }

    /// What a plain container holds: nothing, its one part, or its parts as
    /// a stack. An id stays on what it becomes.
    #[inline(never)]
    fn container(&mut self, element: &Element) -> Read {
        let children = self.of(element);
        let id = id(element);
        Ok(match (children.len(), id) {
            (0, None) => Vec::new(),
            (1, None) => children,
            (1, Some(id)) => vec![with_id(one(children), Some(id))],
            (_, id) => vec![N::Stack(n::Stack {
                id,
                children,
                ..n::Stack::default()
            })],
        })
    }

    /// A part that fetches itself.
    #[inline(never)]
    pub(crate) fn region(&mut self, element: &Element) -> Read<N> {
        let Some(id) = id(element) else {
            return fail("a region with no id");
        };
        let Some(src) = attr(element, "data-ui-src") else {
            return fail("a region with no source");
        };
        Ok(N::Region(n::Region {
            id,
            src: src.into_owned(),
            lazy: has(element, "data-ui-lazy"),
            live: attr(element, "data-ui-live-src").map(|live| n::Live {
                src: live.into_owned(),
                event: attr(element, "data-ui-live-event").map(Cow::into_owned),
            }),
            children: self.of(element),
        }))
    }

    /// A `<dl>`: terms with their descriptions, loose or grouped in `<div>`s.
    #[inline(never)]
    fn facts(&mut self, element: &Element) -> Read<n::Facts> {
        let mut items: Vec<n::Fact> = Vec::new();
        let mut pairs: Vec<&Element> = Vec::new();
        for child in flat(element.child_nodes()) {
            match child {
                Node::Element(group) if group.tag() == "div" => {
                    for inner in flat(group.child_nodes()) {
                        match inner {
                            Node::Element(part) => pairs.push(part),
                            _ => return fail("text between the terms of a list"),
                        }
                    }
                }
                Node::Element(part) => pairs.push(part),
                _ => return fail("text between the terms of a list"),
            }
        }
        for part in pairs {
            match part.tag() {
                "dt" => items.push(n::Fact {
                    label: text_of(part),
                    ..n::Fact::default()
                }),
                "dd" => {
                    let value = self.of(part);
                    match items.last_mut() {
                        Some(fact) => fact.value.extend(value),
                        None => return fail("a description before any term"),
                    }
                }
                other => return fail(format!("a <{other}> among the terms of a list")),
            }
        }
        Ok(n::Facts {
            id: id(element),
            items,
        })
    }

    /// A `<table>`: its caption, its columns from the head, its rows.
    #[inline(never)]
    pub(crate) fn table(&mut self, element: &Element) -> Read<n::Table> {
        let mut table = n::Table {
            id: id(element),
            ..n::Table::default()
        };
        for child in flat(element.child_nodes()) {
            let Node::Element(part) = child else {
                return fail("text between the parts of a table");
            };
            match part.tag() {
                "caption" => table.caption = Some(text_of(part)),
                "colgroup" => {}
                "thead" => {
                    let rows: Vec<&Element> = elements(part)?;
                    let [row] = rows.as_slice() else {
                        return fail("a head of more or less than one row");
                    };
                    for cell in elements(row)? {
                        spans(cell)?;
                        table.columns.push(n::Column {
                            key: attr(cell, "data-col").map(Cow::into_owned),
                            label: text_of(cell),
                            numeric: has(cell, "data-numeric"),
                            align: align_of(cell),
                            secondary: has(cell, "data-hide-mobile"),
                            ..n::Column::default()
                        });
                    }
                }
                "tbody" | "tfoot" => {
                    for row in elements(part)? {
                        table.rows.push(self.table_row(row)?);
                    }
                }
                "tr" => table.rows.push(self.table_row(part)?),
                other => return fail(format!("a <{other}> in a table")),
            }
        }
        Ok(table)
    }

    /// One `<tr>` of a table's body.
    #[inline(never)]
    pub(crate) fn table_row(&mut self, row: &Element) -> Read<n::TableRow> {
        if row.tag() != "tr" {
            return fail(format!("a <{}> among the rows of a table", row.tag()));
        }
        let mut cells = Vec::new();
        for cell in elements(row)? {
            if !matches!(cell.tag(), "td" | "th") {
                return fail(format!("a <{}> among the cells of a row", cell.tag()));
            }
            spans(cell)?;
            let read = self.of(cell);
            cells.push(if read.is_empty() {
                N::text("")
            } else {
                one(read)
            });
        }
        Ok(n::TableRow {
            id: attr(row, "data-key")
                .or_else(|| attr(row, "id"))
                .map(Cow::into_owned),
            cells,
            ..n::TableRow::default()
        })
    }

    /// A `<form>`: where it goes, what it sends unseen, and what it holds.
    #[inline(never)]
    pub(crate) fn form(&mut self, element: &Element) -> Read<n::Form> {
        hooks(element, &["data-ui-form", "data-ui-target"])?;
        let method = match attr(element, "method").as_deref() {
            None | Some("get") | Some("GET") => n::Method::Get,
            Some("post") | Some("POST") => n::Method::Post,
            Some(other) => return fail(format!("a form sent by `{other}`")),
        };
        let mut hidden = Vec::new();
        hidden_in(element.child_nodes(), &mut hidden);
        self.forms += 1;
        let children = self.of(element);
        self.forms -= 1;
        Ok(n::Form {
            id: id(element),
            method,
            action: attr(element, "action")
                .map(Cow::into_owned)
                .unwrap_or_default(),
            hidden,
            children,
        })
    }

    /// Whether a form is being read around the current element.
    pub(crate) fn in_form(&self) -> bool {
        self.forms > 0
    }

    /// Runs `read` as inside a form: for a reader that knows which form the
    /// controls it reads belong to, wherever they stand.
    pub(crate) fn as_form<T>(&mut self, read: impl FnOnce(&mut Self) -> T) -> T {
        self.forms += 1;
        let out = read(self);
        self.forms -= 1;
        out
    }

    /// An `<input>`, a `<select>` or a `<textarea>`.
    #[inline(never)]
    pub(crate) fn control(&mut self, element: &Element) -> Read<Control> {
        hooks(element, CONTROL_HOOKS)?;
        if has(element, "form") && self.forms == 0 {
            return fail("a control that names a form elsewhere");
        }
        let name = attr(element, "name")
            .map(Cow::into_owned)
            .unwrap_or_default();
        let value = attr(element, "value")
            .map(Cow::into_owned)
            .unwrap_or_default();
        let text = |kind: i::TextKind| {
            let mut input = i::TextInput {
                kind,
                value: value.clone(),
                placeholder: attr(element, "placeholder").map(Cow::into_owned),
                max_length: number(element, "maxlength"),
                min: attr(element, "min").map(Cow::into_owned),
                max: attr(element, "max").map(Cow::into_owned),
                step: attr(element, "step").map(Cow::into_owned),
                autofill: attr(element, "autocomplete")
                    .filter(|fill| fill != "off")
                    .map(Cow::into_owned),
                readonly: has(element, "readonly"),
                ..i::TextInput::default()
            };
            if matches!(kind, i::TextKind::Number | i::TextKind::Decimal) {
                plain_number(&mut input);
            }
            i::Input::Text(input)
        };
        let date = |kind: i::DateKind| {
            i::Input::Date(i::DateInput {
                kind,
                value: value.clone(),
                min: attr(element, "min").map(Cow::into_owned),
                max: attr(element, "max").map(Cow::into_owned),
                ..i::DateInput::default()
            })
        };
        let input = match element.tag() {
            "textarea" => i::Input::Text(i::TextInput {
                kind: i::TextKind::Multiline,
                value: raw_text(element),
                placeholder: attr(element, "placeholder").map(Cow::into_owned),
                max_length: number(element, "maxlength"),
                readonly: has(element, "readonly"),
                ..i::TextInput::default()
            }),
            "select" => {
                let mut options = Vec::new();
                let mut chosen = Vec::new();
                let mut placeholder = None;
                options_in(element, &mut options, &mut chosen, &mut placeholder)?;
                if has(element, "multiple") {
                    i::Input::Choices(i::ChoicesInput {
                        options,
                        values: chosen,
                        ..i::ChoicesInput::default()
                    })
                } else {
                    i::Input::Choice(i::ChoiceInput {
                        kind: i::ChoiceKind::Menu,
                        // With none marked, a browser shows the first.
                        value: chosen.into_iter().next().or_else(|| match &placeholder {
                            Some(_) => None,
                            None => options.first().map(|option| option.value.clone()),
                        }),
                        options,
                        placeholder,
                        ..i::ChoiceInput::default()
                    })
                }
            }
            _ => match attr(element, "type").as_deref().unwrap_or("text") {
                "hidden" => {
                    return Ok(Control::Hidden(n::Hidden {
                        name,
                        value,
                        id: has(element, "data-ui-value").then(|| id(element)).flatten(),
                    }));
                }
                "submit" | "button" => {
                    return Ok(Control::Button(Box::new(n::Button {
                        id: id(element),
                        label: value,
                        action: self.action_of(element)?,
                        disabled: has(element, "disabled"),
                        ..n::Button::default()
                    })));
                }
                "text" => match (
                    attr(element, "inputmode").as_deref(),
                    attr(element, "autocomplete").as_deref(),
                ) {
                    (_, Some("one-time-code")) => text(i::TextKind::OneTimeCode),
                    (Some("decimal"), _) => text(i::TextKind::Decimal),
                    (Some("numeric"), _) => text(i::TextKind::Number),
                    (Some("tel"), _) => text(i::TextKind::Phone),
                    (Some("email"), _) => text(i::TextKind::Email),
                    (Some("url"), _) => text(i::TextKind::Url),
                    (Some("search"), _) => text(i::TextKind::Search),
                    _ => text(i::TextKind::Plain),
                },
                "email" => text(i::TextKind::Email),
                "password" => text(i::TextKind::Password),
                "number" => match attr(element, "step").as_deref() {
                    Some(step) if step.contains('.') || step == "any" => text(i::TextKind::Decimal),
                    _ => text(i::TextKind::Number),
                },
                "tel" => text(i::TextKind::Phone),
                "url" => text(i::TextKind::Url),
                "search" => text(i::TextKind::Search),
                "date" => date(i::DateKind::Date),
                "time" => date(i::DateKind::Time),
                "datetime-local" => date(i::DateKind::Datetime),
                "checkbox" => i::Input::Toggle(i::ToggleInput {
                    kind: if attr(element, "role").as_deref() == Some("switch") {
                        i::ToggleKind::Switch
                    } else {
                        i::ToggleKind::Checkbox
                    },
                    on: has(element, "checked"),
                    value: attr(element, "value").map(Cow::into_owned),
                }),
                "range" => i::Input::Slider(i::SliderInput {
                    min: attr(element, "min").map_or_else(|| "0".to_owned(), Cow::into_owned),
                    max: attr(element, "max").map_or_else(|| "100".to_owned(), Cow::into_owned),
                    step: attr(element, "step").map(Cow::into_owned),
                    value,
                }),
                "color" => i::Input::Colour(i::ColourInput { value }),
                "file" => i::Input::File(i::FileInput {
                    accept: attr(element, "accept").map(Cow::into_owned),
                    multiple: has(element, "multiple"),
                    capture: attr(element, "capture").map(Cow::into_owned),
                    ..i::FileInput::default()
                }),
                "radio" => return fail("a radio button outside a group that reads it"),
                other => return fail(format!("an input of type `{other}`")),
            },
        };
        Ok(Control::Field(Box::new(n::Field {
            id: id(element),
            name,
            label: attr(element, "aria-label").map(Cow::into_owned),
            help: None,
            error: None,
            required: has(element, "required"),
            disabled: has(element, "disabled"),
            input,
            changed: self.changed_of(element)?,
            ..n::Field::default()
        })))
    }

    /// What a control does when its value changes, besides holding it: the
    /// commands written for a change or for typing, as one action. A lone send
    /// of a form is the field's own submit.
    pub(crate) fn changed_of(&mut self, element: &Element) -> Read<Option<Action>> {
        let mut list = Vec::new();
        for hook in ["data-ui-input", "data-ui-change"] {
            if let Some(json) = attr(element, hook) {
                list.extend(commands(&json)?);
            }
        }
        Ok(match list.as_slice() {
            [] => None,
            [Command::Submit { id }] => Some(Action::Submit {
                form: Some(id.clone()),
                href: None,
                with: None,
            }),
            _ => Some(Action::Commands { list }),
        })
    }

    /// A `<button>`, or a link drawn as one: its words, its icon, what it
    /// means and what it does.
    #[inline(never)]
    pub(crate) fn button(&mut self, element: &Element) -> Read<n::Button> {
        hooks(
            element,
            &["data-ui-click", "data-ui-refresh", "data-ui-confirm"],
        )?;
        let mut icon = None;
        let mut said = None;
        for child in flat(element.child_nodes()) {
            if let Node::Static(text) = child
                && let Some(role) = crate::hint::role_of(text)
            {
                said = Some(role);
                continue;
            }
            let Node::Element(part) = child else { continue };
            match slot(part).as_deref() {
                Some("icon") => {
                    if icon.is_none() {
                        icon = Some(super::slots::icon(part)?);
                    }
                }
                Some("button-label" | "button-loading" | "kbd" | "spinner") | None => {}
                Some(other) => return fail(format!("a button that holds a `{other}`")),
            }
        }
        let words = match flat(element.child_nodes())
            .into_iter()
            .find_map(|child| match child {
                Node::Element(part) if slot(part).as_deref() == Some("button-label") => Some(part),
                _ => None,
            }) {
            Some(label) => text_of(label),
            None => text_of(element),
        };
        let named = attr(element, "aria-label").map(Cow::into_owned);
        let icon_only = words.is_empty() && icon.is_some();
        let label = if words.is_empty() {
            match named {
                Some(named) => named,
                None => return fail("a button with no words and no name"),
            }
        } else {
            words
        };
        let (role, prominence) = match attr(element, "data-variant").as_deref() {
            Some("primary") => (n::Role::Primary, n::Prominence::High),
            Some("destructive") => (n::Role::Destructive, n::Prominence::High),
            Some("outline") => (n::Role::Secondary, n::Prominence::Medium),
            Some("ghost" | "link") => (n::Role::None, n::Prominence::Low),
            None | Some("secondary") => (n::Role::None, n::Prominence::Medium),
            Some(other) => return fail(format!("a button of the `{other}` variant")),
        };
        // What the page said the button is for comes before what its look says.
        let role = match said {
            Some(crate::Role::Primary) => n::Role::Primary,
            Some(crate::Role::Secondary) => n::Role::Secondary,
            Some(crate::Role::Destructive) => n::Role::Destructive,
            Some(crate::Role::Cancel) => n::Role::Cancel,
            Some(crate::Role::Confirm) => n::Role::Confirm,
            Some(crate::Role::Create) => n::Role::Create,
            Some(crate::Role::Navigation) => n::Role::Navigation,
            None => role,
        };
        let ask = match attr(element, "data-ui-confirm") {
            Some(question) => {
                let Some(asking) = &self.asking else {
                    return fail("a question to ask first, with no dialog to ask it in");
                };
                Some(n::Question {
                    title: asking.title.clone(),
                    text: Some(question.into_owned()).filter(|text| !text.is_empty()),
                    confirm: asking.confirm.clone(),
                    cancel: asking.cancel.clone(),
                })
            }
            None => None,
        };
        let disabled =
            has(element, "disabled") || attr(element, "aria-disabled").as_deref() == Some("true");
        let said = element.said();
        let hints = said.as_ref();
        // What the author said of why it is off comes first; a page that says it in the
        // button's own `title` is read.
        let reason = if disabled {
            hints
                .and_then(|hints| hints.reason.clone())
                .or_else(|| attr(element, "title").map(Cow::into_owned))
                .filter(|reason| !reason.is_empty())
        } else {
            None
        };
        Ok(n::Button {
            id: id(element),
            label,
            icon,
            icon_only,
            role,
            standard: hints.and_then(|hints| hints.standard).map(standard_of),
            prominence,
            undoable: hints.is_some_and(|hints| hints.undoable),
            ask,
            disabled,
            reason,
            pending: has(element, "data-pending"),
            action: self.action_of(element)?,
            ..n::Button::default()
        })
    }

    /// What a link or a button does, from its attributes.
    #[inline(never)]
    pub(crate) fn action_of(&mut self, element: &Element) -> Read<Option<Action>> {
        let commands = match attr(element, "data-ui-click") {
            Some(json) => Some(commands(&json)?),
            None => None,
        };
        let mut actions: Vec<Action> = Vec::new();
        if element.tag() == "a"
            && let Some(href) = attr(element, "href")
        {
            let outside = href.starts_with("http://")
                || href.starts_with("https://")
                || href.starts_with("mailto:")
                || href.starts_with("tel:")
                || attr(element, "target").as_deref() == Some("_blank")
                || has(element, "download");
            actions.push(Action::Navigate {
                href: href.into_owned(),
                present: if outside {
                    Present::External
                } else if let Some(how) = element.said().and_then(|hints| hints.presented) {
                    // What the page said of how the screen it leads to is presented.
                    match how {
                        crate::Presented::Push => Present::Push,
                        crate::Presented::Replace => Present::Replace,
                        crate::Presented::Root => Present::Root,
                        crate::Presented::Sheet => Present::Sheet,
                        crate::Presented::Task => Present::Task,
                        crate::Presented::Full => Present::Full,
                    }
                } else if has(element, "data-ui-in-region") {
                    Present::Replace
                } else {
                    Present::Auto
                },
            });
        }
        if let Some(command) = attr(element, "command") {
            let Some(target) = attr(element, "commandfor") else {
                return fail("a command for nothing");
            };
            let id = target.into_owned();
            actions.push(Action::command(match command.as_ref() {
                "show-modal" | "show-popover" | "toggle-popover" => Command::Open { id },
                "close" | "request-close" | "hide-popover" => Command::Close { id },
                other => return fail_on(format!("cmd:{other}"), "a command nothing here runs"),
            }));
        } else if let Some(target) = attr(element, "popovertarget") {
            let id = target.into_owned();
            actions.push(Action::command(
                match attr(element, "popovertargetaction").as_deref() {
                    Some("hide") => Command::Close { id },
                    _ => Command::Open { id },
                },
            ));
        }
        if has(element, "data-ui-refresh") {
            actions.push(Action::Refresh);
        }
        if let Some(list) = commands {
            actions.push(Action::Commands { list });
        }
        if element.tag() != "a" {
            match attr(element, "type").as_deref() {
                // A post that names the page's one post form: that form holds only
                // the session's token, so the post is a request of its own.
                Some("submit")
                    if attr(element, "form").as_deref() == Some(crate::render::POST_FORM) =>
                {
                    let Some(href) = attr(element, "formaction") else {
                        return fail("a post with no address");
                    };
                    // The session's token goes only with a post to this site.
                    let mut fields: Vec<Pair> = self
                        .csrf
                        .iter()
                        .filter(|_| super::swap::on_this_site(&href))
                        .map(|token| Pair {
                            name: crate::form::CSRF_FIELD.to_owned(),
                            value: token.clone(),
                        })
                        .collect();
                    fields.extend(attr(element, "name").map(|name| {
                        Pair {
                            name: name.into_owned(),
                            value: attr(element, "value")
                                .map(Cow::into_owned)
                                .unwrap_or_default(),
                        }
                    }));
                    actions.push(Action::Request {
                        method: n::Method::Post,
                        href: href.into_owned(),
                        fields,
                    });
                }
                Some("submit") => actions.push(Action::Submit {
                    form: attr(element, "form").map(Cow::into_owned),
                    href: attr(element, "formaction").map(Cow::into_owned),
                    with: attr(element, "name").map(|name| Pair {
                        name: name.into_owned(),
                        value: attr(element, "value")
                            .map(Cow::into_owned)
                            .unwrap_or_default(),
                    }),
                }),
                Some("reset") => return fail("a button that resets its form"),
                _ => {}
            }
        }
        match actions.len() {
            0 => Ok(None),
            1 => Ok(actions.pop()),
            _ => fail("a control that does two things at once"),
        }
    }
}

/// Whether an element's attributes say more than its tag does.
fn is_special(element: &Element) -> bool {
    element.attrs().iter().any(|attr| {
        matches!(
            attr.name,
            "data-ui-sr"
                | "data-ui-region"
                | "hidden"
                | "popover"
                | "contenteditable"
                | "data-split-only"
        ) || (attr.name == "data-ui-click" && !matches!(element.tag(), "a" | "button"))
    })
}

/// What a form control reads as.
pub(crate) enum Control {
    /// A field a reader fills.
    Field(Box<n::Field>),
    /// A button written as an input.
    Button(Box<n::Button>),
    /// A value sent unseen.
    Hidden(n::Hidden),
}

/// The runtime's hooks that change nothing a native client must do: fetching
/// ahead, shortcuts, how a page is swapped in a browser, what a `fill` copies.
const QUIET_HOOKS: &[&str] = &[
    "data-ui-prefetch",
    "data-ui-hotkey",
    "data-ui-hotkey-scope",
    "data-ui-boost",
    "data-ui-keep-attrs",
    "data-ui-in-region",
    "data-ui-cell",
    "data-ui-row-key",
    "data-ui-place",
    "data-ui-sr",
    // How long a filter waits for typing to stop before it asks; a drag a press may start.
    "data-ui-debounce",
    "data-ui-drag",
    // The button that clears a field, and the field that has one: a script's.
    "data-ui-clearable",
];

/// The hooks a form control carries: the value a command sets, and what is done
/// when it changes.
pub(crate) const CONTROL_HOOKS: &[&str] = &["data-ui-value", "data-ui-change", "data-ui-input"];

/// The hooks the generic reader answers for: commands on a click, a region
/// that fetches itself, a form the runtime posts.
const GENERIC_HOOKS: &[&str] = &[
    "data-ui-click",
    "data-ui-region",
    "data-ui-src",
    "data-ui-lazy",
    "data-ui-live-src",
    "data-ui-live-event",
    "data-ui-live-catch-up",
    "data-ui-form",
    "data-ui-target",
    "data-ui-value",
    "data-ui-refresh",
];

/// Holds an element to the hooks its reader knows: a runtime hook is
/// behaviour, and behaviour nothing here understands can't be passed over.
pub(crate) fn hooks(element: &Element, known: &[&str]) -> Read<()> {
    for attr in element.attrs() {
        if attr.name.starts_with("data-ui-")
            && !QUIET_HOOKS.contains(&attr.name)
            && !known.contains(&attr.name)
        {
            return fail_on(
                format!("hook:{}", attr.name),
                "a runtime hook no reader here knows",
            );
        }
    }
    Ok(())
}

/// The browser runtime's commands, as the client's.
pub(crate) fn commands(json: &str) -> Read<Vec<Command>> {
    let Ok(list) = serde_json::from_str::<Vec<Vec<serde_json::Value>>>(json) else {
        return fail("commands that are not a list");
    };
    let mut out = Vec::with_capacity(list.len());
    for command in list {
        let text = |at: usize| {
            command
                .get(at)
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_owned()
        };
        let name = text(0);
        let id = text(1);
        out.push(match name.as_str() {
            "toggle" => Command::Toggle { id },
            "show" => Command::Show { id },
            "hide" => Command::Hide { id },
            "open" => Command::Open { id },
            "close" => Command::Close { id },
            "copy" => Command::Copy { text: id },
            "focus" => Command::Focus { id },
            "submit" => Command::Submit { id },
            "text" => Command::Text { id, text: text(2) },
            "value" => Command::Value { id, value: text(2) },
            "count" => Command::Count {
                id,
                max: command
                    .get(2)
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|max| u32::try_from(max).ok()),
            },
            "toast" => Command::Toast {
                tone: tone(&id).unwrap_or_default(),
                text: text(2),
            },
            "set_attr" | "remove_attr" | "toggle_attr" => {
                let attribute = text(2);
                let Some(state) = state(&attribute) else {
                    return fail_on(
                        format!("cmd:{name}:{attribute}"),
                        "a state no native part keeps",
                    );
                };
                match name.as_str() {
                    "set_attr" => Command::Set {
                        id,
                        state,
                        on: text(3) != "false",
                    },
                    "remove_attr" => Command::Set {
                        id,
                        state,
                        on: false,
                    },
                    _ => Command::Flip { id, state },
                }
            }
            other => {
                return fail_on(format!("cmd:{other}"), "a command no native client runs");
            }
        });
    }
    Ok(out)
}

/// A number as a number keypad types it: digits, a `-` before them and a `.` for the
/// decimal point, with no grouping, and the unit the page wrote around it (`₹`, `kg`)
/// apart: `(before, digits, after)`. `None` when the text is not a number written with
/// the web's `.` and grouping marks (`12,500.50`, `₹ 1,24,050`).
pub(crate) fn plain_digits(text: &str) -> Option<(String, String, String)> {
    let text = text.trim();
    let wanted = |c: char| c.is_ascii_digit() || c == '.';
    let first = text.find(wanted)?;
    let last = text.rfind(wanted)? + 1;
    // A sign stands right before the digits.
    let start = if first > 0 && text[..first].ends_with(['-', '+']) {
        first - 1
    } else {
        first
    };
    let (before, middle, after) = (&text[..start], &text[start..last], &text[last..]);
    // A second sign is no number.
    if before.trim_end().ends_with(['-', '+']) {
        return None;
    }
    let mut digits = String::with_capacity(middle.len());
    for c in middle.chars() {
        match c {
            ',' | '\'' | '_' | ' ' | '\u{a0}' | '\u{202f}' | '\u{2009}' => {}
            c if c.is_ascii_digit() || matches!(c, '.' | '-' | '+') => digits.push(c),
            _ => return None,
        }
    }
    let body = digits.trim_start_matches(['-', '+']);
    if digits.len() - body.len() > 1
        || body.is_empty()
        || body == "."
        || body.matches('.').count() > 1
        || body.contains(['-', '+'])
    {
        return None;
    }
    let digits = digits.trim_start_matches('+').to_owned();
    Some((before.trim().to_owned(), digits, after.trim().to_owned()))
}

/// Writes the value, limits and step of a number or decimal control as plain digits;
/// a unit the value was written with goes to the prefix or suffix.
fn plain_number(input: &mut i::TextInput) {
    if let Some((before, digits, after)) = plain_digits(&input.value) {
        input.value = digits;
        if input.prefix.is_none() && !before.is_empty() {
            input.prefix = Some(before);
        }
        if input.suffix.is_none() && !after.is_empty() {
            input.suffix = Some(after);
        }
    }
    for limit in [&mut input.min, &mut input.max, &mut input.step] {
        if let Some(text) = limit.as_deref()
            && let Some((_, digits, _)) = plain_digits(text)
        {
            *limit = Some(digits);
        }
    }
}

/// The protocol's standard action for the page's.
fn standard_of(standard: crate::Standard) -> n::Standard {
    match standard {
        crate::Standard::Share => n::Standard::Share,
        crate::Standard::Delete => n::Standard::Delete,
        crate::Standard::Add => n::Standard::Add,
        crate::Standard::Edit => n::Standard::Edit,
        crate::Standard::Search => n::Standard::Search,
        crate::Standard::Done => n::Standard::Done,
        crate::Standard::Cancel => n::Standard::Cancel,
        crate::Standard::More => n::Standard::More,
    }
}

/// The state an attribute a command sets stands for.
fn state(attribute: &str) -> Option<State> {
    Some(match attribute {
        "open" | "data-open" | "aria-expanded" => State::Open,
        "hidden" | "aria-hidden" => State::Hidden,
        "data-checked" | "aria-checked" => State::Checked,
        "data-selected" | "aria-selected" => State::Selected,
        "disabled" | "data-disabled" | "aria-disabled" => State::Disabled,
        "data-active" | "aria-current" | "aria-pressed" => State::Active,
        "data-invalid" | "aria-invalid" => State::Invalid,
        "data-pending" | "aria-busy" => State::Pending,
        _ => return None,
    })
}

/// The tone a `data-tone` names. A colour with no meaning (`violet`, `cyan`)
/// has none.
pub(crate) fn tone(name: &str) -> Option<n::Tone> {
    Some(match name {
        "neutral" | "muted" | "default" => n::Tone::Neutral,
        "accent" | "primary" | "brand" => n::Tone::Accent,
        "success" => n::Tone::Success,
        "warning" => n::Tone::Warning,
        "danger" | "destructive" | "error" => n::Tone::Danger,
        "info" => n::Tone::Info,
        _ => return None,
    })
}

/// The tone of an element, from its `data-tone`.
pub(crate) fn tone_of(element: &Element) -> Option<n::Tone> {
    attr(element, "data-tone").and_then(|name| tone(&name))
}

/// A number as the protocol writes it: a whole one whole, a fraction as it is.
pub(crate) fn json_number(value: f64) -> serde_json::Number {
    if value.fract() == 0.0 && value.abs() < 9.0e15 {
        serde_json::Number::from(value as i64)
    } else {
        serde_json::Number::from_f64(value).unwrap_or_else(|| serde_json::Number::from(0))
    }
}

/// The value of an attribute, as text.
pub(crate) fn attr<'a>(element: &'a Element, name: &str) -> Option<Cow<'a, str>> {
    element.get(name).and_then(AttrValue::as_text)
}

/// Whether an attribute is set, with a value or without.
pub(crate) fn has(element: &Element, name: &str) -> bool {
    element.get(name).is_some()
}

/// An attribute that holds a whole number.
pub(crate) fn number<T: std::str::FromStr>(element: &Element, name: &str) -> Option<T> {
    attr(element, name).and_then(|value| value.trim().parse().ok())
}

/// The element's id.
pub(crate) fn id(element: &Element) -> Option<String> {
    attr(element, "id").map(Cow::into_owned)
}

/// The element's `data-slot`.
pub(crate) fn slot(element: &Element) -> Option<Cow<'_, str>> {
    attr(element, "data-slot")
}

/// Where a cell or a row sets its content, from `data-align`.
pub(crate) fn align_of(element: &Element) -> Option<n::Align> {
    match attr(element, "data-align").as_deref() {
        Some("start") => Some(n::Align::Start),
        Some("center") => Some(n::Align::Center),
        Some("end") => Some(n::Align::End),
        Some("stretch") => Some(n::Align::Stretch),
        Some("baseline") => Some(n::Align::Baseline),
        _ => None,
    }
}

/// Nodes with fragments opened and nothing left out but the empty ones.
pub(crate) fn flat(nodes: &[Node]) -> Vec<&Node> {
    let mut out = Vec::with_capacity(nodes.len());
    fn push<'a>(node: &'a Node, out: &mut Vec<&'a Node>) {
        match node {
            Node::Empty => {}
            // What an author said to a native client of the element: written to no one.
            Node::Static(text) if text.is_empty() && crate::hint::key_of(node).is_some() => {}
            Node::Fragment(nodes) => {
                for node in nodes {
                    push(node, out);
                }
            }
            other => out.push(other),
        }
    }
    for node in nodes {
        push(node, &mut out);
    }
    out
}

/// The child elements of `element`, which must hold nothing else but blank text.
pub(crate) fn elements(element: &Element) -> Read<Vec<&Element>> {
    let mut out = Vec::new();
    for child in flat(element.child_nodes()) {
        match child {
            Node::Element(child) => out.push(child),
            Node::Text(text) if text.trim().is_empty() => {}
            _ => return fail(format!("text straight inside a <{}>", element.tag())),
        }
    }
    Ok(out)
}

/// Refuses a cell that spans rows or columns: no native table has those.
fn spans(cell: &Element) -> Read<()> {
    if has(cell, "colspan") || has(cell, "rowspan") {
        return fail("a cell that spans rows or columns");
    }
    Ok(())
}

/// The words inside an element as a reader meets them: one space where the
/// markup had any, none at the ends.
pub(crate) fn text_of(element: &Element) -> String {
    let mut out = String::new();
    for child in element.child_nodes() {
        collect_text(child, &mut out);
    }
    tidy(&out)
}

/// The words a control is named by: its `aria-label`, or the words it holds.
pub(crate) fn label_of(element: &Element) -> String {
    let words = text_of(element);
    if words.is_empty() {
        attr(element, "aria-label")
            .or_else(|| attr(element, "title"))
            .map(Cow::into_owned)
            .unwrap_or_default()
    } else {
        words
    }
}

/// The text inside an element exactly as it was written, for code.
fn raw_text(element: &Element) -> String {
    let mut out = String::new();
    for child in element.child_nodes() {
        collect_text(child, &mut out);
    }
    out
}

fn collect_text(node: &Node, out: &mut String) {
    match node {
        Node::Text(text) => out.push_str(text),
        Node::Int(value) => out.push_str(&value.to_string()),
        Node::Element(element) => {
            // What assistive technology is told to pass over is not the element's words.
            if element.tag() == "svg" || has(element, "data-ui-sr") {
                return;
            }
            if element.tag() == "br" {
                out.push(' ');
                return;
            }
            for child in element.child_nodes() {
                collect_text(child, out);
            }
        }
        Node::Fragment(nodes) => {
            for node in nodes {
                collect_text(node, out);
            }
        }
        Node::Raw(_) | Node::Static(_) | Node::Empty => {}
    }
}

/// Text with each run of blanks as one space and none at the ends.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Several parts as one: the part itself, or a stack of them.
pub(crate) fn one(mut nodes: Vec<N>) -> N {
    if nodes.len() == 1 {
        return nodes.remove(0);
    }
    N::Stack(n::Stack {
        children: nodes,
        ..n::Stack::default()
    })
}

/// `node` carrying `id`: on itself when it has none of its own, or on a stack
/// around it.
pub(crate) fn with_id(mut node: N, id: Option<String>) -> N {
    let Some(id) = id else {
        return node;
    };
    let slot = match &mut node {
        N::Stack(inner) => &mut inner.id,
        N::Grid(inner) => &mut inner.id,
        N::Section(inner) => &mut inner.id,
        N::List(inner) => &mut inner.id,
        N::Row(inner) => &mut inner.id,
        N::Text(inner) => &mut inner.id,
        N::Rich(inner) => &mut inner.id,
        N::Image(inner) => &mut inner.id,
        N::Badge(inner) => &mut inner.id,
        N::Facts(inner) => &mut inner.id,
        N::Table(inner) => &mut inner.id,
        N::Button(inner) => &mut inner.id,
        N::Link(inner) => &mut inner.id,
        N::Form(inner) => &mut inner.id,
        N::Banner(inner) => &mut inner.id,
        N::Empty(inner) => &mut inner.id,
        N::Message(inner) => &mut inner.id,
        N::FeedRow(inner) => &mut inner.id,
        N::Figure(inner) => &mut inner.id,
        N::Timeline(inner) => &mut inner.id,
        N::Gauge(inner) => &mut inner.id,
        N::Chart(inner) => &mut inner.id,
        N::Pane(inner) => &mut inner.id,
        N::Tabs(inner) => &mut inner.id,
        N::Disclosure(inner) => &mut inner.id,
        _ => {
            return N::Stack(n::Stack {
                id: Some(id),
                children: vec![node],
                ..n::Stack::default()
            });
        }
    };
    if slot.is_none() {
        *slot = Some(id);
        return node;
    }
    N::Stack(n::Stack {
        id: Some(id),
        children: vec![node],
        ..n::Stack::default()
    })
}

/// The field with this id, in `node` or in the stacks inside it.
fn field_mut<'a>(node: &'a mut N, id: &str) -> Option<&'a mut n::Field> {
    match node {
        N::Field(field) if field.id.as_deref() == Some(id) => Some(field),
        N::Stack(stack) => stack
            .children
            .iter_mut()
            .find_map(|child| field_mut(child, id)),
        _ => None,
    }
}

/// The id of the control a `<label for>` names, when the label holds no
/// control of its own and belongs to no component.
fn label_for(element: &Element) -> Option<String> {
    if element.tag() != "label" || !matches!(slot(element).as_deref(), None | Some("label")) {
        return None;
    }
    let named = attr(element, "for")?;
    element
        .child_nodes()
        .iter()
        .all(is_inline)
        .then(|| named.into_owned())
}

/// The hidden values in a form, wherever they stand in it.
pub(crate) fn hidden_in(nodes: &[Node], out: &mut Vec<n::Hidden>) {
    for node in nodes {
        match node {
            Node::Element(element) => {
                if element.tag() == "input" && attr(element, "type").as_deref() == Some("hidden") {
                    out.push(n::Hidden {
                        name: attr(element, "name")
                            .map(Cow::into_owned)
                            .unwrap_or_default(),
                        value: attr(element, "value")
                            .map(Cow::into_owned)
                            .unwrap_or_default(),
                        id: has(element, "data-ui-value").then(|| id(element)).flatten(),
                    });
                } else if element.tag() != "form" {
                    hidden_in(element.child_nodes(), out);
                }
            }
            Node::Fragment(nodes) => hidden_in(nodes, out),
            _ => {}
        }
    }
}

/// The options of a `<select>`, through its groups.
fn options_in(
    element: &Element,
    options: &mut Vec<i::ChoiceOption>,
    chosen: &mut Vec<String>,
    placeholder: &mut Option<String>,
) -> Read<()> {
    for child in elements(element)? {
        match child.tag() {
            "option" => {
                let label = text_of(child);
                let value = attr(child, "value").map_or_else(|| label.clone(), Cow::into_owned);
                // An empty first option that can't be chosen is the select's prompt.
                if value.is_empty() && options.is_empty() && placeholder.is_none() {
                    *placeholder = Some(label);
                    continue;
                }
                if has(child, "selected") {
                    chosen.push(value.clone());
                }
                options.push(i::ChoiceOption {
                    value,
                    label,
                    detail: None,
                    disabled: has(child, "disabled"),
                });
            }
            "optgroup" => options_in(child, options, chosen, placeholder)?,
            "hr" => {}
            other => return fail(format!("a <{other}> among the options of a select")),
        }
    }
    Ok(())
}

/// What a `<br>` leaves in a run until the line is ended: a newline written in
/// the markup is only a blank, and this is a real one.
const BREAK: &str = "\u{2028}";

/// How a run of text is set.
#[derive(Clone, Copy, Default)]
struct Style {
    bold: bool,
    italic: bool,
    code: bool,
}

/// Elements that are part of a line of text.
fn inline_tag(tag: &str) -> bool {
    matches!(
        tag,
        "span"
            | "strong"
            | "b"
            | "em"
            | "i"
            | "code"
            | "kbd"
            | "samp"
            | "bdi"
            | "bdo"
            | "small"
            | "mark"
            | "abbr"
            | "time"
            | "u"
            | "s"
            | "sub"
            | "sup"
            | "cite"
            | "q"
            | "var"
            | "br"
            | "wbr"
            | "a"
    )
}

/// Whether `node` is part of a line of text: words, or an inline element
/// that no slot, id or hook sets apart and that holds only such things.
pub(crate) fn is_inline(node: &Node) -> bool {
    match node {
        Node::Text(_) | Node::Int(_) | Node::Empty => true,
        Node::Raw(_) | Node::Static(_) => false,
        Node::Fragment(nodes) => nodes.iter().all(is_inline),
        Node::Element(element) => {
            inline_tag(element.tag())
                && element.attrs().iter().all(|attr| {
                    !matches!(
                        attr.name,
                        "data-slot" | "id" | "hidden" | "popover" | "data-split-only"
                    ) && !attr.name.starts_with("data-ui-")
                })
                && (element.tag() != "a" || has(element, "href"))
                // A link the author said something of has a mark among its children, so it is
                // a link of its own and not words in a run.
                && element.child_nodes().iter().all(is_inline)
        }
    }
}

/// Adds the runs of an inline node.
fn inline(node: &Node, style: Style, href: Option<&str>, runs: &mut Vec<n::Run>) {
    let mut push = |text: &str| {
        if text.is_empty() {
            return;
        }
        // Words set the same way as the run before them join it.
        if let Some(last) = runs.last_mut()
            && last.bold == style.bold
            && last.italic == style.italic
            && last.code == style.code
            && last.href.as_deref() == href
        {
            last.text.push_str(text);
            return;
        }
        runs.push(n::Run {
            text: text.to_owned(),
            bold: style.bold,
            italic: style.italic,
            code: style.code,
            href: href.map(str::to_owned),
        });
    };
    match node {
        Node::Text(text) => push(text),
        Node::Int(value) => push(&value.to_string()),
        Node::Fragment(nodes) => {
            for node in nodes {
                inline(node, style, href, runs);
            }
        }
        Node::Element(element) => {
            let mut style = style;
            match element.tag() {
                "strong" | "b" => style.bold = true,
                "em" | "i" | "cite" | "var" => style.italic = true,
                "code" | "kbd" | "samp" => style.code = true,
                "br" => {
                    push(BREAK);
                    return;
                }
                _ => {}
            }
            let own = if element.tag() == "a" {
                attr(element, "href")
            } else {
                None
            };
            let href = own.as_deref().or(href);
            for child in element.child_nodes() {
                inline(child, style, href, runs);
            }
        }
        _ => {}
    }
}

/// Ends a line of text: its runs become one text node, a link, or rich text.
fn flush(runs: &mut Vec<n::Run>, out: &mut Vec<N>) {
    if runs.is_empty() {
        return;
    }
    let mut taken = std::mem::take(runs);
    // One space where the markup had any; none at the line's ends.
    let mut after_space = true;
    for run in &mut taken {
        let mut text = String::with_capacity(run.text.len());
        for ch in run.text.chars() {
            if ch == '\u{2028}' {
                while text.ends_with(' ') {
                    text.pop();
                }
                text.push('\n');
                after_space = true;
            } else if ch.is_whitespace() {
                if !after_space {
                    text.push(' ');
                    after_space = true;
                }
            } else {
                text.push(ch);
                after_space = false;
            }
        }
        run.text = text;
    }
    while let Some(last) = taken.last_mut() {
        let trimmed = last.text.trim_end().len();
        last.text.truncate(trimmed);
        if last.text.is_empty() {
            taken.pop();
        } else {
            break;
        }
    }
    taken.retain(|run| !run.text.is_empty());
    if taken.is_empty() {
        return;
    }
    let plain = |run: &n::Run| !run.bold && !run.italic && !run.code && run.href.is_none();
    if taken.iter().all(plain) {
        out.push(N::text(
            taken.into_iter().map(|run| run.text).collect::<String>(),
        ));
    } else if let [run] = taken.as_slice()
        && let Some(href) = &run.href
        && !run.bold
        && !run.italic
        && !run.code
    {
        out.push(N::Link(n::Link {
            id: None,
            label: run.text.clone(),
            action: Action::Navigate {
                href: href.clone(),
                present: if href.starts_with("http://")
                    || href.starts_with("https://")
                    || href.starts_with("mailto:")
                    || href.starts_with("tel:")
                {
                    Present::External
                } else {
                    Present::Auto
                },
            },
            children: Vec::new(),
        }));
    } else {
        out.push(N::Rich(n::Rich {
            runs: taken,
            ..n::Rich::default()
        }));
    }
}

/// What is left of a component's children as its reader takes the parts it
/// knows. A reader ends with [`Parts::done`], so a part it never heard of
/// stops the reading instead of vanishing.
pub(crate) struct Parts<'a> {
    rest: Vec<&'a Node>,
}

impl<'a> Parts<'a> {
    /// The children of `element`.
    pub(crate) fn of(element: &'a Element) -> Self {
        Self {
            rest: flat(element.child_nodes()),
        }
    }

    /// Takes the first child whose slot is `name`.
    pub(crate) fn slot(&mut self, name: &str) -> Option<&'a Element> {
        let at = self.rest.iter().position(
            |node| matches!(node, Node::Element(element) if slot(element).as_deref() == Some(name)),
        )?;
        match self.rest.remove(at) {
            Node::Element(element) => Some(element),
            _ => None,
        }
    }

    /// Takes every child whose slot is `name`.
    pub(crate) fn slots(&mut self, name: &str) -> Vec<&'a Element> {
        let mut out = Vec::new();
        while let Some(element) = self.slot(name) {
            out.push(element);
        }
        out
    }

    /// What no one took, by reference.
    pub(crate) fn left(&self) -> &[&'a Node] {
        &self.rest
    }

    /// Ends the reading: anything left but blank text is a structure the
    /// reader does not know.
    pub(crate) fn done(self) -> Read<()> {
        for node in self.rest {
            match node {
                Node::Text(text) if text.trim().is_empty() => {}
                Node::Element(element) => {
                    return fail(match slot(element) {
                        Some(slot) => format!("an unexpected `{slot}`"),
                        None => format!("an unexpected <{}>", element.tag()),
                    });
                }
                _ => return fail("unexpected text"),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::Cmd;
    use crate::{ToastTone, el};

    fn read(node: impl Into<Node>) -> N {
        let lowered = lower(&node.into());
        assert!(lowered.is_native(), "{:?}", lowered.misses);
        lowered.node
    }

    fn missed(node: impl Into<Node>) -> Vec<String> {
        lower(&node.into()).why()
    }

    #[test]
    fn a_number_is_written_as_a_number_keypad_types_it() {
        let plain = |text: &str| plain_digits(text);
        let says = |before: &str, digits: &str, after: &str| {
            Some((before.to_owned(), digits.to_owned(), after.to_owned()))
        };
        assert_eq!(plain("7,848.00"), says("", "7848.00", ""));
        assert_eq!(plain("1,24,050"), says("", "124050", ""));
        assert_eq!(plain("₹ 1,24,050.5"), says("₹", "124050.5", ""));
        assert_eq!(plain("12 kg"), says("", "12", "kg"));
        assert_eq!(plain("-3.5"), says("", "-3.5", ""));
        assert_eq!(plain("+3"), says("", "3", ""));
        assert_eq!(plain(".5"), says("", ".5", ""));
        assert_eq!(plain(" 42 "), says("", "42", ""));
        for not in ["", "kg", "1.2.3", "1 2a3", "--1", "1-2", "."] {
            assert_eq!(plain(not), None, "{not}");
        }
    }

    #[test]
    fn a_number_control_carries_plain_digits_and_its_unit_as_a_prefix_or_suffix() {
        let field = |node: Node| match read(node) {
            N::Field(field) => field,
            other => panic!("not a field: {other:?}"),
        };
        let amount = field(
            el("input")
                .attr("type", "text")
                .attr("inputmode", "decimal")
                .attr("name", "amount")
                .attr("value", "₹ 1,24,050.50")
                .attr("min", "1,000")
                .into(),
        );
        let i::Input::Text(typed) = &amount.input else {
            panic!("not text");
        };
        assert_eq!(typed.kind, i::TextKind::Decimal);
        assert_eq!(typed.value, "124050.50");
        assert_eq!(typed.prefix.as_deref(), Some("₹"));
        assert_eq!(typed.min.as_deref(), Some("1000"));
        // A plain text field keeps what it holds, grouping marks and all.
        let note = field(
            el("input")
                .attr("type", "text")
                .attr("name", "note")
                .attr("value", "1,000 units")
                .into(),
        );
        let i::Input::Text(typed) = &note.input else {
            panic!("not text");
        };
        assert_eq!(typed.value, "1,000 units");
        assert_eq!(typed.prefix, None);
    }

    #[test]
    fn words_and_inline_elements_join_into_one_text() {
        assert_eq!(
            read(el("p").children(("Up to ", el("bdi").child("₹7,848"), " can go back."))),
            N::text("Up to ₹7,848 can go back.")
        );
        assert_eq!(
            read(el("p").children(("  a \n  b  ", 7_i64))),
            N::text("a b 7")
        );
        // A paragraph of nothing is nothing.
        assert_eq!(
            read(el("div").children((el("p").child("  "), el("p").child("x")))),
            N::text("x")
        );
    }

    #[test]
    fn text_set_in_more_than_one_way_is_rich_text() {
        let node = read(el("p").children((
            "Packed by ",
            el("strong").child("Ravneet"),
            ", see ",
            el("a").href("/orders/7").child("the order"),
            el("br"),
            el("code").child("LDH-7"),
        )));
        assert_eq!(
            node,
            N::Rich(n::Rich {
                runs: vec![
                    n::Run {
                        text: "Packed by ".into(),
                        ..n::Run::default()
                    },
                    n::Run {
                        text: "Ravneet".into(),
                        bold: true,
                        ..n::Run::default()
                    },
                    n::Run {
                        text: ", see ".into(),
                        ..n::Run::default()
                    },
                    n::Run {
                        text: "the order".into(),
                        href: Some("/orders/7".into()),
                        ..n::Run::default()
                    },
                    n::Run {
                        text: "\n".into(),
                        ..n::Run::default()
                    },
                    n::Run {
                        text: "LDH-7".into(),
                        code: true,
                        ..n::Run::default()
                    },
                ],
                ..n::Rich::default()
            })
        );
    }

    #[test]
    fn a_link_alone_is_a_link_and_one_that_leaves_the_app_says_so() {
        assert_eq!(
            read(el("a").href("/orders").child("Orders")),
            N::Link(n::Link {
                id: None,
                label: "Orders".into(),
                action: Action::navigate("/orders"),
                children: Vec::new(),
            })
        );
        let N::Link(out) = read(el("a").href("https://example.com/docs").child("Docs")) else {
            panic!("not a link");
        };
        assert_eq!(
            out.action,
            Action::Navigate {
                href: "https://example.com/docs".into(),
                present: Present::External
            }
        );
        // A link around more than words keeps what it holds.
        let N::Link(card) = read(
            el("a")
                .href("/orders/7")
                .children((el("h3").child("CS-7"), el("p").child("Paid"))),
        ) else {
            panic!("not a link");
        };
        assert_eq!(card.label, "CS-7Paid");
        assert_eq!(card.children.len(), 2);
    }

    #[test]
    fn a_container_is_its_one_part_or_a_stack_and_keeps_its_id() {
        assert_eq!(read(el("div").child(el("p").child("a"))), N::text("a"));
        assert_eq!(
            read(el("div").children((el("p").child("a"), el("p").child("b")))),
            N::Stack(n::Stack {
                children: vec![N::text("a"), N::text("b")],
                ..n::Stack::default()
            })
        );
        assert_eq!(
            read(el("div").id("price").child("₹31")),
            N::Text(n::Text {
                id: Some("price".into()),
                value: "₹31".into(),
                ..n::Text::default()
            })
        );
        // Something hidden that nothing names is never shown; with an id, a command can show it.
        assert_eq!(
            read(el("div").children((el("p").flag("hidden", true).child("x"), "y"))),
            N::text("y")
        );
        assert_eq!(
            read(el("div").id("more").flag("hidden", true).child("x")),
            N::Stack(n::Stack {
                id: Some("more".into()),
                hidden: true,
                children: vec![N::text("x")],
                ..n::Stack::default()
            })
        );
    }

    #[test]
    fn a_meter_is_a_gauge_and_a_progress_bar_is_progress() {
        assert_eq!(
            read(
                el("meter")
                    .attr("min", "0")
                    .attr("max", "60000")
                    .attr("value", "42000")
                    .attr("aria-label", "Budget used")
                    .child("₹42,000 of ₹60,000")
            ),
            N::Gauge(n::Gauge {
                id: None,
                value: 42_000.into(),
                min: 0.into(),
                max: 60_000.into(),
                label: Some("Budget used".into()),
                shown: Some("₹42,000 of ₹60,000".into()),
                tone: None,
                ..n::Gauge::default()
            })
        );
        // A fraction stays a fraction; a value past the end is held at it.
        let N::Gauge(level) = read(el("meter").attr("value", "1.4").attr("max", "1")) else {
            panic!("not a gauge");
        };
        assert_eq!(level.value, 1.into());
        let N::Gauge(half) = read(el("meter").attr("value", "0.5")) else {
            panic!("not a gauge");
        };
        assert_eq!(half.value.as_f64(), Some(0.5));
        assert_eq!(
            missed(el("meter").attr("min", "5").attr("max", "5")),
            ["tag:meter"]
        );
    }

    #[test]
    fn headings_lists_pictures_and_rules_read_as_what_they_are() {
        let node = read(
            el("section").children((
                el("h2").child("Items"),
                el("ol").children((el("li").child("One"), el("li").id("two").child("Two"))),
                el("hr"),
                el("img")
                    .attr("src", "/shop.jpg")
                    .attr("alt", "The shop")
                    .attr("width", 320_i64),
                el("pre").child("a\n  b"),
                el("progress").attr("value", "31").attr("max", "50"),
            )),
        );
        let N::Stack(stack) = node else {
            panic!("not a stack");
        };
        assert_eq!(
            stack.children,
            vec![
                N::Text(n::Text {
                    value: "Items".into(),
                    style: n::TextStyle::Title,
                    heading: Some(2),
                    ..n::Text::default()
                }),
                N::List(n::List {
                    ordered: true,
                    children: vec![
                        N::text("One"),
                        N::Text(n::Text {
                            id: Some("two".into()),
                            value: "Two".into(),
                            ..n::Text::default()
                        })
                    ],
                    ..n::List::default()
                }),
                N::Divider(n::Divider::default()),
                N::Image(n::Image {
                    src: "/shop.jpg".into(),
                    alt: "The shop".into(),
                    width: Some(320),
                    ..n::Image::default()
                }),
                N::Code(n::Code {
                    text: "a\n  b".into(),
                    ..n::Code::default()
                }),
                N::Progress(n::Progress {
                    value: Some(620),
                    ..n::Progress::default()
                }),
            ]
        );
    }

    #[test]
    fn a_description_list_is_facts() {
        let node = read(el("dl").children((
            el("div").children((el("dt").child("Customer"), el("dd").child("Tanvi Sharma"))),
            el("dt").child("Pays by"),
            el("dd").child("UPI"),
            el("dd").child("₹7,848 paid"),
        )));
        assert_eq!(
            node,
            N::Facts(n::Facts {
                id: None,
                items: vec![
                    n::Fact {
                        label: "Customer".into(),
                        value: vec![N::text("Tanvi Sharma")],
                        ..n::Fact::default()
                    },
                    n::Fact {
                        label: "Pays by".into(),
                        value: vec![N::text("UPI"), N::text("₹7,848 paid")],
                        ..n::Fact::default()
                    },
                ],
            })
        );
    }

    #[test]
    fn a_table_reads_its_caption_columns_and_rows() {
        let node = read(
            el("table").children((
                el("caption").child("Items"),
                el("thead").child(
                    el("tr").children((
                        el("th").child("Product"),
                        el("th").flag("data-numeric", true).child("Qty"),
                        el("th")
                            .flag("data-numeric", true)
                            .flag("data-hide-mobile", true)
                            .child("Price"),
                    )),
                ),
                el("tbody").child(el("tr").attr("data-key", 4_i64).children((
                    el("td").child("Phulkari dupatta"),
                    el("td").child(2_i64),
                    el("td"),
                ))),
            )),
        );
        assert_eq!(
            node,
            N::Table(n::Table {
                caption: Some("Items".into()),
                columns: vec![
                    n::Column {
                        label: "Product".into(),
                        ..n::Column::default()
                    },
                    n::Column {
                        label: "Qty".into(),
                        numeric: true,
                        ..n::Column::default()
                    },
                    n::Column {
                        label: "Price".into(),
                        numeric: true,
                        secondary: true,
                        ..n::Column::default()
                    },
                ],
                rows: vec![n::TableRow {
                    id: Some("4".into()),
                    cells: vec![N::text("Phulkari dupatta"), N::text("2"), N::text("")],
                    ..n::TableRow::default()
                }],
                ..n::Table::default()
            })
        );
        assert_eq!(
            missed(el("table").child(
                el("tbody").child(el("tr").child(el("td").attr("colspan", 2_i64).child("x")))
            )),
            ["tag:table"]
        );
    }

    #[test]
    fn a_form_reads_where_it_goes_what_it_hides_and_its_controls() {
        let node = read(
            el("form")
                .id("restock")
                .attr("method", "post")
                .attr("action", "/inventory/18/restock")
                .flag("data-ui-form", true)
                .children((
                    el("input")
                        .attr("type", "hidden")
                        .attr("name", "_csrf")
                        .attr("value", "tok"),
                    el("label").attr("for", "q").child("Units"),
                    el("input")
                        .id("q")
                        .attr("type", "number")
                        .attr("name", "quantity")
                        .attr("value", "42")
                        .attr("min", 1_i64)
                        .attr("max", 500_i64)
                        .flag("required", true),
                    el("select").attr("name", "reason").children((
                        el("option").attr("value", "").child("Choose a reason"),
                        el("option")
                            .attr("value", "late")
                            .flag("selected", true)
                            .child("Arrived late"),
                        el("option")
                            .attr("value", "damaged")
                            .child("Arrived damaged"),
                    )),
                    el("label").children((
                        el("input")
                            .attr("type", "checkbox")
                            .attr("name", "notify")
                            .flag("checked", true),
                        "Tell the customer",
                    )),
                    el("textarea")
                        .attr("name", "note")
                        .child("Left at the gate"),
                    el("button").attr("type", "submit").child("Restock"),
                )),
        );
        let N::Form(form) = node else {
            panic!("not a form");
        };
        assert_eq!(form.id.as_deref(), Some("restock"));
        assert_eq!(form.method, n::Method::Post);
        assert_eq!(form.action, "/inventory/18/restock");
        assert_eq!(
            form.hidden,
            [n::Hidden {
                name: "_csrf".into(),
                value: "tok".into(),
                id: None
            }]
        );
        let N::Field(units) = &form.children[0] else {
            panic!("the label did not join its control: {:?}", form.children[0]);
        };
        assert_eq!(units.label.as_deref(), Some("Units"));
        assert_eq!(units.name, "quantity");
        assert!(units.required);
        assert_eq!(
            units.input,
            i::Input::Text(i::TextInput {
                kind: i::TextKind::Number,
                value: "42".into(),
                min: Some("1".into()),
                max: Some("500".into()),
                ..i::TextInput::default()
            })
        );
        let N::Field(reason) = &form.children[1] else {
            panic!("not a field");
        };
        assert_eq!(
            reason.input,
            i::Input::Choice(i::ChoiceInput {
                options: vec![
                    i::ChoiceOption {
                        value: "late".into(),
                        label: "Arrived late".into(),
                        ..i::ChoiceOption::default()
                    },
                    i::ChoiceOption {
                        value: "damaged".into(),
                        label: "Arrived damaged".into(),
                        ..i::ChoiceOption::default()
                    },
                ],
                value: Some("late".into()),
                placeholder: Some("Choose a reason".into()),
                ..i::ChoiceInput::default()
            })
        );
        let N::Field(notify) = &form.children[2] else {
            panic!("not a field: {:?}", form.children[2]);
        };
        assert_eq!(notify.label.as_deref(), Some("Tell the customer"));
        assert_eq!(
            notify.input,
            i::Input::Toggle(i::ToggleInput {
                kind: i::ToggleKind::Checkbox,
                on: true,
                value: None
            })
        );
        let N::Field(note) = &form.children[3] else {
            panic!("not a field");
        };
        assert_eq!(
            note.input,
            i::Input::Text(i::TextInput {
                kind: i::TextKind::Multiline,
                value: "Left at the gate".into(),
                ..i::TextInput::default()
            })
        );
        assert_eq!(
            form.children[4],
            N::Button(n::Button {
                label: "Restock".into(),
                action: Some(Action::submit()),
                ..n::Button::default()
            })
        );
    }

    #[test]
    fn a_button_does_what_its_attributes_say() {
        let open = read(
            el("button")
                .attr("type", "button")
                .attr("command", "show-modal")
                .attr("commandfor", "refund")
                .child("Refund"),
        );
        assert_eq!(
            open,
            N::Button(n::Button {
                label: "Refund".into(),
                action: Some(Action::command(Command::Open {
                    id: "refund".into()
                })),
                ..n::Button::default()
            })
        );
        let N::Button(clicked) = read(el("button").attr("type", "button").child("Copy").on_click(
            vec![
                Cmd::copy("CS-7"),
                Cmd::toast(ToastTone::Success, "Copied"),
                Cmd::set_attr("row", "data-selected", "true"),
                Cmd::toggle_attr("menu", "data-open"),
                Cmd::count("n", 160),
            ],
        )) else {
            panic!("not a button");
        };
        assert_eq!(
            clicked.action,
            Some(Action::Commands {
                list: vec![
                    Command::Copy {
                        text: "CS-7".into()
                    },
                    Command::Toast {
                        tone: n::Tone::Success,
                        text: "Copied".into()
                    },
                    Command::Set {
                        id: "row".into(),
                        state: State::Selected,
                        on: true
                    },
                    Command::Flip {
                        id: "menu".into(),
                        state: State::Open
                    },
                    Command::Count {
                        id: "n".into(),
                        max: Some(160)
                    },
                ]
            })
        );
        let N::Button(named) = read(
            el("button")
                .attr("type", "submit")
                .attr("name", "then")
                .attr("value", "again")
                .attr("formaction", "/save?again=1")
                .child("Save and add another"),
        ) else {
            panic!("not a button");
        };
        assert_eq!(
            named.action,
            Some(Action::Submit {
                form: None,
                href: Some("/save?again=1".into()),
                with: Some(Pair {
                    name: "then".into(),
                    value: "again".into()
                }),
            })
        );
    }

    #[test]
    fn a_details_and_a_dialog_read_as_what_they_are() {
        assert_eq!(
            read(
                el("details")
                    .flag("open", true)
                    .children((el("summary").child("More"), el("p").child("Inside")))
            ),
            N::Disclosure(n::Disclosure {
                id: None,
                label: "More".into(),
                open: true,
                children: vec![N::text("Inside")],
            })
        );
        assert_eq!(
            read(
                el("dialog")
                    .id("about")
                    .attr("aria-label", "About")
                    .child("Hello")
            ),
            N::Dialog(n::Dialog {
                id: "about".into(),
                title: Some("About".into()),
                children: vec![N::text("Hello")],
                ..n::Dialog::default()
            })
        );
    }

    #[test]
    fn what_has_no_native_meaning_is_named_and_never_guessed() {
        assert_eq!(missed(el("canvas")), ["tag:canvas"]);
        assert_eq!(
            missed(el("div").slot("no-such-part")),
            ["slot:no-such-part"]
        );
        assert_eq!(
            missed(el("div").flag("data-ui-sortable", true).child("x")),
            ["hook:data-ui-sortable"]
        );
        assert_eq!(
            missed(
                el("button")
                    .attr("type", "button")
                    .child("x")
                    .on_click(Cmd::fill("d"))
            ),
            ["cmd:fill"]
        );
        assert_eq!(
            missed(
                el("button")
                    .attr("type", "button")
                    .child("x")
                    .on_click(Cmd::set_attr("a", "data-colour", "red"))
            ),
            ["cmd:set_attr:data-colour"]
        );
        assert_eq!(
            missed(Node::Raw(crate::SafeHtml::trusted("<b>hi</b>"))),
            ["raw"]
        );
        assert_eq!(missed(el("input").attr("type", "radio")), ["tag:input"]);
        assert_eq!(
            missed(el("input").attr("name", "q").attr("form", "elsewhere")),
            ["tag:input"]
        );
        assert_eq!(missed(el("svg").attr("role", "img")), ["tag:svg"]);
        // The rest of the tree is still read, with a `web` node where the miss was.
        let lowered = lower(
            &el("div")
                .children((el("p").child("a"), el("canvas")))
                .into(),
        );
        assert_eq!(
            lowered.node,
            N::Stack(n::Stack {
                children: vec![N::text("a"), N::Web(n::WebRegion::default())],
                ..n::Stack::default()
            })
        );
        let kept = lower_keeping_markup(&el("canvas").id("c").into());
        assert_eq!(
            kept.node,
            N::Web(n::WebRegion {
                id: Some("c".into()),
                html: r#"<canvas id="c"></canvas>"#.into()
            })
        );
    }

    #[test]
    fn a_drawing_hidden_from_assistive_technology_is_dropped_as_decoration() {
        let lowered = lower(
            &el("div")
                .children((
                    el("svg").attr("aria-hidden", "true"),
                    el("p").child("Kadhai"),
                ))
                .into(),
        );
        assert!(lowered.is_native());
        assert_eq!(lowered.node, N::text("Kadhai"));
        assert_eq!(lowered.dropped, ["svg"]);
    }

    #[test]
    fn a_region_says_where_it_fetches_itself() {
        let node = read(
            el("div")
                .id("kpis")
                .flag("data-ui-region", true)
                .attr("data-ui-src", "/fragments/figures")
                .attr("data-ui-live-src", "/broadcasting/stream?topics=stats")
                .attr("data-ui-live-event", "stats")
                .child(el("p").child("12")),
        );
        assert_eq!(
            node,
            N::Region(n::Region {
                id: "kpis".into(),
                src: "/fragments/figures".into(),
                lazy: false,
                live: Some(n::Live {
                    src: "/broadcasting/stream?topics=stats".into(),
                    event: Some("stats".into())
                }),
                children: vec![N::text("12")],
            })
        );
    }

    /// `inner` wrapped `levels` times by `wrap`.
    fn nested(levels: usize, inner: Node, wrap: impl Fn(Node, usize) -> Node) -> Node {
        let mut node = inner;
        for level in 0..levels {
            node = wrap(node, level);
        }
        node
    }

    /// What reading `node` leaves: the stack the reading used below where it began, and
    /// what it read.
    fn measured(node: &Node) -> (usize, Lowered) {
        let mut cx = Cx::new(false);
        let nodes = cx.sequence(std::slice::from_ref(node));
        let most = cx.most;
        (most, cx.finish(nodes))
    }

    /// The stack a thread has by default: a test's, and a runtime's worker.
    const DEFAULT_STACK: usize = 2 << 20;

    /// Runs `work` on a thread with `stack` bytes, and says what it came back with.
    fn on_stack<T: Send + 'static>(stack: usize, work: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::Builder::new()
            .stack_size(stack)
            .spawn(work)
            .expect("a thread")
            .join()
            .expect("no overflow")
    }

    #[test]
    fn a_tree_deeper_than_a_page_is_not_read_at_all() {
        let node = nested(5000, el("p").child("x").into(), |inner, _| {
            el("div").child(inner).into()
        });
        let (most, lowered) = measured(&node);
        assert_eq!(lowered.why(), ["depth"]);
        // One miss, and no reader ran: nothing was read to be thrown away.
        assert_eq!(lowered.misses.len(), 1);
        assert_eq!(most, 0);
        assert!(matches!(lowered.node, N::Web(_)));
        // The same for a tree of fragments, which `flat` would open one inside another,
        // and one of inline elements, which each walk of words goes down.
        let fragments = nested(5000, Node::Empty, |inner, _| Node::Fragment(vec![inner]));
        assert_eq!(lower(&fragments).why(), ["depth"]);
        let words = nested(5000, "x".into(), |inner, _| el("b").child(inner).into());
        assert_eq!(lower(&words).why(), ["depth"]);
        // A page that is as deep as one may be is read, and one level more is not.
        let wrap = |inner: Node, _| -> Node {
            el("section").child((el("h2").child("level"), inner)).into()
        };
        let page = nested(DEEPEST - 2, el("p").child("x").into(), wrap);
        assert!(lower(&page).is_native());
        let too_deep = nested(DEEPEST + 1, el("p").child("x").into(), wrap);
        assert_eq!(lower(&too_deep).why(), ["depth"]);
    }

    #[test]
    fn rows_built_apart_from_the_page_are_measured_without_the_stack() {
        let deep = nested(5000, "x".into(), |inner, _| el("span").child(inner).into());
        assert!(web_depth([&deep], DEEPEST) > DEEPEST);
        // A hundred thousand levels cost the limit, not the hundred thousand.
        let more = nested(100_000, "x".into(), |inner, _| {
            el("span").child(inner).into()
        });
        assert_eq!(web_depth([&more], DEEPEST), DEEPEST + 1);
        let shallow: Node = el("tr").child(el("td").child("x")).into();
        assert_eq!(web_depth([&shallow], DEEPEST), 2);
    }

    /// The shapes that spend the most stack a level: a reader of its own for each level,
    /// with another part beside the one that goes deeper.
    fn heavy_shapes(levels: usize) -> Vec<(&'static str, Node)> {
        let bottom = || -> Node { crate::Text::new("the bottom").into() };
        vec![
            (
                "sections with a heading",
                nested(levels, bottom(), |inner, level| {
                    el("section")
                        .children((crate::Text::new(format!("level {level}")), inner))
                        .into()
                }),
            ),
            (
                "cards",
                nested(levels, bottom(), |inner, _| {
                    crate::Card::titled("A card").body(inner).into()
                }),
            ),
            (
                "stacks and rows",
                nested(levels, bottom(), |inner, level| {
                    if level % 2 == 0 {
                        crate::Stack::gap(2)
                            .children((crate::Text::new("a"), inner))
                            .into()
                    } else {
                        crate::Row::gap(2)
                            .children((crate::Text::new("b"), inner))
                            .into()
                    }
                }),
            ),
            (
                "lists of lists",
                nested(levels, bottom(), |inner, _| {
                    el("ul").child(el("li").children(("item", inner))).into()
                }),
            ),
            (
                "disclosures",
                nested(levels, bottom(), |inner, _| {
                    el("details")
                        .children((el("summary").child("more"), inner))
                        .into()
                }),
            ),
            (
                "regions",
                nested(levels, bottom(), |inner, level| {
                    crate::Region::new(format!("r{level}"))
                        .src("/x")
                        .children(inner)
                        .into()
                }),
            ),
            (
                "a control in a form",
                el("form")
                    .attr("action", "/find")
                    .child(nested(
                        levels,
                        el("input").attr("name", "q").into(),
                        |inner, _| el("div").child(inner).into(),
                    ))
                    .into(),
            ),
        ]
    }

    #[test]
    fn a_page_as_deep_as_one_may_be_reads_within_the_budget_on_a_default_stack() {
        // Each shape, `DEEPEST - 3` levels deep, read on a thread with the stack a test or a
        // runtime's worker has. What is measured is the stack the reading used below where it
        // began, in this build; it must stay under three quarters of the budget, which is
        // itself half the default stack, so the handler's frames above the reading and the
        // envelope's after it have the rest.
        let report = on_stack(DEFAULT_STACK, || {
            // A component is several elements a level: each shape is as many levels as make it
            // as deep as a page may be.
            let probe = heavy_shapes(10);
            (0..probe.len())
                .map(|at| {
                    let per = web_depth([&probe[at].1], usize::MAX).div_ceil(10);
                    let (name, node) = heavy_shapes((DEEPEST - 6) / per).swap_remove(at);
                    assert!(web_depth([&node], DEEPEST) <= DEEPEST, "{name}");
                    let (most, lowered) = measured(&node);
                    assert!(lowered.is_native(), "{name}: {:?}", lowered.misses);
                    (name, most)
                })
                .collect::<Vec<_>>()
        });
        for (name, most) in &report {
            assert!(
                *most < STACK_BUDGET / 4 * 3,
                "{name}: {most} bytes, of a budget of {STACK_BUDGET}: {report:?}"
            );
        }
        println!("stack used below the start, in a page as deep as one may be: {report:?}");
    }

    #[test]
    fn what_the_budget_cannot_hold_is_a_part_in_the_web_view_not_an_overflow() {
        // A reading that began a whole budget above where this one stands has none left:
        // it ends in a miss at the first element, however the readers' frames come out.
        let node = nested(100, crate::Text::new("x").into(), |inner, _| {
            crate::Card::titled("A card").body(inner).into()
        });
        let mut cx = Cx::new(false);
        cx.started = stack_at() + STACK_BUDGET;
        let read = cx.sequence(std::slice::from_ref(&node));
        let lowered = cx.finish(read);
        assert!(!lowered.is_native());
        assert_eq!(lowered.why(), ["depth"]);
    }

    #[test]
    fn the_walks_that_go_down_a_tree_on_the_stack_fit_a_small_one_at_the_deepest() {
        // Words in elements in elements, fragments in fragments, a control in a form: the
        // helpers that recurse (`is_inline`, `inline`, `flat`, `text_of`, `hidden_in`) at
        // the deepest a page may be, on a stack a quarter of the default.
        on_stack(DEFAULT_STACK / 4, || {
            let words = nested(DEEPEST - 1, "x".into(), |inner, _| {
                el("b").child(inner).into()
            });
            let fragments = nested(DEEPEST - 1, "x".into(), |inner, _| {
                Node::Fragment(vec![inner])
            });
            let emphasised = nested(DEEPEST - 1, "x".into(), |inner, level| {
                el(if level % 2 == 0 { "em" } else { "strong" })
                    .child(inner)
                    .into()
            });
            let link = nested(DEEPEST - 2, "x".into(), |inner, _| {
                el("span").child(inner).into()
            });
            let link: Node = el("a").attr("href", "/x").child(link).into();
            for node in [&words, &fragments, &emphasised, &link] {
                let lowered = lower(node);
                assert!(lowered.is_native(), "{:?}", lowered.misses);
            }
        });
    }

    /// A chain of `levels` stacks of the protocol's, each holding the next.
    fn chain(levels: usize) -> N {
        let mut node = N::text("the bottom");
        for _ in 0..levels {
            node = N::Stack(n::Stack {
                children: vec![node],
                ..n::Stack::default()
            });
        }
        node
    }

    #[test]
    fn what_a_reading_leaves_is_dropped_on_a_small_stack_at_any_depth_a_page_has() {
        // The tree of the protocol's parts a reading leaves is as deep as the page was; its
        // derived `Drop` goes down it on the stack. Measured at about 70 KiB for the deepest
        // a page may be, in a debug build: a quarter of the default stack has room.
        let lowered = on_stack(DEFAULT_STACK, || {
            lower(&nested(
                DEEPEST - 2,
                crate::Text::new("the bottom").into(),
                |inner, level| {
                    el("section")
                        .children((crate::Text::new(format!("level {level}")), inner))
                        .into()
                },
            ))
        });
        assert!(lowered.is_native(), "{:?}", lowered.misses);
        let tree = lowered.node;
        assert!(tree.depth() > navree_native::MAX_DEPTH);
        on_stack(DEFAULT_STACK / 4, move || drop(tree));
    }

    #[test]
    fn a_tree_a_client_can_read_is_cloned_compared_and_written_on_a_default_stack() {
        // What is answered is at most `MAX_DEPTH` deep (anything deeper is the web view's
        // before it is written or copied). Cloned and compared with the derived code, which
        // holds a copy of a part in every frame, that is about a megabyte in a debug build:
        // half the default stack. Written as JSON it is about 180 KiB, and its depth,
        // `downgrade` and `share_icons` use none that grows with it.
        on_stack(DEFAULT_STACK, || {
            let tree = chain(navree_native::MAX_DEPTH - 1);
            let copy = tree.clone();
            assert!(copy == tree);
            assert!(serde_json::to_string(&tree).expect("json").len() > 100);
            assert_eq!(tree.depth(), navree_native::MAX_DEPTH);
        });
    }
}
