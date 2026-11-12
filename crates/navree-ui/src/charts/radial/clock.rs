//! [`DayClock`]: a day as 24 wedges round a clock face, with moments marked on it.

use chrono::NaiveTime;

use super::super::format::NumberFormat;
use super::super::motion::{CountUp, Effect, Motion};
use super::super::paint::{ColorScale, Defs, Gradient, Ink};
use super::super::value::{IntoValue, clean, finite};
use super::super::{
    Color, Common, Drawn, Key, LegendItem, TableData, Tip, TipLine, common_methods, fill, legend,
    n, value_cell,
};
use super::{RadialLabels, Sizes, fit, glyph, polar, wedge, with_unit, wrap};
use crate::components::Tone;
use crate::icons::Icon;
use crate::node::{AttrValue, Node, el};

/// The drawing's own box: 372 units square, its centre in the middle. The hour names sit
/// in its corners of room outside the ring.
const BOX: f64 = 372.0;
const C: f64 = BOX / 2.0;
/// The hub in the middle, and the wedges from just outside it.
const HUB: f64 = 42.0;
const INNER: f64 = 48.0;
/// The dashed ring round it all, and the moments on it: how big a marker is.
const RING: f64 = 136.0;
const MARKS: f64 = RING;
const MARK: f64 = 11.0;
/// The shortest wedge (an hour of nothing) and the longest, which ends just inside the
/// markers on the ring, so a moment never covers a wedge.
const SHORT: f64 = 54.0;
const LONG: f64 = RING - MARK - 4.0;
/// The space between two wedges, in degrees.
const SPACE: f64 = 1.2;
/// The most moments marked; the table lists every one.
pub const MAX_MOMENTS: usize = 8;

/// Something that happened at a time of day, marked on a [`DayClock`] with an icon: a
/// run, a meal, a meeting, waking up.
///
/// ```
/// use navree_ui::charts::{Ink, Moment};
/// use navree_ui::icons;
///
/// let run = Moment::new(6.75, "Morning run").icon(icons::FOOTPRINTS).ink(Ink::slot(3));
/// # let _ = run;
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Moment {
    at: f64,
    name: String,
    icon: Option<Icon>,
    ink: Option<Ink>,
}

impl Moment {
    /// A moment called `name` at `hour` of the day, 0 to 24: `6.75` is a quarter to
    /// seven in the morning.
    pub fn new(hour: f64, name: impl Into<String>) -> Self {
        Self {
            at: if hour.is_finite() {
                hour.rem_euclid(24.0)
            } else {
                0.0
            },
            name: name.into(),
            icon: None,
            ink: None,
        }
    }

    /// The icon in its marker.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The marker's colour: any [`Ink`] of the theme.
    pub fn ink(mut self, ink: impl Into<Ink>) -> Self {
        self.ink = Some(ink.into());
        self
    }

    /// The marker's colour by what it means.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.ink = Some(Ink::tone(tone));
        self
    }

    /// The hour it falls in, 0 to 23.
    fn hour(&self) -> usize {
        (self.at.floor() as usize).min(23)
    }
}

/// What the hub of a [`DayClock`] shows.
#[derive(Clone, Debug, Default)]
enum Hub {
    #[default]
    Total,
    Custom(Node),
}

/// A day round a clock face: one wedge for each of the 24 hours, midnight at the top
/// and running clockwise. A wedge's length is its hour's value (steps, active minutes);
/// its colour is its category (a mood, a kind of activity) or the band of a
/// [`ColorScale`]; how strong it is says its level. Up to eight moments are marked
/// round the outside with icons, the hour that is now is outlined, and the hub holds
/// the day's total.
///
/// It is interactive: one tab stop, the arrow keys move from hour to hour (Home and End
/// to midnight and eleven at night), and a pointer over an hour or its moment shows the
/// hour's tooltip beside it. The 24 hours are its table, and the summary gives the total
/// and the busiest hour.
///
/// ```
/// use navree_ui::charts::{DayClock, Ink, Moment, Motion};
/// use navree_ui::{icons, Tone};
///
/// let steps = [0, 0, 0, 0, 0, 40, 620, 1_850, 900, 420, 380, 510,
///              760, 300, 280, 350, 600, 1_200, 940, 420, 260, 120, 30, 0];
/// let day = DayClock::new("Your day")
///     .values(steps)
///     .unit("steps")
///     .category("Active", Ink::slot(3))
///     .category("Calm", Ink::tone(Tone::named("cyan")))
///     .category("Rest", Ink::slot(7))
///     .categories([2, 2, 2, 2, 2, 2, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 2, 2, 2])
///     .moment(Moment::new(6.5, "Woke up").icon(icons::SUNRISE).ink(Ink::slot(4)))
///     .moment(Moment::new(7.25, "Morning run").icon(icons::FOOTPRINTS).ink(Ink::slot(3)))
///     .now(14)
///     .locale("en-IN")
///     .motion(Motion::new());
/// # let _ = navree_ui::render(&day.into());
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct DayClock {
    common: Common,
    values: Vec<Option<f64>>,
    levels: Option<Vec<Option<f64>>>,
    categories: Vec<(String, Ink)>,
    of: Vec<Option<usize>>,
    scale: Option<ColorScale>,
    ink: Ink,
    moments: Vec<Moment>,
    now: Option<usize>,
    max: Option<f64>,
    format: NumberFormat,
    unit: Option<String>,
    label: Option<String>,
    icon: Option<Icon>,
    hub: Hub,
    hub_ink: Ink,
    size: Sizes,
    motion: Motion,
    words: RadialLabels,
}

common_methods!(DayClock);

impl DayClock {
    /// A clock called `title`, with no values yet.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            common: Common::new(title.into()),
            values: Vec::new(),
            levels: None,
            categories: Vec::new(),
            of: Vec::new(),
            scale: None,
            ink: Ink::slot(1),
            moments: Vec::new(),
            now: None,
            max: None,
            format: NumberFormat::number(),
            unit: None,
            label: None,
            icon: None,
            hub: Hub::Total,
            hub_ink: Ink::slot(7),
            size: Sizes::new(440),
            motion: Motion::none(),
            words: RadialLabels::english(),
        }
    }

    /// The value of each hour from midnight, 24 of them: what sets a wedge's length.
    /// Fewer leave the rest empty; more are not read. A gap (`None`, NaN) is an hour
    /// with no value.
    pub fn values<V: IntoValue>(mut self, values: impl IntoIterator<Item = V>) -> Self {
        self.values = clean(values.into_iter().take(24));
        self
    }

    /// How strong each hour is drawn, 0 to 1: how much of the hour was active, say. By
    /// default it is the hour's value against the busiest hour's.
    pub fn levels<V: IntoValue>(mut self, levels: impl IntoIterator<Item = V>) -> Self {
        self.levels = Some(clean(levels.into_iter().take(24)));
        self
    }

    /// Adds a category called `name` in `ink`: the next one is numbered after it, from
    /// 0. Wedges take theirs from [`categories`](Self::categories), and the legend
    /// names them.
    pub fn category(mut self, name: impl Into<String>, ink: impl Into<Ink>) -> Self {
        self.categories.push((name.into(), ink.into()));
        self
    }

    /// The category of each hour from midnight, by its number: `None` for an hour with
    /// none.
    pub fn categories<K: Into<Option<usize>>>(mut self, of: impl IntoIterator<Item = K>) -> Self {
        self.of = of.into_iter().take(24).map(Into::into).collect();
        self
    }

    /// Colours each hour by the band its value falls in, where it has no category.
    pub fn colors(mut self, scale: ColorScale) -> Self {
        self.scale = Some(scale);
        self
    }

    /// The colour of an hour with no category or band: chart colour 1 by default.
    pub fn ink(mut self, ink: impl Into<Ink>) -> Self {
        self.ink = ink.into();
        self
    }

    /// Marks a moment round the outside. Eight are marked at most, in the order given;
    /// the table lists every one in its hour.
    pub fn moment(mut self, moment: Moment) -> Self {
        self.moments.push(moment);
        self
    }

    /// Outlines the hour that is now, 0 to 23.
    pub fn now(mut self, hour: u32) -> Self {
        self.now = Some((hour % 24) as usize);
        self
    }

    /// The value a full-length wedge stands for: the busiest hour's by default.
    pub fn max(mut self, max: impl IntoValue) -> Self {
        self.max = finite(max.into_value()).filter(|v| *v > 0.0);
        self
    }

    /// How values are written.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.format = format;
        self
    }

    /// What the values count, written after them: `steps`, `min`.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// The small capitals under the total in the hub: the unit by default.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// An icon over the total in the hub.
    pub fn center_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Puts markup of your own in the hub instead of the total, hidden from assistive
    /// technology.
    pub fn center(mut self, content: impl Into<Node>) -> Self {
        self.hub = Hub::Custom(content.into());
        self
    }

    /// The colour the hub is washed with: chart colour 7 by default.
    pub fn hub(mut self, ink: impl Into<Ink>) -> Self {
        self.hub_ink = ink.into();
        self
    }

    /// The widest the drawing grows, in pixels, 200 to 480 (440 by default): it fills a
    /// narrower space, so the clock is as wide as its container up to this.
    pub fn size(mut self, px: u32) -> Self {
        self.size.base = px.clamp(200, 480);
        self
    }

    /// Another width from `screen` up.
    pub fn size_on(mut self, screen: super::Screen, px: u32) -> Self {
        self.size.set(screen, px.clamp(200, 480));
        self
    }

    /// How it comes in: the wedges pop in round the clock, then the moments, and the
    /// total counts up; the hour that is now pulses.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
        self
    }

    /// The words it writes past the ones every chart shares.
    pub fn radial_labels(mut self, labels: RadialLabels) -> Self {
        self.words = labels;
        self
    }

    fn into_node(self) -> Node {
        let drawn = self.draw();
        let height = self.size.base;
        super::super::frame(self.common, drawn, "day-clock", height)
    }

    /// An hour's name: `6 AM`, or `06:00` on a 24 hour clock.
    fn hour_name(&self, hour: usize) -> String {
        let time = NaiveTime::from_hms_opt(u32::try_from(hour).unwrap_or(0) % 24, 0, 0)
            .unwrap_or(NaiveTime::MIN);
        self.common.dates.format_time(time, false)
    }

    /// An hour's name on the dial: short, `6 AM`, where a marker says the half of the day.
    fn dial_name(&self, hour: usize) -> String {
        let full = self.hour_name(hour);
        if full.chars().any(char::is_alphabetic) {
            full.replacen(":00", "", 1)
        } else {
            full
        }
    }

    fn draw(&self) -> Option<Drawn> {
        let numbers = &self.common.numbers;
        let labels = &self.common.labels;
        let unit = self.unit.as_deref();
        let fmt = |v: f64| with_unit(&self.format.format(v, numbers), unit);
        let value = |h: usize| self.values.get(h).copied().flatten();
        if (0..24).all(|h| value(h).is_none()) {
            return None;
        }
        let top = self
            .max
            .unwrap_or_else(|| (0..24).filter_map(value).fold(0.0_f64, |a, v| a.max(v)));
        let share = |v: f64| {
            if top > 0.0 {
                (v / top).clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        let category = |h: usize| {
            self.of
                .get(h)
                .copied()
                .flatten()
                .and_then(|k| self.categories.get(k).map(|c| (k, c)))
        };
        let ink_of = |h: usize| -> Ink {
            if let Some((_, (_, ink))) = category(h) {
                return *ink;
            }
            match (&self.scale, value(h)) {
                (Some(scale), Some(v)) => scale.ink_of(v),
                _ => self.ink,
            }
        };
        let kind_of = |h: usize| -> Option<String> {
            if let Some((_, (name, _))) = category(h) {
                return Some(name.clone());
            }
            match (&self.scale, value(h)) {
                (Some(scale), Some(v)) => scale.band_of(v).1.map(str::to_owned),
                _ => None,
            }
        };
        let level = |h: usize| -> f64 {
            match &self.levels {
                Some(levels) => levels
                    .get(h)
                    .copied()
                    .flatten()
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0),
                None => value(h).map_or(0.0, share),
            }
        };

        // The places the tooltip comes up beside: one per hour, before the wedges, so the
        // module finds them first.
        let anchors: Vec<Node> = (0..24)
            .map(|h| {
                let (x, y) = polar(C, C, (INNER + LONG) / 2.0, h as f64 * 15.0 + 7.5);
                el("circle")
                    .slot("clock-anchor")
                    .attr("data-i", h as i64)
                    .attr("cx", n(x, 2))
                    .attr("cy", n(y, 2))
                    .attr("r", 1_i64)
                    .into()
            })
            .collect();
        let wedges: Vec<Node> = (0..24)
            .map(|h| {
                let v = value(h);
                let outer = SHORT + (LONG - SHORT) * v.map_or(0.0, share);
                let d = wedge(
                    C,
                    C,
                    INNER,
                    outer,
                    h as f64 * 15.0 + SPACE / 2.0,
                    15.0 - SPACE,
                );
                let strength = if v.is_some_and(|v| v > 0.0) {
                    0.22 + 0.6 * level(h)
                } else {
                    0.08
                };
                let mark = ink_of(h)
                    .on(el("path").slot("clock-wedge"))
                    .attr("data-i", h as i64)
                    .attr("d", d)
                    .attr("fill-opacity", n(strength, 2))
                    .flag("data-now", self.now == Some(h))
                    .flag("data-pulse", self.now == Some(h) && self.motion.is_on());
                self.motion.stagger(20).apply(mark, Effect::Pop, h).into()
            })
            .collect();
        let dial = el("g").slot("clock-dial").children((
            el("circle")
                .slot("clock-ring")
                .attr("cx", n(C, 2))
                .attr("cy", n(C, 2))
                .attr("r", n(RING, 2)),
            [0_usize, 6, 12, 18]
                .iter()
                .map(|h| {
                    let (x, y, anchor) = match h {
                        0 => (C, 15.0, "middle"),
                        6 => (BOX - 2.0, C + 4.0, "end"),
                        12 => (C, BOX - 4.0, "middle"),
                        _ => (2.0, C + 4.0, "start"),
                    };
                    el("text")
                        .slot("clock-hour")
                        .attr("x", n(x, 2))
                        .attr("y", n(y, 2))
                        .attr("text-anchor", anchor)
                        .child(self.dial_name(*h))
                        .into()
                })
                .collect::<Vec<Node>>(),
        ));
        let dial = self.motion.apply(dial, Effect::Fade, 0);
        let mut defs = Defs::new(&self.common.id());
        let wash = defs.gradient(
            &Gradient::radial()
                .stop_with_opacity(0.0, Ink::own(), 0.22)
                .stop_with_opacity(1.0, Ink::own(), 0.05),
            self.hub_ink.color(),
            None,
        );
        let hub = self
            .hub_ink
            .on(el("circle").slot("clock-hub"))
            .attr("cx", n(C, 2))
            .attr("cy", n(C, 2))
            .attr("r", n(HUB, 2))
            .attr("fill", wash);
        let now = self.now.map(|h| {
            let outer = SHORT + (LONG - SHORT) * value(h).map_or(0.0, share);
            el("path")
                .slot("clock-now")
                .attr(
                    "d",
                    wedge(C, C, INNER - 3.0, outer + 3.0, h as f64 * 15.0, 15.0),
                )
                .flag("data-pulse", self.motion.is_on())
        });
        let marked: Vec<Node> = self
            .moments
            .iter()
            .take(MAX_MOMENTS)
            .enumerate()
            .map(|(at, m)| {
                let (x, y) = polar(C, C, MARKS, m.at * 15.0);
                let ink = m.ink.unwrap_or_else(|| ink_of(m.hour()));
                let side = MARK * 1.2;
                let mark = ink
                    .on(el("g").slot("clock-moment"))
                    .attr("data-i", m.hour() as i64)
                    .children((
                        el("circle")
                            .attr("cx", n(x, 2))
                            .attr("cy", n(y, 2))
                            .attr("r", n(MARK, 2)),
                        m.icon.map(|icon| {
                            glyph(icon)
                                .attr("x", n(x - side / 2.0, 2))
                                .attr("y", n(y - side / 2.0, 2))
                                .attr("width", n(side, 2))
                                .attr("height", n(side, 2))
                        }),
                    ));
                self.motion
                    .after(700)
                    .stagger(80)
                    .apply(mark, Effect::Pop, at)
                    .into()
            })
            .collect();
        let drawing = super::plot(BOX, BOX).children((
            defs.node(),
            dial,
            hub,
            el("g").slot("clock-anchors").child(anchors),
            el("g").slot("clock-wedges").child(wedges),
            now,
            el("g").slot("clock-moments").child(marked),
        ));

        let total: f64 = (0..24).filter_map(value).sum();
        let total_text = self.format.format(total, numbers);
        let label = self.label.clone().or_else(|| self.unit.clone());
        let middle = match &self.hub {
            Hub::Total => el("div")
                .slot("radial-center")
                .attr("data-fit", fit(&total_text, None))
                .children((
                    self.icon.map(glyph),
                    el("span").slot("radial-value").child(
                        CountUp::new(total, total_text.clone()).motion(self.motion.after(300)),
                    ),
                    label.map(|l| el("span").slot("radial-label").child(l)),
                )),
            Hub::Custom(node) => el("div").slot("radial-center").child(node.clone()),
        }
        .attr("aria-hidden", "true");
        let body = wrap(
            "clock",
            self.size.sizers(|w| w),
            drawing,
            Some(self.hub_ink.on(middle)),
        );

        // The legend: the categories, or the bands of the scale.
        let items: Vec<LegendItem> = if self.categories.is_empty() {
            self.scale
                .as_ref()
                .map(ColorScale::labelled)
                .unwrap_or_default()
                .into_iter()
                .map(|(ink, name)| LegendItem {
                    name,
                    color: ink.color(),
                    key: Key::Block,
                })
                .collect()
        } else {
            self.categories
                .iter()
                .map(|(name, ink)| LegendItem {
                    name: name.clone(),
                    color: ink.color(),
                    key: Key::Block,
                })
                .collect()
        };
        let id = self.common.id();
        let legend = (!items.is_empty()).then(|| legend(&id, &items, false));

        // The table: an hour a row, the tooltip's lines its columns.
        let kinds = !self.categories.is_empty() || self.scale.is_some();
        let has_moments = !self.moments.is_empty();
        // The value's column is named by the hub's label, the unit, or Value; its tooltip
        // line by the same words.
        let value_name = self
            .label
            .clone()
            .or_else(|| self.unit.clone())
            .unwrap_or_else(|| labels.value.to_string());
        let mut head: Vec<(String, Option<Color>)> =
            vec![(labels.time.to_string(), None), (value_name.clone(), None)];
        let mut lines = vec![TipLine {
            name: value_name,
            unit: self.unit.clone(),
            color: Some(self.ink.color()),
            key: false,
        }];
        if kinds {
            head.push((labels.category.to_string(), None));
            lines.push(TipLine {
                name: labels.category.to_string(),
                unit: None,
                color: None,
                key: true,
            });
        }
        if has_moments {
            head.push((self.words.moments.to_string(), None));
            lines.push(TipLine {
                name: self.words.moments.to_string(),
                unit: None,
                color: None,
                key: false,
            });
        }
        let rows = (0..24)
            .map(|h| {
                let ink = ink_of(h);
                let mut cells = vec![match value(h) {
                    // The number alone: the tooltip writes the unit after it.
                    Some(v) => ink
                        .on(el("span"))
                        .child(self.format.format(v, numbers))
                        .into(),
                    None => value_cell(None, labels),
                }];
                if kinds {
                    cells.push(match kind_of(h) {
                        Some(kind) => ink.on(el("span")).child(kind).into(),
                        None => value_cell(None, labels),
                    });
                }
                if has_moments {
                    let names: Vec<&str> = self
                        .moments
                        .iter()
                        .filter(|m| m.hour() == h)
                        .map(|m| m.name.as_str())
                        .collect();
                    cells.push(value_cell(
                        (!names.is_empty()).then(|| names.join(", ")),
                        labels,
                    ));
                }
                let name = self.hour_name(h);
                let name = if self.now == Some(h) {
                    fill(&self.words.now, &[("time", &name)])
                } else {
                    name
                };
                (name, cells)
            })
            .collect();

        let (peak_hour, peak) = (0..24).filter_map(|h| value(h).map(|v| (h, v))).fold(
            (0, f64::NEG_INFINITY),
            |best, (h, v)| {
                if v > best.1 { (h, v) } else { best }
            },
        );
        let mut summary = fill(
            &self.words.summary_clock,
            &[
                ("total", &fmt(total)),
                ("peak", &fmt(peak)),
                ("peak_x", &self.hour_name(peak_hour)),
            ],
        );
        if has_moments {
            summary.push(' ');
            summary.push_str(&fill(
                &self.words.summary_moments,
                &[
                    ("count", &self.moments.len().to_string()),
                    (
                        "names",
                        &self
                            .moments
                            .iter()
                            .map(|m| m.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                ],
            ));
        }
        let note = (self.moments.len() > MAX_MOMENTS).then(|| {
            fill(
                &labels.stages_cut,
                &[
                    ("shown", &MAX_MOMENTS.to_string()),
                    ("total", &self.moments.len().to_string()),
                ],
            )
        });
        Some(Drawn {
            legend,
            body: body.into(),
            body_attrs: vec![
                ("data-mode", AttrValue::Static("clock")),
                ("data-rows", AttrValue::Int(24)),
            ],
            summary,
            table: TableData {
                head,
                rows,
                links: Vec::new(),
                note: None,
            },
            height: self.size.base,
            note,
            interactive: true,
            tip: Some(Tip { big: true, lines }),
        })
    }
}

crate::node::into_node!(DayClock);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    fn html(clock: DayClock) -> String {
        render(&clock.into())
    }

    fn day() -> DayClock {
        let mut steps = [0_u32; 24];
        steps[7] = 1_800;
        steps[8] = 900;
        steps[18] = 1_200;
        DayClock::new("Your day")
            .id("day")
            .values(steps)
            .unit("steps")
    }

    #[test]
    fn there_are_24_wedges_and_the_busiest_is_the_longest() {
        let out = html(day());
        assert_eq!(
            out.matches(r#"data-slot="clock-wedge""#).count(),
            24,
            "{out}"
        );
        // Seven in the morning, the busiest: from 105.6 degrees to 119.4, out to 114.
        let seven = wedge(C, C, INNER, LONG, 7.0 * 15.0 + SPACE / 2.0, 15.0 - SPACE);
        assert!(out.contains(&seven), "{out}");
        // Midnight, an hour of nothing: a short stub, faint.
        let midnight = wedge(C, C, INNER, SHORT, SPACE / 2.0, 15.0 - SPACE);
        assert!(
            out.contains(&format!(r#"d="{midnight}" fill-opacity="0.08""#)),
            "{out}"
        );
    }

    #[test]
    fn it_is_one_tab_stop_with_an_hour_a_row() {
        let out = html(day());
        assert!(out.contains("data-ui-chart"), "{out}");
        assert!(out.contains(r#"data-rows="24""#), "{out}");
        assert_eq!(
            out.matches("<tr>").count(),
            25,
            "a head and 24 hours: {out}"
        );
        assert!(out.contains("<th scope=\"row\">7:00 AM</th>"), "{out}");
        assert!(
            out.contains("3,900 steps in the day. Busiest at 7:00 AM: 1,800 steps."),
            "{out}"
        );
        // Each hour's anchor comes before its wedge, for the tooltip.
        let anchor = out
            .find(r#"data-slot="clock-anchor" data-i="7""#)
            .unwrap_or(usize::MAX);
        let wedge = out.find(r#"data-i="7" d="#).unwrap_or(0);
        assert!(anchor < wedge);
    }

    #[test]
    fn the_dial_names_four_hours_and_the_now_hour_is_outlined() {
        let out = html(day().now(14));
        for hour in ["12 AM", "6 AM", "12 PM", "6 PM"] {
            assert!(out.contains(&format!(">{hour}</text>")), "{hour}: {out}");
        }
        assert!(out.contains(r#"data-slot="clock-now""#), "{out}");
        assert!(out.contains("2:00 PM, now"), "{out}");
        let de = html(day().locale("de"));
        assert!(de.contains(">06:00</text>"), "{de}");
    }

    #[test]
    fn categories_colour_wedges_and_name_them_in_the_legend_and_table() {
        let out = html(
            day()
                .category("Active", Ink::slot(3))
                .category("Rest", Ink::slot(7))
                .categories([Some(1), Some(1), None, None, None, None, None, Some(0)]),
        );
        assert!(
            out.contains(r#"<path data-slot="clock-wedge" data-color="3" data-i="7""#),
            "{out}"
        );
        assert!(
            out.contains(r#"<path data-slot="clock-wedge" data-color="7" data-i="0""#),
            "{out}"
        );
        assert!(
            out.contains(r#"data-slot="chart-legend""#) && out.contains(">Active<"),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-color="3">Active</span>"#),
            "{out}"
        );
    }

    #[test]
    fn moments_are_marked_with_icons_and_listed_in_their_hour() {
        let mut clock = day();
        for k in 0..9 {
            clock = clock.moment(
                Moment::new(f64::from(k) * 2.0 + 0.5, format!("M{k}")).icon(crate::icons::HEART),
            );
        }
        let out = html(clock);
        assert_eq!(
            out.matches(r#"data-slot="clock-moment""#).count(),
            MAX_MOMENTS,
            "{out}"
        );
        assert!(out.contains(r#"data-slot="radial-icon""#), "{out}");
        assert!(out.contains(">M8<"), "the ninth is in the table: {out}");
        assert!(out.contains("9 moments: M0"), "{out}");
        assert!(out.contains("The first 8 of 9"), "{out}");
    }

    #[test]
    fn the_hub_holds_the_total_written_for_the_locale() {
        let mut big = [0_u32; 24];
        big[10] = 123_456;
        let out = html(
            DayClock::new("Steps")
                .values(big)
                .unit("steps")
                .locale("en-IN"),
        );
        assert!(out.contains(">1,23,456</span>"), "{out}");
        assert!(out.contains(r#"data-slot="radial-label">steps<"#), "{out}");
    }

    #[test]
    fn an_empty_day_is_the_empty_state() {
        let out = html(DayClock::new("Your day"));
        assert!(out.contains(r#"data-state="empty""#), "{out}");
        let out = html(DayClock::new("Your day").values([None::<f64>; 24]));
        assert!(out.contains(r#"data-state="empty""#), "{out}");
    }

    #[test]
    fn motion_pops_the_wedges_round_the_clock_and_pulses_now() {
        let out = html(day().now(9).motion(Motion::new()));
        assert!(out.contains(r#"data-motion="pop""#), "{out}");
        assert!(out.contains("data-pulse"), "{out}");
        let still = html(day().now(9));
        assert!(
            !still.contains("data-motion") && !still.contains("data-pulse"),
            "{still}"
        );
    }
}
