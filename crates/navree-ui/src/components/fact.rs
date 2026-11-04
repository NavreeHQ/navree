//! [`FactCard`] and [`FactGrid`]: a label, a value and the details under it.

use super::tone::Tone;
use super::{icon, root_methods};
use crate::icons::Icon;
use crate::node::{Element, Node, el, into_node};

/// One fact about something: a small label, a value and the details under it.
///
/// ```
/// use navree_ui::{FactCard, Tone, icons, render};
///
/// let card = FactCard::new("Device", "Delivery handset")
///     .icon(icons::MONITOR)
///     .tone(Tone::Info)
///     .detail("Chrome 130 on Android 15");
/// let html = render(&card.into());
/// assert!(html.contains("<dt"));
/// assert!(html.contains("Chrome 130 on Android 15"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct FactCard {
    el: Element,
    label: Node,
    value: Node,
    details: Vec<Node>,
    glyph: Option<Icon>,
    tone: Tone,
}

impl FactCard {
    /// The fact `value`, called `label`.
    pub fn new(label: impl Into<Node>, value: impl Into<Node>) -> Self {
        Self {
            el: el("div").slot("fact-card"),
            label: label.into(),
            value: value.into(),
            details: Vec::new(),
            glyph: None,
            tone: Tone::Neutral,
        }
    }

    /// Adds an icon beside the label.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.glyph = Some(glyph);
        self
    }

    /// Colours the icon.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// Adds a line under the value. Call it again for more lines.
    pub fn detail(mut self, detail: impl Into<Node>) -> Self {
        self.details.push(detail.into());
        self
    }

    fn into_node(self) -> Node {
        self.el
            .attr_opt("data-tone", self.tone.value())
            .flag("data-icon", self.glyph.is_some())
            .children((
                self.glyph.map(|glyph| {
                    el("span")
                        .slot("fact-icon")
                        .attr("aria-hidden", "true")
                        .child(icon(glyph))
                }),
                el("dl").slot("fact-body").child(
                    el("div").children((
                        el("dt").slot("fact-label").child(self.label),
                        el("dd")
                            .slot("fact-value")
                            .child(el("bdi").child(self.value)),
                        self.details
                            .into_iter()
                            .map(|detail| {
                                el("dd").slot("fact-detail").child(el("bdi").child(detail))
                            })
                            .collect::<Vec<_>>(),
                    )),
                ),
            ))
            .into()
    }
}

/// Cards side by side: as many columns as fit, one on a phone.
///
/// ```
/// use navree_ui::{FactCard, FactGrid, render};
///
/// let grid = FactGrid::new()
///     .card(FactCard::new("Opens", "3"))
///     .card(FactCard::new("Clicks", "1"));
/// assert!(render(&grid.into()).contains("fact-grid"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct FactGrid {
    el: Element,
    cards: Vec<Node>,
}

impl FactGrid {
    /// An empty grid.
    pub fn new() -> Self {
        Self {
            el: el("div").slot("fact-grid"),
            cards: Vec::new(),
        }
    }

    /// Adds a card.
    pub fn card(mut self, card: FactCard) -> Self {
        self.cards.push(card.into());
        self
    }

    /// Adds cards.
    pub fn cards(mut self, cards: impl IntoIterator<Item = FactCard>) -> Self {
        self.cards.extend(cards.into_iter().map(Node::from));
        self
    }

    /// Fixes the number of columns (2 to 6) from 768 px wide; narrower, it is
    /// one. Without this the grid fits as many as the width allows.
    pub fn cols(mut self, columns: u8) -> Self {
        self.el = self
            .el
            .attr("data-cols", super::layout::span_name(columns.clamp(2, 6)));
        self
    }

    fn into_node(self) -> Node {
        self.el.child(self.cards).into()
    }
}

impl Default for FactGrid {
    fn default() -> Self {
        Self::new()
    }
}

root_methods!(FactCard, FactGrid);
into_node!(FactCard, FactGrid);
