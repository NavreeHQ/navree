//! [`Chart`]: lines, areas and bars over one x, vertical or horizontal, grouped or
//! stacked.

use std::fmt::Write as _;

use chrono::{NaiveDate, NaiveDateTime, Weekday};

use super::axes::{Axis, Domain, Gridlines, Ticks};
use super::downsample::{merge_ranges, rows_to_keep};
use super::format::NumberFormat;
use super::marks::{
    ActiveDot, Curve, Dots, RefAt, Reference, TooltipStyle, Zone, curve_path, mark_text,
    reference_line, short, zone_rect,
};
use super::motion::{Effect, Motion};
use super::paint::{ColorScale, Defs, Gradient, Ink, glow};
use super::scale::{Grain, Linear, nice, seconds_of, time_label, time_name, time_of, time_ticks};
use super::value::{IntoValue, clean, finite};
use super::{
    Color, Common, Drawn, Key, LegendItem, MAX_SERIES, TableData, Tip, TipLine, column_name,
    common_methods, fill, frame, legend, n, nodes, num, pct, svg, value_cell,
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
    pub(crate) paint: Paint,
}

/// How a series is painted past its colour.
#[derive(Clone, Debug, Default)]
pub(crate) struct Paint {
    /// An area's or a bar's fill.
    pub fill: Option<Gradient>,
    /// A fill of the series' own colour fading down: to nothing for an area, to half
    /// for a bar.
    pub fade: bool,
    /// A line's stroke.
    pub stroke: Option<Gradient>,
    /// A glow around the line or the bars.
    pub glow: bool,
    /// A colour for each value: bars, and the values in the tooltip and the table.
    pub scale: Option<ColorScale>,
    /// What the values count, written after them in the tooltip and the table's head.
    pub unit: Option<String>,
}

impl Series {
    /// A series called `name`. A value that is `None`, NaN or infinite is a gap.
    pub fn new<V: IntoValue>(name: impl Into<String>, values: impl IntoIterator<Item = V>) -> Self {
        Self {
            name: name.into(),
            values: clean(values),
            color: None,
            paint: Paint::default(),
        }
    }

    /// Colours the series with any [`Ink`] of the theme: a slot, a tone, the accent.
    pub fn ink(mut self, ink: impl Into<Ink>) -> Self {
        self.color = Some(ink.into().color());
        self
    }

    /// Fills an area or the bars with `gradient`. [`Ink::own`] in its stops is the
    /// series' colour.
    pub fn fill(mut self, gradient: Gradient) -> Self {
        self.paint.fill = Some(gradient);
        self
    }

    /// Fills an area with a wash of its own colour that fades to nothing at the bottom
    /// ([`Gradient::fade`]); bars with their colour fading to half
    /// ([`Gradient::column`]).
    pub fn fade(mut self) -> Self {
        self.paint.fade = true;
        self.paint.fill = None;
        self
    }

    /// Strokes the line with `gradient`, which spans the plot: a horizontal one runs
    /// from the left edge to the right whatever part of the plot the line covers.
    pub fn stroke(mut self, gradient: Gradient) -> Self {
        self.paint.stroke = Some(gradient.across_plot());
        self
    }

    /// Puts a soft glow of its own colour around the line or the bars.
    pub fn glow(mut self) -> Self {
        self.paint.glow = true;
        self
    }

    /// Colours each value by `scale`: each bar in the colour of its band, and the value
    /// in the tooltip's key. The bands' labels make the legend.
    pub fn colors(mut self, scale: ColorScale) -> Self {
        self.paint.scale = Some(scale);
        self
    }

    /// What the values count (`BPM`, `kg`, `h`): written after the value in the tooltip,
    /// and after the series' name in the table's head.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.paint.unit = Some(unit.into());
        self
    }

    /// Colours the series by what it means: [`Tone::Success`] for a good outcome,
    /// [`Tone::Danger`] for failures. The legend adds the tone's icon.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.color = Some(Color::Tone(tone));
        self
    }

    /// Draws the series in the quiet ink the chart uses for what is not the subject: the
    /// period before, beside the one on show ([`Report::previous_series`](super::Report::previous_series)).
    pub fn muted(mut self) -> Self {
        self.color = Some(Color::Other);
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
    curve: Curve,
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
    dots: Dots,
    active_dot: ActiveDot,
    week_start: Weekday,
    rtl: bool,
    drill: Vec<String>,
    x_axis: Axis,
    y_axis: Axis,
    grid: Gridlines,
    x_ticks: Ticks,
    domain: Domain,
    references: Vec<Reference>,
    zones: Vec<Zone>,
    y_categories: Option<Vec<String>>,
    lanes: bool,
    baseline: Option<f64>,
    tooltip: TooltipStyle,
    details: Vec<(String, Vec<String>)>,
    motion: Motion,
}

common_methods!(Chart);

impl Chart {
    fn new(kind: Kind, title: impl Into<String>) -> Self {
        Self {
            common: Common::new(title.into()),
            kind,
            horizontal: false,
            stack: Stack::None,
            curve: Curve::Linear,
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
            dots: Dots::End,
            active_dot: ActiveDot::Plain,
            week_start: Weekday::Mon,
            rtl: false,
            drill: Vec::new(),
            x_axis: Axis::new(),
            y_axis: Axis::new(),
            grid: Gridlines::horizontal(),
            x_ticks: Ticks::auto(),
            domain: Domain::auto(),
            references: Vec::new(),
            zones: Vec::new(),
            y_categories: None,
            lanes: false,
            baseline: None,
            tooltip: TooltipStyle::new(),
            details: Vec::new(),
            motion: Motion::none(),
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

    /// Draws lines as a monotone curve, which never overshoots a value: the same as
    /// `.curve(Curve::Monotone)`.
    pub fn smooth(mut self) -> Self {
        self.curve = Curve::Monotone;
        self
    }

    /// How lines and areas run between points: straight, a monotone curve, or steps.
    pub fn curve(mut self, curve: Curve) -> Self {
        self.curve = curve;
        self
    }

    /// Which points of a line show a dot: the last (the default), all, or none.
    pub fn dots(mut self, dots: Dots) -> Self {
        self.dots = dots;
        self
    }

    /// The dot that marks the value under the pointer or the keys.
    pub fn active_dot(mut self, dot: ActiveDot) -> Self {
        self.active_dot = dot;
        self
    }

    /// What the x axis draws: its labels (the default), its line, its tick marks.
    /// [`Axis::hidden`] gives the plot the room the labels took.
    pub fn x_axis(mut self, axis: Axis) -> Self {
        self.x_axis = axis;
        self
    }

    /// What the value axis draws: its labels (the default), its line, its tick marks.
    /// [`Axis::hidden`] gives the plot the room the labels took.
    pub fn y_axis(mut self, axis: Axis) -> Self {
        self.y_axis = axis;
        self
    }

    /// The gridlines behind the marks: across at each value tick by default.
    pub fn grid(mut self, grid: Gridlines) -> Self {
        self.grid = grid;
        self
    }

    /// Which x labels are written: every n-th, at most n, the first and the last.
    pub fn x_ticks(mut self, ticks: Ticks) -> Self {
        self.x_ticks = ticks;
        self
    }

    /// The range of the value axis: from the data, padded, or fixed.
    pub fn y_domain(mut self, domain: Domain) -> Self {
        self.domain = domain;
        self
    }

    /// Names the values of the value axis: value 0 is `names[0]`, 1 is `names[1]`.
    /// Each name is a tick, the axis runs from the first to the last, and the table and
    /// the tooltip write the name, not the number: for states such as sleep stages, with
    /// [`Curve::Step`].
    pub fn y_categories<S: Into<String>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        let names: Vec<String> = names.into_iter().map(Into::into).collect();
        self.y_categories = (!names.is_empty()).then_some(names);
        self
    }

    /// Gives each of the [named values](Self::y_categories) a lane of its own: the axis
    /// runs half a step past the first and the last, so each name sits in the middle of
    /// its band, and an area fills down to the foot of the lowest lane. A stepped area
    /// over sleep stages then shows the lowest stage (deep sleep) as an area like the
    /// others, not as a line along the foot of the plot.
    ///
    /// ```
    /// use navree_ui::charts::{Chart, Curve, Series};
    ///
    /// let night = Chart::area("Sleep stages")
    ///     .numbers([0, 30, 60, 90, 120])
    ///     .series(Series::new("Stage", [3, 1, 0, 2, 0]))
    ///     .curve(Curve::Step)
    ///     .y_categories(["Deep", "Core", "REM", "Awake"])
    ///     .lanes();
    /// # let _ = navree_ui::render(&night.into());
    /// ```
    pub fn lanes(mut self) -> Self {
        self.lanes = true;
        self
    }

    /// Bars grow from `value`, up for a value above it and down for one below, rather
    /// than from zero: years from an age, degrees from a normal temperature, a score
    /// against a reference. A line is drawn at the level, and the value axis holds it
    /// and the values without reaching down to zero, so a [domain](Self::y_domain) such
    /// as `Domain::fixed(18, 60)` is kept. Bars side by side only: a stack still grows
    /// from zero. The table and the tooltip say the values as they are.
    ///
    /// ```
    /// use navree_ui::charts::{Chart, Series};
    ///
    /// let ages = Chart::bar("Body age by part")
    ///     .categories(["Heart", "Lungs", "Muscle", "Metabolism"])
    ///     .series(Series::new("Age", [31, 38, 34, 41]).unit("years"))
    ///     .baseline(36);
    /// # let _ = navree_ui::render(&ages.into());
    /// ```
    pub fn baseline(mut self, value: impl IntoValue) -> Self {
        self.baseline = value.into_value().filter(|v| v.is_finite());
        self
    }

    /// Adds a reference line: a level, an average, a zero baseline, the diagonal.
    pub fn reference(mut self, reference: Reference) -> Self {
        self.references.push(reference);
        self
    }

    /// Adds a band of the value axis, shaded behind the data: a target zone.
    pub fn zone(mut self, zone: Zone) -> Self {
        self.zones.push(zone);
        self
    }

    /// What the tooltip shows.
    pub fn tooltip(mut self, tooltip: TooltipStyle) -> Self {
        self.tooltip = tooltip;
        self
    }

    /// Adds a column of words, one for each x: in the table after the series, and as a
    /// line of the tooltip (`Duration`, `23 min`).
    pub fn detail<S: Into<String>>(
        mut self,
        name: impl Into<String>,
        texts: impl IntoIterator<Item = S>,
    ) -> Self {
        self.details
            .push((name.into(), texts.into_iter().map(Into::into).collect()));
        self
    }

    /// How the marks come in: lines draw from the start, bars grow from the baseline,
    /// dots pop in. Off by default; always off for a reader who asks for less motion.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
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

    /// Makes each category or period a link, `links[i]` for the i-th: where its rows are
    /// listed, such as a [`DataTable`](crate::DataTable) filtered to that day. Each bar of
    /// a vertical bar chart is a link named by its series and its period, and the name of
    /// the period in the chart's table is one too, which a keyboard and a screen reader
    /// reach. When bars are merged or a line is thinned, a row stands for several values
    /// and has no link. A link must stay on this site; a `Report::drill_links` builds the
    /// links for a report's periods.
    pub fn drill<S: Into<String>>(mut self, links: impl IntoIterator<Item = S>) -> Self {
        self.drill = links.into_iter().map(Into::into).collect();
        self
    }

    /// Makes the legend a set of switches that hide and show each series, with or
    /// without JavaScript. Not for stacked charts, whose stacks would lose a layer.
    pub fn toggles(mut self) -> Self {
        self.toggles = true;
        self
    }

    /// Leaves out the dot at the end of each line: the same as `.dots(Dots::None)`.
    pub fn no_dots(mut self) -> Self {
        self.dots = Dots::None;
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
            self.height + self.x_band() as u32 + TOP as u32
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
    paint: Paint,
}

impl Ready {
    /// The fill of an area or of bars: the one given, or the fade asked for.
    fn fill(&self, bars: bool) -> Option<Gradient> {
        self.paint.fill.clone().or_else(|| {
            self.paint.fade.then(|| {
                if bars {
                    Gradient::column(Ink::own())
                } else {
                    Gradient::fade(Ink::own())
                }
            })
        })
    }

    /// The colour of value `v`: its band's, or the series'.
    fn color_of(&self, v: f64) -> Color {
        match &self.paint.scale {
            Some(scale) => scale.ink_of(v).resolve(self.color).color(),
            None => self.color,
        }
    }
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
    /// The link of each drawn row, when each row stands for one value of the data.
    fn row_links(&self, rows: &[Row], total: usize) -> Vec<Option<String>> {
        if self.drill.is_empty() || rows.len() != total {
            return Vec::new();
        }
        rows.iter()
            .map(|row| self.drill.get(row.start).cloned())
            .collect()
    }

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
                paint: Paint::default(),
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
                paint: s.paint,
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
        // A time of day is written as the chart's language writes one.
        self.format = std::mem::take(&mut self.format).with_dates(&self.common.dates);
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
        let categories = self.y_categories.clone();
        let fmt = |v: f64| match &categories {
            Some(names) => category_name(names, v),
            None => self.format.format(v, &numbers),
        };

        // The averages a reference line may stand at, over every value.
        let averages: Vec<Option<f64>> = series
            .iter()
            .map(|s| {
                let (sum, count) = s
                    .values
                    .iter()
                    .flatten()
                    .fold((0.0, 0_usize), |(sum, count), v| (sum + v, count + 1));
                (count > 0)
                    .then(|| sum / count as f64)
                    .filter(|v| v.is_finite())
            })
            .collect();

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
        let mut marks: Vec<String> = self
            .annotations
            .iter()
            .map(|a| match a {
                Annotation::Target { value, label } => format!("{label} ({})", fmt(*value)),
                Annotation::Event { label, .. } | Annotation::Band { label, .. } => label.clone(),
            })
            .collect();
        for r in &self.references {
            if let (Some(label), Some(v)) = (&r.label, reference_value(r, &averages)) {
                let value = fmt(v);
                let text = fill(label, &[("value", &value)]);
                marks.push(if label.contains("{value}") {
                    text
                } else {
                    format!("{text} ({value})")
                });
            }
        }
        for z in &self.zones {
            if let Some(label) = &z.label {
                marks.push(format!("{label} ({} – {})", fmt(z.from), fmt(z.to)));
            }
        }
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
            // Bars from their baseline; an area over lanes from the foot of the lowest.
            let floor = match (&self.y_categories, bars) {
                (_, true) => self.baseline.unwrap_or(0.0),
                (Some(_), false) if self.lanes => -0.5,
                _ => 0.0,
            };
            shown
                .iter()
                .map(|values| values.iter().map(|v| v.map(|v| (floor, v))).collect())
                .collect()
        };
        // Where bars start, and the level the zero line marks.
        let origin = if bars && !stacked {
            self.baseline.unwrap_or(0.0)
        } else {
            0.0
        };

        // The value scale.
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for column in &edges {
            for (base, top) in column.iter().flatten() {
                // An area's baseline counts unless a domain was asked for: then the area
                // runs down to the foot of the plot.
                if stacked || bars || (self.kind == Kind::Area && self.domain == Domain::auto()) {
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
        if !percent {
            // Levels and zones are in the data's units: the scale holds them.
            for r in &self.references {
                if let RefAt::Level(v) = r.at {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
            for z in &self.zones {
                lo = lo.min(z.from);
                hi = hi.max(z.to);
            }
        }
        // The range asked for: the categories' own, a domain, or the chart's rules.
        let ranged = match &categories {
            Some(names) if self.lanes => {
                Some((lo.min(-0.5), hi.max((names.len().max(1) - 1) as f64 + 0.5)))
            }
            Some(names) => Some((lo.min(0.0), hi.max((names.len().max(2) - 1) as f64))),
            None if percent => None,
            None => self.domain.range(lo, hi),
        };
        // Bars reach zero unless they grow from a baseline of their own; a line of times
        // of day starts where its times do.
        let from_baseline = bars && !stacked && self.baseline.is_some();
        let zero = (bars && !from_baseline)
            || stacked
            || (ranged.is_none()
                && (self.kind == Kind::Area || self.y_zero.unwrap_or(!self.format.is_clock())));
        if let Some((a, b)) = ranged {
            (lo, hi) = (a, b);
        }
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
        let ticks = match &categories {
            // A tick for each name, from the first to the last.
            Some(names) => super::scale::Ticks {
                scale: Linear { lo, hi },
                values: (0..names.len()).map(|i| i as f64).collect(),
                places: 0,
            },
            None if self.format.is_clock() && !percent => {
                clock_ticks(lo, hi, count, self.domain.is_exact())
            }
            None if self.domain.is_exact() && !percent => {
                let found = nice(lo, hi, count, format.whole());
                let slack = (hi - lo).abs() * 1e-9;
                super::scale::Ticks {
                    scale: Linear { lo, hi },
                    values: found
                        .values
                        .into_iter()
                        .filter(|v| *v >= lo - slack && *v <= hi + slack)
                        .collect(),
                    places: found.places,
                }
            }
            None => nice(lo, hi, count, format.whole()),
        };
        let compact = categories.is_none()
            && format.axis_compact(ticks.values.iter().fold(0.0_f64, |m, v| m.max(v.abs())));
        let tick_text = |v: f64| match &categories {
            Some(names) => category_name(names, v),
            None if compact => format.format_compact(v, &numbers),
            None => format.format_places(v, &numbers, ticks.places),
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
        head.extend(
            series
                .iter()
                .map(|s| (column_name(&s.name, s.paint.unit.as_deref()), Some(s.color))),
        );
        if stacked {
            head.push((labels.total.to_string(), None));
        }
        head.extend(self.details.iter().map(|(name, _)| (name.clone(), None)));
        let table_rows = rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let mut cells: Vec<Node> = series
                    .iter()
                    .map(|s| {
                        let cell = value_cell(s.values[i].map(fmt), &labels);
                        // A value coloured by its band says so to the tooltip's key.
                        match (s.values[i], &s.paint.scale) {
                            (Some(v), Some(_)) => s.color_of(v).on(el("span")).child(cell).into(),
                            _ => cell,
                        }
                    })
                    .collect();
                if stacked {
                    let sum: Option<f64> = series
                        .iter()
                        .filter_map(|s| s.values[i])
                        .fold(None, |acc, v| Some(acc.unwrap_or(0.0) + v));
                    cells.push(value_cell(sum.map(fmt), &labels));
                }
                for (_, texts) in &self.details {
                    let range = &rows_kept[i];
                    let text = (range.len() == 1)
                        .then(|| texts.get(range.start).cloned())
                        .flatten()
                        .filter(|t| !t.is_empty());
                    cells.push(value_cell(text, &labels));
                }
                (row.name.clone(), cells)
            })
            .collect();
        let table = TableData {
            head,
            rows: table_rows,
            links: self.row_links(&rows, n),
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

        // A series coloured by value shows its bands in the legend instead of itself.
        let banded = series.iter().any(|s| {
            s.paint
                .scale
                .as_ref()
                .is_some_and(|scale| !scale.labelled().is_empty())
        });
        let legend_node = (series.len() > 1 || banded).then(|| {
            let key = if self.kind == Kind::Line {
                Key::Line
            } else {
                Key::Block
            };
            let items: Vec<LegendItem> = series
                .iter()
                .flat_map(|s| {
                    let bands = s
                        .paint
                        .scale
                        .as_ref()
                        .map(ColorScale::labelled)
                        .unwrap_or_default();
                    if bands.is_empty() {
                        vec![LegendItem {
                            name: s.name.clone(),
                            color: s.color,
                            key,
                        }]
                    } else {
                        bands
                            .into_iter()
                            .map(|(ink, name)| LegendItem {
                                name,
                                color: ink.resolve(s.color).color(),
                                key: Key::Block,
                            })
                            .collect()
                    }
                })
                .collect();
            legend(
                &self.common.id(),
                &items,
                self.toggles && !stacked && !banded,
            )
        });

        // The tooltip: a line per series with its unit, then the total and the details.
        let mut tip_lines: Vec<TipLine> = series
            .iter()
            .map(|s| TipLine {
                name: column_name(&s.name, s.paint.unit.as_deref()),
                unit: s.paint.unit.clone(),
                color: Some(s.color),
                key: true,
            })
            .collect();
        if stacked {
            tip_lines.push(TipLine {
                name: labels.total.to_string(),
                unit: None,
                color: None,
                key: true,
            });
        }
        tip_lines.extend(self.details.iter().map(|(name, _)| TipLine {
            name: name.clone(),
            unit: None,
            color: None,
            key: false,
        }));

        let plan = Plan {
            rows: &rows,
            series: &series,
            edges: &edges,
            scale: ticks.scale,
            tick_values: &ticks.values,
            tick_text: &tick_text,
            height: f64::from(self.height),
            stacked,
            origin,
            averages: &averages,
            fmt: &fmt,
        };
        let (body, body_attrs, height) = if self.horizontal && bars {
            self.horizontal_body(&plan)
        } else {
            self.vertical_body(&plan, grain, n)
        };
        #[cfg(feature = "native-preview")]
        crate::native::record_chart_numbers(
            &self.common.id(),
            rows.iter().map(|row| row.name.as_str()),
            series
                .iter()
                .map(|s| (s.name.as_str(), s.values.as_slice())),
            Some(self.native_extras(
                &series,
                &averages,
                bars && self.horizontal,
                stacked,
                &self.common.summary.clone().unwrap_or_else(|| summary.join(" ")),
            )),
        );
        Some(Drawn {
            legend: legend_node,
            body,
            body_attrs,
            summary: summary.join(" "),
            table,
            height,
            note,
            interactive: true,
            text: false,
            tip: Some(Tip {
                big: self.tooltip.big,
                lines: tip_lines,
            }),
        })
    }
}

#[cfg(feature = "native-preview")]
impl Chart {
    /// What the chart says that the protocol's `chart` carries beside its numbers, in the
    /// protocol's own words, for a native client: the units and bounds of its axes, its
    /// reference lines and zones, the names of a value axis made of levels, how its line
    /// runs, how its series sit, the colour of each series and its summary. Written only
    /// while a native client asks.
    fn native_extras(
        &self,
        series: &[Ready],
        averages: &[Option<f64>],
        horizontal: bool,
        stacked: bool,
        summary: &str,
    ) -> serde_json::Value {
        use serde_json::json;
        let number = |value: f64| -> serde_json::Value {
            if value.fract() == 0.0 && value.abs() < 9.0e15 {
                json!(value as i64)
            } else {
                json!(value)
            }
        };
        let units: Vec<Option<&str>> = series.iter().map(|s| s.paint.unit.as_deref()).collect();
        // One unit for every series is the value axis's.
        let shared = units
            .first()
            .copied()
            .flatten()
            .filter(|first| units.iter().all(|unit| unit == &Some(*first)));
        let (low, high) = match self.domain.kind {
            super::axes::DomainKind::Fixed(a, b) => (Some(a), Some(b)),
            _ => (None, None),
        };
        let low = self.y_min.or(low);
        let high = self.y_max.or(high);
        let mut y = serde_json::Map::new();
        if let Some(unit) = shared {
            y.insert("unit".to_owned(), json!(unit));
        }
        if let Some(low) = low {
            y.insert("min".to_owned(), number(low));
        }
        if let Some(high) = high {
            y.insert("max".to_owned(), number(high));
        }
        let mut references: Vec<serde_json::Value> = Vec::new();
        for reference in &self.references {
            let label = reference.label.as_deref();
            match reference.at {
                RefAt::Identity => references.push(json!({"kind": "diagonal", "label": label})),
                _ => {
                    if let Some(value) = reference_value(reference, averages) {
                        references.push(json!({"kind": "level", "value": number(value), "label": label}));
                    }
                }
            }
        }
        // A target across the chart is a level; a moment and a stretch of the x axis are not.
        for annotation in &self.annotations {
            if let Annotation::Target { value, label } = annotation {
                references.push(json!({"kind": "level", "value": number(*value), "label": label}));
            }
        }
        let zones: Vec<serde_json::Value> = self
            .zones
            .iter()
            .map(|zone| json!({"from": number(zone.from), "to": number(zone.to), "label": zone.label}))
            .collect();
        let hues: Vec<&str> = series
            .iter()
            .map(|s| match s.color {
                Color::Slot(1) => "blue",
                Color::Slot(2) => "orange",
                Color::Slot(3) | Color::Slot(6) => "green",
                Color::Slot(4) => "amber",
                Color::Slot(5) => "rose",
                Color::Slot(7) => "violet",
                Color::Slot(_) => "red",
                Color::Accent => "accent",
                Color::Other => "neutral",
                Color::Tone(tone) => tone.value().unwrap_or("neutral"),
            })
            .collect();
        json!({
            "x": self.x_label.as_ref().map(|name| json!({"name": name})),
            "y": (!y.is_empty()).then(|| serde_json::Value::Object(y)),
            "references": references,
            "zones": zones,
            "levels": self.y_categories.clone().unwrap_or_default(),
            "curve": match self.curve {
                Curve::Linear => "linear",
                Curve::Monotone => "smooth",
                Curve::Step => "step",
            },
            "stacking": match (stacked, self.stack) {
                (true, Stack::Percent) => "shares",
                (true, _) => "values",
                _ => "apart",
            },
            "horizontal": horizontal,
            "summary": summary,
            "units": units,
            "hues": hues,
        })
    }
}

/// Ticks for times of day in minutes: on whole hours (or quarter and half hours over a
/// short span), about `count` of them. The scale widens to the first and last tick
/// unless it is `exact`, when the ticks outside it are dropped.
fn clock_ticks(lo: f64, hi: f64, count: usize, exact: bool) -> super::scale::Ticks {
    const STEPS: [f64; 9] = [15.0, 30.0, 60.0, 120.0, 180.0, 240.0, 360.0, 720.0, 1440.0];
    let (lo, hi) = if lo.is_finite() && hi.is_finite() && hi > lo {
        (lo, hi)
    } else if lo.is_finite() {
        (lo - 60.0, lo + 60.0)
    } else {
        (0.0, 60.0)
    };
    let span = hi - lo;
    let step = STEPS
        .into_iter()
        .find(|step| span / step <= count.max(2) as f64)
        .unwrap_or(1440.0);
    let (first, last) = ((lo / step).floor() * step, (hi / step).ceil() * step);
    let scale = if exact {
        Linear { lo, hi }
    } else {
        Linear {
            lo: first,
            hi: last,
        }
    };
    let mut values = Vec::new();
    let mut at = first;
    while at <= last + 1e-9 && values.len() < 200 {
        if at >= scale.lo - 1e-9 && at <= scale.hi + 1e-9 {
            values.push(at);
        }
        at += step;
    }
    super::scale::Ticks {
        scale,
        values,
        places: 0,
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
    /// Where bars start: zero, or the chart's baseline.
    origin: f64,
    /// Each series' average over all its values.
    averages: &'a [Option<f64>],
    /// How a value is written.
    fmt: &'a dyn Fn(f64) -> String,
}

/// Where a reference line stands, when the chart has that value.
fn reference_value(r: &Reference, averages: &[Option<f64>]) -> Option<f64> {
    match r.at {
        RefAt::Level(v) => Some(v),
        RefAt::Average(i) => averages.get(i).copied().flatten(),
        RefAt::Zero => Some(0.0),
        RefAt::Identity => None,
    }
}

/// The name of category `v`, the nearest one when `v` falls between.
fn category_name(names: &[String], v: f64) -> String {
    if names.is_empty() || !v.is_finite() {
        return String::new();
    }
    let at = v.round().clamp(0.0, (names.len() - 1) as f64) as usize;
    names[at].clone()
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

impl Chart {
    /// The band under the plot: the x labels' room, a tick mark's, or a dot's.
    fn x_band(&self) -> f64 {
        if self.x_axis.labels {
            X_BAND
        } else if self.x_axis.ticks {
            8.0
        } else {
            4.0
        }
    }

    /// Bars up from a baseline, or lines and areas, with the value axis at the side.
    fn vertical_body(
        &self,
        plan: &Plan<'_>,
        grain: Grain,
        rows_n: usize,
    ) -> (Node, Vec<(&'static str, AttrValue)>, u32) {
        let base = TOP + plan.height;
        let total = base + self.x_band();
        let rows = plan.rows;
        let m = rows.len();
        let bars = self.kind == Kind::Bar;
        let motion = self.motion;
        let mut defs = Defs::new(&self.common.id());
        // The stretched layer's box: x 0 to 1000, y in pixels.
        let layer_box = [0.0, TOP, 1000.0, plan.height];

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
        let y_axis = self.y_axis.shows().then(|| {
            let axis_w = if self.y_axis.labels {
                (widest as f64 * 6.6 + 10.0).clamp(24.0, 120.0).round()
                    + if self.y_axis.ticks { 4.0 } else { 0.0 }
            } else {
                6.0
            };
            let edge = if self.rtl { 0.5 } else { axis_w - 0.5 };
            let texts: Vec<Node> = if self.y_axis.labels {
                ticks
                    .iter()
                    .map(|(v, text)| {
                        let gap = if self.y_axis.ticks { 10.0 } else { 6.0 };
                        el("text")
                            .attr(
                                "x",
                                if self.rtl {
                                    n(gap, 0)
                                } else {
                                    n(axis_w - gap, 0)
                                },
                            )
                            .attr("y", n(plan.y(*v), 1))
                            .attr("dy", "0.32em")
                            .attr("text-anchor", if self.rtl { "start" } else { "end" })
                            .child(text.clone())
                            .into()
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let marks: Vec<Node> = if self.y_axis.ticks {
                ticks
                    .iter()
                    .map(|(v, _)| {
                        let y = crisp(plan.y(*v));
                        el("line")
                            .slot("chart-tick")
                            .attr("x1", n(edge, 1))
                            .attr("x2", n(if self.rtl { 4.5 } else { axis_w - 4.5 }, 1))
                            .attr("y1", n(y, 1))
                            .attr("y2", n(y, 1))
                            .into()
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let line = self.y_axis.line.then(|| {
                el("line")
                    .slot("chart-axis-line")
                    .attr("x1", n(edge, 1))
                    .attr("x2", n(edge, 1))
                    .attr("y1", n(TOP, 0))
                    .attr("y2", n(base, 0))
            });
            svg()
                .slot("chart-y-axis")
                .attr("width", n(axis_w, 0))
                .attr("height", n(total, 0))
                .children((texts, marks, line))
        });

        let dashed = self.grid.dashed.then_some("");
        let mut plot: Vec<Node> = Vec::new();
        // Gridlines across, and the zero line where values cross it.
        let origin = plan.origin;
        let crosses =
            (plan.scale.lo < origin && plan.scale.hi > origin) || (self.baseline.is_some() && bars);
        if self.grid.across {
            for (v, _) in &ticks {
                let y = crisp(plan.y(*v));
                let zero = *v == origin;
                plot.push(
                    el("line")
                        .slot(if zero { "chart-zero" } else { "chart-grid" })
                        .attr_opt("data-dashed", (!zero).then_some(dashed).flatten())
                        .attr("x1", "0")
                        .attr("x2", "100%")
                        .attr("y1", n(y, 1))
                        .attr("y2", n(y, 1))
                        .into(),
                );
            }
        }
        if crosses && (!self.grid.across || !plan.tick_values.contains(&origin)) {
            let y = crisp(plan.y(origin));
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

        // The x labels, and the gridlines down at them.
        let x_labels = self.x_label_list(plan, grain, rows_n);
        if self.grid.down {
            for (x, _, _) in &x_labels {
                plot.push(
                    el("line")
                        .slot("chart-grid")
                        .attr_opt("data-dashed", dashed)
                        .attr("x1", pct(*x))
                        .attr("x2", pct(*x))
                        .attr("y1", n(TOP, 0))
                        .attr("y2", n(base, 0))
                        .into(),
                );
            }
        }

        // Zones behind the data.
        for (at, zone) in self.zones.iter().enumerate() {
            let (top, bottom) = (plan.y(zone.to), plan.y(zone.from));
            let (top, bottom) = (top.clamp(TOP, base), bottom.clamp(TOP, base));
            plot.push(
                motion
                    .apply(
                        zone_rect(top, bottom, zone.label.as_deref(), zone.ink, self.rtl),
                        Effect::Fade,
                        at,
                    )
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

        // The x axis: its labels, its line and its tick marks.
        plot.push(self.x_axis_node(&x_labels, base, total).into());

        // The marks.
        let mut dots: Vec<Node> = Vec::new();
        if bars {
            let k = if plan.stacked { 1 } else { plan.series.len() };
            let band = 100.0 / m as f64;
            let links = self.row_links(rows, rows_n);
            let mut marks: Vec<Node> = Vec::with_capacity(m * plan.series.len());
            for (s, ready) in plan.series.iter().enumerate() {
                // One gradient for each colour the series' bars take.
                let gradient = ready.fill(true);
                let mut painted: Vec<(Color, String)> = Vec::new();
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
                    let color = ready.values[i].map_or(ready.color, |v| ready.color_of(v));
                    let bar = color.on(el("rect")
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
                        ));
                    let bar = match &gradient {
                        Some(g) => {
                            let url = match painted.iter().find(|(c, _)| *c == color) {
                                Some((_, url)) => url.clone(),
                                None => {
                                    let url = defs.gradient(g, color, Some(layer_box));
                                    painted.push((color, url.clone()));
                                    url
                                }
                            };
                            bar.attr("fill", url)
                        }
                        None => bar,
                    };
                    let bar = if ready.paint.glow { glow(bar) } else { bar };
                    let bar =
                        motion.apply(bar, if up { Effect::GrowUp } else { Effect::GrowDown }, i);
                    marks.push(match links.get(i).and_then(Option::as_ref) {
                        Some(href) => el("a")
                            .slot("chart-drill")
                            .attr("href", href.clone())
                            .attr("aria-label", format!("{}: {}", ready.name, rows[i].name))
                            .attr("tabindex", -1_i64)
                            .child(bar)
                            .into(),
                        None => bar.into(),
                    });
                }
            }
            plot.push(el("g").slot("chart-bars").child(marks).into());
        } else {
            let area = self.kind == Kind::Area;
            let wipe = if self.rtl {
                Effect::WipeBack
            } else {
                Effect::Wipe
            };
            let mut layers: Vec<Node> = Vec::new();
            let mut lines: Vec<Node> = Vec::new();
            let mut glowing: Vec<Node> = Vec::new();
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
                    curve_path(&mut line_d, &upper, self.curve, false, 10.0);
                    if area {
                        // The lower edge stays in the plot when the scale doesn't hold zero.
                        let lower: Vec<(f64, f64)> = (start..i)
                            .rev()
                            .filter_map(|j| {
                                edges[j]
                                    .map(|(base_v, _)| (rows[j].x, plan.y(base_v).clamp(TOP, base)))
                            })
                            .collect();
                        curve_path(&mut area_d, &upper, self.curve, false, 10.0);
                        curve_path(&mut area_d, &lower, self.curve, true, 10.0);
                        area_d.push('Z');
                    }
                }
                if area && !area_d.is_empty() {
                    let path = ready.color.on(el("path")
                        .slot("chart-area")
                        .attr("data-s", s as i64)
                        .attr("d", area_d));
                    let path = match ready.fill(false) {
                        Some(g) => {
                            path.attr("fill", defs.gradient(&g, ready.color, Some(layer_box)))
                        }
                        None => path,
                    };
                    layers.push(path.into());
                }
                if !line_d.is_empty() {
                    let path = ready.color.on(el("path")
                        .slot("chart-line")
                        .attr("data-s", s as i64)
                        .attr("d", line_d)
                        .attr("vector-effect", "non-scaling-stroke"));
                    let path = match &ready.paint.stroke {
                        Some(g) => {
                            path.attr("stroke", defs.gradient(g, ready.color, Some(layer_box)))
                        }
                        None => path,
                    };
                    if ready.paint.glow {
                        // A glow on a layer of its own, so the blur is not stretched.
                        glowing.push(
                            motion
                                .apply(glow(ready.color.on(layer(total))).child(path), wipe, 0)
                                .into(),
                        );
                    } else {
                        lines.push(path.into());
                    }
                }
                // The points: every one, or the last, where the series stands now.
                let points: Vec<usize> = match self.dots {
                    Dots::None => Vec::new(),
                    Dots::End => (0..m)
                        .rev()
                        .find(|j| edges[*j].is_some())
                        .into_iter()
                        .collect(),
                    Dots::All => (0..m).filter(|j| edges[*j].is_some()).collect(),
                };
                let slot = if self.dots == Dots::End {
                    "chart-end"
                } else {
                    "chart-point"
                };
                for (at, j) in points.iter().enumerate() {
                    let Some((_, top)) = edges[*j] else { continue };
                    let dot = ready.color.on(el("circle")
                        .slot(slot)
                        .attr("data-s", s as i64)
                        .attr("cx", pct(rows[*j].x))
                        .attr("cy", n(plan.y(top), 1))
                        .attr("r", "4"));
                    let dot = if ready.paint.glow { glow(dot) } else { dot };
                    // Dots come in after the line has drawn to them.
                    let after = rows[*j].x / 100.0;
                    let dot =
                        motion
                            .after((after * 900.0) as u32)
                            .stagger(0)
                            .apply(dot, Effect::Pop, at);
                    dots.push(dot.into());
                }
            }
            if !layers.is_empty() {
                plot.push(motion.apply(layer(total).child(layers), wipe, 0).into());
            }
            if !lines.is_empty() {
                plot.push(motion.apply(layer(total).child(lines), wipe, 0).into());
            }
            plot.extend(glowing);
        }

        // Reference lines over the data.
        for (at, r) in self.references.iter().enumerate() {
            let line = match r.at {
                RefAt::Identity => self.identity_line(plan, rows_n),
                _ => reference_value(r, plan.averages)
                    .filter(|v| *v >= plan.scale.lo && *v <= plan.scale.hi)
                    .map(|v| {
                        let y = crisp(plan.y(v));
                        (("0".to_owned(), y), ("100%".to_owned(), y), Some(v))
                    }),
            };
            let Some((from, to, value)) = line else {
                continue;
            };
            let label = r.label.as_ref().map(|label| match value {
                Some(v) => fill(label, &[("value", &(plan.fmt)(v))]),
                None => label.clone(),
            });
            let mark = reference_line(
                from,
                to,
                label.as_deref(),
                r.place,
                r.ink,
                r.solid,
                self.rtl,
            );
            plot.push(motion.after(400).apply(mark, Effect::Fade, at).into());
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
            let (r, style) = match self.active_dot {
                ActiveDot::Large => ("6", None),
                ActiveDot::Halo => ("5", Some("halo")),
                _ => ("4", None),
            };
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
                            .attr_opt("data-dot", style)
                            .attr("cx", "0")
                            .attr("cy", "0")
                            .attr("r", r)
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
            .children((defs.node(), plot));
        let body = el("div")
            .slot("chart-frame")
            .attr_opt("data-rtl", self.rtl.then_some(""))
            .attr_opt("data-bare", y_axis.is_none().then_some(""))
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

    /// The diagonal where x equals y, over what both axes share: its two ends.
    #[allow(clippy::type_complexity)]
    fn identity_line(
        &self,
        plan: &Plan<'_>,
        rows_n: usize,
    ) -> Option<((String, f64), (String, f64), Option<f64>)> {
        let X::Numbers(values) = &self.x else {
            return None;
        };
        let (x0, x1) = (*values.first()?, *values.get(rows_n.checked_sub(1)?)?);
        let lo = x0.max(plan.scale.lo);
        let hi = x1.min(plan.scale.hi);
        if hi.partial_cmp(&lo) != Some(std::cmp::Ordering::Greater) {
            return None;
        }
        let pos = |v: f64| {
            let x = linear_x(v, x0, x1);
            pct(if self.rtl { 100.0 - x } else { x })
        };
        Some(((pos(lo), plan.y(lo)), (pos(hi), plan.y(hi)), None))
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

    /// The labels under the plot: where each sits (percent), its text, and whether a
    /// narrow chart drops it (every other one; on a very narrow one, all but every
    /// other or every third).
    #[allow(clippy::type_complexity)]
    fn x_label_list(
        &self,
        plan: &Plan<'_>,
        grain: Grain,
        rows_n: usize,
    ) -> Vec<(f64, String, (bool, bool))> {
        let rows = plan.rows;
        let m = rows.len();
        let mut labels: Vec<(f64, String)> = Vec::new();
        let bars = self.kind == Kind::Bar;
        let ticks = self.x_ticks;
        let mirror = |x: f64| if self.rtl { 100.0 - x } else { x };
        match &self.x {
            X::Times(times) if !bars => {
                let (lo, hi) = (times[0], times[rows_n - 1]);
                let found = time_ticks(
                    lo,
                    hi,
                    ticks.most.unwrap_or(7),
                    &self.common.dates,
                    self.week_start,
                );
                let keep = Ticks::every(ticks.every.unwrap_or(1)).pick(found.len(), 1);
                for at in keep {
                    let tick = &found[at];
                    labels.push((
                        mirror(linear_x(tick.at as f64, lo as f64, hi as f64)),
                        tick.label.clone(),
                    ));
                }
                if ticks.ends {
                    let name = |t: i64| {
                        time_of(t).map_or_else(String::new, |t| {
                            time_label(t, grain, &self.common.dates, false)
                        })
                    };
                    labels.retain(|(x, _)| (8.0..=92.0).contains(x));
                    labels.insert(0, (mirror(0.0), name(lo)));
                    labels.push((mirror(100.0), name(hi)));
                }
            }
            X::Numbers(values) if !bars => {
                let (lo, hi) = (values[0], values[rows_n - 1]);
                let found = nice(lo, hi, ticks.most.unwrap_or(6), false);
                let inside: Vec<f64> = found
                    .values
                    .iter()
                    .copied()
                    .filter(|v| (lo..=hi).contains(v))
                    .collect();
                let keep = Ticks::every(ticks.every.unwrap_or(1)).pick(inside.len(), 1);
                let text = |v: f64, places: u8| {
                    NumberFormat::number().format_places(v, &self.common.numbers, places)
                };
                for at in keep {
                    let v = inside[at];
                    labels.push((mirror(linear_x(v, lo, hi)), text(v, found.places)));
                }
                if ticks.ends {
                    labels.retain(|(x, _)| (8.0..=92.0).contains(x));
                    labels.insert(0, (mirror(0.0), text(lo, found.places)));
                    labels.push((mirror(100.0), text(hi, found.places)));
                }
            }
            _ => {
                let mut last_year = None;
                for at in ticks.pick(m, m.div_ceil(9).max(1)) {
                    let row = &rows[at];
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
        // On a narrow chart every other label goes, when there are enough to crowd, and on a
        // very narrow one (a card of a dashboard) all but every other, or every third. Labels
        // picked by the page stay as picked.
        let auto = ticks.is_auto();
        let many = auto && labels.len() > 6;
        let crowded = auto && labels.len() > 3;
        labels
            .into_iter()
            .enumerate()
            .map(|(j, (x, text))| {
                let minor = many && j % 2 == 1;
                let tight = crowded && if many { j % 3 != 0 } else { j % 2 == 1 };
                (x, text, (minor, tight))
            })
            .collect()
    }

    /// The x axis: its labels, its line along the plot's foot and a tick mark at each label.
    #[allow(clippy::type_complexity)]
    fn x_axis_node(
        &self,
        labels: &[(f64, String, (bool, bool))],
        base: f64,
        total: f64,
    ) -> Element {
        let y = n(total - 6.0, 1);
        let texts: Vec<Node> = if self.x_axis.labels {
            labels
                .iter()
                .map(|(x, text, (minor, tight))| {
                    let anchor = if *x < 4.0 {
                        "start"
                    } else if *x > 96.0 {
                        "end"
                    } else {
                        "middle"
                    };
                    el("text")
                        .attr("x", pct(*x))
                        .attr("y", y.clone())
                        .attr("text-anchor", anchor)
                        .attr_opt("data-minor", minor.then_some(""))
                        .attr_opt("data-tight", tight.then_some(""))
                        .child(text.clone())
                        .into()
                })
                .collect()
        } else {
            Vec::new()
        };
        let foot = crisp(base);
        let marks: Vec<Node> = if self.x_axis.ticks {
            labels
                .iter()
                .map(|(x, _, (minor, tight))| {
                    el("line")
                        .slot("chart-tick")
                        .attr_opt("data-minor", minor.then_some(""))
                        .attr_opt("data-tight", tight.then_some(""))
                        .attr("x1", pct(*x))
                        .attr("x2", pct(*x))
                        .attr("y1", n(foot, 1))
                        .attr("y2", n(foot + 4.0, 1))
                        .into()
                })
                .collect()
        } else {
            Vec::new()
        };
        let line = self.x_axis.line.then(|| {
            el("line")
                .slot("chart-axis-line")
                .attr("x1", "0")
                .attr("x2", "100%")
                .attr("y1", n(foot, 1))
                .attr("y2", n(foot, 1))
        });
        el("g").slot("chart-x-axis").children((line, marks, texts))
    }

    /// Bars along rows: a label, the bar, and its value, with a value axis under them.
    fn horizontal_body(&self, plan: &Plan<'_>) -> (Node, Vec<(&'static str, AttrValue)>, u32) {
        let mut defs = Defs::new(&self.common.id());
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
        // A baseline that is not a tick still has its line.
        let origin_line = (self.baseline.is_some() && !plan.tick_values.contains(&plan.origin))
            .then(|| at(plan.origin));
        let lines = |h: f64| -> Vec<Node> {
            let origin = origin_line.map(|x| {
                Node::from(
                    el("line")
                        .slot("chart-zero")
                        .attr("x1", pct(x))
                        .attr("x2", pct(x))
                        .attr("y1", "0")
                        .attr("y2", n(h, 0)),
                )
            });
            grid.iter()
                .zip(plan.tick_values)
                .map(|(x, v)| {
                    el("line")
                        .slot(if *v == plan.origin {
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
                .chain(origin)
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
                let color = ready.values[i].map_or(ready.color, |v| ready.color_of(v));
                let bar = color.on(el("rect")
                    .slot("chart-bar")
                    .attr("data-i", i as i64)
                    .attr("data-s", s as i64)
                    .attr("x", pct(x))
                    .attr("width", pct(w.max(0.2)))
                    .attr("y", n(y, 1))
                    .attr("height", n(thick, 1))
                    .attr("clip-path", clip));
                let bar = match ready.paint.fill.clone().or_else(|| {
                    ready.paint.fade.then(|| {
                        Gradient::horizontal()
                            .stop_with_opacity(0.0, Ink::own(), 0.5)
                            .stop(1.0, Ink::own())
                    })
                }) {
                    Some(g) => bar.attr("fill", defs.gradient(&g, color, None)),
                    None => bar,
                };
                let bar = if ready.paint.glow { glow(bar) } else { bar };
                let effect = if ends_right {
                    Effect::GrowRight
                } else {
                    Effect::GrowLeft
                };
                marks.push(self.motion.apply(bar, effect, i).into());
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
            // Reference levels stand across every row; their words are under the axis.
            for r in &self.references {
                let Some(v) = reference_value(r, plan.averages) else {
                    continue;
                };
                if v < plan.scale.lo || v > plan.scale.hi {
                    continue;
                }
                let x = pct(at(v));
                let line = reference_line(
                    (x.clone(), 0.0),
                    (x, row_h),
                    None,
                    r.place,
                    r.ink,
                    r.solid,
                    self.rtl,
                );
                marks.push(line.into());
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
        let mut target_labels: Vec<Node> = self
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
        for r in &self.references {
            let (Some(label), Some(v)) = (&r.label, reference_value(r, plan.averages)) else {
                continue;
            };
            if v < plan.scale.lo || v > plan.scale.hi {
                continue;
            }
            let text = fill(label, &[("value", &(plan.fmt)(v))]);
            let mark = mark_text(&text)
                .slot("chart-target-label")
                .attr("x", pct(at(v)))
                .attr("dx", "4")
                .attr("y", "30");
            target_labels.push(match r.ink {
                Some(ink) => ink.on(mark).into(),
                None => mark.into(),
            });
        }
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
                        .child(nodes((defs.node(), ticks, target_labels))),
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

/// A layer stretched over the plot: x from 0 to 1000 across its width, y in pixels,
/// strokes that don't scale. Lines and areas are drawn in it.
fn layer(total: f64) -> Element {
    svg()
        .slot("chart-marks")
        .attr("x", "0")
        .attr("y", "0")
        .attr("width", "100%")
        .attr("height", n(total, 0))
        .attr("viewBox", format!("0 0 1000 {}", n(total, 0)))
        .attr("preserveAspectRatio", "none")
        .attr("overflow", "visible")
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
}
