//! Radial charts: a value on a ring ([`Ring`]), two to four values on rings inside each
//! other ([`Rings`]), a value on an open arc ([`Gauge`]) and a day as 24 wedges round a
//! clock face ([`DayClock`]).
//!
//! **What they are made of.** Each is a chart like the others: a `<figure>` with its
//! title, a drawing, a text summary and the same numbers as a table. A ring, rings and a
//! gauge are pictures, named by the title and described by the summary, usually inside a
//! card that already has a heading, so the title is for assistive technology and the
//! table for screen readers unless asked otherwise. The day clock is interactive: one tab
//! stop, the arrow keys move from hour to hour, and the tooltip comes up beside the hour.
//!
//! **Drawn on the server, finished.** The arcs are paths in an SVG and the words in the
//! middle are HTML over it, sized by the ring itself, so a ring reads the same with no
//! script, to a screen reader, in a print and in an image. Motion ([`Motion`]) is only
//! a way in: arcs draw on, numbers count up, and nothing moves for a reader who asks
//! for less motion. Colours come from theme tokens ([`Ink`], [`Gradient`],
//! [`ColorScale`]), so light and dark are both right.
//!
//! **Sizes.** A ring has one size in pixels and may take another from a screen width
//! up ([`Screen`]): the arcs, their thickness and the words in the middle all scale
//! with it, and no style attribute is written.
//!
//! ```
//! use navree_ui::charts::{Gradient, Ink, Motion, Ring, Screen};
//!
//! let sleep = Ring::new("Sleep score", 82)
//!     .label("Sleep score")
//!     .sublabel("Great recovery")
//!     .fill(Gradient::sheen(Ink::slot(7)))
//!     .glow()
//!     .size(160)
//!     .size_on(Screen::Md, 200)
//!     .motion(Motion::new().delay(200));
//! let html = navree_ui::render(&sleep.into());
//! assert!(html.contains("<svg") && html.contains("<table"));
//! ```
//!
//! [`Motion`]: super::Motion
//! [`Ink`]: super::Ink
//! [`Gradient`]: super::Gradient
//! [`ColorScale`]: super::ColorScale

use std::borrow::Cow;

use super::motion::{CountUp, Effect, Motion};
use super::paint::{ColorScale, Defs, Gradient, Ink, glow};
use super::{Common, Drawn, TableMode, fill, frame, n, svg};
use crate::icons::Icon;
use crate::node::{Element, Node, Uses, el};

mod clock;
mod gauge;
mod ring;

pub use clock::{DayClock, Moment};
pub use gauge::Gauge;
pub use ring::{Ring, RingValue, Rings};

/// A screen width from which a radial chart takes another size: [`Ring::size_on`].
/// The widths are the toolkit's: 640, 768 and 1024 pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Screen {
    /// 640 pixels and wider: a large phone on its side, a small tablet.
    Sm,
    /// 768 pixels and wider: a tablet, a narrow window.
    Md,
    /// 1024 pixels and wider: a laptop.
    Lg,
}

impl Screen {
    fn name(self) -> &'static str {
        match self {
            Self::Sm => "sm",
            Self::Md => "md",
            Self::Lg => "lg",
        }
    }
}

/// What the track under a ring's arc is drawn in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum RingTrack {
    /// A faint ring of the text ink: the same quiet grey under every colour, in light
    /// and dark.
    #[default]
    Theme,
    /// A faint ring of the arc's own colour.
    Tinted,
    /// No track: the arc alone.
    None,
}

/// The words a radial chart writes, in English by default. Templates name their blanks
/// in braces. Change a field of [`RadialLabels::english`] and give it to the chart's
/// `radial_labels`; the words every chart shares (Value, Share, No data) are its
/// [`ChartLabels`](super::ChartLabels).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RadialLabels {
    /// A column of goals or of the top of a scale.
    pub goal: Cow<'static, str>,
    /// A column of the band a value is in, from a [`ColorScale`].
    pub status: Cow<'static, str>,
    /// A day clock's column of moments.
    pub moments: Cow<'static, str>,
    /// One value: `{name}`, `{value}`, `{max}`, `{share}`.
    pub summary_ring: Cow<'static, str>,
    /// One value on a scale that does not start at zero: `{name}`, `{value}`, `{min}`,
    /// `{max}`.
    pub summary_range: Cow<'static, str>,
    /// After the value, the band it is in: `{band}`.
    pub summary_band: Cow<'static, str>,
    /// A day clock: `{total}`, `{peak}`, `{peak_x}`.
    pub summary_clock: Cow<'static, str>,
    /// A day clock's moments: `{count}`, `{names}`.
    pub summary_moments: Cow<'static, str>,
    /// The name of the hour that is now: `{time}`.
    pub now: Cow<'static, str>,
}

impl Default for RadialLabels {
    fn default() -> Self {
        Self::english()
    }
}

impl RadialLabels {
    /// The English words.
    pub const fn english() -> Self {
        Self {
            goal: Cow::Borrowed("Goal"),
            status: Cow::Borrowed("Status"),
            moments: Cow::Borrowed("Moments"),
            summary_ring: Cow::Borrowed("{name}: {value} of {max}, {share}."),
            summary_range: Cow::Borrowed("{name}: {value}, on a scale from {min} to {max}."),
            summary_band: Cow::Borrowed("{band}."),
            summary_clock: Cow::Borrowed("{total} in the day. Busiest at {peak_x}: {peak}."),
            summary_moments: Cow::Borrowed("{count} moments: {names}."),
            now: Cow::Borrowed("{time}, now"),
        }
    }
}

// ---- Geometry -------------------------------------------------------------------------

/// The point at `deg` degrees clockwise from twelve o'clock, `r` from `(cx, cy)`.
pub(crate) fn polar(cx: f64, cy: f64, r: f64, deg: f64) -> (f64, f64) {
    let (sin, cos) = deg.to_radians().sin_cos();
    (cx + r * sin, cy - r * cos)
}

/// An open arc for a stroke: radius `r` round `(cx, cy)`, from `from` degrees clockwise
/// from twelve o'clock, sweeping `sweep` degrees clockwise (at most a whole turn).
/// `None` when it sweeps nothing. A sweep past half a turn is two arcs, so a sweep a
/// hair short of a whole turn still draws (one arc between two points that round to the
/// same point draws nothing).
pub(crate) fn arc(cx: f64, cy: f64, r: f64, from: f64, sweep: f64) -> Option<String> {
    if !(sweep.is_finite() && from.is_finite() && r.is_finite()) || sweep <= 1e-6 || r <= 0.0 {
        return None;
    }
    let sweep = sweep.min(360.0);
    let mut d = String::with_capacity(64);
    let (x0, y0) = polar(cx, cy, r, from);
    d.push('M');
    point(&mut d, x0, y0);
    let halves = if sweep > 180.0 { 2 } else { 1 };
    let step = sweep / f64::from(halves);
    for k in 1..=halves {
        let (x, y) = polar(cx, cy, r, from + step * f64::from(k));
        d.push('A');
        d.push_str(&n(r, 2));
        d.push(' ');
        d.push_str(&n(r, 2));
        d.push_str(" 0 0 1 ");
        point(&mut d, x, y);
    }
    Some(d)
}

/// A wedge of a ring between radii `inner` and `outer`, from `from` degrees sweeping
/// `sweep` (less than half a turn), clockwise from twelve o'clock.
pub(crate) fn wedge(cx: f64, cy: f64, inner: f64, outer: f64, from: f64, sweep: f64) -> String {
    let sweep = sweep.clamp(0.0, 180.0);
    let to = from + sweep;
    let (x0, y0) = polar(cx, cy, outer, from);
    let (x1, y1) = polar(cx, cy, outer, to);
    let (x2, y2) = polar(cx, cy, inner, to);
    let (x3, y3) = polar(cx, cy, inner, from);
    let mut d = String::with_capacity(96);
    d.push('M');
    point(&mut d, x0, y0);
    d.push('A');
    d.push_str(&format!("{} {} 0 0 1 ", n(outer, 2), n(outer, 2)));
    point(&mut d, x1, y1);
    d.push('L');
    point(&mut d, x2, y2);
    d.push('A');
    d.push_str(&format!("{} {} 0 0 0 ", n(inner, 2), n(inner, 2)));
    point(&mut d, x3, y3);
    d.push('Z');
    d
}

fn point(d: &mut String, x: f64, y: f64) {
    super::num(d, x, 2);
    d.push(' ');
    super::num(d, y, 2);
}

// ---- Sizes ----------------------------------------------------------------------------

/// A drawing's size in pixels, and the sizes it takes from wider screens up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sizes {
    pub base: u32,
    pub at: Vec<(Screen, u32)>,
}

impl Sizes {
    pub fn new(base: u32) -> Self {
        Self {
            base,
            at: Vec::new(),
        }
    }

    /// Takes `px` from `screen` up, replacing what that screen had.
    pub fn set(&mut self, screen: Screen, px: u32) {
        self.at.retain(|(s, _)| *s != screen);
        self.at.push((screen, px));
        self.at.sort_by_key(|(s, _)| *s);
    }

    /// The pictures that give the drawing its size: one per width, each shown from its
    /// screen up to the next one's, so the page needs no style attribute. `height` is a
    /// drawing's height for its width.
    pub fn sizers(&self, height: impl Fn(u32) -> u32) -> Vec<Node> {
        let mut out = Vec::with_capacity(1 + self.at.len());
        let all: Vec<(Option<Screen>, u32)> = std::iter::once((None, self.base))
            .chain(self.at.iter().map(|(s, px)| (Some(*s), *px)))
            .collect();
        for (k, (screen, px)) in all.iter().enumerate() {
            let h = height(*px).max(1);
            let next = all.get(k + 1).and_then(|(s, _)| *s);
            out.push(
                svg()
                    .slot("radial-size")
                    .attr_opt("data-at", screen.map(Screen::name))
                    .attr_opt("data-to", next.map(Screen::name))
                    .attr("width", i64::from(*px))
                    .attr("height", i64::from(h))
                    .attr("viewBox", format!("0 0 {px} {h}"))
                    .into(),
            );
        }
        out
    }
}

/// A ring's thickness at `size` pixels when none is set: thicker for its size when
/// small, so a small ring still reads.
pub(crate) fn thickness_for(size: f64) -> f64 {
    ((size * 0.065 + 2.5) * 2.0).round() / 2.0
}

// ---- Paint ----------------------------------------------------------------------------

/// How a ring's arc is painted: an ink, a gradient along it, or the ink of the band its
/// value is in.
#[derive(Clone, Debug)]
pub(crate) struct Paint {
    /// The ink asked for, if any.
    pub ink: Option<Ink>,
    pub fill: Option<Gradient>,
    pub scale: Option<ColorScale>,
    pub track: RingTrack,
    pub glow: bool,
}

impl Paint {
    pub fn new(ink: Option<Ink>) -> Self {
        Self {
            ink,
            fill: None,
            scale: None,
            track: RingTrack::Theme,
            glow: false,
        }
    }

    /// The ink of a value at `share` percent of the way: its band's, the one asked for,
    /// the gradient's first colour, or chart colour 1. The glow and the words in the
    /// middle wear it.
    pub fn ink_at(&self, share: f64) -> Ink {
        match &self.scale {
            Some(scale) => scale.ink_of(share),
            None => self
                .ink
                .or_else(|| self.fill.as_ref().and_then(Gradient::lead))
                .unwrap_or(Ink::slot(1)),
        }
    }

    /// The label of the band a value at `share` percent is in.
    pub fn band_at(&self, share: f64) -> Option<String> {
        self.scale
            .as_ref()
            .and_then(|s| s.band_of(share).1.map(str::to_owned))
    }
}

/// One ring drawn: the track, the arc to the value, and a second lap past the whole.
pub(crate) struct Band<'a> {
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
    pub thickness: f64,
    /// Where the scale starts, in degrees clockwise from twelve.
    pub from: f64,
    /// How far the whole scale sweeps: 360 for a ring.
    pub sweep: f64,
    /// How much of the scale the value covers: 1 is all of it.
    pub share: f64,
    /// Whether a value past the whole draws a second lap (a ring), or stops at the end.
    pub laps: bool,
    pub ink: Ink,
    pub paint: &'a Paint,
    pub motion: Motion,
    pub index: usize,
    /// The drawing's box, which a gradient spans.
    pub area: [f64; 4],
}

impl Band<'_> {
    /// The track, the arc and the lap.
    pub fn nodes(&self, defs: &mut Defs) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::with_capacity(3);
        let width = n(self.thickness, 2);
        if self.paint.track != RingTrack::None {
            let track = if self.sweep >= 360.0 {
                el("circle")
                    .attr("cx", n(self.cx, 2))
                    .attr("cy", n(self.cy, 2))
                    .attr("r", n(self.r, 2))
            } else {
                el("path").attr(
                    "d",
                    arc(self.cx, self.cy, self.r, self.from, self.sweep).unwrap_or_default(),
                )
            };
            let track = track
                .slot("radial-track")
                .attr("stroke-width", width.clone());
            out.push(match self.paint.track {
                RingTrack::Tinted => self.ink.on(track.attr("data-track", "tint")).into(),
                _ => track.into(),
            });
        }
        let share = if self.share.is_finite() {
            self.share.max(0.0)
        } else {
            0.0
        };
        let first = share.min(1.0);
        let url =
            self.paint.fill.as_ref().map(|g| {
                defs.gradient(&g.clone().across_plot(), self.ink.color(), Some(self.area))
            });
        if let Some(d) = arc(self.cx, self.cy, self.r, self.from, first * self.sweep) {
            let path = self
                .ink
                .on(el("path").slot("radial-arc"))
                .attr("d", d)
                .attr("pathLength", 1_i64)
                .attr("stroke-width", width.clone())
                .attr_opt("stroke", url.clone());
            let path = if self.paint.glow { glow(path) } else { path };
            out.push(self.motion.apply(path, Effect::Draw, self.index).into());
        }
        if self.laps && share > 1.0 {
            // A second lap over the first, brighter, with a shadow of the surface where it
            // ends: past the goal.
            let over = (share - 1.0).min(1.0);
            if let Some(d) = arc(self.cx, self.cy, self.r, self.from, over * self.sweep) {
                let lap = self
                    .ink
                    .on(el("path").slot("radial-lap"))
                    .attr("d", d)
                    .attr("pathLength", 1_i64)
                    .attr("stroke-width", width);
                out.push(
                    self.motion
                        .after(700)
                        .apply(lap, Effect::Draw, self.index)
                        .into(),
                );
            }
        }
        out
    }
}

// ---- The middle -----------------------------------------------------------------------

/// What the middle of a ring shows.
#[derive(Clone, Debug, Default)]
pub(crate) enum Middle {
    /// The value, with its unit, label and sublabel.
    #[default]
    Value,
    /// Markup of the page's own.
    Custom(Node),
    /// An icon.
    Icon(Icon),
    /// Nothing.
    Nothing,
}

/// `text` followed by `unit`: a word with a space before it, a sign such as `%` without.
pub(crate) fn with_unit(text: &str, unit: Option<&str>) -> String {
    match unit {
        Some(unit) if !unit.is_empty() => {
            if unit.starts_with(char::is_alphanumeric) {
                format!("{text} {unit}")
            } else {
                format!("{text}{unit}")
            }
        }
        _ => text.to_owned(),
    }
}

/// How much a figure must shrink to fit inside its ring, 1 to 4, by its length.
pub(crate) fn fit(text: &str, unit: Option<&str>) -> i64 {
    let chars = text.chars().count() + unit.map_or(0, |u| u.chars().count().div_ceil(2));
    match chars {
        0..=3 => 1,
        4 => 2,
        5..=6 => 3,
        _ => 4,
    }
}

/// A figure in the middle: the number (counting up when the motion is on), its unit
/// smaller beside it.
pub(crate) fn figure(value: f64, text: &str, unit: Option<&str>, motion: Motion) -> Element {
    let unit = unit.filter(|u| !u.is_empty());
    let gap = unit.is_some_and(|u| u.starts_with(char::is_alphanumeric));
    el("span").slot("radial-value").children((
        CountUp::new(value, text.to_owned()).motion(motion.after(250)),
        unit.map(|u| {
            el("span")
                .slot("radial-unit")
                .attr_opt("data-gap", gap.then_some(""))
                .child(u.to_owned())
        }),
    ))
}

/// An icon for the middle of a ring or a marker on a clock, as its own small SVG.
pub(crate) fn glyph(icon: Icon) -> Element {
    svg()
        .slot("radial-icon")
        .attr("viewBox", icon.view_box())
        .child(Node::Static(icon.shapes()))
}

/// The wrapper every radial drawing is in: its sizers, its plot and its middle. It links
/// the radial sheet and the paint sheet, which colours the marks.
pub(crate) fn wrap(
    kind: &'static str,
    sizers: Vec<Node>,
    plot: Element,
    middle: Option<Element>,
) -> Element {
    el("div")
        .slot("radial")
        .attr("data-kind", kind)
        .uses(Uses::CHART_RADIAL)
        .uses(Uses::CHART_PAINT)
        .children((sizers, plot, middle))
}

/// The SVG of a drawing `w` by `h` in its own units, stretched over the wrapper.
pub(crate) fn plot(w: f64, h: f64) -> Element {
    svg()
        .slot("radial-plot")
        .attr("viewBox", format!("0 0 {} {}", n(w, 2), n(h, 2)))
}

/// The figure for a ring, rings or a gauge: the frame every chart has, linking the
/// charts sheet only when it shows what that sheet styles (the title, the table's
/// switch, a state), or the drawing alone in a span for a pill or a row.
pub(crate) fn finish(
    common: Common,
    drawn: Option<Drawn>,
    kind: &'static str,
    height: u32,
    inline: bool,
) -> Node {
    if inline {
        let root = el("span")
            .slot("radial-inline")
            .uses(Uses::CHART_RADIAL)
            .attr("role", "img");
        return match drawn {
            Some(drawn) => root
                .attr("aria-label", common.summary.unwrap_or(drawn.summary))
                .child(drawn.body)
                .into(),
            None => root
                .flag("data-empty", true)
                .attr(
                    "aria-label",
                    format!("{}: {}", common.title, common.labels.empty),
                )
                .child("\u{2013}")
                .into(),
        };
    }
    let plain = common.hide_title && common.table != TableMode::Switch;
    let node = frame(common, drawn, kind, height);
    if plain {
        return node;
    }
    // The frame is always a figure.
    node.into_element().map_or_else(
        || Node::list(Vec::<Node>::new()),
        |figure| figure.uses(Uses::CHARTS).into(),
    )
}

/// The summary of one value: `{name}: {value} of {max}, {share}.`, and its band.
pub(crate) fn summary_of(
    words: &RadialLabels,
    name: &str,
    value: &str,
    min: Option<&str>,
    max: &str,
    share: &str,
    band: Option<&str>,
) -> String {
    let mut out = match min {
        Some(min) => fill(
            &words.summary_range,
            &[("name", name), ("value", value), ("min", min), ("max", max)],
        ),
        None => fill(
            &words.summary_ring,
            &[
                ("name", name),
                ("value", value),
                ("max", max),
                ("share", share),
            ],
        ),
    };
    if let Some(band) = band {
        out.push(' ');
        out.push_str(&fill(&words.summary_band, &[("band", band)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    #[test]
    fn arcs_run_clockwise_from_twelve() {
        // A quarter, a half, a whole turn round (50, 50) at radius 40.
        assert_eq!(
            arc(50.0, 50.0, 40.0, 0.0, 90.0).as_deref(),
            Some("M50 10A40 40 0 0 1 90 50")
        );
        assert_eq!(
            arc(50.0, 50.0, 40.0, 0.0, 180.0).as_deref(),
            Some("M50 10A40 40 0 0 1 50 90")
        );
        assert_eq!(
            arc(50.0, 50.0, 40.0, 0.0, 360.0).as_deref(),
            Some("M50 10A40 40 0 0 1 50 90A40 40 0 0 1 50 10")
        );
        // Past a whole turn is a whole turn; nothing, or a turn that is not a number, is no arc.
        assert_eq!(
            arc(50.0, 50.0, 40.0, 0.0, 500.0),
            arc(50.0, 50.0, 40.0, 0.0, 360.0)
        );
        assert_eq!(arc(50.0, 50.0, 40.0, 0.0, 0.0), None);
        assert_eq!(arc(50.0, 50.0, 40.0, 0.0, -10.0), None);
        assert_eq!(arc(50.0, 50.0, 40.0, 0.0, f64::NAN), None);
        assert_eq!(arc(50.0, 50.0, 0.0, 0.0, 90.0), None);
    }

    #[test]
    fn an_arc_a_hair_short_of_a_turn_still_draws() {
        let d = arc(50.0, 50.0, 40.0, 0.0, 359.999).unwrap_or_default();
        assert_eq!(d.matches('A').count(), 2, "{d}");
        // Its two halves end at different points.
        assert!(d.ends_with("49.99 10") || d.ends_with("50 10"), "{d}");
        assert!(!d.contains("NaN"));
    }

    #[test]
    fn a_gauge_arc_starts_at_its_own_angle() {
        // 270 degrees from -135: from the lower left, over the top, to the lower right.
        let d = arc(100.0, 100.0, 80.0, -135.0, 270.0).unwrap_or_default();
        assert!(d.starts_with("M43.43 156.57A80 80 0 0 1 "), "{d}");
        assert!(d.ends_with("156.57 156.57"), "{d}");
    }

    #[test]
    fn a_wedge_closes_between_its_radii() {
        let d = wedge(180.0, 180.0, 60.0, 100.0, 0.0, 90.0);
        assert_eq!(
            d,
            "M180 80A100 100 0 0 1 280 180L240 180A60 60 0 0 0 180 120Z"
        );
    }

    #[test]
    fn sizers_hand_over_from_one_screen_to_the_next() {
        let mut sizes = Sizes::new(120);
        sizes.set(Screen::Lg, 200);
        sizes.set(Screen::Md, 160);
        sizes.set(Screen::Md, 168);
        let out: Vec<String> = sizes.sizers(|w| w).iter().map(render).collect();
        assert_eq!(out.len(), 3);
        assert!(
            out[0].contains(r#"data-to="md" width="120" height="120""#),
            "{}",
            out[0]
        );
        assert!(
            out[1].contains(r#"data-at="md" data-to="lg" width="168""#),
            "{}",
            out[1]
        );
        assert!(out[2].contains(r#"data-at="lg" width="200""#), "{}", out[2]);
        assert!(!out[2].contains("data-to"));
    }

    #[test]
    fn units_and_fit() {
        assert_eq!(with_unit("82", Some("%")), "82%");
        assert_eq!(with_unit("540", Some("kcal")), "540 kcal");
        assert_eq!(with_unit("36.6", Some("°C")), "36.6°C");
        assert_eq!(with_unit("7", None), "7");
        assert_eq!(fit("82", Some("%")), 1);
        assert_eq!(fit("1,234", None), 3);
        assert_eq!(fit("1,23,456", None), 4);
        assert_eq!(thickness_for(160.0), 13.0);
        assert_eq!(thickness_for(56.0), 6.0);
    }

    #[test]
    fn the_middle_counts_up_only_with_motion() {
        let still = render(&figure(82.0, "82", Some("%"), Motion::none()).into());
        assert_eq!(
            still,
            r#"<span data-slot="radial-value"><span data-slot="count-up">82</span><span data-slot="radial-unit">%</span></span>"#
        );
        let moving = render(&figure(540.0, "540", Some("kcal"), Motion::new()).into());
        assert!(moving.contains(r#"data-to="540""#), "{moving}");
        assert!(moving.contains(r#"data-gap="""#), "{moving}");
    }
}
