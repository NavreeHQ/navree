//! [`Gauge`]: one value on an open arc, 270 degrees by default.

use super::super::value::IntoValue;
use super::super::{Common, TableMode, common_methods};
use super::finish;
use super::ring::{One, radial_methods};
use crate::node::Node;

/// One value on an open arc: a stress level, a recovery index, a temperature on its
/// scale. The arc is 270 degrees by default, open at the bottom, and runs clockwise
/// from its lower left end; [`sweep`](Self::sweep) makes it a half circle or any angle
/// from 90 to 360 degrees. [`min`](Self::min) starts the scale somewhere other than zero.
///
/// It has everything a [`Ring`](super::Ring) has: a gradient along the arc, a colour by
/// band, a glow, the value in the middle with its label, motion, a size per screen
/// width. It does not lap: a value past the top fills the arc.
///
/// [Zones](Self::zone) lie on its track, each its stretch of the scale in its own
/// colour, and the arc runs down their middle, half as thick, so every zone shows from
/// end to end; the arc wears the colour of the zone its value is in. A legend names the
/// zones, the summary and the table say which one the value is in, and the table's note
/// gives each zone's ends.
///
/// ```
/// use navree_ui::charts::{Gauge, Gradient, Ink, Motion, Zone};
///
/// let relaxation = Gauge::new("Relaxation", 73)
///     .label("Recovery index")
///     .fill(Gradient::horizontal().stop(0.0, Ink::slot(3)).stop(1.0, Ink::tone(navree_ui::Tone::named("cyan"))))
///     .glow()
///     .size(200)
///     .motion(Motion::new());
/// let temperature = Gauge::new("Body temperature", 36.7)
///     .min(34)
///     .max(39)
///     .unit("°C")
///     .sweep(180.0);
/// // This week's training load against the last four, on its zones.
/// let load = Gauge::new("Load ratio", 1.12)
///     .max(2)
///     .format(navree_ui::charts::NumberFormat::decimals(2).fixed())
///     .zone(Zone::new(0, 0.8).label("Undertraining").ink(Ink::slot(7)))
///     .zone(Zone::new(0.8, 1.3).label("Optimal").ink(Ink::slot(3)))
///     .zone(Zone::new(1.3, 1.5).label("Overreaching").ink(Ink::slot(4)))
///     .zone(Zone::new(1.5, 2).label("Overuse").ink(Ink::slot(8)));
/// # let _ = (navree_ui::render(&relaxation.into()), navree_ui::render(&temperature.into()));
/// # let _ = navree_ui::render(&load.into());
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Gauge {
    common: Common,
    one: One,
}

common_methods!(Gauge);
radial_methods!(Gauge);

impl Gauge {
    /// A gauge called `title` holding `value`, on a scale from 0 to 100 unless
    /// [`min`](Self::min) and [`max`](Self::max) say otherwise.
    pub fn new(title: impl Into<String>, value: impl IntoValue) -> Self {
        let mut common = Common::new(title.into());
        common.hide_title = true;
        common.table = TableMode::ScreenReader;
        Self {
            common,
            one: One::new(value.into_value(), 160, 270.0),
        }
    }

    /// Where the scale starts (0 by default): 34 for a body temperature in °C.
    pub fn min(mut self, min: impl IntoValue) -> Self {
        if let Some(min) = min.into_value().filter(|v| v.is_finite()) {
            self.one.min = min;
        }
        self
    }

    /// How far the arc sweeps, in degrees from 90 to 360 (270 by default): 180 is a
    /// half circle, open below.
    pub fn sweep(mut self, degrees: f64) -> Self {
        if degrees.is_finite() {
            self.one.sweep = degrees.clamp(90.0, 360.0);
        }
        self
    }

    /// Adds a zone of the scale, drawn on the track in its colour: from and to in the
    /// value's units, with a label for the legend, the summary and the table. A zone
    /// with no ink takes the chart colours in turn. Where zones overlap, the one added
    /// last is drawn over and names the value.
    pub fn zone(mut self, zone: super::super::Zone) -> Self {
        self.one.zones.push(zone);
        self
    }

    /// Shows the title over the gauge, and the switch that shows its table.
    pub fn show_title(mut self) -> Self {
        self.common.hide_title = false;
        self.common.table = TableMode::Switch;
        self
    }

    fn into_node(self) -> Node {
        let drawn = self.one.draw(&self.common, "gauge");
        let height = drawn.as_ref().map_or(self.one.sizes.base, |d| d.height);
        finish(self.common, drawn, "gauge", height, self.one.inline)
    }
}

crate::node::into_node!(Gauge);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    fn html(gauge: Gauge) -> String {
        render(&gauge.into())
    }

    fn path_of(out: &str, slot: &str) -> Option<String> {
        let at = out.find(&format!(r#"data-slot="{slot}""#))?;
        let rest = &out[at..];
        let d = rest.find(" d=\"")? + 4;
        let end = rest[d..].find('"')?;
        Some(rest[d..d + end].to_owned())
    }

    #[test]
    fn a_gauge_sweeps_270_degrees_from_its_lower_left() {
        let out = html(Gauge::new("Stress", 100).size(200).thickness(20.0));
        // Radius 90 round (100, 100): the track and a full arc end at the lower right.
        let track = path_of(&out, "radial-track").unwrap_or_default();
        assert!(track.starts_with("M36.36 163.64A90 90 0 0 1 "), "{track}");
        assert!(track.ends_with("163.64 163.64"), "{track}");
        assert_eq!(path_of(&out, "radial-arc"), Some(track));
        // The drawing stops under the ends of the arc: 100 + 63.64 + 10 + 1.
        assert!(out.contains(r#"viewBox="0 0 200 174.64""#), "{out}");
        assert!(out.contains(r#"width="200" height="175""#), "{out}");
    }

    #[test]
    fn a_gauge_at_zero_and_at_half() {
        let out = html(Gauge::new("Stress", 0).size(200).thickness(20.0));
        assert!(!out.contains("radial-arc"), "{out}");
        let half = html(Gauge::new("Stress", 50).size(200).thickness(20.0));
        // Half of 270 from -135 ends at +0: twelve o'clock.
        assert_eq!(
            path_of(&half, "radial-arc").as_deref(),
            Some("M36.36 163.64A90 90 0 0 1 100 10")
        );
    }

    #[test]
    fn past_the_top_a_gauge_fills_and_does_not_lap() {
        let out = html(Gauge::new("Stress", 140).size(200).thickness(20.0));
        assert!(!out.contains("radial-lap"), "{out}");
        assert!(out.contains("140 of 100, 140%."), "{out}");
    }

    #[test]
    fn a_scale_that_starts_above_zero() {
        let out = html(
            Gauge::new("Body temperature", 36.5)
                .min(34)
                .max(39)
                .unit("°C")
                .sweep(180.0)
                .size(200)
                .thickness(20.0),
        );
        // Half way along a half circle from -90: twelve o'clock.
        assert_eq!(
            path_of(&out, "radial-arc").as_deref(),
            Some("M10 100A90 90 0 0 1 100 10")
        );
        assert!(
            out.contains("Body temperature: 36.5°C, on a scale from 34°C to 39°C."),
            "{out}"
        );
        assert!(
            out.contains(r#"data-place="low""#),
            "the value sits low in a half circle: {out}"
        );
        // A half circle is half as tall, and a little more for its thickness.
        assert!(out.contains(r#"viewBox="0 0 200 111""#), "{out}");
    }

    #[test]
    fn odd_sweeps_are_kept_in_range() {
        let out = html(Gauge::new("x", 50).sweep(10.0).size(100));
        assert!(out.contains("radial-arc"), "{out}");
        let full = html(Gauge::new("x", 50).sweep(f64::INFINITY));
        assert!(full.contains("radial-arc"), "{full}");
    }
}
