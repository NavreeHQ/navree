//! [`Sparkline`]: a small line or a row of columns with no axes, beside a figure.

use super::downsample::rows_to_keep;
use super::format::NumberFormat;
use super::marks::{Curve, curve_path};
use super::motion::{Effect, Motion};
use super::paint::{ColorScale, Defs, Gradient, Ink, glow};
use super::scale::Linear;
use super::value::{IntoValue, clean};
use super::{
    Color, Common, Drawn, TableData, TableMode, common_methods, fill, frame, n, num, pct, svg,
    value_cell,
};
use crate::components::Tone;
use crate::node::{Element, Node, Uses, el};

/// A small line with no axes: the shape of a series beside its figure. It is a picture
/// for assistive technology, named by its title and described by its summary, with the
/// values in a table only a screen reader reads.
///
/// It can be a smooth line, with a wash or a gradient under it, a dot at every point,
/// a glow; or a row of columns, each in the colour of its value. It can draw itself on
/// as it comes in ([`Motion`]). A plain sparkline is drawn by the toolkit's own sheet;
/// the rest links the chart paint sheet, and none of it needs a script.
///
/// ```
/// use navree_ui::charts::{Ink, Motion, Sparkline};
/// use navree_ui::{Stat, Tone};
///
/// let trend = Sparkline::new("Opens, last 12 hours", [3, 5, 4, 9, 12, 10, 14, 13, 18, 16, 21, 24])
///     .area()
///     .tone(Tone::Info);
/// let movement = Sparkline::new("Movement, 12 days", [82, 75, 88, 92, 78, 85, 90, 87, 93, 89, 91, 94])
///     .smooth()
///     .fade()
///     .ink(Ink::slot(2))
///     .motion(Motion::new());
/// let stat = Stat::new("Opened", "4,120").trend("+12%", true);
/// # let _ = (navree_ui::render(&trend.into()), navree_ui::render(&movement.into()), stat);
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
    curve: Curve,
    fill: Option<Gradient>,
    fade: bool,
    glow: bool,
    points: bool,
    columns: bool,
    scale: Option<ColorScale>,
    motion: Motion,
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
            curve: Curve::Linear,
            fill: None,
            fade: false,
            glow: false,
            points: false,
            columns: false,
            scale: None,
            motion: Motion::none(),
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

    /// Colours it with any [`Ink`] of the theme: a chart colour, a tone, the accent.
    pub fn ink(mut self, ink: impl Into<Ink>) -> Self {
        self.color = ink.into().color();
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

    /// Draws the line as a monotone curve, which never overshoots a value.
    pub fn smooth(mut self) -> Self {
        self.curve = Curve::Monotone;
        self
    }

    /// How the line runs between points: straight, a monotone curve, or steps.
    pub fn curve(mut self, curve: Curve) -> Self {
        self.curve = curve;
        self
    }

    /// Fills under the line, or the columns, with `gradient`. [`Ink::own`] in its stops
    /// is the sparkline's colour (for columns, each column's).
    pub fn fill(mut self, gradient: Gradient) -> Self {
        self.fill = Some(gradient);
        self.area = true;
        self
    }

    /// A wash of its own colour under the line that fades to nothing at the bottom; on
    /// columns, each column fading to half.
    pub fn fade(mut self) -> Self {
        self.fade = true;
        self.area = true;
        self
    }

    /// Puts a soft glow of its own colour around the line or the columns.
    pub fn glow(mut self) -> Self {
        self.glow = true;
        self
    }

    /// Draws a small dot at every point, as well as the one at the end.
    pub fn points(mut self) -> Self {
        self.points = true;
        self
    }

    /// Draws a column for each value instead of a line: rounded at both ends, from zero.
    pub fn columns(mut self) -> Self {
        self.columns = true;
        self
    }

    /// Colours each column by the band its value falls in (the line keeps one colour).
    pub fn colors(mut self, scale: ColorScale) -> Self {
        self.scale = Some(scale);
        self
    }

    /// How it comes in: the line draws from the start, columns grow from the baseline.
    /// Off by default; always off for a reader who asks for less motion.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
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

    /// Whether it needs the chart paint sheet: anything past a plain line in the first
    /// colour or a tone.
    fn painted(&self) -> bool {
        self.fill.is_some()
            || self.fade
            || self.glow
            || self.points
            || self.columns
            || self.motion.is_on()
            || !matches!(self.color, Color::Slot(1) | Color::Tone(_))
    }

    /// The colour of value `v`: its band's, or the sparkline's.
    fn color_of(&self, v: f64) -> Color {
        match &self.scale {
            Some(scale) => scale.ink_of(v).resolve(self.color).color(),
            None => self.color,
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
        // Columns stand on zero; a line spans its own lowest and highest.
        let (lo, hi) = if self.columns {
            (lo.min(0.0), hi.max(0.0))
        } else {
            (lo, hi)
        };
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
        let mut defs = Defs::new(&self.common.id());
        let (last_at, last) = *finite.last()?;
        let mut marks: Vec<Node> = Vec::new();
        if self.columns {
            marks.push(self.columns_node(&kept, &y, &mut defs).into());
        } else {
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
            let curve = self.curve;
            let wash = self.area;
            let flush = |run: &mut Vec<(f64, f64)>, line: &mut String, area: &mut String| {
                if run.len() >= 2 {
                    curve_path(line, run, curve, false, 10.0);
                    if wash {
                        area.push('M');
                        num(area, run[0].0 * 10.0, 1);
                        area.push(' ');
                        num(area, h, 1);
                        curve_path(area, run, curve, true, 10.0);
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
            let gradient = self
                .fill
                .clone()
                .or_else(|| self.fade.then(|| Gradient::fade(Ink::own())));
            let area_path = (!area.is_empty()).then(|| {
                let path = self.color.on(el("path").slot("chart-area").attr("d", area));
                match &gradient {
                    Some(g) => path.attr(
                        "fill",
                        defs.gradient(g, self.color, Some([0.0, 0.0, 1000.0, h])),
                    ),
                    None => path,
                }
            });
            let line_path = (!line.is_empty()).then(|| {
                self.color.on(el("path")
                    .slot("chart-line")
                    .attr("d", line)
                    .attr("vector-effect", "non-scaling-stroke"))
            });
            let layer = svg()
                .slot("chart-marks")
                .attr("width", "100%")
                .attr("height", i64::from(self.height))
                .attr("viewBox", format!("0 0 1000 {}", self.height))
                .attr("preserveAspectRatio", "none")
                .attr("overflow", "visible")
                .children((area_path, line_path));
            let layer = if self.glow {
                glow(self.color.on(layer))
            } else {
                layer
            };
            marks.push(self.motion.apply(layer, Effect::Wipe, 0).into());
            if self.points {
                for (at, i) in kept.iter().enumerate() {
                    let Some(v) = self.values[*i] else { continue };
                    if *i == last_at {
                        continue;
                    }
                    let dot = self.color.on(el("circle")
                        .slot("chart-point")
                        .attr("cx", pct(x(*i)))
                        .attr("cy", n(y(v), 1))
                        .attr("r", "2.5"));
                    let fraction = x(*i) / 100.0;
                    marks.push(
                        self.motion
                            .after((fraction * 900.0) as u32)
                            .stagger(0)
                            .apply(dot, Effect::Pop, at)
                            .into(),
                    );
                }
            }
            let end = self.color.on(el("circle")
                .slot("chart-end")
                .attr("cx", pct(x(last_at)))
                .attr("cy", n(y(last), 1))
                .attr("r", "3"));
            let end = if self.glow { glow(end) } else { end };
            marks.push(self.motion.after(1000).apply(end, Effect::Pop, 0).into());
        }
        let drawing = svg()
            .slot("chart-plot")
            .attr("width", "100%")
            .attr("height", i64::from(self.height))
            .attr("overflow", "visible")
            .children((defs.node(), marks));
        let drawing = if self.painted() {
            drawing.uses(Uses::CHART_PAINT)
        } else {
            drawing
        };
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
        Some(Drawn {
            legend: None,
            body: drawing.into(),
            body_attrs: Vec::new(),
            summary,
            table,
            height: self.height,
            note: None,
            interactive: false,
            text: false,
            tip: None,
        })
    }

    /// A column for each kept value, from zero, rounded at both ends, a pixel apart.
    fn columns_node(&self, kept: &[usize], y: &dyn Fn(f64) -> f64, defs: &mut Defs) -> Element {
        let count = kept.len().max(1);
        let band = 100.0 / count as f64;
        let zero = y(0.0);
        let gradient = self
            .fill
            .clone()
            .or_else(|| self.fade.then(|| Gradient::column(Ink::own())));
        let mut painted: Vec<(Color, String)> = Vec::new();
        let mut cols: Vec<Node> = Vec::with_capacity(kept.len());
        for (at, i) in kept.iter().enumerate() {
            let Some(v) = self.values[*i] else { continue };
            let top = y(v);
            // A value of zero stays a sliver, so the row keeps its rhythm.
            let (top, h) = if (zero - top).abs() < 2.0 {
                (zero.min(top) - if v >= 0.0 { 2.0 } else { 0.0 }, 2.0)
            } else {
                (zero.min(top), (zero - top).abs())
            };
            let color = self.color_of(v);
            let col = color.on(el("rect")
                .slot("chart-column")
                .attr("x", pct(band * at as f64))
                .attr("width", pct(band))
                .attr("y", n(top, 1))
                .attr("height", n(h, 1))
                .attr("clip-path", "inset(0 max(1px, 22%) round 99px)"));
            let col = match &gradient {
                Some(g) => {
                    let url = match painted.iter().find(|(c, _)| *c == color) {
                        Some((_, url)) => url.clone(),
                        None => {
                            let url = defs.gradient(g, color, None);
                            painted.push((color, url.clone()));
                            url
                        }
                    };
                    col.attr("fill", url)
                }
                None => col,
            };
            let col = if self.glow { glow(col) } else { col };
            let effect = if v >= 0.0 {
                Effect::GrowUp
            } else {
                Effect::GrowDown
            };
            cols.push(self.motion.apply(col, effect, at).into());
        }
        el("g").slot("chart-columns").child(cols)
    }
}

crate::node::into_node!(Sparkline);
