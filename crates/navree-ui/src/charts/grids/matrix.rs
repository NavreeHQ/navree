//! [`MatrixHeatmap`]: every measure against every other, such as a correlation matrix.

use std::collections::BTreeMap;

use super::super::format::NumberFormat;
use super::super::motion::{Effect, Motion};
use super::super::paint::Ink;
use super::super::value::{IntoValue, finite};
use super::super::{
    Common, Drawn, TableData, Tip, TipLine, common_methods, fill, frame, value_cell,
};
use super::{GridLabels, count, heat_legend, swatch};
use crate::node::{AttrValue, Node, Uses, el};

/// How two measures relate: a signed value, whether it is significant, and a note.
///
/// ```
/// use navree_ui::charts::Relation;
///
/// let strong = Relation::new(0.62).note("30 days, p < 0.001");
/// let chance = Relation::new(0.11).significant(false);
/// let never = Relation::untested();
/// # let _ = (strong, chance, never);
/// ```
#[derive(Clone, Debug, PartialEq)]
#[must_use]
pub struct Relation {
    value: Option<f64>,
    significant: bool,
    note: Option<String>,
}

impl Relation {
    /// A pair that came to `value`, significant until told otherwise. A value that is
    /// not a finite number is a pair never tested.
    pub fn new(value: impl IntoValue) -> Self {
        let value = finite(value.into_value());
        Self {
            value,
            significant: value.is_some(),
            note: None,
        }
    }

    /// A pair never tested: too few paired days, say. Drawn almost blank.
    pub fn untested() -> Self {
        Self {
            value: None,
            significant: false,
            note: None,
        }
    }

    /// Whether the value is a real link and not chance. A pair that is not significant is
    /// drawn in a neutral tone, whatever its value.
    pub fn significant(mut self, yes: bool) -> Self {
        self.significant = yes && self.value.is_some();
        self
    }

    /// Words about the pair, shown in its tooltip and its table row: the days it rests on,
    /// the test.
    pub fn note(mut self, text: impl Into<String>) -> Self {
        self.note = Some(text.into());
        self
    }
}

/// What a cell shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Look {
    /// A significant link, with the step of its magnitude, 1 to 6.
    Linked {
        positive: bool,
        step: u8,
    },
    NotSignificant,
    Untested,
}

/// The step of a magnitude, 1 to 6: `share` is the value over the value that peaks.
fn step_of(share: f64) -> u8 {
    match share.abs() {
        s if s < 0.15 => 1,
        s if s < 0.30 => 2,
        s if s < 0.45 => 3,
        s if s < 0.60 => 4,
        s if s < 0.80 => 5,
        _ => 6,
    }
}

/// A matrix heat map: a row and a column for each measure, a cell for each pair. Colour
/// says the sign (one tone for a link that moves together, another for one that moves
/// apart) and the depth of the colour says how strong it is. A pair whose link is not
/// significant is neutral, and a pair never tested is almost blank, so neither is
/// mistaken for no link or for a strong one.
///
/// The cells are in the page with the tooltip's words, the arrow keys move between them,
/// and the pairs are a table: pair, value, result, note. The row and column of the cell
/// under the pointer or the keys are highlighted, and the tooltip names the pair in full
/// where the headers are cut.
///
/// ```
/// use navree_ui::charts::{MatrixHeatmap, Relation};
///
/// let chart = MatrixHeatmap::new("What moves together")
///     .description("Last 30 days, Gurpreet Singh")
///     .metrics(["Sleep", "Stress", "Steps", "HRV"])
///     .relation("Sleep", "Stress", Relation::new(-0.58).note("30 days"))
///     .relation("Sleep", "HRV", Relation::new(0.71))
///     .relation("Stress", "HRV", Relation::new(-0.34).significant(false))
///     .lower_triangle()
///     .value_name("Correlation");
/// # let _ = navree_ui::render(&chart.into());
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct MatrixHeatmap {
    common: Common,
    metrics: Vec<String>,
    relations: BTreeMap<(usize, usize), Relation>,
    lower: bool,
    positive: Ink,
    negative: Ink,
    peak: f64,
    format: NumberFormat,
    name: Option<String>,
    values: bool,
    words: GridLabels,
    motion: Motion,
}

common_methods!(MatrixHeatmap);

impl MatrixHeatmap {
    /// A matrix called `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            common: Common::new(title.into()),
            metrics: Vec::new(),
            relations: BTreeMap::new(),
            lower: false,
            positive: Ink::slot(1),
            negative: Ink::slot(2),
            peak: 1.0,
            format: NumberFormat::decimals(2).fixed(),
            name: None,
            values: false,
            words: GridLabels::english(),
            motion: Motion::none(),
        }
    }

    /// Adds the measures, in the order of the rows and columns.
    pub fn metrics<S: Into<String>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        for name in names {
            let name = name.into();
            if !self.metrics.contains(&name) {
                self.metrics.push(name);
            }
        }
        self
    }

    /// How measures `a` and `b` relate. The pair is the same either way round. A name that
    /// is not a measure, or the same name twice, is ignored; a pair given twice keeps the
    /// last.
    pub fn relation(mut self, a: &str, b: &str, relation: Relation) -> Self {
        let find = |name: &str| self.metrics.iter().position(|m| m == name);
        if let (Some(a), Some(b)) = (find(a), find(b))
            && a != b
        {
            self.relations.insert((a.min(b), a.max(b)), relation);
        }
        self
    }

    /// Draws each pair once, below the diagonal, so the matrix is a staircase: the
    /// first measure heads the columns and the last heads the rows.
    pub fn lower_triangle(mut self) -> Self {
        self.lower = true;
        self
    }

    /// The colours of a link that moves together and of one that moves apart: blue and
    /// orange by default, which stay apart for every reader. Use tones when the sign means
    /// good and bad.
    pub fn colors(mut self, positive: impl Into<Ink>, negative: impl Into<Ink>) -> Self {
        self.positive = positive.into();
        self.negative = negative.into();
        self
    }

    /// The value at which the colour is deepest (1 by default, for a correlation). A
    /// larger value is as deep as it.
    pub fn peak(mut self, value: impl IntoValue) -> Self {
        if let Some(v) = finite(value.into_value()).filter(|v| *v > 0.0) {
            self.peak = v;
        }
        self
    }

    /// How values are written (two decimals by default).
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.format = format;
        self
    }

    /// What the value is called in the tooltip and the table (`Value` by default).
    pub fn value_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Writes each pair's value in its cell. Meant for a matrix of a few measures; cells
    /// are too small for it past eight or so.
    pub fn show_values(mut self) -> Self {
        self.values = true;
        self
    }

    /// The words the chart writes.
    pub fn words(mut self, words: GridLabels) -> Self {
        self.words = words;
        self
    }

    /// How the rows come in: they fade in one after another. Off by default.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
        self
    }

    fn into_node(self) -> Node {
        let mut this = self;
        let drawn = this.draw();
        let height = (this.metrics.len().min(24) as u32 * 40 + 48).max(120);
        frame(this.common, drawn, "matrix-heatmap", height)
    }

    fn look(&self, a: usize, b: usize) -> (Look, Option<f64>, Option<&Relation>) {
        let relation = self.relations.get(&(a.min(b), a.max(b)));
        let value = relation.and_then(|r| r.value);
        let look = match (relation, value) {
            (Some(r), Some(v)) if r.significant => Look::Linked {
                positive: v >= 0.0,
                step: step_of(v / self.peak),
            },
            (Some(_), Some(_)) => Look::NotSignificant,
            _ => Look::Untested,
        };
        (look, value, relation)
    }

    fn draw(&mut self) -> Option<Drawn> {
        let n = self.metrics.len();
        if n < 2 {
            return None;
        }
        let words = self.words.clone();
        let numbers = self.common.numbers.clone();
        let fmt = |v: f64| self.format.format(v, &numbers);
        let name = self.name.clone().unwrap_or_else(|| words.value.to_string());
        let pair_name = |a: usize, b: usize| {
            fill(
                &words.pair_name,
                &[("a", &self.metrics[a]), ("b", &self.metrics[b])],
            )
        };
        let (row_range, col_count) = if self.lower { (1..n, n - 1) } else { (0..n, n) };

        let mut header: Vec<Node> = vec![el("span").slot("heat-corner").into()];
        for c in 0..col_count {
            header.push(
                el("span")
                    .slot("heat-colhead")
                    .attr("data-hc", c as i64)
                    .attr("title", self.metrics[c].clone())
                    .child(self.metrics[c].clone())
                    .into(),
            );
        }
        let mut lines: Vec<Node> = vec![el("div").slot("heat-row").child(header).into()];
        let mut table_rows = Vec::new();
        let mut index = 0_usize;
        let mut tested = 0_usize;
        let mut significant = 0_usize;
        let mut strongest: Option<(usize, usize, f64)> = None;

        for (line, r) in row_range.enumerate() {
            let mut cells: Vec<Node> = vec![
                el("span")
                    .slot("heat-rowhead")
                    .attr("data-hr", line as i64)
                    .attr("title", self.metrics[r].clone())
                    .child(self.metrics[r].clone())
                    .into(),
            ];
            let upto = if self.lower { r } else { n };
            for c in 0..upto {
                if c == r {
                    cells.push(
                        el("div")
                            .slot("heat-cell")
                            .attr("data-result", "self")
                            .into(),
                    );
                    continue;
                }
                let (look, value, relation) = self.look(r, c);
                let mut cell = el("div")
                    .slot("heat-cell")
                    .attr("data-i", index as i64)
                    .attr("data-r", line as i64)
                    .attr("data-c", c as i64);
                let (result, tone) = match look {
                    Look::Linked { positive, step } => {
                        let ink = if positive {
                            self.positive
                        } else {
                            self.negative
                        };
                        cell = ink
                            .on(cell)
                            .attr("data-result", if positive { "pos" } else { "neg" })
                            .attr("data-m", i64::from(step));
                        (
                            if positive {
                                &words.positive
                            } else {
                                &words.negative
                            },
                            Some(ink),
                        )
                    }
                    Look::NotSignificant => {
                        cell = cell.attr("data-result", "ns");
                        (&words.not_significant, None)
                    }
                    Look::Untested => {
                        cell = cell.attr("data-result", "none");
                        (&words.untested, None)
                    }
                };
                if self.values
                    && let Some(v) = value
                {
                    cell = cell.child(fmt(v));
                }
                cells.push(cell.into());

                // A pair counts once, from the half below the diagonal, whichever the matrix draws.
                if c < r {
                    if value.is_some() {
                        tested += 1;
                    }
                    if matches!(look, Look::Linked { .. }) {
                        significant += 1;
                        if let Some(v) = value
                            && strongest.is_none_or(|(_, _, best)| v.abs() > best.abs())
                        {
                            strongest = Some((r, c, v));
                        }
                    }
                }

                let value_node: Node = match (value, tone) {
                    (Some(v), Some(ink)) => ink.on(el("span")).child(fmt(v)).into(),
                    (Some(v), None) => fmt(v).into(),
                    (None, _) => value_cell(None, &self.common.labels),
                };
                table_rows.push((
                    pair_name(r, c),
                    vec![
                        value_node,
                        Node::from(result.clone()),
                        match relation.and_then(|rel| rel.note.clone()) {
                            Some(note) => Node::from(note),
                            None => value_cell(None, &self.common.labels),
                        },
                    ],
                ));
                index += 1;
            }
            lines.push(
                self.motion
                    .apply(el("div").slot("heat-row"), Effect::Fade, line)
                    .child(cells)
                    .into(),
            );
        }

        let pairs = n * (n - 1) / 2;
        // In the full matrix every pair is drawn twice; the count above took the lower half.
        let mut summary = fill(
            &words.matrix_summary,
            &[
                ("metrics", &count(n, &numbers)),
                ("pairs", &count(pairs, &numbers)),
                ("tested", &count(tested, &numbers)),
                ("significant", &count(significant, &numbers)),
            ],
        );
        if let Some((r, c, v)) = strongest {
            summary.push_str(&fill(
                &words.matrix_strongest,
                &[("pair", &pair_name(r, c)), ("value", &fmt(v))],
            ));
        }

        let key = |ink: Option<Ink>, result: &'static str, text: String| -> Node {
            let mut dot = swatch(el("span")).attr("data-result", result);
            if let Some(ink) = ink {
                dot = ink.on(dot).attr("data-m", 5_i64);
            }
            el("li").children((dot, text)).into()
        };
        let legend = heat_legend(
            None,
            vec![
                key(Some(self.positive), "pos", words.positive.to_string()),
                key(Some(self.negative), "neg", words.negative.to_string()),
                key(None, "ns", words.not_significant.to_string()),
                key(None, "none", words.untested.to_string()),
            ],
        );

        let table = TableData {
            head: vec![
                (words.pair.to_string(), None),
                (name.clone(), None),
                (words.result.to_string(), None),
                (words.note.to_string(), None),
            ],
            rows: table_rows,
            links: Vec::new(),
            note: None,
        };
        let height = (n.min(24) as u32 * 40 + 48).max(120);
        let body = el("div")
            .slot("heat-matrix")
            .uses(Uses::CHART_GRIDS)
            .child(lines);
        Some(Drawn {
            legend: Some(legend),
            body: body.into(),
            body_attrs: vec![
                ("data-grid", AttrValue::Flag),
                ("data-rows", AttrValue::Int(index as i64)),
            ],
            summary,
            table,
            height,
            note: None,
            interactive: true,
            tip: Some(Tip {
                big: false,
                lines: vec![
                    TipLine {
                        name,
                        unit: None,
                        color: None,
                        key: true,
                    },
                    TipLine {
                        name: words.result.to_string(),
                        unit: None,
                        color: None,
                        key: false,
                    },
                    TipLine {
                        name: words.note.to_string(),
                        unit: None,
                        color: None,
                        key: false,
                    },
                ],
            }),
        })
    }
}

crate::node::into_node!(MatrixHeatmap);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    fn six() -> MatrixHeatmap {
        MatrixHeatmap::new("Correlations")
            .id("m")
            .metrics(["Sleep", "Stress", "Steps", "HRV", "Resting HR", "Mood"])
            .relation("Sleep", "Stress", Relation::new(-0.62))
            .relation("Sleep", "HRV", Relation::new(0.71))
            .relation("Stress", "Mood", Relation::new(-0.35).significant(false))
            .relation("Steps", "Mood", Relation::new(0.4).note("30 days"))
    }

    #[test]
    fn a_staircase_has_a_cell_for_each_pair_once() {
        let html = render(&six().lower_triangle().into());
        // Six measures: 15 pairs, in 5 rows of 1 to 5 cells.
        assert_eq!(
            html.matches(r#"data-slot="heat-cell""#).count(),
            15,
            "{html}"
        );
        assert_eq!(
            html.matches(r#"data-slot="heat-row""#).count(),
            6,
            "the header row and five"
        );
        assert!(html.contains(r#"data-rows="15""#));
        // The first row heads Stress against Sleep; the last column is the fifth measure.
        assert!(
            html.contains(r#"data-slot="heat-rowhead" data-hr="0" title="Stress""#),
            "{html}"
        );
        assert!(
            html.contains(r#"data-slot="heat-colhead" data-hc="4" title="Resting HR""#),
            "{html}"
        );
        // Row-major, so the table follows the cells.
        let first_pair = html.find("Stress ↔ Sleep").expect("pair");
        let second_pair = html.find("Steps ↔ Sleep").expect("pair");
        assert!(first_pair < second_pair);
    }

    #[test]
    fn the_full_matrix_has_a_blank_diagonal_and_every_pair_twice() {
        let html = render(&six().into());
        assert_eq!(html.matches(r#"data-result="self""#).count(), 6);
        assert_eq!(html.matches(r#"data-i=""#).count(), 30);
        // The self cells are not part of the keys or the table.
        assert!(html.contains(r#"data-rows="30""#));
    }

    #[test]
    fn colour_says_sign_depth_says_strength_and_two_states_stay_neutral() {
        let html = render(&six().lower_triangle().into());
        let cell = |pair: &str| -> String {
            // Find the cell by its table row's position.
            let at = html
                .find(&format!("<th scope=\"row\">{pair}</th>"))
                .expect(pair);
            let rows_before = html[..at].matches("<th scope=\"row\">").count();
            let cells: Vec<&str> = html
                .split(r#"<div data-slot="heat-cell""#)
                .skip(1)
                .collect();
            let cell = cells[rows_before];
            cell[..cell.find('>').unwrap()].to_owned()
        };
        // -0.62: the negative tone (orange, 2), the fifth step.
        assert!(
            cell("Stress ↔ Sleep").contains(r#"data-color="2" data-result="neg" data-m="5""#),
            "{}",
            cell("Stress ↔ Sleep")
        );
        // 0.71 positive: the blue, the fifth step. 0.4 is the third step.
        assert!(cell("HRV ↔ Sleep").contains(r#"data-color="1" data-result="pos" data-m="5""#));
        assert!(cell("Mood ↔ Steps").contains(r#"data-result="pos" data-m="3""#));
        // Not significant is neutral whatever its value, and a pair with no relation is untested.
        assert!(cell("Mood ↔ Stress").contains(r#"data-result="ns""#));
        assert!(!cell("Mood ↔ Stress").contains("data-color"));
        assert!(cell("Steps ↔ Stress").contains(r#"data-result="none""#));
        assert!(html.contains("30 days"));
        assert!(html.contains("Not significant") && html.contains("Not tested"));
    }

    #[test]
    fn the_summary_counts_pairs_and_names_the_strongest() {
        let html = render(&six().lower_triangle().into());
        assert!(
            html.contains(
                "6 measures, 15 pairs: 4 tested, 3 significant. Strongest: HRV ↔ Sleep, 0.71."
            ),
            "{html}"
        );
        // The full matrix counts the same pairs.
        let full = render(&six().into());
        assert!(
            full.contains("4 tested, 3 significant. Strongest: HRV ↔ Sleep, 0.71."),
            "{full}"
        );
    }

    #[test]
    fn values_in_cells_are_opt_in_and_use_the_locales_marks() {
        let plain = render(&six().lower_triangle().into());
        assert!(!plain.contains(r#"data-result="pos" data-m="5">0.71"#));
        let shown = render(&six().lower_triangle().show_values().locale("de").into());
        assert!(shown.contains(">0,71</div>"), "{shown}");
    }

    #[test]
    fn odd_input_is_ignored_or_empty() {
        assert!(render(&MatrixHeatmap::new("x").into()).contains(r#"data-state="empty""#));
        assert!(
            render(&MatrixHeatmap::new("x").metrics(["a"]).into())
                .contains(r#"data-state="empty""#)
        );
        let html = render(
            &MatrixHeatmap::new("x")
                .metrics(["a", "b", "a"])
                .relation("a", "a", Relation::new(1))
                .relation("a", "zzz", Relation::new(1))
                .relation("a", "b", Relation::new(f64::NAN))
                .relation("b", "a", Relation::new(99))
                .into(),
        );
        assert!(!html.contains("NaN"), "{html}");
        // The value is past the peak: as deep as it goes, and no deeper.
        assert!(html.contains(r#"data-m="6""#), "{html}");
        assert!(!html.contains("style="));
    }
}
