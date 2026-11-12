//! [`CalendarHeatmap`]: a day per cell, a column per week.

use std::collections::BTreeMap;

use chrono::{Datelike, Duration, NaiveDate, Weekday};

use super::super::format::NumberFormat;
use super::super::motion::{Effect, Motion};
use super::super::paint::Ink;
use super::super::value::{IntoValue, finite};
use super::super::{Common, Drawn, TableData, Tip, TipLine, common_methods, fill, frame};
use super::{GridLabels, count, heat_legend};
use crate::node::{AttrValue, Node, Uses, el};

/// The most days a calendar draws: the latest ones, when it was given more.
const MAX_DAYS: i64 = 1_100;

/// How a day's value picks one of the four steps above zero.
///
/// ```
/// use navree_ui::charts::{CalendarHeatmap, HeatSteps};
///
/// // Steps from the data's own spread: each holds about a quarter of the active days.
/// let skewed = CalendarHeatmap::new("Sessions").steps(HeatSteps::Quartiles);
/// // Steps you choose: 30 and over is the second step, 60 the third, 90 the fourth.
/// let fixed = CalendarHeatmap::new("Load").steps(HeatSteps::at(30, 60, 90));
/// # let _ = (skewed, fixed);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum HeatSteps {
    /// Four equal parts of the range from zero to the highest value: the default. A
    /// value above zero is at least the first step.
    #[default]
    Even,
    /// Quartiles of the values above zero, so every step is used even when a few days
    /// stand far above the rest.
    Quartiles,
    /// Where the second, third and fourth steps begin.
    At(f64, f64, f64),
}

impl HeatSteps {
    /// The second, third and fourth steps begin at `second`, `third` and `fourth`, in any order.
    pub fn at(second: impl IntoValue, third: impl IntoValue, fourth: impl IntoValue) -> Self {
        let mut edges = [second.into_value(), third.into_value(), fourth.into_value()]
            .map(|v| v.unwrap_or(0.0));
        edges.sort_by(f64::total_cmp);
        Self::At(edges[0], edges[1], edges[2])
    }

    /// Where steps 2, 3 and 4 begin, for these values.
    fn edges(self, values: &[f64]) -> [f64; 3] {
        let mut positive: Vec<f64> = values.iter().copied().filter(|v| *v > 0.0).collect();
        match self {
            Self::Even => {
                let max = positive.iter().copied().fold(0.0, f64::max);
                [max * 0.25, max * 0.5, max * 0.75]
            }
            Self::Quartiles => {
                positive.sort_by(f64::total_cmp);
                let at = |q: f64| {
                    positive
                        .get(((positive.len().saturating_sub(1)) as f64 * q).round() as usize)
                        .copied()
                        .unwrap_or(0.0)
                };
                [at(0.25), at(0.5), at(0.75)]
            }
            Self::At(a, b, c) => [a, b, c],
        }
    }
}

/// The step of a value: 0 for nothing or less, 1 to 4 above it.
fn step_of(value: f64, edges: [f64; 3]) -> u8 {
    if value <= 0.0 {
        0
    } else {
        1 + edges.iter().filter(|edge| value >= **edge).count() as u8
    }
}

/// A calendar heat map: one cell per day, a column per week, a row per weekday, each cell
/// shaded by its day's value in five steps (nothing, then four). Months are named above
/// the weeks and weekdays beside the rows. A day with no value is a hollow dashed cell,
/// which is not the same as a day of zero.
///
/// On a narrow screen the weeks scroll sideways, starting at the latest. The cells grow
/// with the width of the chart, 12, 16 and 24 pixels. The figure carries a legend from
/// "Less" to "More", a summary, a tooltip for a day and its data as a table.
///
/// ```
/// use chrono::{Duration, NaiveDate};
/// use navree_ui::charts::{CalendarHeatmap, HeatSteps, Ink};
///
/// let first = NaiveDate::from_ymd_opt(2026, 5, 15).unwrap();
/// let chart = CalendarHeatmap::new("Training consistency")
///     .description("Last 6 months, Harleen Kaur")
///     .locale("en-IN")
///     .days((0..181).map(|d| {
///         let load = if d % 9 == 4 { None } else { Some((d * 37 % 130) as f64) };
///         (first + Duration::days(d), load)
///     }))
///     .ink(Ink::slot(5))
///     .unit("load")
///     .steps(HeatSteps::Quartiles);
/// # let _ = navree_ui::render(&chart.into());
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct CalendarHeatmap {
    common: Common,
    days: Vec<(NaiveDate, Option<f64>)>,
    range: Option<(NaiveDate, NaiveDate)>,
    week_start: Option<Weekday>,
    ink: Ink,
    steps: HeatSteps,
    format: NumberFormat,
    name: Option<String>,
    unit: Option<String>,
    words: GridLabels,
    motion: Motion,
    weekdays: bool,
}

common_methods!(CalendarHeatmap);

impl CalendarHeatmap {
    /// A calendar called `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            common: Common::new(title.into()),
            days: Vec::new(),
            range: None,
            week_start: None,
            ink: Ink::accent(),
            steps: HeatSteps::Even,
            format: NumberFormat::number(),
            name: None,
            unit: None,
            words: GridLabels::english(),
            motion: Motion::none(),
            weekdays: true,
        }
    }

    /// Adds a day. A value that is not a finite number, or `None`, is a day with no data.
    /// A date given twice keeps the last value.
    pub fn day(mut self, date: NaiveDate, value: impl IntoValue) -> Self {
        self.days.push((date, finite(value.into_value())));
        self
    }

    /// Adds days: dates with their values. Days between them that are not given have no
    /// data.
    pub fn days<V: IntoValue>(mut self, days: impl IntoIterator<Item = (NaiveDate, V)>) -> Self {
        self.days.extend(
            days.into_iter()
                .map(|(date, value)| (date, finite(value.into_value()))),
        );
        self
    }

    /// Draws these dates, first to last, whether or not they have data: a range that
    /// starts before the first day given, or ends after the last, shows the days between
    /// as empty. Days given outside it are left out.
    pub fn range(mut self, first: NaiveDate, last: NaiveDate) -> Self {
        self.range = Some((first.min(last), first.max(last)));
        self
    }

    /// The first row of the grid (the locale's first day of the week by default).
    pub fn week_start(mut self, day: Weekday) -> Self {
        self.week_start = Some(day);
        self
    }

    /// The colour the steps are made of (the accent by default).
    pub fn ink(mut self, ink: impl Into<Ink>) -> Self {
        self.ink = ink.into();
        self
    }

    /// How a value picks its step.
    pub fn steps(mut self, steps: HeatSteps) -> Self {
        self.steps = steps;
        self
    }

    /// How values are written in the tooltip, the table and the summary.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.format = format;
        self
    }

    /// What the value is called in the tooltip and the table (the title by default).
    pub fn value_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// What the value counts, written after it: `load`, `steps`, `min`.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// The words the chart writes.
    pub fn words(mut self, words: GridLabels) -> Self {
        self.words = words;
        self
    }

    /// How the weeks come in: they fade in one after another. Off by default.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
        self
    }

    /// Leaves out the weekday names beside the rows.
    pub fn no_weekdays(mut self) -> Self {
        self.weekdays = false;
        self
    }

    fn into_node(self) -> Node {
        let mut this = self;
        let drawn = this.draw();
        frame(this.common, drawn, "calendar-heatmap", 176)
    }

    fn draw(&mut self) -> Option<Drawn> {
        let words = self.words.clone();
        let numbers = self.common.numbers.clone();
        let dates = self.common.dates.clone();
        let mut by_day: BTreeMap<NaiveDate, Option<f64>> = BTreeMap::new();
        for (date, value) in &self.days {
            by_day.insert(*date, *value);
        }
        if let Some((first, last)) = self.range {
            by_day.retain(|date, _| (first..=last).contains(date));
        }
        if by_day.values().all(Option::is_none) {
            return None;
        }
        let (mut first, last) = self.range.unwrap_or_else(|| {
            let first = by_day.keys().next().copied().unwrap_or_default();
            let last = by_day.keys().next_back().copied().unwrap_or_default();
            (first, last)
        });
        let total = (last - first).num_days() + 1;
        let cut = total > MAX_DAYS;
        if cut {
            first = last - Duration::days(MAX_DAYS - 1);
        }
        let shown = (last - first).num_days() + 1;

        let week_start = self.week_start.unwrap_or(dates.first_day);
        let row_of = |date: NaiveDate| {
            (date.weekday().num_days_from_monday() + 7 - week_start.num_days_from_monday()) % 7
        };
        let grid_start = first - Duration::days(i64::from(row_of(first)));
        let weeks = ((last - grid_start).num_days() / 7 + 1) as usize;

        let values: Vec<f64> = by_day
            .range(first..=last)
            .filter_map(|(_, value)| *value)
            .collect();
        let edges = self.steps.edges(&values);
        let fmt = |v: f64| self.format.format(v, &numbers);
        let with_unit = |v: f64| match &self.unit {
            Some(unit) => format!("{} {unit}", fmt(v)),
            None => fmt(v),
        };

        // Month names: above the first week of each month, and the first week, dropping a
        // name that would stand within two columns of the next.
        let mut marks: Vec<(usize, u32)> = Vec::new();
        for w in 0..weeks {
            let column: Vec<NaiveDate> = (0..7)
                .map(|r| grid_start + Duration::days((w * 7 + r) as i64))
                .filter(|date| (first..=last).contains(date))
                .collect();
            let new_month = column.iter().find(|date| date.day() == 1);
            match (w, new_month, column.first()) {
                (_, Some(date), _) => marks.push((w, date.month())),
                (0, None, Some(date)) => marks.push((0, date.month())),
                _ => {}
            }
        }
        let months: BTreeMap<usize, u32> = marks
            .iter()
            .enumerate()
            .filter(|(at, (w, _))| marks.get(at + 1).is_none_or(|(next, _)| next - w >= 2))
            .map(|(_, mark)| *mark)
            .collect();

        let mut week_nodes: Vec<Node> = Vec::with_capacity(weeks);
        let mut rows = Vec::new();
        let mut index = 0_usize;
        for w in 0..weeks {
            let mut cells: Vec<Node> = Vec::with_capacity(8);
            if let Some(month) = months.get(&w) {
                cells.push(
                    el("span")
                        .slot("heat-month")
                        .child(dates.month_short(*month).to_owned())
                        .into(),
                );
            }
            for r in 0..7_usize {
                let date = grid_start + Duration::days((w * 7 + r) as i64);
                if date < first || date > last {
                    cells.push(el("div").slot("heat-pad").into());
                    continue;
                }
                let value = by_day.get(&date).copied().flatten();
                let level = match value {
                    None => "none",
                    Some(v) => ["0", "1", "2", "3", "4"][usize::from(step_of(v, edges))],
                };
                cells.push(
                    el("div")
                        .slot("heat-cell")
                        .attr("data-i", index as i64)
                        .attr("data-r", r as i64)
                        .attr("data-c", w as i64)
                        .attr("data-level", level)
                        .into(),
                );
                rows.push((
                    dates.format_long(date),
                    vec![match value {
                        Some(v) => Node::from(fmt(v)),
                        None => Node::from(words.no_data.clone()),
                    }],
                ));
                index += 1;
            }
            let week = self
                .motion
                .apply(el("div").slot("heat-week"), Effect::Fade, w)
                .child(cells);
            week_nodes.push(week.into());
        }

        let day_names: Vec<Node> = (0..7_u32)
            .map(|r| {
                let day = Weekday::try_from(((week_start.num_days_from_monday() + r) % 7) as u8)
                    .unwrap_or(Weekday::Mon);
                // Monday, Wednesday and Friday are named: every other row, as a person reads them.
                let named = matches!(day, Weekday::Mon | Weekday::Wed | Weekday::Fri);
                el("span")
                    .child(named.then(|| dates.weekday_short(day).to_owned()))
                    .into()
            })
            .collect();
        let size = match weeks {
            0..=30 => "short",
            31..=60 => "long",
            _ => "huge",
        };
        let heat = self
            .ink
            .on(el("div"))
            .slot("heat")
            .attr("data-weeks", size)
            .uses(Uses::CHART_GRIDS)
            .children((
                self.weekdays
                    .then(|| el("div").slot("heat-days").child(day_names)),
                el("div").slot("heat-weeks").child(week_nodes),
            ));

        let key = |step: &'static str| {
            super::swatch(el("li").attr("data-level", step))
                .uses(Uses::CHART_GRIDS)
                .into()
        };
        let legend = heat_legend(
            Some(self.ink),
            vec![
                el("li")
                    .slot("heat-legend-word")
                    .child(words.less.clone())
                    .into(),
                key("0"),
                key("1"),
                key("2"),
                key("3"),
                key("4"),
                el("li")
                    .slot("heat-legend-word")
                    .child(words.more.clone())
                    .into(),
            ],
        );

        let active = values.iter().filter(|v| **v > 0.0).count();
        let busiest = by_day
            .range(first..=last)
            .filter_map(|(date, value)| value.map(|v| (*date, v)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let missing = by_day
            .range(first..=last)
            .filter(|(_, v)| v.is_none())
            .count()
            + (shown as usize).saturating_sub(by_day.range(first..=last).count());
        let mut summary = fill(
            &words.calendar_summary,
            &[
                ("days", &count(shown as usize, &numbers)),
                ("first", &dates.format_long(first)),
                ("last", &dates.format_long(last)),
                ("active", &count(active, &numbers)),
                (
                    "max",
                    &busiest.map_or_else(|| "–".to_owned(), |(_, v)| with_unit(v)),
                ),
                (
                    "max_date",
                    &busiest.map_or_else(|| "–".to_owned(), |(d, _)| dates.format_long(d)),
                ),
                ("total", &with_unit(values.iter().sum())),
            ],
        );
        if missing > 0 {
            summary.push_str(&fill(
                &words.calendar_missing,
                &[("missing", &count(missing, &numbers))],
            ));
        }

        let name = self
            .name
            .clone()
            .unwrap_or_else(|| self.common.title.clone());
        let table = TableData {
            head: vec![
                (words.date.to_string(), None),
                (
                    match &self.unit {
                        Some(unit) => format!("{name} ({unit})"),
                        None => name.clone(),
                    },
                    None,
                ),
            ],
            rows,
            links: Vec::new(),
            note: None,
        };
        let height = 176;
        Some(Drawn {
            legend: Some(legend),
            body: heat.into(),
            body_attrs: vec![
                ("data-grid", AttrValue::Flag),
                ("data-end", AttrValue::Flag),
                ("data-rows", AttrValue::Int(index as i64)),
            ],
            summary,
            table,
            height,
            note: cut.then(|| {
                fill(
                    &words.calendar_cut,
                    &[
                        ("shown", &count(shown as usize, &numbers)),
                        ("total", &count(total as usize, &numbers)),
                    ],
                )
            }),
            interactive: true,
            tip: Some(Tip {
                big: false,
                lines: vec![TipLine {
                    name,
                    unit: self.unit.clone(),
                    color: None,
                    key: false,
                }],
            }),
        })
    }
}

crate::node::into_node!(CalendarHeatmap);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    fn day(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, d).unwrap()
    }

    #[test]
    fn a_value_picks_a_step() {
        let edges = HeatSteps::Even.edges(&[0.0, 10.0, 40.0]);
        assert_eq!(edges, [10.0, 20.0, 30.0]);
        let steps: Vec<u8> = [-1.0, 0.0, 0.5, 10.0, 25.0, 30.0, 40.0]
            .iter()
            .map(|v| step_of(*v, edges))
            .collect();
        assert_eq!(steps, [0, 0, 1, 2, 3, 4, 4]);
        // Quartiles use every step when a spike stands far above the rest.
        let edges = HeatSteps::Quartiles.edges(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 1000.0]);
        let used: std::collections::BTreeSet<u8> = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 1000.0]
            .iter()
            .map(|v| step_of(*v, edges))
            .collect();
        assert_eq!(used.len(), 4, "{used:?}");
        assert_eq!(HeatSteps::at(90, 30, 60), HeatSteps::At(30.0, 60.0, 90.0));
        assert_eq!(HeatSteps::Even.edges(&[]), [0.0; 3]);
    }

    #[test]
    fn weeks_are_columns_and_weekdays_are_rows() {
        // 1 March 2026 is a Sunday.
        let chart = CalendarHeatmap::new("Load")
            .id("cal")
            .week_start(Weekday::Mon)
            .days((1..=14).map(|d| (day(d), d as f64)));
        let html = render(&chart.into());
        // Sunday 1 March sits in the first column, in the last row; two full weeks follow.
        assert!(
            html.contains(
                r#"<div data-slot="heat-cell" data-i="0" data-r="6" data-c="0" data-level="1">"#
            ),
            "{html}"
        );
        assert_eq!(html.matches(r#"data-slot="heat-week""#).count(), 3);
        assert_eq!(html.matches(r#"data-slot="heat-cell""#).count(), 14);
        // The six days before the first fill the column's start with padding.
        assert_eq!(html.matches(r#"data-slot="heat-pad""#).count(), 6 + 1);
        assert!(html.contains(r#"data-rows="14""#), "{html}");
        assert!(html.contains("data-grid") && html.contains("data-end"));
    }

    #[test]
    fn a_day_with_no_data_is_not_a_day_of_zero() {
        let chart = CalendarHeatmap::new("Load")
            .day(day(2), 0)
            .day(day(3), None::<f64>)
            .day(day(4), f64::NAN)
            .day(day(5), 8)
            .range(day(2), day(6));
        let html = render(&chart.into());
        let level = |i: usize| {
            let at = html.find(&format!(r#"data-i="{i}""#)).unwrap();
            let rest = &html[at..];
            let from = rest.find("data-level=\"").unwrap() + 12;
            rest[from..rest.find("\">").unwrap()].to_owned()
        };
        assert_eq!(level(0), "0");
        assert_eq!(level(1), "none");
        assert_eq!(level(2), "none");
        assert_eq!(level(3), "4");
        // Day 6 was in the range but never given.
        assert_eq!(level(4), "none");
        assert!(html.contains("No data"), "{html}");
        assert!(html.contains("3 days have no data."), "{html}");
    }

    #[test]
    fn months_are_named_above_their_first_week_and_never_crowd() {
        let first = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let chart = CalendarHeatmap::new("Load")
            .days((0..181).map(|d| (first + Duration::days(d), 1)))
            .week_start(Weekday::Sun);
        let html = render(&chart.into());
        let names: Vec<&str> = html
            .split(r#"data-slot="heat-month">"#)
            .skip(1)
            .map(|rest| &rest[..rest.find('<').unwrap()])
            .collect();
        assert_eq!(names, ["Jan", "Feb", "Mar", "Apr", "May", "Jun"], "{html}");
        // A start two days before a month ends gives that month no name of its own.
        let chart = CalendarHeatmap::new("Load")
            .days((0..60).map(|d| {
                (
                    NaiveDate::from_ymd_opt(2026, 1, 30).unwrap() + Duration::days(d),
                    1,
                )
            }))
            .week_start(Weekday::Sun);
        let html = render(&chart.into());
        assert!(!html.contains(">Jan<"), "{html}");
        assert!(html.contains(">Feb<") && html.contains(">Mar<"), "{html}");
    }

    #[test]
    fn the_table_lists_each_day_and_the_summary_names_the_busiest() {
        let chart = CalendarHeatmap::new("Training load")
            .locale("en-IN")
            .days([(day(2), 120_000.0), (day(3), 50_000.0)])
            .unit("kcal");
        let html = render(&chart.into());
        assert!(
            html.contains("<th scope=\"row\">Monday, 2 March 2026</th>") || html.contains("Monday"),
            "{html}"
        );
        assert!(html.contains("1,20,000"), "lakh grouping: {html}");
        assert!(html.contains("Highest 1,20,000 kcal on"), "{html}");
        assert!(html.contains("Training load (kcal)"), "{html}");
    }

    #[test]
    fn nothing_to_draw_is_the_empty_state_and_odd_input_never_panics() {
        let empty = render(&CalendarHeatmap::new("Load").into());
        assert!(empty.contains(r#"data-state="empty""#), "{empty}");
        let none = render(&CalendarHeatmap::new("Load").day(day(1), None::<f64>).into());
        assert!(none.contains(r#"data-state="empty""#), "{none}");
        // One day, a range reversed, huge and negative values, years apart.
        let one = render(&CalendarHeatmap::new("Load").day(day(1), 5).into());
        assert!(one.contains(r#"data-rows="1""#), "{one}");
        let wild = render(
            &CalendarHeatmap::new("Load")
                .day(day(1), f64::MAX)
                .day(day(2), -4)
                .day(NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(), 1)
                .range(day(9), day(1))
                .into(),
        );
        assert!(!wild.contains("NaN") && !wild.contains("inf"), "{wild}");
        // Years of days: the latest are drawn, and the page says so.
        let long = render(
            &CalendarHeatmap::new("Load")
                .days((0..2_000).map(|d| {
                    (
                        NaiveDate::from_ymd_opt(2020, 1, 1).unwrap() + Duration::days(d),
                        3,
                    )
                }))
                .into(),
        );
        assert!(
            long.contains("Showing the latest 1,100 of 2,000 days."),
            "{long}"
        );
        assert_eq!(long.matches(r#"data-slot="heat-cell""#).count(), 1_100);
    }

    #[test]
    fn colour_comes_from_an_ink_and_the_page_links_what_it_needs() {
        let chart = CalendarHeatmap::new("Load")
            .day(day(1), 5)
            .ink(Ink::slot(5));
        let node: Node = chart.into();
        assert!(
            crate::View::new("x")
                .body(node.clone())
                .uses()
                .has(Uses::CHART_GRIDS.with(Uses::CHARTS))
        );
        let html = crate::render(&node);
        assert!(
            html.contains(r#"data-color="5" data-slot="heat""#),
            "{html}"
        );
        assert!(!html.contains("style="), "no style attribute: {html}");
        assert!(!html.contains("#"), "no colour written: {html}");
    }

    #[test]
    fn the_weekday_names_follow_the_first_row() {
        let html = render(
            &CalendarHeatmap::new("Load")
                .day(day(2), 1)
                .week_start(Weekday::Mon)
                .into(),
        );
        // A week that starts on Monday names Monday, Wednesday and Friday.
        assert!(
            html.contains("<span>Mo</span><span></span><span>We</span><span></span><span>Fr</span><span></span><span></span>"),
            "{html}"
        );
        let bare = render(
            &CalendarHeatmap::new("Load")
                .day(day(2), 1)
                .no_weekdays()
                .into(),
        );
        assert!(!bare.contains("heat-days"));
    }
}
