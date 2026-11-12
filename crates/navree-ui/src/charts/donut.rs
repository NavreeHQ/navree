//! [`Donut`]: one whole split into a few parts, with the total in the middle.

use super::format::NumberFormat;
use super::value::{IntoValue, finite};
use super::{Color, Common, Drawn, TableData, common_methods, fill, frame, n, svg, value_cell};
use crate::components::Tone;
use crate::node::{AttrValue, Node, el};

/// One part of a donut.
#[derive(Clone, Debug)]
struct Part {
    name: String,
    value: Option<f64>,
    color: Option<Color>,
}

/// One whole split into parts: at most six and Other, read at a glance, each named
/// with its value and share in the legend. To compare parts with each other, ranked
/// bars read better.
///
/// Parts at zero, missing or negative draw nothing and stay in the table.
///
/// ```
/// use navree_ui::charts::Donut;
/// use navree_ui::Tone;
///
/// let donut = Donut::new("Delivery")
///     .part("Delivered", 9_412)
///     .part_tone("Bounced", 188, Tone::Danger)
///     .part("Deferred", 40);
/// # let _ = navree_ui::render(&donut.into());
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Donut {
    common: Common,
    parts: Vec<Part>,
    format: NumberFormat,
    hole: f64,
    size: u32,
    most: usize,
    center: Option<(String, String)>,
}

common_methods!(Donut);

impl Donut {
    /// A donut called `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            common: Common::new(title.into()),
            parts: Vec::new(),
            format: NumberFormat::number(),
            hole: 0.62,
            size: 168,
            most: 6,
            center: None,
        }
    }

    /// Adds a part.
    pub fn part(mut self, name: impl Into<String>, value: impl IntoValue) -> Self {
        self.parts.push(Part {
            name: name.into(),
            value: finite(value.into_value()),
            color: None,
        });
        self
    }

    /// Adds a part coloured by what it means.
    pub fn part_tone(mut self, name: impl Into<String>, value: impl IntoValue, tone: Tone) -> Self {
        self.parts.push(Part {
            name: name.into(),
            value: finite(value.into_value()),
            color: Some(Color::Tone(tone)),
        });
        self
    }

    /// Adds parts from `(name, value)` pairs.
    pub fn parts<S: Into<String>, V: IntoValue>(
        mut self,
        parts: impl IntoIterator<Item = (S, V)>,
    ) -> Self {
        for (name, value) in parts {
            self = self.part(name, value);
        }
        self
    }

    /// How values are written.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.format = format;
        self
    }

    /// A pie: no hole, no total in the middle.
    pub fn pie(mut self) -> Self {
        self.hole = 0.0;
        self
    }

    /// The drawing's width and height in pixels, 96 to 320 (168 by default).
    pub fn size(mut self, px: u32) -> Self {
        self.size = px.clamp(96, 320);
        self
    }

    /// The most parts drawn (6 by default, 2 to 8): the smallest of the rest fold
    /// into Other.
    pub fn max_parts(mut self, most: usize) -> Self {
        self.most = most.clamp(2, 8);
        self
    }

    /// What the middle says: a value and the words under it. The total, by default.
    pub fn center(mut self, value: impl Into<String>, label: impl Into<String>) -> Self {
        self.center = Some((value.into(), label.into()));
        self
    }

    fn into_node(self) -> Node {
        let mut this = self;
        let size = this.size;
        let drawn = this.draw();
        frame(this.common, drawn, "donut", size)
    }

    fn draw(&mut self) -> Option<Drawn> {
        let labels = self.common.labels.clone();
        let numbers = self.common.numbers.clone();
        let fmt = |v: f64| self.format.format(v, &numbers);
        // Colours follow the parts in the order given, before any fold.
        let mut slot = 0_u8;
        let mut parts: Vec<(String, Option<f64>, Color)> = self
            .parts
            .iter()
            .map(|p| {
                let color = p.color.unwrap_or_else(|| {
                    slot = slot % 8 + 1;
                    Color::Slot(slot)
                });
                (p.name.clone(), p.value, color)
            })
            .collect();
        let positive = |v: Option<f64>| v.filter(|v| *v > 0.0);
        if parts.iter().filter(|p| positive(p.1).is_some()).count() > self.most {
            // The largest stay, in their order; the rest fold into Other.
            let mut ranked: Vec<usize> = (0..parts.len())
                .filter(|i| positive(parts[*i].1).is_some())
                .collect();
            ranked.sort_by(|a, b| {
                positive(parts[*b].1)
                    .unwrap_or(0.0)
                    .total_cmp(&positive(parts[*a].1).unwrap_or(0.0))
            });
            let keep: Vec<usize> = ranked[..self.most - 1].to_vec();
            let rest: f64 = ranked[self.most - 1..]
                .iter()
                .filter_map(|i| parts[*i].1)
                .sum();
            let mut kept: Vec<(String, Option<f64>, Color)> = Vec::new();
            // Parts that draw nothing stay in the table of a short list only.
            let short = parts.len() <= 2 * self.most;
            for (at, part) in parts.into_iter().enumerate() {
                if keep.contains(&at) || (short && positive(part.1).is_none()) {
                    kept.push(part);
                }
            }
            kept.push((labels.other.to_string(), Some(rest), Color::Other));
            parts = kept;
        }
        // Shares are worked out against the largest part, so parts near the largest
        // number a float holds still add up; only the total shown may be too large.
        let top = parts
            .iter()
            .filter_map(|p| positive(p.1))
            .fold(0.0_f64, f64::max);
        if !(top > 0.0 && top.is_finite()) {
            return None;
        }
        let scaled: f64 = parts
            .iter()
            .filter_map(|p| positive(p.1))
            .map(|v| v / top)
            .sum();
        let fraction = |v: f64| (v / top) / scaled;
        let total = top * scaled;
        let share = |v: f64| NumberFormat::percent().format(fraction(v), &numbers);

        // The slices, clockwise from twelve o'clock.
        let outer = 100.0;
        let inner = self.hole * 100.0;
        let mut start = 0.0_f64;
        let mut slices: Vec<Node> = Vec::new();
        for (at, (_, value, color)) in parts.iter().enumerate() {
            let Some(v) = positive(*value) else { continue };
            let sweep = fraction(v) * std::f64::consts::TAU;
            let end = start + sweep;
            let d = arc(start, end, outer, inner);
            slices.push(
                color
                    .on(el("path")
                        .slot("chart-slice")
                        .attr("data-i", at as i64)
                        .attr("d", d))
                    .into(),
            );
            start = end;
        }
        let (center_value, center_label) = self
            .center
            .clone()
            .unwrap_or_else(|| (fmt(total), labels.total.to_string()));
        let drawing = svg()
            .slot("chart-donut")
            .attr("viewBox", "-102 -102 204 204")
            .attr("width", i64::from(self.size))
            .attr("height", i64::from(self.size))
            .child(slices);
        let middle = (self.hole > 0.0).then(|| {
            el("div")
                .slot("chart-donut-center")
                .attr("aria-hidden", "true")
                .children((
                    el("strong").child(center_value),
                    el("span").child(center_label),
                ))
        });
        // The legend names each part with its value and share: the reader needs no hover.
        let items: Vec<Node> = parts
            .iter()
            .enumerate()
            .map(|(at, (name, value, color))| {
                el("li")
                    .slot("chart-legend-item")
                    .attr("data-i", at as i64)
                    .children((
                        color
                            .on(el("span").slot("chart-key").attr("data-key", "block"))
                            .attr("aria-hidden", "true"),
                        color.icon().map(crate::components::icon),
                        el("span").slot("chart-legend-name").child(name.clone()),
                        el("span").slot("chart-legend-value").child(match value {
                            Some(v) => format!("{} · {}", fmt(*v), share(v.max(0.0))),
                            None => "–".to_owned(),
                        }),
                    ))
                    .into()
            })
            .collect();
        let body = el("div").slot("chart-donut-frame").children((
            el("div")
                .slot("chart-donut-plot")
                .children((drawing, middle)),
            el("ul")
                .slot("chart-legend")
                .attr("data-layout", "list")
                .child(items),
        ));

        let largest = parts
            .iter()
            .filter_map(|p| positive(p.1).map(|v| (p, v)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let summary = largest.map_or_else(String::new, |(p, v)| {
            fill(
                &labels.summary_parts,
                &[
                    ("count", &parts.len().to_string()),
                    ("total", &fmt(total)),
                    ("name", &p.0),
                    ("share", &share(v)),
                ],
            )
        });
        let table = TableData {
            head: vec![
                (labels.category.to_string(), None),
                (labels.value.to_string(), None),
                (labels.share.to_string(), None),
            ],
            rows: parts
                .iter()
                .map(|(name, value, _)| {
                    (
                        name.clone(),
                        vec![
                            value_cell(value.map(fmt), &labels),
                            value_cell(value.map(|v| share(v.max(0.0))), &labels),
                        ],
                    )
                })
                .collect(),
            links: Vec::new(),
            note: None,
        };
        Some(Drawn {
            legend: None,
            body: body.into(),
            body_attrs: vec![
                ("data-mode", AttrValue::Static("parts")),
                ("data-rows", AttrValue::Int(parts.len() as i64)),
            ],
            summary,
            table,
            height: self.size,
            note: None,
            interactive: true,
            text: false,
            tip: None,
        })
    }
}

crate::node::into_node!(Donut);

/// A ring slice from angle `a` to `b` (radians clockwise from twelve), between radii
/// `inner` and `outer`. A whole ring is drawn as two halves, which an arc can.
fn arc(a: f64, b: f64, outer: f64, inner: f64) -> String {
    if b - a >= std::f64::consts::TAU - 1e-9 {
        let half = a + std::f64::consts::PI;
        let mut d = arc(a, half, outer, inner);
        d.push_str(&arc(half, a + std::f64::consts::TAU - 1e-6, outer, inner));
        return d;
    }
    let point = |angle: f64, r: f64| (r * angle.sin(), -r * angle.cos());
    let large = if b - a > std::f64::consts::PI { 1 } else { 0 };
    let (x0, y0) = point(a, outer);
    let (x1, y1) = point(b, outer);
    let mut d = format!(
        "M{} {}A{} {} 0 {large} 1 {} {}",
        n(x0, 2),
        n(y0, 2),
        n(outer, 2),
        n(outer, 2),
        n(x1, 2),
        n(y1, 2)
    );
    if inner > 0.0 {
        let (x2, y2) = point(b, inner);
        let (x3, y3) = point(a, inner);
        d.push_str(&format!(
            "L{} {}A{} {} 0 {large} 0 {} {}Z",
            n(x2, 2),
            n(y2, 2),
            n(inner, 2),
            n(inner, 2),
            n(x3, 2),
            n(y3, 2)
        ));
    } else {
        d.push_str("L0 0Z");
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarter_runs_clockwise_from_twelve() {
        let d = arc(0.0, std::f64::consts::FRAC_PI_2, 100.0, 50.0);
        assert_eq!(d, "M0 -100A100 100 0 0 1 100 0L50 0A50 50 0 0 0 0 -50Z");
    }

    #[test]
    fn a_whole_ring_is_two_halves() {
        let d = arc(0.0, std::f64::consts::TAU, 100.0, 60.0);
        assert_eq!(d.matches('M').count(), 2);
    }
}
