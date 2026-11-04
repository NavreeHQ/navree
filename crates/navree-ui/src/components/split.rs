//! SplitPane: panes side by side, or one above another, with separators a person
//! drags or moves with the keys of the WAI-ARIA window splitter pattern. Panes can
//! collapse, the panes show one at a time below a width of the split, and the sizes
//! can be saved to the server, so the next page is drawn with them.
//!
//! The server draws every size, so the first paint is right and a page with
//! JavaScript off keeps its layout. The `split` module adds dragging, the keys and
//! the save. The content security policy forbids `style=""`, so a size is drawn as
//! a whole percent in `data-split-basis`, which the stylesheet reads; the module
//! moves a pane through a custom property, which the policy allows.

use std::fmt::Write as _;

use http::request::Parts;
use http::{Method, StatusCode};
use navree_http::body::{BodyLimit, read_body};
use navree_http::extract::{FromRequest, Request, urlencoded};

use super::{Button, Orientation};
use crate::html::Href;
use crate::node::{Element, IntoNodes, Node, Uses, el, into_node};

/// A width of the split itself, not of the window: the split follows its own room, so
/// a split beside a sidebar stacks when the room left is narrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SplitWidth {
    /// 640px.
    Sm,
    /// 768px.
    Md,
    /// 1024px.
    Lg,
    /// 1280px.
    Xl,
}

impl SplitWidth {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Sm => "sm",
            Self::Md => "md",
            Self::Lg => "lg",
            Self::Xl => "xl",
        }
    }

    /// The width in CSS pixels.
    pub const fn pixels(self) -> u32 {
        match self {
            Self::Sm => 640,
            Self::Md => 768,
            Self::Lg => 1024,
            Self::Xl => 1280,
        }
    }
}

/// How tall a split is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SplitHeight {
    /// As tall as its content: the default, for panes side by side.
    #[default]
    Auto,
    /// 20rem.
    Sm,
    /// 32rem.
    Md,
    /// 44rem.
    Lg,
    /// The rest of the screen: in the app shell, what the top bar and the page's heading
    /// leave; elsewhere, the whole screen. Each pane scrolls on its own, like an app.
    Screen,
}

/// One pane of a [`SplitPane`].
///
/// Sizes are percentages of the split. A pane made with [`fill`](Self::fill) takes what
/// the others leave; the others have a size, a least and a most.
#[derive(Clone, Debug)]
#[must_use]
pub struct Pane {
    id: String,
    label: String,
    fill: bool,
    /// Tenths of a percent.
    size: u16,
    min: u16,
    max: u16,
    collapsible: bool,
    collapsed: bool,
    collapse_below: Option<SplitWidth>,
    header: Vec<Node>,
    body: Vec<Node>,
    footer: Vec<Node>,
    from_end: bool,
    body_label: Option<String>,
}

/// Tenths of a percent in a whole: 100%.
const WHOLE: u16 = 1000;

const fn tenths(percent: u8) -> u16 {
    let percent = if percent > 100 { 100 } else { percent };
    percent as u16 * 10
}

impl Pane {
    fn with(id: impl Into<String>, label: impl Into<String>, fill: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            fill,
            size: 300,
            min: 100,
            max: 900,
            collapsible: false,
            collapsed: false,
            collapse_below: None,
            header: Vec::new(),
            body: Vec::new(),
            footer: Vec::new(),
            from_end: false,
            body_label: None,
        }
    }

    /// A pane with a size, named `id` in the page and `label` for assistive technology.
    /// It is 30% wide until [`size`](Self::size) says otherwise, and at least 10%.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::with(id, label, false)
    }

    /// The pane that takes what the others leave. A split has one: the first pane made
    /// with this, or the last pane when none is.
    pub fn fill(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::with(id, label, true)
    }

    /// Its size, in percent of the split.
    pub fn size(mut self, percent: u8) -> Self {
        self.size = tenths(percent);
        self
    }

    /// The least it may be, in percent. For the fill pane, the least it is left.
    pub fn min(mut self, percent: u8) -> Self {
        self.min = tenths(percent);
        self
    }

    /// The most it may be, in percent.
    pub fn max(mut self, percent: u8) -> Self {
        self.max = tenths(percent);
        self
    }

    /// Lets it collapse to nothing: Enter on its separator, dragging it under its least,
    /// or a [toggle](SplitPane::toggle). It comes back at the size it had.
    pub fn collapsible(mut self) -> Self {
        self.collapsible = true;
        self
    }

    /// Draws it collapsed, if it is collapsible.
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    /// Collapses it while the split is narrower than `width`. A toggle, or Enter on its
    /// separator, still opens it. A pane that does this is [collapsible](Self::collapsible).
    pub fn collapse_below(mut self, width: SplitWidth) -> Self {
        self.collapse_below = Some(width);
        self.collapsible = true;
        self
    }

    /// Adds to the top of the pane, which stays put while the body scrolls.
    pub fn header(mut self, header: impl IntoNodes) -> Self {
        header.push_into(&mut self.header);
        self
    }

    /// Adds to the pane's body, which scrolls on its own.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        body.push_into(&mut self.body);
        self
    }

    /// Adds to the bottom of the pane, which stays put while the body scrolls.
    pub fn footer(mut self, footer: impl IntoNodes) -> Self {
        footer.push_into(&mut self.footer);
        self
    }

    /// Starts the body scrolled to its end, and keeps it there as content is added at
    /// the end: a conversation, a log. Someone who scrolled up stays where they are.
    pub fn from_end(mut self) -> Self {
        self.from_end = true;
        self
    }

    /// Names the body and makes it a tab stop, so it can be scrolled with the keys when
    /// nothing in it takes focus.
    pub fn body_label(mut self, label: impl Into<String>) -> Self {
        self.body_label = Some(label.into());
        self
    }

    /// Its id in the page.
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Panes with separators between them.
///
/// ```
/// use navree_ui::{Pane, SplitPane, SplitWidth, render};
///
/// let split = SplitPane::new("inbox")
///     .stack_below(SplitWidth::Md)
///     .save_to("/inbox/panes")
///     .pane(Pane::new("list", "Conversations").size(28).min(20).max(40))
///     .pane(Pane::fill("thread", "Conversation").min(30))
///     .pane(Pane::new("customer", "Customer").size(24).collapsible())
///     .current("thread");
/// let html = render(&split.into());
/// assert!(html.contains(r#"role="separator""#));
/// assert!(html.contains(r#"aria-controls="list""#));
/// assert!(html.contains(r#"aria-controls="customer""#));
/// assert!(html.contains(r#"data-split-basis="28""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct SplitPane {
    id: String,
    orientation: Orientation,
    panes: Vec<Pane>,
    stack_below: Option<SplitWidth>,
    current: Option<String>,
    save: Option<Href>,
    height: SplitHeight,
    grip: bool,
}

impl SplitPane {
    /// A split named `id` in the page. The id is what its saves say they are for.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            orientation: Orientation::Horizontal,
            panes: Vec::new(),
            stack_below: None,
            current: None,
            save: None,
            height: SplitHeight::Auto,
            grip: false,
        }
    }

    /// Puts the panes one above another, with the separators between them moved up and
    /// down. Give it a [`height`](Self::height).
    pub fn vertical(mut self) -> Self {
        self.orientation = Orientation::Vertical;
        self
    }

    /// Adds a pane.
    pub fn pane(mut self, pane: Pane) -> Self {
        self.panes.push(pane);
        self
    }

    /// Fills in the pane named `id`: a layout written once, for the page and for reading
    /// a save, and the content added where the page is drawn.
    pub fn pane_with(mut self, id: &str, fill: impl FnOnce(Pane) -> Pane) -> Self {
        if let Some(at) = self.panes.iter().position(|pane| pane.id == id) {
            let pane = self.panes.remove(at);
            self.panes.insert(at, fill(pane));
        }
        self
    }

    /// Shows one pane at a time while the split is narrower than `width`: the
    /// [current](Self::current) one. The separators go, and the page links the panes.
    pub fn stack_below(mut self, width: SplitWidth) -> Self {
        self.stack_below = Some(width);
        self
    }

    /// The pane the page is about: the one shown when the panes stack, and shown even if
    /// it is collapsed. The fill pane when not said.
    pub fn current(mut self, id: impl Into<String>) -> Self {
        self.current = Some(id.into());
        self
    }

    /// Posts the sizes to `url` when a person changes them: `split`, `sizes` (a percent
    /// for each pane, one decimal) and `collapsed` (the indexes of the collapsed panes).
    /// Read it with [`SplitSave`]. The URL must be on this site.
    pub fn save_to(mut self, url: impl Into<Href>) -> Self {
        self.save = Some(url.into());
        self
    }

    /// Sets how tall it is.
    pub fn height(mut self, height: SplitHeight) -> Self {
        self.height = height;
        self
    }

    /// Draws a grip on each separator, a hint that it moves.
    pub fn grip(mut self) -> Self {
        self.grip = true;
        self
    }

    /// Draws it with sizes a person saved. Sizes that no longer fit the layout are left
    /// out, so a saved size never breaks a page.
    pub fn sizes(mut self, saved: &SplitSizes) -> Self {
        if saved.sizes.len() != self.panes.len() {
            return self;
        }
        for (pane, (size, collapsed)) in self
            .panes
            .iter_mut()
            .zip(saved.sizes.iter().zip(&saved.collapsed))
        {
            pane.size = *size;
            pane.collapsed = *collapsed;
        }
        self
    }

    /// Its id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// A [`Button`] that opens or collapses the pane named `pane`, with no request. Give
    /// it an `href` for JavaScript off and for stacked panes, where it is a link: a page
    /// that shows that pane, made [current](Self::current).
    pub fn toggle(button: Button, pane: &str) -> Button {
        button
            .attr("data-ui-split-toggle", pane.to_owned())
            .attr("aria-controls", pane.to_owned())
            .with_uses(Uses::SPLIT)
    }

    /// Wraps `content` so it shows only while the split around it shows its panes one
    /// at a time: a link back to the list from a detail pane.
    pub fn stacked_only(content: impl IntoNodes) -> Node {
        el("span")
            .attr("data-split-only", "stacked")
            .child(content)
            .into()
    }

    /// Wraps `content` so it shows only while the split around it shows its panes side
    /// by side.
    pub fn side_by_side_only(content: impl IntoNodes) -> Node {
        el("span")
            .attr("data-split-only", "wide")
            .child(content)
            .into()
    }

    /// Which pane fills: the first marked so, or the last.
    fn fill_index(&self) -> usize {
        self.panes
            .iter()
            .position(|pane| pane.fill)
            .unwrap_or_else(|| self.panes.len().saturating_sub(1))
    }

    /// The layout with every rule held: each size between its least and its most, a
    /// collapsed pane only if it may collapse, and room left for the fill pane.
    fn settled(mut self) -> Self {
        let fill = self.fill_index();
        for (at, pane) in self.panes.iter_mut().enumerate() {
            pane.fill = at == fill;
            pane.min = pane.min.min(WHOLE);
            pane.max = pane.max.clamp(pane.min, WHOLE);
            pane.size = pane.size.clamp(pane.min, pane.max);
            pane.collapsed &= pane.collapsible && !pane.fill;
        }
        let sizes: Vec<u16> = self.panes.iter().map(|pane| pane.size).collect();
        let shown: Vec<bool> = self.panes.iter().map(|pane| !pane.collapsed).collect();
        let limits: Vec<(u16, u16)> = self.panes.iter().map(|p| (p.min, p.max)).collect();
        let fitted = fit(&sizes, &shown, &limits, fill);
        for (pane, size) in self.panes.iter_mut().zip(fitted) {
            pane.size = size;
        }
        self
    }

    fn into_node(self) -> Node {
        let split = self.settled();
        let fill = split.fill_index();
        let current = split
            .current
            .clone()
            .filter(|id| split.panes.iter().any(|pane| &pane.id == id))
            .or_else(|| split.panes.get(fill).map(|pane| pane.id.clone()));
        let horizontal = split.orientation == Orientation::Horizontal;
        let mut root = el("div")
            .slot("split")
            .attr("id", split.id.clone())
            .flag("data-ui-split", true)
            .attr("data-orientation", split.orientation.as_str())
            .attr_opt("data-stack", split.stack_below.map(SplitWidth::as_str))
            .attr_opt("data-ui-split-save", split.save.clone())
            .uses(Uses::SPLIT);
        root = match split.height {
            SplitHeight::Auto => root,
            SplitHeight::Sm => root.attr("data-height", "sm"),
            SplitHeight::Md => root.attr("data-height", "md"),
            SplitHeight::Lg => root.attr("data-height", "lg"),
            SplitHeight::Screen => root.flag("data-split-screen", true),
        };
        let count = split.panes.len();
        let mut children: Vec<Node> = Vec::with_capacity(count * 2);
        let ids: Vec<String> = split.panes.iter().map(|pane| pane.id.clone()).collect();
        let labels: Vec<String> = split.panes.iter().map(|pane| pane.label.clone()).collect();
        let below: Vec<Option<SplitWidth>> =
            split.panes.iter().map(|pane| pane.collapse_below).collect();
        for (at, pane) in split.panes.into_iter().enumerate() {
            let is_current = current.as_deref() == Some(pane.id.as_str());
            children.push(pane_node(pane, is_current));
            if at + 1 < count {
                // A separator moves the sized pane beside it: the one before it, unless
                // that one fills.
                let primary = if at == fill { at + 1 } else { at };
                children.push(
                    handle(
                        &split.id,
                        at,
                        &ids[primary],
                        &labels[primary],
                        primary > at,
                        horizontal,
                        split.grip,
                    )
                    .attr_opt("data-collapse-below", below[primary].map(SplitWidth::as_str))
                    .into(),
                );
            }
        }
        root.child(children).into()
    }
}

/// One pane's markup.
fn pane_node(pane: Pane, current: bool) -> Node {
    let header = (!pane.header.is_empty())
        .then(|| el("header").slot("split-pane-header").child(pane.header));
    let content = el("div").slot("split-pane-content").child(pane.body);
    let body = el("div")
        .slot("split-pane-body")
        .flag("data-from-end", pane.from_end)
        .attr_opt("tabindex", pane.body_label.as_ref().map(|_| 0_i64))
        .attr_opt("aria-label", pane.body_label)
        .child(content);
    let footer = (!pane.footer.is_empty())
        .then(|| el("footer").slot("split-pane-footer").child(pane.footer));
    el("section")
        .slot("split-pane")
        .attr("id", pane.id)
        .attr("aria-label", pane.label)
        .flag("data-split-fill", pane.fill)
        .attr_opt(
            "data-split-basis",
            (!pane.fill).then(|| i64::from((pane.size + 5) / 10)),
        )
        .attr_opt("data-split-size", (!pane.fill).then(|| i64::from(pane.size)))
        .attr("data-split-min", i64::from(pane.min))
        .attr("data-split-max", i64::from(pane.max))
        .flag("data-split-collapsible", pane.collapsible)
        .flag("data-collapsed", pane.collapsed)
        .attr_opt("data-collapse-below", pane.collapse_below.map(SplitWidth::as_str))
        .flag("data-split-current", current)
        // The pane a page is about shows, though it was collapsed: the link to it says so.
        .flag(
            "data-split-open",
            current && !pane.fill && (pane.collapsed || pane.collapse_below.is_some()),
        )
        // What the person did in this page stays through a morph: the size they dragged
        // to, a pane they opened or collapsed.
        .attr("data-ui-keep-attrs", "data-collapsed data-split-open")
        .children((header, body, footer))
        .into()
}

/// One separator. Its value, its keys and its place in the tab order come with the
/// module: with no script it is a line, not a control.
fn handle(
    split: &str,
    at: usize,
    primary: &str,
    label: &str,
    primary_after: bool,
    horizontal: bool,
    grip: bool,
) -> Element {
    el("div")
        .slot("split-handle")
        .attr("id", format!("{split}-separator-{at}"))
        .attr("role", "separator")
        // Panes side by side have an upright separator.
        .attr(
            "aria-orientation",
            if horizontal { "vertical" } else { "horizontal" },
        )
        .attr("aria-controls", primary.to_owned())
        .attr("aria-label", label.to_owned())
        .flag("data-split-after", primary_after)
        .attr(
            "data-ui-keep-attrs",
            "tabindex aria-valuenow aria-valuemin aria-valuemax aria-valuetext data-dragging",
        )
        .child(grip.then(|| el("span").slot("split-grip").attr("aria-hidden", "true")))
}

/// Sizes that fit: each pane between its least and most, and the panes in view leaving the
/// fill pane at least its least. A collapsed pane keeps its size, to come back at, and takes
/// no room. Room is taken back from the last sized pane first, never below a pane's least;
/// if the leasts alone are too much, the fill pane gets what is left.
fn fit(sizes: &[u16], shown: &[bool], limits: &[(u16, u16)], fill: usize) -> Vec<u16> {
    let mut out: Vec<u16> = sizes
        .iter()
        .zip(limits)
        .map(|(size, (min, max))| (*size).clamp(*min, *max))
        .collect();
    let fill_min = limits.get(fill).map_or(0, |(min, _)| *min);
    let budget = WHOLE.saturating_sub(fill_min);
    let used = |out: &[u16]| -> u32 {
        out.iter()
            .zip(shown)
            .enumerate()
            .filter(|(at, (_, shown))| *at != fill && **shown)
            .map(|(_, (size, _))| u32::from(*size))
            .sum()
    };
    let mut over = used(&out).saturating_sub(u32::from(budget));
    for at in (0..out.len()).rev() {
        if over == 0 {
            break;
        }
        if at == fill || !shown[at] {
            continue;
        }
        let room = u32::from(out[at] - limits[at].0);
        let take = room.min(over);
        out[at] -= u16::try_from(take).unwrap_or(0);
        over -= take;
    }
    let rest = u16::try_from(u32::from(WHOLE).saturating_sub(used(&out))).unwrap_or(0);
    if let Some(slot) = out.get_mut(fill) {
        *slot = rest;
    }
    out
}

/// Sizes a person chose, read from a save and checked against the layout. Draw a page
/// with them through [`SplitPane::sizes`], and keep them with [`to_text`](Self::to_text).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SplitSizes {
    /// Tenths of a percent, one per pane.
    sizes: Vec<u16>,
    collapsed: Vec<bool>,
}

/// The longest list of sizes or indexes read.
const MOST_TEXT: usize = 256;
/// The most panes a split has.
const MOST_PANES: usize = 12;

impl SplitSizes {
    /// The sizes in `sizes` (a percent per pane, at most one decimal, from 0 to 100) and
    /// the panes named collapsed in `collapsed` (their indexes), for `split`. `None` when
    /// they don't read: another number of sizes, a size that isn't a number in range, an
    /// index past the end or doubled, or a pane collapsed that may not be. Sizes that read
    /// are held to the layout's rules, as the page draws them.
    pub fn read(sizes: &str, collapsed: &str, split: &SplitPane) -> Option<Self> {
        if sizes.len() > MOST_TEXT || collapsed.len() > MOST_TEXT {
            return None;
        }
        let count = split.panes.len();
        if count == 0 || count > MOST_PANES {
            return None;
        }
        let read: Vec<u16> = sizes
            .split(',')
            .map(|size| parse_tenths(size.trim()))
            .collect::<Option<Vec<u16>>>()?;
        if read.len() != count {
            return None;
        }
        let mut shut = vec![false; count];
        if !collapsed.trim().is_empty() {
            for index in collapsed.split(',') {
                let index: usize = index.trim().parse().ok()?;
                let pane = split.panes.get(index)?;
                if shut[index] || !pane.collapsible {
                    return None;
                }
                shut[index] = true;
            }
        }
        let mut layout = split.clone();
        for ((pane, size), closed) in layout.panes.iter_mut().zip(&read).zip(&shut) {
            pane.size = *size;
            pane.collapsed = *closed;
        }
        let layout = layout.settled();
        Some(Self {
            sizes: layout.panes.iter().map(|pane| pane.size).collect(),
            collapsed: layout.panes.iter().map(|pane| pane.collapsed).collect(),
        })
    }

    /// Sizes kept with [`to_text`](Self::to_text), read again for `split`. `None` when
    /// they no longer fit it.
    pub fn from_text(text: &str, split: &SplitPane) -> Option<Self> {
        let (sizes, collapsed) = text.split_once(';').unwrap_or((text, ""));
        Self::read(sizes, collapsed, split)
    }

    /// The sizes as one line of text, to keep: `"28,48,24;2"`.
    pub fn to_text(&self) -> String {
        let mut out = String::with_capacity(self.sizes.len() * 6 + 4);
        for (at, size) in self.sizes.iter().enumerate() {
            if at > 0 {
                out.push(',');
            }
            write_tenths(&mut out, *size);
        }
        out.push(';');
        let mut first = true;
        for (at, closed) in self.collapsed.iter().enumerate() {
            if *closed {
                if !first {
                    out.push(',');
                }
                first = false;
                let _ = write!(out, "{at}");
            }
        }
        out
    }

    /// Each pane's size in tenths of a percent, the fill pane's included. A collapsed pane
    /// keeps the size it comes back at.
    pub fn tenths(&self) -> &[u16] {
        &self.sizes
    }

    /// Whether the pane at `index` is collapsed.
    pub fn is_collapsed(&self, index: usize) -> bool {
        self.collapsed.get(index).copied().unwrap_or(false)
    }
}

/// `"26.3"` as 263: a number from 0 to 100 with at most one decimal.
fn parse_tenths(text: &str) -> Option<u16> {
    if text.is_empty() || text.len() > 5 {
        return None;
    }
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() || fraction.len() > 1 || (text.contains('.') && fraction.is_empty()) {
        return None;
    }
    if !whole.bytes().all(|b| b.is_ascii_digit()) || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let whole: u16 = whole.parse().ok()?;
    let fraction: u16 = if fraction.is_empty() {
        0
    } else {
        fraction.parse().ok()?
    };
    let value = whole.checked_mul(10)?.checked_add(fraction)?;
    (value <= WHOLE).then_some(value)
}

fn write_tenths(out: &mut String, value: u16) {
    if value % 10 == 0 {
        let _ = write!(out, "{}", value / 10);
    } else {
        let _ = write!(out, "{}.{}", value / 10, value % 10);
    }
}

/// What a split's separators post when a person changes the sizes. It answers forgery
/// as a form does: a post from another site gets 403, anything but a urlencoded body 415,
/// a `GET` 405, and a body over 4 KiB 413.
///
/// ```ignore
/// async fn panes(save: SplitSave, uow: Uow, me: Agent) -> Result<StatusCode> {
///     let Some(sizes) = save.sizes_for(&inbox_layout()) else {
///         return Ok(StatusCode::BAD_REQUEST);
///     };
///     store(&uow, me.id, save.split(), &sizes.to_text()).await?;
///     Ok(StatusCode::NO_CONTENT)
/// }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SplitSave {
    split: String,
    sizes: String,
    collapsed: String,
}

/// The largest save read.
const MOST_SAVE: usize = 4096;

impl SplitSave {
    /// Reads a save from a urlencoded body. `None` when a field is missing or doubled.
    pub fn parse(body: &[u8]) -> Option<Self> {
        let mut split = None;
        let mut sizes = None;
        let mut collapsed = None;
        for (count, (name, value)) in urlencoded::parse(body).enumerate() {
            if count > 16 {
                return None;
            }
            let slot = match name.as_ref() {
                "split" => &mut split,
                "sizes" => &mut sizes,
                "collapsed" => &mut collapsed,
                _ => continue,
            };
            if slot.is_some() {
                return None;
            }
            *slot = Some(value.into_owned());
        }
        Some(Self {
            split: split?,
            sizes: sizes?,
            collapsed: collapsed.unwrap_or_default(),
        })
    }

    /// The split it says it is for.
    pub fn split(&self) -> &str {
        &self.split
    }

    /// The sizes, if this save is for `split` and they read: see [`SplitSizes::read`].
    pub fn sizes_for(&self, split: &SplitPane) -> Option<SplitSizes> {
        if self.split != split.id {
            return None;
        }
        SplitSizes::read(&self.sizes, &self.collapsed, split)
    }
}

impl FromRequest for SplitSave {
    type Rejection = StatusCode;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        let (parts, body): (Parts, _) = request.into_parts();
        if parts.method != Method::POST {
            return Err(StatusCode::METHOD_NOT_ALLOWED);
        }
        if let Some(status) = crate::form::refusal(&parts).await {
            return Err(status);
        }
        let limit = BodyLimit::of(&parts.extensions);
        let bytes = read_body(body, limit)
            .await
            .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
        if bytes.len() > MOST_SAVE {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        Self::parse(&bytes).ok_or(StatusCode::BAD_REQUEST)
    }
}

into_node!(SplitPane);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    fn inbox() -> SplitPane {
        SplitPane::new("inbox")
            .stack_below(SplitWidth::Md)
            .pane(Pane::new("list", "Conversations").size(28).min(20).max(40))
            .pane(Pane::fill("thread", "Conversation").min(30))
            .pane(
                Pane::new("customer", "Customer")
                    .size(24)
                    .min(16)
                    .max(36)
                    .collapsible(),
            )
    }

    #[test]
    fn each_separator_moves_the_sized_pane_beside_it() {
        let html = render(&inbox().into());
        assert!(html.contains(r#"id="inbox-separator-0" role="separator" aria-orientation="vertical" aria-controls="list" aria-label="Conversations""#), "{html}");
        assert!(html.contains(r#"id="inbox-separator-1" role="separator" aria-orientation="vertical" aria-controls="customer" aria-label="Customer" data-split-after"#), "{html}");
        // No tab stop and no value until the module runs: with no script it is a line.
        assert!(!html.contains("tabindex=\"0\" role=\"separator\""));
        assert!(!html.contains("aria-valuenow=\""));
        assert!(html.contains(r#"data-split-basis="28""#));
        assert!(html.contains(r#"data-split-basis="24""#));
        assert!(html.contains(r#"<section data-slot="split-pane" id="thread" aria-label="Conversation" data-split-fill"#), "{html}");
        assert!(crate::render::measure(&inbox().into()).uses.has(Uses::SPLIT));
    }

    #[test]
    fn the_current_pane_is_the_fill_pane_unless_said() {
        let html = render(&inbox().into());
        assert_eq!(html.matches("data-split-current").count(), 1);
        assert!(html.contains(r#"id="thread" aria-label="Conversation" data-split-fill data-split-min="300" data-split-max="900" data-split-current"#), "{html}");
        let html = render(&inbox().current("list").into());
        assert!(html.contains(r#"data-split-max="400" data-split-current"#), "{html}");
        // A pane that isn't there is no current pane: the fill pane is.
        let html = render(&inbox().current("nope").into());
        assert!(html.contains(r#"data-split-fill data-split-min="300" data-split-max="900" data-split-current"#));
    }

    #[test]
    fn sizes_are_held_to_their_limits_and_leave_the_fill_pane_its_least() {
        let wide = SplitPane::new("s")
            .pane(Pane::new("a", "A").size(60).min(10).max(90))
            .pane(Pane::new("b", "B").size(60).min(10).max(90))
            .pane(Pane::fill("c", "C").min(20));
        let settled = wide.settled();
        let sizes: Vec<u16> = settled.panes.iter().map(|p| p.size).collect();
        // 80% is all the sized panes may have; the last gives back first.
        assert_eq!(sizes, vec![600, 200, 200]);
        // Min over max: the least wins.
        let odd = SplitPane::new("s")
            .pane(Pane::new("a", "A").size(5).min(50).max(20))
            .pane(Pane::fill("b", "B"));
        let settled = odd.settled();
        assert_eq!(settled.panes[0].size, 500);
        assert_eq!(settled.panes[0].max, 500);
        // Leasts that leave too little: the fill pane gets what is left, never less than 0.
        let tight = SplitPane::new("s")
            .pane(Pane::new("a", "A").min(70))
            .pane(Pane::new("b", "B").min(70))
            .pane(Pane::fill("c", "C").min(20));
        let settled = tight.settled();
        assert_eq!(settled.panes.iter().map(|p| p.size).collect::<Vec<_>>(), vec![700, 700, 0]);
    }

    #[test]
    fn with_no_fill_pane_the_last_fills_and_with_two_the_first_does() {
        let none = SplitPane::new("s").pane(Pane::new("a", "A")).pane(Pane::new("b", "B"));
        assert_eq!(none.fill_index(), 1);
        let html = render(&none.into());
        assert!(html.contains(r#"id="b" aria-label="B" data-split-fill"#));
        let two = SplitPane::new("s")
            .pane(Pane::fill("a", "A"))
            .pane(Pane::fill("b", "B"));
        let html = render(&two.into());
        assert_eq!(html.matches("data-split-fill").count(), 1);
        assert!(html.contains(r#"id="a" aria-label="A" data-split-fill"#));
        // With the first pane filling, the separator moves the second.
        assert!(html.contains(r#"aria-controls="b""#));
    }

    #[test]
    fn a_pane_collapses_only_if_it_may() {
        let html = render(
            &SplitPane::new("s")
                .pane(Pane::new("a", "A").collapsed(true))
                .pane(Pane::new("b", "B").collapsible().collapsed(true))
                .pane(Pane::fill("c", "C"))
                .into(),
        );
        assert_eq!(html.matches(" data-collapsed").count(), 1, "{html}");
        assert!(html.contains(r#"data-split-collapsible data-collapsed"#));
    }

    #[test]
    fn saved_sizes_read_and_are_held_to_the_layout() {
        let layout = inbox();
        let sizes = SplitSizes::read("26.5,49.5,24", "", &layout).unwrap();
        assert_eq!(sizes.to_text(), "26.5,49.5,24;");
        // Out of the pane's limits: held to them.
        let sizes = SplitSizes::read("5,10,85", "", &layout).unwrap();
        assert_eq!(sizes.tenths(), &[200, 440, 360]);
        // Collapsed: the fill pane takes its room, and it keeps its size to come back at.
        let sizes = SplitSizes::read("30,46,24", "2", &layout).unwrap();
        assert!(sizes.is_collapsed(2));
        assert_eq!(sizes.to_text(), "30,70,24;2");
        let page = render(&layout.clone().sizes(&sizes).into());
        assert!(page.contains("data-split-collapsible data-collapsed"));
        assert!(page.contains(r#"data-split-basis="30""#));
        // What was kept reads back.
        assert_eq!(SplitSizes::from_text(&sizes.to_text(), &layout), Some(sizes));
    }

    #[test]
    fn saved_sizes_that_do_not_read_are_refused() {
        let layout = inbox();
        for (sizes, collapsed) in [
            ("", ""),
            ("30,40", ""),
            ("30,40,30,0", ""),
            ("NaN,40,30", ""),
            ("-1,40,30", ""),
            ("101,0,0", ""),
            ("1e2,0,0", ""),
            ("30.25,40,30", ""),
            ("30.,40,30", ""),
            (".5,40,30", ""),
            ("0x1e,40,30", ""),
            ("30, 40 ,30 ", "0"),
            ("30,40,30", "1"),
            ("30,40,30", "2,2"),
            ("30,40,30", "3"),
            ("30,40,30", "-1"),
            ("30,40,30", "x"),
            ("99999999999999999999,0,0", ""),
        ] {
            assert_eq!(SplitSizes::read(sizes, collapsed, &layout), None, "{sizes:?} {collapsed:?}");
        }
        let long = "1,".repeat(200);
        assert_eq!(SplitSizes::read(&long, "", &layout), None);
        assert_eq!(SplitSizes::read("30,40,30", "", &SplitPane::new("empty")), None);
        // Saved sizes for another layout leave the page as it is drawn.
        let other = SplitSizes::read("50,50", "", &SplitPane::new("x").pane(Pane::new("a", "A")).pane(Pane::fill("b", "B"))).unwrap();
        let page = render(&layout.sizes(&other).into());
        assert!(page.contains(r#"data-split-basis="28""#));
    }

    #[test]
    fn a_save_names_its_split_and_each_field_once() {
        let save = SplitSave::parse(b"split=inbox&sizes=30%2C46%2C24&collapsed=2").unwrap();
        assert_eq!(save.split(), "inbox");
        assert!(save.sizes_for(&inbox()).unwrap().is_collapsed(2));
        let other = SplitSave::parse(b"split=other&sizes=30,46,24").unwrap();
        assert_eq!(other.sizes_for(&inbox()), None);
        assert_eq!(SplitSave::parse(b"split=inbox"), None);
        assert_eq!(SplitSave::parse(b"split=inbox&split=x&sizes=1,2,3"), None);
        assert_eq!(SplitSave::parse(b"split=inbox&sizes=1&sizes=2"), None);
        let many = "a=1&".repeat(40) + "split=inbox&sizes=30,46,24";
        assert_eq!(SplitSave::parse(many.as_bytes()), None);
        assert!(SplitSave::parse(b"").is_none());
    }

    #[test]
    fn hostile_ids_and_labels_are_escaped() {
        let html = render(
            &SplitPane::new("\"><script>")
                .pane(Pane::new("a\"b", "<b>A</b>"))
                .pane(Pane::fill("c", "C"))
                .into(),
        );
        assert!(!html.contains("<script>"));
        assert!(!html.contains("<b>"));
        assert!(html.contains("&lt;b&gt;A&lt;/b&gt;"));
    }

    #[test]
    fn panes_have_a_header_a_body_that_scrolls_and_a_footer() {
        let html = render(
            &inbox()
                .pane_with("thread", |pane| {
                    pane.header("Subject")
                        .body("Messages")
                        .footer("Composer")
                        .from_end()
                        .body_label("Messages")
                })
                .into(),
        );
        assert!(html.contains(r#"<header data-slot="split-pane-header">Subject</header><div data-slot="split-pane-body" data-from-end tabindex="0" aria-label="Messages"><div data-slot="split-pane-content">Messages</div></div><footer data-slot="split-pane-footer">Composer</footer>"#), "{html}");
        // A pane with no header or footer has none.
        assert!(html.contains(r#"id="list" aria-label="Conversations" data-split-basis="28" data-split-size="280" data-split-min="200" data-split-max="400" data-ui-keep-attrs="data-collapsed data-split-open"><div data-slot="split-pane-body"><div data-slot="split-pane-content"></div></div></section>"#), "{html}");
    }

    #[test]
    fn a_toggle_names_the_pane_and_loads_the_module() {
        let toggle = SplitPane::toggle(Button::outline("Customer").href("/inbox/7?pane=customer"), "customer");
        let node: Node = toggle.into();
        assert!(crate::render::measure(&node).uses.has(Uses::SPLIT));
        let html = render(&node);
        assert!(html.contains(r#"data-ui-split-toggle="customer" aria-controls="customer""#), "{html}");
    }

    #[test]
    fn stacked_and_side_by_side_content_is_marked() {
        assert_eq!(
            render(&SplitPane::stacked_only("Back")),
            r#"<span data-split-only="stacked">Back</span>"#
        );
        assert_eq!(
            render(&SplitPane::side_by_side_only("Wide")),
            r#"<span data-split-only="wide">Wide</span>"#
        );
    }

    #[test]
    fn a_vertical_split_has_level_separators_and_a_height() {
        let html = render(
            &SplitPane::new("v")
                .vertical()
                .height(SplitHeight::Md)
                .grip()
                .pane(Pane::new("top", "Top"))
                .pane(Pane::fill("bottom", "Bottom"))
                .into(),
        );
        assert!(html.contains(r#"data-orientation="vertical""#));
        assert!(html.contains(r#"data-height="md""#));
        assert!(html.contains(r#"aria-orientation="horizontal""#));
        assert!(html.contains(r#"<span data-slot="split-grip" aria-hidden="true"></span>"#));
        let screen = render(&SplitPane::new("s").height(SplitHeight::Screen).pane(Pane::fill("a", "A")).into());
        assert!(screen.contains("data-split-screen"));
    }

    #[test]
    fn tenths_read_and_write() {
        assert_eq!(parse_tenths("0"), Some(0));
        assert_eq!(parse_tenths("100"), Some(1000));
        assert_eq!(parse_tenths("26.3"), Some(263));
        assert_eq!(parse_tenths("100.1"), None);
        assert_eq!(parse_tenths("007"), Some(70));
        let mut out = String::new();
        write_tenths(&mut out, 263);
        out.push(' ');
        write_tenths(&mut out, 300);
        assert_eq!(out, "26.3 30");
    }
}
