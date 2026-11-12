//! [`Sparkline`]: a small line with no axes, beside a figure.

use super::downsample::rows_to_keep;
use super::format::NumberFormat;
use super::scale::Linear;
use super::value::{IntoValue, clean};
use super::{
    Color, Common, Drawn, TableData, TableMode, common_methods, fill, frame, n, num, pct, svg,
    value_cell,
};
use crate::components::Tone;
use crate::node::{Node, Uses, el};

/// A small line with no axes: the shape of a series beside its figure. It is a picture
/// for assistive technology, named by its title and described by its summary, with the
/// values in a table only a screen reader reads.
///
/// ```
/// use navree_ui::charts::Sparkline;
/// use navree_ui::{Stat, Tone};
///
/// let trend = Sparkline::new("Opens, last 12 hours", [3, 5, 4, 9, 12, 10, 14, 13, 18, 16, 21, 24])
///     .area()
///     .tone(Tone::Info);
/// let stat = Stat::new("Opened", "4,120").trend("+12%", true);
/// # let _ = (navree_ui::render(&trend.into()), stat);
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Sparkline {
    common: Common,
    values: Vec<Option<f64>>,
    names: Option<Vec<String>>,
    area: bool,
    color: Color,
    format: NumberFormat,
    height: u32,
    max_points: usize,
    cell: bool,
}

common_methods!(Sparkline);

impl Sparkline {
    /// A sparkline of `values`, named `title` for assistive technology.
    pub fn new<V: IntoValue>(
        title: impl Into<String>,
        values: impl IntoIterator<Item = V>,
    ) -> Self {
        let mut common = Common::new(title.into());
        common.hide_title = true;
        common.table = TableMode::ScreenReader;
        Self {
            common,
            values: clean(values),
            names: None,
            area: false,
            color: Color::Slot(1),
            format: NumberFormat::number(),
            height: 32,
            max_points: 120,
            cell: false,
        }
    }

    /// Draws it for a table cell: the picture alone in a small box, named by its summary,
    /// with no figure, no heading and no table behind it, so fifty rows hold fifty small
    /// pictures and no nested tables. A reader hears the summary (first, last, highest and
    /// lowest value); the full values are the page's own table. It keeps at most 16 points
    /// and is 24 pixels high unless [`height`](Self::height) says otherwise. It links the
    /// parts stylesheet and no script.
    pub fn cell(mut self) -> Self {
        self.cell = true;
        self.max_points = 16;
        self.height = 24;
        self
    }

    /// What each value is called in the table: `14:00`, `Nov 3`. Counted from 1 when
    /// not given.
    pub fn names<S: Into<String>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        self.names = Some(names.into_iter().map(Into::into).collect());
        self
    }

    /// Adds a wash under the line.
    pub fn area(mut self) -> Self {
        self.area = true;
        self
    }

    /// Colours the line by what it means.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.color = Color::Tone(tone);
        self
    }

    /// How values are written in the summary and the table.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.format = format;
        self
    }

    /// The height in pixels, 16 to 120 (32 by default).
    pub fn height(mut self, px: u32) -> Self {
        self.height = px.clamp(16, 120);
        self
    }

    fn into_node(self) -> Node {
        let mut this = self;
        let height = this.height;
        let drawn = this.draw();
        if this.cell {
            return Self::in_cell(this.common, drawn);
        }
        frame(this.common, drawn, "sparkline", height)
    }

    /// The cell form: one `img` named by the summary, or a dash named by the title when
    /// there is nothing to draw.
    fn in_cell(common: Common, drawn: Option<Drawn>) -> Node {
        let root = el("span").slot("sparkline-cell").uses(Uses::PARTS_STYLES);
        match drawn {
            Some(drawn) => root
                .attr("role", "img")
                .attr("aria-label", common.summary.unwrap_or(drawn.summary))
                .child(drawn.body)
                .into(),
            None => root
                .flag("data-empty", true)
                .attr("role", "img")
                .attr(
                    "aria-label",
                    format!("{}: {}", common.title, common.labels.empty),
                )
                .child("\u{2013}")
                .into(),
        }
    }

    fn draw(&mut self) -> Option<Drawn> {
        let total = self.values.len();
        if self.values.iter().all(Option::is_none) {
            return None;
        }
        let labels = self.common.labels.clone();
        let numbers = self.common.numbers.clone();
        let fmt = |v: f64| self.format.format(v, &numbers);
        let name = |at: usize| match &self.names {
            Some(names) => names.get(at).cloned().unwrap_or_default(),
            None => (at + 1).to_string(),
        };
        let xs: Vec<f64> = (0..total).map(|i| i as f64).collect();
        let kept = rows_to_keep(&xs, &[&self.values], self.max_points);
        let finite: Vec<(usize, f64)> = self
            .values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| v.map(|v| (i, v)))
            .collect();
        let (lo, hi) = finite
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (_, v)| {
                (lo.min(*v), hi.max(*v))
            });
        let scale = if hi > lo {
            Linear { lo, hi }
        } else {
            Linear {
                lo: lo - 1.0,
                hi: hi + 1.0,
            }
        };
        let h = f64::from(self.height);
        let pad = 4.0;
        let y = |v: f64| pad + (1.0 - scale.at(v)) * (h - 2.0 * pad);
        let x = |i: usize| {
            if total <= 1 {
                50.0
            } else {
                i as f64 * 100.0 / (total - 1) as f64
            }
        };
        let mut line = String::with_capacity(kept.len() * 12);
        let mut area = String::new();
        let mut run: Vec<(f64, f64)> = Vec::new();
        let flush = |run: &mut Vec<(f64, f64)>, line: &mut String, area: &mut String| {
            if run.len() >= 2 {
                for (at, (px, py)) in run.iter().enumerate() {
                    line.push(if at == 0 { 'M' } else { 'L' });
                    num(line, px * 10.0, 1);
                    line.push(' ');
                    num(line, *py, 1);
                }
                if self.area {
                    area.push('M');
                    num(area, run[0].0 * 10.0, 1);
                    area.push(' ');
                    num(area, h, 1);
                    for (px, py) in run.iter() {
                        area.push('L');
                        num(area, px * 10.0, 1);
                        area.push(' ');
                        num(area, *py, 1);
                    }
                    area.push('L');
                    num(area, run[run.len() - 1].0 * 10.0, 1);
                    area.push(' ');
                    num(area, h, 1);
                    area.push('Z');
                }
            }
            run.clear();
        };
        for i in &kept {
            match self.values[*i] {
                Some(v) => run.push((x(*i), y(v))),
                None => flush(&mut run, &mut line, &mut area),
            }
        }
        flush(&mut run, &mut line, &mut area);
        let (last_at, last) = *finite.last()?;
        let marks = svg()
            .slot("chart-marks")
            .attr("width", "100%")
            .attr("height", i64::from(self.height))
            .attr("viewBox", format!("0 0 1000 {}", self.height))
            .attr("preserveAspectRatio", "none")
            .attr("overflow", "visible")
            .children((
                (!area.is_empty())
                    .then(|| self.color.on(el("path").slot("chart-area").attr("d", area))),
                (!line.is_empty()).then(|| {
                    self.color.on(el("path")
                        .slot("chart-line")
                        .attr("d", line)
                        .attr("vector-effect", "non-scaling-stroke"))
                }),
            ));
        let drawing = svg()
            .slot("chart-plot")
            .attr("width", "100%")
            .attr("height", i64::from(self.height))
            .attr("overflow", "visible")
            .children((
                marks,
                self.color.on(el("circle")
                    .slot("chart-end")
                    .attr("cx", pct(x(last_at)))
                    .attr("cy", n(y(last), 1))
                    .attr("r", "3")),
            ));
        let (first_at, first) = finite[0];
        let (min_at, min) = finite
            .iter()
            .copied()
            .fold(finite[0], |m, p| if p.1 < m.1 { p } else { m });
        let (max_at, max) = finite
            .iter()
            .copied()
            .fold(finite[0], |m, p| if p.1 > m.1 { p } else { m });
        let summary = if finite.len() == 1 {
            fill(
                &labels.summary_one,
                &[
                    ("series", &self.common.title),
                    ("value", &fmt(last)),
                    ("x", &name(last_at)),
                ],
            )
        } else {
            fill(
                &labels.summary_series,
                &[
                    ("series", &self.common.title),
                    ("first", &fmt(first)),
                    ("first_x", &name(first_at)),
                    ("last", &fmt(last)),
                    ("last_x", &name(last_at)),
                    ("max", &fmt(max)),
                    ("max_x", &name(max_at)),
                    ("min", &fmt(min)),
                    ("min_x", &name(min_at)),
                ],
            )
        };
        let table = TableData {
            head: vec![
                (labels.category.to_string(), None),
                (self.common.title.clone(), Some(self.color)),
            ],
            rows: kept
                .iter()
                .map(|i| {
                    (
                        name(*i),
                        vec![value_cell(self.values[*i].map(fmt), &labels)],
                    )
                })
                .collect(),
            links: Vec::new(),
            note: None,
        };
        #[cfg(feature = "native-preview")]
        let native = {
            let values: Vec<Option<f64>> = kept.iter().map(|i| self.values[*i]).collect();
            let names: Vec<String> = kept.iter().map(|i| name(*i)).collect();
            crate::native::chart_numbers(
                names.iter().map(String::as_str),
                std::iter::once((self.common.title.as_str(), values.as_slice())),
            )
        };
        Some(Drawn {
            legend: None,
            body: drawing.into(),
            body_attrs: Vec::new(),
            summary,
            table,
            height: self.height,
            note: None,
            interactive: false,
            #[cfg(feature = "native-preview")]
            native,
        })
    }
}

crate::node::into_node!(Sparkline);
