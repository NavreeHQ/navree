//! Paint for chart marks: a colour from the theme ([`Ink`]), gradients ([`Gradient`]),
//! colour by value ([`ColorScale`]) and glow.
//!
//! **Nothing here writes a colour.** A mark or a gradient stop carries a `data-color`
//! (a categorical slot, `accent` or `other`) or a `data-tone`, and the stylesheets turn
//! it into `--c`, a theme token. Light and dark come from the tokens, so a gradient is
//! right in both, and in every theme. A stop can be made [vivid](Ink::vivid) (mixed
//! toward the text ink) or [soft](Ink::soft) (mixed toward the surface): both are
//! computed by the stylesheet from tokens.
//!
//! **How a gradient reaches a mark.** Inside the crate, a chart collects its gradients
//! in a [`Defs`], which names each one after the chart's id (`{id}-p0`, `{id}-p1`, ...),
//! so two charts on a page never share a gradient. The mark gets `fill="url(#...)"` or
//! `stroke="url(#...)"`; the stylesheets leave a mark that has a `fill` or `stroke`
//! attribute alone. A [`Defs`] with anything in it links [`Uses::CHART_PAINT`], the
//! sheet that colours the stops.
//!
//! ```
//! use navree_ui::charts::{Gradient, Ink};
//! use navree_ui::Tone;
//!
//! // An area's wash: its own colour at 40%, fading to nothing at the bottom.
//! let wash = Gradient::fade(Ink::own());
//! // A ring's stroke: the second chart colour, turning brighter along the diagonal.
//! let sheen = Gradient::sheen(Ink::slot(2));
//! // Four stops, one per level of a stepped chart, across the whole plot.
//! let levels = Gradient::vertical()
//!     .stop_with_opacity(0.0, Ink::tone(Tone::Danger), 0.3)
//!     .stop_with_opacity(0.33, Ink::slot(7), 0.4)
//!     .stop_with_opacity(0.66, Ink::slot(1), 0.4)
//!     .stop_with_opacity(1.0, Ink::slot(7).vivid(), 0.5)
//!     .across_plot();
//! # let _ = (wash, sheen, levels);
//! ```

use super::{Color, n};
use crate::components::Tone;
use crate::node::{Element, Node, Uses, el};

/// A colour from the theme for a chart mark or a gradient stop: one of the eight
/// categorical chart colours, a tone, the muted ink, the accent, or the mark's own
/// colour. It is written as a `data-color` or `data-tone` attribute and the stylesheet
/// picks the token, so it follows light and dark and the theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub struct Ink {
    base: Base,
    mix: Mix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base {
    Slot(u8),
    Tone(Tone),
    Muted,
    Accent,
    Own,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mix {
    #[default]
    None,
    Vivid,
    Soft,
}

impl Ink {
    /// Categorical chart colour `slot`, 1 to 8 (`--chart-1` .. `--chart-8`).
    pub const fn slot(slot: u8) -> Self {
        let slot = if slot < 1 {
            1
        } else if slot > 8 {
            8
        } else {
            slot
        };
        Self {
            base: Base::Slot(slot),
            mix: Mix::None,
        }
    }

    /// A tone's colour: [`Tone::Success`], [`Tone::Warning`], a named tone. Neutral is
    /// the muted ink.
    pub const fn tone(tone: Tone) -> Self {
        Self {
            base: Base::Tone(tone),
            mix: Mix::None,
        }
    }

    /// The quiet ink a chart uses for what is not the subject.
    pub const fn muted() -> Self {
        Self {
            base: Base::Muted,
            mix: Mix::None,
        }
    }

    /// The theme's accent.
    pub const fn accent() -> Self {
        Self {
            base: Base::Accent,
            mix: Mix::None,
        }
    }

    /// The colour of the mark being painted: a series' own colour in a gradient given to
    /// that series. Anywhere it has no mark to follow, it is chart colour 1.
    pub const fn own() -> Self {
        Self {
            base: Base::Own,
            mix: Mix::None,
        }
    }

    /// The colour mixed a third of the way toward the text ink: brighter on a dark
    /// page, deeper on a light one, so it stands out more in both. The second stop of a
    /// two-stop sheen.
    pub const fn vivid(self) -> Self {
        Self {
            base: self.base,
            mix: Mix::Vivid,
        }
    }

    /// The colour mixed most of the way toward the surface: a soft version for a
    /// track, a halo or the faint end of a fade.
    pub const fn soft(self) -> Self {
        Self {
            base: self.base,
            mix: Mix::Soft,
        }
    }

    /// The same ink, its [`own`](Self::own) colour replaced by `color`.
    pub(crate) fn resolve(self, color: Color) -> Self {
        match self.base {
            Base::Own => Self {
                base: match color {
                    Color::Slot(s) => Base::Slot(s),
                    Color::Tone(t) => Base::Tone(t),
                    Color::Other => Base::Muted,
                    Color::Accent => Base::Accent,
                },
                mix: self.mix,
            },
            _ => self,
        }
    }

    /// The colour a chart's marks and legend take for this ink. An own colour with no
    /// mark to follow is chart colour 1.
    pub(crate) fn color(self) -> Color {
        match self.base {
            Base::Slot(s) => Color::Slot(s),
            Base::Tone(t) => Color::Tone(t),
            Base::Muted => Color::Other,
            Base::Accent => Color::Accent,
            Base::Own => Color::Slot(1),
        }
    }

    /// Sets the attributes the stylesheets colour `element` by: `data-color` or
    /// `data-tone`, and `data-mix` for a vivid or soft ink.
    pub(crate) fn on(self, element: Element) -> Element {
        let element = self.color().on(element);
        match self.mix {
            Mix::None => element,
            Mix::Vivid => element.attr("data-mix", "vivid"),
            Mix::Soft => element.attr("data-mix", "soft"),
        }
    }
}

impl From<Tone> for Ink {
    fn from(tone: Tone) -> Self {
        Self::tone(tone)
    }
}

/// A gradient: linear in a direction, or radial from the centre, through stops of
/// [`Ink`] at an opacity each. It spans the mark it paints (its box), or the whole plot
/// with [`across_plot`](Self::across_plot), so stops can line up with the levels of the
/// value axis.
///
/// Opacities are numbers, not colours, so a fade to nothing needs no colour of its own.
#[derive(Clone, Debug, PartialEq)]
#[must_use]
pub struct Gradient {
    shape: Shape,
    stops: Vec<GradientStop>,
    across: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    /// From (x1, y1) to (x2, y2), as fractions of the box.
    Linear(f64, f64, f64, f64),
    Radial,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct GradientStop {
    offset: f64,
    ink: Ink,
    opacity: f64,
}

impl Gradient {
    fn new(shape: Shape) -> Self {
        Self {
            shape,
            stops: Vec::new(),
            across: false,
        }
    }

    /// From the top down.
    pub fn vertical() -> Self {
        Self::new(Shape::Linear(0.0, 0.0, 0.0, 1.0))
    }

    /// From the left across.
    pub fn horizontal() -> Self {
        Self::new(Shape::Linear(0.0, 0.0, 1.0, 0.0))
    }

    /// From the top left to the bottom right.
    pub fn diagonal() -> Self {
        Self::new(Shape::Linear(0.0, 0.0, 1.0, 1.0))
    }

    /// Along `degrees`, clockwise from pointing right: 0 runs left to right, 90 top to
    /// bottom, 180 right to left.
    pub fn angle(degrees: f64) -> Self {
        let d = if degrees.is_finite() { degrees } else { 0.0 };
        let (sin, cos) = d.to_radians().sin_cos();
        Self::new(Shape::Linear(
            0.5 - cos / 2.0,
            0.5 - sin / 2.0,
            0.5 + cos / 2.0,
            0.5 + sin / 2.0,
        ))
    }

    /// From the centre out.
    pub fn radial() -> Self {
        Self::new(Shape::Radial)
    }

    /// Adds a stop of `ink` at `offset` (0 to 1), fully opaque.
    pub fn stop(self, offset: f64, ink: impl Into<Ink>) -> Self {
        self.stop_with_opacity(offset, ink, 1.0)
    }

    /// Adds a stop of `ink` at `offset` (0 to 1) and `opacity` (0 to 1).
    pub fn stop_with_opacity(mut self, offset: f64, ink: impl Into<Ink>, opacity: f64) -> Self {
        let clean = |v: f64| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        self.stops.push(GradientStop {
            offset: clean(offset),
            ink: ink.into(),
            opacity: clean(opacity),
        });
        self
    }

    /// An area's wash: `ink` at 40% at the top, nothing at the bottom.
    pub fn fade(ink: impl Into<Ink>) -> Self {
        let ink = ink.into();
        Self::vertical()
            .stop_with_opacity(0.0, ink, 0.4)
            .stop_with_opacity(1.0, ink, 0.0)
    }

    /// A column's paint: `ink` at the top, half as strong at the bottom.
    pub fn column(ink: impl Into<Ink>) -> Self {
        let ink = ink.into();
        Self::vertical()
            .stop(0.0, ink)
            .stop_with_opacity(1.0, ink, 0.5)
    }

    /// A ring's or a bar's sheen: `ink` turning [vivid](Ink::vivid) along the diagonal.
    pub fn sheen(ink: impl Into<Ink>) -> Self {
        let ink = ink.into();
        Self::diagonal().stop(0.0, ink).stop(1.0, ink.vivid())
    }

    /// Spans the plot (or the whole drawing) rather than the box of each mark: a
    /// vertical gradient's stops then sit at fixed levels of the value axis, and a
    /// horizontal one runs the full width whatever part a line covers.
    pub fn across_plot(mut self) -> Self {
        self.across = true;
        self
    }

    /// The gradient's element, called `id`, its own colour stops resolved to `color`.
    /// `area` is the drawing's box in user units (x, y, width, height): used when the
    /// gradient spans the plot, and always for a stroke, whose box can have no height.
    pub(crate) fn element(&self, id: &str, color: Color, area: Option<[f64; 4]>) -> Element {
        let mut stops = self.stops.clone();
        if stops.is_empty() {
            stops.push(GradientStop {
                offset: 0.0,
                ink: Ink::own(),
                opacity: 1.0,
            });
        }
        stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        let children: Vec<Node> = stops
            .iter()
            .map(|s| {
                let stop = el("stop").attr("offset", n(s.offset, 3));
                let stop = if s.opacity < 1.0 {
                    stop.attr("stop-opacity", n(s.opacity, 3))
                } else {
                    stop
                };
                s.ink.resolve(color).on(stop).into()
            })
            .collect();
        let gradient = match self.shape {
            Shape::Linear(x1, y1, x2, y2) => {
                let g = el("linearGradient").id(id.to_owned());
                match area.filter(|_| self.across) {
                    Some([x, y, w, h]) => g
                        .attr("gradientUnits", "userSpaceOnUse")
                        .attr("x1", n(x + x1 * w, 2))
                        .attr("y1", n(y + y1 * h, 2))
                        .attr("x2", n(x + x2 * w, 2))
                        .attr("y2", n(y + y2 * h, 2)),
                    None => g
                        .attr("x1", n(x1, 3))
                        .attr("y1", n(y1, 3))
                        .attr("x2", n(x2, 3))
                        .attr("y2", n(y2, 3)),
                }
            }
            Shape::Radial => {
                let g = el("radialGradient").id(id.to_owned());
                match area.filter(|_| self.across) {
                    Some([x, y, w, h]) => g
                        .attr("gradientUnits", "userSpaceOnUse")
                        .attr("cx", n(x + w / 2.0, 2))
                        .attr("cy", n(y + h / 2.0, 2))
                        .attr("r", n(w.max(h) / 2.0, 2)),
                    None => g,
                }
            }
        };
        gradient.child(children)
    }
}

/// Colour by value: an [`Ink`] for values below every threshold, and an ink for values
/// at or above each threshold. A bar, a column or a progress bar takes the colour of
/// its value; the bands' labels make a legend, so the colour is never alone.
///
/// ```
/// use navree_ui::charts::{ColorScale, Ink};
/// use navree_ui::Tone;
///
/// // Below 50 is low, 50 to 75 fair, 75 and over good.
/// let recovery = ColorScale::new(Ink::tone(Tone::Danger))
///     .label("Low")
///     .from(50, Ink::tone(Tone::Warning))
///     .label("Fair")
///     .from(75, Ink::tone(Tone::Success))
///     .label("Good");
/// assert_eq!(recovery.ink_of(80.0), Ink::tone(Tone::Success));
/// ```
#[derive(Clone, Debug, PartialEq)]
#[must_use]
pub struct ColorScale {
    first: (Ink, Option<String>),
    steps: Vec<(f64, Ink, Option<String>)>,
}

impl ColorScale {
    /// `ink` for every value, until a threshold says otherwise.
    pub fn new(ink: impl Into<Ink>) -> Self {
        Self {
            first: (ink.into(), None),
            steps: Vec::new(),
        }
    }

    /// Values at or above `at` take `ink`. Thresholds may come in any order; a value
    /// takes the band of the highest threshold it reaches.
    pub fn from(mut self, at: impl super::IntoValue, ink: impl Into<Ink>) -> Self {
        if let Some(at) = at.into_value() {
            self.steps.push((at, ink.into(), None));
        }
        self
    }

    /// Names the band added last (or the first band, before any threshold): the
    /// legend's words for its colour.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        match self.steps.last_mut() {
            Some(step) => step.2 = Some(label.into()),
            None => self.first.1 = Some(label.into()),
        }
        self
    }

    /// The ink of `value`.
    pub fn ink_of(&self, value: f64) -> Ink {
        self.band_of(value).0
    }

    /// The ink and label of `value`'s band.
    pub(crate) fn band_of(&self, value: f64) -> (Ink, Option<&str>) {
        let mut out = (self.first.0, self.first.1.as_deref());
        let mut best = f64::NEG_INFINITY;
        for (at, ink, label) in &self.steps {
            if value >= *at && *at >= best {
                best = *at;
                out = (*ink, label.as_deref());
            }
        }
        out
    }

    /// The bands with a label, lowest first: a legend.
    pub(crate) fn labelled(&self) -> Vec<(Ink, String)> {
        let mut steps: Vec<&(f64, Ink, Option<String>)> = self.steps.iter().collect();
        steps.sort_by(|a, b| a.0.total_cmp(&b.0));
        std::iter::once((&self.first.0, &self.first.1))
            .chain(steps.into_iter().map(|(_, ink, label)| (ink, label)))
            .filter_map(|(ink, label)| label.clone().map(|l| (*ink, l)))
            .collect()
    }
}

/// The gradients and other definitions of one drawing, each named after the drawing's
/// id so two drawings on a page never share one.
///
/// ```ignore
/// let mut defs = Defs::new(&chart_id);
/// let fill = defs.gradient(&Gradient::fade(Ink::own()), series_color, Some(plot_box));
/// let area = el("path").attr("fill", fill);
/// let svg = svg().children((defs.node(), area));
/// ```
pub(crate) struct Defs {
    prefix: String,
    items: Vec<Node>,
}

impl Defs {
    /// Definitions for the drawing whose id is `id`.
    pub fn new(id: &str) -> Self {
        Self {
            prefix: id.to_owned(),
            items: Vec::new(),
        }
    }

    /// Adds `gradient`, its own colour stops in `color`, and returns the paint that
    /// points at it: `url(#{id}-p{n})`. `area` is the drawing's box, for a gradient that
    /// spans the plot or paints a stroke.
    pub fn gradient(
        &mut self,
        gradient: &Gradient,
        color: Color,
        area: Option<[f64; 4]>,
    ) -> String {
        let id = format!("{}-p{}", self.prefix, self.items.len());
        self.items.push(gradient.element(&id, color, area).into());
        format!("url(#{id})")
    }

    /// The `<defs>` element, linking the paint sheet, or nothing when empty.
    pub fn node(self) -> Option<Element> {
        (!self.items.is_empty()).then(|| el("defs").uses(Uses::CHART_PAINT).child(self.items))
    }
}

/// Puts a soft glow in the mark's own colour around `element`. The glow is a CSS
/// `drop-shadow` from `--c`, so the element (or an ancestor) must carry the colour; on
/// a drawing stretched to its box, put it on the stretched `<svg>` itself so the glow
/// stays round.
pub(crate) fn glow(element: Element) -> Element {
    element.flag("data-glow", true).uses(Uses::CHART_PAINT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    #[test]
    fn inks_write_attributes_never_colours() {
        let out = render(&Ink::slot(3).vivid().on(el("stop")).into());
        assert_eq!(out, r#"<stop data-color="3" data-mix="vivid"></stop>"#);
        let out = render(&Ink::tone(Tone::Success).soft().on(el("stop")).into());
        assert_eq!(out, r#"<stop data-tone="success" data-mix="soft"></stop>"#);
        assert_eq!(Ink::slot(0), Ink::slot(1));
        assert_eq!(Ink::slot(99), Ink::slot(8));
        assert_eq!(Ink::own().resolve(Color::Slot(5)), Ink::slot(5));
        assert_eq!(
            Ink::own().vivid().resolve(Color::Tone(Tone::Danger)),
            Ink::tone(Tone::Danger).vivid()
        );
        let out = render(&Ink::accent().on(el("rect")).into());
        assert_eq!(out, r#"<rect data-color="accent"></rect>"#);
    }

    #[test]
    fn a_gradient_has_its_stops_in_order_and_its_own_id() {
        let g = Gradient::vertical()
            .stop_with_opacity(1.0, Ink::own(), 0.0)
            .stop_with_opacity(0.0, Ink::own(), 0.4);
        let out = render(&g.element("c-p0", Color::Slot(2), None).into());
        assert_eq!(
            out,
            r#"<linearGradient id="c-p0" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-opacity="0.4" data-color="2"></stop><stop offset="1" stop-opacity="0" data-color="2"></stop></linearGradient>"#
        );
        // Across the plot: in the drawing's own units.
        let out = render(
            &Gradient::vertical()
                .stop(0.0, Ink::slot(1))
                .across_plot()
                .element("c-p1", Color::Slot(1), Some([0.0, 10.0, 1000.0, 200.0]))
                .into(),
        );
        assert!(
            out.contains(r#"gradientUnits="userSpaceOnUse" x1="0" y1="10" x2="0" y2="210""#),
            "{out}"
        );
        assert!(!out.contains('#') || out.contains("id=\"c-p1\""));
    }

    #[test]
    fn odd_numbers_make_a_gradient_that_still_draws() {
        let g = Gradient::angle(f64::NAN).stop_with_opacity(f64::INFINITY, Ink::muted(), -3.0);
        let out = render(&g.element("x", Color::Slot(1), None).into());
        assert!(!out.contains("NaN") && !out.contains("inf"), "{out}");
        assert!(out.contains(r#"offset="0""#), "{out}");
        // No stops: the mark's own colour.
        let out = render(&Gradient::radial().element("y", Color::Slot(4), None).into());
        assert!(out.contains(r#"<stop offset="0" data-color="4">"#), "{out}");
        let g = Gradient::angle(90.0);
        let out = render(&g.element("z", Color::Slot(1), None).into());
        assert!(out.contains(r#"x1="0.5" y1="0" x2="0.5" y2="1""#), "{out}");
    }

    #[test]
    fn defs_name_each_gradient_after_the_drawing() {
        let mut defs = Defs::new("chart-sleep-1a2b3c");
        let a = defs.gradient(&Gradient::fade(Ink::own()), Color::Slot(1), None);
        let b = defs.gradient(&Gradient::sheen(Ink::slot(2)), Color::Slot(1), None);
        assert_eq!(a, "url(#chart-sleep-1a2b3c-p0)");
        assert_eq!(b, "url(#chart-sleep-1a2b3c-p1)");
        let node = defs.node().expect("defs");
        assert!(node.uses.has(Uses::CHART_PAINT));
        assert!(Defs::new("x").node().is_none());
    }

    #[test]
    fn a_scale_picks_the_band_of_a_value() {
        let scale = ColorScale::new(Ink::tone(Tone::Danger))
            .label("Low")
            .from(75, Ink::tone(Tone::Success))
            .label("Good")
            .from(50, Ink::tone(Tone::Warning))
            .from(f64::NAN, Ink::slot(1));
        assert_eq!(scale.ink_of(10.0), Ink::tone(Tone::Danger));
        assert_eq!(scale.ink_of(50.0), Ink::tone(Tone::Warning));
        assert_eq!(scale.ink_of(74.9), Ink::tone(Tone::Warning));
        assert_eq!(scale.ink_of(75.0), Ink::tone(Tone::Success));
        assert_eq!(scale.band_of(99.0).1, Some("Good"));
        // A label names the band added last, whatever order the thresholds came in.
        let labelled = scale.labelled();
        assert_eq!(labelled.len(), 2, "{labelled:?}");
        assert_eq!(labelled[0].1, "Low");
        let fair = ColorScale::new(Ink::slot(1))
            .from(75, Ink::slot(3))
            .label("Good")
            .from(50, Ink::slot(4))
            .label("Fair");
        assert_eq!(fair.band_of(60.0).1, Some("Fair"));
        assert_eq!(fair.band_of(80.0).1, Some("Good"));
        assert_eq!(
            fair.labelled()
                .iter()
                .map(|(_, l)| l.as_str())
                .collect::<Vec<_>>(),
            ["Fair", "Good"]
        );
    }
}
