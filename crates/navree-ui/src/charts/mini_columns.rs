//! [`MiniColumns`]: small multiples, one 100% column for each item.

use super::format::NumberFormat;
use super::linear::{Entry, LinearLabels, MAX_PARTS, Shares, Unit, legend_list, say};
use super::motion::{Effect, Motion};
use super::paint::Ink;
use super::value::{IntoValue, clean};
use super::{
    Common, Drawn, TableData, Tip, TipLine, common_methods, fill, frame, pct, svg, value_cell,
};
use crate::node::{AttrValue, Node, Uses, el};

/// The most columns drawn: past this, the table is the way to read them.
pub const MAX_COLUMNS: usize = 48;

/// One column of [`MiniColumns`]: a label and the amount of each part.
///
/// ```
/// use navree_ui::charts::MiniColumn;
///
/// let first = MiniColumn::new("Cycle 1", [36, 18, 40]);
/// let second = MiniColumn::new("Cycle 2", [22, 30, 41]).total("1 h 33 min");
/// # let _ = (first, second);
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct MiniColumn {
    label: String,
    values: Vec<Option<f64>>,
    total: Option<String>,
}

impl MiniColumn {
    /// A column called `label`, holding one value for each part of the chart, in the
    /// order the parts were added. A part with no value, or none above zero, draws
    /// nothing.
    pub fn new<V: IntoValue>(
        label: impl Into<String>,
        values: impl IntoIterator<Item = V>,
    ) -> Self {
        Self {
            label: label.into(),
            values: clean(values),
            total: None,
        }
    }

    /// Writes `text` under the column and in the table, instead of the sum of its parts.
    pub fn total(mut self, text: impl Into<String>) -> Self {
        self.total = Some(text.into());
        self
    }
}

/// Small multiples: one column for each item (a sleep cycle, a day, a store), each split
/// into the same parts and drawn as 100%, so the columns compare their makeup and not
/// their size. A label sits above each column and its total below; hovering or moving to
/// a column lists its parts in a tooltip.
///
/// ```
/// use navree_ui::charts::{Ink, MiniColumn, MiniColumns, Motion};
/// use navree_ui::Tone;
///
/// let cycles = MiniColumns::new("Sleep cycles")
///     .minutes()
///     .part("Deep", Ink::slot(7))
///     .part("REM", Ink::tone(Tone::named("cyan")))
///     .part("Core", Ink::slot(3))
///     .column(MiniColumn::new("Cycle 1", [36, 18, 40]))
///     .column(MiniColumn::new("Cycle 2", [22, 30, 41]))
///     .column(MiniColumn::new("Cycle 3", [14, 36, 38]))
///     .motion(Motion::new());
/// let html = navree_ui::render(&cycles.into());
/// assert!(html.contains("Cycle 2"), "{html}");
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct MiniColumns {
    common: Common,
    words: LinearLabels,
    parts: Vec<(String, Ink)>,
    columns: Vec<MiniColumn>,
    unit: Unit,
    column_name: Option<String>,
    height: u32,
    totals: bool,
    motion: Motion,
}

common_methods!(MiniColumns);

impl MiniColumns {
    /// Mini columns called `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            common: Common::new(title.into()),
            words: LinearLabels::english(),
            parts: Vec::new(),
            columns: Vec::new(),
            unit: Unit::Number(NumberFormat::number()),
            column_name: None,
            height: 48,
            totals: true,
            motion: Motion::none(),
        }
    }

    /// Adds a part, stacked under the ones added before it, in `ink`.
    pub fn part(mut self, name: impl Into<String>, ink: impl Into<Ink>) -> Self {
        self.parts.push((name.into(), ink.into()));
        self
    }

    /// Adds a part in the next of the categorical colours.
    pub fn part_auto(mut self, name: impl Into<String>) -> Self {
        let ink = Ink::nth(self.parts.len());
        self.parts.push((name.into(), ink));
        self
    }

    /// Adds a column.
    pub fn column(mut self, column: MiniColumn) -> Self {
        self.columns.push(column);
        self
    }

    /// Adds columns.
    pub fn columns(mut self, columns: impl IntoIterator<Item = MiniColumn>) -> Self {
        self.columns.extend(columns);
        self
    }

    /// How values are written.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.unit = Unit::Number(format);
        self
    }

    /// Values are minutes, written as durations: `45 min`, `1 h 23 min`. The words come
    /// from [`words`](Self::words).
    pub fn minutes(mut self) -> Self {
        self.unit = Unit::Minutes;
        self
    }

    /// The name of the table's first column (`Cycle`); the chart's word for a category
    /// otherwise.
    pub fn column_name(mut self, name: impl Into<String>) -> Self {
        self.column_name = Some(name.into());
        self
    }

    /// The columns' height in pixels, 24 to 160 (48 by default).
    pub fn height(mut self, px: u32) -> Self {
        self.height = px.clamp(24, 160);
        self
    }

    /// Writes no total under the columns.
    pub fn hide_totals(mut self) -> Self {
        self.totals = false;
        self
    }

    /// How the columns come in: each fills from the top, one after another. Off by
    /// default; always off for a reader who asks for less motion.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
        self
    }

    /// The words the chart writes besides the common ones.
    pub fn words(mut self, words: LinearLabels) -> Self {
        self.words = words;
        self
    }

    fn into_node(self) -> Node {
        let mut this = self;
        let height = this.height + 40;
        let drawn = this.draw();
        frame(this.common, drawn, "mini-columns", height)
    }

    fn draw(&mut self) -> Option<Drawn> {
        let total_columns = self.columns.len();
        self.columns.truncate(MAX_COLUMNS);
        self.parts.truncate(MAX_PARTS);
        if self.columns.is_empty() || self.parts.is_empty() {
            return None;
        }
        let labels = self.common.labels.clone();
        let numbers = self.common.numbers.clone();
        let words = self.words.clone();
        let h = self.height;
        let text = |v: f64| self.unit.text(v, &numbers, &words);
        // Each column: its values, one for each part.
        let sum = |c: &MiniColumn| -> f64 {
            c.values
                .iter()
                .take(self.parts.len())
                .filter_map(|v| v.filter(|v| *v > 0.0))
                .sum()
        };

        let mut items: Vec<Node> = Vec::with_capacity(self.columns.len());
        let mut rows: Vec<(String, Vec<Node>)> = Vec::with_capacity(self.columns.len());
        for (at, column) in self.columns.iter().enumerate() {
            let values: Vec<Option<f64>> = (0..self.parts.len())
                .map(|p| column.values.get(p).copied().flatten())
                .collect();
            let shares = Shares::of(&values);
            let mut y = 0.0_f64;
            let mut rects: Vec<Node> = Vec::new();
            for (p, value) in values.iter().enumerate() {
                let Some(shares) = shares else { break };
                let fraction = shares.fraction(*value);
                if fraction <= 0.0 {
                    continue;
                }
                let rect = self.parts[p].1.on(el("rect")
                    .slot("cols-part")
                    .attr("x", "0")
                    .attr("width", "100%")
                    .attr("y", pct(y * 100.0))
                    .attr("height", pct(fraction * 100.0)));
                rects.push(self.motion.apply(rect, Effect::GrowDown, at).into());
                y += fraction;
            }
            let total = column.total.clone().unwrap_or_else(|| text(sum(column)));
            items.push(
                el("li")
                    .slot("cols-item")
                    .attr("data-i", at as i64)
                    .children((
                        el("span").slot("cols-label").child(column.label.clone()),
                        svg()
                            .slot("cols-bar")
                            .attr("width", "100%")
                            .attr("height", i64::from(h))
                            .uses(Uses::CHART_PAINT.with(Uses::CHART_LINEAR))
                            .children((
                                el("rect")
                                    .slot("cols-track")
                                    .attr("width", "100%")
                                    .attr("height", i64::from(h)),
                                rects,
                            )),
                        self.totals
                            .then(|| el("span").slot("cols-total").child(total.clone())),
                    ))
                    .into(),
            );
            let mut cells: Vec<Node> = values
                .iter()
                .map(|v| value_cell(v.map(text), &labels))
                .collect();
            cells.push(Node::from(total));
            rows.push((column.label.clone(), cells));
        }

        let legend = legend_list(
            self.parts
                .iter()
                .map(|(name, ink)| Entry {
                    ink: *ink,
                    name: name.clone(),
                    value: None,
                    index: None,
                    detail: None,
                    empty: false,
                })
                .collect(),
        );

        let mut head: Vec<(String, Option<super::Color>)> = vec![(
            self.column_name
                .clone()
                .unwrap_or_else(|| labels.category.to_string()),
            None,
        )];
        head.extend(
            self.parts
                .iter()
                .map(|(n, ink)| (n.clone(), Some(ink.color()))),
        );
        head.push((labels.total.to_string(), None));

        let largest = self
            .columns
            .iter()
            .max_by(|a, b| sum(a).total_cmp(&sum(b)))
            .map(|c| {
                (
                    c.label.clone(),
                    c.total.clone().unwrap_or_else(|| text(sum(c))),
                )
            });
        let (name, total) = largest.unwrap_or_default();
        let part_names = self
            .parts
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let summary = say(
            &words.summary_columns,
            &[
                ("count", &self.columns.len().to_string()),
                ("parts", &part_names),
                ("name", &name),
                ("total", &total),
            ],
        );

        let mut lines: Vec<TipLine> = self
            .parts
            .iter()
            .map(|(name, ink)| TipLine {
                name: name.clone(),
                unit: None,
                color: Some(ink.color()),
                key: true,
            })
            .collect();
        lines.push(TipLine {
            name: labels.total.to_string(),
            unit: None,
            color: None,
            key: false,
        });

        Some(Drawn {
            legend: Some(legend),
            body: el("ul").slot("cols").child(items).into(),
            body_attrs: vec![
                ("data-rows", AttrValue::Int(self.columns.len() as i64)),
                ("data-float", AttrValue::Flag),
            ],
            summary,
            table: TableData {
                head,
                rows,
                links: Vec::new(),
                note: None,
            },
            height: h + 40,
            note: (total_columns > self.columns.len()).then(|| {
                fill(
                    &words.columns_cut,
                    &[
                        ("shown", &self.columns.len().to_string()),
                        ("total", &total_columns.to_string()),
                    ],
                )
            }),
            interactive: true,
            tip: Some(Tip { big: false, lines }),
        })
    }
}

crate::node::into_node!(MiniColumns);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Tone;
    use crate::render;

    fn cycles() -> MiniColumns {
        MiniColumns::new("Sleep cycles")
            .minutes()
            .part("Deep", Ink::slot(7))
            .part("REM", Ink::tone(Tone::named("cyan")))
            .part("Core", Ink::slot(3))
            .column(MiniColumn::new("Cycle 1", [36, 18, 40]))
            .column(MiniColumn::new("Cycle 2", [22, 30, 41]))
            .column(MiniColumn::new("Cycle 3", [14, 36, 38]))
    }

    fn html(columns: MiniColumns) -> String {
        render(&columns.into())
    }

    fn count(haystack: &str, needle: &str) -> usize {
        haystack.matches(needle).count()
    }

    #[test]
    fn every_column_is_drawn_to_one_hundred_percent_from_the_top() {
        let out = html(cycles());
        // 36 + 18 + 40 = 94.
        assert!(
            out.contains(r##"y="0%" height="38.298%" data-color="7""##),
            "{out}"
        );
        assert!(
            out.contains(r##"y="38.298%" height="19.149%" data-tone="cyan""##),
            "{out}"
        );
        assert!(
            out.contains(r##"y="57.447%" height="42.553%" data-color="3""##),
            "{out}"
        );
        assert_eq!(count(&out, r#"data-slot="cols-item""#), 3);
        assert_eq!(count(&out, r#"data-slot="cols-part""#), 9);
        assert_eq!(count(&out, r#"data-slot="cols-track""#), 3);
        assert!(out.contains(r#"data-rows="3" data-float>"#), "{out}");
    }

    #[test]
    fn a_label_sits_above_and_a_total_below() {
        let out = html(cycles());
        assert!(
            out.contains(r#"<li data-slot="cols-item" data-i="0"><span data-slot="cols-label">Cycle 1</span><svg"#),
            "{out}"
        );
        assert!(
            out.contains(r#"</svg><span data-slot="cols-total">1 h 34 min</span></li>"#),
            "{out}"
        );
        // A total of the caller's own wording replaces the sum, in the table too.
        let out = html(
            cycles().column(MiniColumn::new("Cycle 4", [10, 10, 10]).total("about half an hour")),
        );
        assert!(
            out.contains(r#"<span data-slot="cols-total">about half an hour</span>"#),
            "{out}"
        );
        assert!(
            out.contains("<td data-num=\"\">about half an hour</td>"),
            "{out}"
        );
        let out = html(cycles().hide_totals());
        assert!(!out.contains("cols-total"), "{out}");
    }

    #[test]
    fn the_table_twin_has_a_column_for_each_part_and_the_total() {
        let out = html(cycles().column_name("Cycle"));
        assert!(
            out.contains(r#"<th scope="col">Cycle</th><th scope="col" data-num="" data-color="7">Deep</th><th scope="col" data-num="" data-tone="cyan">REM</th><th scope="col" data-num="" data-color="3">Core</th><th scope="col" data-num="">Total</th>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<th scope="row">Cycle 2</th><td data-num="">22 min</td><td data-num="">30 min</td><td data-num="">41 min</td><td data-num="">1 h 33 min</td>"#),
            "{out}"
        );
        assert!(
            out.contains(
                "3 columns, each split into Deep, REM, Core. Largest total: Cycle 1, 1 h 34 min."
            ),
            "{out}"
        );
    }

    #[test]
    fn the_tooltip_lists_each_part_in_its_colour_and_the_total_without_a_key() {
        let out = html(cycles());
        assert!(out.contains(r#"<div data-tip="row" data-s="0" data-color="7"><span data-slot="chart-key" data-key="line"></span><strong></strong><span>Deep</span></div>"#), "{out}");
        assert!(
            out.contains(
                r#"<div data-tip="row" data-s="3"><strong></strong><span>Total</span></div>"#
            ),
            "{out}"
        );
    }

    #[test]
    fn a_legend_names_the_parts_once() {
        let out = html(cycles());
        assert_eq!(count(&out, r#"data-slot="chart-legend-item""#), 3);
        assert!(out.contains(r#"<span data-slot="chart-legend-name">Core</span>"#));
        let out = html(
            MiniColumns::new("x")
                .part_auto("a")
                .part_auto("b")
                .column(MiniColumn::new("c", [1, 1])),
        );
        assert!(
            out.contains(r#"data-key="block" data-color="1""#)
                && out.contains(r#"data-key="block" data-color="2""#),
            "{out}"
        );
    }

    #[test]
    fn missing_values_draw_nothing_and_a_column_with_none_is_an_empty_track() {
        let out = html(
            cycles()
                .column(MiniColumn::new("Short", [10]))
                .column(MiniColumn::new("Gaps", [Some(5.0), None, Some(f64::NAN)]))
                .column(MiniColumn::new("None", [0, -3, 0])),
        );
        // "Short" has one part, so it fills the column; "None" draws no part.
        assert!(out.contains(r#"<th scope="row">Short</th><td data-num="">10 min</td><td data-num=""><span aria-hidden="true">–</span><span data-sr="">No value</span></td>"#), "{out}");
        assert!(
            out.contains(r#"<th scope="row">None</th><td data-num="">0 min</td>"#),
            "{out}"
        );
        assert_eq!(count(&out, r#"data-slot="cols-part""#), 9 + 1 + 1, "{out}");
    }

    #[test]
    fn nothing_to_draw_is_an_empty_chart() {
        assert!(
            html(MiniColumns::new("x").part("a", Ink::slot(1))).contains(r#"data-state="empty""#)
        );
        assert!(
            html(MiniColumns::new("x").column(MiniColumn::new("c", [1])))
                .contains(r#"data-state="empty""#)
        );
    }

    #[test]
    fn more_columns_than_the_most_are_cut_and_the_chart_says_so() {
        let mut chart = MiniColumns::new("Days").part("a", Ink::slot(1));
        for at in 0..60 {
            chart = chart.column(MiniColumn::new(format!("d{at}"), [at + 1]));
        }
        let out = html(chart);
        assert_eq!(count(&out, r#"data-slot="cols-item""#), MAX_COLUMNS);
        assert!(
            out.contains("The first 48 of 60 columns are drawn."),
            "{out}"
        );
    }

    #[test]
    fn columns_fill_from_the_top_in_turn_when_motion_is_on() {
        let out = html(cycles().motion(Motion::new().stagger(100)));
        assert_eq!(count(&out, r#"data-motion="grow-down""#), 9, "{out}");
        // The columns come one after another, their parts together.
        assert_eq!(
            count(&out, r#"data-motion="grow-down" data-delay="2""#),
            3,
            "{out}"
        );
        assert_eq!(
            count(&out, r#"data-motion="grow-down" data-delay="4""#),
            3,
            "{out}"
        );
        assert!(!html(cycles()).contains("data-motion"));
    }

    #[test]
    fn numbers_are_written_in_the_readers_grouping() {
        let out = html(
            MiniColumns::new("Orders")
                .format(NumberFormat::currency("₹"))
                .locale("en-IN")
                .part("Web", Ink::slot(1))
                .part("Store", Ink::slot(2))
                .column(MiniColumn::new("Mon", [1_234_567, 7_654_321])),
        );
        assert!(
            out.contains("₹12,34,567") && out.contains("₹76,54,321") && out.contains("₹88,88,888"),
            "{out}"
        );
    }

    #[test]
    fn the_figure_is_a_named_group_with_its_sheets() {
        let out = html(cycles());
        assert!(
            out.contains(r#"data-kind="mini-columns""#)
                && out.contains(r#"role="group" aria-label="Sleep cycles, chart""#),
            "{out}"
        );
        assert!(
            out.contains(r#"<svg aria-hidden="true" focusable="false" data-slot="cols-bar""#),
            "{out}"
        );
        assert!(!out.contains("style="), "{out}");
        let uses = crate::View::new("x").body(cycles()).uses();
        assert_eq!(
            uses,
            Uses::CHARTS
                .with(Uses::CHART_PAINT)
                .with(Uses::CHART_LINEAR)
        );
    }

    #[test]
    fn height_is_held_and_the_state_keeps_it() {
        assert!(html(cycles().height(1)).contains(r#"height="24""#));
        assert!(html(cycles().height(9999)).contains(r#"height="160""#));
        assert!(html(cycles().loading()).contains(r#"data-h="88""#));
    }
}
