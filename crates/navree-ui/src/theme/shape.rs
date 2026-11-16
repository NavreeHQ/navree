//! What a theme says beyond its [`ThemeSpec`](super::ThemeSpec): a [`Shape`].
//!
//! A spec is a struct apps build literally, so it can't gain a field in 1.x. A shape is a
//! value of its own that a theme of your own carries beside its spec
//! ([`Theme::with_shape`](super::Theme::with_shape)): the corners of each kind of part
//! ([`Corners`]). Each value is a CSS custom property that the stylesheet reads with today's
//! value as its fallback (`var(--radius-button,var(--radius))`), and a theme's stylesheet
//! writes it only when the shape sets it. So a theme with the default shape, and every
//! built-in theme, writes and draws exactly what it did before shapes existed: the same
//! stylesheet, the same short form and key, the same stored theme and Rust.
//!
//! Every value is typed and bounded, and checked when the theme takes it: nothing in a shape
//! is a CSS string.

use std::fmt::Write as _;

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

/// What kind of value a slot holds, and so how it is checked, written and read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A corner: 0 to 32 px, or a pill.
    Corner,
}

impl Kind {
    /// How many characters a value takes in the short form.
    const fn width(self) -> usize {
        match self {
            Self::Corner => 1,
        }
    }

    /// Whether `value` is in range.
    fn holds(self, value: i32) -> bool {
        match self {
            Self::Corner => (0..=MOST_CORNER).contains(&value) || value == PILL,
        }
    }

    /// Why a value out of range is refused.
    const fn rule(self) -> &'static str {
        match self {
            Self::Corner => "A corner takes 0 to 32 px, or a pill.",
        }
    }

    /// The value as CSS.
    fn css(self, value: i32, out: &mut String) {
        match self {
            Self::Corner => {
                let _ = write!(out, "{value}px");
            }
        }
    }

    /// The value in the short form: only `[0-9a-z]`.
    fn encode(self, value: i32, out: &mut String) {
        match self {
            Self::Corner => out.push(if value == PILL {
                'x'
            } else {
                char::from_digit(value.unsigned_abs(), 36).unwrap_or('z')
            }),
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
        let value = match self {
            Self::Corner => match text {
                "x" => PILL,
                _ => i32::try_from(u32::from_str_radix(text, 36).ok()?).ok()?,
            },
        };
        self.holds(value).then_some(value)
    }

    /// The value in a stored theme.
    fn json(self, value: i32) -> String {
        match self {
            Self::Corner if value == PILL => "pill".to_owned(),
            Self::Corner => format!("{value}px"),
        }
    }

    /// The value a stored theme's text holds, if it is one, in range.
    fn parse_json(self, text: &str) -> Option<i32> {
        let value = match self {
            Self::Corner => match text {
                "pill" => PILL,
                _ => i32::from(super::Radius::parse(text).ok()?),
            },
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
        }
    }
}

/// One value a shape can set.
struct Slot {
    /// The custom property, without `--`: also its name in a stored theme and in errors.
    name: &'static str,
    kind: Kind,
    /// Its group's tag in the short form, and its entry's letter in the group. The slots of
    /// one entry follow each other.
    group: char,
    key: char,
    /// The builder method that sets it, for the Rust export.
    method: &'static str,
}

const fn slot(
    name: &'static str,
    kind: Kind,
    group: char,
    key: char,
    method: &'static str,
) -> Slot {
    Slot {
        name,
        kind,
        group,
        key,
        method,
    }
}

/// Every value a shape can set, in the order a sheet writes them. Corners first, one slot a
/// part, in [`Corners`]' order.
const SLOTS: [Slot; PARTS] = [
    slot("radius-button", Kind::Corner, 'c', 'a', "button"),
    slot("radius-badge", Kind::Corner, 'c', 'b', "badge"),
    slot("radius-toggle", Kind::Corner, 'c', 'c', "toggle"),
    slot("radius-avatar", Kind::Corner, 'c', 'd', "avatar"),
    slot("radius-field", Kind::Corner, 'c', 'e', "field"),
    slot("radius-nav", Kind::Corner, 'c', 'f', "nav"),
    slot("radius-card", Kind::Corner, 'c', 'g', "card"),
    slot("radius-panel", Kind::Corner, 'c', 'h', "panel"),
    slot("radius-overlay", Kind::Corner, 'c', 'i', "overlay"),
];

/// The short form's groups, in the order they are written.
const GROUPS: [char; 1] = ['c'];

/// The marker of a value left unset inside a short form's entry: a letter no value uses.
const UNSET: char = 'z';

/// Everything a theme says beyond its [`ThemeSpec`](super::ThemeSpec): the corners of each
/// kind of part. A theme takes it with [`Theme::with_shape`](super::Theme::with_shape),
/// which checks every value; a built-in theme has the default shape, which sets nothing.
///
/// ```
/// use navree_ui::theme::{Corner, Corners, Shape, Theme, ThemeId};
///
/// let shape = Shape::new().corners(
///     Corners::new()
///         .button(Corner::Pill)
///         .badge(Corner::Pill)
///         .field(Corner::Px(12))
///         .card(Corner::Px(16)),
/// );
/// let theme = Theme::NAVREE
///     .renamed(ThemeId::parse("agra").unwrap(), "Agra")
///     .with_shape(shape)
///     .unwrap();
/// assert!(theme.css().unwrap().contains("--radius-button:999px"));
/// assert!(theme.short_form().starts_with("v1-"));
///
/// let refused = Shape::new().corners(Corners::new().card(Corner::Px(33)));
/// assert!(Theme::NAVREE.renamed(ThemeId::parse("agra").unwrap(), "Agra").with_shape(refused).is_err());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[must_use]
pub struct Shape {
    corners: Corners,
}

impl Shape {
    /// Sets nothing: every part as the spec draws it.
    pub const fn new() -> Self {
        Self {
            corners: Corners::new(),
        }
    }

    /// The corners of each kind of part, in place of any set before.
    pub const fn corners(mut self, corners: Corners) -> Self {
        self.corners = corners;
        self
    }

    /// Whether it sets nothing.
    pub fn is_default(&self) -> bool {
        *self == Self::new()
    }

    /// The value of slot `at`, if set.
    fn get(&self, at: usize) -> Option<i32> {
        match at {
            0..PARTS => self.corners.0[at].map(Corner::value),
            _ => None,
        }
    }

    /// Sets slot `at` to a value already checked.
    fn set(&mut self, at: usize, value: i32) {
        if at < PARTS {
            self.corners.0[at] = Some(Corner::of(value));
        }
    }

    /// Every slot it sets, with its value, in [`SLOTS`]' order.
    fn values(&self) -> impl Iterator<Item = (&'static Slot, i32)> + '_ {
        SLOTS
            .iter()
            .enumerate()
            .filter_map(|(at, slot)| self.get(at).map(|value| (slot, value)))
    }

    /// The values out of range, each by its name with why: empty when every value holds.
    pub(crate) fn errors(&self) -> Vec<(String, String)> {
        self.values()
            .filter(|(slot, value)| !slot.kind.holds(*value))
            .map(|(slot, _)| (slot.name.to_owned(), slot.kind.rule().to_owned()))
            .collect()
    }

    /// `;--name:value` for every value it sets.
    pub(crate) fn css(&self, out: &mut String) {
        for (slot, value) in self.values() {
            let _ = write!(out, ";--{}:", slot.name);
            slot.kind.css(value, out);
        }
    }

    /// The custom properties it sets, which a part of the page drawn in another theme must
    /// not inherit.
    pub(crate) fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.values().map(|(slot, _)| slot.name)
    }

    /// Its part of the short form: a `-` and a group's tag, then an entry for each key with a
    /// value set (the key, then each of its slots' value or `z`s), for each group that has
    /// one. Nothing for the default shape.
    pub(crate) fn form(&self, out: &mut String) {
        for group in GROUPS {
            let mut written = false;
            let mut at = 0;
            while at < SLOTS.len() {
                let (own, key) = (SLOTS[at].group, SLOTS[at].key);
                // The entry: this slot and the ones after it with its group and key.
                let end = SLOTS[at..]
                    .iter()
                    .position(|slot| slot.group != own || slot.key != key)
                    .map_or(SLOTS.len(), |len| at + len);
                if own == group && (at..end).any(|i| self.get(i).is_some()) {
                    if !written {
                        out.push('-');
                        out.push(group);
                        written = true;
                    }
                    out.push(key);
                    for (i, slot) in SLOTS.iter().enumerate().take(end).skip(at) {
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
            let first = SLOTS
                .iter()
                .position(|slot| slot.group == group && slot.key == key)?;
            let mut any = false;
            for (at, slot) in SLOTS.iter().enumerate().skip(first) {
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
            map.insert(slot.name.to_owned(), Value::String(slot.kind.json(value)));
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
            let Some(index) = SLOTS.iter().position(|slot| slot.name == name) else {
                errors.push((at, "This is not part of a shape.".to_owned()));
                continue;
            };
            let kind = SLOTS[index].kind;
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
        out
    }

    /// The names a Rust export of it uses from the theme module.
    pub(crate) fn rust_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if !self.corners.is_empty() {
            names.extend(["Corner", "Corners"]);
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

    #[test]
    fn every_slot_has_a_unique_name_and_key_and_the_corners_are_in_order() {
        for (at, slot) in SLOTS.iter().enumerate() {
            assert!(
                !SLOTS[..at].iter().any(|other| other.name == slot.name),
                "{}",
                slot.name
            );
            assert!(GROUPS.contains(&slot.group), "{}", slot.name);
            assert!(
                slot.key.is_ascii_lowercase() && slot.key != UNSET,
                "{}",
                slot.name
            );
        }
        let shape = every_corner();
        let names: Vec<&str> = shape.names().collect();
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
        let shape = every_corner();
        let mut css = String::new();
        shape.css(&mut css);
        assert!(css.starts_with(";--radius-button:999px;"), "{css}");
        assert!(css.contains(";--radius-card:16px;"), "{css}");
        assert!(css.contains(";--radius-panel:0px;"), "{css}");
        let mut form = String::new();
        shape.form(&mut form);
        assert_eq!(form, "-caxbxcxdxecfaggh0iw");
        let mut back = Shape::new();
        back.read_group('c', &form[2..]).unwrap();
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
        for bad in [
            "", "a", "ay", "az", "bxax", "axax", "a!", "jx", "aX", "a0b", "ax-",
        ] {
            let mut shape = Shape::new();
            assert_eq!(shape.read_group('c', bad), None, "{bad:?}");
        }
    }

    #[test]
    fn corners_out_of_range_are_refused_by_name() {
        let shape =
            Shape::new().corners(Corners::new().card(Corner::Px(33)).button(Corner::Px(32)));
        assert_eq!(
            shape.errors(),
            vec![(
                "radius-card".to_owned(),
                "A corner takes 0 to 32 px, or a pill.".to_owned()
            )]
        );
        let mut errors = Vec::new();
        let value: Value =
            serde_json::json!({"radius-card": "40px", "radius-pill": "pill", "radius-nav": 4});
        let _ = Shape::from_json(&value, &mut errors);
        let mut at: Vec<&str> = errors.iter().map(|(at, _)| at.as_str()).collect();
        at.sort_unstable();
        assert_eq!(
            at,
            ["shape.radius-card", "shape.radius-nav", "shape.radius-pill"]
        );
    }

    #[test]
    fn the_rust_of_a_shape_calls_its_builders() {
        let shape = Shape::new().corners(Corners::new().button(Corner::Pill).card(Corner::Px(16)));
        assert_eq!(
            shape.rust(),
            "Shape::new()\n        .corners(Corners::new().button(Corner::Pill).card(Corner::Px(16)))"
        );
        assert_eq!(shape.rust_names(), ["Corner", "Corners", "Shape"]);
        assert!(Shape::new().rust_names().is_empty());
    }
}
