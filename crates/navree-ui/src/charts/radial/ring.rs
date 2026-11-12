//! [`Ring`]: one value on a ring; [`Rings`]: two to four values on rings inside each
//! other. The single value's drawing is shared with [`Gauge`](super::Gauge).

use super::super::format::{NumberFormat, share_text};
use super::super::motion::{Effect, Motion};
use super::super::paint::{Defs, Gradient, Ink};
use super::super::value::{IntoValue, finite};
use super::super::{Common, Drawn, TableData, TableMode, common_methods, fill, n, value_cell};
use super::{
    Band, Middle, Paint, RadialLabels, RingTrack, Screen, Sizes, figure, finish, fit, glyph, plot,
    summary_of, thickness_for, with_unit, wrap,
};
use crate::components::Tone;
use crate::icons::Icon;
use crate::node::{AttrValue, Node, el};

/// The smallest and largest size of a ring, in pixels.
const SMALLEST: u32 = 24;
const LARGEST: u32 = 320;
/// Below this size the label and sublabel go under the ring, not inside it.
const ROOMY: u32 = 96;
/// The most rings drawn inside each other; the table lists every value.
pub(crate) const MAX_RINGS: usize = 4;

fn size_of(px: u32) -> u32 {
    px.clamp(SMALLEST, LARGEST)
}

/// One value on a scale, drawn on a ring or an arc: what [`Ring`] and
/// [`Gauge`](super::Gauge) share.
#[derive(Clone, Debug)]
pub(crate) struct One {
    pub value: Option<f64>,
    pub min: f64,
    pub max: f64,
    pub format: NumberFormat,
    pub unit: Option<String>,
    pub paint: Paint,
    pub halo: bool,
    pub middle: Middle,
    pub label: Option<String>,
    pub sublabel: Option<String>,
    pub sizes: Sizes,
    pub thickness: Option<f64>,
    pub motion: Motion,
    pub inline: bool,
    pub words: RadialLabels,
    /// How far the scale sweeps, in degrees: 360 for a ring.
    pub sweep: f64,
}

impl One {
    pub fn new(value: Option<f64>, size: u32, sweep: f64) -> Self {
        Self {
            value: finite(value),
            min: 0.0,
            max: 100.0,
            format: NumberFormat::number(),
            unit: None,
            paint: Paint::new(None),
            halo: false,
            middle: Middle::Value,
            label: None,
            sublabel: None,
            sizes: Sizes::new(size),
            thickness: None,
            motion: Motion::none(),
            inline: false,
            words: RadialLabels::english(),
            sweep,
        }
    }

    /// The drawing's width and height at `size` pixels wide, and where its centre is.
    fn frame_at(&self, size: f64, thickness: f64, r: f64) -> (f64, f64) {
        let c = size / 2.0;
        if self.sweep >= 360.0 {
            return (size, c);
        }
        // An open arc ends above or below the centre: the drawing stops under its lowest
        // point, the round caps included.
        let low = -(self.sweep / 2.0).to_radians().cos() * r;
        let bottom = c + low.max(0.0) + thickness / 2.0 + 1.0;
        (bottom.min(size), c)
    }

    /// The table's first column, the row's name and the summary's name.
    fn name<'a>(&'a self, common: &'a Common) -> &'a str {
        self.label.as_deref().unwrap_or(&common.title)
    }

    /// The drawing, its summary and its table; `None` when there is no value.
    pub fn draw(&self, common: &Common, kind: &'static str) -> Option<Drawn> {
        let value = self.value?;
        let span = self.max - self.min;
        if !(span.is_finite() && span > 0.0) {
            return None;
        }
        let numbers = &common.numbers;
        let labels = &common.labels;
        let unit = self.unit.as_deref();
        let text = self.format.format(value, numbers);
        let share = (value - self.min) / span;
        let share_pct = share * 100.0;
        let ink = self.paint.ink_at(share_pct);
        let band = self.paint.band_at(share_pct);

        // Geometry, in the drawing's own units: one unit is a pixel at the base size.
        let size = f64::from(self.sizes.base);
        let thickness = self
            .thickness
            .unwrap_or_else(|| thickness_for(size))
            .clamp(1.0, size / 4.0);
        let pad = if self.halo {
            (thickness * 0.45).max(3.0) + 1.5
        } else {
            0.0
        };
        let r = size / 2.0 - thickness / 2.0 - pad;
        let (height, c) = self.frame_at(size, thickness, r);
        let ratio = height / size;
        let open = self.sweep < 360.0;
        let from = if open { -self.sweep / 2.0 } else { 0.0 };

        let mut defs = Defs::new(&common.id());
        let area = [0.0, 0.0, size, size];
        let halo = self.halo.then(|| {
            let halo = el("circle")
                .slot("radial-halo")
                .attr("cx", n(c, 2))
                .attr("cy", n(c, 2))
                .attr("r", n(size / 2.0 - 0.75, 2))
                .attr("stroke-width", "1.5");
            let halo = match &self.paint.fill {
                Some(g) => halo.attr(
                    "stroke",
                    defs.gradient(&g.clone().across_plot(), ink.color(), Some(area)),
                ),
                None => halo,
            };
            self.motion.apply(halo, Effect::Fade, 0)
        });
        let marks = Band {
            cx: c,
            cy: c,
            r,
            thickness,
            from,
            sweep: if open { self.sweep } else { 360.0 },
            share,
            laps: !open,
            ink,
            paint: &self.paint,
            motion: self.motion,
            index: 0,
            area,
        }
        .nodes(&mut defs);
        let drawing = plot(size, height).children((defs.node(), halo, marks));

        // The middle, and the words under a small ring.
        let roomy = self.sizes.base >= ROOMY;
        let words = |slot: &'static str, text: &Option<String>| {
            text.clone().map(|t| el("span").slot(slot).child(t))
        };
        let middle = match &self.middle {
            Middle::Value => Some(
                el("div")
                    .slot("radial-center")
                    .attr("data-fit", fit(&text, unit))
                    .children((
                        figure(value, &text, unit, self.motion),
                        roomy.then(|| words("radial-label", &self.label)),
                        roomy.then(|| words("radial-sub", &self.sublabel)),
                    )),
            ),
            Middle::Custom(node) => Some(el("div").slot("radial-center").child(node.clone())),
            Middle::Icon(icon) => Some(el("div").slot("radial-center").child(glyph(*icon))),
            Middle::Nothing => None,
        };
        let middle = middle.map(|m| {
            let m = m.attr("aria-hidden", "true");
            match (open, self.sweep <= 200.0) {
                (true, true) => m.attr("data-place", "low"),
                (true, false) => m.attr("data-place", "square"),
                _ => m,
            }
        });
        let ring = ink
            .on(wrap(
                kind,
                self.sizes.sizers(|w| (f64::from(w) * ratio).round() as u32),
                drawing,
                middle,
            ))
            .attr_opt("data-glow-room", self.paint.glow.then_some(""));
        let caption = (!roomy && matches!(self.middle, Middle::Value))
            .then(|| {
                (
                    words("radial-label", &self.label),
                    words("radial-sub", &self.sublabel),
                )
            })
            .filter(|(a, b)| a.is_some() || b.is_some())
            .map(|(a, b)| el("div").slot("radial-caption").children((a, b)));
        let body: Node = match caption {
            Some(caption) => el("div")
                .slot("radial-stack")
                .children((ring, caption))
                .into(),
            None => ring.into(),
        };

        // The words: the summary and the table.
        let with = |t: &str| with_unit(t, unit);
        let value_text = with(&text);
        let max_text = with(&self.format.format(self.max, numbers));
        let share_words = share_text(share, numbers);
        let ranged = self.min != 0.0;
        let min_text = ranged.then(|| with(&self.format.format(self.min, numbers)));
        let name = self.name(common);
        let summary = summary_of(
            &self.words,
            name,
            &value_text,
            min_text.as_deref(),
            &max_text,
            &share_words,
            band.as_deref(),
        );
        let mut head = vec![
            (labels.category.to_string(), None),
            (labels.value.to_string(), None),
            (self.words.goal.to_string(), None),
            (labels.share.to_string(), None),
        ];
        let mut cells = vec![
            Node::from(value_text),
            Node::from(max_text),
            Node::from(share_words),
        ];
        if band.is_some() {
            head.push((self.words.status.to_string(), None));
            cells.push(value_cell(band.clone(), labels));
        }
        Some(Drawn {
            legend: None,
            body,
            body_attrs: Vec::new(),
            summary,
            table: TableData {
                head,
                rows: vec![(name.to_owned(), cells)],
                links: Vec::new(),
                note: None,
            },
            height: (f64::from(self.sizes.base) * ratio).round() as u32,
            note: None,
            interactive: false,
            tip: None,
        })
    }
}

/// The builder methods a single value on a ring or an arc has: [`Ring`] and
/// [`Gauge`](super::Gauge).
macro_rules! radial_methods {
    ($($type:ident),+ $(,)?) => {
        $(impl $type {
            /// The top of the scale: the goal the value is measured against (100 by
            /// default). A value past it draws a second lap on a ring.
            pub fn max(mut self, max: impl $crate::charts::IntoValue) -> Self {
                if let Some(max) = max.into_value().filter(|v| v.is_finite()) {
                    self.one.max = max;
                }
                self
            }

            /// How the value is written in the middle, the summary and the table.
            pub fn format(mut self, format: $crate::charts::NumberFormat) -> Self {
                self.one.format = format;
                self
            }

            /// What the value counts, written after it: `%`, `kcal`, `bpm`. A word gets a
            /// space before it; a sign such as `%` or `°C` doesn't.
            pub fn unit(mut self, unit: impl Into<String>) -> Self {
                self.one.unit = Some(unit.into());
                self
            }

            /// Colours the arc with any [`Ink`]($crate::charts::Ink) of the theme.
            pub fn ink(mut self, ink: impl Into<$crate::charts::Ink>) -> Self {
                self.one.paint.ink = Some(ink.into());
                self
            }

            /// Colours the arc by what it means.
            pub fn tone(mut self, tone: $crate::components::Tone) -> Self {
                self.one.paint.ink = Some($crate::charts::Ink::tone(tone));
                self
            }

            /// Paints the arc with `gradient`, which spans the whole ring, so the colour
            /// moves round it. [`Ink::own`]($crate::charts::Ink::own) in its stops is the
            /// arc's own colour: `Gradient::sheen(Ink::own())` turns it brighter along the
            /// diagonal.
            pub fn fill(mut self, gradient: $crate::charts::Gradient) -> Self {
                self.one.paint.fill = Some(gradient);
                self
            }

            /// Colours the arc by the band its share of the scale (0 to 100) falls in.
            /// The band's label is said in the summary and the table, so the colour is
            /// never alone.
            pub fn colors(mut self, scale: $crate::charts::ColorScale) -> Self {
                self.one.paint.scale = Some(scale);
                self
            }

            /// What the track under the arc is drawn in: a faint grey (the default), a
            /// faint tint of the arc's colour, or nothing.
            pub fn track(mut self, track: $crate::charts::RingTrack) -> Self {
                self.one.paint.track = track;
                self
            }

            /// Puts a soft glow of its own colour around the arc.
            pub fn glow(mut self) -> Self {
                self.one.paint.glow = true;
                self
            }

            /// Draws a thin halo ring just outside the track, in the arc's colour or
            /// gradient.
            pub fn halo(mut self) -> Self {
                self.one.halo = true;
                self
            }

            /// Small capitals under the value: what it is, such as `Sleep score`. Under a
            /// ring smaller than 96 pixels it is written under the ring. It also names the
            /// row of the table.
            pub fn label(mut self, label: impl Into<String>) -> Self {
                self.one.label = Some(label.into());
                self
            }

            /// A quieter line under the label: `Great recovery`.
            pub fn sublabel(mut self, sublabel: impl Into<String>) -> Self {
                self.one.sublabel = Some(sublabel.into());
                self
            }

            /// Puts markup of your own in the middle instead of the value. It is hidden
            /// from assistive technology, which reads the summary: say what it says there.
            pub fn center(mut self, content: impl Into<$crate::node::Node>) -> Self {
                self.one.middle = $crate::charts::radial::Middle::Custom(content.into());
                self
            }

            /// Puts an icon in the middle instead of the value.
            pub fn center_icon(mut self, icon: $crate::icons::Icon) -> Self {
                self.one.middle = $crate::charts::radial::Middle::Icon(icon);
                self
            }

            /// Leaves the middle empty.
            pub fn center_none(mut self) -> Self {
                self.one.middle = $crate::charts::radial::Middle::Nothing;
                self
            }

            /// The drawing's width in pixels, 24 to 320.
            pub fn size(mut self, px: u32) -> Self {
                self.one.sizes.base = px.clamp(24, 320);
                self
            }

            /// Another width from `screen` up: `size(112).size_on(Screen::Md, 128)`. The
            /// arc, its thickness and the words in the middle all scale with it.
            pub fn size_on(mut self, screen: $crate::charts::Screen, px: u32) -> Self {
                self.one.sizes.set(screen, px.clamp(24, 320));
                self
            }

            /// The arc's thickness in pixels at the base size. By default it grows with
            /// the size, a little thicker for its size when small.
            pub fn thickness(mut self, px: f64) -> Self {
                if px.is_finite() && px > 0.0 {
                    self.one.thickness = Some(px);
                }
                self
            }

            /// How the arc comes in: it draws on from the start, and the value counts up.
            /// Off by default, and always off for a reader who asks for less motion;
            /// [`Motion::none`]($crate::charts::Motion::none) is the still drawing an
            /// image is made from.
            pub fn motion(mut self, motion: $crate::charts::Motion) -> Self {
                self.one.motion = motion;
                self
            }

            /// Draws it small for a pill, a button or a row: the picture alone in a span,
            /// named by its summary, with no figure, heading or table.
            pub fn inline(mut self) -> Self {
                self.one.inline = true;
                self
            }

            /// The words it writes past the ones every chart shares.
            pub fn radial_labels(mut self, labels: $crate::charts::RadialLabels) -> Self {
                self.one.words = labels;
                self
            }
        })+
    };
}
pub(crate) use radial_methods;

/// One value on a ring: a score out of 100, steps toward a goal, a share. The ring
/// starts at twelve o'clock and runs clockwise; past its goal it draws a second lap.
///
/// The middle holds the value (counting up when it comes in), with small capitals for
/// what it is and a quieter line under them; or markup of your own, an icon, or
/// nothing. The arc is a colour, a gradient round the ring, or the colour of the band
/// its value is in, with a track under it, a glow and a halo if asked.
///
/// It is a picture for assistive technology, named by its title and described by its
/// summary (`Sleep score: 82 of 100, 82%.`), with the value in a table only a screen
/// reader reads; the title is shown by [`show_title`](Self::show_title).
///
/// ```
/// use navree_ui::charts::{ColorScale, Gradient, Ink, Motion, Ring, Screen};
/// use navree_ui::Tone;
///
/// // A score, its colour by band.
/// let recovery = Ring::new("Recovery", 87)
///     .label("Recovery")
///     .unit("%")
///     .colors(
///         ColorScale::new(Ink::tone(Tone::Danger))
///             .label("Low")
///             .from(34, Ink::tone(Tone::Warning))
///             .label("Fair")
///             .from(67, Ink::tone(Tone::Success))
///             .label("Recovered"),
///     )
///     .glow()
///     .size(184);
/// // Steps toward a goal, written the Indian way.
/// let steps = Ring::new("Steps", 12_450)
///     .max(10_000)
///     .locale("en-IN")
///     .fill(Gradient::sheen(Ink::slot(3)))
///     .size(112)
///     .size_on(Screen::Md, 128)
///     .motion(Motion::new());
/// # let _ = (navree_ui::render(&recovery.into()), navree_ui::render(&steps.into()));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Ring {
    common: Common,
    one: One,
}

common_methods!(Ring);
radial_methods!(Ring);

impl Ring {
    /// A ring called `title` holding `value`, out of 100 unless [`max`](Self::max)
    /// says otherwise. A value that is missing or not a number draws the empty state.
    pub fn new(title: impl Into<String>, value: impl IntoValue) -> Self {
        let mut common = Common::new(title.into());
        common.hide_title = true;
        common.table = TableMode::ScreenReader;
        Self {
            common,
            one: One::new(value.into_value(), 120, 360.0),
        }
    }

    /// Shows the title over the ring, as a chart's caption, and the switch that shows
    /// its table.
    pub fn show_title(mut self) -> Self {
        self.common.hide_title = false;
        self.common.table = TableMode::Switch;
        self
    }

    fn into_node(self) -> Node {
        let drawn = self.one.draw(&self.common, "ring");
        let height = drawn.as_ref().map_or(self.one.sizes.base, |d| d.height);
        finish(self.common, drawn, "ring", height, self.one.inline)
    }
}

crate::node::into_node!(Ring);

/// One value of [`Rings`]: a name, a value and the goal it is measured against, with its
/// own colour, gradient and track.
///
/// ```
/// use navree_ui::charts::{Gradient, Ink, RingValue};
///
/// let movement = RingValue::new("Move", 540).max(600).unit("kcal").ink(Ink::slot(8));
/// let exercise = RingValue::new("Exercise", 38).max(30).unit("min")
///     .fill(Gradient::sheen(Ink::slot(3)));
/// # let _ = (movement, exercise);
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct RingValue {
    name: String,
    value: Option<f64>,
    max: f64,
    unit: Option<String>,
    ink: Option<Ink>,
    fill: Option<Gradient>,
    track: Option<RingTrack>,
}

impl RingValue {
    /// `value`, called `name`, out of 100 unless [`max`](Self::max) says otherwise.
    pub fn new(name: impl Into<String>, value: impl IntoValue) -> Self {
        Self {
            name: name.into(),
            value: finite(value.into_value()),
            max: 100.0,
            unit: None,
            ink: None,
            fill: None,
            track: None,
        }
    }

    /// The goal: the whole ring. A value past it draws a second lap.
    pub fn max(mut self, max: impl IntoValue) -> Self {
        if let Some(max) = max.into_value().filter(|v| v.is_finite()) {
            self.max = max;
        }
        self
    }

    /// What the value counts, written after it: `kcal`, `min`, `%`.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Its colour: any [`Ink`] of the theme. The rings take the chart colours in order
    /// by default.
    pub fn ink(mut self, ink: impl Into<Ink>) -> Self {
        self.ink = Some(ink.into());
        self
    }

    /// Its colour by what it means.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.ink = Some(Ink::tone(tone));
        self
    }

    /// Paints its arc with `gradient`, spanning the drawing.
    pub fn fill(mut self, gradient: Gradient) -> Self {
        self.fill = Some(gradient);
        self
    }

    /// Its own track, in place of the one [`Rings::track`] gives every ring.
    pub fn track(mut self, track: RingTrack) -> Self {
        self.track = Some(track);
        self
    }
}

/// What the middle of [`Rings`] shows.
#[derive(Clone, Debug, Default)]
enum RingsMiddle {
    #[default]
    Nothing,
    Values,
    Custom(Node),
    Icon(Icon),
}

/// Two to four values on rings inside each other, the first outside: the day's
/// movement, exercise and standing; the two branches of the nervous system. Each ring
/// starts at twelve o'clock, has its own colour or gradient and track, and draws a
/// second lap past its goal. They come in one after another.
///
/// The middle holds each ring's value over its goal, an icon, markup of your own, or
/// nothing; a legend beside the rings names each with its value and share. Small, it
/// fits a date pill ([`inline`](Self::inline), 28 or 36 pixels).
///
/// ```
/// use navree_ui::charts::{Gradient, Ink, Motion, RingTrack, RingValue, Rings};
///
/// let activity = Rings::new("Activity")
///     .ring(RingValue::new("Move", 540).max(600).unit("kcal").ink(Ink::slot(8)))
///     .ring(RingValue::new("Exercise", 38).max(30).unit("min").ink(Ink::slot(3)))
///     .ring(RingValue::new("Stand", 9).max(12).unit("hr").ink(Ink::slot(1)))
///     .track(RingTrack::Tinted)
///     .center_values()
///     .legend()
///     .size(200)
///     .motion(Motion::new());
/// let pill = Rings::new("Today's scores")
///     .ring(RingValue::new("Movement", 35))
///     .ring(RingValue::new("Recovery", 72))
///     .ring(RingValue::new("Sleep", 78))
///     .size(28)
///     .inline();
/// # let _ = (navree_ui::render(&activity.into()), navree_ui::render(&pill.into()));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Rings {
    common: Common,
    rings: Vec<RingValue>,
    format: NumberFormat,
    sizes: Sizes,
    thickness: Option<f64>,
    gap: Option<f64>,
    glow: bool,
    track: RingTrack,
    middle: RingsMiddle,
    legend: bool,
    motion: Motion,
    inline: bool,
    words: RadialLabels,
}

common_methods!(Rings);

impl Rings {
    /// Rings called `title`, with no values yet.
    pub fn new(title: impl Into<String>) -> Self {
        let mut common = Common::new(title.into());
        common.hide_title = true;
        common.table = TableMode::ScreenReader;
        Self {
            common,
            rings: Vec::new(),
            format: NumberFormat::number(),
            sizes: Sizes::new(160),
            thickness: None,
            gap: None,
            glow: false,
            track: RingTrack::Theme,
            middle: RingsMiddle::Nothing,
            legend: false,
            motion: Motion::none(),
            inline: false,
            words: RadialLabels::english(),
        }
    }

    /// Adds a ring inside the ones before it. Four are drawn at most; the table lists
    /// every value.
    pub fn ring(mut self, ring: RingValue) -> Self {
        self.rings.push(ring);
        self
    }

    /// How values are written.
    pub fn format(mut self, format: NumberFormat) -> Self {
        self.format = format;
        self
    }

    /// The drawing's width in pixels, 24 to 320 (160 by default).
    pub fn size(mut self, px: u32) -> Self {
        self.sizes.base = size_of(px);
        self
    }

    /// Another width from `screen` up.
    pub fn size_on(mut self, screen: Screen, px: u32) -> Self {
        self.sizes.set(screen, size_of(px));
        self
    }

    /// Each ring's thickness in pixels at the base size: in step with the size by default, 12 at 160.
    pub fn thickness(mut self, px: f64) -> Self {
        if px.is_finite() && px > 0.0 {
            self.thickness = Some(px);
        }
        self
    }

    /// The space between rings in pixels: three tenths of the thickness by default.
    pub fn gap(mut self, px: f64) -> Self {
        if px.is_finite() && px >= 0.0 {
            self.gap = Some(px);
        }
        self
    }

    /// Puts a soft glow of its own colour around each arc.
    pub fn glow(mut self) -> Self {
        self.glow = true;
        self
    }

    /// The track every ring has unless its own says otherwise.
    pub fn track(mut self, track: RingTrack) -> Self {
        self.track = track;
        self
    }

    /// Writes each ring's value over its goal in the middle, in the ring's colour.
    pub fn center_values(mut self) -> Self {
        self.middle = RingsMiddle::Values;
        self
    }

    /// Puts markup of your own in the middle, hidden from assistive technology.
    pub fn center(mut self, content: impl Into<Node>) -> Self {
        self.middle = RingsMiddle::Custom(content.into());
        self
    }

    /// Puts an icon in the middle.
    pub fn center_icon(mut self, icon: Icon) -> Self {
        self.middle = RingsMiddle::Icon(icon);
        self
    }

    /// Names each ring beside the drawing, with its value and share.
    pub fn legend(mut self) -> Self {
        self.legend = true;
        self
    }

    /// Shows the title over the rings, and the switch that shows the table.
    pub fn show_title(mut self) -> Self {
        self.common.hide_title = false;
        self.common.table = TableMode::Switch;
        self
    }

    /// How the rings come in: each draws on after the one outside it.
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = motion;
        self
    }

    /// Draws it small for a pill, a button or a row: the picture alone in a span, named
    /// by its summary.
    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }

    /// The words it writes past the ones every chart shares.
    pub fn radial_labels(mut self, labels: RadialLabels) -> Self {
        self.words = labels;
        self
    }

    fn into_node(self) -> Node {
        let drawn = self.draw();
        let height = self.sizes.base;
        finish(self.common, drawn, "rings", height, self.inline)
    }

    fn draw(&self) -> Option<Drawn> {
        let numbers = &self.common.numbers;
        let labels = &self.common.labels;
        let fmt = |v: f64| self.format.format(v, numbers);
        // Each ring with its colour, its share and its words.
        struct Ready<'a> {
            ring: &'a RingValue,
            color: Ink,
            share: Option<f64>,
        }
        let ready: Vec<Ready> = self
            .rings
            .iter()
            .enumerate()
            .map(|(at, ring)| Ready {
                ring,
                color: ring
                    .ink
                    .or_else(|| ring.fill.as_ref().and_then(Gradient::lead))
                    .unwrap_or_else(|| Ink::nth(at)),
                share: ring
                    .value
                    .filter(|_| ring.max > 0.0 && ring.max.is_finite())
                    .map(|v| v / ring.max),
            })
            .collect();
        if ready.iter().all(|r| r.share.is_none()) {
            return None;
        }
        let size = f64::from(self.sizes.base);
        let thickness = self
            .thickness
            .unwrap_or_else(|| (size * 0.075).max(2.0))
            .clamp(1.0, size / 8.0);
        let gap = self
            .gap
            .unwrap_or_else(|| (thickness * 0.3).max(1.0))
            .min(size / 8.0);
        let c = size / 2.0;
        let area = [0.0, 0.0, size, size];
        let mut defs = Defs::new(&self.common.id());
        let drawn_rings = ready.len().min(MAX_RINGS);
        let mut marks: Vec<Node> = Vec::new();
        for (at, r) in ready.iter().take(MAX_RINGS).enumerate() {
            let radius = c - thickness / 2.0 - at as f64 * (thickness + gap);
            if radius <= thickness / 2.0 {
                break;
            }
            let paint = Paint {
                ink: Some(r.color),
                fill: r.ring.fill.clone(),
                scale: None,
                track: r.ring.track.unwrap_or(self.track),
                glow: self.glow,
            };
            let band = Band {
                cx: c,
                cy: c,
                r: radius,
                thickness,
                from: 0.0,
                sweep: 360.0,
                share: r.share.unwrap_or(0.0),
                laps: true,
                ink: r.color,
                paint: &paint,
                motion: self
                    .motion
                    .stagger(0)
                    .after(u32::try_from(at).unwrap_or(0) * self.motion_step()),
                index: 0,
                area,
            };
            marks.push(
                el("g")
                    .attr("data-ring", at as i64)
                    .child(band.nodes(&mut defs))
                    .into(),
            );
        }
        let drawing = super::plot(size, size).children((defs.node(), marks));

        let words = |r: &Ready| {
            let unit = r.ring.unit.as_deref();
            (
                r.ring.value.map(|v| with_unit(&fmt(v), unit)),
                with_unit(&fmt(r.ring.max), unit),
                r.share.map(|s| share_text(s, numbers)),
            )
        };
        let middle = match &self.middle {
            RingsMiddle::Nothing => None,
            RingsMiddle::Values => Some(
                el("div")
                    .slot("radial-center")
                    .attr("data-rows", drawn_rings as i64)
                    .child(
                        ready
                            .iter()
                            .take(MAX_RINGS)
                            .enumerate()
                            .map(|(at, r)| {
                                let unit = r.ring.unit.as_deref();
                                r.color
                                    .on(el("span").slot("radial-row"))
                                    .children((
                                        el("strong").child(
                                            r.ring.value.map_or_else(|| "–".to_owned(), fmt),
                                        ),
                                        el("span").child(format!(
                                            "/{}",
                                            with_unit(&fmt(r.ring.max), unit)
                                        )),
                                    ))
                                    .attr("data-i", at as i64)
                                    .into()
                            })
                            .collect::<Vec<Node>>(),
                    ),
            ),
            RingsMiddle::Custom(node) => Some(el("div").slot("radial-center").child(node.clone())),
            // The icon wears the outer ring's colour.
            RingsMiddle::Icon(icon) => Some(
                ready
                    .first()
                    .map_or(Ink::muted(), |r| r.color)
                    .on(el("div").slot("radial-center"))
                    .child(glyph(*icon)),
            ),
        };
        let middle = middle.map(|m| {
            self.motion
                .after(400)
                .apply(m.attr("aria-hidden", "true"), Effect::Fade, 0)
        });
        let picture = wrap("rings", self.sizes.sizers(|w| w), drawing, middle);
        let body: Node = if self.legend {
            let items: Vec<Node> = ready
                .iter()
                .map(|r| {
                    let (value, max, share) = words(r);
                    el("li")
                        .children((
                            r.color
                                .on(el("span").slot("radial-key"))
                                .attr("aria-hidden", "true"),
                            el("span").slot("radial-name").child(r.ring.name.clone()),
                            el("span").slot("radial-of").child(format!(
                                "{} / {}",
                                value.unwrap_or_else(|| "–".to_owned()),
                                max
                            )),
                            r.color
                                .on(el("strong").slot("radial-share"))
                                .child(share.unwrap_or_else(|| "–".to_owned())),
                        ))
                        .into()
                })
                .collect();
            el("div")
                .slot("radial-split")
                .children((picture, el("ul").slot("radial-legend").child(items)))
                .into()
        } else {
            picture.into()
        };

        let summary = ready
            .iter()
            .filter_map(|r| {
                let (value, max, share) = words(r);
                Some(fill(
                    &self.words.summary_ring,
                    &[
                        ("name", r.ring.name.as_str()),
                        ("value", value?.as_str()),
                        ("max", max.as_str()),
                        ("share", share?.as_str()),
                    ],
                ))
            })
            .collect::<Vec<_>>()
            .join(" ");
        let rows = ready
            .iter()
            .map(|r| {
                let (value, max, share) = words(r);
                (
                    r.ring.name.clone(),
                    vec![
                        value_cell(value, labels),
                        Node::from(max),
                        value_cell(share, labels),
                    ],
                )
            })
            .collect();
        let note = (ready.len() > MAX_RINGS).then(|| {
            fill(
                &labels.stages_cut,
                &[
                    ("shown", &MAX_RINGS.to_string()),
                    ("total", &ready.len().to_string()),
                ],
            )
        });
        Some(Drawn {
            legend: None,
            body,
            body_attrs: vec![("data-rings", AttrValue::Int(drawn_rings as i64))],
            summary,
            table: TableData {
                head: ready.first().map_or_else(Vec::new, |_| {
                    vec![
                        (labels.category.to_string(), None),
                        (labels.value.to_string(), None),
                        (self.words.goal.to_string(), None),
                        (labels.share.to_string(), None),
                    ]
                }),
                rows,
                links: Vec::new(),
                note: None,
            },
            height: self.sizes.base,
            note,
            interactive: false,
            tip: None,
        })
    }

    /// The wait between one ring and the next, in milliseconds.
    fn motion_step(&self) -> u32 {
        150
    }
}

crate::node::into_node!(Rings);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    fn html(ring: impl Into<Node>) -> String {
        render(&ring.into())
    }

    /// The `d` of the first path with `slot`.
    fn path_of(out: &str, slot: &str) -> Option<String> {
        let at = out.find(&format!(r#"data-slot="{slot}""#))?;
        let rest = &out[at..];
        let d = rest.find(" d=\"")? + 4;
        let end = rest[d..].find('"')?;
        Some(rest[d..d + end].to_owned())
    }

    #[test]
    fn a_ring_at_zero_draws_its_track_and_no_arc() {
        let out = html(Ring::new("Sleep score", 0).size(100).thickness(10.0));
        assert!(out.contains(r#"data-slot="radial-track""#), "{out}");
        assert!(!out.contains(r#"data-slot="radial-arc""#), "{out}");
        assert!(out.contains(">0</span>"), "the middle says zero: {out}");
    }

    #[test]
    fn a_ring_at_half_runs_from_twelve_to_six() {
        let out = html(Ring::new("Sleep score", 50).size(100).thickness(10.0));
        // Radius 45 round (50, 50).
        assert_eq!(
            path_of(&out, "radial-arc").as_deref(),
            Some("M50 5A45 45 0 0 1 50 95")
        );
        assert!(out.contains(r#"pathLength="1""#));
        assert!(
            out.contains(r#"<circle cx="50" cy="50" r="45" data-slot="radial-track""#),
            "{out}"
        );
    }

    #[test]
    fn a_full_ring_is_two_halves_and_past_full_draws_a_lap() {
        let out = html(
            Ring::new("Steps", 10_000)
                .max(10_000)
                .size(100)
                .thickness(10.0),
        );
        assert_eq!(
            path_of(&out, "radial-arc").as_deref(),
            Some("M50 5A45 45 0 0 1 50 95A45 45 0 0 1 50 5")
        );
        assert!(!out.contains("radial-lap"), "{out}");
        let over = html(
            Ring::new("Steps", 12_500)
                .max(10_000)
                .size(100)
                .thickness(10.0),
        );
        assert_eq!(
            path_of(&over, "radial-arc").as_deref(),
            Some("M50 5A45 45 0 0 1 50 95A45 45 0 0 1 50 5"),
            "the first lap is whole"
        );
        assert_eq!(
            path_of(&over, "radial-lap").as_deref(),
            Some("M50 5A45 45 0 0 1 95 50"),
            "the second lap is the quarter past the goal"
        );
        assert_eq!(
            over.matches(r#"stroke-width="10""#).count(),
            3,
            "the lap is as thick as the arc and its track: {over}"
        );
        // The table and summary say the real value.
        assert!(over.contains("12,500 of 10,000, 125%."), "{over}");
    }

    #[test]
    fn a_ring_is_a_named_picture_with_its_value_in_a_table() {
        let out = html(
            Ring::new("Sleep score", 82)
                .label("Sleep score")
                .sublabel("Great"),
        );
        assert!(out.contains(r#"role="img""#), "{out}");
        assert!(out.contains(r#"aria-label="Sleep score, chart""#), "{out}");
        assert!(out.contains("Sleep score: 82 of 100, 82%."), "{out}");
        assert!(out.contains("<table"), "{out}");
        assert!(
            !out.contains("chart-table-switch"),
            "no switch by default: {out}"
        );
        assert!(!out.contains("data-ui-chart"), "no tooltip: {out}");
        assert!(
            out.contains(r#"data-slot="radial-label">Sleep score<"#),
            "{out}"
        );
        assert!(out.contains(r#"data-slot="radial-sub">Great<"#), "{out}");
        assert!(!out.contains("style="), "no style attribute: {out}");
    }

    #[test]
    fn a_ring_with_its_title_shown_links_the_charts_sheet() {
        let node: Node = Ring::new("Sleep score", 82).show_title().into();
        let html = render(&node);
        assert!(html.contains("chart-table-switch"), "{html}");
        let figure = node.into_element().expect("a figure");
        assert!(figure.uses.has(crate::node::Uses::CHARTS));
    }

    #[test]
    fn numbers_are_written_for_the_locale() {
        let out = html(Ring::new("Steps", 123_456).max(150_000).locale("en-IN"));
        assert!(out.contains(">1,23,456<"), "{out}");
        assert!(out.contains("1,23,456 of 1,50,000"), "{out}");
    }

    #[test]
    fn a_missing_value_is_the_empty_state_never_a_zero() {
        let out = html(Ring::new("Sleep score", None::<f64>));
        assert!(out.contains(r#"data-state="empty""#), "{out}");
        assert!(!out.contains("radial-arc"));
        let out = html(Ring::new("Sleep score", f64::NAN));
        assert!(out.contains(r#"data-state="empty""#), "{out}");
        let out = html(Ring::new("Sleep score", 4).max(0));
        assert!(
            out.contains(r#"data-state="empty""#),
            "a scale of nothing: {out}"
        );
        let out = html(Ring::new("Sleep score", None::<f64>).inline());
        assert!(
            out.contains(r#"data-empty"#) && out.contains("Sleep score: No data"),
            "{out}"
        );
    }

    #[test]
    fn a_negative_value_draws_nothing_and_says_itself() {
        let out = html(Ring::new("Balance", -20));
        assert!(!out.contains("radial-arc"), "{out}");
        assert!(out.contains("Balance: -20 of 100"), "{out}");
    }

    #[test]
    fn gradients_are_named_after_the_chart_and_span_the_ring() {
        let out = html(
            Ring::new("Sleep score", 82)
                .id("sleep")
                .size(160)
                .fill(Gradient::sheen(Ink::own()))
                .ink(Ink::slot(7))
                .halo(),
        );
        assert!(out.contains(r#"<linearGradient id="sleep-p0" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="160" y2="160">"#), "{out}");
        assert!(out.contains(r#"stroke="url(#sleep-p0)""#), "{out}");
        assert!(out.contains(r#"data-color="7" data-mix="vivid""#), "{out}");
        assert!(out.contains(r#"data-slot="radial-halo""#), "{out}");
        // Two rings on a page, two names.
        let other = html(
            Ring::new("Sleep score", 82)
                .id("sleep-2")
                .fill(Gradient::sheen(Ink::own())),
        );
        assert!(other.contains("sleep-2-p0"));
    }

    #[test]
    fn a_threshold_scale_colours_the_ring_and_says_the_band() {
        let scale = super::super::super::ColorScale::new(Ink::tone(Tone::Danger))
            .label("Low")
            .from(67, Ink::tone(Tone::Success))
            .label("Recovered");
        let out = html(Ring::new("Recovery", 87).colors(scale.clone()));
        assert!(
            out.contains(r#"data-slot="radial" data-kind="ring""#),
            "{out}"
        );
        assert!(out.contains(r#"data-tone="success""#), "{out}");
        assert!(out.contains("87 of 100, 87%. Recovered."), "{out}");
        let low = html(Ring::new("Recovery", 20).colors(scale));
        assert!(
            low.contains(r#"data-tone="danger""#) && low.contains("Low."),
            "{low}"
        );
    }

    #[test]
    fn motion_draws_on_and_counts_up_and_still_writes_nothing() {
        let out = html(Ring::new("Sleep score", 82).motion(Motion::new().delay(200)));
        assert!(
            out.contains(r#"data-motion="draw" data-delay="4""#),
            "{out}"
        );
        assert!(out.contains(r#"data-to="82""#), "{out}");
        let still = html(Ring::new("Sleep score", 82));
        assert!(
            !still.contains("data-motion") && !still.contains("data-to"),
            "{still}"
        );
    }

    #[test]
    fn a_small_ring_puts_its_words_under_it_and_grows_on_wider_screens() {
        let out = html(
            Ring::new("Steps", 64)
                .label("Steps")
                .size(64)
                .size_on(Screen::Md, 80),
        );
        assert!(out.contains(r#"data-slot="radial-caption""#), "{out}");
        assert!(
            out.contains(r#"data-to="md" width="64" height="64""#),
            "{out}"
        );
        assert!(
            out.contains(r#"data-at="md" width="80" height="80""#),
            "{out}"
        );
        // The drawing keeps the base size's units, and scales.
        assert!(out.contains(r#"viewBox="0 0 64 64""#), "{out}");
    }

    #[test]
    fn the_middle_can_be_an_icon_markup_or_nothing() {
        let out = html(Ring::new("Oxygen", 97).center_icon(crate::icons::HEART));
        assert!(out.contains(r#"data-slot="radial-icon""#), "{out}");
        let out = html(Ring::new("Oxygen", 97).center(el("b").child("97")));
        assert!(out.contains("<b>97</b>"), "{out}");
        let out = html(Ring::new("Oxygen", 97).center_none());
        assert!(!out.contains("radial-center"), "{out}");
    }

    #[test]
    fn hostile_words_are_text() {
        let nasty = "<script>alert(1)</script>\"'";
        let out = html(Ring::new(nasty, 5).label(nasty).sublabel(nasty).unit(nasty));
        assert!(!out.contains("<script>"), "{out}");
    }

    #[test]
    fn rings_sit_inside_each_other_and_come_in_one_after_another() {
        let out = html(
            Rings::new("Activity")
                .ring(RingValue::new("Move", 300).max(600).unit("kcal"))
                .ring(RingValue::new("Exercise", 30).max(30).unit("min"))
                .ring(RingValue::new("Stand", 15).max(12).unit("hr"))
                .size(100)
                .thickness(10.0)
                .gap(2.0)
                .motion(Motion::new()),
        );
        // Radii 45, 33, 21: each a thickness and a gap inside the last.
        assert!(
            out.contains(r#"r="45""#) && out.contains(r#"r="33""#) && out.contains(r#"r="21""#),
            "{out}"
        );
        assert!(
            out.contains("M50 5A45 45 0 0 1 50 95\""),
            "move at half: {out}"
        );
        assert!(out.contains("radial-lap"), "stand is past its goal: {out}");
        assert!(
            out.contains(r#"data-delay="3""#),
            "the second ring waits: {out}"
        );
        assert!(out.contains("Move: 300 kcal of 600 kcal, 50%."), "{out}");
        assert!(out.contains("Stand: 15 hr of 12 hr, 125%."), "{out}");
        assert!(
            out.contains(r#"data-color="1""#) && out.contains(r#"data-color="3""#),
            "{out}"
        );
    }

    #[test]
    fn rings_with_no_value_are_empty_and_a_fifth_is_listed_not_drawn() {
        let out = html(Rings::new("Activity"));
        assert!(out.contains(r#"data-state="empty""#), "{out}");
        let mut five = Rings::new("Five");
        for k in 0..5 {
            five = five.ring(RingValue::new(format!("R{k}"), 50));
        }
        let out = html(five);
        assert_eq!(out.matches(r#"data-slot="radial-arc""#).count(), 4, "{out}");
        assert!(out.contains(">R4<"), "the table lists the fifth: {out}");
    }

    #[test]
    fn tiny_rings_fit_a_pill() {
        let out = html(
            Rings::new("Today")
                .ring(RingValue::new("Movement", 35))
                .ring(RingValue::new("Recovery", 72))
                .ring(RingValue::new("Sleep", 78))
                .size(28)
                .inline(),
        );
        assert!(out.starts_with(r#"<span data-slot="radial-inline" role="img" aria-label="Movement: 35 of 100, 35%."#), "{out}");
        assert!(!out.contains("<figure") && !out.contains("<table"), "{out}");
        assert_eq!(out.matches(r#"data-slot="radial-arc""#).count(), 3, "{out}");
    }

    #[test]
    fn rings_say_their_values_in_the_middle_and_the_legend() {
        let out = html(
            Rings::new("Nervous system")
                .ring(RingValue::new("Sympathetic", 42).unit("%"))
                .ring(RingValue::new("Parasympathetic", 68).unit("%"))
                .center_values()
                .legend(),
        );
        assert!(
            out.contains("<strong>42</strong><span>/100%</span>"),
            "{out}"
        );
        assert!(out.contains(r#"data-slot="radial-legend""#), "{out}");
        assert!(out.contains("68% / 100%"), "{out}");
    }
}
