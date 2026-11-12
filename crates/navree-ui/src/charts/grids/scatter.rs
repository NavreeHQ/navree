//! [`Scatter`]: a point for each pair of values.

use super::super::axes::{Axis, Domain, Gridlines};
use super::super::format::NumberFormat;
use super::super::marks::{RefAt, Reference, reference_line};
use super::super::motion::{Effect, Motion};
use super::super::paint::Ink;
use super::super::scale::{Linear, nice};
use super::super::value::{IntoValue, finite};
use super::super::{
    Common, Drawn, Key, LegendItem, TableData, Tip, TipLine, common_methods, fill, frame, legend,
    n, svg,
};
use super::{GridLabels, count};
use crate::node::{AttrValue, Node, Uses, el};

/// The most points a scatter draws: it thins evenly past it, and the page says so.
const MAX_POINTS: usize = 2_000;
/// A series as drawn: its place among the series, the points kept, and their names.
type Kept = (usize, Vec<(f64, f64)>, Vec<Option<String>>);
/// Room above the plot for the top tick label.
const TOP: f64 = 10.0;
/// Room for the y axis name above the top tick label.
const NAMED: f64 = 14.0;

/// One cloud of points: a name, the points, and optionally a name for each point and a
/// colour.
///
/// ```
/// use navree_ui::charts::{Ink, PointSeries};
///
/// let beats = PointSeries::new("Resting", [(812, 820), (820, 805), (805, 830)])
///     .ink(Ink::slot(5))
///     .labels(["Beat 1", "Beat 2", "Beat 3"]);
/// # let _ = beats;
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct PointSeries {
    name: String,
    points: Vec<(f64, f64)>,
    labels: Vec<String>,
    ink: Option<Ink>,
}

impl PointSeries {
    /// A series called `name`. A pair with a value that is not a finite number is left out.
    pub fn new<X: IntoValue, Y: IntoValue>(
        name: impl Into<String>,
        points: impl IntoIterator<Item = (X, Y)>,
    ) -> Self {
        Self {
            name: name.into(),
            points: points
                .into_iter()
                .filter_map(|(x, y)| Some((finite(x.into_value())?, finite(y.into_value())?)))
                .collect(),
            labels: Vec::new(),
            ink: None,
        }
    }

    /// The colour of the points (a chart colour in turn by default).
    pub fn ink(mut self, ink: impl Into<Ink>) -> Self {
        self.ink = Some(ink.into());
        self
    }

    /// A name for each point, in the order given: the head of its tooltip and the name of
    /// its row in the table. Points past the names are numbered.
    pub fn labels<S: Into<String>>(mut self, labels: impl IntoIterator<Item = S>) -> Self {
        self.labels = labels.into_iter().map(Into::into).collect();
        self
    }
}

/// A scatter plot: a point for each pair of values, in one series or several, with the two
/// axes numeric. Points are translucent, so where they pile up the cloud is darker.
///
/// Options for the look a cloud of beats or readings needs: [hidden axes](Self::x_axis),
/// [equal domains](Self::equal_domains) on both axes, so a square plot reads at once, and
/// the dashed line where the two values are equal
/// ([`Reference::identity`](crate::charts::Reference::identity)). A point is a small circle in the
/// page, a few hundred are cheap and 2,000 is the most drawn. The figure's summary gives
/// the count, the range, the average and the correlation, so a long table is not the way
/// to read it.
///
/// ```
/// use navree_ui::charts::{Axis, Gridlines, Ink, PointSeries, Reference, Scatter};
///
/// let beats = [(812, 820), (820, 805), (805, 830), (830, 841), (841, 823)];
/// let plot = Scatter::new("Successive heartbeat intervals")
///     .description("Simran Kaur, resting")
///     .locale("en-IN")
///     .series(PointSeries::new("Resting", beats).ink(Ink::slot(8)))
///     .x_name("Interval n")
///     .y_name("Interval n+1")
///     .unit("ms")
///     .equal_domains()
///     .x_axis(Axis::hidden())
///     .y_axis(Axis::hidden())
///     .grid(Gridlines::none())
///     .reference(Reference::identity().label("Equal"));
/// # let _ = navree_ui::render(&plot.into());
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Scatter {
    common: Common,
    series: Vec<PointSeries>,
    x_name: String,
    y_name: String,
    x_unit: Option<String>,
    y_unit: Option<String>,
    x_axis: Axis,
    y_axis: Axis,
    grid: Gridlines,
    x_domain: Domain,
    y_domain: Domain,
    equal: bool,
    references: Vec<Reference>,
    radius: f64,
    opacity: f64,
    height: u32,
    x_format: NumberFormat,
    y_format: NumberFormat,
    words: GridLabels,
    motion: Motion,
}

common_methods!(Scatter);

impl Scatter {
    /// A scatter plot called `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            common: Common::new(title.into()),
            series: Vec::new(),
            x_name: "X".to_owned(),
            y_name: "Y".to_owned(),
            x_unit: None,
            y_unit: None,
            x_axis: Axis::new(),
            y_axis: Axis::new(),
            grid: Gridlines::both(),
            x_domain: Domain::auto(),
            y_domain: Domain::auto(),
            equal: false,
            references: Vec::new(),
            radius: 3.5,
            opacity: 0.65,
            height: 280,
            x_format: NumberFormat::number(),
            y_format: NumberFormat::number(),
            words: GridLabels::english(),
            motion: Motion::none(),
        }
    }

    /// Adds a cloud of points.
    pub fn series(mut self, series: PointSeries) -> Self {
        self.series.push(series);
        self
    }

    /// Adds clouds.
    pub fn series_list(mut self, series: impl IntoIterator<Item = PointSeries>) -> Self {
        self.series.extend(series);
        self
    }

    /// What x is: the axis name, the tooltip's line and the table's column.
    pub fn x_name(mut self, name: impl Into<String>) -> Self {
        self.x_name = name.into();
        self
    }

    /// What y is.
    pub fn y_name(mut self, name: impl Into<String>) -> Self {
        self.y_name = name.into();
        self
    }

    /// What x and y count, written after their values: `ms`, `kg`.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        let unit = unit.into();
        self.x_unit = Some(unit.clone());
        self.y_unit = Some(unit);
        self
    }

    /// What x counts, when it is not what y counts.
    pub fn x_unit(mut self, unit: impl Into<String>) -> Self {
        self.x_unit = Some(unit.into());
        self
    }

    /// What y counts.
    pub fn y_unit(mut self, unit: impl Into<String>) -> Self {
        self.y_unit = Some(unit.into());
        self
    }

    /// What the x axis draws: [`Axis::hidden`] leaves it out, and the plot takes its room.
    pub fn x_axis(mut self, axis: Axis) -> Self {
        self.x_axis = axis;
        self
    }

    /// What the y axis draws.
    pub fn y_axis(mut self, axis: Axis) -> Self {
        self.y_axis = axis;
        self
    }

    /// The gridlines behind the points (hairlines both ways by default).
    pub fn grid(mut self, grid: Gridlines) -> Self {
        self.grid = grid;
        self
    }

    /// The range of x. The default fits the data to nice numbers, not from zero.
    pub fn x_domain(mut self, domain: Domain) -> Self {
        self.x_domain = domain;
        self
    }

    /// The range of y.
    pub fn y_domain(mut self, domain: Domain) -> Self {
        self.y_domain = domain;
        self
    }

    /// Gives both axes one range, from the lowest value of either to the highest of
    /// either, with the x domain's setting. The plot is drawn square, so the line where
    /// x equals y runs corner to corner at 45 degrees.
    pub fn equal_domains(mut self) -> Self {
        self.equal = true;
        self
    }

    /// Draws a reference: [`Reference::identity`] for the line where x equals y, or a
    /// level across at a value of y.
    pub fn reference(mut self, reference: Reference) -> Self {
        self.references.push(reference);
        self
    }

    /// The radius of a point in pixels (3.5 by default, 1 to 12).
    pub fn point_size(mut self, radius: f64) -> Self {
        if radius.is_finite() {
            self.radius = radius.clamp(1.0, 12.0);
        }
        self
    }

    /// How opaque a point is, 0.1 to 1 (0.65 by default). Where points overlap they add up.
    pub fn opacity(mut self, opacity: f64) -> Self {
        if opacity.is_finite() {
            self.opacity = opacity.clamp(0.1, 1.0);
        }
        self
    }

    /// The height of the plot in pixels (280 by default, 80 to 800).
    pub fn height(mut self, px: u32) -> Self {
        self.height = px.clamp(80, 800);
        self
    }

    /// How values of x and y are written.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.x_format = format.clone();
        self.y_format = format;
        self
    }

    /// How values of y are written, when not as x.
    pub fn y_format(mut self, format: NumberFormat) -> Self {
        self.y_format = format;
        self
    }

    /// The words the chart writes.
    pub fn words(mut self, words: GridLabels) -> Self {
        self.words = words;
        self
    }

    /// How the clouds come in: each fades in after the one before. Off by default.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
        self
    }

    fn into_node(self) -> Node {
        let mut this = self;
        let drawn = this.draw();
        let height = this.height + 32;
        frame(this.common, drawn, "scatter", height)
    }

    fn draw(&mut self) -> Option<Drawn> {
        let words = self.words.clone();
        let numbers = self.common.numbers.clone();
        let total: usize = self.series.iter().map(|s| s.points.len()).sum();
        if total == 0 {
            return None;
        }
        // Thin evenly past the most, series by series, keeping each series' share.
        let stride = total.div_ceil(MAX_POINTS).max(1);
        let series: Vec<Kept> = self
            .series
            .iter()
            .enumerate()
            .map(|(at, s)| {
                let kept: Vec<usize> = (0..s.points.len()).step_by(stride).collect();
                (
                    at,
                    kept.iter().map(|i| s.points[*i]).collect(),
                    kept.iter().map(|i| s.labels.get(*i).cloned()).collect(),
                )
            })
            .collect();
        let drawn_count: usize = series.iter().map(|(_, p, _)| p.len()).sum();

        let all = || series.iter().flat_map(|(_, p, _)| p.iter());
        let range = |pick: fn(&(f64, f64)) -> f64| {
            all()
                .map(pick)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                    (lo.min(v), hi.max(v))
                })
        };
        let (mut x_lo, mut x_hi) = range(|p| p.0);
        let (mut y_lo, mut y_hi) = range(|p| p.1);
        if self.equal {
            let (lo, hi) = (x_lo.min(y_lo), x_hi.max(y_hi));
            (x_lo, x_hi, y_lo, y_hi) = (lo, hi, lo, hi);
        }

        let height = f64::from(self.height);
        let y_count = ((height / 48.0).round() as usize).clamp(3, 8);
        let x_count = if self.equal { y_count } else { 6 };
        let (xs, x_ticks, x_places) = scale_of(self.x_domain, x_lo, x_hi, x_count);
        let (ys, y_ticks, y_places) = if self.equal {
            (xs, x_ticks.clone(), x_places)
        } else {
            scale_of(self.y_domain, y_lo, y_hi, y_count)
        };
        let fx = |v: f64| self.x_format.format(v, &numbers);
        let fy = |v: f64| self.y_format.format(v, &numbers);
        let tick_x = |v: f64| self.x_format.format_places(v, &numbers, x_places);
        let tick_y = |v: f64| self.y_format.format_places(v, &numbers, y_places);

        let named = self.y_axis.labels;
        let top = if named { TOP + NAMED } else { TOP };
        let x_band = match (self.x_axis.labels, self.x_axis.shows()) {
            (true, _) => 40.0,
            (false, true) => 10.0,
            _ => 8.0,
        };
        let base = top + height;
        let total_h = base + x_band;
        let y_at = |v: f64| top + (1.0 - ys.at(v)) * height;
        let x_pct = |v: f64| format!("{}%", n(xs.at(v) * 100.0, 2));

        // The y axis, in its own drawing so its labels keep their size.
        let y_axis = self.y_axis.shows().then(|| {
            let widest = y_ticks
                .iter()
                .map(|v| tick_y(*v).chars().count())
                .max()
                .unwrap_or(1);
            let axis_w = if self.y_axis.labels {
                (widest as f64 * 6.6 + 10.0).clamp(24.0, 120.0).round()
                    + if self.y_axis.ticks { 4.0 } else { 0.0 }
            } else {
                6.0
            };
            let edge = axis_w - 0.5;
            let texts: Vec<Node> = if self.y_axis.labels {
                y_ticks
                    .iter()
                    .map(|v| {
                        el("text")
                            .attr(
                                "x",
                                n(axis_w - if self.y_axis.ticks { 10.0 } else { 6.0 }, 0),
                            )
                            .attr("y", n(y_at(*v), 1))
                            .attr("dy", "0.32em")
                            .attr("text-anchor", "end")
                            .child(tick_y(*v))
                            .into()
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let title = (self.y_axis.labels).then(|| {
                el("text")
                    .attr("x", "0")
                    .attr("y", "9")
                    .attr("text-anchor", "start")
                    .attr("data-axis-name", "")
                    .child(match &self.y_unit {
                        Some(unit) => format!("{} ({unit})", self.y_name),
                        None => self.y_name.clone(),
                    })
            });
            let marks: Vec<Node> = if self.y_axis.ticks {
                y_ticks
                    .iter()
                    .map(|v| {
                        let y = y_at(*v).floor() + 0.5;
                        el("line")
                            .slot("chart-tick")
                            .attr("x1", n(edge, 1))
                            .attr("x2", n(axis_w - 4.5, 1))
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
                    .attr("y1", n(top, 0))
                    .attr("y2", n(base, 0))
            });
            svg()
                .slot("chart-y-axis")
                .attr("width", n(axis_w, 0))
                .attr("height", n(total_h, 0))
                .children((title, texts, marks, line))
        });

        let dashed = self.grid.dashed.then_some("");
        let mut plot: Vec<Node> = Vec::new();
        if self.grid.across {
            for v in &y_ticks {
                let y = y_at(*v).floor() + 0.5;
                plot.push(
                    el("line")
                        .slot("chart-grid")
                        .attr_opt("data-dashed", dashed)
                        .attr("x1", "0")
                        .attr("x2", "100%")
                        .attr("y1", n(y, 1))
                        .attr("y2", n(y, 1))
                        .into(),
                );
            }
        }
        if self.grid.down {
            for v in &x_ticks {
                let x = x_pct(*v);
                plot.push(
                    el("line")
                        .slot("chart-grid")
                        .attr_opt("data-dashed", dashed)
                        .attr("x1", x.clone())
                        .attr("x2", x)
                        .attr("y1", n(top, 0))
                        .attr("y2", n(base, 0))
                        .into(),
                );
            }
        }

        // References: the line where x equals y, and levels of y.
        for (at, reference) in self.references.iter().enumerate() {
            let level = |v: f64| {
                (ys.lo..=ys.hi).contains(&v).then(|| {
                    let y = y_at(v);
                    let label = reference
                        .label
                        .as_ref()
                        .map(|l| l.replace("{value}", &fy(v)));
                    reference_line(
                        ("0".to_owned(), y),
                        ("100%".to_owned(), y),
                        label.as_deref(),
                        reference.place,
                        reference.ink,
                        reference.solid,
                        false,
                    )
                })
            };
            let line = match reference.at {
                RefAt::Identity => {
                    let lo = xs.lo.max(ys.lo);
                    let hi = xs.hi.min(ys.hi);
                    (hi > lo).then(|| {
                        reference_line(
                            (x_pct(lo), y_at(lo)),
                            (x_pct(hi), y_at(hi)),
                            reference.label.as_deref(),
                            reference.place,
                            reference.ink,
                            reference.solid,
                            false,
                        )
                    })
                }
                RefAt::Level(v) => level(v),
                RefAt::Zero => level(0.0),
                RefAt::Average(i) => series
                    .iter()
                    .find(|(s, _, _)| *s == i)
                    .and_then(|(_, p, _)| {
                        (!p.is_empty()).then(|| p.iter().map(|q| q.1).sum::<f64>() / p.len() as f64)
                    })
                    .and_then(level),
            };
            if let Some(line) = line {
                plot.push(self.motion.apply(line, Effect::Fade, at).into());
            }
        }

        // The points: a group for each series, a circle for each point.
        let mut rows = Vec::with_capacity(drawn_count);
        let mut index = 0_usize;
        let multi = self.series.len() > 1;
        let mut legend_items = Vec::new();
        let mut summaries = Vec::new();
        for (at, points, labels) in &series {
            let source = &self.series[*at];
            let ink = source.ink.unwrap_or_else(|| Ink::slot((*at % 8) as u8 + 1));
            let mut circles: Vec<Node> = Vec::with_capacity(points.len());
            for (point, label) in points.iter().zip(labels) {
                circles.push(
                    el("circle")
                        .attr("cx", x_pct(point.0))
                        .attr("cy", n(y_at(point.1), 1))
                        .attr("r", n(self.radius, 1))
                        .attr("data-i", index as i64)
                        .into(),
                );
                let name = label
                    .clone()
                    .unwrap_or_else(|| fill(&words.point, &[("n", &(index + 1).to_string())]));
                let mut cells = vec![Node::from(fx(point.0)), Node::from(fy(point.1))];
                if multi {
                    cells.push(ink.on(el("span")).child(source.name.clone()).into());
                }
                rows.push((name, cells));
                index += 1;
            }
            plot.push(
                self.motion
                    .apply(
                        ink.on(el("g"))
                            .slot("chart-points")
                            .attr("fill-opacity", n(self.opacity, 2)),
                        Effect::Fade,
                        *at,
                    )
                    .child(circles)
                    .into(),
            );
            legend_items.push(LegendItem {
                name: source.name.clone(),
                color: ink.color(),
                key: Key::Block,
            });
            summaries.push(self.summary_of(source, &words, &fx, &fy));
        }

        // The x labels and the axis under the plot.
        if self.x_axis.shows() {
            let mut axis: Vec<Node> = Vec::new();
            if self.x_axis.line {
                axis.push(
                    el("line")
                        .slot("chart-axis-line")
                        .attr("x1", "0")
                        .attr("x2", "100%")
                        .attr("y1", n(base + 0.5, 1))
                        .attr("y2", n(base + 0.5, 1))
                        .into(),
                );
            }
            for (at, v) in x_ticks.iter().enumerate() {
                let x = x_pct(*v);
                if self.x_axis.ticks {
                    axis.push(
                        el("line")
                            .slot("chart-tick")
                            .attr("x1", x.clone())
                            .attr("x2", x.clone())
                            .attr("y1", n(base, 1))
                            .attr("y2", n(base + 4.0, 1))
                            .into(),
                    );
                }
                if self.x_axis.labels {
                    axis.push(
                        el("text")
                            .attr("x", x)
                            .attr("y", n(base + 16.0, 1))
                            .attr("text-anchor", "middle")
                            .attr_opt(
                                "data-minor",
                                (at % 2 == 1 && x_ticks.len() > 4).then_some(""),
                            )
                            .child(tick_x(*v))
                            .into(),
                    );
                }
            }
            if self.x_axis.labels {
                axis.push(
                    el("text")
                        .attr("x", "100%")
                        .attr("y", n(base + 34.0, 1))
                        .attr("text-anchor", "end")
                        .attr("data-axis-name", "")
                        .child(match &self.x_unit {
                            Some(unit) => format!("{} ({unit})", self.x_name),
                            None => self.x_name.clone(),
                        })
                        .into(),
                );
            }
            plot.push(el("g").slot("chart-x-axis").child(axis).into());
        }

        let plot_svg = svg()
            .slot("chart-plot")
            .attr(
                "width",
                if self.equal {
                    n(height, 0)
                } else {
                    "100%".to_owned()
                },
            )
            .attr("height", n(total_h, 0))
            .child(plot);
        let frame_el = el("div")
            .slot("chart-frame")
            .attr_opt("data-bare", y_axis.is_none().then_some(""))
            .attr_opt("data-square", self.equal.then_some(""))
            .uses(Uses::CHART_GRIDS)
            .children((y_axis, el("div").slot("chart-plot-wrap").child(plot_svg)));

        let head_name = |name: &str, unit: &Option<String>| match unit {
            Some(unit) => format!("{name} ({unit})"),
            None => name.to_owned(),
        };
        let mut head = vec![
            (words.point.replace("{n}", "").trim().to_owned(), None),
            (head_name(&self.x_name, &self.x_unit), None),
            (head_name(&self.y_name, &self.y_unit), None),
        ];
        let mut lines = vec![
            TipLine {
                name: self.x_name.clone(),
                unit: self.x_unit.clone(),
                color: None,
                key: false,
            },
            TipLine {
                name: self.y_name.clone(),
                unit: self.y_unit.clone(),
                color: None,
                key: false,
            },
        ];
        if multi {
            head.push((words.series.to_string(), None));
            lines.push(TipLine {
                name: words.series.to_string(),
                unit: None,
                color: None,
                key: true,
            });
        }
        let cut = drawn_count < total;
        Some(Drawn {
            legend: multi.then(|| legend(&self.common.id(), &legend_items, false)),
            body: frame_el.into(),
            body_attrs: vec![
                ("data-mode", AttrValue::Static("points")),
                ("data-rows", AttrValue::Int(drawn_count as i64)),
            ],
            summary: summaries.join(" "),
            table: TableData {
                head,
                rows,
                links: Vec::new(),
                note: None,
            },
            height: total_h as u32,
            note: cut.then(|| {
                fill(
                    &words.scatter_cut,
                    &[
                        ("shown", &count(drawn_count, &numbers)),
                        ("total", &count(total, &numbers)),
                    ],
                )
            }),
            interactive: true,
            tip: Some(Tip { big: false, lines }),
        })
    }

    /// A series in words: how many, the range and average of each value, and the
    /// correlation when there is one.
    fn summary_of(
        &self,
        series: &PointSeries,
        words: &GridLabels,
        fx: &dyn Fn(f64) -> String,
        fy: &dyn Fn(f64) -> String,
    ) -> String {
        let points = &series.points;
        let count = points.len();
        let stat = |pick: fn(&(f64, f64)) -> f64| {
            let lo = points.iter().map(pick).fold(f64::INFINITY, f64::min);
            let hi = points.iter().map(pick).fold(f64::NEG_INFINITY, f64::max);
            let mean = points.iter().map(pick).sum::<f64>() / count.max(1) as f64;
            (lo, hi, mean)
        };
        if count == 0 {
            return String::new();
        }
        let (x_min, x_max, x_mean) = stat(|p| p.0);
        let (y_min, y_max, y_mean) = stat(|p| p.1);
        let mut out = fill(
            &words.scatter_summary,
            &[
                ("series", &series.name),
                ("count", &self.common_count(count)),
                ("x", &self.x_name),
                ("x_min", &fx(x_min)),
                ("x_max", &fx(x_max)),
                ("x_mean", &fx(x_mean)),
                ("y", &self.y_name),
                ("y_min", &fy(y_min)),
                ("y_max", &fy(y_max)),
                ("y_mean", &fy(y_mean)),
            ],
        );
        if let Some(r) = correlation(points) {
            let text = NumberFormat::decimals(2)
                .fixed()
                .format(r, &self.common.numbers);
            out.push_str(&fill(&words.scatter_correlation, &[("r", &text)]));
        }
        out
    }

    fn common_count(&self, count: usize) -> String {
        NumberFormat::integer().format(count as f64, &self.common.numbers)
    }
}

/// The scale of an axis over data from `lo` to `hi`, its ticks, and the decimals they need.
fn scale_of(domain: Domain, lo: f64, hi: f64, count: usize) -> (Linear, Vec<f64>, u8) {
    let widened = |a: f64, b: f64| {
        let t = nice(a, b, count, false);
        (t.scale, t.values, t.places)
    };
    match domain.range(lo, hi) {
        Some((a, b)) if domain.is_exact() && b > a => {
            let t = nice(a, b, count, false);
            let inside = t
                .values
                .into_iter()
                .filter(|v| *v >= a - (b - a) * 1e-9 && *v <= b + (b - a) * 1e-9)
                .collect();
            (Linear { lo: a, hi: b }, inside, t.places)
        }
        Some((a, b)) => widened(a, b),
        None => widened(lo, hi),
    }
}

/// Pearson's r over the pairs, or `None` when a value does not vary or there are too few.
fn correlation(points: &[(f64, f64)]) -> Option<f64> {
    let count = points.len();
    if count < 3 {
        return None;
    }
    let mean_x = points.iter().map(|p| p.0).sum::<f64>() / count as f64;
    let mean_y = points.iter().map(|p| p.1).sum::<f64>() / count as f64;
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in points {
        sxy += (x - mean_x) * (y - mean_y);
        sxx += (x - mean_x).powi(2);
        syy += (y - mean_y).powi(2);
    }
    let r = sxy / (sxx * syy).sqrt();
    r.is_finite().then_some(r.clamp(-1.0, 1.0))
}

crate::node::into_node!(Scatter);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    fn beats() -> Vec<(f64, f64)> {
        (0..40)
            .map(|i| {
                let x = 800.0 + f64::from(i) * 3.0 + f64::from(i % 5) * 4.0;
                (x, x + f64::from((i * 7) % 11) - 5.0)
            })
            .collect()
    }

    #[test]
    fn a_point_is_one_small_circle_and_a_series_one_group() {
        let html = render(
            &Scatter::new("Beats")
                .id("sc")
                .series(PointSeries::new("Resting", beats()))
                .into(),
        );
        assert_eq!(html.matches("<circle").count(), 40, "{html}");
        assert_eq!(html.matches(r#"data-slot="chart-points""#).count(), 1);
        assert!(html.contains(r#"fill-opacity="0.65""#), "{html}");
        assert!(html.contains(r#"data-rows="40""#));
        assert!(html.contains(r#"data-color="1""#));
        assert!(!html.contains("style="), "no style attribute");
        // A point costs a short element, not a path of its own with a gradient and a class.
        let circle = &html[html.find("<circle").unwrap()..];
        assert!(circle[..circle.find('>').unwrap()].len() < 80, "{circle}");
    }

    #[test]
    fn equal_domains_share_one_scale_and_square_the_plot() {
        let html = render(
            &Scatter::new("Beats")
                .series(PointSeries::new("Resting", [(1, 90), (50, 20), (95, 60)]))
                .equal_domains()
                .height(300)
                .into(),
        );
        assert!(html.contains(r#"data-square"#), "{html}");
        assert!(
            html.contains(
                r#"<svg aria-hidden="true" focusable="false" data-slot="chart-plot" width="300""#
            ),
            "{html}"
        );
        // The lowest value (1, on x) and the highest (95, on y) set both ends of both axes:
        // the labels along x and up y are the same set.
        let x_labels: Vec<&str> = html
            .split(r#"text-anchor="middle""#)
            .skip(1)
            .map(|t| &t[t.find('>').unwrap() + 1..t.find('<').unwrap()])
            .collect();
        let y_axis = &html[html.find(r#"data-slot="chart-y-axis""#).unwrap()..];
        let y_axis = &y_axis[..y_axis.find("</svg>").unwrap()];
        for label in x_labels {
            assert!(y_axis.contains(&format!(">{label}<")), "{label}: {y_axis}");
        }
    }

    #[test]
    fn the_identity_line_runs_from_corner_to_corner_on_equal_domains() {
        let html = render(
            &Scatter::new("Beats")
                .series(PointSeries::new("Resting", [(0, 100), (100, 0)]))
                .equal_domains()
                .height(200)
                .x_axis(Axis::hidden())
                .y_axis(Axis::hidden())
                .grid(Gridlines::none())
                .reference(Reference::identity().label("Equal"))
                .into(),
        );
        // Axes hidden: the plot starts TOP down. From (0, 0) at the bottom to (100, 100) at the top.
        assert!(
            html.contains(r#"<line x1="0%" y1="210" x2="100%" y2="10">"#),
            "{html}"
        );
        assert!(html.contains(">Equal</text>"), "{html}");
        assert!(html.contains(r#"data-slot="chart-ref""#));
        assert!(
            html.contains("data-bare"),
            "a hidden y axis leaves its column out"
        );
        assert!(!html.contains(r#"data-slot="chart-y-axis""#));
        assert!(!html.contains(r#"data-slot="chart-grid""#));
    }

    #[test]
    fn several_series_are_told_apart_in_the_legend_the_tooltip_and_the_table() {
        let html = render(
            &Scatter::new("Readings")
                .id("two")
                .series(PointSeries::new("Morning", [(1, 2), (2, 3)]).labels(["A", "B"]))
                .series(PointSeries::new("Evening", [(2, 1), (3, 5), (4, 4)]).ink(Ink::slot(6)))
                .x_name("Sleep")
                .y_name("Mood")
                .x_unit("h")
                .into(),
        );
        assert_eq!(html.matches("<circle").count(), 5);
        assert!(html.contains(r#"data-slot="chart-legend""#), "{html}");
        assert!(
            html.contains("<th scope=\"row\">A</th>")
                && html.contains("<th scope=\"row\">Point 3</th>"),
            "{html}"
        );
        assert!(
            html.contains("Sleep (h)") && html.contains(">Series<"),
            "{html}"
        );
        assert!(
            html.contains(r#"<span data-color="6">Evening</span>"#),
            "{html}"
        );
        // Indices run through both series so the keys and the table agree.
        assert!(html.contains(r#"data-i="4""#));
    }

    #[test]
    fn a_big_cloud_is_thinned_and_summarised() {
        let many: Vec<(f64, f64)> = (0..7_000)
            .map(|i| (f64::from(i), f64::from((i * 37) % 1000)))
            .collect();
        let html = render(
            &Scatter::new("Cloud")
                .series(PointSeries::new("All", many))
                .into(),
        );
        let drawn = html.matches("<circle").count();
        assert!((1_500..=2_000).contains(&drawn), "{drawn}");
        assert!(
            html.contains("The drawing shows 1,750 of 7,000 points"),
            "{}",
            &html[html.len().saturating_sub(300)..]
        );
        assert!(
            html.contains("All: 7,000 points."),
            "the summary stands for all the points"
        );
    }

    #[test]
    fn the_summary_gives_the_count_range_average_and_correlation() {
        let html = render(
            &Scatter::new("Beats")
                .locale("en-IN")
                .series(PointSeries::new(
                    "Resting",
                    [(1000, 1000), (2000, 2100), (300_000, 310_000)],
                ))
                .x_name("Interval")
                .y_name("Next")
                .into(),
        );
        assert!(
            html.contains("Resting: 3 points. Interval from 1,000 to 3,00,000, average"),
            "{html}"
        );
        assert!(html.contains("Correlation 1.00."), "{html}");
        // Too few points, or no spread: no correlation claimed.
        let flat = render(
            &Scatter::new("x")
                .series(PointSeries::new("a", [(1, 1), (1, 2), (1, 3)]))
                .into(),
        );
        assert!(!flat.contains("Correlation"), "{flat}");
    }

    #[test]
    fn levels_and_averages_are_drawn_where_they_fall() {
        let html = render(
            &Scatter::new("Beats")
                .series(PointSeries::new("a", [(0, 0), (10, 10), (20, 20)]))
                .reference(Reference::level(10).label("At {value}"))
                .reference(Reference::average())
                .reference(Reference::level(9_999))
                .into(),
        );
        assert!(html.contains(">At 10</text>"), "{html}");
        assert_eq!(
            html.matches(r#"data-slot="chart-ref""#).count(),
            2,
            "a level off the plot is left out"
        );
    }

    #[test]
    fn odd_input_draws_or_is_empty() {
        assert!(render(&Scatter::new("x").into()).contains(r#"data-state="empty""#));
        let none = PointSeries::new("a", [(f64::NAN, 1.0), (1.0, f64::INFINITY)]);
        assert!(render(&Scatter::new("x").series(none).into()).contains(r#"data-state="empty""#));
        for pts in [
            vec![(5.0, 5.0)],
            vec![(1.0, 1.0), (1.0, 1.0), (1.0, 1.0)],
            vec![(f64::MAX, f64::MIN), (-1e300, 1e300)],
            vec![(1e-300, 2e-300), (0.0, 0.0)],
        ] {
            let html = render(
                &Scatter::new("x")
                    .series(PointSeries::new("a", pts))
                    .equal_domains()
                    .reference(Reference::identity())
                    .x_domain(Domain::padded(1))
                    .into(),
            );
            assert!(!html.contains("NaN") && !html.contains("inf"), "{html}");
        }
        let tiny = Scatter::new("x")
            .series(PointSeries::new("a", [(1, 2)]))
            .point_size(f64::NAN)
            .opacity(9.0)
            .height(1);
        let html = render(&tiny.into());
        assert!(
            html.contains(r#"fill-opacity="1""#) && html.contains(r#"r="3.5""#),
            "{html}"
        );
    }

    #[test]
    fn the_page_links_the_modules_it_needs() {
        let node: Node = Scatter::new("x")
            .series(PointSeries::new("a", [(1, 2)]))
            .into();
        assert!(
            crate::View::new("x")
                .body(node)
                .uses()
                .has(Uses::CHART_GRIDS.with(Uses::CHARTS))
        );
        let plain: Node = Scatter::new("x")
            .series(PointSeries::new("a", [(1, 2)]))
            .motion(Motion::new())
            .into();
        assert!(
            crate::View::new("x")
                .body(plain)
                .uses()
                .has(Uses::CHART_PAINT)
        );
    }
}
