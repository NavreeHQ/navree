//! [`Meter`]: one bar split into segments that add up to the whole.

use super::format::{NumberFormat, share_text};
use super::linear::{Entry, LinearLabels, MAX_PARTS, Shares, Unit, legend_list, say};
use super::motion::{Effect, Motion};
use super::paint::{Defs, Gradient, Ink};
use super::value::{IntoValue, finite};
use super::{
    Common, Drawn, TableData, TableMode, Tip, TipLine, common_methods, frame, n, pct, svg,
    value_cell,
};
use crate::node::{AttrValue, Node, Uses, el};

/// One segment of a [`Meter`]: a name and the amount it holds.
///
/// ```
/// use navree_ui::charts::{Gradient, Ink, Segment};
/// use navree_ui::Tone;
///
/// let deep = Segment::new("Deep", 83).ink(Ink::slot(7));
/// let rem = Segment::new("REM", 75)
///     .ink(Ink::tone(Tone::named("cyan")))
///     .fill(Gradient::horizontal().stop(0.0, Ink::own()).stop(1.0, Ink::own().vivid()))
///     .text("1 h 15 min");
/// # let _ = (deep, rem);
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Segment {
    name: String,
    value: Option<f64>,
    ink: Option<Ink>,
    fill: Option<Gradient>,
    text: Option<String>,
}

impl Segment {
    /// A segment called `name` that holds `value`. A value that is missing, zero or
    /// below zero draws nothing and stays in the legend and the table.
    pub fn new(name: impl Into<String>, value: impl IntoValue) -> Self {
        Self {
            name: name.into(),
            value: finite(value.into_value()),
            ink: None,
            fill: None,
            text: None,
        }
    }

    /// Colours the segment with any [`Ink`] of the theme. Without one, segments take the
    /// categorical colours in turn.
    pub fn ink(mut self, ink: impl Into<Ink>) -> Self {
        self.ink = Some(ink.into());
        self
    }

    /// Paints the segment with `gradient` instead of its plain colour;
    /// [`Ink::own`] in its stops is the segment's colour.
    pub fn fill(mut self, gradient: Gradient) -> Self {
        self.fill = Some(gradient);
        self
    }

    /// Writes the segment's value as `text` in the legend, the tooltip and the table
    /// (`1 h 23 min`, `₹1,20,000`), instead of the meter's format.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }
}

/// One bar split into segments, drawn as 100%: how a night divides into sleep stages,
/// how a workout divides into heart-rate zones, where an order stands. Each segment can
/// say its share inside the bar when it is wide enough, its name, value and share are in
/// the legend under the bar, and hovering or moving to a segment names it in a tooltip.
///
/// ```
/// use navree_ui::charts::{Ink, Meter, Motion, Segment};
/// use navree_ui::Tone;
///
/// let stages = Meter::new("Sleep stages, last night")
///     .minutes()
///     .gradient()
///     .gloss()
///     .motion(Motion::new())
///     .segment(Segment::new("Deep", 83).ink(Ink::slot(7)))
///     .segment(Segment::new("REM", 74).ink(Ink::tone(Tone::named("cyan"))))
///     .segment(Segment::new("Core", 158).ink(Ink::slot(3)))
///     .segment(Segment::new("Awake", 16).ink(Ink::slot(4)));
/// let html = navree_ui::render(&stages.into());
/// assert!(html.contains("1 h 23 min"), "{html}");
/// ```
///
/// **Compact.** [`compact`](Self::compact) draws the bar 16 px high with no labels and no
/// legend, for a cell of a grid; the table stays for a screen reader.
///
/// **No script needed.** The bar, its labels and its legend are written in the page. The
/// charts module only adds the tooltip and the keys, and the bar grows in only when
/// [`motion`](Self::motion) is set and the reader hasn't asked for less.
#[derive(Clone, Debug)]
#[must_use]
pub struct Meter {
    common: Common,
    words: LinearLabels,
    segments: Vec<Segment>,
    unit: Unit,
    fill: Option<Gradient>,
    gloss: bool,
    height: u32,
    compact: bool,
    pill: bool,
    label_from: f64,
    legend: Option<bool>,
    value_name: Option<String>,
    motion: Motion,
    rtl: bool,
}

common_methods!(Meter);

/// The segment as drawn: after the fold, with its colour.
struct Part {
    name: String,
    value: Option<f64>,
    ink: Ink,
    fill: Option<Gradient>,
    text: Option<String>,
}

impl Meter {
    /// A meter called `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            common: Common::new(title.into()),
            words: LinearLabels::english(),
            segments: Vec::new(),
            unit: Unit::Number(NumberFormat::number()),
            fill: None,
            gloss: false,
            height: 24,
            compact: false,
            pill: false,
            label_from: 15.0,
            legend: None,
            value_name: None,
            motion: Motion::none(),
            rtl: false,
        }
    }

    /// Adds a segment.
    pub fn segment(mut self, segment: Segment) -> Self {
        self.segments.push(segment);
        self
    }

    /// Adds segments.
    pub fn segments(mut self, segments: impl IntoIterator<Item = Segment>) -> Self {
        self.segments.extend(segments);
        self
    }

    /// How values are written.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.unit = Unit::Number(format);
        self
    }

    /// Values are minutes, written as durations: `45 min`, `1 h 23 min`, `2 h`. The words
    /// come from [`words`](Self::words).
    pub fn minutes(mut self) -> Self {
        self.unit = Unit::Minutes;
        self
    }

    /// Paints every segment with `gradient` (a segment's own [`fill`](Segment::fill)
    /// wins); [`Ink::own`] in its stops is the segment's colour.
    pub fn fill(mut self, gradient: Gradient) -> Self {
        self.fill = Some(gradient);
        self
    }

    /// Paints every segment from a softer shade of its colour to the full colour, left
    /// to right.
    pub fn gradient(self) -> Self {
        self.fill(
            Gradient::horizontal()
                .stop_with_opacity(0.0, Ink::own(), 0.7)
                .stop(1.0, Ink::own()),
        )
    }

    /// A soft highlight along the top of the bar.
    pub fn gloss(mut self) -> Self {
        self.gloss = true;
        self
    }

    /// The bar's thickness in pixels, 8 to 48 (24 by default).
    pub fn height(mut self, px: u32) -> Self {
        self.height = px.clamp(8, 48);
        self
    }

    /// The small form for a cell of a grid: 16 px high, no labels inside, no legend, and
    /// the table for a screen reader only. [`height`](Self::height),
    /// [`legend`](Self::legend) and [`table`](Self::table) after it change that.
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self.height = 16;
        self.common.table = TableMode::ScreenReader;
        self.common.hide_title = true;
        self
    }

    /// Rounds both ends into a pill. Corners are 4 px otherwise.
    pub fn pill(mut self) -> Self {
        self.pill = true;
        self
    }

    /// The smallest share, in percent, that is written inside its segment (15 by default).
    /// A bar under 18 px high writes none.
    pub fn label_from(mut self, percent: f64) -> Self {
        self.label_from = if percent.is_finite() {
            percent.clamp(0.0, 100.0)
        } else {
            15.0
        };
        self
    }

    /// Shows or hides the legend under the bar. It is shown, except on a
    /// [compact](Self::compact) meter.
    pub fn legend(mut self, shown: bool) -> Self {
        self.legend = Some(shown);
        self
    }

    /// The name of the value column of the table and the tooltip (`Duration`); the
    /// chart's word for a value otherwise.
    pub fn value_name(mut self, name: impl Into<String>) -> Self {
        self.value_name = Some(name.into());
        self
    }

    /// How the segments come in: they grow from the start, one after another, and their
    /// labels fade in after them. Off by default; always off for a reader who asks for
    /// less motion.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
        self
    }

    /// Runs the bar from the right, for a right-to-left page.
    pub fn rtl(mut self) -> Self {
        self.rtl = true;
        self
    }

    /// The words the chart writes besides the common ones: the units of a duration.
    pub fn words(mut self, words: LinearLabels) -> Self {
        self.words = words;
        self
    }

    fn into_node(self) -> Node {
        let mut this = self;
        let height = this.height + if this.compact { 8 } else { 48 };
        let drawn = this.draw();
        frame(this.common, drawn, "meter", height)
    }

    /// The segments with colours, and the surplus folded into Other.
    fn parts(&self) -> Vec<Part> {
        let mut parts: Vec<Part> = self
            .segments
            .iter()
            .enumerate()
            .map(|(at, s)| Part {
                name: s.name.clone(),
                value: s.value,
                ink: s.ink.unwrap_or_else(|| Ink::nth(at)),
                fill: s.fill.clone(),
                text: s.text.clone(),
            })
            .collect();
        if parts.len() > MAX_PARTS {
            // The first keep their places and colours; the rest are added up.
            let rest = parts.split_off(MAX_PARTS - 1);
            let sum: f64 = rest
                .iter()
                .filter_map(|p| p.value)
                .filter(|v| *v > 0.0)
                .sum();
            parts.push(Part {
                name: self.common.labels.other.to_string(),
                value: Some(sum),
                ink: Ink::muted(),
                fill: None,
                text: None,
            });
        }
        parts
    }

    fn draw(&mut self) -> Option<Drawn> {
        let labels = self.common.labels.clone();
        let numbers = self.common.numbers.clone();
        let parts = self.parts();
        let values: Vec<Option<f64>> = parts.iter().map(|p| p.value).collect();
        let shares = Shares::of(&values)?;
        let h = self.height;
        let hf = f64::from(h);
        let id = self.common.id();
        let mut defs = Defs::new(&id);
        let gloss_id = format!("{id}-gloss");
        let write = |p: &Part| match (&p.text, p.value) {
            (Some(text), _) => text.clone(),
            (None, Some(v)) => self.unit.text(v, &numbers, &self.words),
            (None, None) => "–".to_owned(),
        };
        let inside = !self.compact && h >= 18;

        // The segments, from the start edge.
        let mut start = 0.0_f64;
        let mut cells: Vec<Node> = Vec::new();
        let mut gradients: Vec<(Ink, bool, String)> = Vec::new();
        for (at, p) in parts.iter().enumerate() {
            let fraction = shares.fraction(p.value);
            if fraction <= 0.0 {
                continue;
            }
            let width = fraction * 100.0;
            let x = if self.rtl {
                100.0 - start - width
            } else {
                start
            };
            let mut rect = p.ink.on(el("rect")
                .slot("seg")
                .attr("x", pct(x))
                .attr("width", pct(width))
                .attr("y", "0")
                .attr("height", i64::from(h)));
            // A gradient is made once for each colour (and for each segment of its own).
            let own = p.fill.is_some();
            let gradient = p.fill.as_ref().or(self.fill.as_ref());
            if let Some(g) = gradient {
                let found = if own {
                    None
                } else {
                    gradients
                        .iter()
                        .find(|(i, own, _)| *i == p.ink && !own)
                        .map(|(_, _, url)| url.clone())
                };
                let url = found.unwrap_or_else(|| {
                    let url = defs.gradient(g, p.ink.color(), None);
                    gradients.push((p.ink, own, url.clone()));
                    url
                });
                rect = rect.attr("fill", url);
            }
            let rect = self.motion.apply(rect, Effect::GrowRight, at);
            let label = (inside && fraction * 100.0 >= self.label_from).then(|| {
                let centre = x + width / 2.0;
                let text = p.ink.on(el("text")
                    .slot("seg-label")
                    .attr("x", pct(centre))
                    .attr("y", n(hf / 2.0 + 0.5, 1))
                    .attr("text-anchor", "middle")
                    .attr("dominant-baseline", "central")
                    .child(share_text(fraction, &numbers)));
                self.motion.after(300).apply(text, Effect::Fade, at)
            });
            cells.push(
                el("g")
                    .slot("seg-part")
                    .attr("data-i", at as i64)
                    .children((rect, label))
                    .into(),
            );
            start += width;
        }

        let gloss = self.gloss.then(|| {
            el("rect")
                .slot("seg-gloss")
                .attr("width", "100%")
                .attr("height", i64::from(h))
                .attr("fill", format!("url(#{gloss_id})"))
        });
        let gloss_defs = self.gloss.then(|| {
            el("defs").child(
                el("linearGradient")
                    .id(gloss_id.clone())
                    .attr("x1", "0")
                    .attr("y1", "0")
                    .attr("x2", "0")
                    .attr("y2", "1")
                    .children((
                        el("stop")
                            .slot("seg-gloss-stop")
                            .attr("offset", "0")
                            .attr("stop-opacity", "0.28"),
                        el("stop")
                            .slot("seg-gloss-stop")
                            .attr("offset", "0.65")
                            .attr("stop-opacity", "0"),
                    )),
            )
        });
        let drawing = svg()
            .slot("seg-bar")
            .attr("width", "100%")
            .attr("height", i64::from(h))
            .attr_opt("data-pill", self.pill.then_some(""))
            .children((
                defs.node(),
                gloss_defs,
                el("rect")
                    .slot("seg-track")
                    .attr("width", "100%")
                    .attr("height", i64::from(h)),
                cells,
                gloss,
            ))
            .uses(Uses::CHART_PAINT.with(Uses::CHART_LINEAR));

        // The legend: a key, the name, the value and the share.
        let value_name = self.value_name.clone().unwrap_or_else(|| match self.unit {
            Unit::Minutes => self.words.duration.to_string(),
            Unit::Number(_) => labels.value.to_string(),
        });
        let show_legend = self.legend.unwrap_or(!self.compact);
        let legend = show_legend.then(|| {
            legend_list(
                parts
                    .iter()
                    .enumerate()
                    .map(|(at, p)| Entry {
                        ink: p.ink,
                        name: p.name.clone(),
                        value: Some(match p.value {
                            Some(_) => format!(
                                "{} · {}",
                                write(p),
                                share_text(shares.fraction(p.value), &numbers)
                            ),
                            None => "–".to_owned(),
                        }),
                        index: Some(at),
                    })
                    .collect(),
            )
        });

        let rows: Vec<(String, Vec<Node>)> = parts
            .iter()
            .map(|p| {
                let value: Node = match p.value {
                    Some(_) => p.ink.on(el("span")).child(write(p)).into(),
                    None => value_cell(None, &labels),
                };
                (
                    p.name.clone(),
                    vec![
                        value,
                        value_cell(
                            p.value
                                .map(|_| share_text(shares.fraction(p.value), &numbers)),
                            &labels,
                        ),
                    ],
                )
            })
            .collect();

        let largest = parts
            .iter()
            .max_by(|a, b| {
                shares
                    .fraction(a.value)
                    .total_cmp(&shares.fraction(b.value))
            })
            .map(|p| (p.name.clone(), shares.fraction(p.value)));
        let (name, share) = largest.unwrap_or_default();
        let total = self.unit.text(shares.total(), &numbers, &self.words);
        let summary = say(
            &labels.summary_parts,
            &[
                ("count", &parts.len().to_string()),
                ("total", &total),
                ("name", &name),
                ("share", &share_text(share, &numbers)),
            ],
        );

        Some(Drawn {
            legend: None,
            body: Node::list((drawing, legend)),
            body_attrs: vec![
                ("data-rows", AttrValue::Int(parts.len() as i64)),
                ("data-float", AttrValue::Flag),
            ],
            summary,
            table: TableData {
                head: vec![
                    (labels.category.to_string(), None),
                    (value_name.clone(), None),
                    (labels.share.to_string(), None),
                ],
                rows,
                links: Vec::new(),
                note: None,
            },
            height: h + if show_legend { 40 } else { 0 },
            note: None,
            interactive: true,
            tip: Some(Tip {
                big: true,
                lines: vec![
                    TipLine {
                        name: value_name,
                        unit: None,
                        color: None,
                        key: false,
                    },
                    TipLine {
                        name: labels.share.to_string(),
                        unit: None,
                        color: None,
                        key: false,
                    },
                ],
            }),
        })
    }
}

crate::node::into_node!(Meter);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Tone;
    use crate::render;

    fn sleep() -> Meter {
        Meter::new("Sleep stages")
            .minutes()
            .segment(Segment::new("Deep", 83).ink(Ink::slot(7)))
            .segment(Segment::new("REM", 74).ink(Ink::tone(Tone::named("cyan"))))
            .segment(Segment::new("Core", 158).ink(Ink::slot(3)))
            .segment(Segment::new("Awake", 16).ink(Ink::slot(4)))
    }

    fn html(meter: Meter) -> String {
        render(&meter.into())
    }

    fn count(haystack: &str, needle: &str) -> usize {
        haystack.matches(needle).count()
    }

    #[test]
    fn segments_run_edge_to_edge_in_percent_and_add_up_to_the_whole() {
        let out = html(sleep());
        // 83 + 74 + 158 + 16 = 331.
        assert!(out.contains(r#"x="0%" width="25.076%""#), "{out}");
        assert!(out.contains(r#"x="25.076%" width="22.356%""#), "{out}");
        assert!(out.contains(r#"x="47.432%" width="47.734%""#), "{out}");
        assert!(out.contains(r#"x="95.166%" width="4.834%""#), "{out}");
        assert_eq!(count(&out, r#"data-slot="seg" "#), 4);
        // Each part is one mark for the tooltip and the keys.
        assert_eq!(count(&out, r#"data-slot="seg-part" data-i=""#), 4);
        assert!(out.contains(r#"data-rows="4""#), "{out}");
    }

    #[test]
    fn a_label_is_written_inside_the_segments_of_fifteen_percent_or_more() {
        let out = html(sleep());
        assert_eq!(count(&out, r#"data-slot="seg-label""#), 3, "{out}");
        assert!(
            out.contains(">25%</text>")
                && out.contains(">22%</text>")
                && out.contains(">48%</text>")
        );
        assert!(!out.contains(">5%</text>"), "the 4.8% segment has no label");
        // The threshold moves, and a bar too thin to hold text writes none.
        let out = html(sleep().label_from(4.0));
        assert_eq!(count(&out, r#"data-slot="seg-label""#), 4, "{out}");
        let out = html(sleep().label_from(50.0));
        assert_eq!(count(&out, r#"data-slot="seg-label""#), 0, "{out}");
        let out = html(sleep().height(16));
        assert_eq!(count(&out, r#"data-slot="seg-label""#), 0, "{out}");
        // Labels are for the eye: the legend and the table say the same in words.
        assert!(html(sleep()).contains(r#"<text data-slot="seg-label" x="12.538%""#));
    }

    #[test]
    fn the_legend_gives_durations_and_shares_and_the_table_the_same() {
        let out = html(sleep());
        assert!(out.contains("1 h 23 min · 25%"), "{out}");
        assert!(out.contains("2 h 38 min · 48%"), "{out}");
        assert!(out.contains("16 min · 5%"), "{out}");
        assert!(
            out.contains(r#"<th scope="col" data-num="">Duration</th>"#),
            "{out}"
        );
        assert!(out.contains(r#"<th scope="row">Deep</th><td data-num=""><span data-color="7">1 h 23 min</span></td><td data-num="">25%</td>"#), "{out}");
        assert!(
            out.contains("4 parts, total 5 h 31 min. Largest: Core, 48%."),
            "{out}"
        );
        // The tooltip lists the value large, then the share.
        assert!(
            out.contains(r#"data-big="""#)
                && out.contains("<span>Duration</span>")
                && out.contains("<span>Share</span>")
        );
    }

    #[test]
    fn a_segment_writes_its_value_as_asked_and_the_chart_in_the_readers_digits() {
        let out = html(
            Meter::new("Spend")
                .format(NumberFormat::currency("₹"))
                .locale("en-IN")
                .segment(Segment::new("Rent", 1_200_000))
                .segment(Segment::new("Food", 340_000.5))
                .segment(Segment::new("Other", 90_000).text("₹90K")),
        );
        assert!(out.contains("₹12,00,000 · "), "{out}");
        assert!(out.contains("₹3,40,000.5 · "), "{out}");
        assert!(out.contains("₹90K · "), "text replaces the format: {out}");
        assert!(out.contains("total ₹16,30,000.5"), "{out}");
    }

    #[test]
    fn colours_are_attributes_and_gradients_belong_to_their_chart() {
        let out = html(sleep().gradient());
        assert!(!out.contains("style="), "{out}");
        assert!(
            !out.contains("rgb")
                && !out.contains("hsl")
                && !out.contains("#0")
                && !out.contains("#f"),
            "{out}"
        );
        // One gradient for each colour, named after the chart; the segments point at them.
        assert_eq!(count(&out, "<linearGradient"), 4, "{out}");
        assert!(out.contains(r#"fill="url(#chart-sleep-stages-"#), "{out}");
        assert!(
            out.contains(r#"<stop offset="0" stop-opacity="0.7" data-color="7">"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<stop offset="0" stop-opacity="0.7" data-tone="cyan">"#),
            "{out}"
        );
        let other = html(sleep().gradient().id("night"));
        assert!(other.contains(r#"id="night-p0""#) && other.contains("url(#night-p0)"));
        assert!(!other.contains("chart-sleep-stages"));
        // A plain meter has no gradient, and the sheet colours the segments.
        let plain = html(sleep());
        assert!(!plain.contains("linearGradient"), "{plain}");
        assert!(!plain.contains(r#" fill=""#), "{plain}");
    }

    #[test]
    fn a_segment_can_bring_its_own_gradient_and_colour() {
        let out = html(
            Meter::new("Zones")
                .gradient()
                .segment(
                    Segment::new("Rest", 10).ink(Ink::slot(1)).fill(
                        Gradient::vertical()
                            .stop(0.0, Ink::own())
                            .stop(1.0, Ink::own().soft()),
                    ),
                )
                .segment(Segment::new("Peak", 10).ink(Ink::slot(1))),
        );
        // Two gradients for one colour: the segment's own and the meter's.
        assert_eq!(count(&out, "<linearGradient"), 2, "{out}");
        assert!(out.contains(r#"data-mix="soft""#), "{out}");
        // Segments without a colour take the categorical colours in turn.
        let out = html(
            Meter::new("Auto")
                .segment(Segment::new("a", 1))
                .segment(Segment::new("b", 1)),
        );
        assert!(
            out.contains(r#"data-slot="seg" x="0%" width="50%" y="0" height="24" data-color="1""#),
            "{out}"
        );
        assert!(
            out.contains(r#"width="50%" y="0" height="24" data-color="2""#),
            "{out}"
        );
    }

    #[test]
    fn a_gloss_is_a_gradient_over_the_bar_that_the_stylesheet_colours() {
        let out = html(sleep().gloss());
        assert!(
            out.contains(r#"<stop data-slot="seg-gloss-stop" offset="0" stop-opacity="0.28">"#),
            "{out}"
        );
        assert!(out.contains(r#"<rect data-slot="seg-gloss" width="100%" height="24" fill="url(#chart-sleep-stages-"#), "{out}");
        assert!(!html(sleep()).contains("seg-gloss"));
    }

    #[test]
    fn the_compact_form_is_sixteen_pixels_with_no_labels_legend_or_switch() {
        let out = html(sleep().compact().gradient());
        assert!(out.contains(r#"width="100%" height="16""#), "{out}");
        assert!(
            !out.contains("seg-label") && !out.contains("chart-legend"),
            "{out}"
        );
        assert!(
            !out.contains("chart-table-switch") && !out.contains("Show table"),
            "{out}"
        );
        // The title is for assistive technology, the table still there.
        assert!(
            out.contains(r#"data-slot="chart-header" data-sr="""#),
            "{out}"
        );
        assert!(
            out.contains("<table>") && out.contains("1 h 23 min"),
            "{out}"
        );
        // A legend can be asked for again; so can the title's room.
        assert!(html(sleep().compact().legend(true)).contains("chart-legend"));
        assert!(!html(sleep().legend(false)).contains("chart-legend"));
        assert!(html(sleep().compact().height(20)).contains(r#"height="20""#));
    }

    #[test]
    fn corners_are_four_pixels_and_a_pill_on_request() {
        assert!(!html(sleep()).contains("data-pill"));
        assert!(
            html(sleep().pill())
                .contains(r#"data-slot="seg-bar" width="100%" height="24" data-pill="""#)
        );
    }

    #[test]
    fn motion_grows_each_segment_in_turn_and_is_absent_when_off() {
        let out = html(sleep().motion(Motion::new().stagger(100)));
        assert_eq!(count(&out, r#"data-motion="grow-right""#), 4, "{out}");
        assert_eq!(count(&out, r#"data-motion="fade""#), 3, "{out}");
        assert!(
            out.contains(r#"data-motion="grow-right" data-delay="2""#),
            "{out}"
        );
        assert!(
            out.contains(r#"data-motion="grow-right" data-delay="6""#),
            "{out}"
        );
        // The labels come after their segments.
        assert!(
            out.contains(r#"data-motion="fade" data-delay="6">25%"#),
            "{out}"
        );
        let still = html(sleep());
        assert!(
            !still.contains("data-motion") && !still.contains("data-delay"),
            "{still}"
        );
    }

    #[test]
    fn a_right_to_left_meter_runs_from_the_right() {
        let out = html(sleep().rtl());
        // The first segment ends at the right edge.
        assert!(out.contains(r#"x="74.924%" width="25.076%""#), "{out}");
        assert!(out.contains(r#"x="0%" width="4.834%""#), "{out}");
    }

    #[test]
    fn nothing_to_draw_is_an_empty_chart_and_odd_values_draw_nothing() {
        for none in [
            Meter::new("Empty"),
            Meter::new("Zeros")
                .segment(Segment::new("a", 0))
                .segment(Segment::new("b", 0)),
            Meter::new("Gaps")
                .segment(Segment::new("a", f64::NAN))
                .segment(Segment::new("b", None::<f64>)),
            Meter::new("Negative").segment(Segment::new("a", -5)),
        ] {
            let out = html(none);
            assert!(
                out.contains(r#"data-state="empty""#) && !out.contains("seg-bar"),
                "{out}"
            );
        }
        // A part that draws nothing stays in the legend and the table.
        let out = html(
            sleep()
                .segment(Segment::new("Naps", 0))
                .segment(Segment::new("Unknown", None::<f64>)),
        );
        assert_eq!(count(&out, r#"data-slot="seg-part""#), 4, "{out}");
        assert!(
            out.contains(">Naps</span>") && out.contains(r#"<th scope="row">Unknown</th>"#),
            "{out}"
        );
        assert!(out.contains("0 min · 0%"), "{out}");
        assert!(out.contains("No value"), "{out}");
    }

    #[test]
    fn one_segment_is_the_whole_bar() {
        let out = html(Meter::new("Done").segment(Segment::new("All", 7)));
        assert!(
            out.contains(r#"x="0%" width="100%""#) && out.contains(">100%</text>"),
            "{out}"
        );
    }

    #[test]
    fn a_hairline_segment_still_draws_and_the_largest_numbers_still_add_up() {
        let out = html(
            Meter::new("Big")
                .segment(Segment::new("a", f64::MAX))
                .segment(Segment::new("b", f64::MAX))
                .segment(Segment::new("c", 1)),
        );
        assert!(
            out.contains(r#"width="50%""#) && !out.contains("NaN") && !out.contains("inf"),
            "{out}"
        );
    }

    #[test]
    fn more_than_twelve_segments_fold_the_rest_into_other() {
        let mut meter = Meter::new("Many");
        for at in 0..20 {
            meter = meter.segment(Segment::new(format!("s{at}"), 1));
        }
        let out = html(meter);
        assert_eq!(count(&out, r#"data-slot="seg-part""#), MAX_PARTS, "{out}");
        assert!(
            out.contains(">Other</span>") && out.contains(r#"<th scope="row">Other</th>"#),
            "{out}"
        );
        // 20 parts, of which the last nine make Other: 9 of 20.
        assert!(out.contains("45%"), "{out}");
        assert!(!out.contains(">s19<"), "{out}");
    }

    #[test]
    fn the_figure_is_named_described_and_one_tab_stop() {
        let out = html(sleep());
        assert!(
            out.contains(r#"data-kind="meter""#) && out.contains("data-ui-chart"),
            "{out}"
        );
        assert!(
            out.contains(r#"role="group" aria-label="Sleep stages, chart""#),
            "{out}"
        );
        assert!(
            out.contains("aria-describedby=\"chart-sleep-stages-") && out.contains("-summary\""),
            "{out}"
        );
        // The drawing hides from assistive technology; the keys are told once.
        assert!(
            out.contains(r#"<svg aria-hidden="true" focusable="false" data-slot="seg-bar""#),
            "{out}"
        );
        assert!(
            out.contains("Use the arrow keys to move between values."),
            "{out}"
        );
        // The tooltip floats over the page rather than growing the chart.
        assert!(out.contains(r#"data-rows="4" data-float>"#), "{out}");
    }

    #[test]
    fn a_meter_links_its_sheets_and_the_charts_module() {
        let uses = crate::View::new("x").body(sleep()).uses();
        assert_eq!(
            uses,
            Uses::CHARTS
                .with(Uses::CHART_PAINT)
                .with(Uses::CHART_LINEAR)
        );
        // Compact or not, the bar is the same marks.
        let uses = crate::View::new("x")
            .body(sleep().compact().motion(Motion::new()))
            .uses();
        assert!(uses.has(Uses::CHART_LINEAR) && uses.has(Uses::CHARTS));
    }

    #[test]
    fn other_words_come_from_the_labels() {
        let mut words = LinearLabels::english();
        words.hours = "घं".into();
        words.minutes = "मि".into();
        words.duration = "अवधि".into();
        let out = html(sleep().words(words));
        assert!(
            out.contains("1 घं 23 मि") && out.contains("16 मि") && out.contains(">अवधि</th>"),
            "{out}"
        );
        let out = html(sleep().value_name("Time asleep"));
        assert!(out.contains(">Time asleep</th>"), "{out}");
    }

    #[test]
    fn a_state_keeps_the_height_of_the_meter() {
        let out = html(sleep().loading());
        assert!(
            out.contains(r#"data-state="loading""#) && out.contains(r#"data-h="72""#),
            "{out}"
        );
        let out = html(sleep().compact().loading());
        assert!(out.contains(r#"data-h="24""#), "{out}");
    }
}
