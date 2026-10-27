//! [`Chart`]: bar and line charts for the `chart` macro, and
//! [`Sparkline`]s for stat cards.
//!
//! The scale (round ticks) and the geometry are computed here, so a
//! template only draws SVG elements with plain attributes: no inline styles,
//! no client-side charting library. The plot is a `0 0 100 100` view box
//! stretched to its container; strokes keep their width
//! (`vector-effect: non-scaling-stroke`) and labels are HTML around it.

use serde::Serialize;

/// The color of a series.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SeriesTone {
    /// Grey (the default for data that is not the point of the chart:
    /// comparisons, other periods, the rest of a breakdown).
    Muted,
    /// Light neutral: the series the chart is about, when it is not
    /// positive data (traffic, counts, durations).
    Bright,
    /// The accent: positive data (revenue, won deals, sign-ups) and the
    /// highlighted bar.
    Accent,
    /// Amber (warnings, slow requests).
    Amber,
    /// The danger color (errors, failures).
    Danger,
}

/// One data series.
#[derive(Clone, Debug)]
struct Series {
    name: String,
    tone: SeriesTone,
    values: Vec<f64>,
    dashed: bool,
}

/// The chart's height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartSize {
    /// 160px.
    Small,
    /// 240px (the default).
    Medium,
    /// 320px.
    Large,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Bar,
    Line,
}

/// A bar or line chart over labelled x values (days, months, buckets).
///
/// ```
/// use navree_ui::{Chart, SeriesTone};
///
/// let chart = Chart::bars(["Jan", "Feb", "Mar"])
///     .series("Revenue", SeriesTone::Muted, [1200.0, 1800.0, 2400.0])
///     .highlight(2)
///     .prefix("$");
/// let view = serde_json::to_value(&chart).unwrap();
/// assert_eq!(view["ticks"][0], "$3k");
/// assert_eq!(view["bars"].as_array().unwrap().len(), 3);
/// assert_eq!(view["bars"][2]["tone"], "accent");
/// ```
#[derive(Clone, Debug)]
pub struct Chart {
    kind: Kind,
    labels: Vec<String>,
    series: Vec<Series>,
    highlight: Option<usize>,
    size: ChartSize,
    prefix: String,
    suffix: String,
    label_every: usize,
    max: Option<f64>,
}

impl Chart {
    fn new(kind: Kind, labels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            kind,
            labels: labels.into_iter().map(Into::into).collect(),
            series: Vec::new(),
            highlight: None,
            size: ChartSize::Medium,
            prefix: String::new(),
            suffix: String::new(),
            label_every: 0,
            max: None,
        }
    }

    /// A bar chart over `labels` (several series draw grouped bars).
    pub fn bars(labels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::new(Kind::Bar, labels)
    }

    /// A line chart over `labels` (the first series also gets an area).
    pub fn lines(labels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::new(Kind::Line, labels)
    }

    /// Adds a series: one value per label (missing values count as 0).
    pub fn series(
        mut self,
        name: impl Into<String>,
        tone: SeriesTone,
        values: impl IntoIterator<Item = f64>,
    ) -> Self {
        self.series.push(Series {
            name: name.into(),
            tone,
            values: values.into_iter().collect(),
            dashed: false,
        });
        self
    }

    /// Adds a dashed series (a comparison: last period, a target).
    pub fn dashed_series(
        mut self,
        name: impl Into<String>,
        tone: SeriesTone,
        values: impl IntoIterator<Item = f64>,
    ) -> Self {
        self = self.series(name, tone, values);
        if let Some(last) = self.series.last_mut() {
            last.dashed = true;
        }
        self
    }

    /// Draws the bars at `index` in the accent (the current period).
    pub fn highlight(mut self, index: usize) -> Self {
        self.highlight = Some(index);
        self
    }

    /// The height.
    pub fn size(mut self, size: ChartSize) -> Self {
        self.size = size;
        self
    }

    /// Written before tick labels and values (`"$"`).
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Written after tick labels and values (`" ms"`, `"%"`).
    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self
    }

    /// Shows every `n`th x label (the default picks about eight).
    pub fn label_every(mut self, n: usize) -> Self {
        self.label_every = n.max(1);
        self
    }

    /// The top of the scale (rounded up to a tick), instead of the data's
    /// maximum.
    pub fn max(mut self, max: f64) -> Self {
        self.max = Some(max);
        self
    }

    fn value(&self, series: &Series, index: usize) -> f64 {
        series
            .values
            .get(index)
            .copied()
            .filter(|v| v.is_finite())
            .unwrap_or(0.0)
    }

    fn format(&self, value: f64) -> String {
        format!("{}{}{}", self.prefix, format_number(value), self.suffix)
    }

    fn format_tick(&self, value: f64) -> String {
        format!("{}{}{}", self.prefix, compact(value), self.suffix)
    }
}

/// The top of the scale and the tick step: about four round steps.
fn scale(max: f64) -> (f64, f64) {
    if max <= 0.0 || !max.is_finite() {
        return (4.0, 1.0);
    }
    let rough = max / 4.0;
    let magnitude = 10f64.powf(rough.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * magnitude)
        .find(|step| *step >= rough)
        .unwrap_or(10.0 * magnitude);
    let top = (max / step).ceil() * step;
    (top, step)
}

/// `1234` as `1.2k`, `2500000` as `2.5M`; small numbers as they are.
pub(crate) fn compact(value: f64) -> String {
    let abs = value.abs();
    let (scaled, unit) = if abs >= 1e9 {
        (value / 1e9, "B")
    } else if abs >= 1e6 {
        (value / 1e6, "M")
    } else if abs >= 1e3 {
        (value / 1e3, "k")
    } else {
        (value, "")
    };
    let text = if (scaled - scaled.round()).abs() < 1e-9 {
        format!("{}", scaled.round() as i64)
    } else {
        let text = format!("{scaled:.1}");
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    };
    format!("{text}{unit}")
}

/// `1234567` as `1,234,567`; fractions keep up to two decimals.
pub(crate) fn format_number(value: f64) -> String {
    let cents = (value.abs() * 100.0).round() as u64;
    let (whole, fraction) = (cents / 100, cents % 100);
    let digits = whole.to_string();
    let mut out = String::new();
    if value < 0.0 && cents > 0 {
        out.push('-');
    }
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    if fraction > 0 {
        let decimals = format!("{fraction:02}");
        out.push('.');
        out.push_str(decimals.trim_end_matches('0'));
    }
    out
}

/// Rounds SVG coordinates to three decimals (stable, compact markup).
fn r(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

impl Serialize for Chart {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Legend<'a> {
            name: &'a str,
            tone: SeriesTone,
            dashed: bool,
        }
        #[derive(Serialize)]
        struct Bar {
            x: f64,
            y: f64,
            w: f64,
            h: f64,
            tone: SeriesTone,
            title: String,
        }
        #[derive(Serialize)]
        struct Line {
            line: String,
            area: Option<String>,
            tone: SeriesTone,
            dashed: bool,
        }
        #[derive(Serialize)]
        struct Hover {
            x: f64,
            w: f64,
            title: String,
        }
        #[derive(Serialize)]
        struct XLabel<'a> {
            text: &'a str,
            show: bool,
        }
        #[derive(Serialize)]
        struct View<'a> {
            kind: &'static str,
            size: &'static str,
            legend: Vec<Legend<'a>>,
            ticks: Vec<String>,
            grid: Vec<f64>,
            x_labels: Vec<XLabel<'a>>,
            bars: Vec<Bar>,
            lines: Vec<Line>,
            hovers: Vec<Hover>,
            summary: String,
        }

        let count = self.labels.len().max(1);
        let data_max = self
            .series
            .iter()
            .flat_map(|s| (0..count).map(move |i| (s, i)))
            .map(|(s, i)| self.value(s, i))
            .fold(0.0_f64, f64::max);
        let (top, step) = scale(self.max.unwrap_or(0.0).max(data_max));
        let steps = (top / step).round() as usize;
        let ticks = (0..=steps)
            .rev()
            .map(|i| self.format_tick(i as f64 * step))
            .collect();
        let grid = (0..=steps)
            .map(|i| r(100.0 - i as f64 * step / top * 100.0))
            .collect();
        let y = |value: f64| r(100.0 - (value.max(0.0) / top * 100.0).min(100.0));

        // Bars: every nth label under its bar. Lines: labels at both ends and
        // evenly between (the template spreads them with space-between).
        let shown: Vec<bool> = match self.kind {
            Kind::Bar => {
                let every = if self.label_every > 0 {
                    self.label_every
                } else {
                    count.div_ceil(8).max(1)
                };
                (0..count).map(|i| i % every == 0).collect()
            }
            Kind::Line => {
                let slots = (count - 1)
                    .checked_div(self.label_every)
                    .map_or(count.min(7), |steps| steps + 1)
                    .clamp(1, count);
                let mut shown = vec![false; count];
                if slots == 1 {
                    shown[0] = true;
                } else {
                    for j in 0..slots {
                        let index = (j as f64 * (count - 1) as f64 / (slots - 1) as f64).round();
                        shown[index as usize] = true;
                    }
                }
                shown
            }
        };
        let x_labels = self
            .labels
            .iter()
            .zip(shown)
            .map(|(text, show)| XLabel { text, show })
            .collect();

        let slot = 100.0 / count as f64;
        let mut bars = Vec::new();
        let mut lines = Vec::new();
        let mut hovers = Vec::new();
        let describe = |i: usize| {
            let label = self.labels.get(i).map(String::as_str).unwrap_or("");
            let values: Vec<String> = self
                .series
                .iter()
                .map(|s| {
                    if self.series.len() > 1 {
                        format!("{}: {}", s.name, self.format(self.value(s, i)))
                    } else {
                        self.format(self.value(s, i))
                    }
                })
                .collect();
            format!("{label}: {}", values.join(", "))
        };
        match self.kind {
            Kind::Bar => {
                let groups = self.series.len().max(1) as f64;
                let group_w = slot * 0.64;
                let gap = if groups > 1.0 { slot * 0.03 } else { 0.0 };
                let bar_w = (group_w - gap * (groups - 1.0)) / groups;
                for i in 0..count {
                    let start = i as f64 * slot + (slot - group_w) / 2.0;
                    for (s_index, series) in self.series.iter().enumerate() {
                        let value = self.value(series, i);
                        let top_y = y(value);
                        let tone = if self.highlight == Some(i) {
                            SeriesTone::Accent
                        } else {
                            series.tone
                        };
                        bars.push(Bar {
                            x: r(start + s_index as f64 * (bar_w + gap)),
                            y: top_y,
                            w: r(bar_w),
                            h: r(100.0 - top_y),
                            tone,
                            title: if self.series.len() > 1 {
                                format!(
                                    "{} · {}: {}",
                                    self.labels.get(i).map(String::as_str).unwrap_or(""),
                                    series.name,
                                    self.format(value)
                                )
                            } else {
                                describe(i)
                            },
                        });
                    }
                }
            }
            Kind::Line => {
                let x = |i: usize| {
                    if count == 1 {
                        50.0
                    } else {
                        r(i as f64 / (count - 1) as f64 * 100.0)
                    }
                };
                for (s_index, series) in self.series.iter().enumerate() {
                    let points: Vec<(f64, f64)> = (0..count)
                        .map(|i| (x(i), y(self.value(series, i))))
                        .collect();
                    let line = points
                        .iter()
                        .enumerate()
                        .map(|(i, (px, py))| format!("{}{px} {py}", if i == 0 { "M" } else { "L" }))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let area = (s_index == 0 && !series.dashed).then(|| {
                        let first = points.first().map_or(0.0, |p| p.0);
                        let last = points.last().map_or(100.0, |p| p.0);
                        format!("{line} L{last} 100 L{first} 100 Z")
                    });
                    lines.push(Line {
                        line,
                        area,
                        tone: series.tone,
                        dashed: series.dashed,
                    });
                }
                let half = if count > 1 {
                    50.0 / (count - 1) as f64
                } else {
                    50.0
                };
                for i in 0..count {
                    let start = (x(i) - half).max(0.0);
                    let end = (x(i) + half).min(100.0);
                    hovers.push(Hover {
                        x: r(start),
                        w: r(end - start),
                        title: describe(i),
                    });
                }
            }
        }

        let summary = match (self.labels.first(), self.labels.last()) {
            (Some(first), Some(last)) => {
                let names: Vec<&str> = self.series.iter().map(|s| s.name.as_str()).collect();
                format!(
                    "{} from {first} to {last}, up to {}",
                    names.join(" and "),
                    self.format(data_max)
                )
            }
            _ => "No data".to_owned(),
        };

        View {
            kind: match self.kind {
                Kind::Bar => "bar",
                Kind::Line => "line",
            },
            size: match self.size {
                ChartSize::Small => "sm",
                ChartSize::Medium => "md",
                ChartSize::Large => "lg",
            },
            legend: self
                .series
                .iter()
                .map(|s| Legend {
                    name: &s.name,
                    tone: s.tone,
                    dashed: s.dashed,
                })
                .collect(),
            ticks,
            grid,
            x_labels,
            bars,
            lines,
            hovers,
            summary,
        }
        .serialize(serializer)
    }
}

/// A small trend line for a stat card (`ui.stat(.., spark=..)`). It is
/// drawn neutral (the delta next to it carries the color) unless marked
/// [`positive`](Sparkline::positive) or [`negative`](Sparkline::negative).
///
/// ```
/// use navree_ui::Sparkline;
///
/// let spark = serde_json::to_value(Sparkline::new([3.0, 5.0, 4.0, 8.0])).unwrap();
/// assert_eq!(spark["trend"], "up");
/// assert_eq!(spark["tone"], "neutral");
/// assert!(spark["line"].as_str().unwrap().starts_with("M0 "));
/// ```
#[derive(Clone, Debug)]
pub struct Sparkline {
    values: Vec<f64>,
    tone: &'static str,
}

impl Sparkline {
    /// A sparkline of `values` (non-finite values are dropped).
    pub fn new(values: impl IntoIterator<Item = f64>) -> Self {
        Self {
            values: values.into_iter().filter(|v| v.is_finite()).collect(),
            tone: "neutral",
        }
    }

    /// Drawn in the accent: the trend is good news.
    pub fn positive(mut self) -> Self {
        self.tone = "positive";
        self
    }

    /// Drawn in the danger color: the trend is bad news.
    pub fn negative(mut self) -> Self {
        self.tone = "negative";
        self
    }
}

impl Serialize for Sparkline {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct View {
            line: String,
            area: String,
            trend: &'static str,
            tone: &'static str,
        }
        let count = self.values.len();
        let min = self.values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = self
            .values
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let span = if max > min { max - min } else { 1.0 };
        let points: Vec<(f64, f64)> = self
            .values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let x = if count > 1 {
                    i as f64 / (count - 1) as f64 * 100.0
                } else {
                    50.0
                };
                // Keep 2 units of room at the top and bottom for the stroke.
                let y = 30.0 - (v - min) / span * 28.0;
                (r(x), r(y))
            })
            .collect();
        let line = points
            .iter()
            .enumerate()
            .map(|(i, (x, y))| format!("{}{x} {y}", if i == 0 { "M" } else { "L" }))
            .collect::<Vec<_>>()
            .join(" ");
        let area = match (points.first(), points.last()) {
            (Some(first), Some(last)) => format!("{line} L{} 32 L{} 32 Z", last.0, first.0),
            _ => String::new(),
        };
        let trend = match (self.values.first(), self.values.last()) {
            (Some(first), Some(last)) if last > first => "up",
            (Some(first), Some(last)) if last < first => "down",
            _ => "flat",
        };
        View {
            line,
            area,
            trend,
            tone: self.tone,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_round_up_to_about_four_round_steps() {
        assert_eq!(scale(0.0), (4.0, 1.0));
        assert_eq!(scale(10.0), (10.0, 2.5));
        assert_eq!(scale(137.0), (150.0, 50.0));
        assert_eq!(scale(2400.0), (3000.0, 1000.0));
        assert_eq!(scale(0.8), (0.8, 0.2));
        assert_eq!(scale(99.0), (100.0, 25.0));
    }

    #[test]
    fn numbers_format_compact_and_grouped() {
        assert_eq!(compact(0.0), "0");
        assert_eq!(compact(950.0), "950");
        assert_eq!(compact(1200.0), "1.2k");
        assert_eq!(compact(3000.0), "3k");
        assert_eq!(compact(2_500_000.0), "2.5M");
        assert_eq!(compact(0.26), "0.3");
        assert_eq!(format_number(1_234_567.0), "1,234,567");
        assert_eq!(format_number(12.5), "12.5");
        assert_eq!(format_number(-1200.25), "-1,200.25");
        assert_eq!(format_number(3.0), "3");
        assert_eq!(format_number(2.999), "3");
        assert_eq!(format_number(0.05), "0.05");
    }

    #[test]
    fn line_charts_end_at_the_edges_and_describe_each_point() {
        let chart = Chart::lines(["Mon", "Tue", "Wed"])
            .series("Requests", SeriesTone::Accent, [10.0, 20.0, 15.0])
            .dashed_series("Last week", SeriesTone::Muted, [8.0, 12.0, 30.0]);
        let view = serde_json::to_value(&chart).unwrap();
        assert_eq!(view["kind"], "line");
        assert!(
            view["lines"][0]["line"]
                .as_str()
                .unwrap()
                .starts_with("M0 ")
        );
        assert!(view["lines"][0]["area"].is_string());
        assert!(view["lines"][1]["area"].is_null());
        assert_eq!(
            view["hovers"][1]["title"],
            "Tue: Requests: 20, Last week: 12"
        );
        assert_eq!(view["x_labels"][2]["show"], true);
    }

    #[test]
    fn sparklines_report_their_trend_and_tone() {
        let down = serde_json::to_value(Sparkline::new([5.0, 3.0]).negative()).unwrap();
        assert_eq!(down["trend"], "down");
        assert_eq!(down["tone"], "negative");
        let up = serde_json::to_value(Sparkline::new([5.0, 9.0]).positive()).unwrap();
        assert_eq!(up["tone"], "positive");
        let flat = serde_json::to_value(Sparkline::new([4.0, 4.0])).unwrap();
        assert_eq!(flat["trend"], "flat");
        let empty = serde_json::to_value(Sparkline::new([])).unwrap();
        assert_eq!(empty["line"], "");
    }
}
