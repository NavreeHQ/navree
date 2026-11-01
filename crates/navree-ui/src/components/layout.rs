//! Layout primitives on the 4px grid. Gaps, columns and widths are a closed
//! set, written as attributes the stylesheet already has rules for.

use super::root_methods;
use crate::node::{Element, IntoNodes, Node, el, into_node};

/// A gap's attribute value: the nearest step of the scale (in units of 4px).
const fn gap_name(gap: u8) -> &'static str {
    match gap {
        0 => "0",
        1 => "1",
        2 => "2",
        3 => "3",
        4 => "4",
        5 => "5",
        6 => "6",
        7 | 8 => "8",
        9..=10 => "10",
        11..=12 => "12",
        _ => "16",
    }
}

pub(crate) const fn span_name(columns: u8) -> &'static str {
    match columns {
        0 | 1 => "1",
        2 => "2",
        3 => "3",
        4 => "4",
        5 => "5",
        _ => "6",
    }
}

/// Where items sit across a row or stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Align {
    /// At the start.
    Start,
    /// In the middle.
    Center,
    /// At the end.
    End,
    /// Filling the space.
    Stretch,
}

impl Align {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
            Self::Stretch => "stretch",
        }
    }
}

/// How leftover space is shared along a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Justify {
    /// At the start.
    Start,
    /// In the middle.
    Center,
    /// At the end.
    End,
    /// Between the items.
    Between,
}

impl Justify {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Center => "center",
            Self::End => "end",
            Self::Between => "between",
        }
    }
}

/// Items one above the other.
///
/// ```
/// use navree_ui::{Stack, render};
///
/// let html = render(&Stack::gap(4).children(("a", "b")).into());
/// assert_eq!(html, r#"<div data-slot="stack" data-gap="4">ab</div>"#);
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Stack {
    el: Element,
}

impl Stack {
    /// A stack with `gap` units of 4px between its items.
    pub fn gap(gap: u8) -> Self {
        Self { el: el("div").slot("stack").attr("data-gap", gap_name(gap)) }
    }

    /// Sets where items sit.
    pub fn align(mut self, align: Align) -> Self {
        self.el = self.el.attr("data-align", align.as_str());
        self
    }

    /// Adds items.
    pub fn children(mut self, children: impl IntoNodes) -> Self {
        self.el = self.el.child(children);
        self
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// Items side by side.
#[derive(Clone, Debug)]
#[must_use]
pub struct Row {
    el: Element,
}

impl Row {
    /// A row with `gap` units of 4px between its items, centred across.
    pub fn gap(gap: u8) -> Self {
        Self { el: el("div").slot("row").attr("data-gap", gap_name(gap)) }
    }

    /// Sets where items sit across the row.
    pub fn align(mut self, align: Align) -> Self {
        self.el = self.el.attr("data-align", align.as_str());
        self
    }

    /// Sets how leftover space is shared.
    pub fn justify(mut self, justify: Justify) -> Self {
        self.el = self.el.attr("data-justify", justify.as_str());
        self
    }

    /// Lets items wrap onto more lines.
    pub fn wrap(mut self) -> Self {
        self.el = self.el.flag("data-wrap", true);
        self
    }

    /// Adds items.
    pub fn children(mut self, children: impl IntoNodes) -> Self {
        self.el = self.el.child(children);
        self
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// A grid of equal columns, one column on phones.
#[derive(Clone, Debug)]
#[must_use]
pub struct Grid {
    el: Element,
}

impl Grid {
    /// A grid of `columns` columns (1 to 6).
    pub fn cols(columns: u8) -> Self {
        Self { el: el("div").slot("grid").attr("data-cols", span_name(columns)).attr("data-gap", "4") }
    }

    /// Sets the gap, in units of 4px.
    pub fn gap(mut self, gap: u8) -> Self {
        self.el.attrs.retain(|attr| attr.name != "data-gap");
        self.el = self.el.attr("data-gap", gap_name(gap));
        self
    }

    /// Adds items.
    pub fn children(mut self, children: impl IntoNodes) -> Self {
        self.el = self.el.child(children);
        self
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// A width for [`Container`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContainerSize {
    /// 40rem: forms and reading.
    Sm,
    /// 64rem.
    Md,
    /// 80rem: the default.
    #[default]
    Lg,
    /// The whole width.
    Full,
}

/// Content centred up to a maximum width.
#[derive(Clone, Debug)]
#[must_use]
pub struct Container {
    el: Element,
}

impl Container {
    /// A container of the default width.
    pub fn new() -> Self {
        Self { el: el("div").slot("container") }
    }

    /// Sets the maximum width.
    pub fn size(mut self, size: ContainerSize) -> Self {
        let name = match size {
            ContainerSize::Sm => "sm",
            ContainerSize::Md => "md",
            ContainerSize::Lg => "lg",
            ContainerSize::Full => "full",
        };
        self.el = self.el.attr("data-size", name);
        self
    }

    /// Adds items.
    pub fn children(mut self, children: impl IntoNodes) -> Self {
        self.el = self.el.child(children);
        self
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

root_methods!(Stack, Row, Grid, Container);
into_node!(Stack, Row, Grid, Container);
