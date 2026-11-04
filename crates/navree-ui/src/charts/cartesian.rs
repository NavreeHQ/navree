//! [`Chart`]: lines, areas and bars over one x, vertical or horizontal, grouped or
//! stacked.

use std::fmt::Write as _;

use chrono::{NaiveDate, NaiveDateTime, Weekday};

use super::downsample::{merge_ranges, rows_to_keep};
use super::format::NumberFormat;
use super::scale::{Grain, Linear, nice, seconds_of, time_label, time_name, time_of, time_ticks};
use super::value::{IntoValue, clean, finite};
use super::{
    Color, Common, Drawn, Key, LegendItem, MAX_SERIES, TableData, common_methods, fill, frame,
    legend, n, nodes, num, pct, svg, value_cell,
};
use crate::components::Tone;
use crate::node::{AttrValue, Element, Node, el};

/// Room above the plot for the top tick's text and the markers.
const TOP: f64 = 10.0;
/// The band under the plot that holds the x labels.
const X_BAND: f64 = 22.0;
/// A horizontal bar row's height.
const ROW: f64 = 28.0;

/// One series: a name and a value for each x.
///
/// ```
/// use navree_ui::charts::Series;
/// use navree_ui::Tone;
///
/// let opens = Series::new("Opens", [12, 40, 31]);
/// let failures = Series::new("Failures", [Some(1.0), None, Some(f64::NAN)]).tone(Tone::Danger);
/// # let _ = (opens, failures);
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Series {
    pub(crate) name: String,
    pub(crate) values: Vec<Option<f64>>,
    pub(crate) color: Option<Color>,
}

impl Series {
    /// A series called `name`. A value that is `None`, NaN or infinite is a gap.
    pub fn new<V: IntoValue>(name: impl Into<String>, values: impl IntoIterator<Item = V>) -> Self {
        Self {
            name: name.into(),
            values: clean(values),
            color: None,
        }
    }

    /// Colours the series by what it means: [`Tone::Success`] for a good outcome,
    /// [`Tone::Danger`] for failures. The legend adds the tone's icon.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.color = Some(Color::Tone(tone));
        self
    }

    /// Pins the series to categorical colour `slot` (1 to 8), so it keeps its colour
    /// when other series come and go.
    pub fn color(mut self, slot: u8) -> Self {
        self.color = Some(Color::Slot(slot.clamp(1, 8)));
        self
    }
}

/// A place on the x axis, for an annotation.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum At {
    /// The row at this index.
    Index(usize),
    /// A time.
    Time(NaiveDateTime),
    /// A number.
    Number(f64),
    /// A category, by its name.
    Category(String),
}

impl From<NaiveDateTime> for At {
    fn from(at: NaiveDateTime) -> Self {
        Self::Time(at)
    }
}

impl From<NaiveDate> for At {
    fn from(day: NaiveDate) -> Self {
        Self::Time(day.and_time(chrono::NaiveTime::MIN))
    }
}

impl From<&str> for At {
    fn from(name: &str) -> Self {
        Self::Category(name.to_owned())
    }
}

impl From<String> for At {
    fn from(name: String) -> Self {
        Self::Category(name)
    }
}

impl From<f64> for At {
    fn from(x: f64) -> Self {
        Self::Number(x)
    }
}

/// Something written on a chart beside the data.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Annotation {
    /// A level across the chart: a target, a limit.
    Target {
        /// The value.
        value: f64,
        /// What it is.
        label: String,
    },
    /// A moment: "Reminder sent".
    Event {
        /// Where.
        at: At,
        /// What happened.
        label: String,
    },
    /// A stretch of the x axis: "Holiday".
    Band {
        /// From.
        from: At,
        /// To.
        to: At,
        /// What it is.
        label: String,
    },
}

impl Annotation {
    /// A dashed line across the chart at `value`.
    pub fn target(value: impl IntoValue, label: impl Into<String>) -> Self {
        Self::Target {
            value: value.into_value().unwrap_or(0.0),
            label: label.into(),
        }
    }

    /// A line up the chart at `at`.
    pub fn event(at: impl Into<At>, label: impl Into<String>) -> Self {
        Self::Event {
            at: at.into(),
            label: label.into(),
        }
    }

    /// A shaded stretch from `from` to `to`.
    pub fn band(from: impl Into<At>, to: impl Into<At>, label: impl Into<String>) -> Self {
        Self::Band {
            from: from.into(),
            to: to.into(),
            label: label.into(),
        }
    }
}

/// How bars that are merged to fit combine their values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Merge {
    /// Added up: counts, amounts.
    #[default]
    Sum,
    /// Averaged: rates, temperatures.
    Mean,
    /// The largest.
    Max,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Line,
    Area,
    Bar,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Stack {
    #[default]
    None,
    Normal,
    Percent,
}

#[derive(Clone, Debug)]
enum X {
    None,
    Categories(Vec<String>),
    Times(Vec<i64>),
    Numbers(Vec<f64>),
}

impl X {
    fn len(&self) -> usize {
        match self {
            Self::None => 0,
            Self::Categories(v) => v.len(),
            Self::Times(v) => v.len(),
            Self::Numbers(v) => v.len(),
        }
    }
}

/// A chart of lines, areas or bars over one x: categories, times or numbers.
///
/// ```
/// use navree_ui::charts::*;
/// use chrono::NaiveDate;
///
/// let days: Vec<NaiveDate> = (1..=7).map(|d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap()).collect();
/// let chart = Chart::bar("Sent and opened")
///     .days(days)
///     .series(Series::new("Sent", [120, 140, 90, 160, 180, 40, 30]))
///     .series(Series::new("Opened", [80, 95, 50, 120, 130, 22, 18]))
///     .annotation(Annotation::target(100, "Goal"));
/// # let _ = navree_ui::render(&chart.into());
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Chart {
    common: Common,
    kind: Kind,
    horizontal: bool,
    stack: Stack,
    smooth: bool,
    x: X,
    grain: Option<Grain>,
    x_label: Option<String>,
    series: Vec<Series>,
    format: NumberFormat,
    y_min: Option<f64>,
    y_max: Option<f64>,
    y_zero: Option<bool>,
    y_ticks: Option<u8>,
    annotations: Vec<Annotation>,
    height: u32,
    max_points: usize,
    max_bars: usize,
    merge: Merge,
    toggles: bool,
    dots: bool,
    week_start: Weekday,
    rtl: bool,
}

common_methods!(Chart);

impl Chart {
    fn new(kind: Kind, title: impl Into<String>) -> Self {
        Self {
            common: Common::new(title.into()),
            kind,
            horizontal: false,
            stack: Stack::None,
            smooth: false,
            x: X::None,
            grain: None,
            x_label: None,
            series: Vec::new(),
            format: NumberFormat::number(),
            y_min: None,
            y_max: None,
            y_zero: None,
            y_ticks: None,
            annotations: Vec::new(),
            height: 200,
            max_points: 640,
            max_bars: 120,
            merge: Merge::Sum,
            toggles: false,
            dots: true,
            week_start: Weekday::Mon,
            rtl: false,
        }
    }

    /// Lines: change over time, several series side by side.
    pub fn line(title: impl Into<String>) -> Self {
        Self::new(Kind::Line, title)
    }

    /// Areas: a line with a wash under it; stacked, parts of a total over time.
    pub fn area(title: impl Into<String>) -> Self {
        Self::new(Kind::Area, title)
    }

    /// Bars from one baseline: compare amounts. Several series sit side by side.
    pub fn bar(title: impl Into<String>) -> Self {
        Self::new(Kind::Bar, title)
    }

    /// Bars lie along rows, a label on each: for long names and ranked lists.
    pub fn horizontal(mut self) -> Self {
        self.horizontal = true;
        self
    }

    /// Stacks the series: bars on bars, areas on areas.
    pub fn stacked(mut self) -> Self {
        self.stack = Stack::Normal;
        self
    }

    /// Stacks the series as shares of each x's total, to 100%.
    pub fn stacked_percent(mut self) -> Self {
        self.stack = Stack::Percent;
        self
    }

    /// Draws lines as a monotone curve, which never overshoots a value.
    pub fn smooth(mut self) -> Self {
        self.smooth = true;
        self
    }

    /// Named categories along x.
    pub fn categories<S: Into<String>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        self.x = X::Categories(names.into_iter().map(Into::into).collect());
        self
    }

    /// Times along x, as wall-clock times in the zone the data was counted in.
    pub fn times(mut self, times: impl IntoIterator<Item = NaiveDateTime>) -> Self {
        self.x = X::Times(times.into_iter().map(seconds_of).collect());
        self
    }

    /// Days along x.
    pub fn days(mut self, days: impl IntoIterator<Item = NaiveDate>) -> Self {
        self.x = X::Times(
            days.into_iter()
                .map(|day| seconds_of(day.and_time(chrono::NaiveTime::MIN)))
                .collect(),
        );
        self.grain.get_or_insert(Grain::Day);
        self
    }

    /// Numbers along x.
    pub fn numbers<V: IntoValue>(mut self, values: impl IntoIterator<Item = V>) -> Self {
        self.x = X::Numbers(
            values
                .into_iter()
                .map(|v| finite(v.into_value()).unwrap_or(0.0))
                .collect(),
        );
        self
    }

    /// The period each time stands for, which sets how it reads: an hour reads
    /// `14:00`, a month `Nov 2026`. Found from the spacing of the times when not set.
    pub fn grain(mut self, grain: Grain) -> Self {
        self.grain = Some(grain);
        self
    }

    /// The table's first column: what x is.
    pub fn x_label(mut self, label: impl Into<String>) -> Self {
        self.x_label = Some(label.into());
        self
    }

    /// Adds a series. Colours follow the order series are added in.
    pub fn series(mut self, series: Series) -> Self {
        self.series.push(series);
        self
    }

    /// Adds several series.
    pub fn series_list(mut self, series: impl IntoIterator<Item = Series>) -> Self {
        self.series.extend(series);
        self
    }

    /// How values are written: on the axis, in the tooltip and in the table.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.format = format;
        self
    }

    /// The value axis starts here at most.
    pub fn y_min(mut self, value: impl IntoValue) -> Self {
        self.y_min = value.into_value();
        self
    }

    /// The value axis ends here at least.
    pub fn y_max(mut self, value: impl IntoValue) -> Self {
        self.y_max = value.into_value();
        self
    }

    /// Whether a line's value axis starts at zero (it does by default; bars always do).
    pub fn y_zero(mut self, zero: bool) -> Self {
        self.y_zero = Some(zero);
        self
    }

    /// About how many ticks the value axis writes.
    pub fn y_ticks(mut self, count: u8) -> Self {
        self.y_ticks = Some(count.clamp(2, 12));
        self
    }

    /// Adds a target, an event or a band.
    pub fn annotation(mut self, annotation: Annotation) -> Self {
        self.annotations.push(annotation);
        self
    }

    /// The plot's height in pixels, 60 to 800 (200 by default). The x labels add 22.
    pub fn height(mut self, px: u32) -> Self {
        self.height = px.clamp(60, 800);
        self
    }

    /// The most points a line or an area draws (640 by default, at least 16): more are
    /// reduced with largest-triangle-three-buckets.
    pub fn max_points(mut self, most: usize) -> Self {
        self.max_points = most.max(16);
        self
    }

    /// The most bars (120 by default, at least 4): times and numbers past it merge
    /// their neighbours, categories fold their tail into Other.
    pub fn max_bars(mut self, most: usize) -> Self {
        self.max_bars = most.max(4);
        self
    }

    /// How merged bars combine.
    pub fn merge(mut self, merge: Merge) -> Self {
        self.merge = merge;
        self
    }

    /// Makes the legend a set of switches that hide and show each series, with or
    /// without JavaScript. Not for stacked charts, whose stacks would lose a layer.
    pub fn toggles(mut self) -> Self {
        self.toggles = true;
        self
    }

    /// Leaves out the dot at the end of each line.
    pub fn no_dots(mut self) -> Self {
        self.dots = false;
        self
    }

    /// The first day of a week, for weekly ticks (Monday by default).
    pub fn week_start(mut self, day: Weekday) -> Self {
        self.week_start = day;
        self
    }

    /// Mirrors the chart for a right-to-left page: time runs right to left and the
    /// value axis stands on the right.
    pub fn rtl(mut self) -> Self {
        self.rtl = true;
        self
    }

    fn into_node(self) -> Node {
        let kind = match (self.kind, self.horizontal) {
            (Kind::Line, _) => "line",
            (Kind::Area, _) => "area",
            (Kind::Bar, false) => "bar",
            (Kind::Bar, true) => "bar-horizontal",
        };
        let height = if self.horizontal {
            0
        } else {
            self.height + X_BAND as u32 + TOP as u32
        };
        let mut this = self;
        let drawn = this.draw();
        let height = if this.horizontal {
            drawn.as_ref().map_or(160, |d| d.height)
        } else {
            height
        };
        frame(this.common, drawn, kind, height)
    }
}

crate::node::into_node!(Chart);

/// A row of the table the chart draws.
struct Row {
    /// Where x is, from 0 to 100.
    x: f64,
    /// The full name: the table's row header and the tooltip's heading.
    name: String,
    /// The first row of the data it stands for.
    start: usize,
}

/// What a series looks like once it is ready to draw.
struct Ready {
    name: String,
    color: Color,
    values: Vec<Option<f64>>,
}

/// One series' figures for the summary, over all its data.
struct Stats {
    first: Option<(usize, f64)>,
    last: Option<(usize, f64)>,
    min: Option<(usize, f64)>,
    max: Option<(usize, f64)>,
    total: f64,
}

fn stats(values: &[Option<f64>]) -> Stats {
    let mut out = Stats {
        first: None,
        last: None,
        min: None,
        max: None,
        total: 0.0,
    };
    for (at, value) in values.iter().enumerate() {
        let Some(v) = *value else { continue };
        if out.first.is_none() {
            out.first = Some((at, v));
        }
        out.last = Some((at, v));
        if out.min.is_none_or(|(_, m)| v < m) {
            out.min = Some((at, v));
        }
        if out.max.is_none_or(|(_, m)| v > m) {
            out.max = Some((at, v));
        }
        out.total += v;
    }
    out
}

impl Chart {
    /// The time grain: the one set, or the spacing of the times.
    fn time_grain(&self, times: &[i64]) -> Grain {
        if let Some(grain) = self.grain {
            return grain;
        }
        let spacing = times
            .windows(2)
            .map(|w| w[1].saturating_sub(w[0]))
            .filter(|gap| *gap > 0)
            .min()
            .unwrap_or(86_400);
        Grain::of_spacing(spacing)
    }

    /// The full name of x at `at`.
    fn x_name(&self, at: usize, grain: Grain) -> String {
        match &self.x {
            X::None => (at + 1).to_string(),
            X::Categories(names) => names.get(at).cloned().unwrap_or_default(),
            X::Times(times) => match times.get(at).copied().and_then(time_of) {
                Some(t) => {
                    let name = time_name(t, grain, &self.common.dates);
                    let name = if grain == Grain::Week {
                        fill(&self.common.labels.week_of, &[("date", &name)])
                    } else {
                        name
                    };
                    // The clocks went back: the second of two equal times is that hour again.
                    if at > 0 && times.get(at - 1) == times.get(at) {
                        fill(&self.common.labels.again, &[("time", &name)])
                    } else {
                        name
                    }
                }
                None => String::new(),
            },
            X::Numbers(values) => values
                .get(at)
                .map(|v| NumberFormat::number().format(*v, &self.common.numbers))
                .unwrap_or_default(),
        }
    }

    /// Puts rows in the order of x when x is times or numbers given out of order.
    fn sort_rows(&mut self) {
        let order: Option<Vec<usize>> = match &self.x {
            X::Times(times) if times.windows(2).any(|w| w[0] > w[1]) => {
                let mut order: Vec<usize> = (0..times.len()).collect();
                order.sort_by_key(|i| times[*i]);
                Some(order)
            }
            X::Numbers(values) if values.windows(2).any(|w| w[0] > w[1]) => {
                let mut order: Vec<usize> = (0..values.len()).collect();
                order.sort_by(|a, b| values[*a].total_cmp(&values[*b]));
                Some(order)
            }
            _ => None,
        };
        let Some(order) = order else { return };
        match &mut self.x {
            X::Times(times) => *times = order.iter().map(|i| times[*i]).collect(),
            X::Numbers(values) => *values = order.iter().map(|i| values[*i]).collect(),
            _ => {}
        }
        for series in &mut self.series {
            series.values = order
                .iter()
                .map(|i| series.values.get(*i).copied().flatten())
                .collect();
        }
    }

    /// Folds series past the eighth into Other and gives each its colour.
    fn ready_series(&mut self, n: usize) -> Vec<Ready> {
        let mut series = std::mem::take(&mut self.series);
        for s in &mut series {
            s.values.resize(n, None);
        }
        if series.len() > MAX_SERIES {
            let rest = series.split_off(MAX_SERIES - 1);
            let values = (0..n)
                .map(|at| {
                    let found: Vec<f64> = rest.iter().filter_map(|s| s.values[at]).collect();
                    (!found.is_empty()).then(|| found.iter().sum())
                })
                .collect();
            series.push(Series {
                name: self.common.labels.other.to_string(),
                values,
                color: Some(Color::Other),
            });
        }
        // Categorical slots go in order to the series that mean nothing by colour.
        let mut slot = 0_u8;
        series
            .into_iter()
            .map(|s| Ready {
                color: s.color.unwrap_or_else(|| {
                    slot += 1;
                    Color::Slot(slot)
                }),
                name: s.name,
                values: s.values,
            })
            .collect()
    }

    fn draw(&mut self) -> Option<Drawn> {
        let n = self.x.len().max(
            self.series
                .iter()
                .map(|s| s.values.len())
                .max()
                .unwrap_or(0),
        );
        if n == 0 || self.series.is_empty() {
            return None;
        }
        if let X::None = self.x {
            self.x = X::Numbers((1..=n).map(|i| i as f64).collect());
        }
        // x shorter than the values: the extra values have no place.
        let n = n.min(self.x.len());
        self.sort_rows();
        let mut series = self.ready_series(n);
        if series.iter().all(|s| s.values.iter().all(Option::is_none)) {
            return None;
        }
        let grain = match &self.x {
            X::Times(times) => self.time_grain(times),
            _ => Grain::Day,
        };
        let labels = self.common.labels.clone();
        let numbers = self.common.numbers.clone();
        let fmt = |v: f64| self.format.format(v, &numbers);

        // The summary, over every value before any is merged or left out.
        let bars = self.kind == Kind::Bar;
        let mut summary = Vec::with_capacity(series.len());
        let mut note = None;
        for s in &series {
            let st = stats(&s.values);
            let name_of = |at: usize| self.x_name(at, grain);
            let text = match (st.first, st.last, st.min, st.max) {
                (None, ..) => fill(&labels.summary_none, &[("series", &s.name)]),
                (Some((a, v)), Some((b, _)), ..) if a == b => {
                    if !bars {
                        note = Some(labels.one_point.to_string());
                    }
                    fill(
                        &labels.summary_one,
                        &[("series", &s.name), ("value", &fmt(v)), ("x", &name_of(a))],
                    )
                }
                (Some((fa, fv)), Some((la, lv)), Some((mina, minv)), Some((maxa, maxv))) => {
                    if bars {
                        fill(
                            &labels.summary_bars,
                            &[
                                ("series", &s.name),
                                ("max", &fmt(maxv)),
                                ("max_x", &name_of(maxa)),
                                ("min", &fmt(minv)),
                                ("min_x", &name_of(mina)),
                                ("total", &fmt(st.total)),
                            ],
                        )
                    } else {
                        fill(
                            &labels.summary_series,
                            &[
                                ("series", &s.name),
                                ("first", &fmt(fv)),
                                ("first_x", &name_of(fa)),
                                ("last", &fmt(lv)),
                                ("last_x", &name_of(la)),
                                ("max", &fmt(maxv)),
                                ("max_x", &name_of(maxa)),
                                ("min", &fmt(minv)),
                                ("min_x", &name_of(mina)),
                            ],
                        )
                    }
                }
                _ => String::new(),
            };
            summary.push(text);
        }
        if series.iter().any(|s| s.values.iter().flatten().count() > 1) {
            note = None;
        }
        let marks: Vec<String> = self
            .annotations
            .iter()
            .map(|a| match a {
                Annotation::Target { value, label } => format!("{label} ({})", fmt(*value)),
                Annotation::Event { label, .. } | Annotation::Band { label, .. } => label.clone(),
            })
            .collect();
        if !marks.is_empty() {
            summary.push(fill(&labels.summary_marks, &[("marks", &marks.join(", "))]));
        }

        // Fewer points than pixels.
        let total_rows = n;
        let mut rows_kept: Vec<std::ops::Range<usize>> = (0..n).map(|i| i..i + 1).collect();
        let mut other_bar = false;
        if bars {
            if n > self.max_bars {
                if let X::Categories(_) = self.x {
                    let keep = self.max_bars - 1;
                    rows_kept = (0..keep).map(|i| i..i + 1).collect();
                    rows_kept.push(keep..n);
                    other_bar = true;
                } else {
                    rows_kept = merge_ranges(n, self.max_bars);
                }
            }
        } else if n > self.max_points {
            let xs: Vec<f64> = match &self.x {
                X::Times(t) => t.iter().map(|v| *v as f64).collect(),
                X::Numbers(v) => v.clone(),
                _ => (0..n).map(|i| i as f64).collect(),
            };
            let columns: Vec<&[Option<f64>]> = series.iter().map(|s| &s.values[..]).collect();
            rows_kept = rows_to_keep(&xs, &columns, self.max_points)
                .into_iter()
                .map(|i| i..i + 1)
                .collect();
        }
        let reduced = rows_kept.len() < total_rows || rows_kept.iter().any(|r| r.len() > 1);
        if reduced {
            for s in &mut series {
                s.values = rows_kept
                    .iter()
                    .map(|range| {
                        let found: Vec<f64> =
                            s.values[range.clone()].iter().flatten().copied().collect();
                        if found.is_empty() {
                            return None;
                        }
                        Some(match self.merge {
                            Merge::Sum => found.iter().sum(),
                            Merge::Mean => found.iter().sum::<f64>() / found.len() as f64,
                            Merge::Max => found.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                        })
                        .filter(|v: &f64| v.is_finite())
                    })
                    .collect();
            }
        }
        let m = rows_kept.len();

        // Where each row sits, and its name.
        let rows: Vec<Row> = rows_kept
            .iter()
            .enumerate()
            .map(|(at, range)| {
                let name = if other_bar && at == m - 1 {
                    labels.other.to_string()
                } else if range.len() > 1 {
                    format!(
                        "{} – {}",
                        self.x_name(range.start, grain),
                        self.x_name(range.end - 1, grain)
                    )
                } else {
                    self.x_name(range.start, grain)
                };
                let x = if bars {
                    (at as f64 + 0.5) * 100.0 / m as f64
                } else {
                    match &self.x {
                        X::Times(t) => {
                            linear_x(t[range.start] as f64, t[0] as f64, t[n - 1] as f64)
                        }
                        X::Numbers(v) => linear_x(v[range.start], v[0], v[n - 1]),
                        // A kept row stands where its data row stood.
                        _ if n == 1 => 50.0,
                        _ => range.start as f64 * 100.0 / (n - 1) as f64,
                    }
                };
                Row {
                    x: if self.rtl { 100.0 - x } else { x },
                    name,
                    start: range.start,
                }
            })
            .collect();

        // The values as drawn: stacked, or as they are.
        let stacked = self.stack != Stack::None && series.len() > 1;
        let percent = self.stack == Stack::Percent;
        let shown: Vec<Vec<Option<f64>>> = if percent {
            let totals: Vec<f64> = (0..m)
                .map(|i| {
                    series
                        .iter()
                        .filter_map(|s| s.values[i])
                        .filter(|v| *v > 0.0)
                        .sum()
                })
                .collect();
            series
                .iter()
                .map(|s| {
                    s.values
                        .iter()
                        .zip(&totals)
                        .map(|(v, t)| v.map(|v| if *t > 0.0 { v.max(0.0) / t } else { 0.0 }))
                        .collect()
                })
                .collect()
        } else {
            series.iter().map(|s| s.values.clone()).collect()
        };
        // Each series' lower and upper edge at each row.
        let edges: Vec<Vec<Option<(f64, f64)>>> = if stacked {
            let mut up = vec![0.0; m];
            let mut down = vec![0.0; m];
            shown
                .iter()
                .map(|values| {
                    values
                        .iter()
                        .enumerate()
                        .map(|(i, v)| {
                            let v = (*v)?;
                            let (base, top) = if v >= 0.0 {
                                let base = up[i];
                                up[i] += v;
                                (base, up[i])
                            } else {
                                let base = down[i];
                                down[i] += v;
                                (base, down[i])
                            };
                            Some((base, top))
                        })
                        .collect()
                })
                .collect()
        } else {
            shown
                .iter()
                .map(|values| values.iter().map(|v| v.map(|v| (0.0, v))).collect())
                .collect()
        };

        // The value scale.
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for column in &edges {
            for (base, top) in column.iter().flatten() {
                if stacked || bars || self.kind == Kind::Area {
                    lo = lo.min(*base);
                    hi = hi.max(*base);
                }
                lo = lo.min(*top);
                hi = hi.max(*top);
            }
        }
        for a in &self.annotations {
            if let Annotation::Target { value, .. } = a
                && !percent
            {
                lo = lo.min(*value);
                hi = hi.max(*value);
            }
        }
        let zero = bars || self.kind == Kind::Area || stacked || self.y_zero.unwrap_or(true);
        if zero {
            lo = lo.min(0.0);
            hi = hi.max(0.0);
        }
        if percent {
            lo = lo.min(0.0);
            hi = hi.max(1.0);
        }
        if let Some(v) = self.y_min {
            lo = lo.min(v);
        }
        if let Some(v) = self.y_max {
            hi = hi.max(v);
        }
        let format = if percent {
            NumberFormat::percent().max_decimals(0)
        } else {
            self.format.clone()
        };
        let count = usize::from(self.y_ticks.unwrap_or((self.height / 50).clamp(2, 8) as u8));
        let ticks = nice(lo, hi, count, format.whole());
        let compact = format.axis_compact(ticks.values.iter().fold(0.0_f64, |m, v| m.max(v.abs())));
        let tick_text = |v: f64| {
            if compact {
                format.format_compact(v, &numbers)
            } else {
                format.format_places(v, &numbers, ticks.places)
            }
        };

        // The table: the rows drawn, every series, a total for stacks.
        let mut head = vec![(
            self.x_label.clone().unwrap_or_else(|| match self.x {
                X::Times(_) => labels.time.to_string(),
                X::Numbers(_) => labels.value.to_string(),
                _ => labels.category.to_string(),
            }),
            None,
        )];
        head.extend(series.iter().map(|s| (s.name.clone(), Some(s.color))));
        if stacked {
            head.push((labels.total.to_string(), None));
        }
        let table_rows = rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let mut cells: Vec<Node> = series
                    .iter()
                    .map(|s| value_cell(s.values[i].map(fmt), &labels))
                    .collect();
                if stacked {
                    let sum: Option<f64> = series
                        .iter()
                        .filter_map(|s| s.values[i])
                        .fold(None, |acc, v| Some(acc.unwrap_or(0.0) + v));
                    cells.push(value_cell(sum.map(fmt), &labels));
                }
                (row.name.clone(), cells)
            })
            .collect();
        let table = TableData {
            head,
            rows: table_rows,
            note: reduced.then(|| {
                fill(
                    &labels.reduced,
                    &[
                        ("shown", &NumberFormat::number().format(m as f64, &numbers)),
                        (
                            "total",
                            &NumberFormat::number().format(total_rows as f64, &numbers),
                        ),
                    ],
                )
            }),
        };

        let legend_node = (series.len() > 1).then(|| {
            let items: Vec<LegendItem> = series
                .iter()
                .map(|s| LegendItem {
                    name: s.name.clone(),
                    color: s.color,
                    key: if self.kind == Kind::Line {
                        Key::Line
                    } else {
                        Key::Block
                    },
                })
                .collect();
            legend(&self.common.id(), &items, self.toggles && !stacked)
        });

        let plan = Plan {
            rows: &rows,
            series: &series,
            edges: &edges,
            scale: ticks.scale,
            tick_values: &ticks.values,
            tick_text: &tick_text,
            height: f64::from(self.height),
            stacked,
        };
        let (body, body_attrs, height) = if self.horizontal && bars {
            self.horizontal_body(&plan)
        } else {
            self.vertical_body(&plan, grain, n)
        };
        Some(Drawn {
            legend: legend_node,
            body,
            body_attrs,
            summary: summary.join(" "),
            table,
            height,
            note,
            interactive: true,
        })
    }
}

/// Where `v` sits between `lo` and `hi`, from 0 to 100.
fn linear_x(v: f64, lo: f64, hi: f64) -> f64 {
    if hi > lo {
        ((v - lo) / (hi - lo) * 100.0).clamp(0.0, 100.0)
    } else {
        50.0
    }
}

/// What both orientations draw from.
struct Plan<'a> {
    rows: &'a [Row],
    series: &'a [Ready],
    edges: &'a [Vec<Option<(f64, f64)>>],
    scale: Linear,
    tick_values: &'a [f64],
    tick_text: &'a dyn Fn(f64) -> String,
    height: f64,
    stacked: bool,
}

impl Plan<'_> {
    fn y(&self, value: f64) -> f64 {
        TOP + (1.0 - self.scale.at(value)) * self.height
    }
}

/// A gridline's y on a whole pixel's middle, so a hairline stays one pixel.
fn crisp(y: f64) -> f64 {
    y.floor() + 0.5
}

/// The clip that turns a band-wide rect into bar `slot` of `count` side by side, at most
/// 24 pixels wide, its data end rounded when `round` names the corners.
fn bar_clip(count: usize, slot: usize, round: Option<&str>) -> String {
    let mut out = String::with_capacity(96);
    if count <= 1 {
        out.push_str("inset(0 max(20%, calc(50% - 12px))");
    } else {
        let k = count as f64;
        let group = format!("min(70%, {}px)", 26 * count - 2);
        let side = |j: usize| {
            let a = 0.5 - j as f64 / k;
            let b = 2.0 * j as f64 / k;
            format!("calc(50% - {group} * {} + {}px)", n(a, 4), n(b, 3))
        };
        let _ = write!(out, "inset(0 {} 0 {}", side(count - 1 - slot), side(slot));
    }
    if let Some(corners) = round {
        out.push_str(" round ");
        out.push_str(corners);
    }
    out.push(')');
    out
}

/// A run of points as a path in the stretched layer (x 0 to 1000), straight or as a
/// monotone curve.
fn path_of(out: &mut String, points: &[(f64, f64)], smooth: bool, join: bool) {
    let x = |v: f64| v * 10.0;
    for (at, (px, py)) in points.iter().enumerate() {
        if at == 0 {
            out.push(if join { 'L' } else { 'M' });
            num(out, x(*px), 1);
            out.push(' ');
            num(out, *py, 1);
            continue;
        }
        if !smooth || points.len() < 3 {
            out.push('L');
            num(out, x(*px), 1);
            out.push(' ');
            num(out, *py, 1);
        }
    }
    if smooth && points.len() >= 3 {
        // Fritsch–Carlson tangents: no overshoot between two values.
        let m = points.len();
        let slope = |i: usize| {
            let dx = points[i + 1].0 - points[i].0;
            if dx.abs() < 1e-12 {
                0.0
            } else {
                (points[i + 1].1 - points[i].1) / dx
            }
        };
        let d: Vec<f64> = (0..m - 1).map(slope).collect();
        let mut t = vec![0.0; m];
        t[0] = d[0];
        t[m - 1] = d[m - 2];
        for i in 1..m - 1 {
            t[i] = if d[i - 1] * d[i] <= 0.0 {
                0.0
            } else {
                let h0 = points[i].0 - points[i - 1].0;
                let h1 = points[i + 1].0 - points[i].0;
                let w1 = 2.0 * h1 + h0;
                let w2 = h1 + 2.0 * h0;
                (w1 + w2) / (w1 / d[i - 1] + w2 / d[i])
            };
        }
        for i in 0..m - 1 {
            let (x0, y0) = points[i];
            let (x1, y1) = points[i + 1];
            let h = (x1 - x0) / 3.0;
            out.push('C');
            for (cx, cy) in [
                (x0 + h, y0 + t[i] * h),
                (x1 - h, y1 - t[i + 1] * h),
                (x1, y1),
            ] {
                num(out, x(cx), 1);
                out.push(' ');
                num(out, cy, 1);
                out.push(' ');
            }
            out.pop();
        }
    }
}

impl Chart {
    /// Bars up from a baseline, or lines and areas, with the value axis at the side.
    fn vertical_body(
        &self,
        plan: &Plan<'_>,
        grain: Grain,
        rows_n: usize,
    ) -> (Node, Vec<(&'static str, AttrValue)>, u32) {
        let base = TOP + plan.height;
        let total = base + X_BAND;
        let rows = plan.rows;
        let m = rows.len();
        let bars = self.kind == Kind::Bar;

        // The value axis.
        let mut widest = 0;
        let ticks: Vec<(f64, String)> = plan
            .tick_values
            .iter()
            .map(|v| {
                let text = (plan.tick_text)(*v);
                widest = widest.max(text.chars().count());
                (*v, text)
            })
            .collect();
        let axis_w = (widest as f64 * 6.6 + 10.0).clamp(24.0, 120.0).round();
        let y_axis = svg()
            .slot("chart-y-axis")
            .attr("width", n(axis_w, 0))
            .attr("height", n(total, 0))
            .child(
                ticks
                    .iter()
                    .map(|(v, text)| {
                        el("text")
                            .attr(
                                "x",
                                if self.rtl {
                                    "6".to_owned()
                                } else {
                                    n(axis_w - 6.0, 0)
                                },
                            )
                            .attr("y", n(plan.y(*v), 1))
                            .attr("dy", "0.32em")
                            .attr("text-anchor", if self.rtl { "start" } else { "end" })
                            .child(text.clone())
                    })
                    .collect::<Vec<_>>(),
            );

        let mut plot: Vec<Node> = Vec::new();
        // Gridlines, the zero line when values cross it.
        for (v, _) in &ticks {
            let y = crisp(plan.y(*v));
            plot.push(
                el("line")
                    .slot(if *v == 0.0 {
                        "chart-zero"
                    } else {
                        "chart-grid"
                    })
                    .attr("x1", "0")
                    .attr("x2", "100%")
                    .attr("y1", n(y, 1))
                    .attr("y2", n(y, 1))
                    .into(),
            );
        }
        if !plan.tick_values.contains(&0.0) && plan.scale.lo < 0.0 && plan.scale.hi > 0.0 {
            let y = crisp(plan.y(0.0));
            plot.push(
                el("line")
                    .slot("chart-zero")
                    .attr("x1", "0")
                    .attr("x2", "100%")
                    .attr("y1", n(y, 1))
                    .attr("y2", n(y, 1))
                    .into(),
            );
        }

        // Bands behind the data.
        for a in &self.annotations {
            if let Annotation::Band { from, to, label } = a {
                let (Some(a), Some(b)) =
                    (self.x_of(from, plan, rows_n), self.x_of(to, plan, rows_n))
                else {
                    continue;
                };
                let (a, b) = (a.min(b), a.max(b));
                let half = if bars { 50.0 / m as f64 } else { 0.0 };
                let (a, b) = ((a - half).max(0.0), (b + half).min(100.0));
                plot.push(
                    el("g")
                        .slot("chart-band")
                        .children((
                            el("rect")
                                .attr("x", pct(a))
                                .attr("width", pct(b - a))
                                .attr("y", n(TOP, 0))
                                .attr("height", n(plan.height, 0)),
                            mark_text(label)
                                .attr("x", pct(a))
                                .attr("dx", "4")
                                .attr("y", n(TOP + 12.0, 0)),
                        ))
                        .into(),
                );
            }
        }

        // The x labels.
        plot.push(self.x_labels(plan, grain, rows_n, total).into());

        // The marks.
        let mut dots: Vec<Node> = Vec::new();
        if bars {
            let k = if plan.stacked { 1 } else { plan.series.len() };
            let band = 100.0 / m as f64;
            let mut marks: Vec<Node> = Vec::with_capacity(m * plan.series.len());
            for (s, ready) in plan.series.iter().enumerate() {
                // In a stack, the outermost segment of each side is the one with the rounded end.
                for (i, edge) in plan.edges[s].iter().enumerate() {
                    let Some((lo, hi)) = *edge else { continue };
                    if hi == lo {
                        continue;
                    }
                    let up = hi > lo;
                    let outer = !plan.stacked
                        || !plan.edges[s + 1..]
                            .iter()
                            .any(|e| e[i].is_some_and(|(b, t)| t != b && (t > b) == up));
                    let (y0, y1) = (plan.y(lo), plan.y(hi));
                    let mut top = y0.min(y1);
                    let mut h = (y0 - y1).abs().max(1.0);
                    if plan.stacked && !outer && h > 3.0 {
                        h -= 2.0;
                        if up {
                            top += 2.0;
                        }
                    }
                    let corners = outer.then_some(if up { "4px 4px 0 0" } else { "0 0 4px 4px" });
                    let x = if self.rtl {
                        100.0 - band * (i + 1) as f64
                    } else {
                        band * i as f64
                    };
                    marks.push(
                        ready
                            .color
                            .on(el("rect")
                                .slot("chart-bar")
                                .attr("data-i", i as i64)
                                .attr("data-s", s as i64)
                                .attr("x", pct(x))
                                .attr("width", pct(band))
                                .attr("y", n(top, 1))
                                .attr("height", n(h, 1))
                                .attr(
                                    "clip-path",
                                    bar_clip(k, if plan.stacked { 0 } else { s }, corners),
                                ))
                            .into(),
                    );
                }
            }
            plot.push(el("g").slot("chart-bars").child(marks).into());
        } else {
            let area = self.kind == Kind::Area;
            let mut layers: Vec<Node> = Vec::new();
            let mut lines: Vec<Node> = Vec::new();
            for (s, ready) in plan.series.iter().enumerate() {
                let edges = &plan.edges[s];
                let mut line_d = String::with_capacity(m * 12);
                let mut area_d = String::with_capacity(m * 24);
                let mut i = 0;
                while i < m {
                    if edges[i].is_none() {
                        i += 1;
                        continue;
                    }
                    let start = i;
                    while i < m && edges[i].is_some() {
                        i += 1;
                    }
                    let upper: Vec<(f64, f64)> = (start..i)
                        .filter_map(|j| edges[j].map(|(_, top)| (rows[j].x, plan.y(top))))
                        .collect();
                    if upper.len() == 1 {
                        // One value between gaps: a dot, as a line needs two.
                        let (x, y) = upper[0];
                        dots.push(
                            ready
                                .color
                                .on(el("circle")
                                    .slot("chart-point")
                                    .attr("data-s", s as i64)
                                    .attr("cx", pct(x))
                                    .attr("cy", n(y, 1))
                                    .attr("r", "4"))
                                .into(),
                        );
                        continue;
                    }
                    path_of(&mut line_d, &upper, self.smooth, false);
                    if area {
                        let lower: Vec<(f64, f64)> = (start..i)
                            .rev()
                            .filter_map(|j| edges[j].map(|(base, _)| (rows[j].x, plan.y(base))))
                            .collect();
                        path_of(&mut area_d, &upper, self.smooth, false);
                        path_of(&mut area_d, &lower, self.smooth, true);
                        area_d.push('Z');
                    }
                }
                if area && !area_d.is_empty() {
                    layers.push(
                        ready
                            .color
                            .on(el("path")
                                .slot("chart-area")
                                .attr("data-s", s as i64)
                                .attr("d", area_d))
                            .into(),
                    );
                }
                if !line_d.is_empty() {
                    lines.push(
                        ready
                            .color
                            .on(el("path")
                                .slot("chart-line")
                                .attr("data-s", s as i64)
                                .attr("d", line_d)
                                .attr("vector-effect", "non-scaling-stroke"))
                            .into(),
                    );
                }
                // The end dot: where the series stands now.
                if self.dots
                    && let Some(j) = (0..m).rev().find(|j| edges[*j].is_some())
                    && let Some((_, top)) = edges[j]
                {
                    let y = plan.y(top);
                    dots.push(
                        ready
                            .color
                            .on(el("circle")
                                .slot("chart-end")
                                .attr("data-s", s as i64)
                                .attr("cx", pct(rows[j].x))
                                .attr("cy", n(y, 1))
                                .attr("r", "4"))
                            .into(),
                    );
                }
            }
            layers.extend(lines);
            plot.push(
                svg()
                    .slot("chart-marks")
                    .attr("x", "0")
                    .attr("y", "0")
                    .attr("width", "100%")
                    .attr("height", n(total, 0))
                    .attr("viewBox", format!("0 0 1000 {}", n(total, 0)))
                    .attr("preserveAspectRatio", "none")
                    .attr("overflow", "visible")
                    .child(layers)
                    .into(),
            );
        }

        // Targets and events over the data.
        for a in &self.annotations {
            match a {
                Annotation::Target { value, label } => {
                    let y = crisp(plan.y(*value));
                    plot.push(
                        el("g")
                            .slot("chart-target")
                            .children((
                                el("line")
                                    .attr("x1", "0")
                                    .attr("x2", "100%")
                                    .attr("y1", n(y, 1))
                                    .attr("y2", n(y, 1)),
                                mark_text(label)
                                    .attr("x", if self.rtl { "0" } else { "100%" })
                                    .attr("y", n(y - 4.0, 1))
                                    .attr("text-anchor", if self.rtl { "start" } else { "end" }),
                            ))
                            .into(),
                    );
                }
                Annotation::Event { at, label } => {
                    let Some(x) = self.x_of(at, plan, rows_n) else {
                        continue;
                    };
                    let anchor = if x > 70.0 { "end" } else { "start" };
                    plot.push(
                        el("g")
                            .slot("chart-event")
                            .children((
                                el("line")
                                    .attr("x1", pct(x))
                                    .attr("x2", pct(x))
                                    .attr("y1", n(TOP, 0))
                                    .attr("y2", n(base, 0)),
                                mark_text(label)
                                    .attr("x", pct(x))
                                    .attr("dx", if anchor == "end" { "-4" } else { "4" })
                                    .attr("y", n(TOP + 10.0, 0))
                                    .attr("text-anchor", anchor),
                            ))
                            .into(),
                    );
                }
                Annotation::Band { .. } => {}
            }
        }
        plot.extend(dots);

        // What the module moves: the cursor, a dot per line, and where each row is.
        let mut xs = String::with_capacity(m * 6);
        for row in rows {
            num(&mut xs, row.x, 2);
            xs.push(' ');
        }
        xs.pop();
        if !bars {
            plot.push(
                el("line")
                    .slot("chart-cursor")
                    .attr("x1", "0")
                    .attr("x2", "0")
                    .attr("y1", n(TOP, 0))
                    .attr("y2", n(base, 0))
                    .attr("visibility", "hidden")
                    .into(),
            );
            for (s, ready) in plan.series.iter().enumerate() {
                let mut ys = String::with_capacity(m * 6);
                for edge in &plan.edges[s] {
                    match edge {
                        Some((_, top)) => num(&mut ys, plan.y(*top), 1),
                        None => ys.push('-'),
                    }
                    ys.push(' ');
                }
                ys.pop();
                plot.push(
                    ready
                        .color
                        .on(el("circle")
                            .slot("chart-dot")
                            .attr("data-s", s as i64)
                            .attr("data-ys", ys)
                            .attr("cx", "0")
                            .attr("cy", "0")
                            .attr("r", "4")
                            .attr("visibility", "hidden"))
                        .into(),
                );
            }
        }
        let plot = svg()
            .slot("chart-plot")
            .attr("width", "100%")
            .attr("height", n(total, 0))
            .attr("overflow", "visible")
            .child(plot);
        let body = el("div")
            .slot("chart-frame")
            .attr_opt("data-rtl", self.rtl.then_some(""))
            .children((y_axis, el("div").slot("chart-plot-wrap").child(plot)));
        let attrs = vec![
            (
                "data-mode",
                AttrValue::Static(if bars { "band" } else { "point" }),
            ),
            ("data-xs", AttrValue::from(xs)),
            ("data-rows", AttrValue::Int(m as i64)),
        ];
        (body.into(), attrs, total as u32)
    }

    /// Where an annotation's place is, in percent, when the chart has it.
    fn x_of(&self, at: &At, plan: &Plan<'_>, rows_n: usize) -> Option<f64> {
        let rows = plan.rows;
        let bars = self.kind == Kind::Bar;
        let pos = |x: f64| if self.rtl { 100.0 - x } else { x };
        // The band that holds data row `i`: bars that merged it, or the Other bar.
        let band = |i: usize| -> Option<f64> {
            let r = rows.partition_point(|row| row.start <= i).checked_sub(1)?;
            rows.get(r).map(|row| row.x)
        };
        // Where data row `i` stands, whatever rows the drawing kept.
        let place = |i: usize| -> Option<f64> {
            if i >= rows_n {
                return None;
            }
            if bars {
                return band(i);
            }
            Some(pos(match &self.x {
                X::Times(t) => linear_x(t[i] as f64, t[0] as f64, t[rows_n - 1] as f64),
                X::Numbers(v) => linear_x(v[i], v[0], v[rows_n - 1]),
                _ if rows_n <= 1 => 50.0,
                _ => i as f64 * 100.0 / (rows_n - 1) as f64,
            }))
        };
        match (at, &self.x) {
            (At::Index(i), _) => place(*i),
            (At::Category(name), X::Categories(names)) => {
                place(names.iter().position(|c| c == name)?)
            }
            (At::Time(t), X::Times(times)) => {
                let t = seconds_of(*t);
                if bars {
                    let i = times
                        .iter()
                        .position(|v| *v >= t)
                        .unwrap_or(times.len().saturating_sub(1));
                    band(i.min(rows_n.saturating_sub(1)))
                } else {
                    let (lo, hi) = (*times.first()?, *times.get(rows_n.saturating_sub(1))?);
                    (lo..=hi)
                        .contains(&t)
                        .then(|| pos(linear_x(t as f64, lo as f64, hi as f64)))
                }
            }
            (At::Number(v), X::Numbers(values)) => {
                let (lo, hi) = (*values.first()?, *values.get(rows_n.saturating_sub(1))?);
                (lo..=hi).contains(v).then(|| pos(linear_x(*v, lo, hi)))
            }
            _ => None,
        }
    }

    /// The labels under the plot.
    fn x_labels(&self, plan: &Plan<'_>, grain: Grain, rows_n: usize, total: f64) -> Element {
        let rows = plan.rows;
        let m = rows.len();
        let y = n_(total - 6.0);
        let mut labels: Vec<(f64, String)> = Vec::new();
        let bars = self.kind == Kind::Bar;
        match &self.x {
            X::Times(times) if !bars => {
                let (lo, hi) = (times[0], times[rows_n - 1]);
                for tick in time_ticks(lo, hi, 7, &self.common.dates, self.week_start) {
                    let x = linear_x(tick.at as f64, lo as f64, hi as f64);
                    labels.push((if self.rtl { 100.0 - x } else { x }, tick.label));
                }
            }
            X::Numbers(values) if !bars => {
                let (lo, hi) = (values[0], values[rows_n - 1]);
                let ticks = nice(lo, hi, 6, false);
                for v in ticks.values.iter().filter(|v| (lo..=hi).contains(*v)) {
                    let x = linear_x(*v, lo, hi);
                    labels.push((
                        if self.rtl { 100.0 - x } else { x },
                        NumberFormat::number().format_places(
                            *v,
                            &self.common.numbers,
                            ticks.places,
                        ),
                    ));
                }
            }
            _ => {
                let every = m.div_ceil(9).max(1);
                let mut last_year = None;
                for row in rows.iter().step_by(every) {
                    let text = match &self.x {
                        X::Times(times) => {
                            let at = times.get(row.start).copied().and_then(time_of);
                            match at {
                                Some(t) => {
                                    use chrono::Datelike;
                                    let first = last_year.is_none();
                                    let with_year = (first || last_year != Some(t.year()))
                                        && times
                                            .first()
                                            .and_then(|a| time_of(*a))
                                            .map(|a| a.year())
                                            != times
                                                .last()
                                                .and_then(|a| time_of(*a))
                                                .map(|a| a.year());
                                    last_year = Some(t.year());
                                    time_label(t, grain, &self.common.dates, with_year)
                                }
                                None => row.name.clone(),
                            }
                        }
                        _ => short(&row.name, 16),
                    };
                    labels.push((row.x, text));
                }
            }
        }
        // On a narrow chart every other label goes, when there are enough to crowd.
        let many = labels.len() > 6;
        let texts: Vec<Node> = labels
            .into_iter()
            .enumerate()
            .map(|(j, (x, text))| {
                let anchor = if x < 4.0 {
                    "start"
                } else if x > 96.0 {
                    "end"
                } else {
                    "middle"
                };
                el("text")
                    .attr("x", pct(x))
                    .attr("y", y.clone())
                    .attr("text-anchor", anchor)
                    .attr_opt("data-minor", (many && j % 2 == 1).then_some(""))
                    .child(text)
                    .into()
            })
            .collect();
        el("g").slot("chart-x-axis").child(texts)
    }

    /// Bars along rows: a label, the bar, and its value, with a value axis under them.
    fn horizontal_body(&self, plan: &Plan<'_>) -> (Node, Vec<(&'static str, AttrValue)>, u32) {
        let k = if plan.stacked { 1 } else { plan.series.len() };
        let thick = if k == 1 {
            16.0
        } else {
            (20.0 / k as f64).max(4.0).floor()
        };
        let row_h = if k == 1 {
            ROW
        } else {
            (thick + 2.0) * k as f64 + 10.0
        };
        let mirror = |x: f64| if self.rtl { 100.0 - x } else { x };
        let at = |v: f64| mirror(plan.scale.at(v) * 100.0);
        let grid: Vec<f64> = plan.tick_values.iter().map(|v| at(*v)).collect();
        let numbers = &self.common.numbers;
        let single = plan.series.len() == 1 || plan.stacked;
        let lines = |h: f64| -> Vec<Node> {
            grid.iter()
                .zip(plan.tick_values)
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
                        .attr("y2", n(h, 0))
                        .into()
                })
                .collect()
        };
        let mut rows_out: Vec<Node> = Vec::with_capacity(plan.rows.len() + 1);
        for (i, row) in plan.rows.iter().enumerate() {
            let mut marks: Vec<Node> = lines(row_h);
            for (s, ready) in plan.series.iter().enumerate() {
                let Some((lo, hi)) = plan.edges[s][i] else {
                    continue;
                };
                if lo == hi {
                    continue;
                }
                let up = hi > lo;
                let outer = !plan.stacked
                    || !plan.edges[s + 1..]
                        .iter()
                        .any(|e| e[i].is_some_and(|(b, t)| t != b && (t > b) == up));
                let (a, b) = (at(lo), at(hi));
                let (x, w) = (a.min(b), (a - b).abs());
                let ends_right = b > a;
                let clip = if outer {
                    if ends_right {
                        "inset(0 round 0 4px 4px 0)"
                    } else {
                        "inset(0 round 4px 0 0 4px)"
                    }
                } else if ends_right {
                    "inset(0 2px 0 0)"
                } else {
                    "inset(0 0 0 2px)"
                };
                let y = if plan.stacked || k == 1 {
                    (row_h - thick) / 2.0
                } else {
                    5.0 + s as f64 * (thick + 2.0)
                };
                marks.push(
                    ready
                        .color
                        .on(el("rect")
                            .slot("chart-bar")
                            .attr("data-i", i as i64)
                            .attr("data-s", s as i64)
                            .attr("x", pct(x))
                            .attr("width", pct(w.max(0.2)))
                            .attr("y", n(y, 1))
                            .attr("height", n(thick, 1))
                            .attr("clip-path", clip))
                        .into(),
                );
            }
            for a in &self.annotations {
                if let Annotation::Target { value, .. } = a {
                    let x = at(*value);
                    marks.push(
                        el("line")
                            .slot("chart-target-line")
                            .attr("x1", pct(x))
                            .attr("x2", pct(x))
                            .attr("y1", "0")
                            .attr("y2", n(row_h, 0))
                            .into(),
                    );
                }
            }
            let value = if single {
                let sum: Option<f64> = plan
                    .series
                    .iter()
                    .filter_map(|s| s.values[i])
                    .fold(None, |acc, v| Some(acc.unwrap_or(0.0) + v));
                sum.map(|v| self.format.format(v, numbers))
            } else {
                None
            };
            rows_out.push(
                el("div")
                    .slot("chart-row")
                    .attr("data-i", i as i64)
                    .children((
                        el("span").slot("chart-row-label").child(row.name.clone()),
                        svg()
                            .slot("chart-track")
                            .attr("width", "100%")
                            .attr("height", n(row_h, 0))
                            .attr("overflow", "visible")
                            .child(marks),
                        el("span")
                            .slot("chart-row-value")
                            .child(value.unwrap_or_default()),
                    ))
                    .into(),
            );
        }
        // The value axis.
        let ticks: Vec<Node> = grid
            .iter()
            .zip(plan.tick_values)
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
                    .child((plan.tick_text)(*v))
                    .into()
            })
            .collect();
        let target_labels: Vec<Node> = self
            .annotations
            .iter()
            .filter_map(|a| match a {
                Annotation::Target { value, label } => Some(
                    mark_text(label)
                        .slot("chart-target-label")
                        .attr("x", pct(at(*value)))
                        .attr("dx", "4")
                        .attr("y", "30")
                        .into(),
                ),
                _ => None,
            })
            .collect();
        let axis_h = if target_labels.is_empty() { 20.0 } else { 36.0 };
        rows_out.push(
            el("div")
                .slot("chart-row")
                .attr("data-axis", "")
                .children((
                    el("span").slot("chart-row-label"),
                    svg()
                        .slot("chart-track")
                        .attr("width", "100%")
                        .attr("height", n(axis_h, 0))
                        .attr("overflow", "visible")
                        .child(nodes((ticks, target_labels))),
                    el("span").slot("chart-row-value"),
                ))
                .into(),
        );
        let body = el("div")
            .slot("chart-rows")
            .attr_opt("data-rtl", self.rtl.then_some(""))
            .attr_opt("data-values", (!single).then_some("none"))
            .child(rows_out);
        let height = (plan.rows.len() as f64 * row_h + axis_h) as u32;
        let attrs = vec![
            ("data-mode", AttrValue::Static("rows")),
            ("data-rows", AttrValue::Int(plan.rows.len() as i64)),
        ];
        (body.into(), attrs, height)
    }
}

/// An annotation's words on the drawing: cut to 32 characters, the whole as its title.
fn mark_text(label: &str) -> Element {
    let cut = short(label, 32);
    let whole = (cut != label).then(|| el("title").child(label.to_owned()));
    el("text").children((whole, cut))
}

/// A number as SVG writes it.
fn n_(v: f64) -> String {
    n(v, 1)
}

/// `text` cut to `most` characters, with an ellipsis.
fn short(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(most.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_clips_split_a_band_evenly() {
        assert_eq!(
            bar_clip(1, 0, Some("4px 4px 0 0")),
            "inset(0 max(20%, calc(50% - 12px)) round 4px 4px 0 0)"
        );
        let two = bar_clip(2, 0, None);
        assert_eq!(
            two,
            "inset(0 calc(50% - min(70%, 50px) * 0 + 1px) 0 calc(50% - min(70%, 50px) * 0.5 + 0px))"
        );
    }

    #[test]
    fn smooth_paths_never_overshoot() {
        let mut d = String::new();
        path_of(
            &mut d,
            &[(0.0, 10.0), (50.0, 0.0), (100.0, 0.0)],
            true,
            false,
        );
        assert!(d.starts_with("M0 10C"), "{d}");
        // The flat stretch stays flat: its control points sit on y = 0.
        assert!(d.ends_with("833.3 0 1000 0"), "{d}");
    }

    #[test]
    fn long_names_are_cut_on_the_axis() {
        assert_eq!(
            short("Northern Europe and the Baltics", 16),
            "Northern Europe…"
        );
        assert_eq!(short("Short", 16), "Short");
    }
}
