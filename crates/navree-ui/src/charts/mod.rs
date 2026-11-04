//! Charts drawn on the server as SVG: [`Chart`] (line, area and bar, vertical and
//! horizontal, grouped and stacked), [`Donut`], [`Sparkline`] and [`Funnel`], and
//! [`Report`], which reads a date range from the URL, picks a grain and buckets a
//! query by it.
//!
//! ```
//! use navree_ui::charts::*;
//! use navree_ui::render;
//!
//! let chart = Chart::line("Sign-ups")
//!     .categories(["Mon", "Tue", "Wed", "Thu", "Fri"])
//!     .series(Series::new("Trial", [12, 18, 9, 22, 30]))
//!     .series(Series::new("Paid", [3, 4, 2, 6, 8]));
//! let html = render(&chart.into());
//! assert!(html.contains("<svg"));
//! assert!(html.contains("<table"), "every chart carries its data as a table");
//! ```
//!
//! **What a chart is made of.** A `<figure>` with the title, a legend for two series
//! or more, the drawing, a text summary, and the same numbers as a table: hidden from
//! sight, read by screen readers, shown by its switch. The drawing keeps its text at
//! its size at any width: values sit on a pixel axis (a chart has a height) and the
//! other axis is in percent of the width. There is no `style` attribute anywhere, so
//! the page's content security policy holds.
//!
//! **What loads.** Nothing, for a page to read. The `charts` module adds tooltips,
//! a crosshair, and keys: a chart is one tab stop, the arrow keys move between its
//! values, Home and End go to the ends, Escape hides the tooltip. Legend toggles and
//! the table switch are checkboxes, and work with JavaScript off.
//!
//! **Data.** Every series shares the chart's x values, like the columns of a table.
//! NaN, infinities and `None` are gaps: a line breaks there and a bar is left out.
//! A line with more points than `max_points` (640) keeps its shape with
//! largest-triangle-three-buckets; bars past `max_bars` merge their neighbours, or
//! fold the tail into Other. A ninth series folds into Other.

use std::borrow::Cow;

use crate::components::Tone;
use crate::icons;
use crate::node::{Element, IntoNodes, Node, Uses, el};

mod cartesian;
mod donut;
mod downsample;
mod format;
mod funnel;
mod report;
#[cfg(feature = "orm")]
mod report_orm;
mod scale;
mod sparkline;
mod value;

pub use cartesian::{Annotation, At, Chart, Merge, Series};
pub use donut::Donut;
pub use downsample::lttb;
pub use format::{Compact, CompactStep, Grouping, NumberFormat, NumberLocale};
pub use funnel::{Funnel, MAX_STAGES, Stage};
pub use report::{Bucket, MAX_BUCKETS, Report, ReportQuery, ReportZone};
#[cfg(feature = "orm")]
pub use report_orm::{Bucketing, Metric, TimeColumn};
pub use scale::Grain;
pub use sparkline::Sparkline;
pub use value::IntoValue;

/// The most categorical colours: a ninth series folds into Other.
pub const MAX_SERIES: usize = 8;

/// The words a chart writes, in English by default. Templates name their blanks in
/// braces: `{title}`, `{series}`, `{value}`.
#[derive(Clone, Debug)]
pub struct ChartLabels {
    /// The switch that shows the table.
    pub show_table: Cow<'static, str>,
    /// The drawing's name for assistive technology: `{title}, chart`.
    pub chart: Cow<'static, str>,
    /// How to move through the values with keys.
    pub keys: Cow<'static, str>,
    /// While loading: `Loading {title}`.
    pub loading: Cow<'static, str>,
    /// A failure's heading.
    pub failed: Cow<'static, str>,
    /// The link that asks again.
    pub retry: Cow<'static, str>,
    /// Nothing to draw.
    pub empty: Cow<'static, str>,
    /// Under the empty heading.
    pub empty_text: Cow<'static, str>,
    /// The series or the part that holds the rest.
    pub other: Cow<'static, str>,
    /// A column of totals.
    pub total: Cow<'static, str>,
    /// A column of shares.
    pub share: Cow<'static, str>,
    /// The first column of a table over categories.
    pub category: Cow<'static, str>,
    /// The first column of a table over time.
    pub time: Cow<'static, str>,
    /// A value column with no other name.
    pub value: Cow<'static, str>,
    /// A cell with no value, for a screen reader.
    pub no_value: Cow<'static, str>,
    /// A funnel's column: the share of the first stage.
    pub of_first: Cow<'static, str>,
    /// A funnel's column: the share of the stage before.
    pub of_previous: Cow<'static, str>,
    /// Under a table whose drawing has fewer points than the data: `{shown}`, `{total}`.
    pub reduced: Cow<'static, str>,
    /// One series over x: `{series}`, `{first}`, `{first_x}`, `{last}`, `{last_x}`,
    /// `{max}`, `{max_x}`, `{min}`, `{min_x}`.
    pub summary_series: Cow<'static, str>,
    /// A series with one value: `{series}`, `{value}`, `{x}`.
    pub summary_one: Cow<'static, str>,
    /// Bars: `{series}`, `{max}`, `{max_x}`, `{min}`, `{min_x}`, `{total}`.
    pub summary_bars: Cow<'static, str>,
    /// Parts of a whole: `{count}`, `{total}`, `{name}`, `{share}`.
    pub summary_parts: Cow<'static, str>,
    /// A funnel: `{first_name}`, `{first}`, `{last_name}`, `{last}`, `{rate}`.
    pub summary_funnel: Cow<'static, str>,
    /// A series with no value.
    pub summary_none: Cow<'static, str>,
    /// The targets, events and bands on a chart: `{marks}`.
    pub summary_marks: Cow<'static, str>,
    /// Beside a line with one value: a line needs two.
    pub one_point: Cow<'static, str>,
    /// A week's name: `Week of {date}`.
    pub week_of: Cow<'static, str>,
    /// An hour the clocks showed twice, the second time: `{time}, second time`.
    pub again: Cow<'static, str>,
    /// Under a funnel with more stages than it draws: `{shown}`, `{total}`.
    pub stages_cut: Cow<'static, str>,
}

impl Default for ChartLabels {
    fn default() -> Self {
        Self::english()
    }
}

impl ChartLabels {
    /// The English words.
    pub const fn english() -> Self {
        Self {
            show_table: Cow::Borrowed("Show table"),
            chart: Cow::Borrowed("{title}, chart"),
            keys: Cow::Borrowed("Use the arrow keys to move between values."),
            loading: Cow::Borrowed("Loading {title}"),
            failed: Cow::Borrowed("This chart could not be loaded"),
            retry: Cow::Borrowed("Try again"),
            empty: Cow::Borrowed("No data"),
            empty_text: Cow::Borrowed("Nothing to show for this period."),
            other: Cow::Borrowed("Other"),
            total: Cow::Borrowed("Total"),
            share: Cow::Borrowed("Share"),
            category: Cow::Borrowed("Category"),
            time: Cow::Borrowed("Time"),
            value: Cow::Borrowed("Value"),
            no_value: Cow::Borrowed("No value"),
            of_first: Cow::Borrowed("Of first"),
            of_previous: Cow::Borrowed("Of previous"),
            reduced: Cow::Borrowed(
                "The drawing shows {shown} of {total} points; the table lists the points drawn.",
            ),
            summary_series: Cow::Borrowed(
                "{series}: {first} at {first_x}, {last} at {last_x}. Highest {max} at {max_x}, lowest {min} at {min_x}.",
            ),
            summary_one: Cow::Borrowed("{series}: {value} at {x}."),
            summary_bars: Cow::Borrowed(
                "{series}: highest {max} for {max_x}, lowest {min} for {min_x}, total {total}.",
            ),
            summary_parts: Cow::Borrowed("{count} parts, total {total}. Largest: {name}, {share}."),
            summary_funnel: Cow::Borrowed(
                "From {first_name}, {first}, to {last_name}, {last}: {rate} of the first stage.",
            ),
            summary_none: Cow::Borrowed("{series}: no values."),
            summary_marks: Cow::Borrowed("Marked: {marks}."),
            one_point: Cow::Borrowed("A line needs two values; one is shown."),
            week_of: Cow::Borrowed("Week of {date}"),
            again: Cow::Borrowed("{time}, second time"),
            stages_cut: Cow::Borrowed("The first {shown} of {total} stages are drawn."),
        }
    }
}

/// `template` with each `{name}` replaced. A blank with no value stays as written.
pub(crate) fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                match values.iter().find(|(key, _)| *key == name) {
                    Some((_, value)) => out.push_str(value),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Where the table of a chart sits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TableMode {
    /// Hidden from sight, read by screen readers, shown by a switch: the default.
    #[default]
    Switch,
    /// Shown under the drawing.
    Shown,
    /// Read by screen readers only, with no switch: for a sparkline in a row.
    ScreenReader,
}

/// What a chart shows instead of its drawing.
#[derive(Clone, Debug, Default)]
pub(crate) enum State {
    #[default]
    Ready,
    Loading,
    Failed {
        reason: Option<String>,
        retry: Option<crate::html::Href>,
    },
}

/// What every chart has: its name, its words and its states.
#[derive(Clone, Debug)]
pub(crate) struct Common {
    pub id: Option<String>,
    pub title: String,
    pub heading: Option<u8>,
    pub hide_title: bool,
    pub description: Option<Node>,
    pub summary: Option<String>,
    pub show_summary: bool,
    pub labels: ChartLabels,
    pub numbers: NumberLocale,
    pub dates: crate::dates::DateLocale,
    pub table: TableMode,
    pub state: State,
    pub empty: Option<(String, Option<String>)>,
    pub action: Option<Node>,
    pub attrs: Vec<(&'static str, crate::node::AttrValue)>,
}

impl Common {
    pub fn new(title: String) -> Self {
        Self {
            id: None,
            title,
            heading: None,
            hide_title: false,
            description: None,
            summary: None,
            show_summary: false,
            labels: ChartLabels::english(),
            numbers: NumberLocale::english(),
            dates: crate::dates::DateLocale::english(),
            table: TableMode::Switch,
            state: State::Ready,
            empty: None,
            action: None,
            attrs: Vec::new(),
        }
    }

    /// The figure's id: the one given, kept to letters, digits, `-` and `_` of any
    /// script; or one made from the title, its words joined by `-` and a short hash of
    /// the whole title after them, so titles in any script, and titles that differ only
    /// in what an id can't hold, give different ids.
    pub fn id(&self) -> String {
        let given = self.id.is_some();
        let source = self.id.as_deref().unwrap_or(&self.title);
        let mut out = String::with_capacity(source.len().min(96) + 16);
        let mut dash = false;
        for (count, c) in source.chars().enumerate() {
            if count >= 48 {
                break;
            }
            if c.is_alphanumeric() || (given && c == '_') {
                // A given id keeps its case: the app may style or link to it.
                if given {
                    out.push(c);
                } else {
                    out.extend(c.to_lowercase());
                }
                dash = false;
            } else if !dash && !out.is_empty() {
                out.push('-');
                dash = true;
            }
        }
        while out.ends_with('-') {
            out.pop();
        }
        if given {
            if !out.starts_with(char::is_alphabetic) {
                out.insert_str(0, "chart-");
            }
            return out;
        }
        // FNV-1a over the whole title: six hex digits tell titles apart.
        let mut hash: u32 = 0x811c_9dc5;
        for byte in self.title.bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(0x0100_0193);
        }
        if out.is_empty() {
            format!("chart-{:06x}", hash & 0x00ff_ffff)
        } else {
            format!("chart-{out}-{:06x}", hash & 0x00ff_ffff)
        }
    }
}

/// Adds the builder methods every chart shares to a type with a `common` field.
macro_rules! common_methods {
    ($($type:ident),+ $(,)?) => {
        $(impl $type {
            /// Sets the figure's id, which the table switch and the names inside it
            /// build on. Give one when two charts on a page share a title.
            pub fn id(mut self, id: impl Into<String>) -> Self {
                self.common.id = Some(id.into());
                self
            }

            /// Writes the title as a heading of `level` (2 to 6) rather than a caption.
            pub fn heading(mut self, level: u8) -> Self {
                self.common.heading = Some(level.clamp(2, 6));
                self
            }

            /// Keeps the title for assistive technology and the table's caption, but
            /// shows none: for a chart in a card that has the title already.
            pub fn hide_title(mut self) -> Self {
                self.common.hide_title = true;
                self
            }

            /// A line under the title.
            pub fn description(mut self, text: impl Into<$crate::node::Node>) -> Self {
                self.common.description = Some(text.into());
                self
            }

            /// Replaces the summary written from the data.
            pub fn summary(mut self, text: impl Into<String>) -> Self {
                self.common.summary = Some(text.into());
                self
            }

            /// Shows the summary under the drawing, as well as giving it to assistive
            /// technology.
            pub fn show_summary(mut self) -> Self {
                self.common.show_summary = true;
                self
            }

            /// The words the chart writes.
            pub fn labels(mut self, labels: $crate::charts::ChartLabels) -> Self {
                self.common.labels = labels;
                self
            }

            /// Writes numbers and dates for a language: `en-IN`, `de`, `fr`, `ar`.
            pub fn locale(mut self, tag: &str) -> Self {
                self.common.numbers = $crate::charts::NumberLocale::of(tag);
                self.common.dates = $crate::dates::DateLocale::of(tag);
                self
            }

            /// Writes numbers with these marks.
            pub fn number_locale(mut self, locale: $crate::charts::NumberLocale) -> Self {
                self.common.numbers = locale;
                self
            }

            /// Writes dates with this locale.
            pub fn date_locale(mut self, locale: $crate::dates::DateLocale) -> Self {
                self.common.dates = locale;
                self
            }

            /// Where the table sits: behind a switch (the default), shown, or for
            /// screen readers only.
            pub fn table(mut self, mode: $crate::charts::TableMode) -> Self {
                self.common.table = mode;
                self
            }

            /// Shows a placeholder the height of the chart while its data loads.
            pub fn loading(mut self) -> Self {
                self.common.state = $crate::charts::State::Loading;
                self
            }

            /// Shows that the data could not be loaded, never zeros: `reason` says
            /// why when given.
            pub fn failed(mut self, reason: Option<String>) -> Self {
                let retry = match &self.common.state {
                    $crate::charts::State::Failed { retry, .. } => retry.clone(),
                    _ => None,
                };
                self.common.state = $crate::charts::State::Failed { reason, retry };
                self
            }

            /// Adds a link that asks again to a failed chart.
            pub fn retry(mut self, href: impl Into<$crate::html::Href>) -> Self {
                let reason = match &self.common.state {
                    $crate::charts::State::Failed { reason, .. } => reason.clone(),
                    _ => None,
                };
                self.common.state = $crate::charts::State::Failed { reason, retry: Some(href.into()) };
                self
            }

            /// What an empty chart says: a heading and a line under it.
            pub fn empty(mut self, title: impl Into<String>, text: Option<String>) -> Self {
                self.common.empty = Some((title.into(), text));
                self
            }

            /// Something beside the title: a link to the full report, a menu.
            pub fn action(mut self, action: impl Into<$crate::node::Node>) -> Self {
                self.common.action = Some(action.into());
                self
            }

            /// Sets an attribute on the figure.
            pub fn attr(mut self, name: &'static str, value: impl Into<$crate::node::AttrValue>) -> Self {
                self.common.attrs.push((name, value.into()));
                self
            }
        })+
    };
}
pub(crate) use common_methods;

/// A colour a series or a part wears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Color {
    /// A categorical slot, 1 to 8.
    Slot(u8),
    /// A status or named tone.
    Tone(Tone),
    /// The rest, in the quiet ink.
    Other,
}

impl Color {
    /// Sets the attribute the stylesheet colours by.
    pub fn on(self, element: Element) -> Element {
        match self {
            Self::Slot(slot) => element.attr("data-color", i64::from(slot.clamp(1, 8))),
            Self::Tone(tone) => match tone.value() {
                Some(name) => element.attr("data-tone", name),
                None => element.attr("data-color", "other"),
            },
            Self::Other => element.attr("data-color", "other"),
        }
    }

    /// The icon a status tone shows in a legend, so colour is never alone.
    pub fn icon(self) -> Option<icons::Icon> {
        match self {
            Self::Tone(Tone::Success) => Some(icons::CIRCLE_CHECK),
            Self::Tone(Tone::Warning) => Some(icons::TRIANGLE_ALERT),
            Self::Tone(Tone::Danger) => Some(icons::CIRCLE_ALERT),
            Self::Tone(Tone::Info) => Some(icons::INFO),
            _ => None,
        }
    }
}

/// The shape of a legend's key: it mirrors the mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Line,
    Block,
}

/// One entry of a legend.
pub(crate) struct LegendItem {
    pub name: String,
    pub color: Color,
    pub key: Key,
}

/// A legend: a list, or with `toggles` a checkbox per series that hides it.
pub(crate) fn legend(id: &str, items: &[LegendItem], toggles: bool) -> Node {
    let entries: Vec<Node> = items
        .iter()
        .enumerate()
        .map(|(at, item)| {
            let key = item
                .color
                .on(el("span").slot("chart-key").attr(
                    "data-key",
                    match item.key {
                        Key::Line => "line",
                        Key::Block => "block",
                    },
                ))
                .attr("aria-hidden", "true");
            let icon = item.color.icon().map(crate::components::icon);
            let name = el("span")
                .slot("chart-legend-name")
                .child(item.name.clone());
            if toggles {
                let input_id = format!("{id}-s{at}");
                el("li")
                    .slot("chart-legend-item")
                    .children((
                        el("input")
                            .attr("type", "checkbox")
                            .slot("chart-toggle")
                            .id(input_id.clone())
                            .attr("data-s", at as i64)
                            .flag("checked", true),
                        el("label")
                            .attr("for", input_id)
                            .children((key, icon, name)),
                    ))
                    .into()
            } else {
                el("li")
                    .slot("chart-legend-item")
                    .children((key, icon, name))
                    .into()
            }
        })
        .collect();
    el("ul")
        .slot("chart-legend")
        .attr_opt("data-toggles", toggles.then_some("true"))
        .child(entries)
        .into()
}

/// A cell's text for a value, or the no-value words for a screen reader.
pub(crate) fn value_cell(text: Option<String>, labels: &ChartLabels) -> Node {
    match text {
        Some(text) => Node::from(text),
        None => Node::list((
            el("span").attr("aria-hidden", "true").child("–"),
            el("span")
                .attr("data-sr", "")
                .child(labels.no_value.clone()),
        )),
    }
}

/// The table twin: `head` names the columns (the first names the rows), each row its
/// header and cells. Header cells carry the series colour so a tooltip keys its rows.
pub(crate) struct TableData {
    pub head: Vec<(String, Option<Color>)>,
    pub rows: Vec<(String, Vec<Node>)>,
    pub note: Option<String>,
}

fn table_node(caption: &str, data: TableData) -> Node {
    let head: Vec<Node> = data
        .head
        .into_iter()
        .enumerate()
        .map(|(at, (name, color))| {
            let th = el("th").attr("scope", "col").child(name);
            let th = if at > 0 { th.attr("data-num", "") } else { th };
            match color {
                Some(color) => color.on(th).into(),
                None => th.into(),
            }
        })
        .collect();
    let rows: Vec<Node> = data
        .rows
        .into_iter()
        .map(|(name, cells)| {
            el("tr")
                .children((
                    el("th").attr("scope", "row").child(name),
                    cells
                        .into_iter()
                        .map(|cell| el("td").attr("data-num", "").child(cell))
                        .collect::<Vec<_>>(),
                ))
                .into()
        })
        .collect();
    Node::list((
        el("table").children((
            el("caption").child(caption.to_owned()),
            el("thead").child(el("tr").child(head)),
            el("tbody").child(rows),
        )),
        data.note.map(|note| el("p").slot("chart-note").child(note)),
    ))
}

/// What a chart hands the frame.
pub(crate) struct Drawn {
    /// The legend, if any.
    pub legend: Option<Node>,
    /// The drawing, inside the group the module makes one tab stop.
    pub body: Node,
    /// Attributes on the group: `data-xs` and the like.
    pub body_attrs: Vec<(&'static str, crate::node::AttrValue)>,
    /// The summary written from the data.
    pub summary: String,
    /// The table.
    pub table: TableData,
    /// A height to keep for the states, in pixels.
    pub height: u32,
    /// Something to say under the drawing, such as that a line needs two values.
    pub note: Option<String>,
    /// Whether the module makes it a tab stop with tooltips. A sparkline in a row of
    /// figures is not: it is a picture with its summary as its name.
    pub interactive: bool,
}

/// The figure every chart is drawn in. `drawn` is `None` when there is nothing to
/// draw: the chart is empty.
pub(crate) fn frame(common: Common, drawn: Option<Drawn>, kind: &'static str, height: u32) -> Node {
    let id = common.id();
    let labels = &common.labels;
    let title_id = format!("{id}-title");
    let summary_id = format!("{id}-summary");
    let keys_id = format!("{id}-keys");
    let title = match common.heading {
        Some(level) => el(match level {
            2 => "h2",
            3 => "h3",
            4 => "h4",
            5 => "h5",
            _ => "h6",
        }),
        None => el("span"),
    }
    .slot("chart-title")
    .id(title_id.clone())
    .child(common.title.clone());
    let header = el("div")
        .slot("chart-header")
        .attr_opt("data-sr", common.hide_title.then_some(""))
        .children((
            el("div").slot("chart-heading").children((
                title,
                common
                    .description
                    .map(|d| el("span").slot("chart-description").child(d)),
            )),
            common
                .action
                .map(|a| el("div").slot("chart-action").child(a)),
        ));
    let mut figure = el("figure")
        .slot("chart")
        .id(id.clone())
        .attr("data-kind", kind)
        .attr("aria-labelledby", title_id);
    for (name, value) in common.attrs {
        figure = figure.attr(name, value);
    }
    let named = fill(&labels.chart, &[("title", &common.title)]);
    let content: Node = match (&common.state, drawn) {
        (State::Loading, _) => {
            figure = figure.attr("aria-busy", "true");
            Node::list((
                header,
                el("div")
                    .slot("chart-state")
                    .attr("data-state", "loading")
                    .attr("data-h", i64::from(height))
                    .attr("role", "status")
                    .children((
                        spacer(height),
                        el("div").slot("skeleton").attr("data-shape", "block"),
                        el("span")
                            .attr("data-sr", "")
                            .child(fill(&labels.loading, &[("title", &common.title)])),
                    )),
            ))
        }
        (State::Failed { reason, retry }, _) => Node::list((
            header,
            el("div")
                .slot("chart-state")
                .attr("data-state", "failed")
                .attr("data-h", i64::from(height))
                .attr("role", "alert")
                .children((
                    spacer(height),
                    el("div").slot("chart-state-body").children((
                        crate::components::icon(icons::CIRCLE_ALERT),
                        el("strong").child(labels.failed.clone()),
                        reason.clone().map(|r| el("span").child(r)),
                        retry.clone().map(|href| {
                            el("a")
                                .href(href)
                                .slot("button")
                                .attr("data-variant", "outline")
                                .attr("data-size", "sm")
                                .child(labels.retry.clone())
                        }),
                    )),
                )),
        )),
        (State::Ready, None) => {
            let (heading, text) = common.empty.clone().unwrap_or_else(|| {
                (
                    labels.empty.to_string(),
                    Some(labels.empty_text.to_string()),
                )
            });
            Node::list((
                header,
                el("div")
                    .slot("chart-state")
                    .attr("data-state", "empty")
                    .attr("data-h", i64::from(height))
                    .children((
                        spacer(height),
                        el("div").slot("chart-state-body").children((
                            crate::components::icon(icons::CHART_NO_AXES_COLUMN),
                            el("strong").child(heading),
                            text.map(|t| el("span").child(t)),
                        )),
                    )),
            ))
        }
        (State::Ready, Some(drawn)) => {
            let summary = common.summary.clone().unwrap_or(drawn.summary);
            let mut body = el("div").slot("chart-body");
            if drawn.interactive {
                figure = figure.flag("data-ui-chart", true).uses(Uses::CHARTS);
                body = body
                    .attr("role", "group")
                    .attr("aria-label", named)
                    .attr("aria-describedby", summary_id.clone())
                    .attr("data-keys", keys_id.clone());
            } else {
                body = body
                    .attr("role", "img")
                    .attr("aria-label", named)
                    .attr("aria-describedby", summary_id.clone());
            }
            for (name, value) in drawn.body_attrs {
                body = body.attr(name, value);
            }
            let switch_id = format!("{id}-table-switch");
            let table = table_node(&common.title, drawn.table);
            let data = match common.table {
                TableMode::Switch => el("div").slot("chart-data").children((
                    el("input")
                        .attr("type", "checkbox")
                        .slot("chart-table-switch")
                        .id(switch_id.clone()),
                    el("label")
                        .slot("chart-table-label")
                        .attr("for", switch_id)
                        .children((
                            crate::components::icon(icons::TABLE_2),
                            labels.show_table.clone(),
                        )),
                    el("div").slot("chart-table").child(table),
                )),
                TableMode::Shown => el("div").slot("chart-data").child(
                    el("div")
                        .slot("chart-table")
                        .attr("data-shown", "")
                        .child(table),
                ),
                TableMode::ScreenReader => el("div")
                    .slot("chart-data")
                    .child(el("div").slot("chart-table").child(table)),
            };
            Node::list((
                header,
                drawn.legend,
                body.children((
                    drawn.body,
                    drawn
                        .interactive
                        .then(|| el("div").slot("chart-tip").attr("aria-hidden", "true")),
                )),
                drawn.note.map(|n| el("p").slot("chart-note").child(n)),
                el("p")
                    .slot("chart-summary")
                    .id(summary_id)
                    .attr_opt("data-sr", (!common.show_summary).then_some(""))
                    .child(summary),
                drawn.interactive.then(|| {
                    Node::list((
                        el("span")
                            .id(keys_id)
                            .flag("hidden", true)
                            .child(labels.keys.clone()),
                        el("div")
                            .slot("chart-announce")
                            .attr("data-sr", "")
                            .attr("aria-live", "polite"),
                    ))
                }),
                data,
            ))
        }
    };
    figure.child(content).into()
}

/// An empty picture as tall as the chart would be: a state takes the drawing's place
/// to the pixel, so the page doesn't move when the data comes, and no style attribute
/// is needed to say how tall.
fn spacer(height: u32) -> Element {
    svg()
        .slot("chart-spacer")
        .attr("width", "0")
        .attr("height", i64::from(height.clamp(16, 4000)))
}

/// Writes a coordinate with at most `places` decimals and no trailing zeros.
pub(crate) fn num(out: &mut String, value: f64, places: usize) {
    use std::fmt::Write as _;
    let value = if value.is_finite() { value } else { 0.0 };
    let start = out.len();
    let _ = write!(out, "{value:.places$}");
    if places > 0 && out[start..].contains('.') {
        while out.ends_with('0') {
            out.pop();
        }
        if out.ends_with('.') {
            out.pop();
        }
    }
    if &out[start..] == "-0" {
        out.truncate(start);
        out.push('0');
    }
}

/// A coordinate as text.
pub(crate) fn n(value: f64, places: usize) -> String {
    let mut out = String::with_capacity(8);
    num(&mut out, value, places);
    out
}

/// A percentage of the width: `12.5%`.
pub(crate) fn pct(value: f64) -> String {
    let mut out = n(value.clamp(-1000.0, 1000.0), 3);
    out.push('%');
    out
}

/// An empty SVG element with no text for assistive technology.
pub(crate) fn svg() -> Element {
    el("svg")
        .attr("aria-hidden", "true")
        .attr("focusable", "false")
}

/// Nodes from a list.
pub(crate) fn nodes(items: impl IntoNodes) -> Vec<Node> {
    let mut out = Vec::new();
    items.push_into(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_fill_their_blanks_and_keep_unknown_ones() {
        assert_eq!(fill("{a} and {b}", &[("a", "1"), ("b", "2")]), "1 and 2");
        assert_eq!(fill("{a} {zzz} {", &[("a", "x")]), "x {zzz} {");
        assert_eq!(fill("no blanks", &[]), "no blanks");
        assert_eq!(
            fill("{a}", &[("a", "{a}")]),
            "{a}",
            "a value is never filled again"
        );
    }

    #[test]
    fn ids_are_safe() {
        let id = |title: &str, id: Option<&str>| {
            let mut c = Common::new(title.to_owned());
            c.id = id.map(str::to_owned);
            c.id()
        };
        assert!(id("Opens by hour", None).starts_with("chart-opens-by-hour-"));
        assert!(id("\"><script>", None).starts_with("chart-script-"));
        assert!(id("", None).starts_with("chart-"));
        assert_eq!(id("x", Some("opens")), "opens");
        assert_eq!(id("x", Some("9 lives \"<b>")), "chart-9-lives-b");
        assert!(id(&"a".repeat(500), None).len() < 70);
        // An id holds nothing a selector, a `for` or an `aria-*` list would trip on.
        for title in ["a b", "\"'<>&", "tab\there", "x\u{200b}y"] {
            let made = id(title, None);
            assert!(
                !made
                    .chars()
                    .any(|c| c.is_whitespace() || "\"'<>&\u{200b}".contains(c)),
                "{made}"
            );
        }
    }

    #[test]
    fn titles_in_any_script_give_their_own_ids() {
        let id = |title: &str| Common::new(title.to_owned()).id();
        // Russian, Japanese, Hindi (Devanagari) and Punjabi (Gurmukhi).
        let titles = [
            "Продажи",
            "Выручка",
            "売上",
            "利益",
            "बिक्री",
            "राजस्व",
            "ਵਿਕਰੀ",
            "ਆਮਦਨ",
        ];
        let ids: Vec<String> = titles.iter().map(|t| id(t)).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), titles.len(), "{ids:?}");
        assert!(ids[0].starts_with("chart-продажи-"), "{ids:?}");
        assert!(ids[2].starts_with("chart-売上-"), "{ids:?}");
        // The same title gives the same id on every render, so a morph keeps it.
        assert_eq!(id("ਵਿਕਰੀ"), id("ਵਿਕਰੀ"));
        // Titles an id can't tell apart by their letters still differ.
        assert_ne!(id("Sales!"), id("Sales?"));
        // A given id keeps its own script.
        let mut c = Common::new("x".to_owned());
        c.id = Some("ਵਿਕਰੀ_2026".to_owned());
        assert_eq!(c.id(), "ਵਿਕਰੀ_2026");
        c.id = Some("salesChart".to_owned());
        assert_eq!(c.id(), "salesChart");
    }

    #[test]
    fn coordinates_are_short() {
        assert_eq!(n(12.0, 2), "12");
        assert_eq!(n(12.345, 2), "12.35");
        assert_eq!(n(-0.0001, 2), "0");
        assert_eq!(n(f64::NAN, 2), "0");
        assert_eq!(pct(33.33333), "33.333%");
    }
}
