//! [`Heatmap`]: rows by columns, a cell for each, coloured by its reading or its category.

use super::super::format::{NumberFormat, share_text};
use super::super::motion::{Effect, Motion};
use super::super::paint::{ColorScale, Ink};
use super::super::value::{IntoValue, finite};
use super::super::{
    Common, Drawn, TableData, Tip, TipLine, column_name, common_methods, fill, frame, value_cell,
};
use super::{GridLabels, count, swatch};
use crate::node::{AttrValue, Element, Node, Uses, el};

/// The most rows a heat map draws: the first ones, when it was given more.
pub const MAX_HEAT_ROWS: usize = 50;

/// The most columns a heat map draws: a day in ten-minute slots. The first ones, when it
/// was given more.
pub const MAX_HEAT_COLUMNS: usize = 144;

/// What the end of each row of a [`Heatmap`] says about the row's readings.
///
/// ```
/// use navree_ui::charts::{Heatmap, RowSummary};
///
/// let sleep = Heatmap::new("Hours asleep")
///     .columns(["Mon", "Tue", "Wed"])
///     .row("Harleen Kaur", [7.5, 6.25, 8.0])
///     .row_summary(RowSummary::Average);
/// # let _ = navree_ui::render(&sleep.into());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum RowSummary {
    /// Nothing: the default.
    #[default]
    None,
    /// The average of the row's readings, the cells with no data left out.
    Average,
    /// The sum of the row's readings.
    Total,
}

/// What a cell holds.
#[derive(Clone, Debug, PartialEq)]
enum Cell {
    Empty,
    Value(f64),
    Category(String),
}

/// A heat map of rows by columns: a cell for each row at each column, such as each day at
/// each hour, each person in each week, or each store on each day. A cell holds a reading or
/// a category.
///
/// A reading is shaded in five steps of one [`Ink`] through the page's surface, from the
/// low end of the [range](Self::range) to the high end, or it takes the colour of the band
/// of a [`ColorScale`] it falls in ([`colors`](Self::colors)), which can run from one colour
/// through a neutral middle to another. A category takes the colour it was declared in. A
/// cell with nothing recorded is a hollow dashed cell, not the faintest step.
///
/// The grid has one legend for every row: the steps from one end of the range to the other,
/// the scale's named bands, the categories. The cells share the chart's width and keep their
/// height; past the width the grid scrolls sideways. Each cell has a tooltip, the arrow keys
/// move from cell to cell, and the numbers are a table of the same rows and columns.
///
/// ```
/// use navree_ui::charts::{Heatmap, Ink, NumberFormat, RowSummary};
///
/// let chart = Heatmap::new("Share of the hour spent moving")
///     .description("Simran Kaur, last week")
///     .columns(["6 am", "7 am", "8 am", "9 am", "10 am", "11 am"])
///     .row("Mon", [0.12, 0.48, 0.91, 0.97, 0.64, 0.30])
///     .row("Tue", [Some(0.05), Some(0.10), Some(0.44), None, Some(0.88), Some(0.93)])
///     .range(0, 1)
///     .format(NumberFormat::percent())
///     .value_name("Moving")
///     .ink(Ink::slot(3))
///     .row_summary(RowSummary::Average);
/// # let _ = navree_ui::render(&chart.into());
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Heatmap {
    common: Common,
    columns: Vec<String>,
    rows: Vec<(String, Vec<Cell>)>,
    categories: Vec<(String, Ink)>,
    ink: Ink,
    range: Option<(f64, f64)>,
    scale: Option<ColorScale>,
    format: NumberFormat,
    name: Option<String>,
    unit: Option<String>,
    row_name: Option<String>,
    summary: RowSummary,
    ends: Option<(String, String)>,
    every: Option<usize>,
    values: bool,
    words: GridLabels,
    motion: Motion,
}

common_methods!(Heatmap);

impl Heatmap {
    /// A heat map called `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            common: Common::new(title.into()),
            columns: Vec::new(),
            rows: Vec::new(),
            categories: Vec::new(),
            ink: Ink::accent(),
            range: None,
            scale: None,
            format: NumberFormat::number(),
            name: None,
            unit: None,
            row_name: None,
            summary: RowSummary::None,
            ends: None,
            every: None,
            values: false,
            words: GridLabels::english(),
            motion: Motion::none(),
        }
    }

    /// Adds the columns' names, in order: the hours, the days, the weeks.
    pub fn columns<S: Into<String>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        self.columns.extend(names.into_iter().map(Into::into));
        self
    }

    /// Adds a row called `name` of readings, one for each column in order. A value that is
    /// not a finite number, or `None`, is a cell with no data; a row shorter than the columns
    /// has no data in the rest, and values past the last column are left out.
    pub fn row<V: IntoValue>(
        mut self,
        name: impl Into<String>,
        values: impl IntoIterator<Item = V>,
    ) -> Self {
        let cells = values
            .into_iter()
            .map(|value| match finite(value.into_value()) {
                Some(v) => Cell::Value(v),
                None => Cell::Empty,
            })
            .collect();
        self.rows.push((name.into(), cells));
        self
    }

    /// Adds a row called `name` of categories, one for each column in order: the kind of
    /// workout each day, the state of a store each hour. An empty name
    /// is a cell with no data. A category that was not [declared](Self::category) is added in
    /// the next of the categorical colours.
    pub fn category_row<S: AsRef<str>>(
        mut self,
        name: impl Into<String>,
        categories: impl IntoIterator<Item = S>,
    ) -> Self {
        let cells = categories
            .into_iter()
            .map(|category| {
                let category = category.as_ref().trim();
                if category.is_empty() {
                    return Cell::Empty;
                }
                if !self.categories.iter().any(|(n, _)| n == category) {
                    let ink = Ink::nth(self.categories.len());
                    self.categories.push((category.to_owned(), ink));
                }
                Cell::Category(category.to_owned())
            })
            .collect();
        self.rows.push((name.into(), cells));
        self
    }

    /// Declares a category in `ink`, which puts it in the legend in this order. Declare the
    /// categories before the rows that use them.
    pub fn category(mut self, name: impl Into<String>, ink: impl Into<Ink>) -> Self {
        let name = name.into();
        let ink = ink.into();
        match self.categories.iter_mut().find(|(n, _)| *n == name) {
            Some(found) => found.1 = ink,
            None => self.categories.push((name, ink)),
        }
        self
    }

    /// The colour the five steps of a reading are made of (the accent by default).
    pub fn ink(mut self, ink: impl Into<Ink>) -> Self {
        self.ink = ink.into();
        self
    }

    /// The readings at the two ends of the steps: `range(0, 1)` for a share. A reading past
    /// an end takes the step at that end. By default the lowest and the highest reading.
    pub fn range(mut self, low: impl IntoValue, high: impl IntoValue) -> Self {
        if let (Some(low), Some(high)) = (finite(low.into_value()), finite(high.into_value())) {
            self.range = Some((low.min(high), low.max(high)));
        }
        self
    }

    /// Colours each reading by the band of `scale` it falls in, instead of in steps: name
    /// each band with [`label`](ColorScale::label), and the legend names it. A band of a
    /// [soft](Ink::soft) ink is a light step of its colour, so a scale can run from one
    /// colour through a neutral middle to another.
    pub fn colors(mut self, scale: ColorScale) -> Self {
        self.scale = Some(scale);
        self
    }

    /// How readings are written in the tooltip, the table, the legend and the summary.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.format = format;
        self
    }

    /// What a reading is called in the tooltip (the title by default).
    pub fn value_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// What a reading counts, written after it: `steps`, `min`, `orders`.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// The name of the table's first column, which names the rows (`Day`); the chart's word
    /// for a category otherwise.
    pub fn row_name(mut self, name: impl Into<String>) -> Self {
        self.row_name = Some(name.into());
        self
    }

    /// Writes the average or the total of each row's readings at its end, and in the table.
    pub fn row_summary(mut self, summary: RowSummary) -> Self {
        self.summary = summary;
        self
    }

    /// The words at the two ends of the legend's steps (`Still`, `Active`); the ends of the
    /// range, written as readings, by default.
    pub fn ends(mut self, low: impl Into<String>, high: impl Into<String>) -> Self {
        self.ends = Some((low.into(), high.into()));
        self
    }

    /// Names every `n`th column above the grid, from the first. By default as many as fit
    /// about sixteen names across. The tooltip and the table name every column.
    pub fn label_every(mut self, n: usize) -> Self {
        self.every = Some(n.max(1));
        self
    }

    /// Writes each reading in its cell. Meant for a grid of a few columns: past twelve or
    /// so, the cells are too small for it.
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
        let height = height_of(this.rows.len().min(MAX_HEAT_ROWS));
        frame(this.common, drawn, "heatmap", height)
    }

    fn draw(&mut self) -> Option<Drawn> {
        let total_rows = self.rows.len();
        let total_columns = self.columns.len();
        let rows = &self.rows[..total_rows.min(MAX_HEAT_ROWS)];
        let columns = &self.columns[..total_columns.min(MAX_HEAT_COLUMNS)];
        let cell_of = |cells: &[Cell], c: usize| cells.get(c).cloned().unwrap_or(Cell::Empty);
        let has_data = rows
            .iter()
            .any(|(_, cells)| (0..columns.len()).any(|c| cell_of(cells, c) != Cell::Empty));
        if columns.is_empty() || !has_data {
            return None;
        }
        let words = self.words.clone();
        let labels = self.common.labels.clone();
        let numbers = self.common.numbers.clone();
        let fmt = |v: f64| self.format.format(v, &numbers);
        let with_unit = |v: f64| match &self.unit {
            Some(unit) => format!("{} {unit}", fmt(v)),
            None => fmt(v),
        };

        let readings: Vec<f64> = rows
            .iter()
            .flat_map(|(_, cells)| {
                (0..columns.len()).filter_map(move |c| match cells.get(c) {
                    Some(Cell::Value(v)) => Some(*v),
                    _ => None,
                })
            })
            .collect();
        let (low, high) = self.range.unwrap_or_else(|| {
            let low = readings.iter().copied().fold(f64::INFINITY, f64::min);
            let high = readings.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            (low, high)
        });
        let ink_of = |name: &str| {
            self.categories
                .iter()
                .find(|(n, _)| n == name)
                .map_or_else(Ink::muted, |(_, ink)| *ink)
        };
        // A reading's paint: the step of the grid's ink, or its band's own ink at full strength.
        let paint = |cell: Element, v: f64| -> Element {
            match &self.scale {
                Some(scale) => scale.ink_of(v).on(cell).attr("data-level", "4"),
                None => cell.attr("data-level", level_of(v, low, high)),
            }
        };

        let summary_name = match self.summary {
            RowSummary::None => None,
            RowSummary::Average => Some(words.average.to_string()),
            RowSummary::Total => Some(words.total.to_string()),
        };
        let every = self
            .every
            .unwrap_or_else(|| columns.len().div_ceil(16))
            .max(1);
        let mut header: Vec<Node> = vec![el("span").slot("heat-corner").into()];
        for (c, name) in columns.iter().enumerate() {
            header.push(
                el("span")
                    .slot("heat-colhead")
                    .attr("data-hc", c as i64)
                    .attr("title", name.clone())
                    .child((c % every == 0).then(|| name.clone()))
                    .into(),
            );
        }
        if let Some(name) = &summary_name {
            header.push(el("span").slot("heat-rowend").child(name.clone()).into());
        }
        let mut lines: Vec<Node> = vec![el("div").slot("heat-row").child(header).into()];

        let mut table_rows: Vec<(String, Vec<Node>)> = Vec::with_capacity(rows.len());
        let mut index = 0_usize;
        let mut missing = 0_usize;
        let mut counts: Vec<usize> = vec![0; self.categories.len()];
        let mut highest: Option<(f64, usize, usize)> = None;
        let mut lowest: Option<(f64, usize, usize)> = None;
        for (r, (name, cells)) in rows.iter().enumerate() {
            let mut drawn: Vec<Node> = vec![
                el("span")
                    .slot("heat-rowhead")
                    .attr("data-hr", r as i64)
                    .attr("title", name.clone())
                    .child(name.clone())
                    .into(),
            ];
            let mut table_cells: Vec<Node> = Vec::with_capacity(columns.len() + 1);
            let mut row_readings: Vec<f64> = Vec::new();
            for c in 0..columns.len() {
                let cell = el("div")
                    .slot("heat-cell")
                    .attr("data-i", index as i64)
                    .attr("data-r", r as i64)
                    .attr("data-c", c as i64);
                index += 1;
                match cell_of(cells, c) {
                    Cell::Empty => {
                        missing += 1;
                        drawn.push(cell.attr("data-level", "none").into());
                        table_cells.push(
                            Ink::muted()
                                .on(el("span"))
                                .child(words.no_data.clone())
                                .into(),
                        );
                    }
                    Cell::Value(v) => {
                        row_readings.push(v);
                        if highest.is_none_or(|(best, ..)| v > best) {
                            highest = Some((v, r, c));
                        }
                        if lowest.is_none_or(|(least, ..)| v < least) {
                            lowest = Some((v, r, c));
                        }
                        let cell = paint(cell, v);
                        let cell = if self.values {
                            cell.child(fmt(v))
                        } else {
                            cell
                        };
                        drawn.push(cell.into());
                        // Every value carries an ink, so a tooltip never keeps the last cell's colour.
                        let ink = self.scale.as_ref().map_or(self.ink, |s| s.ink_of(v));
                        table_cells.push(ink.on(el("span")).child(with_unit(v)).into());
                    }
                    Cell::Category(category) => {
                        if let Some(at) = self.categories.iter().position(|(n, _)| *n == category) {
                            counts[at] += 1;
                        }
                        let ink = ink_of(&category);
                        drawn.push(ink.on(cell).attr("data-level", "4").into());
                        table_cells.push(ink.on(el("span")).child(category).into());
                    }
                }
            }
            if self.summary != RowSummary::None {
                let figure = (!row_readings.is_empty()).then(|| {
                    let sum: f64 = row_readings.iter().sum();
                    match self.summary {
                        RowSummary::Average => sum / row_readings.len() as f64,
                        _ => sum,
                    }
                });
                let text = figure.map(with_unit);
                drawn.push(
                    el("span")
                        .slot("heat-rowend")
                        .child(text.clone().unwrap_or_else(|| "–".to_owned()))
                        .into(),
                );
                table_cells.push(value_cell(text, &labels));
            }
            lines.push(
                self.motion
                    .apply(el("div").slot("heat-row"), Effect::Fade, r)
                    .child(drawn)
                    .into(),
            );
            table_rows.push((name.clone(), table_cells));
        }

        // One legend for the whole grid: the steps or the bands, the categories, and no data.
        let mut keys: Vec<Node> = Vec::new();
        if !readings.is_empty() {
            match &self.scale {
                Some(scale) => {
                    for (ink, label) in scale.labelled() {
                        keys.push(key(
                            ink.on(swatch(el("span"))).attr("data-level", "4"),
                            label,
                        ));
                    }
                }
                None => {
                    let (from, to) = self
                        .ends
                        .clone()
                        .unwrap_or_else(|| (with_unit(low), with_unit(high.max(low))));
                    let steps: Vec<Node> = ["0", "1", "2", "3", "4"]
                        .into_iter()
                        .map(|step| swatch(el("span")).attr("data-level", step).into())
                        .collect();
                    keys.push(
                        self.ink
                            .on(el("li"))
                            .children((
                                el("span").slot("heat-legend-word").child(from),
                                steps,
                                el("span").slot("heat-legend-word").child(to),
                            ))
                            .into(),
                    );
                }
            }
        }
        let used = rows.iter().any(|(_, cells)| {
            cells
                .iter()
                .take(columns.len())
                .any(|cell| matches!(cell, Cell::Category(_)))
        });
        if used {
            for (name, ink) in &self.categories {
                keys.push(key(
                    ink.on(swatch(el("span"))).attr("data-level", "4"),
                    name.clone(),
                ));
            }
        }
        if missing > 0 {
            keys.push(key(
                swatch(el("span")).attr("data-level", "none"),
                words.no_data.to_string(),
            ));
        }
        let legend = el("ul")
            .slot("heat-legend")
            .attr("aria-hidden", "true")
            .uses(Uses::CHART_GRIDS)
            .child(keys);

        let mut summary = fill(
            &words.heatmap_summary,
            &[
                ("rows", &count(rows.len(), &numbers)),
                ("columns", &count(columns.len(), &numbers)),
            ],
        );
        if let (Some((max, max_r, max_c)), Some((min, min_r, min_c))) = (highest, lowest) {
            summary.push_str(&fill(
                &words.heatmap_extremes,
                &[
                    ("max", &with_unit(max)),
                    ("max_row", &rows[max_r].0),
                    ("max_column", &columns[max_c]),
                    ("min", &with_unit(min)),
                    ("min_row", &rows[min_r].0),
                    ("min_column", &columns[min_c]),
                ],
            ));
        }
        let most = counts
            .iter()
            .enumerate()
            .filter(|(_, n)| **n > 0)
            .max_by_key(|(at, n)| (**n, std::cmp::Reverse(*at)));
        if let Some((at, n)) = most {
            summary.push_str(&fill(
                &words.heatmap_most,
                &[
                    ("name", &self.categories[at].0),
                    ("share", &share_text(*n as f64 / index as f64, &numbers)),
                ],
            ));
        }
        if missing > 0 {
            summary.push_str(&fill(
                &words.heatmap_missing,
                &[("missing", &count(missing, &numbers))],
            ));
        }

        let mut notes: Vec<String> = Vec::new();
        if total_rows > rows.len() {
            notes.push(fill(
                &words.heatmap_rows_cut,
                &[
                    ("shown", &count(rows.len(), &numbers)),
                    ("total", &count(total_rows, &numbers)),
                ],
            ));
        }
        if total_columns > columns.len() {
            notes.push(fill(
                &words.heatmap_columns_cut,
                &[
                    ("shown", &count(columns.len(), &numbers)),
                    ("total", &count(total_columns, &numbers)),
                ],
            ));
        }

        let mut head: Vec<(String, Option<super::super::Color>)> = vec![(
            self.row_name
                .clone()
                .unwrap_or_else(|| labels.category.to_string()),
            None,
        )];
        head.extend(columns.iter().map(|name| (name.clone(), None)));
        if let Some(name) = summary_name {
            head.push((name, None));
        }
        let name = self
            .name
            .clone()
            .unwrap_or_else(|| self.common.title.clone());
        let body = self
            .ink
            .on(el("div"))
            .slot("heat-grid")
            .uses(Uses::CHART_GRIDS)
            .child(lines);
        Some(Drawn {
            legend: Some(legend.into()),
            body: body.into(),
            body_attrs: vec![
                ("data-grid", AttrValue::Flag),
                ("data-cross", AttrValue::Flag),
                ("data-rows", AttrValue::Int(index as i64)),
            ],
            summary,
            table: TableData {
                head,
                rows: table_rows,
                links: Vec::new(),
                note: None,
            },
            height: height_of(rows.len()),
            note: (!notes.is_empty()).then(|| notes.join(" ")),
            interactive: true,
            text: false,
            tip: Some(Tip {
                big: false,
                lines: vec![TipLine {
                    name: column_name(&name, self.unit.as_deref()),
                    unit: self.unit.clone(),
                    color: None,
                    key: true,
                }],
            }),
        })
    }
}

crate::node::into_node!(Heatmap);

/// A legend's entry: a swatch and its words.
fn key(swatch: Element, text: String) -> Node {
    el("li").children((swatch, text)).into()
}

/// The step of a reading between `low` and `high`, `"0"` to `"4"`: five equal bands, a
/// reading past an end in the band at that end. Every reading is the deepest step when
/// the range has no width.
fn level_of(value: f64, low: f64, high: f64) -> &'static str {
    let span = high - low;
    if !(span > 0.0 && span.is_finite()) {
        return "4";
    }
    let share = ((value - low) / span).clamp(0.0, 1.0);
    ["0", "1", "2", "3", "4"][((share * 5.0).floor() as usize).min(4)]
}

/// The height the chart keeps for its states: a header and a line for each row.
fn height_of(rows: usize) -> u32 {
    (rows as u32 * 26 + 28).max(96)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Tone;
    use crate::render;

    fn moving_by_hour() -> Heatmap {
        Heatmap::new("Time moving")
            .id("h")
            .columns(["6 am", "7 am", "8 am", "9 am"])
            .row("Mon", [0.0, 0.25, 0.5, 1.0])
            .row("Tue", [Some(0.9), None, Some(0.1), Some(0.75)])
            .range(0, 1)
            .format(NumberFormat::percent())
            .value_name("Moving")
    }

    fn html(chart: Heatmap) -> String {
        render(&chart.into())
    }

    fn cell(html: &str, i: usize) -> String {
        let from = html
            .find(&format!(r#"data-slot="heat-cell" data-i="{i}""#))
            .unwrap_or_else(|| panic!("cell {i}: {html}"));
        let end = html[from..].find('>').expect("end");
        html[from..from + end].to_owned()
    }

    #[test]
    fn a_reading_takes_one_of_five_equal_steps() {
        assert_eq!(level_of(0.0, 0.0, 1.0), "0");
        assert_eq!(level_of(0.19, 0.0, 1.0), "0");
        assert_eq!(level_of(0.2, 0.0, 1.0), "1");
        assert_eq!(level_of(0.5, 0.0, 1.0), "2");
        assert_eq!(level_of(0.79, 0.0, 1.0), "3");
        assert_eq!(level_of(0.8, 0.0, 1.0), "4");
        assert_eq!(level_of(1.0, 0.0, 1.0), "4");
        // Past the ends, the step at the end; a range with no width, the deepest.
        assert_eq!(level_of(-3.0, 0.0, 1.0), "0");
        assert_eq!(level_of(9.0, 0.0, 1.0), "4");
        assert_eq!(level_of(5.0, 5.0, 5.0), "4");
    }

    #[test]
    fn a_cell_for_each_row_at_each_column_in_reading_order() {
        let out = html(moving_by_hour());
        assert_eq!(out.matches(r#"data-slot="heat-cell""#).count(), 8, "{out}");
        assert_eq!(
            out.matches(r#"data-slot="heat-row""#).count(),
            3,
            "the header and two rows"
        );
        assert!(
            cell(&out, 0).ends_with(r#"data-r="0" data-c="0" data-level="0""#),
            "{}",
            cell(&out, 0)
        );
        assert!(cell(&out, 3).ends_with(r#"data-r="0" data-c="3" data-level="4""#));
        assert!(cell(&out, 4).ends_with(r#"data-r="1" data-c="0" data-level="4""#));
        // No data is hollow, not the faintest step.
        assert!(cell(&out, 5).ends_with(r#"data-level="none""#));
        assert!(out.contains(r#"data-rows="8""#), "{out}");
        assert!(out.contains("data-grid") && out.contains("data-cross"));
        assert!(
            out.contains(r#"data-slot="heat-rowhead" data-hr="1" title="Tue">Tue</span>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"data-slot="heat-colhead" data-hc="2" title="8 am">8 am</span>"#),
            "{out}"
        );
    }

    #[test]
    fn the_table_has_the_same_rows_and_columns() {
        let out = html(moving_by_hour().row_name("Day"));
        assert!(
            out.contains(r#"<th scope="col">Day</th><th scope="col" data-num="">6 am</th><th scope="col" data-num="">7 am</th>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<th scope="row">Mon</th><td data-num=""><span data-color="accent">0%</span></td><td data-num=""><span data-color="accent">25%</span></td>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<td data-num=""><span data-color="other">No data</span></td>"#),
            "{out}"
        );
        // The tooltip names the reading.
        assert!(
            out.contains(r#"<div data-tip="row" data-s="0"><span data-slot="chart-key" data-key="line"></span><strong></strong><span>Moving</span></div>"#),
            "{out}"
        );
    }

    #[test]
    fn one_legend_for_the_whole_grid() {
        let out = html(moving_by_hour());
        assert_eq!(
            out.matches(r#"data-slot="heat-legend""#).count(),
            1,
            "{out}"
        );
        // The steps between the ends of the range, then no data.
        assert!(
            out.contains(r#"<span data-slot="heat-legend-word">0%</span><span data-slot="heat-key" data-level="0"></span>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-slot="heat-key" data-level="4"></span><span data-slot="heat-legend-word">100%</span>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<li><span data-slot="heat-key" data-level="none"></span>No data</li>"#),
            "{out}"
        );
        let words = html(moving_by_hour().ends("Still", "Active"));
        assert!(words.contains(">Still</span>") && words.contains(">Active</span>"));
    }

    #[test]
    fn a_scale_colours_each_reading_by_its_band_and_names_the_bands() {
        let diverging = ColorScale::new(Ink::slot(2))
            .label("Far under")
            .from(0.5, Ink::slot(2).soft())
            .label("Under")
            .from(0.9, Ink::muted().soft())
            .label("About right")
            .from(1.1, Ink::slot(1).soft())
            .label("Over")
            .from(1.5, Ink::slot(1))
            .label("Far over");
        let out = html(
            Heatmap::new("Resting heart rate against usual")
                .columns(["Week 1", "Week 2"])
                .row("Harleen Kaur", [0.4, 1.0])
                .row("Gurpreet Singh", [1.2, 2.0])
                .colors(diverging),
        );
        assert!(
            cell(&out, 0).ends_with(r#"data-c="0" data-color="2" data-level="4""#),
            "{}",
            cell(&out, 0)
        );
        assert!(
            cell(&out, 1).ends_with(r#"data-color="other" data-mix="soft" data-level="4""#),
            "{}",
            cell(&out, 1)
        );
        assert!(cell(&out, 2).ends_with(r#"data-color="1" data-mix="soft" data-level="4""#));
        assert!(cell(&out, 3).ends_with(r#"data-color="1" data-level="4""#));
        let bands: Vec<&str> = out
            .split(r#"data-slot="heat-key""#)
            .skip(1)
            .map(|rest| {
                let text = &rest[rest.find("</span>").unwrap() + 7..];
                &text[..text.find('<').unwrap()]
            })
            .collect();
        assert_eq!(
            bands,
            ["Far under", "Under", "About right", "Over", "Far over"],
            "{out}"
        );
        // The table's value takes its band's colour, so the tooltip does.
        assert!(out.contains(r#"<span data-color="2">0.4</span>"#), "{out}");
    }

    #[test]
    fn categories_take_their_colours_and_the_legend_names_them_once() {
        let out = html(
            Heatmap::new("Workouts")
                .columns(["Mon", "Tue", "Wed"])
                .category("Rest", Ink::tone(Tone::Success))
                .category("Run", Ink::slot(1))
                .category_row("Harleen Kaur", ["Rest", "Run", "Run"])
                .category_row("Arjun Mehta", ["Run", "", "Swim"]),
        );
        assert!(cell(&out, 0).ends_with(r#"data-tone="success" data-level="4""#));
        assert!(cell(&out, 4).ends_with(r#"data-level="none""#));
        // A category met and not declared takes the next categorical colour.
        assert!(cell(&out, 5).ends_with(r#"data-color="3" data-level="4""#));
        assert_eq!(out.matches(r#"data-slot="heat-legend""#).count(), 1);
        assert_eq!(out.matches(r#"data-slot="heat-key""#).count(), 4, "{out}");
        assert!(
            out.contains(
                "2 rows by 3 columns. Most often Run, 50% of the cells. 1 cells have no data."
            ),
            "{out}"
        );
        assert!(out.contains(r#"<span data-color="1">Run</span>"#));
    }

    #[test]
    fn a_rows_end_says_its_average_or_total() {
        let out = html(moving_by_hour().row_summary(RowSummary::Average));
        // 0, 25, 50 and 100: 43.75%, written as the format writes it.
        assert!(
            out.contains(r#"<span data-slot="heat-rowend">Average</span>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-slot="heat-rowend">43.8%</span>"#),
            "{out}"
        );
        assert!(out.contains(r#"<th scope="col" data-num="">Average</th>"#));
        let total = html(
            Heatmap::new("Steps")
                .locale("en-IN")
                .columns(["a", "b"])
                .row("x", [100_000, 250_000])
                .row_summary(RowSummary::Total)
                .unit("steps"),
        );
        assert!(
            total.contains(r#"<span data-slot="heat-rowend">3,50,000 steps</span>"#),
            "{total}"
        );
    }

    #[test]
    fn the_summary_names_the_highest_and_the_lowest() {
        let out = html(moving_by_hour());
        assert!(
            out.contains("2 rows by 4 columns. Highest 100%, Mon at 9 am. Lowest 0%, Mon at 6 am. 1 cells have no data."),
            "{out}"
        );
    }

    #[test]
    fn many_columns_are_named_in_turn_and_values_are_opt_in() {
        let slots: Vec<String> = (0..37).map(|n| format!("s{n}")).collect();
        let out = html(
            Heatmap::new("Slots")
                .columns(slots.clone())
                .row("a", (0..37).map(f64::from)),
        );
        // 37 columns: one name in three.
        assert!(out.contains(r#"title="s3">s3</span>"#), "{out}");
        assert!(out.contains(r#"title="s4"></span>"#), "{out}");
        let every = html(
            Heatmap::new("Slots")
                .columns(slots)
                .row("a", (0..37).map(f64::from))
                .label_every(6),
        );
        assert!(
            every.contains(r#"title="s6">s6</span>"#) && every.contains(r#"title="s3"></span>"#)
        );
        assert!(!html(moving_by_hour()).contains(r#"data-level="4">100%</div>"#));
        assert!(html(moving_by_hour().show_values()).contains(r#"data-level="4">100%</div>"#));
    }

    #[test]
    fn more_than_the_most_is_cut_and_the_chart_says_so() {
        let columns: Vec<String> = (0..200).map(|n| format!("c{n}")).collect();
        let mut chart = Heatmap::new("Big").columns(columns);
        for r in 0..60 {
            chart = chart.row(format!("r{r}"), (0..200).map(|c| f64::from(c + r)));
        }
        let out = html(chart);
        assert_eq!(
            out.matches(r#"data-slot="heat-cell""#).count(),
            MAX_HEAT_ROWS * MAX_HEAT_COLUMNS
        );
        assert!(
            out.contains(
                "The first 50 of 60 rows are drawn. The first 144 of 200 columns are drawn."
            ),
            "the note"
        );
    }

    #[test]
    fn nothing_to_draw_is_the_empty_state_and_odd_input_never_panics() {
        assert!(html(Heatmap::new("x")).contains(r#"data-state="empty""#));
        assert!(html(Heatmap::new("x").row("a", [1])).contains(r#"data-state="empty""#));
        assert!(
            html(Heatmap::new("x").columns(["a"]).row("r", [f64::NAN]))
                .contains(r#"data-state="empty""#)
        );
        let odd = html(
            Heatmap::new("x")
                .columns(["a", "b"])
                .row("r", [f64::MAX, -f64::MAX, 3.0])
                .row("s", [5])
                .range(f64::NAN, 1),
        );
        assert!(!odd.contains("NaN") && !odd.contains("inf"), "{odd}");
        assert!(!odd.contains("style="));
    }

    #[test]
    fn colour_comes_from_inks_and_the_page_links_what_it_needs() {
        let node: Node = moving_by_hour().ink(Ink::slot(5)).into();
        assert_eq!(
            crate::View::new("x").body(node.clone()).uses(),
            Uses::CHARTS.with(Uses::CHART_GRIDS)
        );
        let out = render(&node);
        assert!(
            out.contains(r#"data-color="5" data-slot="heat-grid""#),
            "{out}"
        );
        assert!(out.contains(r#"data-kind="heatmap""#));
        assert!(out.contains(r#"role="group" aria-label="Time moving, chart""#));
        assert!(!out.contains("style="), "no style attribute");
        assert!(!out.contains('#'), "no colour written");
    }

    #[test]
    fn the_rows_fade_in_one_after_another_when_asked() {
        let out = html(moving_by_hour().motion(Motion::new().stagger(100)));
        assert_eq!(out.matches(r#"data-motion="fade""#).count(), 2, "{out}");
        assert!(!html(moving_by_hour()).contains("data-motion"));
    }
}
