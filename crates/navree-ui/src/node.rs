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
//! Attribute names are `&'static str`, so a request can't choose one. `style`
//! and `on*` are refused (the content security policy would block them
//! anyway), and URL attributes go through [`Href`].

use std::borrow::Cow;

use crate::cmd::{Cmd, Commands};
use crate::html::{Href, SafeHtml};

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

impl Node {
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
                out.push_str(&text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&"));
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
pub struct Uses(pub(crate) u8);

impl Uses {
    /// Nothing: the page ships no script.
    pub const NONE: Self = Self(0);
    /// The core runtime: commands, ops, morph, link boosting, pending states.
    pub const CORE: Self = Self(1);
    /// The sidebar module: collapse, `Ctrl/Cmd+B`, the phone sheet.
    pub const SIDEBAR: Self = Self(1 << 1);
    /// The overlay module: tooltip and hover card, placement where CSS can't.
    pub const OVERLAY: Self = Self(1 << 2);

    /// Both sets together.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every bit of `other` is here.
    pub const fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether this is [`Uses::NONE`].
    pub const fn is_none(self) -> bool {
        self.0 == 0
    }
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

/// An element named `tag`.
#[inline]
pub fn el(tag: &'static str) -> Element {
    Element {
        tag,
        attrs: Vec::new(),
        children: Vec::new(),
        uses: Uses::NONE,
    }
}

/// Attributes that hold URLs: their values go through [`Href`].
fn is_url_attr(name: &str) -> bool {
    matches!(name, "href" | "src" | "action" | "formaction" | "poster" | "cite" | "ping")
}

/// Attributes that are never written: inline style and handlers break the
/// content security policy, and `srcdoc` is a document in an attribute.
fn is_refused_attr(name: &str) -> bool {
    name == "style" || name == "srcdoc" || name.starts_with("on")
}

impl Element {
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
        self.attrs.iter().find(|attr| attr.name == name).map(|attr| &attr.value)
    }

    /// Sets attribute `name`. A URL attribute is checked like an [`Href`];
    /// `style`, `srcdoc` and `on*` are dropped.
    #[inline]
    #[must_use]
    pub fn attr(mut self, name: &'static str, value: impl Into<AttrValue>) -> Self {
        if is_refused_attr(name) {
            debug_assert!(false, "`{name}` is not allowed under the content security policy");
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
            self.attrs.push(Attr { name, value: AttrValue::Flag });
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
        self.attrs.push(Attr { name: "href", value: href.into().into() });
        self
    }

    /// Runs `commands` in the browser when this element is clicked, with no
    /// request to the server.
    #[must_use]
    pub fn on_click(self, commands: impl Into<Commands>) -> Self {
        self.on(crate::cmd::Trigger::Click, commands)
    }

    /// Runs `commands` in the browser when `trigger` happens on this element.
    #[must_use]
    pub fn on(mut self, trigger: crate::cmd::Trigger, commands: impl Into<Commands>) -> Self {
        self.uses = self.uses.with(Uses::CORE);
        let commands: Commands = commands.into();
        self.attrs.push(Attr {
            name: trigger.attribute(),
            value: AttrValue::Text(commands.to_json().into_boxed_str()),
        });
        self
    }

    /// Runs one command when clicked.
    #[must_use]
    pub fn on_click_one(self, command: Cmd) -> Self {
        self.on_click(vec![command])
    }

    /// Says this page must load `uses` when this element is on it.
    #[inline]
    #[must_use]
    pub fn uses(mut self, uses: Uses) -> Self {
        self.uses = self.uses.with(uses);
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

/// Implements `From<$type> for Node` by calling the type's `into_node`.
macro_rules! into_node {
    ($($type:ty),+ $(,)?) => {
        $(impl From<$type> for $crate::node::Node {
            #[inline]
            fn from(value: $type) -> Self {
                value.into_node()
            }
        })+
    };
}
pub(crate) use into_node;

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
        let element = el("div").children(("a", Some("b"), None::<&'static str>, vec!["c", "d"], 7_i64));
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
    fn a_node_stays_small() {
        // Children hold nodes inline, so one size change touches every page.
        assert!(std::mem::size_of::<Node>() <= 72, "{}", std::mem::size_of::<Node>());
    }
}
