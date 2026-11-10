//! [`Funnel`]: how many reach each stage of an ordered process.

use super::format::NumberFormat;
use super::value::{IntoValue, finite};
use super::{Color, Common, Drawn, TableData, common_methods, fill, frame, pct, svg, value_cell};
use crate::components::Tone;
use crate::node::{AttrValue, Node, el};

/// One stage of a funnel.
#[derive(Clone, Debug)]
#[must_use]
pub struct Stage {
    name: String,
    value: Option<f64>,
    rate: Option<String>,
    tone: Option<Tone>,
}

impl Stage {
    /// A stage called `name` that `value` reached.
    pub fn new(name: impl Into<String>, value: impl IntoValue) -> Self {
        Self {
            name: name.into(),
            value: finite(value.into_value()),
            rate: None,
            tone: None,
        }
    }

    /// The rate as the server worded it (`95% of delivered`), shown instead of the
    /// share the funnel works out.
    pub fn rate(mut self, text: impl Into<String>) -> Self {
        self.rate = Some(text.into());
        self
    }

    /// Colours the stage by what it means, such as the last stage as a success.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = Some(tone);
        self
    }
}

/// The most stages a funnel draws: a process with more is a table.
pub const MAX_STAGES: usize = 12;

/// Stages of an ordered process as bars from one baseline, each as wide as its share
/// of the first stage, with its count, that share and the share of the stage before.
/// An ordered list, so it reads in order without the picture; the stages step through
/// one hue, faintest first.
///
/// ```
/// use navree_ui::charts::{Funnel, Stage};
///
/// let funnel = Funnel::new("Campaign funnel")
///     .stage(Stage::new("Delivered", 9_412))
///     .stage(Stage::new("Opened", 4_120))
///     .stage(Stage::new("Clicked", 812))
///     .stage(Stage::new("Converted", 96));
/// # let _ = navree_ui::render(&funnel.into());
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Funnel {
    common: Common,
    stages: Vec<Stage>,
    format: NumberFormat,
    rtl: bool,
}

common_methods!(Funnel);

impl Funnel {
    /// A funnel called `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            common: Common::new(title.into()),
            stages: Vec::new(),
            format: NumberFormat::number(),
            rtl: false,
        }
    }

    /// Adds a stage.
    pub fn stage(mut self, stage: Stage) -> Self {
        self.stages.push(stage);
        self
    }

    /// Adds stages.
    pub fn stages(mut self, stages: impl IntoIterator<Item = Stage>) -> Self {
        self.stages.extend(stages);
        self
    }

    /// How counts are written.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.format = format;
        self
    }

    /// Bars grow from the right, for a right-to-left page.
    pub fn rtl(mut self) -> Self {
        self.rtl = true;
        self
    }

    fn into_node(self) -> Node {
        let mut this = self;
        let drawn = this.draw();
        let height = (this.stages.len().min(MAX_STAGES) as u32 * 44).max(88);
        frame(this.common, drawn, "funnel", height)
    }

    fn draw(&mut self) -> Option<Drawn> {
        let total = self.stages.len();
        self.stages.truncate(MAX_STAGES);
        if self.stages.is_empty() || self.stages.iter().all(|s| s.value.is_none()) {
            return None;
        }
        let labels = self.common.labels.clone();
        let numbers = self.common.numbers.clone();
        let fmt = |v: f64| self.format.format(v, &numbers);
        let share = |v: Option<f64>, of: Option<f64>| match (v, of) {
            (Some(v), Some(of)) if of > 0.0 => {
                Some(NumberFormat::percent().format(v / of, &numbers))
            }
            _ => None,
        };
        let first = self.stages[0].value;
        let count = self.stages.len();
        let mut items: Vec<Node> = Vec::with_capacity(count);
        let mut rows = Vec::with_capacity(count);
        for (at, stage) in self.stages.iter().enumerate() {
            let previous = at.checked_sub(1).and_then(|p| self.stages[p].value);
            let of_first = share(stage.value, first);
            let of_previous = if at == 0 {
                None
            } else {
                share(stage.value, previous)
            };
            let width = match (stage.value, first) {
                (Some(v), Some(f)) if f > 0.0 => (v.max(0.0) / f * 100.0).clamp(0.0, 100.0),
                _ => 0.0,
            };
            // Faintest first: the step through one hue follows the stage's place.
            let step = if count <= 1 {
                6
            } else {
                1 + (at * 5) / (count - 1)
            };
            let color = stage.tone.map(Color::Tone);
            let x = if self.rtl { 100.0 - width } else { 0.0 };
            let corners = if self.rtl {
                "inset(0 round 4px 0 0 4px)"
            } else {
                "inset(0 round 0 4px 4px 0)"
            };
            let mut bar = el("rect")
                .slot("chart-bar")
                .attr("data-i", at as i64)
                .attr("x", pct(x))
                .attr("width", pct(width))
                .attr("y", "0")
                .attr("height", "12")
                .attr("clip-path", corners);
            bar = match color {
                Some(color) => color.on(bar),
                None => bar.attr("data-step", step as i64),
            };
            let detail = stage
                .rate
                .clone()
                .or_else(|| of_first.clone().filter(|_| at > 0));
            items.push(
                el("li")
                    .slot("chart-stage")
                    .attr("data-i", at as i64)
                    .children((
                        el("div").slot("chart-stage-head").children((
                            el("span")
                                .slot("chart-stage-name")
                                .child(stage.name.clone()),
                            el("span").slot("chart-stage-value").children((
                                el("strong").child(stage.value.map_or_else(|| "–".to_owned(), fmt)),
                                detail.map(|d| el("span").child(d)),
                            )),
                        )),
                        svg()
                            .slot("chart-track")
                            .attr("width", "100%")
                            .attr("height", "12")
                            .child((
                                el("rect")
                                    .slot("chart-track-bg")
                                    .attr("width", "100%")
                                    .attr("height", "12"),
                                bar,
                            )),
                        // The second stage's share of the one before is its share of the first.
                        of_previous.clone().filter(|_| at > 1).map(|p| {
                            el("span")
                                .slot("chart-stage-step")
                                .child(format!("{} {p}", labels.of_previous))
                        }),
                    ))
                    .into(),
            );
            rows.push((
                stage.name.clone(),
                vec![
                    value_cell(stage.value.map(fmt), &labels),
                    value_cell(of_first, &labels),
                    value_cell(of_previous, &labels),
                ],
            ));
        }
        let last = self.stages.iter().rev().find(|s| s.value.is_some());
        let summary = match (self.stages.first(), last) {
            (Some(a), Some(b)) => fill(
                &labels.summary_funnel,
                &[
                    ("first_name", &a.name),
                    ("first", &a.value.map_or_else(|| "–".to_owned(), fmt)),
                    ("last_name", &b.name),
                    ("last", &b.value.map_or_else(|| "–".to_owned(), fmt)),
                    (
                        "rate",
                        &share(b.value, first).unwrap_or_else(|| "–".to_owned()),
                    ),
                ],
            ),
            _ => String::new(),
        };
        let table = TableData {
            head: vec![
                (labels.category.to_string(), None),
                (labels.value.to_string(), None),
                (labels.of_first.to_string(), None),
                (labels.of_previous.to_string(), None),
            ],
            rows,
            links: Vec::new(),
            note: None,
        };
        Some(Drawn {
            legend: None,
            body: el("ol")
                .slot("chart-stages")
                .attr_opt("data-rtl", self.rtl.then_some(""))
                .child(items)
                .into(),
            body_attrs: vec![
                ("data-mode", AttrValue::Static("rows")),
                ("data-rows", AttrValue::Int(count as i64)),
            ],
            summary,
            table,
            height: (count as u32 * 44).max(88),
            note: (total > count).then(|| {
                fill(
                    &labels.stages_cut,
                    &[("shown", &count.to_string()), ("total", &total.to_string())],
                )
            }),
            interactive: true,
        })
    }
}

crate::node::into_node!(Funnel);
