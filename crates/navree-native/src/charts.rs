//! The parts a chart is made of, and the kinds of chart a health or fitness
//! screen draws: rings, a day on a clock, a bar of segments, a strip of cells,
//! a scale, a calendar of days, a matrix, a cloud of points.
//!
//! Every kind here says what its numbers and words *mean* and never how the
//! web draws them: numbers are numbers, the words a person reads beside them
//! (the value as shown, its unit, a label, a caption) come with them, and one
//! sentence says the whole chart to a reader who can't see it. A kind never
//! carries a size in points, a stroke width, a glow, the angle of a gradient
//! or an animation. Where a choice of size says something (a hero ring against
//! a small one) a closed word says it: [`Prominence`](crate::Prominence).
//!
//! Colour is data, by name. A part that has a colour of its own names a
//! [`Hue`] from one closed list, so each system draws its own purple, red,
//! green, orange and blue; [`Colour`] adds the second hue of a blend and the
//! theme's own colours as a fallback. A hue this build does not know is read as
//! [`Hue::Neutral`].
//!
//! The chart kinds live in [`node`](crate::node) as well: they are the parts a
//! [`Node`](crate::Node) holds.

use serde::{Deserialize, Serialize};

use crate::node::{Icon, Node, Prominence, Tone, is_default, is_false};

/// A colour of data by name. A client draws each as its own system's colour of
/// that name, so a pillar that is violet is the system's violet on iPhone and
/// on Android, in light and in dark.
///
/// The status tones keep their names beside the plain hues: `danger` says
/// that red means trouble, `red` that it is only the colour of a heart rate. A
/// client may draw both with the same red, and a chart never uses a status
/// tone for a colour that carries no meaning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Hue {
    /// The quiet ink: what is not the subject of the chart, a track, the rest.
    /// Also what a name this build does not know is read as.
    #[default]
    Neutral,
    /// The app's accent.
    Accent,
    /// It worked, or it is well.
    Success,
    /// It needs attention.
    Warning,
    /// It failed, or it is dangerous.
    Danger,
    /// For your information.
    Info,
    /// Blue.
    Blue,
    /// Green.
    Green,
    /// Amber: yellow turning to orange.
    Amber,
    /// Orange.
    Orange,
    /// Red.
    Red,
    /// Rose: a red that leans to pink.
    Rose,
    /// Violet: the system's purple.
    Violet,
    /// Cyan: a blue that leans to green.
    Cyan,
    /// Slate: a grey that leans to blue.
    Slate,
}

impl Hue {
    /// The tone this hue is the colour of: the status tones and the accent are
    /// their own, and a plain colour has none. An older form of a part that
    /// has a tone and no hue takes it from here.
    pub const fn tone(self) -> Option<Tone> {
        match self {
            Self::Accent => Some(Tone::Accent),
            Self::Success => Some(Tone::Success),
            Self::Warning => Some(Tone::Warning),
            Self::Danger => Some(Tone::Danger),
            Self::Info => Some(Tone::Info),
            _ => None,
        }
    }
}

/// The colour of one part of a chart: a hue by name, and where it helps, the
/// hue it blends into and the theme's own colours.
///
/// A client draws `hue` in its system's colour of that name. A part that names
/// no colour is drawn in the colours the client takes in turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Colour {
    /// The hue.
    #[serde(deserialize_with = "crate::lenient::or_default")]
    pub hue: Hue,
    /// The hue it blends into, along the part: a ring that turns from cyan to
    /// violet is `hue` cyan and `to` violet. A blend with more stops than two
    /// is carried by its first and its last. A name this build does not know
    /// leaves the part one colour.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::lenient::or_none")]
    pub to: Option<Hue>,
    /// The theme's own colour for `hue` on a light ground, as `#rrggbb`: a
    /// fallback for a client that does not know the hue. A client that does
    /// ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<String>,
    /// The theme's own colour for `hue` on a dark ground, as `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<String>,
}

impl Colour {
    /// The hue `hue`, with nothing beside it.
    pub const fn new(hue: Hue) -> Self {
        Self {
            hue,
            to: None,
            light: None,
            dark: None,
        }
    }

    /// A blend from `hue` into `to`.
    pub const fn blend(hue: Hue, to: Hue) -> Self {
        Self {
            hue,
            to: Some(to),
            light: None,
            dark: None,
        }
    }
}

impl Default for Colour {
    fn default() -> Self {
        Self::new(Hue::Neutral)
    }
}

impl From<Hue> for Colour {
    fn from(hue: Hue) -> Self {
        Self::new(hue)
    }
}

/// Which mark a hint wears.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HintMark {
    /// A question mark: how to read what it is beside.
    #[default]
    Question,
    /// An "i": more about what it is beside.
    Info,
}

/// A note about the part it is on: how to read a chart, what a score is made
/// of. A client shows it when the person asks, as its system shows a hint (a
/// popover, a tooltip, a sheet), by a mark beside the part; never in the page.
///
/// A part that can have one carries it as `hint`: a chart, a figure, a section
/// beside its heading. It is not a kind of its own.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hint {
    /// What it is about: the name of the thing it explains, `the sleep score`.
    /// The mark is named after it for assistive technology: "About the sleep
    /// score".
    pub about: String,
    /// A heading for the note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The note.
    pub text: String,
    /// The mark that opens it.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub mark: HintMark,
}

/// A stretch of a scale with a name and a colour: the zones of a gauge (what
/// is under, in and over the usual range), the bands behind a line, the parts
/// of a scale a value stands in.
///
/// The ends are in the scale's own units. Zones may touch, and where they
/// overlap the one listed last holds the value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zone {
    /// Where it starts.
    pub from: serde_json::Number,
    /// Where it ends.
    pub to: serde_json::Number,
    /// What it is: `Optimal`. A client names the zone a value is in beside the
    /// value, so the colour is never alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Its colour. A client takes its colours in turn when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
}

impl Default for Zone {
    fn default() -> Self {
        Self {
            from: 0.into(),
            to: 0.into(),
            label: None,
            colour: None,
        }
    }
}

/// What stands in the middle of a ring.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Centre {
    /// The value, with its unit, and the label and the caption beside or under
    /// it where the ring has room: the usual. A ring with no value says so in
    /// its middle, with a dash.
    #[default]
    Value,
    /// The ring's icon, in place of the value.
    Icon,
    /// Nothing: the ring alone.
    Nothing,
}

/// One value against its whole, drawn as a ring: a score out of 100, steps
/// toward a goal, a share. It starts at the top and runs clockwise, as the
/// system's own rings do; a value above `max` is past its goal, which a client
/// shows as its system does (a second lap).
///
/// The numbers are numbers. `shown` is the value in the page's words, without
/// its `unit`, which follows it: `12,450` and `steps`. A ring with no `value`
/// is a day with no reading; the client draws the empty ring and a dash.
///
/// How big the ring is says something: `prominence` is `high` for the ring a
/// screen is about (the health score), `medium` for one among its neighbours
/// (a pillar of the score) and `low` for a small one beside words or in a row
/// (the day's scores in a date control). A client sizes each by its system's
/// rules and puts the label and the caption inside the ring where there is room
/// and beside it where there is not.
///
/// A ring on a screen is named by its `summary`, one sentence that says the
/// whole ring (`Sleep score: 87 of 100, 87%.`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ring {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The value, in the same units as `max`. None when there is no reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Number>,
    /// The whole: the goal the value is measured against. 100 for a score.
    pub max: serde_json::Number,
    /// What the value counts, written after it: `steps`, `kcal`, `%`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// The value as the page wrote it, without its unit: `12,450`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
    /// What it measures: `Health score`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// A quieter line after the label: `Great recovery`, `25% of the score`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    /// Its colour. A client takes its colours in turn when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
    /// What stands in its middle.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub centre: Centre,
    /// The icon in its middle, when `centre` is `icon`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// How much the ring matters on the screen: the hero, one of several, a
    /// small one.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub prominence: Prominence,
    /// One sentence that says the whole ring to a reader who can't see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// A note about what the ring means, shown when asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Hint>,
}

impl Default for Ring {
    fn default() -> Self {
        Self {
            id: None,
            value: None,
            max: 100.into(),
            unit: None,
            shown: None,
            label: None,
            caption: None,
            colour: None,
            centre: Centre::Value,
            icon: None,
            prominence: Prominence::Medium,
            summary: None,
            hint: None,
        }
    }
}

/// What stands in the middle of rings inside each other.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RingsCentre {
    /// Nothing: the rings alone.
    #[default]
    Nothing,
    /// Each ring's value over its goal, in the ring's colour, one under the
    /// other in the order of the rings.
    Values,
    /// The `icon` of the rings.
    Icon,
}

/// One ring of [`Rings`]: a name, a value and the goal it is measured against.
/// The numbers and the words are a [`Ring`]'s, and a value above the goal is
/// past it in the same way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RingValue {
    /// What it measures: `Move`.
    pub name: String,
    /// The value, in the same units as `max`. None when there is no reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Number>,
    /// The goal: the whole ring.
    pub max: serde_json::Number,
    /// What the value counts, written after it: `kcal`, `min`, `hr`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// The value as the page wrote it, without its unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
    /// Its colour. The rings take the client's colours in turn when none says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
}

impl Default for RingValue {
    fn default() -> Self {
        Self {
            name: String::new(),
            value: None,
            max: 100.into(),
            unit: None,
            shown: None,
            colour: None,
        }
    }
}

/// The most rings a client draws inside each other. A client draws the first
/// ones and the legend lists every one.
pub const MAX_RINGS: usize = 6;

/// Two to six values on rings inside each other, the first outside: the day's
/// movement, exercise and standing; the two halves of a nervous system; five
/// scores of a day. Each ring has its own goal, colour and words, and a ring
/// past its goal is shown as the system's own rings show it.
///
/// A client draws them as its system draws rings of this kind (the activity
/// rings on Apple systems), puts each ring's value over its goal in the middle
/// when `centre` says so, and lists each beside the drawing with its value and
/// its share when `legend` is set. More than [`MAX_RINGS`] are listed in the
/// legend and not drawn. How big they are says something: `prominence` is as a
/// [`Ring`]'s.
///
/// The set is named by its `summary`, one sentence that says every ring.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rings {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What the rings show together: `Activity rings`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The rings, the outside one first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rings: Vec<RingValue>,
    /// What stands in the middle.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub centre: RingsCentre,
    /// The icon in the middle, when `centre` is `icon`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// Whether each ring is named beside the drawing, with its value and its
    /// share.
    #[serde(default, skip_serializing_if = "is_false")]
    pub legend: bool,
    /// How much the rings matter on the screen.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub prominence: Prominence,
    /// One sentence that says every ring to a reader who can't see them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// A note about what the rings mean, shown when asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Hint>,
}

/// A name in a key, with the colour it wears: a kind of hour on a clock, a stage
/// of sleep in a strip, a part of a column. A client lists the categories of a
/// chart beside it, each with its colour, so a colour is never alone.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    /// What it is: `Deep`, `Active`.
    pub name: String,
    /// Its colour. A client takes its colours in turn when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
}

/// One hour of a [`DayClock`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hour {
    /// The hour's value: steps, active minutes. None for an hour with no
    /// reading, which is not an hour of zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Number>,
    /// How strong the hour is, 0 to 1: how much of it was active, say. A client
    /// draws a stronger hour stronger. Left out, the hour's value against the
    /// busiest hour's says it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<serde_json::Number>,
    /// What the hour was: the place of one of the clock's `key`, from 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<u32>,
}

/// Something that happened at a time of day, marked on a [`DayClock`]: a run, a
/// meal, a meeting, waking up.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockMoment {
    /// The time of day in hours, 0 to 24: `6.75` is a quarter to seven in the
    /// morning.
    pub at: serde_json::Number,
    /// What happened, with the time or the figure that matters: `Morning run,
    /// 3.2 km`.
    pub name: String,
    /// Its mark.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// The colour of its mark. It takes the colour of its hour when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
}

impl Default for ClockMoment {
    fn default() -> Self {
        Self {
            at: 0.into(),
            name: String::new(),
            icon: None,
            colour: None,
        }
    }
}

/// The most moments a client marks on a clock; the others are listed.
pub const MAX_MOMENTS: usize = 8;

/// A day as 24 hours round a dial, midnight at the top and running clockwise:
/// how much happened in each hour (steps, active minutes), what each hour was
/// (a kind from the `key`), and the moments that stand out. A client draws the
/// dial as its system draws a 24 hour dial, with the hours' values as marks
/// round it, each in the colour of its category and as strong as its level; the
/// hour that is now is outlined; the middle holds the day's total.
///
/// `hours` holds the 24 hours from midnight; fewer leave the rest empty. The
/// total is `shown`, in the page's words and without its unit, and `label` says
/// what it counts under it (`Steps`). More than [`MAX_MOMENTS`] are listed
/// beside the dial and not marked on it.
///
/// The clock is named by its `summary`, one sentence with the total, the
/// busiest hour and the moments.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayClock {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What the clock shows: `Steps by hour`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The hours, from midnight.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hours: Vec<Hour>,
    /// What an hour can be, each with its colour: the key of the dial.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key: Vec<Category>,
    /// The moments that stand out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moments: Vec<ClockMoment>,
    /// The hour that is now, 0 to 23, when the day is today.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now: Option<u8>,
    /// The value a full hour stands for. The busiest hour's when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<serde_json::Number>,
    /// What the values count: `steps`, `min`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// The day's total in the page's words, without its unit: `9,412`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
    /// What the total counts, small under it: `Steps`. The unit when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// An icon over the total.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    /// The colour of an hour that has no category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
    /// The colour the middle is washed with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hub: Option<Colour>,
    /// One sentence that says the whole clock to a reader who can't see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// A note about what the clock means, shown when asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Hint>,
}

/// One segment of a [`Meter`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    /// What it is: `Deep`.
    pub name: String,
    /// The amount it holds. None, zero or below zero gives it no width: it
    /// stays in the key, so every part is listed, the empty ones too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Number>,
    /// The amount in the page's words, with its unit: `1 h 23 min`,
    /// `₹1,20,000`. A duration is written by the page, so a client shows these
    /// words and does not write its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
    /// A second line under its name in the key: the range a heart rate zone
    /// covers (`109–127 bpm`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Its colour. A client takes its colours in turn when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
}

/// One bar split into segments that add up to the whole, drawn as 100%: how a
/// night divides into sleep stages, how a workout divides into heart rate
/// zones, where an order stands.
///
/// A segment's share is its value over the total of the values above zero. A
/// client draws the bar as its system draws a bar of this kind, writes a share
/// inside a segment that is wide enough for it, and lists every segment beside
/// or under the bar with its colour, its words and its share.
///
/// The bar is named by its `summary`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meter {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What the bar shows: `Sleep stages, last night`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The segments, in order from the start.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<Segment>,
    /// One sentence that says the whole bar to a reader who can't see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// A note about what the bar means, shown when asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Hint>,
}

/// One column of [`MiniColumns`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiniColumn {
    /// What it is: `Cycle 1`, `Mon`.
    pub label: String,
    /// What each part of the chart holds in this column, one for each part in
    /// the order the parts were given. A part with no value, or none above
    /// zero, has no height.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<Option<serde_json::Number>>,
    /// The column's total in the page's words: `1 h 34 min`. A client writes
    /// the sum of the parts when there are none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
}

/// The most columns a client draws; the others are listed.
pub const MAX_COLUMNS: usize = 48;

/// Small multiples: one column for each item (a sleep cycle, a day), each split
/// into the same parts and drawn as a whole, so the columns compare what they
/// are made of and not how big they are. A client writes each column's label
/// above it and its total below, and lists the parts, each with its colour.
///
/// One column may stand out: `highlight` is its place, from 0. A client draws
/// the others quieter, as its system's charts draw a column that is chosen.
///
/// The chart is named by its `summary`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiniColumns {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What the columns show: `Sleep cycles`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What every column is split into, with the colour of each part.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<Category>,
    /// The columns, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<MiniColumn>,
    /// The place of the column that stands out, from 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<u32>,
    /// One sentence that says the whole chart to a reader who can't see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// A note about what the chart means, shown when asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Hint>,
}

/// One cell of a [`ColorStrip`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StripCell {
    /// What it stands for: a time, a number. `11:30 pm`.
    pub label: String,
    /// What it was: the place of one of the strip's `key`, from 0. None for a
    /// cell with no reading, which a client draws in the quiet colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<u32>,
    /// The reading the cell stands for, when the strip is of readings: its
    /// category is the band the reading fell in, which the server has worked
    /// out. None for a strip of stages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Number>,
}

/// The most cells a client draws; a longer strip is drawn from every few cells.
pub const MAX_CELLS: usize = 600;

/// A row of cells of equal width, each in the colour of its category: the stage
/// each half hour of a night was in, the zone each heart rate reading of a
/// workout fell in, the state of each check of a service. The cells are equal
/// in width, so the strip is in proportion to the number of cells, and to time
/// only when the cells are equal in time.
///
/// A client lists the `key` beside the strip, each category with its colour,
/// and with its share of the cells when `shares` is set. For a strip of
/// readings, `label` names what the readings are and `unit` what they count.
///
/// The strip is named by its `summary`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorStrip {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What the strip shows: `The night in half hours`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What a cell can be, each with its colour: the key of the strip.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key: Vec<Category>,
    /// The cells, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<StripCell>,
    /// What a reading is: `Heart rate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// What a reading counts: `bpm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Whether the key gives each category's share of the cells.
    #[serde(default, skip_serializing_if = "is_false")]
    pub shares: bool,
    /// One sentence that says the whole strip to a reader who can't see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// A note about what the strip means, shown when asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Hint>,
}

/// Where one value sits on a scale between a low end and a high end: a resting
/// heart rate between 40 and 120, a blood pressure reading, a price between its
/// day's low and high. The scale may be divided into named zones, each in its
/// own colour, so the ends can read as good and bad; the zones, the words under
/// the ends and the summary give the numbers, so the colours are never alone.
///
/// A value outside the scale sits at the nearer end, and the summary says it is
/// below or above. `shown` is the value in the page's words, without its `unit`,
/// which follows it. A client draws the scale as its system draws a gauge on a
/// line, with a marker where the value is.
///
/// The scale is named by its `summary`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeScale {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What the scale measures: `Resting heart rate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The value. None when there is no reading, which leaves the scale empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Number>,
    /// The low end of the scale.
    pub min: serde_json::Number,
    /// The high end of the scale.
    pub max: serde_json::Number,
    /// What the value counts, written after it: `bpm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// The value as the page wrote it, without its unit: `54`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
    /// Words under the low end, in place of its number: `Slower`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low: Option<String>,
    /// Words under the high end, in place of its number: `Faster`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high: Option<String>,
    /// The zones of the scale, with their names and bounds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<Zone>,
    /// One sentence that says the whole scale to a reader who can't see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// A note about what the scale means, shown when asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Hint>,
}

impl Default for RangeScale {
    fn default() -> Self {
        Self {
            id: None,
            title: None,
            value: None,
            min: 0.into(),
            max: 100.into(),
            unit: None,
            shown: None,
            low: None,
            high: None,
            zones: Vec::new(),
            summary: None,
            hint: None,
        }
    }
}

/// What one axis of a chart measures: its name, its unit and its bounds. Each
/// part is optional; a client takes from the data what is not said.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measure {
    /// What the axis is: `Night`, `Interval n`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What its values count: `h`, `ms`, `bpm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// The least the axis shows. A value below it widens it, so a mark is
    /// never cut off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<serde_json::Number>,
    /// The most the axis shows. A value above it widens it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<serde_json::Number>,
}

/// What a reference line is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ReferenceKind {
    /// A level across the chart at a value: a target, a limit, an average.
    #[default]
    Level,
    /// The diagonal where x equals y, across what both axes share: for a chart
    /// with numbers along both, such as one reading against the next.
    Diagonal,
}

/// A line drawn for reference across a chart: a level (a target, a resting
/// rate, the average of a series, a zero baseline) or the diagonal where x
/// equals y. A client draws it quietly and names it with its label, in its
/// colour when it has one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    /// What it is.
    #[serde(default, skip_serializing_if = "is_default", deserialize_with = "crate::lenient::or_default")]
    pub kind: ReferenceKind,
    /// The level, in the chart's value units, for a `level`. An average or a
    /// baseline is worked out by the server and sent as a level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Number>,
    /// What the line is, with the value when it helps: `Your need: 8 h`,
    /// `Average: 612`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Its colour. A client draws it in the quiet ink when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
}

/// How a line or an area runs from one value to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Curve {
    /// Straight between values.
    #[default]
    Linear,
    /// A smooth curve through them that never overshoots a value.
    Smooth,
    /// Each value holds until the next: for states that last, such as the stages
    /// of sleep.
    Step,
}

/// How the series of a bar or area chart sit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Stacking {
    /// Apart: bars side by side, areas each from the baseline.
    #[default]
    Apart,
    /// On one another: parts of a total at each place.
    Values,
    /// On one another as shares of each place's total, to a whole.
    Shares,
}

/// One day of a [`CalendarHeatmap`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeatDay {
    /// The day, as a date: `2026-11-12`.
    pub date: String,
    /// What the day came to. None for a day with no reading, which is not a
    /// day of zero: a client draws it as a day with no data, hollow, and a day
    /// of zero as the lightest shade.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Number>,
}

/// The most days a client draws: the latest ones, when it is given more.
pub const MAX_DAYS: usize = 1_100;

/// A calendar of days, one cell for each, a column for each week and a row for
/// each day of the week, each cell shaded by its day's value: how hard each
/// day of training was, how much each day was walked. Months are named above
/// the weeks and the days of the week beside the rows, in the person's
/// language and with the first day of the week their system keeps.
///
/// A client draws the days as its system draws a calendar of this kind (the
/// activity calendars of the system's own apps), with the shades of one `colour`
/// from the lightest to the deepest. `steps` are the three values at which a
/// day moves from the first shade above nothing to the second, the third and
/// the fourth. Left out, the four shades are equal parts of the range from zero
/// to the highest value. A day below the first step but above zero is the first
/// shade; a day at zero or below is the lightest, as a day of rest.
///
/// `days` need not be every day: a day between two that are given is a day with
/// no data, and `first` and `last` draw the calendar from one date to another
/// whether or not the days at its ends were given.
///
/// The calendar is named by its `summary`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarHeatmap {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What the calendar shows: `Training calendar`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The days with what each came to, in date order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub days: Vec<HeatDay>,
    /// The first day drawn, as a date. The first day given when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first: Option<String>,
    /// The last day drawn, as a date. The last day given when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,
    /// The colour the shades are made of. The app's accent when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
    /// Where the second, third and fourth shades begin, three numbers in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<serde_json::Number>,
    /// What a day's value is called: `Training load`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// What a day's value counts, written after it: `load`, `steps`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// One sentence that says the whole calendar to a reader who can't see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// A note about what the calendar means, shown when asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<Hint>,
}

/// Where a day the person chooses is asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayPick {
    /// The address of the screen.
    pub href: String,
    /// The name the chosen day is added to it under, as a query value in the
    /// form `2026-11-11`: `date`.
    pub name: String,
}

/// The words of a day pager's controls, in the page's language.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PagerWords {
    /// What the control to the day before is called: `Previous day`.
    pub previous: String,
    /// What the control to the day after is called: `Next day`.
    pub next: String,
    /// What the control that opens the choice of a day is called: `Choose a day`.
    pub choose: String,
}

/// The control that steps a screen from day to day: the day shown, the day
/// before and the day after, a way back to today and a choice of any day.
///
/// The day is said twice: `day` in the page's words (`Today`, `Wed, 11 Nov`)
/// and `date` as a date, which a client's date control takes. The addresses are
/// where the screen of each day is: a client steps to `previous` and `next`,
/// to `today` when it is not today, and to the day the person chooses by asking
/// `pick`. A step replaces the day on the screen; it does not add a screen to
/// the back stack, so Back leaves the screen and never walks through the days.
/// A day with nothing to step to has no address: the control is shown without
/// it. `first` and `last` bound the days that can be chosen.
///
/// A client draws it with its own date control and steppers, in the place its
/// system puts them (the top of the screen, a bar), and takes the words for the
/// steppers from `words`. `scores` is a small graphic before the date: the
/// day's scores as small rings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayPager {
    /// The node's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The day shown, in the page's words.
    pub day: String,
    /// The day shown, as a date: `2026-11-11`.
    pub date: String,
    /// Where the day before is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
    /// Where the day after is. None when this is the last day there is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// Where today is. None when the day shown is today.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub today: Option<String>,
    /// The first day that can be chosen, as a date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first: Option<String>,
    /// The last day that can be chosen, as a date: usually today.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,
    /// How a day the person chooses is asked for. None when only the steps are
    /// offered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pick: Option<DayPick>,
    /// A small graphic before the date: the day's scores as small rings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scores: Option<Box<Node>>,
    /// The words of its controls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<PagerWords>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_is_a_hue_by_name_and_writes_only_what_it_holds() {
        let plain = Colour::new(Hue::Violet);
        assert_eq!(serde_json::to_string(&plain).unwrap(), r#"{"hue":"violet"}"#);
        let blend = Colour::blend(Hue::Cyan, Hue::Violet);
        assert_eq!(serde_json::to_string(&blend).unwrap(), r#"{"hue":"cyan","to":"violet"}"#);
        let themed: Colour = serde_json::from_str(
            r##"{"hue":"rose","light":"#b3254f","dark":"#ff8aa8"}"##,
        )
        .unwrap();
        assert_eq!(themed.hue, Hue::Rose);
        assert_eq!(themed.light.as_deref(), Some("#b3254f"));
        assert_eq!(themed.dark.as_deref(), Some("#ff8aa8"));
        assert_eq!(Colour::from(Hue::Slate), Colour::new(Hue::Slate));
    }

    #[test]
    fn a_hue_this_build_does_not_know_is_neutral_and_a_blend_into_one_is_no_blend() {
        let unknown: Colour = serde_json::from_str(r#"{"hue":"chartreuse","to":"teal"}"#).unwrap();
        assert_eq!(unknown.hue, Hue::Neutral);
        assert_eq!(unknown.to, None);
        // A known blend is read as it was written; only a word is forgiven.
        let blend: Colour = serde_json::from_str(r#"{"hue":"green","to":"cyan"}"#).unwrap();
        assert_eq!((blend.hue, blend.to), (Hue::Green, Some(Hue::Cyan)));
        assert!(serde_json::from_str::<Colour>(r#"{"hue":7}"#).is_err());
        assert!(serde_json::from_str::<Colour>(r#"{}"#).is_err());
    }

    #[test]
    fn a_hint_has_what_it_is_about_and_its_words() {
        let hint: Hint = serde_json::from_str(
            r#"{"about":"the sleep score","title":"How it is made","text":"Duration, stages and regularity."}"#,
        )
        .unwrap();
        assert_eq!(hint.mark, HintMark::Question);
        assert_eq!(hint.title.as_deref(), Some("How it is made"));
        let info: Hint =
            serde_json::from_str(r#"{"about":"x","text":"y","mark":"info"}"#).unwrap();
        assert_eq!(info.mark, HintMark::Info);
        // A mark this build does not know is the usual one; a hint with no words is refused.
        let newer: Hint =
            serde_json::from_str(r#"{"about":"x","text":"y","mark":"star"}"#).unwrap();
        assert_eq!(newer.mark, HintMark::Question);
        assert!(serde_json::from_str::<Hint>(r#"{"about":"x"}"#).is_err());
        assert!(!serde_json::to_string(&Hint::default()).unwrap().contains("mark"));
    }
}
