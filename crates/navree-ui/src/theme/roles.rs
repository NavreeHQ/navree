//! Colour roles beyond the 31 tokens: [`Roles`].
//!
//! A palette can't gain a token in 1.x: apps build it from an array of 31 colours, and the
//! Rust export writes it so. One token paints several things (`ink-1` paints cards, inputs,
//! the sidebar and a table's header), so a theme couldn't make its cards white and its
//! sidebar off-white. A role is a colour for one of those things, in both palettes, that a
//! theme of your own carries beside its spec ([`Theme::with_roles`](super::Theme::with_roles)).
//! The stylesheet reads each with the token it stands in for behind it
//! (`var(--card,var(--ink-1))`), and a theme's stylesheet writes it only when it is set, so
//! a theme without roles draws what it did before.
//!
//! A role is checked as its token is: every text pair the components draw with the token is
//! measured again with the role in its place, with each other role that could sit beside it,
//! in both palettes. A theme whose role fails one is refused.
//!
//! The inverse surface ([`Inverse`]) is a part of a page drawn in the theme's other palette: a
//! dark card on a light page, a light one on a dark page. The stylesheet flips the colour
//! scheme of an element marked `data-inverse`, so every token in it reads its other value
//! and any part inside reads, with no work of its own; a theme's `Inverse` then sets the
//! surface, the lines and the text there to colours of its own, checked against every pair
//! like a palette.

use std::fmt::Write as _;

use serde_json::{Map, Value};

use super::pairs::{self, PairKind};
use super::{Palette, Rgb, Side, ThemeSpec, Token};

/// A colour in each palette: the one a page in light mode shows, then dark.
type Pair = (Rgb, Rgb);

/// How many roles there are, the inverse surface's five parts last.
const ROLES: usize = 12;

/// Where the inverse surface's parts start in [`TABLE`].
const INVERSE: usize = 7;

/// One role.
struct Role {
    /// The custom property, without `--`: also its name in a stored theme and in errors.
    name: &'static str,
    /// The token the stylesheet reads behind it, which it is checked as. The inverse
    /// surface's parts set several tokens in their scope ([`inverse_tokens`]).
    stands_for: Token,
    /// Its letter in the short form.
    key: char,
    /// The builder method that sets it, for the Rust export.
    method: &'static str,
}

const fn role(name: &'static str, stands_for: Token, key: char, method: &'static str) -> Role {
    Role {
        name,
        stands_for,
        key,
        method,
    }
}

/// Every role, in the order a sheet writes them.
const TABLE: [Role; ROLES] = [
    role("card", Token::Ink1, 'a', "card"),
    role("field", Token::Ink1, 'b', "field"),
    role("ghost-fg", Token::Fg2, 'c', "ghost_text"),
    role("shell-muted", Token::Fg3, 'd', "shell_muted"),
    role("segment-track", Token::Ink2, 'e', "segment_track"),
    role("segment-on", Token::Ink3, 'f', "segment_on"),
    role("row-selected", Token::Ink3, 'g', "selected_row"),
    role("inverse", Token::Ink1, 'h', "surface"),
    role("inverse-raised", Token::Ink2, 'i', "raised"),
    role("inverse-line", Token::Line, 'j', "line"),
    role("inverse-fg", Token::Fg, 'k', "text"),
    role("inverse-fg-2", Token::Fg2, 'l', "muted"),
];

/// The tokens a role sets inside the inverse surface: the inverse parts' own, then the
/// plain roles the surface takes over. Index by [`TABLE`].
fn inverse_tokens(at: usize) -> (&'static [Token], &'static [usize]) {
    match at {
        // The surface: the page and cards, the text on the accent, cards and fields.
        7 => (&[Token::Ink0, Token::Ink1, Token::AccentInk], &[0, 1]),
        // Raised: hover and selected, the segment's track and chosen tab, a selected row.
        8 => (&[Token::Ink2, Token::Ink3], &[4, 5, 6]),
        9 => (&[Token::Line, Token::Line2], &[]),
        // The text, and the accent: on a dark card the primary button is the text's colour.
        10 => (
            &[
                Token::Fg,
                Token::Fg1,
                Token::Accent,
                Token::AccentText,
                Token::Ring,
            ],
            &[2],
        ),
        11 => (&[Token::Fg2, Token::Fg3], &[3]),
        _ => (&[], &[]),
    }
}

/// The colours of the inverse surface: a part of a page drawn on the theme's other palette,
/// such as a dark card on a light page. Each part you don't set stays the other palette's.
/// Each colour is given for a page in light mode, then in dark mode: `surface(#0a0a0a,
/// #eaeae7)` is a near-black card on a light page and a near-white one on a dark page.
///
/// ```
/// use navree_ui::theme::{Inverse, Rgb};
///
/// let inverse = Inverse::new()
///     .surface(Rgb::hex(0x0a0a0a), Rgb::hex(0xeaeae7))
///     .text(Rgb::hex(0xffffff), Rgb::hex(0x0a0a0a))
///     .muted(Rgb::hex(0xb5b5b0), Rgb::hex(0x3d3d3a));
/// assert!(!inverse.is_empty());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[must_use]
pub struct Inverse([Option<Pair>; ROLES - INVERSE]);

impl Inverse {
    /// Every part the other palette's.
    pub const fn new() -> Self {
        Self([None; ROLES - INVERSE])
    }

    const fn with(mut self, at: usize, light: Rgb, dark: Rgb) -> Self {
        self.0[at] = Some((light, dark));
        self
    }

    /// The surface: its page and cards, its fields, and the text on its accent.
    pub const fn surface(self, light: Rgb, dark: Rgb) -> Self {
        self.with(0, light, dark)
    }

    /// A panel raised on it, hover and a selected item: an inner panel, a chip.
    pub const fn raised(self, light: Rgb, dark: Rgb) -> Self {
        self.with(1, light, dark)
    }

    /// Its dividers and borders.
    pub const fn line(self, light: Rgb, dark: Rgb) -> Self {
        self.with(2, light, dark)
    }

    /// Its text and headings, and its accent: a primary button on it takes this colour, with
    /// the surface's as its text.
    pub const fn text(self, light: Rgb, dark: Rgb) -> Self {
        self.with(3, light, dark)
    }

    /// Its secondary text and hints.
    pub const fn muted(self, light: Rgb, dark: Rgb) -> Self {
        self.with(4, light, dark)
    }

    /// Whether every part is the other palette's.
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }
}

/// Colours for things one token paints today, each in both palettes and each falling back
/// to that token: white cards beside an off-white sidebar, a ghost button in the text's own
/// colour, and the inverse surface. A theme takes them with
/// [`Theme::with_roles`](super::Theme::with_roles), which refuses any whose text falls short
/// of WCAG AA, in light or in dark. Each colour is given for a page in light mode, then in
/// dark mode.
///
/// ```
/// use navree_ui::theme::{Inverse, Rgb, Roles, Theme, ThemeId};
///
/// let roles = Roles::new()
///     .card(Rgb::hex(0xffffff), Rgb::hex(0x0f1411))
///     .field(Rgb::hex(0xffffff), Rgb::hex(0x0f1411))
///     .inverse(
///         Inverse::new()
///             .surface(Rgb::hex(0x0a0a0a), Rgb::hex(0xeaeae7))
///             .text(Rgb::hex(0xffffff), Rgb::hex(0x0a0a0a)),
///     );
/// let theme = Theme::NAVREE
///     .renamed(ThemeId::parse("agra").unwrap(), "Agra")
///     .with_roles(roles)
///     .unwrap();
/// assert!(theme.css().unwrap().contains("--card:light-dark(#ffffff,#0f1411)"));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[must_use]
pub struct Roles([Option<Pair>; ROLES]);

impl Roles {
    /// No role set: every thing in its token's colour.
    pub const fn new() -> Self {
        Self([None; ROLES])
    }

    const fn with(mut self, at: usize, light: Rgb, dark: Rgb) -> Self {
        self.0[at] = Some((light, dark));
        self
    }

    /// Cards, stats, fact cards, tables, dialogs, sheets, popovers and menus: the surfaces
    /// above the page. Without it, `ink-1`, which the sidebar and a table's header keep.
    pub const fn card(self, light: Rgb, dark: Rgb) -> Self {
        self.with(0, light, dark)
    }

    /// Inputs, text areas, selects, input groups, checkboxes and radios, the select and
    /// combobox controls and the phone input. Without it, `ink-1`.
    pub const fn field(self, light: Rgb, dark: Rgb) -> Self {
        self.with(1, light, dark)
    }

    /// The text of a ghost button, such as the icon buttons of a top bar. Without it, `fg-2`.
    pub const fn ghost_text(self, light: Rgb, dark: Rgb) -> Self {
        self.with(2, light, dark)
    }

    /// The sidebar's quieter text: the brand's context line, the user's email, the groups'
    /// labels and the counts. Without it, `fg-3`.
    pub const fn shell_muted(self, light: Rgb, dark: Rgb) -> Self {
        self.with(3, light, dark)
    }

    /// The track of segmented tabs and of a segmented button group. Without it, `ink-2`.
    pub const fn segment_track(self, light: Rgb, dark: Rgb) -> Self {
        self.with(4, light, dark)
    }

    /// The chosen tab, or the current link, on a segmented track. Without it, `ink-3`.
    pub const fn segment_on(self, light: Rgb, dark: Rgb) -> Self {
        self.with(5, light, dark)
    }

    /// A selected row of a table. Without it, `ink-3`.
    pub const fn selected_row(self, light: Rgb, dark: Rgb) -> Self {
        self.with(6, light, dark)
    }

    /// The inverse surface's colours, in place of any set before.
    pub const fn inverse(mut self, inverse: Inverse) -> Self {
        let mut at = 0;
        while at < ROLES - INVERSE {
            self.0[INVERSE + at] = inverse.0[at];
            at += 1;
        }
        self
    }

    /// Whether no role is set.
    pub fn is_default(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }

    /// Every role set, with its colours, in [`TABLE`]'s order.
    fn values(&self) -> impl Iterator<Item = (usize, &'static Role, Pair)> + '_ {
        TABLE
            .iter()
            .enumerate()
            .filter_map(|(at, role)| self.0[at].map(|pair| (at, role, pair)))
    }

    /// The plain roles' colours in one palette.
    fn plain(&self, side: Side) -> [Option<Rgb>; INVERSE] {
        let mut out = [None; INVERSE];
        for (at, colour) in out.iter_mut().enumerate() {
            *colour = self.0[at].map(|pair| pick(pair, side));
        }
        out
    }

    /// Whether any part of the inverse surface is set.
    fn has_inverse(&self) -> bool {
        self.0[INVERSE..].iter().any(Option::is_some)
    }

    /// The palette and the plain roles inside the inverse surface, on a page in `side`'s
    /// mode: the other palette and its roles, with the inverse parts in place. A role the
    /// surface sets to its own token's colour there is left out: the token stands for it.
    fn inside(&self, spec: &ThemeSpec, side: Side) -> (Palette, [Option<Rgb>; INVERSE]) {
        let other = other(side);
        let mut palette = *spec.palette(other);
        let mut plain = self.plain(other);
        for at in INVERSE..ROLES {
            let Some(pair) = self.0[at] else { continue };
            let colour = pick(pair, side);
            let (tokens, roles) = inverse_tokens(at);
            for token in tokens {
                palette.set(*token, colour);
            }
            for role in roles {
                plain[*role] = Some(colour);
            }
        }
        for (at, colour) in plain.iter_mut().enumerate() {
            if *colour == Some(palette.get(TABLE[at].stands_for)) {
                *colour = None;
            }
        }
        (palette, plain)
    }

    /// The inverse part that sets `token` inside the surface, if one does.
    fn part_of(&self, token: Token) -> Option<&'static str> {
        (INVERSE..ROLES)
            .find(|at| self.0[*at].is_some() && inverse_tokens(*at).0.contains(&token))
            .map(|at| TABLE[at].name)
    }

    /// The text pairs that fall short, each by the role it draws, with what and why: empty
    /// when every role reads in both palettes.
    pub(crate) fn errors(&self, spec: &ThemeSpec) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        if self.is_default() {
            return out;
        }
        for side in Side::BOTH {
            check(spec.palette(side), &self.plain(side), side, None, &mut out);
            if self.has_inverse() {
                let (palette, plain) = self.inside(spec, side);
                check(&palette, &plain, side, Some(self), &mut out);
            }
        }
        out
    }

    /// `;--name:colour` for each plain role set, `light-dark()` when the palettes differ.
    pub(crate) fn css(&self, out: &mut String) {
        for (_, role, pair) in self.values().filter(|(at, _, _)| *at < INVERSE) {
            let _ = write!(out, ";--{}:", role.name);
            colour(out, pair);
        }
    }

    /// The custom properties it sets on the theme's block, which a part of the page drawn
    /// in another theme must not inherit.
    pub(crate) fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.values()
            .filter(|(at, _, _)| *at < INVERSE)
            .map(|(_, role, _)| role.name)
    }

    /// The inverse surface's block for the theme `key`, or nothing when it sets no part.
    /// The surface's scheme is flipped, so `light-dark()` there picks its second colour on a
    /// light page: each colour is written in the other order.
    pub(crate) fn inverse_css(&self, key: &str, spec: &ThemeSpec, out: &mut String) {
        if !self.has_inverse() {
            return;
        }
        let _ = write!(out, ":where([data-theme={key}] [data-inverse]){{");
        let mut first = true;
        let mut put = |out: &mut String, name: &str, pair: Pair| {
            let _ = write!(out, "{}--{name}:", if first { "" } else { ";" });
            first = false;
            colour(out, (pair.1, pair.0));
        };
        for at in INVERSE..ROLES {
            let Some(pair) = self.0[at] else { continue };
            let (tokens, roles) = inverse_tokens(at);
            for token in tokens {
                put(out, token.name(), pair);
            }
            for role in roles {
                put(out, TABLE[*role].name, pair);
            }
        }
        // What the accent moves toward on hover follows the text on it.
        if self.0[INVERSE].is_some() || self.0[INVERSE + 3].is_some() {
            let light = self.inside(spec, Side::Light).0.hover_mix();
            let dark = self.inside(spec, Side::Dark).0.hover_mix();
            put(out, "hover-mix", (light, dark));
        }
        out.push_str("}\n");
    }

    /// Its part of the short form: `-r`, then each role set as its letter and its two
    /// colours' digits, light first. Nothing when no role is set.
    pub(crate) fn form(&self, out: &mut String) {
        if self.is_default() {
            return;
        }
        out.push_str("-r");
        for (_, role, (light, dark)) in self.values() {
            out.push(role.key);
            light.write_digits(out);
            dark.write_digits(out);
        }
    }

    /// Reads the short form's roles (the text after `-r`). `None` for anything but roles in
    /// order, each with twelve lower-case hex digits.
    pub(crate) fn read_form(text: &str) -> Option<Self> {
        let mut roles = Self::new();
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
            let at = TABLE.iter().position(|role| role.key == key)?;
            let digits = rest.get(key.len_utf8()..key.len_utf8() + 12)?;
            if !digits
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return None;
            }
            let read = |from: usize| u32::from_str_radix(&digits[from..from + 6], 16).ok();
            roles.0[at] = Some((Rgb::hex(read(0)?), Rgb::hex(read(6)?)));
            rest = &rest[key.len_utf8() + 12..];
        }
        Some(roles)
    }

    /// Its stored form: each role by its name, with its light and dark colours.
    pub(crate) fn json(&self) -> Value {
        let mut map = Map::new();
        for (_, role, (light, dark)) in self.values() {
            let mut pair = Map::new();
            pair.insert("light".to_owned(), Value::String(light.to_hex()));
            pair.insert("dark".to_owned(), Value::String(dark.to_hex()));
            map.insert(role.name.to_owned(), Value::Object(pair));
        }
        Value::Object(map)
    }

    /// Reads stored roles, naming each field that is wrong under `roles.`.
    pub(crate) fn from_json(value: &Value, errors: &mut Vec<(String, String)>) -> Self {
        let mut roles = Self::new();
        let Value::Object(map) = value else {
            errors.push((
                "roles".to_owned(),
                "Give the roles by name, each with a light and a dark colour.".to_owned(),
            ));
            return roles;
        };
        for (name, value) in map {
            let at = format!("roles.{}", super::codec::clip(name));
            let Some(index) = TABLE.iter().position(|role| role.name == name) else {
                errors.push((at, "This is not a colour role.".to_owned()));
                continue;
            };
            let Value::Object(pair) = value else {
                errors.push((at, "Give a light and a dark colour.".to_owned()));
                continue;
            };
            if let Some(key) = pair
                .keys()
                .find(|key| !matches!(key.as_str(), "light" | "dark"))
            {
                errors.push((
                    format!("{at}.{}", super::codec::clip(key)),
                    "This field is not part of a role.".to_owned(),
                ));
                continue;
            }
            let read = |side: &str| match pair.get(side).and_then(Value::as_str) {
                Some(text) => Rgb::parse(text).map_err(|why| why.to_string()),
                None => Err("Missing.".to_owned()),
            };
            match (read("light"), read("dark")) {
                (Ok(light), Ok(dark)) => roles.0[index] = Some((light, dark)),
                (light, dark) => {
                    if let Err(why) = light {
                        errors.push((format!("{at}.light"), why));
                    }
                    if let Err(why) = dark {
                        errors.push((format!("{at}.dark"), why));
                    }
                }
            }
        }
        roles
    }

    /// The roles as Rust: `Roles::new()` and a call for each role set.
    pub(crate) fn rust(&self) -> String {
        let mut out = String::from("Roles::new()");
        let call = |out: &mut String, method: &str, (light, dark): Pair| {
            let _ = write!(
                out,
                ".{method}(rgb(0x{:06x}), rgb(0x{:06x}))",
                light.to_u32(),
                dark.to_u32()
            );
        };
        for (_, role, pair) in self.values().filter(|(at, _, _)| *at < INVERSE) {
            out.push_str("\n        ");
            call(&mut out, role.method, pair);
        }
        if self.has_inverse() {
            out.push_str("\n        .inverse(Inverse::new()");
            for (_, role, pair) in self.values().filter(|(at, _, _)| *at >= INVERSE) {
                out.push_str("\n            ");
                call(&mut out, role.method, pair);
            }
            out.push(')');
        }
        out
    }

    /// The names a Rust export of them uses from the theme module.
    pub(crate) fn rust_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.has_inverse() {
            names.push("Inverse");
        }
        if !self.is_default() {
            names.push("Roles");
        }
        names
    }
}

/// The colour of `pair` a page in `side`'s mode shows.
const fn pick(pair: Pair, side: Side) -> Rgb {
    match side {
        Side::Light => pair.0,
        Side::Dark => pair.1,
    }
}

/// The other palette.
const fn other(side: Side) -> Side {
    match side {
        Side::Light => Side::Dark,
        Side::Dark => Side::Light,
    }
}

/// `light-dark(light,dark)`, or the one colour when both agree.
fn colour(out: &mut String, (light, dark): Pair) {
    if light == dark {
        dark.write_hex(out);
    } else {
        out.push_str("light-dark(");
        light.write_hex(out);
        out.push(',');
        dark.write_hex(out);
        out.push(')');
    }
}

/// The tokens a plain role stands in for, each once.
const STANDS_FOR: [Token; 5] = [
    Token::Ink1,
    Token::Ink2,
    Token::Ink3,
    Token::Fg2,
    Token::Fg3,
];

/// Measures every text pair that draws a token a role stands in for, with each choice of the
/// roles that could be in its place (the token itself among them), and records each that
/// falls short. Inside the inverse surface (`inverse` is the roles it comes from), every
/// text pair is measured, since its palette is new.
fn check(
    palette: &Palette,
    plain: &[Option<Rgb>; INVERSE],
    side: Side,
    inverse: Option<&Roles>,
    out: &mut Vec<(String, String)>,
) {
    for pair in pairs::pairs()
        .iter()
        .filter(|pair| pair.kind == PairKind::Text)
    {
        let drawn: Vec<Token> = STANDS_FOR
            .into_iter()
            .filter(|token| pairs::draws(pair.text, *token) || pairs::draws(pair.on, *token))
            .collect();
        // For each token drawn, what may stand in its place: the token, then each role set.
        let options: Vec<Vec<Option<usize>>> = drawn
            .iter()
            .map(|token| {
                std::iter::once(None)
                    .chain(
                        (0..INVERSE)
                            .filter(|at| TABLE[*at].stands_for == *token && plain[*at].is_some())
                            .map(Some),
                    )
                    .collect()
            })
            .collect();
        let combos: usize = options.iter().map(Vec::len).product();
        for combo in 0..combos {
            let mut chosen = Vec::with_capacity(drawn.len());
            let mut rest = combo;
            for choices in &options {
                chosen.push(choices[rest % choices.len()]);
                rest /= choices.len();
            }
            if inverse.is_none() && chosen.iter().all(Option::is_none) {
                continue;
            }
            let mut measured = *palette;
            let mut names: Vec<(&str, &str)> = Vec::new();
            for (token, role) in drawn.iter().zip(&chosen) {
                if let Some(at) = role {
                    measured.set(*token, plain[*at].unwrap_or(Rgb::BLACK));
                    names.push((token.name(), TABLE[*at].name));
                }
            }
            let ratio = pair.ratio(&measured, side);
            if ratio >= pair.min {
                continue;
            }
            let (text, on) = (
                named(pair.text, &names, inverse),
                named(pair.on, &names, inverse),
            );
            // Inside the inverse surface, a pair is the part's that draws it, else the
            // surface's; elsewhere, the first role in it.
            let part = inverse.and_then(|roles| {
                [pair.text, pair.on].into_iter().find_map(|paint| {
                    Token::ALL
                        .into_iter()
                        .filter(|token| pairs::draws(paint, *token))
                        .find_map(|token| roles.part_of(token))
                })
            });
            let at = match inverse {
                Some(_) => part.unwrap_or("inverse"),
                None => names.first().map_or("inverse", |(_, role)| *role),
            };
            let what = format!(
                "{text} on {on} is {ratio:.2}:1 in {}{}; text needs {}:1.",
                side.as_str(),
                if inverse.is_some() {
                    ", on the inverse surface"
                } else {
                    ""
                },
                pair.min
            );
            if !out.iter().any(|(a, w)| a == at && *w == what) {
                out.push((at.to_owned(), what));
            }
        }
    }
}

/// A paint in words, each token a role stands in for called by the role. Inside the inverse
/// surface, each token one of its parts sets is called by the part.
fn named(paint: pairs::Paint, names: &[(&str, &str)], inverse: Option<&Roles>) -> String {
    paint
        .describe()
        .split(' ')
        .map(|word| {
            if let Some((_, role)) = names.iter().find(|(token, _)| *token == word) {
                return (*role).to_owned();
            }
            if let Some(part) = inverse
                .and_then(|roles| Token::from_name(word).and_then(|token| roles.part_of(token)))
            {
                return part.to_owned();
            }
            word.to_owned()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    fn design() -> Roles {
        Roles::new()
            .card(Rgb::hex(0xffffff), Rgb::hex(0x0f1411))
            .field(Rgb::hex(0xffffff), Rgb::hex(0x0f1411))
            .ghost_text(Rgb::hex(0x0a0a0a), Rgb::hex(0xe9efeb))
            .shell_muted(Rgb::hex(0x5e5e5a), Rgb::hex(0xa9b5ae))
            .segment_track(Rgb::hex(0xefefec), Rgb::hex(0x111714))
            .segment_on(Rgb::hex(0xffffff), Rgb::hex(0x16201b))
            .selected_row(Rgb::hex(0xf6f9fe), Rgb::hex(0x16201b))
            .inverse(
                Inverse::new()
                    .surface(Rgb::hex(0x0a0a0a), Rgb::hex(0xffffff))
                    .raised(Rgb::hex(0x161615), Rgb::hex(0xf2f2f0))
                    .line(Rgb::hex(0x2a2a28), Rgb::hex(0xc4c4bf))
                    .text(Rgb::hex(0xffffff), Rgb::hex(0x0a0a0a))
                    .muted(Rgb::hex(0xb5b5b0), Rgb::hex(0x3d3d3a)),
            )
    }

    #[test]
    fn every_role_has_a_unique_name_and_key() {
        for (at, role) in TABLE.iter().enumerate() {
            assert!(
                !TABLE[..at]
                    .iter()
                    .any(|other| other.name == role.name || other.key == role.key)
            );
        }
        assert_eq!(TABLE[INVERSE].name, "inverse");
    }

    #[test]
    fn the_roles_of_a_light_design_read_in_both_palettes() {
        let errors = design().errors(Theme::NAVREE.spec());
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn roles_write_their_colours_and_read_them_back() {
        let roles = design();
        let mut css = String::new();
        roles.css(&mut css);
        assert!(
            css.starts_with(";--card:light-dark(#ffffff,#0f1411);"),
            "{css}"
        );
        assert!(
            !css.contains("inverse"),
            "the inverse parts are written in their scope"
        );
        let mut scope = String::new();
        roles.inverse_css("cabc", Theme::NAVREE.spec(), &mut scope);
        assert!(
            scope.starts_with(
                ":where([data-theme=cabc] [data-inverse]){--ink-0:light-dark(#ffffff,#0a0a0a);"
            ),
            "the surface's scheme is flipped, so its colours swap: {scope}"
        );
        assert!(
            scope.contains(";--card:light-dark(#ffffff,#0a0a0a)"),
            "{scope}"
        );
        assert!(
            scope.contains(";--accent:light-dark(#0a0a0a,#ffffff)"),
            "{scope}"
        );
        let mut form = String::new();
        roles.form(&mut form);
        assert!(form.starts_with("-raffffff0f1411b"), "{form}");
        assert_eq!(Roles::read_form(&form[2..]), Some(roles));
        let mut errors = Vec::new();
        assert_eq!(Roles::from_json(&roles.json(), &mut errors), roles);
        assert!(errors.is_empty(), "{errors:?}");
        let mut nothing = String::new();
        Roles::new().form(&mut nothing);
        assert_eq!(nothing, "");
    }

    #[test]
    fn a_short_forms_roles_have_one_spelling() {
        for bad in [
            "",
            "a",
            "affffff0f141",
            "affffff0f1411x",
            "aFFFFFF0f1411",
            "bffffff0f1411affffff0f1411",
            "affffff0f1411affffff0f1411",
            "zffffff0f1411",
            "affffff0f141g",
        ] {
            assert_eq!(Roles::read_form(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_role_that_fails_contrast_is_named_in_light_and_in_dark() {
        // A pale card in light: secondary text and hints no longer read on it.
        let pale = Roles::new().card(Rgb::hex(0x9a9a9a), Rgb::hex(0x0f1411));
        let errors = pale.errors(Theme::NAVREE.spec());
        assert!(!errors.is_empty());
        assert!(errors.iter().all(|(at, _)| at == "card"), "{errors:?}");
        assert!(
            errors.iter().any(
                |(_, what)| what.starts_with("fg-3 on card is ") && what.contains(" in light;")
            ),
            "{errors:?}"
        );
        // A ghost text too dark to read in dark mode.
        let dim = Roles::new().ghost_text(Rgb::hex(0x0a0a0a), Rgb::hex(0x333333));
        let errors = dim.errors(Theme::NAVREE.spec());
        assert!(
            errors
                .iter()
                .any(|(at, what)| at == "ghost-fg" && what.contains(" in dark;")),
            "{errors:?}"
        );
        // A ghost button on a card: both roles at once.
        let both = Roles::new()
            .card(Rgb::hex(0xd0d0d0), Rgb::hex(0x0f1411))
            .ghost_text(Rgb::hex(0x6a6a6a), Rgb::hex(0xe9efeb));
        let errors = both.errors(Theme::NAVREE.spec());
        assert!(
            errors
                .iter()
                .any(|(_, what)| what.starts_with("ghost-fg on card ")),
            "{errors:?}"
        );
        // Muted text on the inverse surface that doesn't read.
        let murky = Roles::new().inverse(
            Inverse::new()
                .surface(Rgb::hex(0x0a0a0a), Rgb::hex(0xeaeae7))
                .muted(Rgb::hex(0x333333), Rgb::hex(0x3d3d3a)),
        );
        let errors = murky.errors(Theme::NAVREE.spec());
        assert!(
            errors.iter().any(|(at, what)| at == "inverse-fg-2"
                && what.starts_with("inverse-fg-2 on inverse is ")
                && what.contains(" in light, on the inverse surface;")),
            "{errors:?}"
        );
        assert!(
            errors.iter().all(|(at, _)| at.starts_with("inverse")),
            "named by the surface's parts: {errors:?}"
        );
    }

    #[test]
    fn the_stylesheet_reads_each_role_with_the_token_it_stands_for() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles");
        let mut css = String::new();
        for dir in [root.clone(), root.join("extra")] {
            for entry in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
                if entry.path().extension().is_some_and(|ext| ext == "css") {
                    css.push_str(&std::fs::read_to_string(entry.path()).unwrap());
                }
            }
        }
        for role in &TABLE[..INVERSE] {
            let read = format!("var(--{},", role.name);
            let reads: Vec<&str> = css
                .match_indices(&read)
                .map(|(at, _)| &css[at + read.len()..])
                .collect();
            assert!(!reads.is_empty(), "{} is read by no rule", role.name);
            for fallback in reads {
                assert!(
                    fallback.starts_with(&format!("var(--{}))", role.stands_for.name())),
                    "{} falls back to {fallback:.20}, not {}",
                    role.name,
                    role.stands_for.name()
                );
            }
        }
        // The inverse surface flips the scheme its parts read.
        assert!(css.contains(":where([data-inverse]){color-scheme:light;color:var(--fg-1)}"));
    }
}
