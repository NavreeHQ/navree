//! What a theme says beyond its [`ThemeSpec`](super::ThemeSpec): a [`Shape`].
//!
//! A spec is a struct apps build literally, so it can't gain a field in 1.x. A shape is a
//! value of its own that a theme of your own carries beside its spec
//! ([`Theme::with_shape`](super::Theme::with_shape)): the corners of each kind of part
//! ([`Corners`]) and the type scale ([`TypeScale`]). Each value is a CSS custom property that
//! the stylesheet reads with today's value as its fallback
//! (`var(--radius-button,var(--radius))`), and a theme's stylesheet writes it only when the
//! shape sets it. So a theme with the default shape, and every
//! built-in theme, writes and draws exactly what it did before shapes existed: the same
//! stylesheet, the same short form and key, the same stored theme and Rust.
//!
//! Every value is typed and bounded, and checked when the theme takes it: nothing in a shape
//! is a CSS string.

use std::fmt::Write as _;
use std::sync::LazyLock;

use serde_json::{Map, Value};

/// A corner: a length, or round ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Corner {
    /// `n` px, from 0 to 32.
    Px(u8),
    /// Round ends whatever the part's height: a pill, or a circle when the part is square.
    /// Written as `999px`.
    Pill,
}

/// The value a pill is kept and written as, in px.
const PILL: i32 = 999;

/// The largest corner in px, short of a pill.
const MOST_CORNER: i32 = 32;

impl Corner {
    /// The value a slot keeps.
    const fn value(self) -> i32 {
        match self {
            Self::Px(n) => n as i32,
            Self::Pill => PILL,
        }
    }

    /// The corner a slot's value is.
    fn of(value: i32) -> Self {
        if value == PILL {
            Self::Pill
        } else {
            Self::Px(u8::try_from(value).unwrap_or(u8::MAX))
        }
    }
}

/// How many kinds of part take a corner of their own.
const PARTS: usize = 9;

/// The corners of each kind of part. A part you don't set keeps the theme's
/// [`Radius`](super::Radius): its control corner for controls and rows, its surface corner
/// for surfaces. A checkbox keeps the control corner always: a round box reads as a radio.
///
/// ```
/// use navree_ui::theme::{Corner, Corners};
///
/// let corners = Corners::new()
///     .button(Corner::Pill)
///     .badge(Corner::Pill)
///     .field(Corner::Px(10))
///     .card(Corner::Px(16));
/// assert!(!corners.is_empty());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[must_use]
pub struct Corners([Option<Corner>; PARTS]);

impl Corners {
    /// No corner of its own for any part.
    pub const fn new() -> Self {
        Self([None; PARTS])
    }

    const fn with(mut self, at: usize, corner: Corner) -> Self {
        self.0[at] = Some(corner);
        self
    }

    /// Buttons, and the outer corners of a button group.
    pub const fn button(self, corner: Corner) -> Self {
        self.with(0, corner)
    }

    /// Badges, and the count on a tab.
    pub const fn badge(self, corner: Corner) -> Self {
        self.with(1, corner)
    }

    /// Toggles, the items of a toggle group, segmented tabs and their track, and the
    /// switch, whose thumb follows it.
    pub const fn toggle(self, corner: Corner) -> Self {
        self.with(2, corner)
    }

    /// Avatars.
    pub const fn avatar(self, corner: Corner) -> Self {
        self.with(3, corner)
    }

    /// Inputs, text areas, native selects, input groups, the select and combobox controls
    /// and the phone input.
    pub const fn field(self, corner: Corner) -> Self {
        self.with(4, corner)
    }

    /// The sidebar's items, its brand and user rows, the top bar's links, and the rows of
    /// menus and popovers.
    pub const fn nav(self, corner: Corner) -> Self {
        self.with(5, corner)
    }

    /// Cards, stats, fact cards, items and tables.
    pub const fn card(self, corner: Corner) -> Self {
        self.with(6, corner)
    }

    /// Alerts, a tab's alert and code blocks.
    pub const fn panel(self, corner: Corner) -> Self {
        self.with(7, corner)
    }

    /// Popovers, hover cards, menus, the select's list, dialogs and drawers.
    pub const fn overlay(self, corner: Corner) -> Self {
        self.with(8, corner)
    }

    /// Whether no part has a corner of its own.
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }
}

/// A face: one of a page's three families. The text face is the theme's [`Font`](super::Font)
/// or the app's own; the display face draws headings, the page title and the brand when the
/// app gives one ([`AppFont::as_face`](crate::AppFont::as_face)), and the text face when it
/// doesn't; the mono face draws code and figures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Face {
    /// The text face: `--font-sans`.
    Text,
    /// The display face: `--font-display`, or the text face when the app has none.
    Display,
    /// The mono face: `--font-mono`.
    Mono,
}

impl Face {
    /// Every face.
    const ALL: [Self; 3] = [Self::Text, Self::Display, Self::Mono];

    /// The id a stored theme names it by.
    const fn id(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Display => "display",
            Self::Mono => "mono",
        }
    }

    /// Its letter in the short form.
    const fn letter(self) -> char {
        match self {
            Self::Text => 't',
            Self::Display => 'd',
            Self::Mono => 'm',
        }
    }

    /// The custom property that holds it, without `--`.
    pub(crate) const fn property(self) -> &'static str {
        match self {
            Self::Text => "font-sans",
            Self::Display => "font-display",
            Self::Mono => "font-mono",
        }
    }

    /// What a face of the app's own falls back to while it loads or where it lacks a glyph.
    pub(crate) const fn fallback(self) -> &'static str {
        match self {
            Self::Text | Self::Display => {
                "ui-sans-serif,system-ui,-apple-system,\"Segoe UI\",Roboto,sans-serif"
            }
            Self::Mono => "ui-monospace,SFMono-Regular,Menlo,Consolas,monospace",
        }
    }

    /// What a text step drawn in it reads.
    const fn css(self) -> &'static str {
        match self {
            Self::Text => "var(--font-sans)",
            Self::Display => "var(--font-display,var(--font-sans))",
            Self::Mono => "var(--font-mono)",
        }
    }

    const fn value(self) -> i32 {
        match self {
            Self::Text => 0,
            Self::Display => 1,
            Self::Mono => 2,
        }
    }

    fn of(value: i32) -> Option<Self> {
        Self::ALL.into_iter().find(|face| face.value() == value)
    }
}

/// How one kind of text is set: its size in px, its line height, its weight, its tracking in
/// em and its [`Face`]. A value you don't set keeps the toolkit's.
///
/// ```
/// use navree_ui::theme::{Face, TextStyle};
///
/// let title = TextStyle::px(28).line(1.15).weight(600).tracking(-0.025).face(Face::Display);
/// let quieter = TextStyle::new().weight(500);
/// assert_ne!(title, quieter);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[must_use]
pub struct TextStyle([Option<i32>; FIELDS]);

/// How many values a text style holds.
const FIELDS: usize = 5;

impl TextStyle {
    /// Nothing set: the toolkit's own.
    pub const fn new() -> Self {
        Self([None; FIELDS])
    }

    /// Text `size` px tall, from 10 to 72.
    pub const fn px(size: u16) -> Self {
        let mut style = Self::new();
        style.0[0] = Some(size as i32);
        style
    }

    /// Its line height, as a multiple of its size: from 1 to 2, to two places.
    pub fn line(mut self, line: f64) -> Self {
        self.0[1] = Some(scaled(line, 100.0));
        self
    }

    /// Its weight: 400, 500, 600 or 700.
    pub const fn weight(mut self, weight: u16) -> Self {
        self.0[2] = Some(weight as i32);
        self
    }

    /// Its tracking (letter spacing) in em: from -0.06 to 0.1, to three places.
    pub fn tracking(mut self, em: f64) -> Self {
        self.0[3] = Some(scaled(em, 1000.0));
        self
    }

    /// The face it is drawn in.
    pub const fn face(mut self, face: Face) -> Self {
        self.0[4] = Some(face.value());
        self
    }
}

/// `value` times `by`, to the nearest whole: what a slot keeps. Anything that isn't a number
/// is kept as a value no slot holds, so it is refused.
fn scaled(value: f64, by: f64) -> i32 {
    let scaled = (value * by).round();
    if scaled.is_finite() && scaled.abs() < 1e6 {
        scaled as i32
    } else {
        i32::MIN
    }
}

/// How many kinds of text take a style of their own.
const STEPS: usize = 16;

/// The type scale: a [`TextStyle`] for each kind of text the toolkit draws that a design sets
/// apart. A kind you don't set keeps the toolkit's. Sizes are pixels, so text keeps its size
/// under every density, as the toolkit's own text does.
///
/// ```
/// use navree_ui::theme::{Face, TextStyle, TypeScale};
///
/// let scale = TypeScale::new()
///     .page_title(TextStyle::px(28).line(1.15).weight(600).tracking(-0.025).face(Face::Display))
///     .card_title(TextStyle::px(15).line(1.4).tracking(-0.01))
///     .stat_value(TextStyle::px(22).weight(500).face(Face::Mono));
/// assert!(!scale.is_empty());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[must_use]
pub struct TypeScale([TextStyle; STEPS]);

impl TypeScale {
    /// Every kind of text as the toolkit sets it.
    pub const fn new() -> Self {
        Self([TextStyle::new(); STEPS])
    }

    const fn with(mut self, at: usize, style: TextStyle) -> Self {
        self.0[at] = style;
        self
    }

    /// A page's title in the app shell. Without it: 20 px, 600, line 1.25, -0.01 em, in the
    /// display face.
    pub const fn page_title(self, style: TextStyle) -> Self {
        self.with(0, style)
    }

    /// A first-level [`Heading`](crate::Heading). Without it: 24 px, 600, line 1.25, -0.01 em,
    /// in the display face.
    pub const fn h1(self, style: TextStyle) -> Self {
        self.with(1, style)
    }

    /// A second-level heading. Without it: 20 px, as `h1` otherwise.
    pub const fn h2(self, style: TextStyle) -> Self {
        self.with(2, style)
    }

    /// A third-level heading. Without it: 16 px, as `h1` otherwise.
    pub const fn h3(self, style: TextStyle) -> Self {
        self.with(3, style)
    }

    /// A fourth-level heading. Without it: 14 px, as `h1` otherwise, in the text face.
    pub const fn h4(self, style: TextStyle) -> Self {
        self.with(4, style)
    }

    /// A card's title. Without it: 14 px, 600.
    pub const fn card_title(self, style: TextStyle) -> Self {
        self.with(5, style)
    }

    /// The app's name: the brand in the sidebar and a site's brand. Without it: the sidebar's
    /// 14 px, 500, and a site's 17 px, 650, -0.015 em; both in the display face.
    pub const fn brand(self, style: TextStyle) -> Self {
        self.with(6, style)
    }

    /// The page's body text. Without it: 14 px, 400, line 1.5.
    pub const fn body(self, style: TextStyle) -> Self {
        self.with(7, style)
    }

    /// A field's label. Without it: 13 px, 500, line 1.25.
    pub const fn label(self, style: TextStyle) -> Self {
        self.with(8, style)
    }

    /// A button of the default size. Without it: 14 px, 500, line 1.
    pub const fn button(self, style: TextStyle) -> Self {
        self.with(9, style)
    }

    /// A badge. Without it: 12 px, 500.
    pub const fn badge(self, style: TextStyle) -> Self {
        self.with(10, style)
    }

    /// The text in an input, a text area, a native select and the select's control. Without
    /// it: 14 px.
    pub const fn input(self, style: TextStyle) -> Self {
        self.with(11, style)
    }

    /// A breadcrumb trail. Without it: 13 px.
    pub const fn breadcrumb(self, style: TextStyle) -> Self {
        self.with(12, style)
    }

    /// A stat's figure. Without it: 24 px, 600, line 1.2, with figures of one width in any
    /// face.
    pub const fn stat_value(self, style: TextStyle) -> Self {
        self.with(13, style)
    }

    /// A table's column headings. Without it: 13 px, 500 in a table, 14 px in a data table.
    pub const fn table_head(self, style: TextStyle) -> Self {
        self.with(14, style)
    }

    /// The signed-in person's name in the sidebar. Without it: 14 px, 500.
    pub const fn sidebar_user(self, style: TextStyle) -> Self {
        self.with(15, style)
    }

    /// Whether no kind of text has a style of its own.
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|style| *style == TextStyle::new())
    }
}

/// Each kind of text: its custom properties' stem, its letter in the short form and the
/// method that sets it.
const STEP_TABLE: [(&str, char, &str); STEPS] = [
    ("text-page-title", 'a', "page_title"),
    ("text-h1", 'b', "h1"),
    ("text-h2", 'c', "h2"),
    ("text-h3", 'd', "h3"),
    ("text-h4", 'e', "h4"),
    ("text-card-title", 'f', "card_title"),
    ("text-brand", 'g', "brand"),
    ("text-body", 'h', "body"),
    ("text-label", 'i', "label"),
    ("text-button", 'j', "button"),
    ("text-badge", 'k', "badge"),
    ("text-input", 'l', "input"),
    ("text-breadcrumb", 'm', "breadcrumb"),
    ("text-stat-value", 'n', "stat_value"),
    ("text-table-head", 'o', "table_head"),
    ("text-sidebar-user", 'p', "sidebar_user"),
];

/// Each value of a text style: its custom property's suffix, its kind and its method.
const FIELD_TABLE: [(&str, Kind, &str); FIELDS] = [
    ("size", Kind::Size, "px"),
    ("line", Kind::Line, "line"),
    ("weight", Kind::Weight, "weight"),
    ("tracking", Kind::Tracking, "tracking"),
    ("face", Kind::Face, "face"),
];

/// What kind of value a slot holds, and so how it is checked, written and read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A corner: 0 to 32 px, or a pill.
    Corner,
    /// A text size: 10 to 72 px.
    Size,
    /// A line height in hundredths: 100 to 200.
    Line,
    /// A weight: 400, 500, 600 or 700.
    Weight,
    /// Tracking in thousandths of an em: -60 to 100.
    Tracking,
    /// A [`Face`].
    Face,
}

/// What the short form adds to a tracking value, so it is never negative.
const TRACKING_OFFSET: i32 = 60;

impl Kind {
    /// How many characters a value takes in the short form.
    const fn width(self) -> usize {
        match self {
            Self::Corner | Self::Weight | Self::Face => 1,
            Self::Size | Self::Line | Self::Tracking => 2,
        }
    }

    /// Whether `value` is in range.
    fn holds(self, value: i32) -> bool {
        match self {
            Self::Corner => (0..=MOST_CORNER).contains(&value) || value == PILL,
            Self::Size => (10..=72).contains(&value),
            Self::Line => (100..=200).contains(&value),
            Self::Weight => matches!(value, 400 | 500 | 600 | 700),
            Self::Tracking => (-60..=100).contains(&value),
            Self::Face => Face::of(value).is_some(),
        }
    }

    /// Why a value out of range is refused.
    const fn rule(self) -> &'static str {
        match self {
            Self::Corner => "A corner takes 0 to 32 px, or a pill.",
            Self::Size => "A text size takes 10 to 72 px.",
            Self::Line => "A line height takes 1 to 2, to two places.",
            Self::Weight => "A weight is 400, 500, 600 or 700.",
            Self::Tracking => "Tracking takes -0.06 to 0.1 em, to three places.",
            Self::Face => "A face is text, display or mono.",
        }
    }

    /// The value as CSS.
    fn css(self, value: i32, out: &mut String) {
        match self {
            Self::Corner | Self::Size => {
                let _ = write!(out, "{value}px");
            }
            Self::Line => out.push_str(&decimal(value, 2)),
            Self::Weight => {
                let _ = write!(out, "{value}");
            }
            Self::Tracking => {
                let _ = write!(out, "{}em", decimal(value, 3));
            }
            Self::Face => out.push_str(Face::of(value).map_or("inherit", Face::css)),
        }
    }

    /// The value in the short form: only `[0-9a-z]`.
    fn encode(self, value: i32, out: &mut String) {
        let digit = |n: i32| char::from_digit(n.unsigned_abs() % 36, 36).unwrap_or(UNSET);
        match self {
            Self::Corner if value == PILL => out.push('x'),
            Self::Corner => out.push(digit(value)),
            Self::Weight => out.push(digit(value / 100)),
            Self::Face => out.push(Face::of(value).map_or(UNSET, Face::letter)),
            Self::Size | Self::Line | Self::Tracking => {
                let value = if self == Self::Tracking {
                    value + TRACKING_OFFSET
                } else {
                    value
                };
                out.push(digit(value / 36));
                out.push(digit(value));
            }
        }
    }

    /// The value a short form's characters hold, if they are one, in range.
    fn decode(self, text: &str) -> Option<i32> {
        if !text
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
        {
            return None;
        }
        let number = || i32::try_from(u32::from_str_radix(text, 36).ok()?).ok();
        let value = match self {
            Self::Corner if text == "x" => PILL,
            Self::Corner | Self::Size | Self::Line => number()?,
            Self::Weight => number()? * 100,
            Self::Tracking => number()? - TRACKING_OFFSET,
            Self::Face => Face::ALL
                .into_iter()
                .find(|face| text.len() == 1 && text.starts_with(face.letter()))?
                .value(),
        };
        self.holds(value).then_some(value)
    }

    /// The value in a stored theme.
    fn json(self, value: i32) -> String {
        match self {
            Self::Corner if value == PILL => "pill".to_owned(),
            Self::Corner | Self::Size => format!("{value}px"),
            Self::Line => decimal(value, 2),
            Self::Weight => value.to_string(),
            Self::Tracking => format!("{}em", decimal(value, 3)),
            Self::Face => Face::of(value).map_or("", Face::id).to_owned(),
        }
    }

    /// The value a stored theme's text holds, if it is one, in range.
    fn parse_json(self, text: &str) -> Option<i32> {
        let value = match self {
            Self::Corner if text == "pill" => PILL,
            Self::Corner | Self::Size => {
                let digits = text.strip_suffix("px")?;
                if digits.is_empty()
                    || digits.len() > 3
                    || !digits.bytes().all(|b| b.is_ascii_digit())
                {
                    return None;
                }
                digits.parse().ok()?
            }
            Self::Line => parse_decimal(text, 2)?,
            Self::Weight => {
                if !text.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                text.parse().ok()?
            }
            Self::Tracking => parse_decimal(text.strip_suffix("em")?, 3)?,
            Self::Face => Face::ALL
                .into_iter()
                .find(|face| face.id() == text)?
                .value(),
        };
        self.holds(value).then_some(value)
    }

    /// The value as Rust, for [`Theme::to_rust`](super::Theme::to_rust).
    fn rust(self, value: i32) -> String {
        match self {
            Self::Corner => match Corner::of(value) {
                Corner::Pill => "Corner::Pill".to_owned(),
                Corner::Px(n) => format!("Corner::Px({n})"),
            },
            Self::Size | Self::Weight => value.to_string(),
            Self::Line | Self::Tracking => {
                let text = decimal(value, if self == Self::Line { 2 } else { 3 });
                if text.contains('.') {
                    text
                } else {
                    format!("{text}.0")
                }
            }
            Self::Face => match Face::of(value) {
                Some(Face::Display) => "Face::Display".to_owned(),
                Some(Face::Mono) => "Face::Mono".to_owned(),
                _ => "Face::Text".to_owned(),
            },
        }
    }
}

/// `value` with `places` decimal places, without trailing zeros: `115, 2` is `1.15`, `-25, 3`
/// is `-0.025`, `100, 2` is `1`.
fn decimal(value: i32, places: u32) -> String {
    let scale = 10_i32.pow(places);
    let (whole, part) = (value.abs() / scale, value.abs() % scale);
    let sign = if value < 0 { "-" } else { "" };
    if part == 0 {
        return format!("{sign}{whole}");
    }
    let digits = format!("{part:0width$}", width = places as usize);
    format!("{sign}{whole}.{}", digits.trim_end_matches('0'))
}

/// A decimal with at most `places` places, as a whole number of its smallest unit:
/// `1.15, 2` is `115`. `None` for anything else.
fn parse_decimal(text: &str, places: u32) -> Option<i32> {
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text),
    };
    let (whole, part) = digits.split_once('.').unwrap_or((digits, ""));
    let ok = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty()
        || whole.len() > 3
        || !ok(whole)
        || !ok(part)
        || part.len() > places as usize
    {
        return None;
    }
    if digits.ends_with('.') {
        return None;
    }
    let scale = 10_i32.pow(places);
    let part: i32 = format!("{part:0<width$}", width = places as usize)
        .parse()
        .ok()?;
    Some(sign * (whole.parse::<i32>().ok()? * scale + part))
}

/// One value a shape can set.
struct Slot {
    /// The custom property's stem and suffix, without `--`: `radius-button` and nothing, or
    /// `text-h1` and `size`. Its whole name is also its name in a stored theme and in errors.
    stem: &'static str,
    suffix: &'static str,
    kind: Kind,
    /// Its group's tag in the short form, and its entry's letter in the group. The slots of
    /// one entry follow each other.
    group: char,
    key: char,
    /// The builder method that sets it, for the Rust export: its group's builder's for a
    /// corner, its text style's for a value of one.
    method: &'static str,
}

impl Slot {
    /// The custom property, without `--`.
    fn name(&self) -> String {
        if self.suffix.is_empty() {
            self.stem.to_owned()
        } else {
            format!("{}-{}", self.stem, self.suffix)
        }
    }
}

/// Each kind of part's corner: its custom property, its letter and its method.
const CORNER_TABLE: [(&str, char, &str); PARTS] = [
    ("radius-button", 'a', "button"),
    ("radius-badge", 'b', "badge"),
    ("radius-toggle", 'c', "toggle"),
    ("radius-avatar", 'd', "avatar"),
    ("radius-field", 'e', "field"),
    ("radius-nav", 'f', "nav"),
    ("radius-card", 'g', "card"),
    ("radius-panel", 'h', "panel"),
    ("radius-overlay", 'i', "overlay"),
];

/// Where the type scale's slots start.
const TYPE_AT: usize = PARTS;

/// How many slots there are.
const SLOT_COUNT: usize = TYPE_AT + STEPS * FIELDS;

/// Every value a shape can set, in the order a sheet writes them: the corners, one slot a
/// part, then the type scale, five slots a kind of text.
fn slots() -> &'static [Slot] {
    static SLOTS: LazyLock<Vec<Slot>> = LazyLock::new(|| {
        let mut slots = Vec::with_capacity(SLOT_COUNT);
        for (stem, key, method) in CORNER_TABLE {
            slots.push(Slot {
                stem,
                suffix: "",
                kind: Kind::Corner,
                group: 'c',
                key,
                method,
            });
        }
        for (stem, key, _) in STEP_TABLE {
            for (suffix, kind, method) in FIELD_TABLE {
                slots.push(Slot {
                    stem,
                    suffix,
                    kind,
                    group: 't',
                    key,
                    method,
                });
            }
        }
        slots
    });
    &SLOTS
}

/// The short form's groups, in the order they are written.
const GROUPS: [char; 2] = ['c', 't'];

/// The marker of a value left unset inside a short form's entry: a letter no value uses.
const UNSET: char = 'z';

/// Everything a theme says beyond its [`ThemeSpec`](super::ThemeSpec) but colours: the
/// corners of each kind of part and the type scale. A theme takes it with
/// [`Theme::with_shape`](super::Theme::with_shape), which checks every value; a built-in
/// theme has the default shape, which sets nothing.
///
/// ```
/// use navree_ui::theme::{Corner, Corners, Face, Shape, TextStyle, Theme, ThemeId, TypeScale};
///
/// let shape = Shape::new()
///     .corners(
///         Corners::new()
///             .button(Corner::Pill)
///             .badge(Corner::Pill)
///             .field(Corner::Px(12))
///             .card(Corner::Px(16)),
///     )
///     .type_scale(TypeScale::new().page_title(TextStyle::px(32).face(Face::Display)));
/// let theme = Theme::NAVREE
///     .renamed(ThemeId::parse("agra").unwrap(), "Agra")
///     .with_shape(shape)
///     .unwrap();
/// let css = theme.css().unwrap();
/// assert!(css.contains("--radius-button:999px"));
/// assert!(css.contains("--text-page-title-size:32px"));
/// assert!(theme.short_form().starts_with("v1-"));
///
/// let refused = Shape::new().corners(Corners::new().card(Corner::Px(33)));
/// assert!(Theme::NAVREE.renamed(ThemeId::parse("agra").unwrap(), "Agra").with_shape(refused).is_err());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[must_use]
pub struct Shape {
    corners: Corners,
    type_scale: TypeScale,
}

impl Shape {
    /// Sets nothing: every part as the spec draws it.
    pub const fn new() -> Self {
        Self {
            corners: Corners::new(),
            type_scale: TypeScale::new(),
        }
    }

    /// The corners of each kind of part, in place of any set before.
    pub const fn corners(mut self, corners: Corners) -> Self {
        self.corners = corners;
        self
    }

    /// The type scale, in place of any set before.
    pub const fn type_scale(mut self, type_scale: TypeScale) -> Self {
        self.type_scale = type_scale;
        self
    }

    /// Whether it sets nothing.
    pub fn is_default(&self) -> bool {
        *self == Self::new()
    }

    /// The value of slot `at`, if set.
    fn get(&self, at: usize) -> Option<i32> {
        match at {
            0..TYPE_AT => self.corners.0[at].map(Corner::value),
            TYPE_AT..SLOT_COUNT => {
                let at = at - TYPE_AT;
                self.type_scale.0[at / FIELDS].0[at % FIELDS]
            }
            _ => None,
        }
    }

    /// Sets slot `at` to a value already checked.
    fn set(&mut self, at: usize, value: i32) {
        match at {
            0..TYPE_AT => self.corners.0[at] = Some(Corner::of(value)),
            TYPE_AT..SLOT_COUNT => {
                let at = at - TYPE_AT;
                self.type_scale.0[at / FIELDS].0[at % FIELDS] = Some(value);
            }
            _ => {}
        }
    }

    /// Every slot it sets, with its value, in [`slots`]' order.
    fn values(&self) -> impl Iterator<Item = (&'static Slot, i32)> + '_ {
        slots()
            .iter()
            .enumerate()
            .filter_map(|(at, slot)| self.get(at).map(|value| (slot, value)))
    }

    /// The values out of range, each by its name with why: empty when every value holds.
    pub(crate) fn errors(&self) -> Vec<(String, String)> {
        self.values()
            .filter(|(slot, value)| !slot.kind.holds(*value))
            .map(|(slot, _)| (slot.name(), slot.kind.rule().to_owned()))
            .collect()
    }

    /// `;--name:value` for every value it sets.
    pub(crate) fn css(&self, out: &mut String) {
        for (slot, value) in self.values() {
            let _ = write!(out, ";--{}:", slot.name());
            slot.kind.css(value, out);
        }
    }

    /// The custom properties it sets, which a part of the page drawn in another theme must
    /// not inherit.
    pub(crate) fn names(&self) -> impl Iterator<Item = String> + '_ {
        self.values().map(|(slot, _)| slot.name())
    }

    /// Its part of the short form: a `-` and a group's tag, then an entry for each key with a
    /// value set (the key, then each of its slots' value or `z`s), for each group that has
    /// one. Nothing for the default shape.
    pub(crate) fn form(&self, out: &mut String) {
        let slots = slots();
        for group in GROUPS {
            let mut written = false;
            let mut at = 0;
            while at < slots.len() {
                let (own, key) = (slots[at].group, slots[at].key);
                // The entry: this slot and the ones after it with its group and key.
                let end = slots[at..]
                    .iter()
                    .position(|slot| slot.group != own || slot.key != key)
                    .map_or(slots.len(), |len| at + len);
                if own == group && (at..end).any(|i| self.get(i).is_some()) {
                    if !written {
                        out.push('-');
                        out.push(group);
                        written = true;
                    }
                    out.push(key);
                    for (i, slot) in slots.iter().enumerate().take(end).skip(at) {
                        match self.get(i) {
                            Some(value) => slot.kind.encode(value, out),
                            None => (0..slot.kind.width()).for_each(|_| out.push(UNSET)),
                        }
                    }
                }
                at = end;
            }
        }
    }

    /// Reads the short form's part for `group` (the text after its tag), into this shape.
    /// `None` for anything but entries in order, each in range.
    pub(crate) fn read_group(&mut self, group: char, text: &str) -> Option<()> {
        let slots = slots();
        let mut rest = text;
        let mut last = None;
        if rest.is_empty() {
            return None;
        }
        while let Some(key) = rest.chars().next() {
            if last.is_some_and(|last| key <= last) {
                return None;
            }
            last = Some(key);
            rest = &rest[key.len_utf8()..];
            let first = slots
                .iter()
                .position(|slot| slot.group == group && slot.key == key)?;
            let mut any = false;
            for (at, slot) in slots.iter().enumerate().skip(first) {
                if slot.group != group || slot.key != key {
                    break;
                }
                let width = slot.kind.width();
                let code = rest.get(..width)?;
                rest = &rest[width..];
                if code.chars().all(|c| c == UNSET) {
                    continue;
                }
                self.set(at, slot.kind.decode(code)?);
                any = true;
            }
            if !any {
                return None;
            }
        }
        Some(())
    }

    /// Whether `group` is one of the short form's.
    pub(crate) fn has_group(group: char) -> bool {
        GROUPS.contains(&group)
    }

    /// Its stored form: each value by its name.
    pub(crate) fn json(&self) -> Value {
        let mut map = Map::new();
        for (slot, value) in self.values() {
            map.insert(slot.name(), Value::String(slot.kind.json(value)));
        }
        Value::Object(map)
    }

    /// Reads a stored shape, naming each field that is wrong under `shape.`.
    pub(crate) fn from_json(value: &Value, errors: &mut Vec<(String, String)>) -> Self {
        let mut shape = Self::new();
        let Value::Object(map) = value else {
            errors.push((
                "shape".to_owned(),
                "Give the shape's values by name, such as \"radius-card\": \"16px\".".to_owned(),
            ));
            return shape;
        };
        for (name, value) in map {
            let at = format!("shape.{}", super::codec::clip(name));
            let Some(index) = slots().iter().position(|slot| slot.name() == *name) else {
                errors.push((at, "This is not part of a shape.".to_owned()));
                continue;
            };
            let kind = slots()[index].kind;
            match value.as_str().and_then(|text| kind.parse_json(text)) {
                Some(value) => shape.set(index, value),
                None => errors.push((at, kind.rule().to_owned())),
            }
        }
        shape
    }

    /// The shape as Rust: `Shape::new()` and a call for each group it sets.
    pub(crate) fn rust(&self) -> String {
        let mut out = String::from("Shape::new()");
        if !self.corners.is_empty() {
            out.push_str("\n        .corners(Corners::new()");
            for (slot, value) in self.values().filter(|(slot, _)| slot.group == 'c') {
                let _ = write!(out, ".{}({})", slot.method, slot.kind.rust(value));
            }
            out.push(')');
        }
        if !self.type_scale.is_empty() {
            out.push_str("\n        .type_scale(\n            TypeScale::new()");
            for (step, style) in STEP_TABLE.iter().zip(self.type_scale.0) {
                if style == TextStyle::new() {
                    continue;
                }
                let _ = write!(out, "\n                .{}(", step.2);
                out.push_str(&match style.0[0] {
                    Some(size) => format!("TextStyle::px({size})"),
                    None => "TextStyle::new()".to_owned(),
                });
                for ((_, kind, method), value) in FIELD_TABLE.iter().zip(style.0).skip(1) {
                    if let Some(value) = value {
                        let _ = write!(out, ".{method}({})", kind.rust(value));
                    }
                }
                out.push(')');
            }
            out.push_str(",\n        )");
        }
        out
    }

    /// The names a Rust export of it uses from the theme module.
    pub(crate) fn rust_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if !self.corners.is_empty() {
            names.extend(["Corner", "Corners"]);
        }
        if self.type_scale.0.iter().any(|style| style.0[4].is_some()) {
            names.push("Face");
        }
        if !self.type_scale.is_empty() {
            names.extend(["TextStyle", "TypeScale"]);
        }
        if !self.is_default() {
            names.push("Shape");
        }
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_corner() -> Shape {
        Shape::new().corners(
            Corners::new()
                .button(Corner::Pill)
                .badge(Corner::Pill)
                .toggle(Corner::Pill)
                .avatar(Corner::Pill)
                .field(Corner::Px(12))
                .nav(Corner::Px(10))
                .card(Corner::Px(16))
                .panel(Corner::Px(0))
                .overlay(Corner::Px(32)),
        )
    }

    fn a_type_scale() -> TypeScale {
        TypeScale::new()
            .page_title(
                TextStyle::px(28)
                    .line(1.15)
                    .weight(600)
                    .tracking(-0.025)
                    .face(Face::Display),
            )
            .brand(TextStyle::px(18).weight(600).tracking(-0.03))
            .card_title(TextStyle::px(15).line(1.4).tracking(-0.01))
            .stat_value(TextStyle::px(22).weight(500).face(Face::Mono))
            .sidebar_user(TextStyle::new().weight(500).tracking(0.0))
    }

    #[test]
    fn every_slot_has_a_unique_name_and_each_entry_a_unique_key() {
        let slots = slots();
        assert_eq!(slots.len(), SLOT_COUNT);
        for (at, slot) in slots.iter().enumerate() {
            assert!(
                !slots[..at].iter().any(|other| other.name() == slot.name()),
                "{}",
                slot.name()
            );
            assert!(GROUPS.contains(&slot.group), "{}", slot.name());
            assert!(
                slot.key.is_ascii_lowercase() && slot.key != UNSET,
                "{}",
                slot.name()
            );
        }
        let shape = every_corner();
        let names: Vec<String> = shape.names().collect();
        assert_eq!(
            names,
            [
                "radius-button",
                "radius-badge",
                "radius-toggle",
                "radius-avatar",
                "radius-field",
                "radius-nav",
                "radius-card",
                "radius-panel",
                "radius-overlay"
            ]
        );
    }

    #[test]
    fn a_shape_writes_its_values_and_reads_them_back() {
        let shape = every_corner().type_scale(a_type_scale());
        let mut css = String::new();
        shape.css(&mut css);
        assert!(css.starts_with(";--radius-button:999px;"), "{css}");
        assert!(css.contains(";--radius-card:16px;"), "{css}");
        assert!(css.contains(";--radius-panel:0px;"), "{css}");
        for part in [
            ";--text-page-title-size:28px;--text-page-title-line:1.15;--text-page-title-weight:600;--text-page-title-tracking:-0.025em;--text-page-title-face:var(--font-display,var(--font-sans))",
            ";--text-card-title-line:1.4;",
            ";--text-stat-value-face:var(--font-mono)",
            ";--text-sidebar-user-tracking:0em",
        ] {
            assert!(css.contains(part), "{part}: {css}");
        }
        let mut form = String::new();
        shape.form(&mut form);
        assert!(
            form.starts_with("-caxbxcxdxecfaggh0iw-ta0s3760zd"),
            "{form}"
        );
        let mut back = Shape::new();
        for group in form[1..].split('-') {
            back.read_group(group.chars().next().unwrap(), &group[1..])
                .unwrap();
        }
        assert_eq!(back, shape);
        let mut errors = Vec::new();
        assert_eq!(Shape::from_json(&shape.json(), &mut errors), shape);
        assert!(errors.is_empty(), "{errors:?}");
        let mut nothing = String::new();
        Shape::new().form(&mut nothing);
        assert_eq!(
            nothing, "",
            "the default shape adds nothing to a short form"
        );
    }

    #[test]
    fn a_short_forms_shape_has_one_spelling() {
        for (group, bad) in [
            ('c', ""),
            ('c', "a"),
            ('c', "ay"),
            ('c', "az"),
            ('c', "bxax"),
            ('c', "axax"),
            ('c', "a!"),
            ('c', "jx"),
            ('c', "aX"),
            ('c', "a0b"),
            ('c', "ax-"),
            ('t', "azzzzzzzz"),
            ('t', "a0szzzzzzz"),
            ('t', "a09zzzzzz"),
            ('t', "a0szzzzzx"),
            ('t', "a0szz8zzz"),
            ('t', "q0szzzzzz"),
        ] {
            let mut shape = Shape::new();
            assert_eq!(shape.read_group(group, bad), None, "{group}{bad:?}");
        }
    }

    #[test]
    fn values_out_of_range_are_refused_by_name() {
        let shape = Shape::new()
            .corners(Corners::new().card(Corner::Px(33)).button(Corner::Px(32)))
            .type_scale(
                TypeScale::new()
                    .h1(TextStyle::px(80).line(2.5).weight(650).tracking(0.2))
                    .body(TextStyle::px(9).line(f64::NAN))
                    .label(TextStyle::px(13).line(1.0).tracking(-0.06)),
            );
        let fields: Vec<(String, String)> = shape.errors();
        let at: Vec<&str> = fields.iter().map(|(at, _)| at.as_str()).collect();
        assert_eq!(
            at,
            [
                "radius-card",
                "text-h1-size",
                "text-h1-line",
                "text-h1-weight",
                "text-h1-tracking",
                "text-body-size",
                "text-body-line"
            ]
        );
        assert_eq!(fields[0].1, "A corner takes 0 to 32 px, or a pill.");
        let mut errors = Vec::new();
        let value: Value = serde_json::json!({
            "radius-card": "40px",
            "radius-pill": "pill",
            "radius-nav": 4,
            "text-h1-size": "28",
            "text-h1-line": "1.155",
            "text-h1-face": "serif",
            "text-h1-tracking": "-0.02em",
        });
        let shape = Shape::from_json(&value, &mut errors);
        let mut at: Vec<&str> = errors.iter().map(|(at, _)| at.as_str()).collect();
        at.sort_unstable();
        assert_eq!(
            at,
            [
                "shape.radius-card",
                "shape.radius-nav",
                "shape.radius-pill",
                "shape.text-h1-face",
                "shape.text-h1-line",
                "shape.text-h1-size"
            ]
        );
        assert_eq!(
            shape,
            Shape::new().type_scale(TypeScale::new().h1(TextStyle::new().tracking(-0.02)))
        );
    }

    #[test]
    fn decimals_are_written_short_and_read_strictly() {
        assert_eq!(decimal(115, 2), "1.15");
        assert_eq!(decimal(120, 2), "1.2");
        assert_eq!(decimal(100, 2), "1");
        assert_eq!(decimal(-25, 3), "-0.025");
        assert_eq!(decimal(0, 3), "0");
        assert_eq!(decimal(100, 3), "0.1");
        assert_eq!(parse_decimal("1.15", 2), Some(115));
        assert_eq!(parse_decimal("-0.025", 3), Some(-25));
        assert_eq!(parse_decimal("2", 2), Some(200));
        for bad in ["", ".5", "1.", "1.155", "+1", "1e2", "1,5", "--1", "1.5x"] {
            assert_eq!(parse_decimal(bad, 2), None, "{bad:?}");
        }
    }

    #[test]
    fn the_rust_of_a_shape_calls_its_builders() {
        let shape = Shape::new()
            .corners(Corners::new().button(Corner::Pill).card(Corner::Px(16)))
            .type_scale(
                TypeScale::new()
                    .page_title(TextStyle::px(32).line(1.1).face(Face::Display))
                    .sidebar_user(TextStyle::new().weight(500).tracking(0.0)),
            );
        assert_eq!(
            shape.rust(),
            "Shape::new()\n        .corners(Corners::new().button(Corner::Pill).card(Corner::Px(16)))\n        .type_scale(\n            TypeScale::new()\n                .page_title(TextStyle::px(32).line(1.1).face(Face::Display))\n                .sidebar_user(TextStyle::new().weight(500).tracking(0.0)),\n        )"
        );
        assert_eq!(
            shape.rust_names(),
            [
                "Corner",
                "Corners",
                "Face",
                "TextStyle",
                "TypeScale",
                "Shape"
            ]
        );
        assert!(Shape::new().rust_names().is_empty());
    }
}
