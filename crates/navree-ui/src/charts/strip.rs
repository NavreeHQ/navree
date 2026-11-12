//! [`ColorStrip`]: a sequence of equal-width cells, each in the colour of its category.

use super::format::NumberFormat;
use super::linear::{Entry, LinearLabels, default_ink, legend_list, say, share_text};
use super::motion::{Effect, Motion};
use super::paint::{ColorScale, Ink};
use super::value::{IntoValue, finite};
use super::{
    Common, Drawn, TableData, Tip, TipLine, common_methods, fill, frame, pct, svg, value_cell,
};
use crate::node::{AttrValue, Node, Uses, el};

/// The most cells drawn. A longer strip draws every k-th cell and says so; the table
/// lists the cells drawn.
pub const MAX_CELLS: usize = 600;

/// A sequence of equal-width cells, each in the colour of its category: the zone every
/// heart-rate reading of a workout fell in, the stage every half hour of a night was
/// in, the state of every check of a service. Cells are equal in width, so the strip is
/// proportional to the number of readings, not to time. The legend names the categories
/// and can give each one's share of the cells.
///
/// Cells are given as a label and a category, or as a reading coloured by a
/// [`ColorScale`]:
///
/// ```
/// use navree_ui::charts::{ColorScale, ColorStrip, Ink, Motion};
/// use navree_ui::Tone;
///
/// let stages = ColorStrip::new("Sleep stages, every 30 minutes")
///     .category("Awake", Ink::slot(4))
///     .category("REM", Ink::tone(Tone::named("cyan")))
///     .category("Core", Ink::slot(3))
///     .category("Deep", Ink::slot(7))
///     .cell("11:00 pm", "Awake")
///     .cell("11:30 pm", "Core")
///     .cell("12:00 am", "Deep")
///     .cell("12:30 am", "Deep")
///     .cell("1:00 am", "REM");
///
/// let zones = ColorScale::new(Ink::slot(7))
///     .label("Rest")
///     .from(60, Ink::tone(Tone::Success))
///     .label("Fat burn")
///     .from(100, Ink::slot(4))
///     .label("Cardio");
/// let workout = ColorStrip::new("Heart rate zones")
///     .colors(zones)
///     .reading("6:00 pm", 54)
///     .reading("6:01 pm", 88)
///     .reading("6:02 pm", 121)
///     .motion(Motion::new());
/// # let _ = (navree_ui::render(&stages.into()), navree_ui::render(&workout.into()));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ColorStrip {
    common: Common,
    words: LinearLabels,
    categories: Vec<(String, Ink)>,
    cells: Vec<Cell>,
    scale: Option<ColorScale>,
    format: NumberFormat,
    height: u32,
    shares: bool,
    cell_name: Option<String>,
    value_name: Option<String>,
    motion: Motion,
}

#[derive(Clone, Debug)]
struct Cell {
    label: String,
    category: Option<String>,
    value: Option<f64>,
}

common_methods!(ColorStrip);

impl ColorStrip {
    /// A strip called `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            common: Common::new(title.into()),
            words: LinearLabels::english(),
            categories: Vec::new(),
            cells: Vec::new(),
            scale: None,
            format: NumberFormat::number(),
            height: 8,
            shares: false,
            cell_name: None,
            value_name: None,
            motion: Motion::none(),
        }
    }

    /// Declares a category in `ink`, which puts it in the legend in this order. A cell
    /// whose category was not declared adds one in the next of the categorical colours.
    pub fn category(mut self, name: impl Into<String>, ink: impl Into<Ink>) -> Self {
        let name = name.into();
        if !self.categories.iter().any(|(n, _)| *n == name) {
            self.categories.push((name, ink.into()));
        }
        self
    }

    /// Adds a cell called `label` (a time, a number) in `category`.
    pub fn cell(mut self, label: impl Into<String>, category: impl Into<String>) -> Self {
        self.cells.push(Cell {
            label: label.into(),
            category: Some(category.into()),
            value: None,
        });
        self
    }

    /// Adds cells from `(label, category)` pairs.
    pub fn cells<L: Into<String>, C: Into<String>>(
        mut self,
        cells: impl IntoIterator<Item = (L, C)>,
    ) -> Self {
        for (label, category) in cells {
            self = self.cell(label, category);
        }
        self
    }

    /// Adds a cell called `label` for a reading, in the colour of the band of the
    /// [`colors`](Self::colors) scale its value falls in. A missing reading is a cell in
    /// the quiet colour.
    pub fn reading(mut self, label: impl Into<String>, value: impl IntoValue) -> Self {
        self.cells.push(Cell {
            label: label.into(),
            category: None,
            value: finite(value.into_value()),
        });
        self
    }

    /// Adds readings from `(label, value)` pairs.
    pub fn readings<L: Into<String>, V: IntoValue>(
        mut self,
        readings: impl IntoIterator<Item = (L, V)>,
    ) -> Self {
        for (label, value) in readings {
            self = self.reading(label, value);
        }
        self
    }

    /// The scale that colours [readings](Self::reading). Name each of its bands with
    /// [`label`](ColorScale::label): the legend and the table use the names.
    pub fn colors(mut self, scale: ColorScale) -> Self {
        self.scale = Some(scale);
        self
    }

    /// How readings are written.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.format = format;
        self
    }

    /// The strip's thickness in pixels, 4 to 32 (8 by default).
    pub fn height(mut self, px: u32) -> Self {
        self.height = px.clamp(4, 32);
        self
    }

    /// Writes each category's share of the cells in the legend.
    pub fn legend_shares(mut self) -> Self {
        self.shares = true;
        self
    }

    /// The name of the table's first column (`Time`, the chart's word for it otherwise).
    pub fn cell_name(mut self, name: impl Into<String>) -> Self {
        self.cell_name = Some(name.into());
        self
    }

    /// The name of the table's column of readings (`Heart rate`).
    pub fn value_name(mut self, name: impl Into<String>) -> Self {
        self.value_name = Some(name.into());
        self
    }

    /// How the cells come in: they fade in from the start, one after another, within
    /// about a second however many there are. Off by default; always off for a reader who
    /// asks for less motion.
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
        frame(this.common, drawn, "color-strip", height)
    }

    fn draw(&mut self) -> Option<Drawn> {
        if self.cells.is_empty() {
            return None;
        }
        let labels = self.common.labels.clone();
        let numbers = self.common.numbers.clone();
        let words = self.words.clone();

        // The categories in the order declared, then in the order met.
        let mut cats: Vec<(String, Ink, usize)> = self
            .categories
            .iter()
            .map(|(n, i)| (n.clone(), *i, 0))
            .collect();
        // Each cell's category, as an index.
        let mut placed: Vec<usize> = Vec::with_capacity(self.cells.len());
        for cell in &self.cells {
            let (name, ink) = match (&cell.category, cell.value, &self.scale) {
                (Some(name), _, _) => (name.clone(), None),
                (None, Some(v), Some(scale)) => {
                    let (ink, label) = scale.band_of(v);
                    (
                        label.map_or_else(|| labels.value.to_string(), str::to_owned),
                        Some(ink),
                    )
                }
                (None, _, _) => (labels.no_value.to_string(), Some(Ink::muted())),
            };
            let at = match cats.iter().position(|(n, ..)| *n == name) {
                Some(at) => at,
                None => {
                    let ink = ink.unwrap_or_else(|| default_ink(cats.len()));
                    cats.push((name, ink, 0));
                    cats.len() - 1
                }
            };
            cats[at].2 += 1;
            placed.push(at);
        }

        // Past the most, every k-th cell stands for the ones between.
        let total = self.cells.len();
        let step = total.div_ceil(MAX_CELLS);
        let drawn: Vec<usize> = (0..total).step_by(step.max(1)).collect();
        let count = drawn.len();
        let h = self.height;
        let slice = 100.0 / count as f64;
        let rects: Vec<Node> = drawn
            .iter()
            .enumerate()
            .map(|(at, source)| {
                let rect = cats[placed[*source]].1.on(el("rect")
                    .slot("strip-cell")
                    .attr("data-i", at as i64)
                    .attr("x", pct(slice * at as f64))
                    .attr("width", pct(slice))
                    .attr("y", "0")
                    .attr("height", i64::from(h)));
                // However long the strip, the sweep takes about a second.
                let order = if count <= 24 { at } else { at * 24 / count };
                self.motion.apply(rect, Effect::Fade, order).into()
            })
            .collect();
        let drawing = svg()
            .slot("strip")
            .attr("width", "100%")
            .attr("height", i64::from(h))
            .uses(Uses::CHART_PAINT.with(Uses::CHART_LINEAR))
            .child(rects);

        let legend = legend_list(
            cats.iter()
                .map(|(name, ink, n)| Entry {
                    ink: *ink,
                    name: name.clone(),
                    value: self
                        .shares
                        .then(|| share_text(*n as f64 / total as f64, &numbers)),
                    index: None,
                })
                .collect(),
        );

        let has_values = self.cells.iter().any(|c| c.value.is_some());
        let cell_name = self
            .cell_name
            .clone()
            .unwrap_or_else(|| labels.time.to_string());
        let mut head = vec![(cell_name, None), (labels.category.to_string(), None)];
        if has_values {
            head.push((
                self.value_name
                    .clone()
                    .unwrap_or_else(|| labels.value.to_string()),
                None,
            ));
        }
        let rows: Vec<(String, Vec<Node>)> = drawn
            .iter()
            .map(|source| {
                let cell = &self.cells[*source];
                let (name, ink, _) = &cats[placed[*source]];
                let mut cells: Vec<Node> = vec![ink.on(el("span")).child(name.clone()).into()];
                if has_values {
                    cells.push(value_cell(
                        cell.value.map(|v| self.format.format(v, &numbers)),
                        &labels,
                    ));
                }
                (cell.label.clone(), cells)
            })
            .collect();

        let most = cats
            .iter()
            .max_by_key(|(_, _, n)| *n)
            .map(|(name, _, n)| (name.clone(), *n))
            .unwrap_or_default();
        let summary = say(
            &words.summary_strip,
            &[
                ("count", &total.to_string()),
                ("categories", &cats.len().to_string()),
                ("name", &most.0),
                ("share", &share_text(most.1 as f64 / total as f64, &numbers)),
            ],
        );

        let mut lines = vec![TipLine {
            name: labels.category.to_string(),
            unit: None,
            color: None,
            key: true,
        }];
        if has_values {
            lines.push(TipLine {
                name: self
                    .value_name
                    .clone()
                    .unwrap_or_else(|| labels.value.to_string()),
                unit: None,
                color: None,
                key: false,
            });
        }

        Some(Drawn {
            legend: None,
            body: Node::list((drawing, legend)),
            body_attrs: vec![
                ("data-rows", AttrValue::Int(count as i64)),
                ("data-float", AttrValue::Flag),
            ],
            summary,
            table: TableData {
                head,
                rows,
                links: Vec::new(),
                note: None,
            },
            height: h + 32,
            note: (count < total).then(|| {
                fill(
                    &labels.reduced,
                    &[("shown", &count.to_string()), ("total", &total.to_string())],
                )
            }),
            interactive: true,
            tip: Some(Tip { big: false, lines }),
        })
    }
}

crate::node::into_node!(ColorStrip);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Tone;
    use crate::render;

    fn stages() -> ColorStrip {
        ColorStrip::new("Sleep stages")
            .category("Awake", Ink::slot(4))
            .category("Core", Ink::slot(3))
            .category("Deep", Ink::slot(7))
            .cells([
                ("11:00 pm", "Awake"),
                ("11:30 pm", "Core"),
                ("12:00 am", "Deep"),
                ("12:30 am", "Core"),
            ])
    }

    fn zones() -> ColorScale {
        ColorScale::new(Ink::slot(7))
            .label("Rest")
            .from(60, Ink::tone(Tone::Success))
            .label("Fat burn")
            .from(100, Ink::slot(4))
            .label("Cardio")
    }

    fn html(strip: ColorStrip) -> String {
        render(&strip.into())
    }

    fn count(haystack: &str, needle: &str) -> usize {
        haystack.matches(needle).count()
    }

    #[test]
    fn cells_are_equal_in_width_and_run_edge_to_edge() {
        let out = html(stages());
        assert!(out.contains(r#"data-slot="strip-cell" data-i="0" x="0%" width="25%" y="0" height="8" data-color="4""#), "{out}");
        assert!(
            out.contains(r#"data-i="1" x="25%" width="25%" y="0" height="8" data-color="3""#),
            "{out}"
        );
        assert!(out.contains(r#"data-i="3" x="75%" width="25%""#), "{out}");
        assert_eq!(count(&out, r#"data-slot="strip-cell""#), 4);
        assert!(out.contains(r#"data-rows="4" data-float>"#), "{out}");
        let thin = ColorStrip::new("x")
            .cell("a", "A")
            .cell("b", "B")
            .cell("c", "C");
        assert!(html(thin).contains(r#"x="33.333%" width="33.333%""#));
    }

    #[test]
    fn the_legend_follows_the_declared_order_then_the_order_met() {
        let out = html(stages().cell("1:00 am", "REM").cell("1:30 am", "Core"));
        let names: Vec<&str> = out
            .split(r#"<span data-slot="chart-legend-name">"#)
            .skip(1)
            .map(|s| s.split('<').next().unwrap_or_default())
            .collect();
        assert_eq!(names, ["Awake", "Core", "Deep", "REM"], "{out}");
        // A category met and not declared takes the next categorical colour: the fourth.
        assert!(out.contains(r#"<span data-slot="chart-key" data-key="block" data-color="4" aria-hidden="true"></span><span data-slot="chart-legend-name">REM"#), "{out}");
        // Names alone by default; shares of the cells when asked.
        assert!(!out.contains("chart-legend-value"));
        let out = html(stages().legend_shares());
        assert!(
            out.contains(r#"<span data-slot="chart-legend-value">25%</span>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-slot="chart-legend-value">50%</span>"#),
            "{out}"
        );
    }

    #[test]
    fn a_declared_category_with_no_cell_is_still_named_with_a_share_of_nothing() {
        let out = html(stages().category("Naps", Ink::slot(1)).legend_shares());
        assert!(
            out.contains(">Naps</span><span data-slot=\"chart-legend-value\">0%</span>"),
            "{out}"
        );
    }

    #[test]
    fn readings_take_the_colour_and_the_name_of_their_band() {
        let out = html(
            ColorStrip::new("Heart rate zones")
                .colors(zones())
                .format(NumberFormat::number().suffix(" bpm"))
                .value_name("Heart rate")
                .readings([
                    ("6:00 pm", 54),
                    ("6:01 pm", 88),
                    ("6:02 pm", 121),
                    ("6:03 pm", 95),
                ]),
        );
        assert!(
            out.contains(r#"data-i="0" x="0%" width="25%" y="0" height="8" data-color="7""#),
            "{out}"
        );
        assert!(
            out.contains(r#"data-i="1" x="25%" width="25%" y="0" height="8" data-tone="success""#),
            "{out}"
        );
        assert!(
            out.contains(r#"data-i="2" x="50%" width="25%" y="0" height="8" data-color="4""#),
            "{out}"
        );
        assert!(out.contains(r#"<th scope="col">Time</th><th scope="col" data-num="">Category</th><th scope="col" data-num="">Heart rate</th>"#), "{out}");
        assert!(out.contains(r#"<th scope="row">6:02 pm</th><td data-num=""><span data-color="4">Cardio</span></td><td data-num="">121 bpm</td>"#), "{out}");
        // The bands are named in the legend, in the order the readings met them.
        assert!(
            out.contains(">Rest</span>")
                && out.contains(">Fat burn</span>")
                && out.contains(">Cardio</span>"),
            "{out}"
        );
        assert!(
            out.contains("4 readings in 3 categories. Most: Fat burn, 50%."),
            "{out}"
        );
    }

    #[test]
    fn a_missing_reading_is_a_cell_in_the_quiet_colour() {
        let out = html(
            ColorStrip::new("x")
                .colors(zones())
                .reading("a", 70)
                .reading("b", f64::NAN)
                .reading("c", None::<f64>),
        );
        assert!(
            out.contains(
                r#"data-i="1" x="33.333%" width="33.333%" y="0" height="8" data-color="other""#
            ),
            "{out}"
        );
        assert!(
            out.contains("<span data-color=\"other\">No value</span>"),
            "{out}"
        );
        assert!(
            out.contains(r#"<span aria-hidden="true">–</span><span data-sr="">No value</span>"#),
            "{out}"
        );
        // A band the scale doesn't name is called by the chart's word for a value.
        let unnamed = ColorScale::new(Ink::slot(1)).from(5, Ink::slot(2));
        let out = html(
            ColorStrip::new("x")
                .colors(unnamed)
                .reading("a", 1)
                .reading("b", 9),
        );
        assert!(
            out.contains("<span data-color=\"1\">Value</span>")
                && !out.contains("<span data-color=\"2\">Value</span>"),
            "{out}"
        );
    }

    #[test]
    fn height_motion_and_words_are_held() {
        assert!(html(stages().height(1)).contains(r#"height="4""#));
        assert!(html(stages().height(500)).contains(r#"height="32""#));
        let out = html(stages().cell_name("Half hour"));
        assert!(out.contains(r#"<th scope="col">Half hour</th>"#), "{out}");
    }

    #[test]
    fn cells_fade_in_across_about_a_second_however_many_there_are() {
        let mut strip = ColorStrip::new("Long").motion(Motion::new());
        for at in 0..300 {
            strip = strip.cell(format!("t{at}"), if at % 2 == 0 { "a" } else { "b" });
        }
        let out = html(strip);
        assert_eq!(count(&out, r#"data-motion="fade""#), 300);
        let cell = |i: usize| {
            let from = out
                .find(&format!(r#"data-slot="strip-cell" data-i="{i}""#))
                .expect("cell");
            let end = out[from..].find('>').expect("end");
            out[from..from + end].to_owned()
        };
        assert!(!cell(0).contains("data-delay"), "{}", cell(0));
        // Cell 150 of 300 is halfway through 24 steps of 30 ms: 12 x 30 ms = 360 ms = 7 steps.
        assert!(cell(150).contains(r#"data-delay="7""#), "{}", cell(150));
        assert!(cell(299).contains(r#"data-delay="14""#), "{}", cell(299));
        // A short strip fades its cells in one at a time.
        let out = html(stages().motion(Motion::new().stagger(100)));
        assert!(out.contains(r#"data-i="3" x="75%" width="25%" y="0" height="8" data-color="3" data-motion="fade" data-delay="6""#), "{out}");
        assert!(!html(stages()).contains("data-motion"));
    }

    #[test]
    fn a_very_long_strip_draws_every_kth_cell_and_says_so() {
        let mut strip = ColorStrip::new("Long");
        for at in 0..1_300 {
            strip = strip.cell(format!("t{at}"), "a");
        }
        let out = html(strip);
        // 1,300 cells: every third, 434 of them.
        assert_eq!(count(&out, r#"data-slot="strip-cell""#), 434, "{out}");
        assert!(
            out.contains("The drawing shows 434 of 1300 points; the table lists the points drawn."),
            "{out}"
        );
        assert!(
            out.contains(r#"<th scope="row">t3</th>"#)
                && !out.contains(r#"<th scope="row">t1</th>"#),
            "{out}"
        );
        assert!(out.contains("1300 readings in 1 categories."), "{out}");
        // Up to the most, nothing is left out.
        let mut exact = ColorStrip::new("Exact");
        for at in 0..MAX_CELLS {
            exact = exact.cell(format!("t{at}"), "a");
        }
        let out = html(exact);
        assert_eq!(count(&out, r#"data-slot="strip-cell""#), MAX_CELLS);
        assert!(!out.contains("The drawing shows"));
    }

    #[test]
    fn no_cells_is_an_empty_chart() {
        assert!(html(ColorStrip::new("x")).contains(r#"data-state="empty""#));
        assert!(
            html(ColorStrip::new("x").category("a", Ink::slot(1)))
                .contains(r#"data-state="empty""#)
        );
    }

    #[test]
    fn the_tooltip_names_the_category_and_the_reading() {
        let out = html(ColorStrip::new("x").colors(zones()).reading("a", 70));
        assert!(out.contains(r#"<div data-tip="row" data-s="0"><span data-slot="chart-key" data-key="line"></span><strong></strong><span>Category</span></div><div data-tip="row" data-s="1"><strong></strong><span>Value</span></div>"#), "{out}");
        let out = html(stages());
        assert!(!out.contains(r#"data-s="1""#), "{out}");
    }

    #[test]
    fn the_figure_is_a_named_group_with_its_sheets_and_no_style() {
        let out = html(stages());
        assert!(
            out.contains(r#"data-kind="color-strip""#)
                && out.contains(r#"role="group" aria-label="Sleep stages, chart""#),
            "{out}"
        );
        assert!(!out.contains("style="), "{out}");
        let uses = crate::View::new("x").body(stages()).uses();
        assert_eq!(
            uses,
            Uses::CHARTS
                .with(Uses::CHART_PAINT)
                .with(Uses::CHART_LINEAR)
        );
    }
}
