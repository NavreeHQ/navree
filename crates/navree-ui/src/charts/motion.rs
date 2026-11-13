//! Motion for chart marks: lines that draw on, bars that grow, dots that pop in, and
//! numbers that count up.
//!
//! **The final state is the page.** Every mark is written where it ends; motion is a
//! CSS animation that starts *from* somewhere else (a scale of zero, a clip that hides
//! the line) and is only declared under `prefers-reduced-motion: no-preference`. So a
//! reader who asks for less motion, a page with no script, a print and an image
//! renderer all see the finished chart, and [`Motion::none`] (the default) writes
//! nothing at all. Animations fill backwards only: before its delay a mark waits in its
//! start state, and when it ends nothing is left on it.
//!
//! **No style attribute.** Delays are steps of 50 ms written as `data-delay="n"`
//! (at most 24, 1.2 s), speeds as `data-speed`, the effect as `data-motion`; the chart
//! paint sheet ([`Uses::CHART_PAINT`]) holds the keyframes. Inside the crate, a drawing
//! calls [`Motion::apply`] on each mark with its [`Effect`] and its index for the
//! stagger:
//!
//! ```ignore
//! let bar = motion.apply(rect, Effect::GrowUp, i);
//! let line_layer = motion.apply(svg, Effect::Wipe, 0);
//! let dot = motion.apply(circle, Effect::Pop, i);
//! ```

use super::value::IntoValue;
use crate::node::{Element, Node, Uses, el};

/// The longest delay, in steps of 50 ms.
const MAX_STEPS: u32 = 24;

/// How long an effect takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Speed {
    /// About half the usual time: small marks, a dashboard full of them.
    Fast,
    /// The usual time: a line draws in 1.2 s, a bar grows in 0.7 s.
    #[default]
    Normal,
    /// About half as long again: a hero chart, a ring on its own.
    Slow,
}

/// How a chart's marks come in: off ([`Motion::none`], the default), or on with a
/// delay, a stagger between marks and a speed.
///
/// Motion is only ever a way in: the marks are drawn where they end, so the chart reads
/// the same with no script, when the reader asks for less motion, and in an image.
///
/// ```
/// use navree_ui::charts::{Chart, Motion, Series, Speed};
///
/// let chart = Chart::bar("Stress by hour")
///     .categories(["06:00", "07:00", "08:00"])
///     .series(Series::new("Stress", [3, 5, 4]))
///     .motion(Motion::new().delay(200).stagger(30).speed(Speed::Fast));
/// # let _ = navree_ui::render(&chart.into());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub struct Motion {
    on: bool,
    delay: u32,
    stagger: u32,
    speed: Speed,
}

impl Default for Motion {
    fn default() -> Self {
        Self::none()
    }
}

impl Motion {
    /// Marks come in: lines draw from the start, bars grow from the baseline, dots pop
    /// in, each mark 30 ms after the one before.
    pub const fn new() -> Self {
        Self {
            on: true,
            delay: 0,
            stagger: 30,
            speed: Speed::Normal,
        }
    }

    /// No motion: the chart is drawn finished. For images, prints and anything rendered
    /// once.
    pub const fn none() -> Self {
        Self {
            on: false,
            delay: 0,
            stagger: 0,
            speed: Speed::Normal,
        }
    }

    /// Waits `ms` before the first mark moves (in steps of 50 ms, at most 1.2 s).
    pub const fn delay(mut self, ms: u32) -> Self {
        self.delay = ms;
        self
    }

    /// Waits `ms` more for each mark after the first (0 moves them together).
    pub const fn stagger(mut self, ms: u32) -> Self {
        self.stagger = ms;
        self
    }

    /// How long each mark takes.
    pub const fn speed(mut self, speed: Speed) -> Self {
        self.speed = speed;
        self
    }

    /// The same motion, `ms` later: a mark that comes in after others have.
    pub(crate) const fn after(mut self, ms: u32) -> Self {
        self.delay = self.delay.saturating_add(ms);
        self
    }

    /// Whether marks move at all.
    pub const fn is_on(self) -> bool {
        self.on
    }

    /// The delay step of mark `index`: 50 ms each.
    pub(crate) fn step(self, index: usize) -> u32 {
        let ms = u64::from(self.delay).saturating_add(
            u64::from(self.stagger).saturating_mul(u64::try_from(index).unwrap_or(u64::MAX)),
        );
        u32::try_from(ms.saturating_add(25) / 50)
            .unwrap_or(MAX_STEPS)
            .min(MAX_STEPS)
    }

    /// Gives `element` the `effect`, as mark `index` of its drawing; nothing when
    /// motion is off.
    pub(crate) fn apply(self, element: Element, effect: Effect, index: usize) -> Element {
        if !self.on {
            return element;
        }
        let step = self.step(index);
        let element = element
            .attr("data-motion", effect.name())
            .uses(Uses::CHART_PAINT);
        let element = if step > 0 {
            element.attr("data-delay", i64::from(step))
        } else {
            element
        };
        match self.speed {
            Speed::Normal => element,
            Speed::Fast => element.attr("data-speed", "fast"),
            Speed::Slow => element.attr("data-speed", "slow"),
        }
    }
}

/// What a mark does as it comes in. Each is a keyframe set in the paint sheet that
/// starts somewhere and ends at the mark as it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    /// A stroke draws along its path: the element needs `pathLength="1"` and no
    /// dashes of its own. For rings and arcs; not for a stroke that doesn't scale.
    #[allow(dead_code)]
    Draw,
    /// Revealed from the left edge to the right: a line or an area in a stretched layer.
    Wipe,
    /// Revealed from the right edge to the left: the same, right to left.
    WipeBack,
    /// Grows up from its bottom edge: a bar above its baseline.
    GrowUp,
    /// Grows down from its top edge: a bar below its baseline.
    GrowDown,
    /// Grows right from its left edge: a bar along a row, a progress fill.
    GrowRight,
    /// Grows left from its right edge.
    GrowLeft,
    /// Fades in: labels, reference lines, zones.
    Fade,
    /// Scales up from its centre: dots, a centre value.
    Pop,
}

impl Effect {
    fn name(self) -> &'static str {
        match self {
            Self::Draw => "draw",
            Self::Wipe => "wipe",
            Self::WipeBack => "wipe-back",
            Self::GrowUp => "grow-up",
            Self::GrowDown => "grow-down",
            Self::GrowRight => "grow-right",
            Self::GrowLeft => "grow-left",
            Self::Fade => "fade",
            Self::Pop => "pop",
        }
    }
}

/// A number that counts up from zero to its value when it comes into view: the big
/// figure in a ring, a score, a percentage.
///
/// The value is written in the page as the server formatted it, so it reads the same
/// with no script, to a screen reader, and when the reader asks for less motion; the
/// count-up module ([`Uses::COUNT_UP`]) only animates it, keeping what is written
/// before and after the digits (`₹`, `%`, `+`) and grouping the digits as the locale
/// does on the way (`en-IN` writes `1,23,456`). It ends on the server's text exactly.
///
/// ```
/// use navree_ui::charts::CountUp;
///
/// let score = CountUp::new(87, "87");
/// let sleep = CountUp::new(92.5, "92.5%").locale("en-IN");
/// let html = navree_ui::render(&score.into());
/// assert!(html.contains(r#"data-to="87""#) && html.contains(">87<"));
/// # let _ = sleep;
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct CountUp {
    to: Option<f64>,
    text: String,
    locale: Option<String>,
    motion: Motion,
}

impl CountUp {
    /// Counts up to `value`, which is written `text`: the number as the page shows it,
    /// with its prefix and suffix. A value that is not a finite number doesn't count.
    pub fn new(value: impl IntoValue, text: impl Into<String>) -> Self {
        Self {
            to: value.into_value().filter(|v| v.is_finite()),
            text: text.into(),
            locale: None,
            motion: Motion::new().delay(200),
        }
    }

    /// The language the digits are grouped by while counting (`en-IN`, `de`): the
    /// page's language when not set.
    pub fn locale(mut self, tag: impl Into<String>) -> Self {
        let tag: String = tag.into();
        if !tag.is_empty()
            && tag.len() <= 35
            && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            self.locale = Some(tag);
        }
        self
    }

    /// The motion it fades in with; [`Motion::none`] writes the number alone, with no
    /// module and no count.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
        self
    }

    fn into_node(self) -> Node {
        let span = el("span").slot("count-up");
        match self.to {
            Some(to) if self.motion.is_on() => {
                // The value with at most three decimals, as many as the module counts with.
                let mut out = String::new();
                super::num(&mut out, to, 3);
                self.motion
                    .apply(span, Effect::Fade, 0)
                    .attr("data-to", out)
                    .attr_opt("data-locale", self.locale)
                    .uses(Uses::COUNT_UP)
                    .child(self.text)
                    .into()
            }
            _ => span.child(self.text).into(),
        }
    }
}

crate::node::into_node!(CountUp);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    #[test]
    fn delays_are_steps_of_fifty_milliseconds_and_stop_at_the_longest() {
        let m = Motion::new().delay(200).stagger(30);
        assert_eq!(m.step(0), 4);
        assert_eq!(m.step(1), 5);
        assert_eq!(m.step(10), 10);
        assert_eq!(m.step(10_000), MAX_STEPS);
        assert_eq!(Motion::new().delay(u32::MAX).step(usize::MAX), MAX_STEPS);
        assert_eq!(Motion::new().stagger(0).step(50), 0);
    }

    #[test]
    fn a_still_chart_writes_no_motion() {
        let out = render(&Motion::none().apply(el("rect"), Effect::GrowUp, 3).into());
        assert_eq!(out, "<rect></rect>");
        assert_eq!(Motion::default(), Motion::none());
    }

    #[test]
    fn motion_is_attributes_and_links_the_paint_sheet() {
        let rect = Motion::new()
            .delay(100)
            .speed(Speed::Fast)
            .apply(el("rect"), Effect::GrowUp, 2);
        assert!(rect.uses.has(Uses::CHART_PAINT));
        let out = render(&rect.into());
        assert_eq!(
            out,
            r#"<rect data-motion="grow-up" data-delay="3" data-speed="fast"></rect>"#
        );
        // The first mark with no delay has no delay attribute.
        let out = render(&Motion::new().apply(el("path"), Effect::Wipe, 0).into());
        assert_eq!(out, r#"<path data-motion="wipe"></path>"#);
    }

    #[test]
    fn a_count_up_writes_its_value_and_its_text() {
        let node: Node = CountUp::new(123456, "₹1,23,456").locale("en-IN").into();
        let Node::Element(el) = &node else {
            panic!("an element")
        };
        assert!(el.uses.has(Uses::COUNT_UP) && el.uses.has(Uses::CHART_PAINT));
        let out = render(&node);
        assert!(out.contains(r#"data-to="123456""#), "{out}");
        assert!(out.contains(r#"data-locale="en-IN""#), "{out}");
        assert!(out.contains(">₹1,23,456</span>"), "{out}");
        // Still: the number alone.
        let out = render(&CountUp::new(5, "5").motion(Motion::none()).into());
        assert_eq!(out, r#"<span data-slot="count-up">5</span>"#);
        // Not a number: the text alone; a hostile locale is dropped.
        let out = render(&CountUp::new(f64::NAN, "–").locale("\"><x").into());
        assert_eq!(out, r#"<span data-slot="count-up">–</span>"#);
        let out = render(&CountUp::new(1.0 / 3.0, "0.33").into());
        assert!(out.contains(r#"data-to="0.333""#), "{out}");
    }
}
