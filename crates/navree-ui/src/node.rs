//! The tree every component builds: [`Node`], [`Element`] and the attribute
//! contract.
//!
//! Components are plain structs with builder methods that turn into one
//! non-generic [`Node`], so pages have one type and no generic bloat.
//!
//! **The attribute contract.** Style and behaviour meet at attributes:
//! - `data-slot="button"`: what a part is. Set with [`Element::slot`].
//! - `data-variant`, `data-size`, `data-tone`: a closed choice, from a Rust enum.
//! - `data-open`, `data-checked`, `data-selected`, `data-disabled`,
//!   `data-active`, `data-invalid`, `data-pending`: states, present or absent.
//! - `data-ui-*`: the runtime's hooks.
//!
//! Attribute names are `&'static str`, so a request can't choose one. `style`,
//! `srcdoc` and `on*` are refused in any case (the content security policy would
//! block them anyway), URL attributes go through [`Href`], and elements that load
//! or navigate (`script`, `style`, `link`, `meta`, `base`, `iframe`, `object`,
//! `embed`) can't be built.

use std::borrow::Cow;

use serde::Serialize;

use crate::cmd::{Cmd, Commands};
use crate::extension::UiExtension;
use crate::html::{Href, LocalHref, SafeHtml};

/// One thing in a page.
#[derive(Clone, Debug, Default)]
pub enum Node {
    /// Nothing.
    #[default]
    Empty,
    /// Text, escaped when written.
    Text(Cow<'static, str>),
    /// An integer, written without allocating.
    Int(i64),
    /// Markup written as it is.
    Raw(SafeHtml),
    /// Compile-time markup written as it is, such as an icon's shapes.
    Static(&'static str),
    /// An element.
    Element(Element),
    /// Several nodes in a row.
    Fragment(Vec<Node>),
}

/// Dropping a tree takes no stack per level: however deep an app built it from its
/// data, a free walks an explicit list and can't overflow. Shallow nodes, nearly all
/// of them, are the first check.
impl Drop for Node {
    fn drop(&mut self) {
        let kids = match self {
            Self::Element(element) => &mut element.children,
            Self::Fragment(nodes) => nodes,
            _ => return,
        };
        if kids.iter().all(Self::is_leaf) {
            return;
        }
        let mut stack = std::mem::take(kids);
        while let Some(mut node) = stack.pop() {
            match &mut node {
                Self::Element(element) => stack.append(&mut element.children),
                Self::Fragment(nodes) => stack.append(nodes),
                _ => {}
            }
        }
    }
}

impl Node {
    /// Whether this holds no children, so dropping it can't recurse.
    fn is_leaf(&self) -> bool {
        match self {
            Self::Element(element) => element.children.is_empty(),
            Self::Fragment(nodes) => nodes.is_empty(),
            _ => true,
        }
    }

    /// The element this is, or nothing.
    pub(crate) fn into_element(mut self) -> Option<Element> {
        match &mut self {
            Self::Element(element) => Some(std::mem::replace(element, el("div"))),
            _ => None,
        }
    }

    /// A text node.
    pub fn text(text: impl Into<Cow<'static, str>>) -> Self {
        Self::Text(text.into())
    }

    /// Several nodes in a row.
    pub fn list(items: impl IntoNodes) -> Self {
        let mut nodes = Vec::new();
        items.push_into(&mut nodes);
        Self::Fragment(nodes)
    }

    /// The text inside this node, as a user reads it.
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out
    }

    fn collect_text(&self, out: &mut String) {
        match self {
            Self::Empty | Self::Static(_) => {}
            Self::Raw(html) => {
                // Tests read this: tags go, and the entities the escaping path writes.
                let mut inside = false;
                let mut text = String::new();
                for c in html.as_str().chars() {
                    match c {
                        '<' => inside = true,
                        '>' => inside = false,
                        _ if !inside => text.push(c),
                        _ => {}
                    }
                }
                out.push_str(
                    &text
                        .replace("&lt;", "<")
                        .replace("&gt;", ">")
                        .replace("&quot;", "\"")
                        .replace("&#39;", "'")
                        .replace("&amp;", "&"),
                );
            }
            Self::Text(text) => out.push_str(text),
            Self::Int(value) => out.push_str(&value.to_string()),
            Self::Element(element) => {
                for child in &element.children {
                    child.collect_text(out);
                }
            }
            Self::Fragment(nodes) => {
                for node in nodes {
                    node.collect_text(out);
                }
            }
        }
    }
}

/// An attribute's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttrValue {
    /// Present with no value (`disabled`).
    Flag,
    /// Text known at compile time.
    Static(&'static str),
    /// Text, escaped when written.
    Text(Box<str>),
    /// An integer.
    Int(i64),
}

impl AttrValue {
    /// The value as text, when it has one (`None` for a flag).
    pub fn as_text(&self) -> Option<Cow<'_, str>> {
        match self {
            Self::Flag => None,
            Self::Static(text) => Some(Cow::Borrowed(text)),
            Self::Text(text) => Some(Cow::Borrowed(text)),
            Self::Int(value) => Some(Cow::Owned(value.to_string())),
        }
    }
}

impl From<&'static str> for AttrValue {
    fn from(text: &'static str) -> Self {
        Self::Static(text)
    }
}

impl From<String> for AttrValue {
    fn from(text: String) -> Self {
        Self::Text(text.into_boxed_str())
    }
}

impl From<Cow<'static, str>> for AttrValue {
    fn from(text: Cow<'static, str>) -> Self {
        match text {
            Cow::Borrowed(text) => Self::Static(text),
            Cow::Owned(text) => Self::from(text),
        }
    }
}

impl From<i64> for AttrValue {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<u32> for AttrValue {
    fn from(value: u32) -> Self {
        Self::Int(i64::from(value))
    }
}

impl From<Href> for AttrValue {
    fn from(href: Href) -> Self {
        Self::Text(href.into_inner())
    }
}

/// What a page must load for the elements in it: a small set of bits, so
/// finding out allocates nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Uses(pub(crate) u64);

impl Uses {
    /// Nothing: the page ships no script.
    pub const NONE: Self = Self(0);
    /// The core runtime: commands, ops, morph, link boosting, pending states.
    pub const CORE: Self = Self(1);
    /// The sidebar module: collapse, `Ctrl/Cmd+B`, the phone sheet.
    pub const SIDEBAR: Self = Self(1 << 1);
    /// The overlay module: tooltip and hover card, placement where CSS can't.
    pub const OVERLAY: Self = Self(1 << 2);
    /// The table module: selection, resizing and reordering columns.
    pub const TABLE: Self = Self(1 << 3);
    /// The select module: Select, Combobox, MultiSelect and the command palette.
    pub const SELECT: Self = Self(1 << 4);
    /// The live module: regions and tables that fetch themselves again when a
    /// server-sent event stream speaks.
    pub const LIVE: Self = Self(1 << 6);
    /// The dates module: calendars, range pickers and the calendar of a date picker.
    pub const DATES: Self = Self(1 << 7);
    /// The date box module: the box a person types a date in, read by the server's
    /// rules in the page's language, with what it can't take said at the box.
    pub const DATE_TEXT: Self = Self(1 << 26);
    /// The time module: time pickers, their typed times and their list of times.
    pub const TIME: Self = Self(1 << 25);
    /// The menus module: the keys of tabs, toggle groups and dropdown menus, and
    /// tab switching in the page.
    pub const MENUS: Self = Self(1 << 5);
    /// The phone module: the country list, formatting as you type and the
    /// number in E.164.
    pub const PHONE: Self = Self(1 << 8);
    /// The theme builder's stylesheet: no script, only its styles, linked on a
    /// page that draws the builder.
    pub const THEME_BUILDER: Self = Self(1 << 9);
    /// The charts module: tooltips, the crosshair and the keys that move through a
    /// chart's values. Its stylesheet comes with it, and with a chart that is loading,
    /// failed or empty.
    pub const CHARTS: Self = Self(1 << 11);
    /// The slider module: the filled part, the value as text, a gap between the
    /// two thumbs of a range and a click on its track.
    pub const SLIDER: Self = Self(1 << 12);
    /// The tree module: the keys of a tree view, expanding, choosing and children
    /// loaded as they are opened.
    pub const TREE: Self = Self(1 << 13);
    /// The toolbar module: one tab stop with the arrow keys, and what does not fit
    /// moved into a menu.
    pub const TOOLBAR: Self = Self(1 << 14);
    /// The sortable module: lists and boards whose items move by pointer, touch
    /// or keys, each move posted to the server.
    pub const SORTABLE: Self = Self(1 << 15);
    /// The split module: separators that resize panes by pointer and by the keys of
    /// the window splitter pattern, collapsing panes, and saving the sizes.
    pub const SPLIT: Self = Self(1 << 16);
    /// The keys module: single-key shortcuts ([`Element::hotkey`]) that click or focus
    /// what names them, never while someone types.
    pub const KEYS: Self = Self(1 << 17);
    /// The lazy module: lists that load pages by cursor as they are read, keep the
    /// reader's place, drop what is far off screen, and merge live changes.
    pub const LAZY: Self = Self(1 << 18);
    /// The markdown stylesheet: no script, only the styles of rendered Markdown and of
    /// a highlighted code block, linked on a page that draws one.
    pub const MARKDOWN: Self = Self(1 << 19);
    /// The upload module: a drop area, a list of the chosen files, progress over a
    /// plain form post, and large files sent in chunks. Its stylesheet comes with it.
    pub const UPLOAD: Self = Self(1 << 20);
    /// The one-time code module: digits only, a pasted code cleaned, and a form sent
    /// when the code is whole. Its stylesheet comes with it.
    pub const OTP: Self = Self(1 << 21);
    /// The carousel module: the buttons, the slide pickers, the keys of the pattern and
    /// automatic rotation that stops for the reader. Its stylesheet comes with it.
    pub const CAROUSEL: Self = Self(1 << 22);
    /// The menu bar module: context menus, menu bars and the navigation menu, on the
    /// dropdown menu's own popups. Its stylesheet comes with it.
    pub const MENUBAR: Self = Self(1 << 23);
    /// The islands module: imports the script of each [`Island`](crate::Island) on the
    /// page, mounts it, gives it new props and lets it go when it leaves.
    pub const ISLAND: Self = Self(1 << 10);
    /// The parts module: the colour picker keeping its fields and swatches in step, a
    /// clear button, a checkbox that is indeterminate and an avatar whose picture did not
    /// load. Its stylesheet is [`Uses::PARTS_STYLES`].
    pub const PARTS: Self = Self(1 << 28);
    /// The parts stylesheet: no script, only the styles of the parts that need none, such
    /// as an avatar group, a trace, a questionnaire, an alert's close button and actions,
    /// and a sparkline in a table cell.
    pub const PARTS_STYLES: Self = Self(1 << 29);
    /// The conversation module: Enter sends from a composer, and a conversation the
    /// reader is at the end of stays there while an answer grows.
    pub const GENUI: Self = Self(1 << 24);
    /// The chart paint stylesheet: no script, only the styles that chart marks share
    /// beyond the plain ones: the colours of gradients, glow, draw and grow motion (off
    /// when the reader asks for less motion), the points and columns of a sparkline, and
    /// a progress bar drawn as a meter.
    pub const CHART_PAINT: Self = Self(1 << 32);
    /// The count-up module: a number that counts up to its value when it comes into
    /// view. The value is written in the page, so it reads the same with no script and
    /// when the reader asks for less motion.
    pub const COUNT_UP: Self = Self(1 << 33);
    /// The radial charts' stylesheet: no script, only the styles of rings, gauges and the
    /// day clock: their tracks and arcs, the words in the middle sized by the ring, and the
    /// size each takes from a screen width up.
    pub const CHART_RADIAL: Self = Self(1 << 34);
    /// The linear chart stylesheet: no script, only the styles of the meter, the mini
    /// columns, the colour strip, the range scale and the trend indicator. The ones with
    /// a tooltip link the charts module as well.
    pub const CHART_LINEAR: Self = Self(1 << 35);
    /// The grid charts stylesheet: no script of its own, only the styles of the calendar and
    /// matrix heat maps and of the scatter plot, linked on a page that draws one. The charts
    /// module moves between their cells and points.
    pub const CHART_GRIDS: Self = Self(1 << 36);
    /// The app blocks stylesheet: no script of its own, only the styles of a figure tile's
    /// icon, unit, status and ring, the info tip, the chart skeletons, the breathing guide
    /// and the dock, linked on a page that draws one. An info tip links the overlay module.
    pub const APP_BLOCKS: Self = Self(1 << 37);
    /// The marketing stylesheet: no script, only the styles of the blocks of a public site,
    /// its header, hero, sections, pricing, questions, frames and footer, linked on a page
    /// that draws one. A part of a block that needs a script, such as a form, loads its own.
    pub const MARKETING: Self = Self(1 << 40);
    /// The sortable grid module: tiles that move by pointer, touch or keys and are taken
    /// away, with the whole order posted a moment after the last change. Its stylesheet is
    /// [`Uses::SORTABLE_GRID_STYLES`].
    pub const SORTABLE_GRID: Self = Self(1 << 41);
    /// The sortable grid's stylesheet: the grid, its tiles and their tools, linked with
    /// every sortable grid, one that doesn't move too.
    pub const SORTABLE_GRID_STYLES: Self = Self(1 << 42);
    /// The extensions module: mounts the scripts elements name, links sheets that arrive
    /// later, and seals what a script draws. It needs the core runtime.
    pub const EXTENSIONS: Self = Self(1 << 43);
    /// No script: the head links the stylesheets the page's elements name.
    pub const EXTENSION_SHEETS: Self = Self(1 << 44);

    /// Both sets together.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every bit of `other` is here.
    pub const fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any bit of `other` is here.
    pub const fn meets(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Whether this is [`Uses::NONE`].
    pub const fn is_none(self) -> bool {
        self.0 == 0
    }

    /// The extension bits of these, and nothing else.
    pub(crate) const fn extension_bits(self) -> Self {
        Self(self.0 & (Self::EXTENSIONS.0 | Self::EXTENSION_SHEETS.0))
    }

    /// These without the extension bits.
    pub(crate) const fn without_extensions(self) -> Self {
        Self(self.0 & !(Self::EXTENSIONS.0 | Self::EXTENSION_SHEETS.0))
    }
}

/// Whether `key` can be a hotkey: one character that a key types, neither a space
/// nor a control character.
pub(crate) fn valid_hotkey(key: &str) -> bool {
    let mut chars = key.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if !c.is_whitespace() && !c.is_control())
}

/// One attribute. Names are static, and written without escaping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attr {
    /// The name.
    pub name: &'static str,
    /// The value.
    pub value: AttrValue,
}

/// An HTML element.
#[derive(Clone, Debug)]
pub struct Element {
    pub(crate) tag: &'static str,
    pub(crate) attrs: Vec<Attr>,
    pub(crate) children: Vec<Node>,
    pub(crate) uses: Uses,
}

/// Elements a page never writes through the tree: each loads something, runs
/// something or moves the page. The head has its own writer, in `view`.
fn is_refused_tag(tag: &str) -> bool {
    [
        "script", "style", "link", "meta", "base", "iframe", "object", "embed",
    ]
    .iter()
    .any(|refused| refused.eq_ignore_ascii_case(tag))
}

/// An element named `tag`. A refused one comes out as a `div`.
#[inline]
pub fn el(tag: &'static str) -> Element {
    let tag = if is_refused_tag(tag) {
        debug_assert!(false, "`<{tag}>` is not allowed in a page's tree");
        "div"
    } else {
        tag
    };
    Element {
        tag,
        attrs: Vec::new(),
        children: Vec::new(),
        uses: Uses::NONE,
    }
}

/// Attributes that hold URLs: their values go through [`Href`].
fn is_url_attr(name: &str) -> bool {
    [
        "href",
        "src",
        "action",
        "formaction",
        "poster",
        "cite",
        "ping",
        "data",
        "background",
        "xlink:href",
    ]
    .iter()
    .any(|url| url.eq_ignore_ascii_case(name))
}

/// Attributes that are never written: inline style and handlers break the
/// content security policy, and `srcdoc` is a document in an attribute.
fn is_refused_attr(name: &str) -> bool {
    name.eq_ignore_ascii_case("style")
        || name.eq_ignore_ascii_case("srcdoc")
        || name
            .as_bytes()
            .get(..2)
            .is_some_and(|start| start.eq_ignore_ascii_case(b"on"))
}

/// Whether `path` holds an encoded slash or backslash (`%2f` or `%5c`, in any case).
fn has_encoded_slash(path: &str) -> bool {
    path.as_bytes()
        .windows(3)
        .any(|window| window.eq_ignore_ascii_case(b"%2f") || window.eq_ignore_ascii_case(b"%5c"))
}

/// Whether `path` holds a `.` or `..` segment, written plainly or with `%2e` in any case.
fn has_dot_segment(path: &str) -> bool {
    path.split('/').any(|segment| {
        if segment == "." || segment == ".." {
            return true;
        }
        if !segment.contains('%') || segment.len() > 6 {
            return false;
        }
        let bytes = segment.as_bytes();
        let mut dots = 0_usize;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'.' {
                dots += 1;
                i += 1;
            } else if bytes
                .get(i..i + 3)
                .is_some_and(|esc| esc.eq_ignore_ascii_case(b"%2e"))
            {
                dots += 1;
                i += 3;
            } else {
                return false;
            }
        }
        dots == 1 || dots == 2
    })
}

impl Element {
    /// Says how the screen this link leads to is presented by a native client, when the
    /// page knows: a form to fill in is a [`Task`](crate::Presented::Task), a record to
    /// glance at is a [`Sheet`](crate::Presented::Sheet). Without it the shell's routes
    /// decide. It writes no HTML: a browser's page is the page it was.
    ///
    /// ```
    /// use navree_ui::{Presented, el, render};
    ///
    /// let link = el("a").href("/orders/new").child("New order").presented(Presented::Task);
    /// assert_eq!(render(&link.into()), r#"<a href="/orders/new">New order</a>"#);
    /// ```
    #[cfg_attr(not(feature = "native-preview"), allow(unused_variables, unused_mut))]
    pub fn presented(mut self, how: crate::Presented) -> Self {
        #[cfg(feature = "native-preview")]
        self.say(|hints| hints.presented = Some(how));
        self
    }

    /// Says what a tap on this control starts on the device of a native app, in place of
    /// what the control does in a browser: a passkey in the system's own sheet where a
    /// browser goes to the passkey page. See [`NativeCall`](crate::NativeCall).
    ///
    /// The call is written only for a native client whose app lists the call's feature
    /// among its capabilities, and only when every path of the call is one on this site.
    /// Every other client gets the control as it is, and so does a browser: it writes no
    /// HTML. A link keeps its address as what the client does where its build can't start
    /// the call after all.
    ///
    /// ```
    /// use navree_ui::{NativeCall, NativeThen, el, render};
    ///
    /// let call = NativeCall::passkey_sign_in(
    ///     "/sign-in/passkey/native/options",
    ///     NativeThen::new("/sign-in/passkey/native", "sealed-by-the-server", 1_794_000_000),
    /// );
    /// let link = el("a").href("/sign-in/passkey").child("Sign in with a passkey").native_call(call);
    /// assert_eq!(
    ///     render(&link.into()),
    ///     r#"<a href="/sign-in/passkey">Sign in with a passkey</a>"#
    /// );
    /// ```
    #[cfg_attr(not(feature = "native-preview"), allow(unused_variables, unused_mut))]
    pub fn native_call(mut self, call: crate::NativeCall) -> Self {
        #[cfg(feature = "native-preview")]
        if let Some(device) = call.for_the_client() {
            self.say(|hints| hints.device = Some(device));
        }
        self
    }

    /// Sets a hint for a native client, when one is asking. What is said is kept with the
    /// request, and the element carries its key as a child that writes nothing (a
    /// zero-length slice of a text of the toolkit's own), so an element is no larger and
    /// the HTML is the HTML of the element without it.
    #[cfg(feature = "native-preview")]
    pub(crate) fn say(&mut self, set: impl FnOnce(&mut crate::hint::Hints)) {
        let Some(request) = crate::native::scope::native() else {
            return;
        };
        let held = self.children.iter().find_map(crate::hint::key_of);
        let key = request.hint(held.unwrap_or(0), set);
        if held.is_none()
            && let Some(mark) = crate::hint::key_mark(key)
        {
            self.children.push(mark);
        }
    }

    /// What was said of this element to the native client the current request is from.
    #[cfg(feature = "native-preview")]
    pub(crate) fn said(&self) -> Option<crate::hint::Hints> {
        let key = self.children.iter().find_map(crate::hint::key_of)?;
        crate::native::scope::native()?.hinted(key)
    }

    /// The element's tag name.
    pub fn tag(&self) -> &'static str {
        self.tag
    }

    /// The attributes, in order.
    pub fn attrs(&self) -> &[Attr] {
        &self.attrs
    }

    /// The children, in order.
    pub fn child_nodes(&self) -> &[Node] {
        &self.children
    }

    /// The value of attribute `name`, when set.
    pub fn get(&self, name: &str) -> Option<&AttrValue> {
        self.attrs
            .iter()
            .find(|attr| attr.name == name)
            .map(|attr| &attr.value)
    }

    /// Sets attribute `name`. A URL attribute is checked like an [`Href`];
    /// `style`, `srcdoc` and `on*` are dropped.
    #[inline]
    #[must_use]
    pub fn attr(mut self, name: &'static str, value: impl Into<AttrValue>) -> Self {
        if is_refused_attr(name) {
            debug_assert!(
                false,
                "`{name}` is not allowed under the content security policy"
            );
            return self;
        }
        let mut value = value.into();
        if is_url_attr(name)
            && let Some(text) = value.as_text()
        {
            value = Href::from(text.as_ref()).into();
        }
        self.attrs.push(Attr { name, value });
        self
    }

    /// Sets attribute `name` when `value` is `Some`.
    #[inline]
    #[must_use]
    pub fn attr_opt(self, name: &'static str, value: Option<impl Into<AttrValue>>) -> Self {
        match value {
            Some(value) => self.attr(name, value),
            None => self,
        }
    }

    /// Sets an attribute with no value (`disabled`, `data-open`) when `on`.
    #[inline]
    #[must_use]
    pub fn flag(mut self, name: &'static str, on: bool) -> Self {
        if on && !is_refused_attr(name) {
            self.attrs.push(Attr {
                name,
                value: AttrValue::Flag,
            });
        }
        self
    }

    /// Sets `data-slot`: what this part is.
    #[inline]
    #[must_use]
    pub fn slot(self, slot: &'static str) -> Self {
        self.attr("data-slot", slot)
    }

    /// Sets `id`.
    #[inline]
    #[must_use]
    pub fn id(self, id: impl Into<AttrValue>) -> Self {
        self.attr("id", id)
    }

    /// Sets `href`, already checked.
    #[inline]
    #[must_use]
    pub fn href(mut self, href: impl Into<Href>) -> Self {
        self.attrs.push(Attr {
            name: "href",
            value: href.into().into(),
        });
        self
    }

    /// Says this link may be fetched ahead of a click, on hover or focus. Only mark
    /// a link whose `GET` changes nothing: a request goes out with no click at all.
    #[inline]
    #[must_use]
    pub fn prefetch(self) -> Self {
        self.flag("data-ui-prefetch", true)
    }

    /// Says this link is fetched as soon as the page is quiet, with no hover: for the
    /// one or two places a user almost always goes next. It costs that page's bytes on
    /// every visit, so nothing is fetched this way unless a link asks; the runtime takes
    /// at most four a page, and none on a slow or data-saving connection.
    ///
    /// It pays most for a page that is the same for every user and says so with
    /// `View::cache("public, max-age=60")`. A page that differs per user stays
    /// `no-store`, and what was fetched for it stands in for a click only for a few
    /// seconds. Only mark a link whose `GET` changes nothing.
    #[inline]
    #[must_use]
    pub fn prefetch_eager(self) -> Self {
        self.attr("data-ui-prefetch", "eager")
    }

    /// Runs `commands` in the browser when this element is clicked, with no
    /// request to the server.
    #[must_use]
    pub fn on_click(self, commands: impl Into<Commands>) -> Self {
        self.on(crate::cmd::Trigger::Click, commands)
    }

    /// Runs `commands` in the browser when `trigger` happens on this element.
    #[must_use]
    ///
    /// Called again for the same trigger, or after [`on_click`](Self::on_click), it adds to
    /// the list: every command runs, in the order given.
    pub fn on(mut self, trigger: crate::cmd::Trigger, commands: impl Into<Commands>) -> Self {
        self.uses = self.uses.with(Uses::CORE);
        let json = commands.into().to_json();
        let name = trigger.attribute();
        if let Some(attr) = self.attrs.iter_mut().find(|attr| attr.name == name) {
            // Two lists the same code wrote, so one list is the first's items and then
            // the second's.
            let old = attr
                .value
                .as_text()
                .map(|text| text.into_owned())
                .unwrap_or_default();
            let merged = match (old.as_str(), json.as_str()) {
                ("" | "[]", _) => json,
                (_, "[]") => old,
                (old, new) => format!("{},{}", &old[..old.len() - 1], &new[1..]),
            };
            attr.value = AttrValue::Text(merged.into_boxed_str());
            return self;
        }
        self.attrs.push(Attr {
            name,
            value: AttrValue::Text(json.into_boxed_str()),
        });
        self
    }

    /// Runs one command when clicked.
    #[must_use]
    pub fn on_click_one(self, command: Cmd) -> Self {
        self.on_click(vec![command])
    }

    /// Says [`Cmd::fill`] writes the text of the row's cell in column `column` here: the
    /// column of a [`DataTable`](crate::DataTable) by its field's name, or, in a list that
    /// isn't a table, the row's element with that `data-col`. A hidden field gets it as
    /// its value.
    #[must_use]
    pub fn fill_cell(self, column: &'static str) -> Self {
        self.attr("data-ui-cell", column)
    }

    /// Lets [`Cmd::value`] set this hidden field: the state a set of buttons changes, such
    /// as the column a form sorts by. A hidden field without it, such as a form's token or
    /// method, can't be changed by a command.
    #[must_use]
    pub fn settable(self) -> Self {
        self.flag("data-ui-value", true)
    }

    /// Says [`Cmd::fill`] writes the row's key here. A hidden field gets it as its value,
    /// so a form in the dialog posts which row it acts on.
    #[must_use]
    pub fn fill_key(self) -> Self {
        self.flag("data-ui-row-key", true)
    }

    /// Makes `key` a shortcut for this element: pressed anywhere on the page, it clicks
    /// the element, or focuses it if it is a field. It never fires while someone types in
    /// a field or edits text, while a modifier other than Shift is held, during an input
    /// method's composition, or from outside a modal dialog that is open, so a single
    /// letter is safe. `aria-keyshortcuts` tells assistive technology.
    ///
    /// A key is one character a keyboard types: a letter, a digit or a sign such as `/`
    /// or `?` (uppercase letters mean Shift). Anything else, a space or a word, is
    /// refused, and the element is left as it was. When two elements name the same key,
    /// the first in the page that can take it does.
    ///
    /// ```
    /// use navree_ui::{el, render};
    ///
    /// let html = render(&el("a").href("/inbox/8").hotkey("j").child("Next").into());
    /// assert!(html.contains(r#"data-ui-hotkey="j""#));
    /// assert!(html.contains(r#"aria-keyshortcuts="j""#));
    /// ```
    #[must_use]
    pub fn hotkey(self, key: &str) -> Self {
        if !valid_hotkey(key) {
            return self;
        }
        self.attr("data-ui-hotkey", key.to_owned())
            .attr("aria-keyshortcuts", key.to_owned())
            .uses(Uses::KEYS)
    }

    /// Makes `key` a shortcut for this element, as [`hotkey`](Self::hotkey) does, that
    /// works only while the focus is inside the element whose id is `scope`: for a key
    /// that changes something, such as closing the conversation the reader is in, so it
    /// can't act from anywhere on the page. An invalid key or scope leaves the element as
    /// it was.
    ///
    /// Single keys need a way to be turned off for the people who can't use them (WCAG
    /// 2.1.4): keep each person's choice, and draw no hotkeys for someone who turned them
    /// off.
    ///
    /// ```
    /// use navree_ui::{el, render};
    ///
    /// let html = render(&el("button").hotkey_within("e", "thread").child("Close").into());
    /// assert!(html.contains(r#"data-ui-hotkey="e" aria-keyshortcuts="e" data-ui-hotkey-scope="thread""#));
    /// ```
    #[must_use]
    pub fn hotkey_within(self, key: &str, scope: &str) -> Self {
        let scope_ok = !scope.is_empty()
            && scope.len() <= 64
            && scope
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':' | '.'));
        if !valid_hotkey(key) || !scope_ok {
            return self;
        }
        self.hotkey(key)
            .attr("data-ui-hotkey-scope", scope.to_owned())
    }

    /// Says this page must load `uses` when this element is on it.
    #[inline]
    #[must_use]
    pub fn uses(mut self, uses: Uses) -> Self {
        self.uses = self.uses.with(uses);
        self
    }

    /// Makes this element the root of `extension`: it carries the addresses of the extension's
    /// script and stylesheet, the route prefixes its script may call, and the attributes a
    /// morph from the server leaves as the script set them. The page links the stylesheet and
    /// preloads the script, and the browser mounts the script on this element once. Give the
    /// element a slot of the namespace, a colon and a word (`geo:map`), which is what the
    /// stylesheet is scoped to.
    ///
    /// A second call replaces what the first wrote. An element that is an island names the
    /// extension's stylesheet only: its script is the island's own.
    ///
    /// ```ignore
    /// use navree_ui::{el, render};
    ///
    /// // `MAP` is the extension of plugins/geo, whose tests prove this example.
    /// let html = render(&el("div").slot("geo:map").extension(&MAP).into());
    /// assert!(html.contains("data-ui-ext="));
    /// ```
    #[must_use]
    pub fn extension(self, extension: &'static UiExtension) -> Self {
        self.named_extension(extension, None)
    }

    /// [`extension`](Self::extension) with props for the script: any value that serializes to
    /// JSON. They go in an attribute, escaped as every attribute is, and the script receives
    /// them parsed, so a value can't become markup or code. A value that can't be written as
    /// JSON is written as `null`. There is no size limit when the element is drawn; keep props
    /// under 64 KB and fetch the rest from a route (`kit::problems` reports props over that).
    ///
    /// ```ignore
    /// use navree_ui::{el, render};
    ///
    /// #[derive(serde::Serialize)]
    /// struct Place {
    ///     city: &'static str,
    ///     zoom: u8,
    /// }
    ///
    /// // `MAP` is the extension of plugins/geo, whose tests prove this example.
    /// let place = Place { city: "Ludhiana", zoom: 12 };
    /// let html = render(&el("div").slot("geo:map").extension_with(&MAP, &place).into());
    /// assert!(html.contains("data-ui-props="));
    /// ```
    #[must_use]
    pub fn extension_with(self, extension: &'static UiExtension, props: &impl Serialize) -> Self {
        let json = serde_json::to_string(props).unwrap_or_else(|_| "null".to_owned());
        self.named_extension(extension, Some(json))
    }

    /// Lets this element's script call exactly `href`, beside its extension's route
    /// prefixes: for an address only the server can name, such as the drop of one delivery.
    /// The query and the fragment are left out, and so is an address that is not a path of this
    /// site, holds whitespace, or has a `.` or `..` segment (write whitespace percent-encoded).
    /// Call it after [`extension`](Self::extension), which replaces the addresses an earlier call
    /// wrote.
    ///
    /// ```ignore
    /// use navree_ui::{el, render};
    ///
    /// // `MAP` is the extension of plugins/geo, whose tests prove this example.
    /// let element = el("div").slot("geo:map").extension(&MAP).extension_call("/deliveries/42/drop");
    /// assert!(render(&element.into()).contains("=/deliveries/42/drop"));
    /// ```
    #[must_use]
    pub fn extension_call(mut self, href: impl Into<LocalHref>) -> Self {
        let href = href.into();
        let path = href.as_str().split(['?', '#']).next().unwrap_or_default();
        if !path.starts_with('/')
            || path.contains('\\')
            || path.bytes().any(|byte| byte <= b' ')
            || path.chars().any(char::is_whitespace)
            || has_encoded_slash(path)
            || has_dot_segment(path)
        {
            return self;
        }
        let token = format!("={path}");
        let held = self
            .get("data-ui-ext-routes")
            .and_then(AttrValue::as_text)
            .map(Cow::into_owned);
        match held {
            Some(list) if list.split(' ').any(|have| have == token) => {}
            Some(list) => self.put("data-ui-ext-routes", format!("{list} {token}")),
            None => self.put("data-ui-ext-routes", token),
        }
        self
    }

    /// Sets attribute `name` to `value`, in place when the element has it already.
    fn put(&mut self, name: &'static str, value: String) {
        let value = AttrValue::Text(value.into_boxed_str());
        match self.attrs.iter_mut().find(|attr| attr.name == name) {
            Some(attr) => attr.value = value,
            None => self.attrs.push(Attr { name, value }),
        }
    }

    /// Writes what an element carries for `extension`, in this order, each only as it applies:
    /// the script's address, the stylesheet's, the route prefixes, the attributes a morph leaves
    /// (the toolkit's two and the extension's own, joined to any the element has), and the
    /// props. What an earlier call wrote is replaced.
    fn named_extension(mut self, extension: &'static UiExtension, props: Option<String>) -> Self {
        const OWN: [&str; 4] = [
            "data-ui-ext",
            "data-ui-ext-sheet",
            "data-ui-ext-routes",
            "data-ui-props",
        ];
        // An island's script, props and kept attributes are its own: only the sheet is named. A
        // plugin's island keeps the extensions module, which seals its body.
        if self.attrs.iter().any(|attr| attr.name == "data-ui-island") {
            debug_assert!(
                extension.script_file().is_none(),
                "an element is an island or carries an extension script, not both"
            );
            self.uses = Uses(self.uses.0 & !Uses::EXTENSION_SHEETS.0);
            self.attrs.retain(|attr| attr.name != "data-ui-ext-sheet");
            if let Some(sheet) = extension.sheet_url() {
                self.put("data-ui-ext-sheet", sheet.to_owned());
                self.uses = self.uses.with(Uses::EXTENSION_SHEETS);
            }
            return self;
        }
        self.uses = self.uses.without_extensions();
        self.attrs.retain(|attr| !OWN.contains(&attr.name));
        if let Some(script) = extension.script_url() {
            self.put("data-ui-ext", script.to_owned());
            self.uses = self.uses.with(Uses::EXTENSIONS).with(Uses::CORE);
        }
        if let Some(sheet) = extension.sheet_url() {
            self.put("data-ui-ext-sheet", sheet.to_owned());
            self.uses = self.uses.with(Uses::EXTENSION_SHEETS);
        }
        if !extension.routes_of().is_empty() {
            self.put("data-ui-ext-routes", extension.routes_of().join(" "));
        }
        let mut kept: Vec<String> = Vec::new();
        let held = self
            .get("data-ui-keep-attrs")
            .and_then(AttrValue::as_text)
            .map(Cow::into_owned);
        let tokens = held
            .iter()
            .flat_map(|held| held.split_whitespace())
            .chain(["data-ui-mounted", "data-ui-failed"])
            .chain(extension.keeps_of().iter().copied());
        for token in tokens {
            if !kept.iter().any(|have| have == token) {
                kept.push(token.to_owned());
            }
        }
        self.put("data-ui-keep-attrs", kept.join(" "));
        if let Some(json) = props {
            self.put("data-ui-props", json);
        }
        self
    }

    /// Adds one child.
    #[inline]
    #[must_use]
    pub fn child(mut self, child: impl IntoNodes) -> Self {
        child.push_into(&mut self.children);
        self
    }

    /// Adds children: a tuple, a list, an `Option`, or one node.
    #[inline]
    #[must_use]
    pub fn children(self, children: impl IntoNodes) -> Self {
        self.child(children)
    }

    /// Reserves room for `count` more children, when you know how many.
    #[inline]
    #[must_use]
    pub fn with_child_capacity(mut self, count: usize) -> Self {
        self.children.reserve_exact(count);
        self
    }
}

impl From<Element> for Node {
    #[inline]
    fn from(element: Element) -> Self {
        Self::Element(element)
    }
}

impl From<&'static str> for Node {
    #[inline]
    fn from(text: &'static str) -> Self {
        Self::Text(Cow::Borrowed(text))
    }
}

impl From<String> for Node {
    #[inline]
    fn from(text: String) -> Self {
        Self::Text(Cow::Owned(text))
    }
}

impl From<Cow<'static, str>> for Node {
    #[inline]
    fn from(text: Cow<'static, str>) -> Self {
        Self::Text(text)
    }
}

impl From<i64> for Node {
    #[inline]
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<SafeHtml> for Node {
    fn from(html: SafeHtml) -> Self {
        Self::Raw(html)
    }
}

/// What can stand as children: one node, a tuple, a list, an `Option`.
pub trait IntoNodes {
    /// Appends the nodes to `out`.
    fn push_into(self, out: &mut Vec<Node>);
}

impl<T: Into<Node>> IntoNodes for T {
    #[inline]
    fn push_into(self, out: &mut Vec<Node>) {
        out.push(self.into());
    }
}

impl<T: IntoNodes> IntoNodes for Vec<T> {
    #[inline]
    fn push_into(self, out: &mut Vec<Node>) {
        out.reserve(self.len());
        for item in self {
            item.push_into(out);
        }
    }
}

impl<T: IntoNodes, const N: usize> IntoNodes for [T; N] {
    #[inline]
    fn push_into(self, out: &mut Vec<Node>) {
        out.reserve(N);
        for item in self {
            item.push_into(out);
        }
    }
}

impl<T: IntoNodes> IntoNodes for Option<T> {
    #[inline]
    fn push_into(self, out: &mut Vec<Node>) {
        if let Some(item) = self {
            item.push_into(out);
        }
    }
}

macro_rules! tuples {
    ($($name:ident)+) => {
        impl<$($name: IntoNodes),+> IntoNodes for ($($name,)+) {
            #[inline]
            #[allow(non_snake_case)]
            fn push_into(self, out: &mut Vec<Node>) {
                let ($($name,)+) = self;
                $($name.push_into(out);)+
            }
        }
    };
}

tuples!(A);
tuples!(A B);
tuples!(A B C);
tuples!(A B C D);
tuples!(A B C D E);
tuples!(A B C D E F);
tuples!(A B C D E F G);
tuples!(A B C D E F G H);
tuples!(A B C D E F G H I);
tuples!(A B C D E F G H I J);
tuples!(A B C D E F G H I J K);
tuples!(A B C D E F G H I J K L);

/// Turns each type named into a `Node` by calling its own `into_node` method, so the component
/// can be given wherever a node goes: as a child, as a page's body, in a table's cell. It is
/// the last step of a component of your own, once its root element is built; the toolkit's own
/// components end the same way.
///
/// ```
/// use navree_ui::{Element, Node, el};
///
/// /// A badge with a city in it.
/// pub struct CityBadge {
///     el: Element,
///     city: &'static str,
/// }
///
/// navree_ui::component_node!(CityBadge);
///
/// impl CityBadge {
///     /// A badge for `city`.
///     pub fn new(city: &'static str) -> Self {
///         Self { el: el("span").slot("app:city-badge"), city }
///     }
///
///     fn into_node(self) -> Node {
///         self.el.child(self.city).into()
///     }
/// }
///
/// let page = navree_ui::View::new("Depots").body(CityBadge::new("Ludhiana"));
/// assert!(page.to_html().contains("Ludhiana"));
/// ```
#[macro_export]
macro_rules! component_node {
    ($($type:ty),+ $(,)?) => {
        $(impl ::core::convert::From<$type> for $crate::Node {
            #[inline]
            fn from(value: $type) -> Self {
                value.into_node()
            }
        })+
    };
}
pub(crate) use crate::component_node as into_node;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_in_attributes_are_checked() {
        let element = el("a").attr("href", "javascript:alert(1)");
        assert_eq!(element.get("href"), Some(&AttrValue::Text("#".into())));
        let element = el("a").attr("href", "/ok".to_owned());
        assert_eq!(element.get("href"), Some(&AttrValue::Text("/ok".into())));
    }

    #[test]
    fn children_take_tuples_lists_and_options() {
        let element =
            el("div").children(("a", Some("b"), None::<&'static str>, vec!["c", "d"], 7_i64));
        assert_eq!(element.child_nodes().len(), 5);
        assert_eq!(Node::from(element).text_content(), "abcd7");
    }

    #[test]
    fn commands_ask_for_the_core_runtime() {
        assert!(el("div").uses.is_none());
        let element = el("button").on_click_one(Cmd::toggle("menu"));
        assert!(element.uses.has(Uses::CORE));
    }

    #[test]
    fn a_hotkey_is_one_typed_character_and_loads_the_keys_module() {
        for key in ["j", "K", "/", "?", "1", "é"] {
            let element = el("a").hotkey(key);
            assert_eq!(
                element.get("data-ui-hotkey"),
                Some(&AttrValue::Text(key.into()))
            );
            assert_eq!(
                element.get("aria-keyshortcuts"),
                Some(&AttrValue::Text(key.into()))
            );
            assert!(element.uses.has(Uses::KEYS));
        }
        for bad in ["", " ", "jk", "Enter", "\n", "\u{7}", "ctrl+k"] {
            let element = el("a").hotkey(bad);
            assert!(element.get("data-ui-hotkey").is_none(), "{bad:?}");
            assert!(element.uses.is_none(), "{bad:?}");
        }
    }

    #[test]
    fn a_scoped_hotkey_names_its_scope_and_refuses_a_bad_one() {
        let html = crate::render(&el("button").hotkey_within("e", "thread").into());
        assert!(html.contains(r#"data-ui-hotkey="e""#), "{html}");
        assert!(html.contains(r#"data-ui-hotkey-scope="thread""#), "{html}");
        for bad in ["", "a b", "x\"y", &"z".repeat(65)] {
            let html = crate::render(&el("button").hotkey_within("e", bad).into());
            assert!(!html.contains("data-ui-hotkey"), "{bad}: {html}");
        }
        let html = crate::render(&el("button").hotkey_within("two", "thread").into());
        assert!(!html.contains("data-ui-hotkey"), "{html}");
    }

    #[test]
    fn the_new_modules_have_bits_of_their_own() {
        let known = [
            Uses::CORE,
            Uses::SIDEBAR,
            Uses::OVERLAY,
            Uses::TABLE,
            Uses::SELECT,
            Uses::MENUS,
            Uses::LIVE,
            Uses::DATES,
            Uses::PHONE,
            Uses::THEME_BUILDER,
            Uses::SLIDER,
            Uses::TREE,
            Uses::TOOLBAR,
            Uses::SORTABLE,
        ];
        for module in [Uses::SPLIT, Uses::KEYS, Uses::LAZY] {
            assert!(known.iter().all(|other| other.0 & module.0 == 0));
        }
        assert_eq!(Uses::SPLIT.0 & Uses::KEYS.0, 0);
        assert_eq!((Uses::SPLIT.0 | Uses::KEYS.0) & Uses::LAZY.0, 0);
    }

    #[test]
    fn the_markdown_upload_code_carousel_and_menu_bar_modules_have_bits_of_their_own() {
        let older = [
            Uses::CORE,
            Uses::SIDEBAR,
            Uses::OVERLAY,
            Uses::TABLE,
            Uses::SELECT,
            Uses::MENUS,
            Uses::LIVE,
            Uses::DATES,
            Uses::PHONE,
            Uses::THEME_BUILDER,
            Uses::SLIDER,
            Uses::TREE,
            Uses::TOOLBAR,
            Uses::SORTABLE,
            Uses::SPLIT,
            Uses::KEYS,
            Uses::LAZY,
        ];
        let newer = [
            Uses::MARKDOWN,
            Uses::UPLOAD,
            Uses::OTP,
            Uses::CAROUSEL,
            Uses::MENUBAR,
            Uses::PARTS,
            Uses::PARTS_STYLES,
            Uses::GENUI,
        ];
        for (at, module) in newer.iter().enumerate() {
            assert_eq!(module.0.count_ones(), 1);
            assert!(older.iter().all(|other| other.0 & module.0 == 0));
            assert!(newer[at + 1..].iter().all(|other| other.0 & module.0 == 0));
        }
    }

    #[test]
    fn a_node_stays_small() {
        // Children hold nodes inline, so one size change touches every page.
        assert!(
            std::mem::size_of::<Node>() <= 72,
            "{}",
            std::mem::size_of::<Node>()
        );
    }

    use crate::extension::ExtensionFile;

    /// A map with a script and a stylesheet, and a badge with a stylesheet only.
    static MAP: UiExtension = UiExtension::new("geo", "map")
        .script(ExtensionFile::__new(
            b"export const contract = 1; export function mount() {}",
            "e1613860bbf0",
            None,
        ))
        .sheet(ExtensionFile::__new(
            b"/* the map's stylesheet, scoped for geo */",
            "bbefcf02ae04",
            Some("geo"),
        ))
        .contract(1)
        .keeps(&["data-moving"])
        .routes(&["/geo/"]);
    static BADGE: UiExtension = UiExtension::new("geo", "badge").sheet(ExtensionFile::__new(
        b"/* the badge's stylesheet, scoped for geo */",
        "ea46b024fa00",
        Some("geo"),
    ));

    const MAP_ATTRIBUTES: &str = r#" data-ui-ext="/_navree/ui/ext/geo.map-e1613860bbf0.js" data-ui-ext-sheet="/_navree/ui/ext/geo.map-bbefcf02ae04.css" data-ui-ext-routes="/geo/""#;

    #[test]
    fn an_element_with_an_extension_names_its_files_and_asks_for_the_modules() {
        let element = el("div").slot("geo:map").extension(&MAP);
        assert_eq!(
            crate::render(&element.clone().into()),
            format!(
                r#"<div data-slot="geo:map"{MAP_ATTRIBUTES} data-ui-keep-attrs="data-ui-mounted data-ui-failed data-moving"></div>"#
            )
        );
        assert!(element.uses.has(Uses::EXTENSIONS));
        assert!(element.uses.has(Uses::EXTENSION_SHEETS));
        assert!(element.uses.has(Uses::CORE));

        // What the element kept already stays in front of what the extension keeps.
        let held = el("div")
            .attr("data-ui-keep-attrs", "data-collapsed")
            .slot("geo:map")
            .extension(&MAP);
        assert_eq!(
            held.get("data-ui-keep-attrs"),
            Some(&AttrValue::Text(
                "data-collapsed data-ui-mounted data-ui-failed data-moving".into()
            ))
        );
        let repeated = el("div")
            .attr("data-ui-keep-attrs", "data-moving data-ui-failed")
            .extension(&MAP);
        assert_eq!(
            repeated.get("data-ui-keep-attrs"),
            Some(&AttrValue::Text(
                "data-moving data-ui-failed data-ui-mounted".into()
            ))
        );

        // A second call replaces the attributes and the extension bits of the first.
        let replaced = el("div")
            .extension_with(&MAP, &serde_json::json!({ "zoom": 15 }))
            .extension(&BADGE);
        assert!(replaced.get("data-ui-ext").is_none());
        assert!(replaced.get("data-ui-ext-routes").is_none());
        assert!(replaced.get("data-ui-props").is_none());
        assert_eq!(
            replaced.get("data-ui-ext-sheet"),
            Some(&AttrValue::Text(
                "/_navree/ui/ext/geo.badge-ea46b024fa00.css".into()
            ))
        );
        assert!(replaced.uses.has(Uses::EXTENSION_SHEETS));
        assert!(!replaced.uses.has(Uses::EXTENSIONS));
    }

    #[test]
    fn a_sheet_only_extension_asks_for_no_script() {
        let element = el("span").extension(&BADGE);
        assert_eq!(
            crate::render(&element.clone().into()),
            r#"<span data-ui-ext-sheet="/_navree/ui/ext/geo.badge-ea46b024fa00.css" data-ui-keep-attrs="data-ui-mounted data-ui-failed"></span>"#
        );
        assert_eq!(element.uses, Uses::EXTENSION_SHEETS);
    }

    #[test]
    fn an_island_names_the_sheet_and_keeps_its_own_script_and_props() {
        let island = || {
            el("div")
                .attr("data-ui-island", "/_navree/ui/island/clock-0123456789ab.js")
                .attr("data-ui-props", "{}")
                .uses(Uses::ISLAND)
        };
        let styled = island().extension(&BADGE);
        assert_eq!(
            crate::render(&styled.clone().into()),
            r#"<div data-ui-island="/_navree/ui/island/clock-0123456789ab.js" data-ui-props="{}" data-ui-ext-sheet="/_navree/ui/ext/geo.badge-ea46b024fa00.css"></div>"#
        );
        assert!(styled.uses.has(Uses::ISLAND));
        assert!(styled.uses.has(Uses::EXTENSION_SHEETS));
        assert!(!styled.uses.has(Uses::EXTENSIONS));
    }

    /// A plugin's island has the page load the extensions module, which seals its body, and a
    /// sheet named on it does not take that away. An app's island loads no extensions module.
    #[test]
    fn a_plugins_island_keeps_the_extensions_module_when_it_names_a_sheet() {
        use crate::island::{Island, IslandScript};

        static CLOCK: IslandScript =
            IslandScript::new("clock", b"export function mount() {}", 1024).namespace("geo");
        static PLAIN_CLOCK: IslandScript =
            IslandScript::new("clock", b"export function mount() {}", 1024);
        let element = |island: Island| {
            Node::from(island)
                .into_element()
                .expect("an island is one element")
        };

        let plugins = element(Island::new("clock", &CLOCK));
        assert_eq!(plugins.uses, Uses::ISLAND.with(Uses::EXTENSIONS));
        let styled = plugins.extension(&BADGE);
        assert_eq!(
            styled.uses,
            Uses::ISLAND
                .with(Uses::EXTENSIONS)
                .with(Uses::EXTENSION_SHEETS)
        );
        // A second sheet replaces the first and keeps the module.
        assert_eq!(
            styled.extension(&BADGE).uses,
            Uses::ISLAND
                .with(Uses::EXTENSIONS)
                .with(Uses::EXTENSION_SHEETS)
        );

        let apps = element(Island::new("clock", &PLAIN_CLOCK));
        assert_eq!(apps.uses, Uses::ISLAND);
        assert_eq!(
            apps.extension(&BADGE).uses,
            Uses::ISLAND.with(Uses::EXTENSION_SHEETS)
        );
    }

    /// An island and an extension script on one element is a mistake that a debug build reports.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "an element is an island or carries an extension script, not both")]
    fn an_island_with_an_extension_script_is_refused_in_a_debug_build() {
        let _ = el("div")
            .attr("data-ui-island", "/_navree/ui/island/clock-0123456789ab.js")
            .extension(&MAP);
    }

    #[test]
    fn extension_call_adds_one_exact_address() {
        let plain = crate::render(&el("div").extension(&MAP).into());
        let called = crate::render(
            &el("div")
                .extension(&MAP)
                .extension_call("/deliveries/42/drop?from=table")
                .into(),
        );
        assert_eq!(
            called,
            plain.replace(
                r#"data-ui-ext-routes="/geo/""#,
                r#"data-ui-ext-routes="/geo/ =/deliveries/42/drop""#
            )
        );
        // The same address twice is one address; another one is another.
        let twice = el("div")
            .extension(&MAP)
            .extension_call("/deliveries/42/drop")
            .extension_call("/deliveries/42/drop#top")
            .extension_call("/deliveries/43/drop");
        assert_eq!(
            twice.get("data-ui-ext-routes"),
            Some(&AttrValue::Text(
                "/geo/ =/deliveries/42/drop =/deliveries/43/drop".into()
            ))
        );
        // An extension with no prefixes gets the attribute from the call.
        let only = el("div")
            .extension(&BADGE)
            .extension_call("/deliveries/42/drop");
        assert_eq!(
            only.get("data-ui-ext-routes"),
            Some(&AttrValue::Text("=/deliveries/42/drop".into()))
        );
        // A relative address, the page itself, another site and a path with a space add nothing.
        for refused in [
            "relative",
            "#",
            "?x=1",
            "//other.example/geo/x",
            "https://other.example/geo/x",
            "/deliveries/42 /drop",
            "",
        ] {
            let element = el("div").extension(&MAP).extension_call(refused);
            assert_eq!(crate::render(&element.into()), plain, "{refused:?}");
        }
    }

    #[test]
    fn extension_props_are_escaped_json() {
        #[derive(serde::Serialize)]
        struct Props {
            note: &'static str,
            city: &'static str,
        }
        let props = Props {
            note: "</div><script>alert(1)</script>",
            city: "Ludhiana",
        };
        let html = crate::render(&el("div").extension_with(&MAP, &props).into());
        assert!(
            html.contains(
                r#" data-ui-props="{&quot;note&quot;:&quot;&lt;/div&gt;&lt;script&gt;alert(1)&lt;/script&gt;&quot;,&quot;city&quot;:&quot;Ludhiana&quot;}""#
            ),
            "{html}"
        );
        assert!(!html.contains("<script"), "{html}");
        // Props that JSON can't write are `null`: a map keyed by pairs.
        let pairs = std::collections::BTreeMap::from([((1, 2), "x")]);
        let html = crate::render(&el("div").extension_with(&MAP, &pairs).into());
        assert!(html.contains(r#" data-ui-props="null""#), "{html}");
    }

    #[test]
    fn the_extension_bits_are_two_modules_of_their_own() {
        assert_eq!(Uses::EXTENSIONS.0.count_ones(), 1);
        assert_eq!(Uses::EXTENSION_SHEETS.0.count_ones(), 1);
        assert_ne!(Uses::EXTENSIONS, Uses::EXTENSION_SHEETS);
        let both = Uses::EXTENSIONS
            .with(Uses::EXTENSION_SHEETS)
            .with(Uses::CORE);
        assert_eq!(
            both.extension_bits(),
            Uses::EXTENSIONS.with(Uses::EXTENSION_SHEETS)
        );
        assert_eq!(both.without_extensions(), Uses::CORE);
    }
}
