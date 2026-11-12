//! Marks written over or under the data: reference lines ([`Reference`]) and target
//! bands ([`Zone`]); how a line shows its points ([`Dots`], [`ActiveDot`]) and how it
//! runs between them ([`Curve`]); and what its tooltip says ([`Tooltip`]).
//!
//! Inside the crate, [`curve_path`] writes a run of points as a path in any of the
//! curves, [`reference_line`] draws a labelled line between any two points and
//! [`zone_rect`] a labelled band, so a new chart draws them the same way.

use super::paint::Ink;
use super::value::IntoValue;
use super::{Color, n, num};
use crate::node::{Element, el};

/// Where a reference line's label sits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Place {
    /// Above the line at its end: the default.
    #[default]
    EndAbove,
    /// Below the line at its end.
    EndBelow,
    /// Above the line at its start.
    StartAbove,
    /// Below the line at its start.
    StartBelow,
}

/// A line drawn for reference: a level across the plot (a target, a resting rate), the
/// average of a series, a zero baseline, or the diagonal where x equals y. Dashed and
/// in a muted ink unless told otherwise; its label can name its value with `{value}`.
///
/// ```
/// use navree_ui::charts::{Chart, Ink, Place, Reference, Series};
/// use navree_ui::Tone;
///
/// let chart = Chart::bar("Stress by hour")
///     .categories(["06:00", "07:00", "08:00", "09:00"])
///     .series(Series::new("Stress", [3.2, 5.1, 4.4, 6.0]))
///     .reference(Reference::average().label("Avg: {value}"))
///     .reference(Reference::level(5).label("High").ink(Ink::tone(Tone::Warning)).place(Place::StartAbove));
/// # let _ = navree_ui::render(&chart.into());
/// ```
#[derive(Clone, Debug, PartialEq)]
#[must_use]
pub struct Reference {
    pub(crate) at: RefAt,
    pub(crate) label: Option<String>,
    pub(crate) place: Place,
    pub(crate) ink: Option<Ink>,
    pub(crate) solid: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RefAt {
    Level(f64),
    Average(usize),
    Zero,
    Identity,
}

impl Reference {
    fn new(at: RefAt) -> Self {
        Self {
            at,
            label: None,
            place: Place::EndAbove,
            ink: None,
            solid: false,
        }
    }

    /// A level across the plot at `value`. A value that is not a finite number is zero.
    pub fn level(value: impl IntoValue) -> Self {
        Self::new(RefAt::Level(value.into_value().unwrap_or(0.0)))
    }

    /// A level at the average of the first series, over all its values.
    pub fn average() -> Self {
        Self::new(RefAt::Average(0))
    }

    /// A level at the average of series `index` (from 0, in the order added).
    pub fn average_of(index: usize) -> Self {
        Self::new(RefAt::Average(index))
    }

    /// A baseline at zero, drawn even when zero is not a tick.
    pub fn zero() -> Self {
        Self::new(RefAt::Zero)
    }

    /// The diagonal where x equals y, across what both axes share: for a chart with
    /// numbers along x, such as one value against the next.
    pub fn identity() -> Self {
        Self::new(RefAt::Identity)
    }

    /// What the line is. `{value}` in it is the line's value, written as the chart
    /// writes values (`Avg: {value}`).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Where the label sits.
    pub fn place(mut self, place: Place) -> Self {
        self.place = place;
        self
    }

    /// The line's colour (a muted ink by default): a level per category in its colour.
    pub fn ink(mut self, ink: impl Into<Ink>) -> Self {
        self.ink = Some(ink.into());
        self
    }

    /// Draws the line solid rather than dashed.
    pub fn solid(mut self) -> Self {
        self.solid = true;
        self
    }
}

/// A band of the value axis: a target zone, a healthy range, a heart-rate zone. It is
/// shaded behind the data across the plot, in a soft wash of its ink, with its label
/// inside its top edge.
///
/// ```
/// use navree_ui::charts::{Chart, Ink, Series, Zone};
/// use navree_ui::Tone;
///
/// let chart = Chart::line("Resting heart rate")
///     .categories(["Mon", "Tue", "Wed"])
///     .series(Series::new("Resting", [58, 61, 57]))
///     .zone(Zone::new(50, 60).label("Healthy").ink(Ink::tone(Tone::Success)));
/// # let _ = navree_ui::render(&chart.into());
/// ```
#[derive(Clone, Debug, PartialEq)]
#[must_use]
pub struct Zone {
    pub(crate) from: f64,
    pub(crate) to: f64,
    pub(crate) label: Option<String>,
    pub(crate) ink: Option<Ink>,
}

impl Zone {
    /// The band from `from` to `to` (in either order).
    pub fn new(from: impl IntoValue, to: impl IntoValue) -> Self {
        let a = from.into_value().unwrap_or(0.0);
        let b = to.into_value().unwrap_or(0.0);
        Self {
            from: a.min(b),
            to: a.max(b),
            label: None,
            ink: None,
        }
    }

    /// What the band is.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The band's colour (the muted ink by default).
    pub fn ink(mut self, ink: impl Into<Ink>) -> Self {
        self.ink = Some(ink.into());
        self
    }
}

/// Which points of a line show a dot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Dots {
    /// None: the line alone.
    None,
    /// The last point: where the series stands now. The default.
    #[default]
    End,
    /// Every point.
    All,
}

/// The dot that marks the value under the pointer or the keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActiveDot {
    /// A small dot with a ring of the surface: the default.
    #[default]
    Plain,
    /// A larger dot with a ring of the surface.
    Large,
    /// A dot with a soft halo of its own colour.
    Halo,
}

/// How a line runs from one point to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Curve {
    /// Straight segments: the default.
    #[default]
    Linear,
    /// A monotone curve, which never overshoots a value.
    Monotone,
    /// Steps: each value holds until the next x, then the line jumps. For states that
    /// last, such as sleep stages.
    Step,
}

/// What a chart's tooltip shows, past the default (the row's name, then a line per
/// series with its key, value and name).
///
/// ```
/// use navree_ui::charts::{Chart, Series, Tooltip};
///
/// let chart = Chart::area("Heart rate")
///     .categories(["06:00", "06:05", "06:10"])
///     .series(Series::new("Heart rate", [64, 71, 69]).unit("BPM"))
///     .tooltip(Tooltip::big());
/// # let _ = navree_ui::render(&chart.into());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct Tooltip {
    pub(crate) big: bool,
}

impl Tooltip {
    /// The default: a line per series.
    pub const fn new() -> Self {
        Self { big: false }
    }

    /// The first series' value large, in its colour, with its unit: one figure that
    /// matters, read at a glance. The other lines follow in the usual size.
    pub const fn big() -> Self {
        Self { big: true }
    }
}

/// Writes a run of points as path commands in the curve: `M` first (or `L` when
/// `join`), then straight lines, a monotone curve, or steps. `scale_x` multiplies x (10
/// turns percent into a 0..1000 stretched layer). Fewer than three points draw
/// straight, whatever the curve, except steps.
pub(crate) fn curve_path(out: &mut String, points: &[(f64, f64)], curve: Curve, join: bool, scale_x: f64) {
    let x = |v: f64| v * scale_x;
    let Some((first_x, first_y)) = points.first() else {
        return;
    };
    out.push(if join { 'L' } else { 'M' });
    num(out, x(*first_x), 1);
    out.push(' ');
    num(out, *first_y, 1);
    match curve {
        Curve::Step => {
            for pair in points.windows(2) {
                let ((_, y0), (x1, y1)) = (pair[0], pair[1]);
                out.push('H');
                num(out, x(x1), 1);
                if (y1 - y0).abs() > 1e-9 {
                    out.push('V');
                    num(out, y1, 1);
                }
            }
        }
        Curve::Monotone if points.len() >= 3 => {
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
                for (cx, cy) in [(x0 + h, y0 + t[i] * h), (x1 - h, y1 - t[i + 1] * h), (x1, y1)] {
                    num(out, x(cx), 1);
                    out.push(' ');
                    num(out, cy, 1);
                    out.push(' ');
                }
                out.pop();
            }
        }
        _ => {
            for (px, py) in &points[1..] {
                out.push('L');
                num(out, x(*px), 1);
                out.push(' ');
                num(out, *py, 1);
            }
        }
    }
}

/// A reference line from (`x1`, `y1`) to (`x2`, `y2`), in the units of the drawing it
/// sits in (`"12.5%"` or pixels), dashed unless `solid`, in `ink` or the muted ink, with
/// `label` written at `place` beside it. `rtl` mirrors where start and end are.
#[allow(clippy::too_many_arguments)]
pub(crate) fn reference_line(
    (x1, y1): (String, f64),
    (x2, y2): (String, f64),
    label: Option<&str>,
    place: Place,
    ink: Option<Ink>,
    solid: bool,
    rtl: bool,
) -> Element {
    let line = el("line")
        .attr("x1", x1.clone())
        .attr("y1", n(y1, 1))
        .attr("x2", x2.clone())
        .attr("y2", n(y2, 1));
    let text = label.map(|label| {
        let at_end = matches!(place, Place::EndAbove | Place::EndBelow) != rtl;
        let below = matches!(place, Place::EndBelow | Place::StartBelow);
        let (x, y) = if at_end { (x2.clone(), y2) } else { (x1.clone(), y1) };
        mark_text(label)
            .attr("x", x)
            .attr("y", n(if below { y + 13.0 } else { y - 4.0 }, 1))
            .attr("text-anchor", if at_end { "end" } else { "start" })
            .attr("dx", if at_end { "-2" } else { "2" })
    });
    let group = el("g").slot("chart-ref").attr_opt("data-solid", solid.then_some(""));
    let group = match ink {
        Some(ink) => ink.on(group),
        None => group,
    };
    group.children((line, text))
}

/// A band of the plot from pixel `top` to `bottom`, the full width, in `ink` or the
/// muted ink, with `label` inside its top edge at the start.
pub(crate) fn zone_rect(top: f64, bottom: f64, label: Option<&str>, ink: Option<Ink>, rtl: bool) -> Element {
    let (top, bottom) = (top.min(bottom), top.max(bottom));
    let group = el("g").slot("chart-zone");
    let group = match ink {
        Some(ink) => ink.on(group),
        None => Color::Other.on(group),
    };
    group.children((
        el("rect")
            .attr("x", "0")
            .attr("width", "100%")
            .attr("y", n(top, 1))
            .attr("height", n((bottom - top).max(0.0), 1)),
        label.map(|label| {
            mark_text(label)
                .attr("x", if rtl { "100%" } else { "0" })
                .attr("dx", if rtl { "-4" } else { "4" })
                .attr("y", n(top + 12.0, 1))
                .attr("text-anchor", if rtl { "end" } else { "start" })
        }),
    ))
}

/// A mark's words on the drawing: cut to 32 characters, the whole as its title.
pub(crate) fn mark_text(label: &str) -> Element {
    let cut = short(label, 32);
    let whole = (cut != label).then(|| el("title").child(label.to_owned()));
    el("text").children((whole, cut))
}

/// `text` cut to `most` characters, with an ellipsis.
pub(crate) fn short(text: &str, most: usize) -> String {
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
    use crate::render;

    #[test]
    fn steps_hold_each_value_until_the_next() {
        let mut d = String::new();
        curve_path(&mut d, &[(0.0, 30.0), (50.0, 10.0), (100.0, 10.0)], Curve::Step, false, 10.0);
        assert_eq!(d, "M0 30H500V10H1000");
        let mut one = String::new();
        curve_path(&mut one, &[(5.0, 5.0)], Curve::Step, true, 1.0);
        assert_eq!(one, "L5 5");
        let mut none = String::new();
        curve_path(&mut none, &[], Curve::Monotone, false, 1.0);
        assert!(none.is_empty());
    }

    #[test]
    fn monotone_curves_never_overshoot() {
        let mut d = String::new();
        curve_path(&mut d, &[(0.0, 10.0), (50.0, 0.0), (100.0, 0.0)], Curve::Monotone, false, 10.0);
        assert!(d.starts_with("M0 10C"), "{d}");
        assert!(d.ends_with("833.3 0 1000 0"), "{d}");
        let mut two = String::new();
        curve_path(&mut two, &[(0.0, 1.0), (100.0, 2.0)], Curve::Monotone, false, 1.0);
        assert_eq!(two, "M0 1L100 2");
    }

    #[test]
    fn a_reference_line_is_dashed_muted_and_labelled_where_asked() {
        let out = render(
            &reference_line(
                ("0".into(), 40.0),
                ("100%".into(), 40.0),
                Some("Avg: 4.2"),
                Place::EndAbove,
                None,
                false,
                false,
            )
            .into(),
        );
        assert!(out.starts_with(r#"<g data-slot="chart-ref"><line x1="0" y1="40" x2="100%" y2="40">"#), "{out}");
        assert!(out.contains(r#"x="100%" y="36" text-anchor="end""#), "{out}");
        let out = render(
            &reference_line(
                ("0".into(), 40.0),
                ("100%".into(), 40.0),
                Some("x"),
                Place::StartBelow,
                Some(Ink::slot(3)),
                true,
                false,
            )
            .into(),
        );
        assert!(out.contains(r#"data-solid="" data-color="3""#), "{out}");
        assert!(out.contains(r#"x="0" y="53" text-anchor="start""#), "{out}");
    }

    #[test]
    fn a_zone_is_a_band_with_its_label_inside() {
        let out = render(&zone_rect(80.0, 20.0, Some("Healthy"), None, false).into());
        assert!(out.contains(r#"data-color="other""#), "{out}");
        assert!(out.contains(r#"y="20" height="60""#), "{out}");
        assert!(out.contains(">Healthy</text>"), "{out}");
        let z = Zone::new(9, 3);
        assert_eq!((z.from, z.to), (3.0, 9.0));
    }

    #[test]
    fn long_names_are_cut() {
        assert_eq!(short("Northern Europe and the Baltics", 16), "Northern Europe…");
        assert_eq!(short("Short", 16), "Short");
        let out = render(&mark_text(&"x".repeat(40)).into());
        assert!(out.contains("<title>"), "{out}");
    }
}
