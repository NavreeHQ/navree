//! [`Waterfall`]: an amount built up and taken down step by step, each bar floating at the
//! running total.

use std::borrow::Cow;

use super::format::NumberFormat;
use super::motion::{Effect, Motion};
use super::paint::Ink;
use super::scale::nice;
use super::value::{IntoValue, finite};
use super::{
    Common, Drawn, Key, LegendItem, TableData, Tip, TipLine, common_methods, fill, frame, legend,
    n, nodes, pct, svg, value_cell,
};
use crate::node::{AttrValue, Node, el};

/// The most rows a waterfall draws: the first ones, when it was given more.
pub const MAX_WATERFALL_ROWS: usize = 24;

/// The height of a row, in pixels.
const ROW: f64 = 28.0;
/// The thickness of a bar.
const THICK: f64 = 16.0;

/// The words a waterfall writes besides the ones every chart shares, in English by default.
/// Templates name their blanks in braces.
///
/// ```
/// use navree_ui::charts::{Waterfall, WaterfallLabels};
///
/// let mut words = WaterfallLabels::english();
/// words.added = "Ajouté".into();
/// words.deducted = "Déduit".into();
/// let chart = Waterfall::new("Versement").words(words);
/// # let _ = navree_ui::render(&chart.into());
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct WaterfallLabels {
    /// The legend's name for a step that adds.
    pub added: Cow<'static, str>,
    /// The legend's name for a step that takes away.
    pub deducted: Cow<'static, str>,
    /// The legend's name for a total.
    pub total: Cow<'static, str>,
    /// The first column of the table.
    pub step: Cow<'static, str>,
    /// The column of the table that holds each row's amount.
    pub amount: Cow<'static, str>,
    /// The column of the table that holds the running total after each row.
    pub running: Cow<'static, str>,
    /// The summary: `{first_name}`, `{first}`, `{last_name}`, `{last}`, `{added}`,
    /// `{deducted}`.
    pub summary: Cow<'static, str>,
    /// Under a waterfall with more rows than it draws: `{shown}`, `{total}`.
    pub steps_cut: Cow<'static, str>,
}

impl Default for WaterfallLabels {
    fn default() -> Self {
        Self::english()
    }
}

impl WaterfallLabels {
    /// The English words.
    pub const fn english() -> Self {
        Self {
            added: Cow::Borrowed("Added"),
            deducted: Cow::Borrowed("Deducted"),
            total: Cow::Borrowed("Total"),
            step: Cow::Borrowed("Step"),
            amount: Cow::Borrowed("Amount"),
            running: Cow::Borrowed("Running total"),
            summary: Cow::Borrowed(
                "From {first_name}, {first}, to {last_name}, {last}: {added} added and {deducted} deducted.",
            ),
            steps_cut: Cow::Borrowed("The first {shown} of {total} rows are drawn."),
        }
    }
}

/// One row of a waterfall.
#[derive(Clone, Debug)]
enum Row {
    /// A bar from zero to an amount given, where the running total starts.
    Start { label: String, amount: Option<f64> },
    /// A bar from the running total to the running total and the amount.
    Step { label: String, amount: Option<f64> },
    /// A bar from zero to the running total.
    Total { label: String },
}

/// What a row is drawn as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Added,
    Deducted,
    Total,
}

/// A row's bar: what it is drawn as, from where to where (none for a step with no amount),
/// the running total after it, and the amount as the row writes it.
type Bar = (Kind, Option<(f64, f64)>, f64, Option<String>);

/// A waterfall: an amount built up and taken down in steps, a row each, from what a
/// customer paid to what a store is paid, from revenue to profit. Each step's bar starts where the
/// last one ended, so it floats at the running total: one that adds runs on in one colour,
/// one that takes away runs back in another. A total is a bar from zero, in a third colour.
/// The amount is written at the end of each row, with its sign for a step.
///
/// The rows are in the page with their words, the arrow keys move between them, and the
/// numbers are a table: each row's amount and the running total after it.
///
/// ```
/// use navree_ui::charts::{NumberFormat, Waterfall};
///
/// let settlement = Waterfall::new("This month's settlement")
///     .locale("en-IN")
///     .format(NumberFormat::currency("₹").max_decimals(0))
///     .start("Orders", 2_50_000)
///     .step("Store fee", -25_000)
///     .step("Tax on the fee", -4_500)
///     .step("Returns", -12_000)
///     .step("Promotions refunded", 3_000)
///     .step("Tax withheld", -2_500)
///     .total("Paid to the bank");
/// # let _ = navree_ui::render(&settlement.into());
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Waterfall {
    common: Common,
    rows: Vec<Row>,
    added: Ink,
    deducted: Ink,
    total: Ink,
    format: NumberFormat,
    words: WaterfallLabels,
    motion: Motion,
    rtl: bool,
}

common_methods!(Waterfall);

impl Waterfall {
    /// A waterfall called `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            common: Common::new(title.into()),
            rows: Vec::new(),
            added: Ink::slot(1),
            deducted: Ink::slot(2),
            total: Ink::text(),
            format: NumberFormat::number(),
            words: WaterfallLabels::english(),
            motion: Motion::none(),
            rtl: false,
        }
    }

    /// Starts at `amount`: a bar from zero in the totals' colour, and the running total from
    /// there. Give it first; a later one starts the running total again at its amount.
    pub fn start(mut self, label: impl Into<String>, amount: impl IntoValue) -> Self {
        self.rows.push(Row::Start {
            label: label.into(),
            amount: finite(amount.into_value()),
        });
        self
    }

    /// Adds a step of `amount`: above zero it adds to the running total, below it takes
    /// away. A step with no amount (`None`, or not a finite number) is a row with no bar,
    /// and leaves the running total as it was.
    pub fn step(mut self, label: impl Into<String>, amount: impl IntoValue) -> Self {
        self.rows.push(Row::Step {
            label: label.into(),
            amount: finite(amount.into_value()),
        });
        self
    }

    /// Adds a total: a bar from zero to the running total so far, a subtotal between steps
    /// or the net at the end.
    pub fn total(mut self, label: impl Into<String>) -> Self {
        self.rows.push(Row::Total {
            label: label.into(),
        });
        self
    }

    /// The colours of a step that adds, a step that takes away and a total: chart colours
    /// 1 and 2 and the text ink by default. Use tones when adding means good and taking away
    /// bad.
    pub fn colors(
        mut self,
        added: impl Into<Ink>,
        deducted: impl Into<Ink>,
        total: impl Into<Ink>,
    ) -> Self {
        self.added = added.into();
        self.deducted = deducted.into();
        self.total = total.into();
        self
    }

    /// How amounts are written: `NumberFormat::currency("₹")` for money.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.format = format;
        self
    }

    /// The words the chart writes besides the common ones.
    pub fn words(mut self, words: WaterfallLabels) -> Self {
        self.words = words;
        self
    }

    /// How the bars come in: each grows from where it starts, one after another. Off by
    /// default; always off for a reader who asks for less motion.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
        self
    }

    /// Bars run from the right, for a right-to-left page.
    pub fn rtl(mut self) -> Self {
        self.rtl = true;
        self
    }

    fn into_node(self) -> Node {
        let mut this = self;
        let drawn = this.draw();
        let height = (this.rows.len().min(MAX_WATERFALL_ROWS) as f64 * ROW + 20.0) as u32;
        frame(this.common, drawn, "waterfall", height.max(88))
    }

    fn draw(&mut self) -> Option<Drawn> {
        let given = self.rows.len();
        let rows = &self.rows[..given.min(MAX_WATERFALL_ROWS)];
        let has_amount = rows.iter().any(|row| match row {
            Row::Start { amount, .. } | Row::Step { amount, .. } => amount.is_some(),
            Row::Total { .. } => false,
        });
        if !has_amount {
            return None;
        }
        let labels = self.common.labels.clone();
        let numbers = self.common.numbers.clone();
        let words = self.words.clone();
        let fmt = |v: f64| self.format.format(v, &numbers);
        // A step's amount carries its sign: what it does to the total.
        let signed = |v: f64| {
            let text = fmt(v);
            if v > 0.0 { format!("+{text}") } else { text }
        };

        // Each row's bar, from where to where, and the running total after it.
        let mut running = 0.0_f64;
        let mut bars: Vec<Bar> = Vec::with_capacity(rows.len());
        let (mut added, mut deducted) = (0.0_f64, 0.0_f64);
        for row in rows {
            match row {
                Row::Start { amount, .. } => {
                    if let Some(v) = amount {
                        running = *v;
                    }
                    bars.push((
                        Kind::Total,
                        amount.map(|v| (0.0, v)),
                        running,
                        amount.map(fmt),
                    ));
                }
                Row::Step { amount, .. } => {
                    let from = running;
                    if let Some(v) = amount {
                        running += v;
                        if *v >= 0.0 {
                            added += v;
                        } else {
                            deducted -= v;
                        }
                    }
                    let kind = if amount.is_some_and(|v| v < 0.0) {
                        Kind::Deducted
                    } else {
                        Kind::Added
                    };
                    bars.push((
                        kind,
                        amount.map(|_| (from, running)),
                        running,
                        amount.map(signed),
                    ));
                }
                Row::Total { .. } => {
                    bars.push((
                        Kind::Total,
                        Some((0.0, running)),
                        running,
                        Some(fmt(running)),
                    ));
                }
            }
        }

        let low = bars
            .iter()
            .filter_map(|(_, span, ..)| *span)
            .flat_map(|(a, b)| [a, b])
            .fold(0.0_f64, f64::min);
        let high = bars
            .iter()
            .filter_map(|(_, span, ..)| *span)
            .flat_map(|(a, b)| [a, b])
            .fold(0.0_f64, f64::max);
        let ticks = nice(low, high, 5, self.format.whole());
        // An axis of large amounts writes them short (`₹2.5L`), as a bar chart's does.
        let compact = self
            .format
            .axis_compact(ticks.values.iter().fold(0.0_f64, |m, v| m.max(v.abs())));
        let scale = ticks.scale;
        let mirror = |x: f64| if self.rtl { 100.0 - x } else { x };
        let at = |v: f64| mirror(scale.at(v) * 100.0);
        let grid: Vec<(f64, f64)> = ticks.values.iter().map(|v| (at(*v), *v)).collect();
        let lines = || -> Vec<Node> {
            grid.iter()
                .map(|(x, v)| {
                    el("line")
                        .slot(if *v == 0.0 {
                            "chart-zero"
                        } else {
                            "chart-grid"
                        })
                        .attr("x1", pct(*x))
                        .attr("x2", pct(*x))
                        .attr("y1", "0")
                        .attr("y2", n(ROW, 0))
                        .into()
                })
                .collect()
        };
        let ink_of = |kind: Kind| match kind {
            Kind::Added => self.added,
            Kind::Deducted => self.deducted,
            Kind::Total => self.total,
        };
        let y = (ROW - THICK) / 2.0;

        let mut drawn: Vec<Node> = Vec::with_capacity(rows.len() + 1);
        let mut table_rows: Vec<(String, Vec<Node>)> = Vec::with_capacity(rows.len());
        for (i, (row, (kind, span, after, text))) in rows.iter().zip(&bars).enumerate() {
            let label = match row {
                Row::Start { label, .. } | Row::Step { label, .. } | Row::Total { label } => label,
            };
            let ink = ink_of(*kind);
            let mut marks = lines();
            if let Some((from, to)) = span {
                let (a, b) = (at(*from), at(*to));
                let bar = ink.on(el("rect")
                    .slot("chart-bar")
                    .attr("data-i", i as i64)
                    .attr("x", pct(a.min(b)))
                    .attr("width", pct((a - b).abs().max(0.2)))
                    .attr("y", n(y, 1))
                    .attr("height", n(THICK, 1))
                    .attr("clip-path", "inset(0 round 3px)"));
                let effect = if b >= a {
                    Effect::GrowRight
                } else {
                    Effect::GrowLeft
                };
                marks.push(self.motion.apply(bar, effect, i).into());
            }
            // A dashed line from the running total down to the next row's bar, which starts there.
            if i + 1 < bars.len() && matches!(rows[i + 1], Row::Step { .. }) {
                let x = pct(at(*after));
                marks.push(
                    el("line")
                        .slot("chart-link")
                        .attr("x1", x.clone())
                        .attr("x2", x)
                        .attr("y1", n(y + THICK, 1))
                        .attr("y2", n(ROW + y, 1))
                        .into(),
                );
            }
            drawn.push(
                el("div")
                    .slot("chart-row")
                    .attr("data-i", i as i64)
                    .children((
                        el("span").slot("chart-row-label").child(label.clone()),
                        svg()
                            .slot("chart-track")
                            .attr("width", "100%")
                            .attr("height", n(ROW, 0))
                            .attr("overflow", "visible")
                            .child(marks),
                        el("span")
                            .slot("chart-row-value")
                            .child(text.clone().unwrap_or_default()),
                    ))
                    .into(),
            );
            table_rows.push((
                label.clone(),
                vec![
                    match text {
                        Some(text) => ink.on(el("span")).child(text.clone()).into(),
                        None => Ink::muted()
                            .on(el("span"))
                            .child(value_cell(None, &labels))
                            .into(),
                    },
                    Node::from(fmt(*after)),
                ],
            ));
        }

        // The value axis under the rows.
        let ticks_text: Vec<Node> = grid
            .iter()
            .enumerate()
            .map(|(j, (x, v))| {
                let anchor = if *x < 4.0 {
                    "start"
                } else if *x > 96.0 {
                    "end"
                } else {
                    "middle"
                };
                el("text")
                    .attr("x", pct(*x))
                    .attr("y", "14")
                    .attr("text-anchor", anchor)
                    .attr_opt("data-minor", (grid.len() > 3 && j % 2 == 1).then_some(""))
                    .child(if compact {
                        self.format.format_compact(*v, &numbers)
                    } else {
                        self.format.format_places(*v, &numbers, ticks.places)
                    })
                    .into()
            })
            .collect();
        drawn.push(
            el("div")
                .slot("chart-row")
                .attr("data-axis", "")
                .children((
                    el("span").slot("chart-row-label"),
                    svg()
                        .slot("chart-track")
                        .attr("width", "100%")
                        .attr("height", "20")
                        .attr("overflow", "visible")
                        .child(nodes(ticks_text)),
                    el("span").slot("chart-row-value"),
                ))
                .into(),
        );

        // The legend names what each colour means, in the order the rows meet them.
        let mut kinds: Vec<Kind> = Vec::new();
        for (kind, span, ..) in &bars {
            if span.is_some() && !kinds.contains(kind) {
                kinds.push(*kind);
            }
        }
        let items: Vec<LegendItem> = kinds
            .iter()
            .map(|kind| LegendItem {
                name: match kind {
                    Kind::Added => words.added.to_string(),
                    Kind::Deducted => words.deducted.to_string(),
                    Kind::Total => words.total.to_string(),
                },
                color: ink_of(*kind).color(),
                key: Key::Block,
            })
            .collect();
        let legend = legend(&self.common.id(), &items, false);

        let label_of = |row: &Row| match row {
            Row::Start { label, .. } | Row::Step { label, .. } | Row::Total { label } => {
                label.clone()
            }
        };
        let first = rows
            .iter()
            .zip(&bars)
            .find(|(_, (_, span, ..))| span.is_some());
        let summary = match first {
            Some((row, (_, _, after, _))) => fill(
                &words.summary,
                &[
                    ("first_name", &label_of(row)),
                    ("first", &fmt(*after)),
                    ("last_name", &rows.last().map(label_of).unwrap_or_default()),
                    ("last", &fmt(running)),
                    ("added", &fmt(added)),
                    ("deducted", &fmt(deducted)),
                ],
            ),
            None => String::new(),
        };

        let count = rows.len();
        Some(Drawn {
            legend: Some(legend),
            body: el("div")
                .slot("chart-rows")
                .attr_opt("data-rtl", self.rtl.then_some(""))
                .child(drawn)
                .into(),
            body_attrs: vec![
                ("data-mode", AttrValue::Static("rows")),
                ("data-rows", AttrValue::Int(count as i64)),
            ],
            summary,
            table: TableData {
                head: vec![
                    (words.step.to_string(), None),
                    (words.amount.to_string(), None),
                    (words.running.to_string(), None),
                ],
                rows: table_rows,
                links: Vec::new(),
                note: None,
            },
            height: (count as f64 * ROW + 20.0) as u32,
            note: (given > count).then(|| {
                fill(
                    &words.steps_cut,
                    &[("shown", &count.to_string()), ("total", &given.to_string())],
                )
            }),
            interactive: true,
            text: false,
            tip: Some(Tip {
                big: false,
                lines: vec![
                    TipLine {
                        name: words.amount.to_string(),
                        unit: None,
                        color: None,
                        key: true,
                    },
                    TipLine {
                        name: words.running.to_string(),
                        unit: None,
                        color: None,
                        key: false,
                    },
                ],
            }),
        })
    }
}

crate::node::into_node!(Waterfall);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Tone;
    use crate::render;

    /// A month's settlement in seven rows, from what customers paid to what reaches the bank.
    fn settlement() -> Waterfall {
        Waterfall::new("Settlement")
            .id("w")
            .locale("en-IN")
            .format(NumberFormat::currency("₹").max_decimals(0))
            .start("Orders", 100_000)
            .step("Store fee", -10_000)
            .step("Tax on the fee", -1_800)
            .step("Returns", -8_200)
            .step("Promotions refunded", 2_000)
            .step("Tax withheld", -2_000)
            .total("Paid to the bank")
    }

    fn html(chart: Waterfall) -> String {
        render(&chart.into())
    }

    fn bar(out: &str, i: usize) -> String {
        let from = out
            .find(&format!(r#"<rect data-slot="chart-bar" data-i="{i}""#))
            .unwrap_or_else(|| panic!("bar {i}: {out}"));
        let end = out[from..].find('>').expect("end");
        out[from..from + end].to_owned()
    }

    #[test]
    fn each_bar_starts_where_the_last_ended() {
        let out = html(settlement());
        // The scale runs from 0 to 1,00,000: the first bar fills it.
        assert!(
            bar(&out, 0).contains(r#"x="0%" width="100%""#),
            "{}",
            bar(&out, 0)
        );
        // The fee runs back from 1,00,000 to 90,000.
        assert!(
            bar(&out, 1).contains(r#"x="90%" width="10%""#),
            "{}",
            bar(&out, 1)
        );
        // Then 90,000 to 88,200, and 88,200 to 80,000.
        assert!(bar(&out, 2).contains(r#"x="88.2%" width="1.8%""#));
        assert!(bar(&out, 3).contains(r#"x="80%" width="8.2%""#));
        // A step that adds runs on from the running total.
        assert!(bar(&out, 4).contains(r#"x="80%" width="2%""#));
        assert!(bar(&out, 5).contains(r#"x="80%" width="2%""#));
        // The total is a bar from zero to the running total: 80,000.
        assert!(bar(&out, 6).contains(r#"x="0%" width="80%""#));
        assert_eq!(out.matches(r#"data-slot="chart-row" data-i=""#).count(), 7);
    }

    #[test]
    fn a_step_that_adds_a_step_that_takes_away_and_a_total_each_have_a_colour() {
        let out = html(settlement());
        assert!(
            bar(&out, 0).contains(r#"data-color="text""#),
            "{}",
            bar(&out, 0)
        );
        assert!(bar(&out, 1).contains(r#"data-color="2""#));
        assert!(bar(&out, 4).contains(r#"data-color="1""#));
        assert!(bar(&out, 6).contains(r#"data-color="text""#));
        // The legend names them in the order the rows meet them.
        let names: Vec<&str> = out
            .split(r#"<span data-slot="chart-legend-name">"#)
            .skip(1)
            .map(|s| s.split('<').next().unwrap_or_default())
            .collect();
        assert_eq!(names, ["Total", "Deducted", "Added"], "{out}");
        let toned = html(settlement().colors(
            Ink::tone(Tone::Success),
            Ink::tone(Tone::Danger),
            Ink::slot(7),
        ));
        assert!(bar(&toned, 1).contains(r#"data-tone="danger""#));
        assert!(bar(&toned, 4).contains(r#"data-tone="success""#));
        assert!(bar(&toned, 6).contains(r#"data-color="7""#));
    }

    #[test]
    fn amounts_are_written_with_their_sign_in_the_readers_grouping() {
        let out = html(settlement());
        assert!(
            out.contains(r#"<span data-slot="chart-row-value">₹1,00,000</span>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-slot="chart-row-value">-₹10,000</span>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-slot="chart-row-value">+₹2,000</span>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-slot="chart-row-value">₹80,000</span>"#),
            "{out}"
        );
        // The axis writes large amounts short, so its ticks never run into each other.
        assert!(
            out.contains(">₹20K</text>") && out.contains(">₹1L</text>"),
            "{out}"
        );
        let small = html(Waterfall::new("x").start("a", 50).step("b", -20));
        assert!(small.contains(">50</text>"), "{small}");
    }

    #[test]
    fn the_table_holds_each_amount_and_the_running_total() {
        let out = html(settlement());
        assert!(
            out.contains(r#"<th scope="col">Step</th><th scope="col" data-num="">Amount</th><th scope="col" data-num="">Running total</th>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<th scope="row">Store fee</th><td data-num=""><span data-color="2">-₹10,000</span></td><td data-num="">₹90,000</td>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<th scope="row">Paid to the bank</th><td data-num=""><span data-color="text">₹80,000</span></td><td data-num="">₹80,000</td>"#),
            "{out}"
        );
        assert!(
            out.contains("From Orders, ₹1,00,000, to Paid to the bank, ₹80,000: ₹2,000 added and ₹22,000 deducted."),
            "{out}"
        );
    }

    #[test]
    fn a_dashed_link_runs_from_each_running_total_to_the_next_step() {
        let out = html(settlement());
        // After the first row, a link at 1,00,000 into the fee's row; none before the total.
        assert!(
            out.contains(
                r#"<line data-slot="chart-link" x1="100%" x2="100%" y1="22" y2="34"></line>"#
            ),
            "{out}"
        );
        assert_eq!(out.matches(r#"data-slot="chart-link""#).count(), 5);
    }

    #[test]
    fn below_zero_the_scale_reaches_past_it() {
        let out = html(
            Waterfall::new("Cash")
                .step("Opening", 50)
                .step("Spent", -80)
                .total("Closing"),
        );
        // The scale runs from -40 to 60 here: zero sits at 40%.
        assert!(bar(&out, 0).contains(r#"x="40%" width="50%""#), "{out}");
        assert!(bar(&out, 1).contains(r#"x="10%" width="80%""#), "{out}");
        assert!(bar(&out, 2).contains(r#"x="10%" width="30%""#), "{out}");
        assert!(out.contains(r#"data-slot="chart-zero""#));
    }

    #[test]
    fn right_to_left_mirrors_the_bars() {
        let out = html(settlement().rtl());
        assert!(
            bar(&out, 1).contains(r#"x="0%" width="10%""#),
            "{}",
            bar(&out, 1)
        );
        assert!(bar(&out, 6).contains(r#"x="20%" width="80%""#));
        assert!(out.contains(r#"data-slot="chart-rows" data-rtl"#));
    }

    #[test]
    fn a_step_with_no_amount_draws_no_bar_and_keeps_the_total() {
        let out = html(
            Waterfall::new("x")
                .start("Opening", 10)
                .step("Unknown", None::<f64>)
                .step("Later", 5)
                .total("Closing"),
        );
        assert!(
            !out.contains(r#"data-slot="chart-bar" data-i="1""#),
            "{out}"
        );
        assert!(
            out.contains(r#"<th scope="row">Unknown</th><td data-num=""><span data-color="other"><span aria-hidden="true">–</span><span data-sr="">No value</span></span></td><td data-num="">10</td>"#),
            "{out}"
        );
        assert!(out.contains(
            r#"<th scope="row">Closing</th><td data-num=""><span data-color="text">15</span></td>"#
        ));
    }

    #[test]
    fn nothing_to_draw_is_the_empty_state_and_more_rows_than_the_most_are_cut() {
        assert!(html(Waterfall::new("x")).contains(r#"data-state="empty""#));
        assert!(html(Waterfall::new("x").total("All")).contains(r#"data-state="empty""#));
        let mut long = Waterfall::new("Long").start("Start", 1_000);
        for at in 0..40 {
            long = long.step(format!("s{at}"), -1);
        }
        let out = html(long);
        assert_eq!(
            out.matches(r#"data-slot="chart-bar""#).count(),
            MAX_WATERFALL_ROWS
        );
        assert!(out.contains("The first 24 of 41 rows are drawn."), "{out}");
        let odd = html(
            Waterfall::new("x")
                .start("a", f64::MAX)
                .step("b", f64::NAN)
                .step("c", -f64::MAX),
        );
        assert!(!odd.contains("NaN") && !odd.contains("inf"), "{odd}");
    }

    #[test]
    fn the_figure_is_a_named_group_of_rows_with_its_sheets_and_no_style() {
        let out = html(settlement().motion(Motion::new()));
        assert!(out.contains(r#"data-kind="waterfall""#));
        assert!(
            out.contains(r#"role="group" aria-label="Settlement, chart""#),
            "{out}"
        );
        assert!(out.contains(r#"data-mode="rows" data-rows="7""#), "{out}");
        assert_eq!(out.matches(r#"data-motion="grow-"#).count(), 7);
        assert!(!out.contains("style="), "{out}");
        assert_eq!(
            crate::View::new("x").body(settlement()).uses(),
            crate::Uses::CHARTS
        );
    }
}
