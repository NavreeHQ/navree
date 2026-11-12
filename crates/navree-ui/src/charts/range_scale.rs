//! [`RangeScale`]: where one value sits on a scale.

use super::format::NumberFormat;
use super::linear::{LinearLabels, say};
use super::motion::{Effect, Motion};
use super::paint::{Defs, Gradient};
use super::value::{IntoValue, finite};
use super::{
    Color, Common, Drawn, TableData, TableMode, common_methods, frame, n, pct, svg, value_cell,
};
use crate::node::{Node, Uses, el};

/// One value on a scale between a low end and a high end, with a marker: a resting heart
/// rate between 40 and 120, a blood-pressure reading, a price between its day's low and
/// high. The track can be a gradient through any stops, so the ends can read as good and
/// bad; the colours are never the only way to read it, because the end labels, the
/// summary and the table give the numbers.
///
/// A value outside the scale sits at the nearer end, and the summary says it is below or
/// above.
///
/// ```
/// use navree_ui::charts::{Gradient, Ink, RangeScale};
/// use navree_ui::Tone;
///
/// let resting = RangeScale::new("Resting heart rate", 58)
///     .range(40, 120)
///     .format(navree_ui::charts::NumberFormat::number().suffix(" bpm"))
///     .track(
///         Gradient::horizontal()
///             .stop_with_opacity(0.0, Ink::tone(Tone::Success), 0.5)
///             .stop_with_opacity(0.3, Ink::tone(Tone::named("cyan")), 0.5)
///             .stop_with_opacity(0.6, Ink::slot(2), 0.5)
///             .stop_with_opacity(1.0, Ink::tone(Tone::Danger), 0.5),
///     )
///     .show_value();
/// let html = navree_ui::render(&resting.into());
/// assert!(html.contains("58 bpm on a scale from 40 bpm to 120 bpm."), "{html}");
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct RangeScale {
    common: Common,
    words: LinearLabels,
    value: Option<f64>,
    min: f64,
    max: f64,
    track: Option<Gradient>,
    format: NumberFormat,
    ends: Option<(String, String)>,
    show_value: bool,
    motion: Motion,
}

common_methods!(RangeScale);

impl RangeScale {
    /// A scale called `title` with the marker at `value`. A value that is missing leaves
    /// the chart empty.
    pub fn new(title: impl Into<String>, value: impl IntoValue) -> Self {
        let mut common = Common::new(title.into());
        // One row: nothing to switch.
        common.table = TableMode::ScreenReader;
        Self {
            common,
            words: LinearLabels::english(),
            value: finite(value.into_value()),
            min: 0.0,
            max: 100.0,
            track: None,
            format: NumberFormat::number(),
            ends: None,
            show_value: false,
            motion: Motion::none(),
        }
    }

    /// The low and the high end of the scale (0 and 100 by default). Reversed ends are
    /// put in order; ends that are equal make a scale one wide.
    pub fn range(mut self, min: impl IntoValue, max: impl IntoValue) -> Self {
        if let (Some(a), Some(b)) = (finite(min.into_value()), finite(max.into_value())) {
            let (low, high) = if a <= b { (a, b) } else { (b, a) };
            self.min = low;
            self.max = if high > low { high } else { low + 1.0 };
        }
        self
    }

    /// Paints the track with `gradient`, over the quiet track: its stops set where the
    /// colours change, along the scale from the low end (0) to the high end (1). Use a
    /// [horizontal](Gradient::horizontal) one.
    pub fn track(mut self, gradient: Gradient) -> Self {
        self.track = Some(gradient);
        self
    }

    /// How the value and the ends are written.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.format = format;
        self
    }

    /// Writes `low` and `high` under the ends, instead of the ends as numbers.
    pub fn ends(mut self, low: impl Into<String>, high: impl Into<String>) -> Self {
        self.ends = Some((low.into(), high.into()));
        self
    }

    /// Writes the value above the marker.
    pub fn show_value(mut self) -> Self {
        self.show_value = true;
        self
    }

    /// How the marker comes in: it pops in, after the delay. Off by default; always off
    /// for a reader who asks for less motion.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
        self
    }

    /// The words the chart writes besides the common ones.
    pub fn words(mut self, words: LinearLabels) -> Self {
        self.words = words;
        self
    }

    fn into_node(self) -> Node {
        let mut this = self;
        let drawn = this.draw();
        frame(this.common, drawn, "range-scale", 40)
    }

    fn draw(&mut self) -> Option<Drawn> {
        let value = self.value?;
        let labels = self.common.labels.clone();
        let numbers = self.common.numbers.clone();
        let words = self.words.clone();
        let fmt = |v: f64| self.format.format(v, &numbers);
        let (min, max) = (self.min, self.max);
        let place = ((value - min) / (max - min)).clamp(0.0, 1.0) * 100.0;

        // The marker is a circle of radius 8 on a track 12 high; a value above the marker
        // takes 18 more.
        let top = if self.show_value { 18.0 } else { 0.0 };
        let (track_h, mid) = (12.0, top + 10.0);
        let svg_h = top + 20.0;
        let id = self.common.id();
        let mut defs = Defs::new(&id);
        let paint = self
            .track
            .as_ref()
            .map(|g| defs.gradient(g, Color::Slot(1), None));
        let y = n(mid - track_h / 2.0, 1);
        let track = el("rect")
            .slot("scale-track")
            .attr("x", "0")
            .attr("y", y.clone())
            .attr("width", "100%")
            .attr("height", n(track_h, 0))
            .attr("rx", "4");
        let fill = paint.map(|url| {
            el("rect")
                .slot("scale-fill")
                .attr("x", "0")
                .attr("y", y)
                .attr("width", "100%")
                .attr("height", n(track_h, 0))
                .attr("rx", "4")
                .attr("fill", url)
        });
        let marker = self.motion.apply(
            el("circle")
                .slot("scale-marker")
                .attr("cx", pct(place))
                .attr("cy", n(mid, 1))
                .attr("r", "8"),
            Effect::Pop,
            0,
        );
        let anchor = if place < 10.0 {
            "start"
        } else if place > 90.0 {
            "end"
        } else {
            "middle"
        };
        let shown = self.show_value.then(|| {
            el("text")
                .slot("scale-value")
                .attr("x", pct(place))
                .attr("y", "12")
                .attr("text-anchor", anchor)
                .child(fmt(value))
        });
        let drawing = svg()
            .slot("scale")
            .attr("width", "100%")
            .attr("height", n(svg_h, 0))
            .uses(
                Uses::CHART_PAINT
                    .with(Uses::CHART_LINEAR)
                    .with(Uses::CHARTS),
            )
            .children((defs.node(), track, fill, shown, marker));

        let (low, high) = self.ends.clone().unwrap_or_else(|| (fmt(min), fmt(max)));
        let body = el("div").slot("scale-wrap").children((
            drawing,
            el("div")
                .slot("scale-ends")
                .attr("aria-hidden", "true")
                .children((
                    el("span").child(low.clone()),
                    el("span").child(high.clone()),
                )),
        ));

        let template = if value < min {
            &words.summary_below
        } else if value > max {
            &words.summary_above
        } else {
            &words.summary_scale
        };
        let summary = say(
            template,
            &[
                ("value", &fmt(value)),
                ("min", &fmt(min)),
                ("max", &fmt(max)),
            ],
        );

        Some(Drawn {
            legend: None,
            body: body.into(),
            body_attrs: Vec::new(),
            summary,
            table: TableData {
                head: vec![
                    (labels.category.to_string(), None),
                    (labels.value.to_string(), None),
                    (words.minimum.to_string(), None),
                    (words.maximum.to_string(), None),
                ],
                rows: vec![(
                    self.common.title.clone(),
                    vec![
                        value_cell(Some(fmt(value)), &labels),
                        value_cell(Some(fmt(min)), &labels),
                        value_cell(Some(fmt(max)), &labels),
                    ],
                )],
                links: Vec::new(),
                note: None,
            },
            height: 40,
            note: None,
            interactive: false,
            tip: None,
        })
    }
}

crate::node::into_node!(RangeScale);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::charts::Ink;
    use crate::components::Tone;
    use crate::render;

    fn resting(value: f64) -> RangeScale {
        RangeScale::new("Resting heart rate", value).range(40, 120)
    }

    fn html(scale: RangeScale) -> String {
        render(&scale.into())
    }

    #[test]
    fn the_marker_sits_where_the_value_is_on_the_scale() {
        // (58 - 40) / (120 - 40) = 22.5%.
        let out = html(resting(58.0));
        assert!(
            out.contains(r#"<circle data-slot="scale-marker" cx="22.5%" cy="10" r="8">"#),
            "{out}"
        );
        assert!(
            out.contains(
                r#"<rect data-slot="scale-track" x="0" y="4" width="100%" height="12" rx="4">"#
            ),
            "{out}"
        );
        assert!(out.contains(r#"width="100%" height="20""#), "{out}");
        assert!(out.contains("58 on a scale from 40 to 120."), "{out}");
    }

    #[test]
    fn a_value_off_the_scale_sits_at_the_nearer_end_and_the_summary_says_so() {
        let out = html(resting(30.0));
        assert!(
            out.contains(r#"cx="0%""#) && out.contains("30, below the scale from 40 to 120."),
            "{out}"
        );
        let out = html(resting(200.0));
        assert!(
            out.contains(r#"cx="100%""#) && out.contains("200, above the scale from 40 to 120."),
            "{out}"
        );
        // The ends are in the scale.
        assert!(
            html(resting(40.0)).contains(r#"cx="0%""#)
                && html(resting(120.0)).contains(r#"cx="100%""#)
        );
        assert!(html(resting(40.0)).contains("40 on a scale"));
    }

    #[test]
    fn a_track_gradient_is_painted_over_the_quiet_track() {
        let out = html(
            resting(58.0).track(
                Gradient::horizontal()
                    .stop_with_opacity(0.0, Ink::tone(Tone::Success), 0.5)
                    .stop_with_opacity(0.3, Ink::tone(Tone::named("cyan")), 0.5)
                    .stop_with_opacity(0.6, Ink::slot(2), 0.5)
                    .stop_with_opacity(1.0, Ink::tone(Tone::Danger), 0.5),
            ),
        );
        assert!(
            out.contains(r#"<linearGradient id="chart-resting-heart-rate-"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<stop offset="0.3" stop-opacity="0.5" data-tone="cyan">"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<stop offset="0.6" stop-opacity="0.5" data-color="2">"#),
            "{out}"
        );
        assert!(
            out.contains(r#"fill="url(#chart-resting-heart-rate-"#),
            "{out}"
        );
        // The quiet track comes first, the paint over it, the marker last.
        let track = out.find("scale-track").expect("track");
        let fill = out.find("scale-fill").expect("fill");
        let marker = out.find("scale-marker").expect("marker");
        assert!(track < fill && fill < marker);
        // No gradient asked: the quiet track alone.
        let plain = html(resting(58.0));
        assert!(
            !plain.contains("linearGradient") && !plain.contains("scale-fill"),
            "{plain}"
        );
        assert!(!out.contains("style="), "{out}");
    }

    #[test]
    fn the_ends_are_written_under_the_scale_in_the_readers_digits_or_in_words() {
        let out = html(resting(58.0));
        assert!(out.contains(r#"<div data-slot="scale-ends" aria-hidden="true"><span>40</span><span>120</span></div>"#), "{out}");
        let out = html(
            RangeScale::new("Balance", 4_50_000)
                .range(0, 10_00_000)
                .format(NumberFormat::currency("₹"))
                .locale("en-IN"),
        );
        assert!(
            out.contains("<span>₹0</span><span>₹10,00,000</span>"),
            "{out}"
        );
        assert!(
            out.contains("₹4,50,000 on a scale from ₹0 to ₹10,00,000."),
            "{out}"
        );
        let out = html(resting(58.0).ends("Low", "High"));
        assert!(out.contains("<span>Low</span><span>High</span>"), "{out}");
        // The summary and the table still give the numbers.
        assert!(
            out.contains("58 on a scale from 40 to 120.")
                && out.contains("<td data-num=\"\">120</td>"),
            "{out}"
        );
    }

    #[test]
    fn the_value_can_be_written_above_the_marker_clear_of_the_edges() {
        let out = html(resting(58.0).show_value());
        assert!(
            out.contains(
                r#"<text data-slot="scale-value" x="22.5%" y="12" text-anchor="middle">58</text>"#
            ),
            "{out}"
        );
        assert!(
            out.contains(r#"cy="28""#) && out.contains(r#"height="38""#),
            "{out}"
        );
        assert!(html(resting(41.0).show_value()).contains(r#"text-anchor="start""#));
        assert!(html(resting(119.0).show_value()).contains(r#"text-anchor="end""#));
        assert!(!html(resting(58.0)).contains("scale-value"));
    }

    #[test]
    fn odd_ranges_are_put_right() {
        // Reversed ends are put in order; equal ends make a scale one wide.
        let out = html(RangeScale::new("x", 5).range(10, 0));
        assert!(
            out.contains("5 on a scale from 0 to 10.") && out.contains(r#"cx="50%""#),
            "{out}"
        );
        let out = html(RangeScale::new("x", 5).range(5, 5));
        assert!(
            out.contains("from 5 to 6.") && out.contains(r#"cx="0%""#),
            "{out}"
        );
        // A range that isn't a number leaves the last one alone.
        let out = html(RangeScale::new("x", 50).range(f64::NAN, 10));
        assert!(out.contains("from 0 to 100."), "{out}");
        assert!(!out.contains("NaN") && !out.contains("inf"), "{out}");
    }

    #[test]
    fn no_value_is_an_empty_chart() {
        for missing in [f64::NAN, f64::INFINITY] {
            assert!(html(RangeScale::new("x", missing)).contains(r#"data-state="empty""#));
        }
        assert!(html(RangeScale::new("x", None::<f64>)).contains(r#"data-state="empty""#));
    }

    #[test]
    fn the_table_twin_is_one_row_for_a_screen_reader() {
        let out = html(resting(58.0));
        assert!(out.contains(r#"<th scope="row">Resting heart rate</th><td data-num="">58</td><td data-num="">40</td><td data-num="">120</td>"#), "{out}");
        assert!(out.contains(r#"<th scope="col" data-num="">Minimum</th><th scope="col" data-num="">Maximum</th>"#), "{out}");
        assert!(
            !out.contains("chart-table-switch"),
            "one row needs no switch: {out}"
        );
        // It can be shown, or behind the switch.
        assert!(
            html(resting(58.0).table(crate::charts::TableMode::Switch))
                .contains("chart-table-switch")
        );
    }

    #[test]
    fn the_scale_is_a_named_picture_and_not_a_tab_stop() {
        let out = html(resting(58.0));
        assert!(
            out.contains(r#"data-kind="range-scale""#)
                && out.contains(r#"role="img" aria-label="Resting heart rate, chart""#),
            "{out}"
        );
        assert!(
            !out.contains("data-ui-chart") && !out.contains("data-rows"),
            "{out}"
        );
        assert!(
            out.contains(r#"<svg aria-hidden="true" focusable="false" data-slot="scale""#),
            "{out}"
        );
    }

    #[test]
    fn the_marker_pops_in_when_motion_is_on() {
        let out = html(resting(58.0).motion(Motion::new().delay(500)));
        assert!(
            out.contains(r#"r="8" data-motion="pop" data-delay="10">"#)
                || out.contains(r#"data-motion="pop" data-delay="10""#),
            "{out}"
        );
        assert!(!html(resting(58.0)).contains("data-motion"));
    }

    #[test]
    fn words_come_from_the_labels() {
        let mut words = LinearLabels::english();
        words.minimum = "न्यूनतम".into();
        words.summary_scale = "{value} ({min} से {max})".into();
        let out = html(resting(58.0).words(words));
        assert!(
            out.contains(">न्यूनतम</th>") && out.contains("58 (40 से 120)"),
            "{out}"
        );
    }

    #[test]
    fn the_scale_links_its_sheets() {
        let uses = crate::View::new("x").body(resting(58.0)).uses();
        assert!(uses.has(Uses::CHART_LINEAR) && uses.has(Uses::CHART_PAINT));
        assert!(html(resting(58.0).loading()).contains(r#"data-h="40""#));
    }
}
