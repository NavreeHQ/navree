//! The colours the components draw one on another, and the ratio each needs.
//!
//! A pair is two [`Paint`]s: a token as it is, two tokens mixed in OKLab, the
//! accent toward the hover mix, or a token laid thin over a surface (a tinted
//! badge, a destructive button). The list is what the stylesheet draws: a test
//! (`tests/drawn_pairs.rs`) reads every rule of the toolkit's stylesheet,
//! works out what colour sits on what, and fails when a pair it finds isn't
//! here. Every built-in theme passes every pair, and a theme of your own is
//! refused while a text pair fails.

use std::sync::LazyLock;

use super::{Palette, Rgb, Side, Token};

/// A colour as the stylesheet draws it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Paint {
    /// A token as it is: `var(--fg)`.
    Token(Token),
    /// Two tokens mixed in OKLab: `color-mix(in oklab, var(--a) p%, var(--b))`.
    Mix(Token, u8, Token),
    /// A token toward the hover mix, away from the text on the accent (see
    /// [`Palette::hover_mix`]): `color-mix(in oklab, var(--a) p%, var(--hover-mix))`.
    Hover(Token, u8),
    /// A token laid thin over a surface:
    /// `color-mix(in oklab, var(--a) p%, transparent)` on `var(--s)`.
    Tint(Token, u8, Token),
}

impl Paint {
    /// The colour in a palette.
    pub fn resolve(self, palette: &Palette, _side: Side) -> Rgb {
        let share = |percent: u8| 1.0 - f64::from(percent) / 100.0;
        match self {
            Self::Token(token) => palette.get(token),
            Self::Mix(a, percent, b) => palette.get(a).mix(palette.get(b), share(percent)),
            Self::Hover(a, percent) => palette.get(a).mix(palette.hover_mix(), share(percent)),
            Self::Tint(a, percent, under) => palette
                .get(a)
                .over(palette.get(under), f64::from(percent) / 100.0),
        }
    }

    /// The paint as CSS, for a page to draw it: a tint is the same colour as
    /// `color-mix(in srgb, ...)` over its surface.
    pub fn css(self) -> String {
        match self {
            Self::Token(token) => format!("var(--{})", token.name()),
            Self::Mix(a, percent, b) => format!(
                "color-mix(in oklab,var(--{}) {percent}%,var(--{}))",
                a.name(),
                b.name()
            ),
            Self::Hover(a, percent) => format!(
                "color-mix(in oklab,var(--{}) {percent}%,var(--hover-mix))",
                a.name()
            ),
            Self::Tint(a, percent, under) => format!(
                "color-mix(in srgb,var(--{}) {percent}%,var(--{}))",
                a.name(),
                under.name()
            ),
        }
    }

    /// The paint in words: `danger 24% over ink-1`.
    pub fn describe(self) -> String {
        match self {
            Self::Token(token) => token.name().to_owned(),
            Self::Mix(a, percent, b) => format!("{} {percent}% into {}", a.name(), b.name()),
            Self::Hover(a, percent) => format!("{} {percent}% on hover", a.name()),
            Self::Tint(a, percent, under) => {
                format!("{} {percent}% over {}", a.name(), under.name())
            }
        }
    }

    /// The token alone, when the paint is one.
    pub fn token(self) -> Option<Token> {
        match self {
            Self::Token(token) => Some(token),
            _ => None,
        }
    }
}

/// What a contrast pair is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PairKind {
    /// Text, which must reach WCAG AA (4.5:1): a theme that fails one is refused.
    Text,
    /// A focus ring, an icon or a chart colour, which should reach 3:1: a warning.
    Graphic,
}

/// A colour drawn on another, and the ratio it needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pair {
    /// The colour drawn.
    pub text: Paint,
    /// The colour under it.
    pub on: Paint,
    /// The ratio it needs.
    pub min: f64,
    /// Text or a graphic.
    pub kind: PairKind,
}

impl Pair {
    /// The ratio in a palette.
    pub fn ratio(&self, palette: &Palette, side: Side) -> f64 {
        self.text
            .resolve(palette, side)
            .contrast(self.on.resolve(palette, side))
    }
}

/// Whether `paint` is drawn with `token`, alone or in a mix or a tint.
pub(crate) fn draws(paint: Paint, token: Token) -> bool {
    match paint {
        Paint::Token(t) | Paint::Hover(t, _) => t == token,
        Paint::Mix(a, _, b) | Paint::Tint(a, _, b) => a == token || b == token,
    }
}

/// The page, cards, hover and the selected row: what text sits on.
pub const SURFACES: [Token; 4] = [Token::Ink0, Token::Ink1, Token::Ink2, Token::Ink3];

/// The status and label tones.
pub const TONES: [Token; 7] = [
    Token::Success,
    Token::Amber,
    Token::Danger,
    Token::Info,
    Token::Violet,
    Token::Cyan,
    Token::Rose,
];

/// Every pair the components draw, checked in both palettes of every theme.
pub fn pairs() -> &'static [Pair] {
    static PAIRS: LazyLock<Vec<Pair>> = LazyLock::new(build);
    &PAIRS
}

fn build() -> Vec<Pair> {
    use Paint::{Hover, Mix, Tint, Token as T};
    let mut out: Vec<Pair> = Vec::new();
    let mut add = |text: Paint, on: Paint, kind: PairKind| {
        let min = match kind {
            PairKind::Text => 4.5,
            PairKind::Graphic => 3.0,
        };
        let pair = Pair {
            text,
            on,
            min,
            kind,
        };
        if !out.iter().any(|p| p.text == text && p.on == on) {
            out.push(pair);
        }
    };
    let text = PairKind::Text;
    let graphic = PairKind::Graphic;
    // Text and the accent as text on every surface.
    for surface in SURFACES {
        for fg in [
            Token::Fg,
            Token::Fg1,
            Token::Fg2,
            Token::Fg3,
            Token::AccentText,
        ] {
            add(T(fg), T(surface), text);
        }
        // A status or label tone as text: an error under a field, a falling figure.
        for tone in TONES {
            add(T(tone), T(surface), text);
        }
    }
    // Text on the accent, at rest and on hover.
    add(T(Token::AccentInk), T(Token::Accent), text);
    add(T(Token::AccentInk), Hover(Token::Accent, 86), text);
    // A button's text on its hover background: its fill toward the text.
    add(T(Token::Fg), Mix(Token::Ink3, 85, Token::Fg), text);
    // The destructive button at rest (10%) and on hover (18%), a destructive menu
    // item in focus and the danger badge (14%): danger on its own tint, over any surface.
    for surface in SURFACES {
        for percent in [10, 14, 18] {
            add(
                T(Token::Danger),
                Tint(Token::Danger, percent, surface),
                text,
            );
        }
        // The success and warning badges.
        add(
            Mix(Token::Success, 80, Token::Fg),
            Tint(Token::Success, 12, surface),
            text,
        );
        add(T(Token::Amber), Tint(Token::Amber, 12, surface), text);
        // A tone badge: the tone toward the text colour, on a thin tint of itself.
        for tone in TONES.into_iter().chain([Token::AccentText, Token::Fg3]) {
            add(Mix(tone, 78, Token::Fg), Tint(tone, 13, surface), text);
            add(Mix(tone, 78, Token::Fg), T(surface), text);
        }
        // A search match and a date range, marked in the accent: the text keeps its colour.
        for fg in [Token::Fg, Token::Fg1] {
            add(T(fg), Tint(Token::Accent, 16, surface), text);
            add(T(fg), Tint(Token::Accent, 30, surface), text);
        }
        // A sortable item while it is dragged: its text on a thin tint of the accent.
        add(T(Token::Fg1), Tint(Token::Accent, 6, surface), text);
    }
    // An internal note in a conversation: the text on the panel with 9% amber mixed in.
    add(T(Token::Fg), Mix(Token::Amber, 9, Token::Ink1), text);
    // A tone's icon in its tinted circle (activity, timeline, fact cards).
    for tone in TONES.into_iter().chain([Token::AccentText, Token::Fg3]) {
        for percent in [14, 15, 16] {
            add(
                Mix(tone, 78, Token::Fg),
                Mix(tone, percent, Token::Ink2),
                graphic,
            );
        }
    }
    // The focus ring, a mark in the accent (a chosen option's tick), and chart
    // series: on the page and on cards, and a series' key in the tooltip, which
    // is raised.
    for surface in SURFACES {
        add(T(Token::Ring), T(surface), graphic);
        add(T(Token::Accent), T(surface), graphic);
        for chart in Token::CHARTS {
            add(T(chart), T(surface), graphic);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn a_tint_is_the_colour_laid_over_its_surface() {
        use Paint::Tint;
        let palette = Theme::NAVREE.spec().light;
        let tint = Tint(Token::Danger, 24, Token::Ink0).resolve(&palette, Side::Light);
        let (danger, page) = (palette.get(Token::Danger), palette.get(Token::Ink0));
        let expect = |a: u8, b: u8| {
            let value = f64::from(a) * 0.24 + f64::from(b) * 0.76;
            value.round() as u8
        };
        assert_eq!(tint.r, expect(danger.r, page.r));
        assert_eq!(tint.g, expect(danger.g, page.g));
        assert_eq!(
            Tint(Token::Danger, 24, Token::Ink0).css(),
            "color-mix(in srgb,var(--danger) 24%,var(--ink-0))"
        );
    }

    #[test]
    fn the_hover_mix_runs_away_from_the_text_on_the_accent() {
        let spec = Theme::NAVREE.spec();
        // White text in light: the accent darkens. Dark text in dark: it lightens.
        let light = Paint::Hover(Token::Accent, 86).resolve(&spec.light, Side::Light);
        let dark = Paint::Hover(Token::Accent, 86).resolve(&spec.dark, Side::Dark);
        assert!(light.luminance() < spec.light.get(Token::Accent).luminance());
        assert!(dark.luminance() > spec.dark.get(Token::Accent).luminance());
        // A light accent with dark text in a light palette lightens too, so its text keeps reading.
        let mut palette = spec.light;
        palette.derive_accent(crate::theme::Rgb::hex(0x7c5cff));
        let fill = palette.get(Token::Accent);
        let hovered = Paint::Hover(Token::Accent, 86).resolve(&palette, Side::Light);
        let ink = palette.get(Token::AccentInk);
        assert!(
            ink.contrast(hovered) >= ink.contrast(fill),
            "{fill} {hovered} {ink}"
        );
    }

    #[test]
    fn every_pair_is_listed_once() {
        let all = pairs();
        for (at, pair) in all.iter().enumerate() {
            assert!(
                !all[..at]
                    .iter()
                    .any(|p| p.text == pair.text && p.on == pair.on),
                "{:?}",
                pair
            );
        }
    }
}
