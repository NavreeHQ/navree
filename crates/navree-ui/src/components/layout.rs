//! Layout primitives on the 4px grid. Gaps, columns and widths are a closed
//! set, written as attributes the stylesheet already has rules for.

use super::root_methods;
use crate::node::{Element, IntoNodes, Node, Uses, el, into_node};

/// A gap's attribute value: the nearest step of the scale (in units of 4px).
pub(crate) const fn gap_name(gap: u8) -> &'static str {
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
        Self {
            el: el("div").slot("stack").attr("data-gap", gap_name(gap)),
        }
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
        Self {
            el: el("div").slot("row").attr("data-gap", gap_name(gap)),
        }
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

    /// Lets items wrap onto more lines, but on a phone (under 640 px) keeps them on one
    /// line that scrolls sideways: for a row of actions above content that needs the
    /// room, such as a conversation's messages.
    pub fn scroll_on_phones(mut self) -> Self {
        self.el = self
            .el
            .flag("data-wrap", true)
            .flag("data-scroll-phone", true);
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

/// A grid of columns, one column on phones: equal ([`cols`](Self::cols)), as many as fit
/// ([`fit`](Self::fit)) or in a ratio ([`ratio`](Self::ratio)).
#[derive(Clone, Debug)]
#[must_use]
pub struct Grid {
    el: Element,
}

impl Grid {
    /// A grid of `columns` columns (1 to 6).
    pub fn cols(columns: u8) -> Self {
        Self {
            el: el("div")
                .slot("grid")
                .attr("data-cols", span_name(columns))
                .attr("data-gap", "4"),
        }
    }

    /// A grid of as many columns as fit, each at least `min` rem wide (8, 10, 12, 14, 16, 20
    /// or 24; another number takes the nearest): a row of figure tiles that wraps by the room
    /// it has, from one column on a phone to six on a wide screen, with no breakpoint to
    /// choose. The columns share the width equally.
    ///
    /// ```
    /// use navree_ui::{Grid, Stat, render};
    ///
    /// let tiles = Grid::fit(12).gap(3).children((
    ///     Stat::new("Steps", "9,412"),
    ///     Stat::new("Sleep", "7 h 20 min"),
    ///     Stat::new("Resting heart rate", "58 bpm"),
    /// ));
    /// assert!(render(&tiles.into()).contains(r#"data-fit="12""#));
    /// ```
    pub fn fit(min: u8) -> Self {
        let nearest = [8_u8, 10, 12, 14, 16, 20, 24]
            .into_iter()
            .min_by_key(|step| step.abs_diff(min))
            .unwrap_or(12);
        Self {
            el: el("div")
                .slot("grid")
                .attr("data-fit", nearest.to_string())
                .attr("data-gap", "4")
                .uses(Uses::APP_BLOCKS),
        }
    }

    /// A grid of unequal columns, their widths in the ratio of `parts`: `[7, 5]` for a wide
    /// column beside a narrower one, `[3, 2]`, `[1, 2, 1]`. Two to four columns, each part 1
    /// to 12 (others are taken to the nearest). One column under 768 px, in the order given.
    ///
    /// ```
    /// use navree_ui::{Grid, Text, render};
    ///
    /// let grid = Grid::ratio([7, 5]).children((Text::new("Trends"), Text::new("Notes")));
    /// let html = render(&grid.into());
    /// assert!(html.contains(r#"data-ratio="2" data-c1="7" data-c2="5""#));
    /// ```
    pub fn ratio(parts: impl IntoIterator<Item = u8>) -> Self {
        let parts: Vec<u8> = parts
            .into_iter()
            .take(4)
            .map(|part| part.clamp(1, 12))
            .collect();
        let mut el = el("div")
            .slot("grid")
            .attr("data-gap", "4")
            .uses(Uses::APP_BLOCKS);
        if parts.len() < 2 {
            return Self {
                el: el.attr("data-cols", "1"),
            };
        }
        el = el.attr("data-ratio", parts.len().to_string());
        for (at, part) in parts.iter().enumerate() {
            let name = ["data-c1", "data-c2", "data-c3", "data-c4"][at];
            el = el.attr(name, part.to_string());
        }
        Self { el }
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
        Self {
            el: el("div").slot("container"),
        }
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

/// Wraps `content` so it shows only in a browser that runs scripts: a control the
/// runtime drives, beside a stand-in for when it can't.
///
/// ```
/// use navree_ui::{only_with_script, render};
///
/// assert_eq!(render(&only_with_script("Drag")), r#"<span data-script-only>Drag</span>"#);
/// ```
pub fn only_with_script(content: impl IntoNodes) -> Node {
    el("span")
        .flag("data-script-only", true)
        .child(content)
        .into()
}

/// Wraps `content` so it shows only when scripts don't run: a link to a page that does
/// what a control does with a script.
///
/// ```
/// use navree_ui::{only_without_script, render};
///
/// assert_eq!(render(&only_without_script("Choose")), r#"<span data-noscript-only>Choose</span>"#);
/// ```
pub fn only_without_script(content: impl IntoNodes) -> Node {
    el("span")
        .flag("data-noscript-only", true)
        .child(content)
        .into()
}

#[cfg(test)]
mod ratio_tests {
    use super::*;
    use crate::render::render;

    #[test]
    fn a_ratio_grid_names_each_part_and_keeps_to_four() {
        let html = render(&Grid::ratio([7, 5]).into());
        assert!(
            html.starts_with(
                r#"<div data-slot="grid" data-gap="4" data-ratio="2" data-c1="7" data-c2="5">"#
            ),
            "{html}"
        );
        let html = render(&Grid::ratio([0, 40, 2, 1, 9]).into());
        assert!(
            html.contains(r#"data-ratio="4" data-c1="1" data-c2="12" data-c3="2" data-c4="1""#),
            "{html}"
        );
        assert!(!html.contains("data-c5"), "{html}");
        let one = render(&Grid::ratio([3]).into());
        assert!(
            one.contains(r#"data-cols="1""#) && !one.contains("data-ratio"),
            "{one}"
        );
        // Every part a grid can name has its rule, and the columns stack under 768 px.
        let css = include_str!("../../styles/extra/app-blocks.css");
        for column in 1..=4 {
            for part in 1..=12 {
                let rule = format!(r#"[data-c{column}="{part}"]{{--c{column}:{part}fr}}"#);
                assert!(css.contains(&rule), "{rule}");
            }
        }
        assert!(css.contains(r#"@media (min-width:768px){[data-slot=grid][data-ratio="2"]"#));
    }
}
