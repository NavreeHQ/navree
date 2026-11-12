//! [`RelationList`]: pairs of measures ranked by how strongly they move together.

use super::super::format::NumberFormat;
use super::super::motion::{Effect, Motion};
use super::super::paint::Ink;
use super::super::value::{IntoValue, finite};
use super::super::{Common, Drawn, TableData, common_methods, fill, frame, n, svg, value_cell};
use super::{GridLabels, Relation, count, heat_legend, swatch};
use crate::node::{AttrValue, Node, Uses, el};

/// The most rows a list draws; the table lists every pair.
pub const MAX_RELATIONS: usize = 50;

/// A ranked list of how pairs of measures relate: the findings of a set of correlations,
/// strongest first. Each row names the pair, writes its signed value, draws a bar from
/// the centre to the right for a link that moves together and to the left for one that
/// moves apart (longer the stronger, to the value at [`peak`](Self::peak)), and says its
/// note under it.
///
/// It is the list beside a [`MatrixHeatmap`](super::MatrixHeatmap), or in its place when
/// few of the pairs were tested: a matrix of eleven measures with ten pairs tested is
/// mostly blank, and a list of the ten reads better. A pair that is not significant is
/// drawn in a neutral tone and says so, and a pair never tested comes last with no bar;
/// [`only_significant`](Self::only_significant) leaves both out of the list (the table
/// keeps them).
///
/// The sign is written with the value and shown by the side of the bar, so the colour is
/// never alone. The rows are text in the page, in a list a screen reader walks (the body is a
/// group, not a picture, and the bars are hidden from it); the pairs are also a table (pair, value,
/// result, note), and the summary counts them and names the strongest.
///
/// ```
/// use navree_ui::charts::{Relation, RelationList};
///
/// let found = RelationList::new("What moves together")
///     .description("Last 30 days, Harpreet Kaur")
///     .relation("Sleep", "Next-day HRV", Relation::new(0.62).note("30 days, p < 0.001"))
///     .relation("Steps", "Stress", Relation::new(-0.48).note("28 days, p = 0.01"))
///     .relation("Caffeine", "Deep sleep", Relation::new(-0.21).significant(false))
///     .value_name("Correlation (r)");
/// # let _ = navree_ui::render(&found.into());
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct RelationList {
    common: Common,
    pairs: Vec<(String, String, Relation)>,
    positive: Ink,
    negative: Ink,
    peak: f64,
    format: NumberFormat,
    name: Option<String>,
    only_significant: bool,
    words: GridLabels,
    motion: Motion,
}

common_methods!(RelationList);

impl RelationList {
    /// A list called `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            common: Common::new(title.into()),
            pairs: Vec::new(),
            positive: Ink::slot(1),
            negative: Ink::slot(2),
            peak: 1.0,
            format: NumberFormat::decimals(2).fixed(),
            name: None,
            only_significant: false,
            words: GridLabels::english(),
            motion: Motion::none(),
        }
    }

    /// How measures `a` and `b` relate. The rows are ranked by the size of the value,
    /// whatever order they are given in.
    pub fn relation(
        mut self,
        a: impl Into<String>,
        b: impl Into<String>,
        relation: Relation,
    ) -> Self {
        self.pairs.push((a.into(), b.into(), relation));
        self
    }

    /// The colours of a link that moves together and of one that moves apart: blue and
    /// orange by default, which stay apart for every reader. Use tones when the sign
    /// means good and bad.
    pub fn colors(mut self, positive: impl Into<Ink>, negative: impl Into<Ink>) -> Self {
        self.positive = positive.into();
        self.negative = negative.into();
        self
    }

    /// The value a bar reaches the end at (1 by default, for a correlation). A larger
    /// value is as long as it.
    pub fn peak(mut self, value: impl IntoValue) -> Self {
        if let Some(v) = finite(value.into_value()).filter(|v| *v > 0.0) {
            self.peak = v;
        }
        self
    }

    /// How values are written (two decimals by default). A value above zero is written
    /// with its `+`.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.format = format;
        self
    }

    /// What the value is called in the table (`Value` by default).
    pub fn value_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Lists only the significant pairs. The table still has every pair.
    pub fn only_significant(mut self) -> Self {
        self.only_significant = true;
        self
    }

    /// The words the list writes.
    pub fn words(mut self, words: GridLabels) -> Self {
        self.words = words;
        self
    }

    /// How the rows come in: each bar grows from the centre, one after another. Off by
    /// default.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
        self
    }

    fn into_node(self) -> Node {
        let rows = self.pairs.len().min(MAX_RELATIONS) as u32;
        let drawn = self.draw();
        frame(self.common, drawn, "relation-list", (rows * 56).max(120))
    }

    /// A value as written: with its sign.
    fn text(&self, v: f64) -> String {
        let text = self.format.format(v, &self.common.numbers);
        if v > 0.0 && !text.starts_with('+') && text.bytes().any(|b| matches!(b, b'1'..=b'9')) {
            format!("+{text}")
        } else {
            text
        }
    }

    fn draw(&self) -> Option<Drawn> {
        if self.pairs.is_empty() {
            return None;
        }
        let words = &self.words;
        let labels = &self.common.labels;
        let numbers = &self.common.numbers;
        let name = self.name.clone().unwrap_or_else(|| words.value.to_string());
        // Strongest first; a pair never tested last; a tie keeps the order given.
        let mut ranked: Vec<&(String, String, Relation)> = self.pairs.iter().collect();
        ranked.sort_by(|a, b| {
            let size = |r: &Relation| r.value.map_or(-1.0, f64::abs);
            size(&b.2).total_cmp(&size(&a.2))
        });
        let pair_name = |a: &str, b: &str| fill(&words.pair_name, &[("a", a), ("b", b)]);

        let mut rows: Vec<Node> = Vec::new();
        let mut table_rows = Vec::new();
        let mut significant = 0_usize;
        for (a, b, relation) in &ranked {
            let value = relation.value;
            let linked = relation.significant && value.is_some();
            if linked {
                significant += 1;
            }
            let positive = value.is_some_and(|v| v >= 0.0);
            let (result, ink, kind) = match value {
                Some(_) if linked && positive => (&words.positive, Some(self.positive), "pos"),
                Some(_) if linked => (&words.negative, Some(self.negative), "neg"),
                Some(_) => (&words.not_significant, None, "ns"),
                None => (&words.untested, None, "none"),
            };
            let pair = pair_name(a, b);
            let value_text = value.map(|v| self.text(v));
            table_rows.push((
                pair.clone(),
                vec![
                    match (&value_text, ink) {
                        (Some(t), Some(ink)) => ink.on(el("span")).child(t.clone()).into(),
                        (Some(t), None) => Node::from(t.clone()),
                        (None, _) => value_cell(None, labels),
                    },
                    Node::from(result.to_string()),
                    value_cell(relation.note.clone(), labels),
                ],
            ));
            if self.only_significant && !linked || rows.len() >= MAX_RELATIONS {
                continue;
            }
            let at = rows.len();
            // The bar, in a box 200 wide with the centre at 100.
            let bar = value.map(|v| {
                let reach = (v.abs() / self.peak).min(1.0) * 100.0;
                let x = if positive { 100.0 } else { 100.0 - reach };
                let rect = el("rect")
                    .slot("relation-fill")
                    .attr("x", n(x, 2))
                    .attr("y", "0")
                    .attr("width", n(reach.max(0.5), 2))
                    .attr("height", "8");
                let rect = match ink {
                    Some(ink) => ink.on(rect),
                    None => rect,
                };
                let effect = if positive {
                    Effect::GrowRight
                } else {
                    Effect::GrowLeft
                };
                svg()
                    .slot("relation-bar")
                    .attr("viewBox", "0 0 200 8")
                    .attr("preserveAspectRatio", "none")
                    .children((
                        el("rect")
                            .slot("relation-track")
                            .attr("width", "200")
                            .attr("height", "8"),
                        self.motion.stagger(60).apply(rect, effect, at),
                        el("line")
                            .slot("relation-centre")
                            .attr("x1", "100")
                            .attr("x2", "100")
                            .attr("y1", "-2")
                            .attr("y2", "10"),
                    ))
            });
            let note = match (kind, &relation.note) {
                ("ns" | "none", Some(note)) => Some(format!("{result} \u{b7} {note}")),
                ("ns" | "none", None) => Some(result.to_string()),
                (_, note) => note.clone(),
            };
            let row = el("li")
                .slot("relation")
                .attr("data-result", kind)
                .attr("data-i", at as i64)
                .children((
                    el("span").slot("relation-pair").child(pair),
                    value_text.map(|t| {
                        let text = el("span").slot("relation-value").child(t);
                        match ink {
                            Some(ink) => ink.on(text),
                            None => text,
                        }
                    }),
                    bar,
                    note.map(|t| el("span").slot("relation-note").child(t)),
                ));
            rows.push(self.motion.stagger(60).apply(row, Effect::Fade, at).into());
        }
        let shown = rows.len();
        let total = self.pairs.len();
        // What the list could have held: with only the significant pairs wanted, those.
        let listable = if self.only_significant {
            significant
        } else {
            total
        };
        let mut summary = fill(
            &words.relations_summary,
            &[
                ("pairs", &count(total, numbers)),
                ("significant", &count(significant, numbers)),
            ],
        );
        if let Some((a, b, r)) = ranked
            .iter()
            .find(|(_, _, r)| r.significant && r.value.is_some())
        {
            summary.push_str(&fill(
                &words.matrix_strongest,
                &[
                    ("pair", &pair_name(a, b)),
                    ("value", &r.value.map(|v| self.text(v)).unwrap_or_default()),
                ],
            ));
        }
        let key = |ink: Option<Ink>, result: &'static str, text: String| -> Node {
            let mut dot = swatch(el("span")).attr("data-result", result);
            if let Some(ink) = ink {
                dot = ink.on(dot).attr("data-m", 6_i64);
            }
            el("li").children((dot, el("span").child(text))).into()
        };
        let legend = heat_legend(
            None,
            vec![
                key(Some(self.positive), "pos", words.positive.to_string()),
                key(Some(self.negative), "neg", words.negative.to_string()),
                key(None, "ns", words.not_significant.to_string()),
            ],
        );
        let note = (shown < listable).then(|| {
            fill(
                &words.relations_cut,
                &[
                    ("shown", &count(shown, numbers)),
                    ("total", &count(listable, numbers)),
                ],
            )
        });
        Some(Drawn {
            legend: Some(legend),
            body: el("ol")
                .slot("relation-list")
                // The rows are text; a reset list style must not drop the list.
                .attr("role", "list")
                // The frame, the legend and the table switch are the charts sheet's.
                .uses(Uses::CHART_GRIDS.with(Uses::CHARTS))
                .child(rows)
                .into(),
            body_attrs: vec![("data-rows", AttrValue::Int(shown as i64))],
            summary,
            table: TableData {
                head: vec![
                    (words.pair.to_string(), None),
                    (name, None),
                    (words.result.to_string(), None),
                    (words.note.to_string(), None),
                ],
                rows: table_rows,
                links: Vec::new(),
                note: None,
            },
            height: (shown as u32 * 56).max(120),
            note,
            interactive: false,
            text: true,
            tip: None,
        })
    }
}

crate::node::into_node!(RelationList);
