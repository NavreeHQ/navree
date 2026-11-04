//! Themes: how every page looks, as typed Rust values.
//!
//! A [`Theme`] is a [`ThemeSpec`]: two [`Palette`]s (dark and light) of the
//! colour [`Token`]s, a [`Radius`], a [`Density`] and a [`Font`] from a safe
//! list. The built-in themes are written into the toolkit's stylesheet; a
//! theme of your own is served as a small stylesheet of its own, at a URL that
//! is the theme itself, so it costs no storage and is cached for good.
//!
//! An [`Appearance`] is a theme and a [`Mode`] (light, dark or the device's
//! choice). The server writes it on `<html>` and links the theme's stylesheet
//! in the head, so the first paint is already right: no script decides how a
//! page looks.
//!
//! ```
//! use navree_ui::theme::{Appearance, Mode, Theme};
//! use navree_ui::{Shell, View, el};
//!
//! let shell = Shell::new("Acme").appearance(Appearance::new(Theme::OCEAN, Mode::System));
//! let html = shell.page("Deals", "/deals").to_view().to_html();
//! assert!(html.starts_with(r#"<!doctype html><html lang="en" data-theme="ocean" data-mode="system""#));
//! ```
//!
//! Every value that comes from outside (a form, a cookie, a URL, a stored
//! theme) goes through a strict parser: colours through [`Rgb::parse`],
//! lengths through [`Radius::parse`], fonts from [`Font::ALL`]. The CSS is
//! written by this module alone, so a theme can't carry anything but colours,
//! lengths and a font from the list.

mod appearance;
mod builder;
mod codec;
mod colour;
mod css;
mod menu;
mod pairs;

use std::sync::{Arc, LazyLock};

pub use appearance::{
    Appearance, ChoiceError, Mode, SETTINGS_PATH, THEME_COOKIE, ThemeSetting, appearance_cookie,
};
pub use builder::{BuilderDraft, ThemeBuilder};
pub use codec::{ImportError, ThemeId};
pub use colour::{ColourError, Rgb};
pub use css::{builder_layer, token_layer};
pub use menu::ThemeMenu;
pub use pairs::{Paint, Pair, PairKind, SURFACES, TONES, pairs};

pub(crate) use appearance::{read_setting, respond_setting};
pub(crate) use css::respond as respond_sheet;

/// A colour a theme sets, by its CSS custom property.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Token {
    /// The page.
    Ink0,
    /// Cards, inputs, the sidebar.
    Ink1,
    /// Hover.
    Ink2,
    /// Selected, raised.
    Ink3,
    /// Dividers.
    Line,
    /// Borders of controls.
    Line2,
    /// Borders on hover.
    Line3,
    /// Headings and values.
    Fg,
    /// Body text.
    Fg1,
    /// Secondary text.
    Fg2,
    /// Hints and labels.
    Fg3,
    /// The primary action's fill.
    Accent,
    /// Text on the accent.
    AccentInk,
    /// The accent as text, on the page and on cards.
    AccentText,
    /// The focus ring.
    Ring,
    /// Good news: a won deal, a rising figure.
    Success,
    /// Warnings.
    Amber,
    /// Failures and destructive actions.
    Danger,
    /// Information.
    Info,
    /// A tone for labels.
    Violet,
    /// A tone for labels.
    Cyan,
    /// A tone for labels.
    Rose,
    /// Neutral bars of a chart.
    Bar,
    /// The first series of a chart.
    Chart1,
    /// The second series.
    Chart2,
    /// The third series.
    Chart3,
    /// The fourth series.
    Chart4,
    /// The fifth series.
    Chart5,
}

/// How many colour tokens there are.
pub const TOKEN_COUNT: usize = 28;

/// The groups the builder shows the tokens in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenGroup {
    /// The page, cards and lines.
    Surfaces,
    /// Text.
    Text,
    /// The accent and the focus ring.
    Accent,
    /// Status and label tones.
    Tones,
    /// Chart colours.
    Charts,
}

impl TokenGroup {
    /// The group's heading.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Surfaces => "Surfaces",
            Self::Text => "Text",
            Self::Accent => "Accent",
            Self::Tones => "Tones",
            Self::Charts => "Charts",
        }
    }
}

impl Token {
    /// Every token, in the order a theme writes them.
    pub const ALL: [Self; TOKEN_COUNT] = [
        Self::Ink0,
        Self::Ink1,
        Self::Ink2,
        Self::Ink3,
        Self::Line,
        Self::Line2,
        Self::Line3,
        Self::Fg,
        Self::Fg1,
        Self::Fg2,
        Self::Fg3,
        Self::Accent,
        Self::AccentInk,
        Self::AccentText,
        Self::Ring,
        Self::Success,
        Self::Amber,
        Self::Danger,
        Self::Info,
        Self::Violet,
        Self::Cyan,
        Self::Rose,
        Self::Bar,
        Self::Chart1,
        Self::Chart2,
        Self::Chart3,
        Self::Chart4,
        Self::Chart5,
    ];

    /// The tokens that set the neutral greys.
    pub const NEUTRALS: [Self; 12] = [
        Self::Ink0,
        Self::Ink1,
        Self::Ink2,
        Self::Ink3,
        Self::Line,
        Self::Line2,
        Self::Line3,
        Self::Fg,
        Self::Fg1,
        Self::Fg2,
        Self::Fg3,
        Self::Bar,
    ];

    /// The custom property's name, without `--`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ink0 => "ink-0",
            Self::Ink1 => "ink-1",
            Self::Ink2 => "ink-2",
            Self::Ink3 => "ink-3",
            Self::Line => "line",
            Self::Line2 => "line-2",
            Self::Line3 => "line-3",
            Self::Fg => "fg",
            Self::Fg1 => "fg-1",
            Self::Fg2 => "fg-2",
            Self::Fg3 => "fg-3",
            Self::Accent => "accent",
            Self::AccentInk => "accent-ink",
            Self::AccentText => "accent-text",
            Self::Ring => "ring",
            Self::Success => "success",
            Self::Amber => "amber",
            Self::Danger => "danger",
            Self::Info => "info",
            Self::Violet => "violet",
            Self::Cyan => "cyan",
            Self::Rose => "rose",
            Self::Bar => "bar",
            Self::Chart1 => "chart-1",
            Self::Chart2 => "chart-2",
            Self::Chart3 => "chart-3",
            Self::Chart4 => "chart-4",
            Self::Chart5 => "chart-5",
        }
    }

    /// What the token is for, in a few words.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ink0 => "Page",
            Self::Ink1 => "Cards and inputs",
            Self::Ink2 => "Hover",
            Self::Ink3 => "Selected",
            Self::Line => "Dividers",
            Self::Line2 => "Borders",
            Self::Line3 => "Borders on hover",
            Self::Fg => "Headings",
            Self::Fg1 => "Body text",
            Self::Fg2 => "Secondary text",
            Self::Fg3 => "Hints",
            Self::Accent => "Accent",
            Self::AccentInk => "Text on the accent",
            Self::AccentText => "Accent as text",
            Self::Ring => "Focus ring",
            Self::Success => "Success",
            Self::Amber => "Warning",
            Self::Danger => "Danger",
            Self::Info => "Info",
            Self::Violet => "Violet",
            Self::Cyan => "Cyan",
            Self::Rose => "Rose",
            Self::Bar => "Neutral bars",
            Self::Chart1 => "Chart 1",
            Self::Chart2 => "Chart 2",
            Self::Chart3 => "Chart 3",
            Self::Chart4 => "Chart 4",
            Self::Chart5 => "Chart 5",
        }
    }

    /// The group the builder shows the token in.
    pub const fn group(self) -> TokenGroup {
        match self {
            Self::Ink0
            | Self::Ink1
            | Self::Ink2
            | Self::Ink3
            | Self::Line
            | Self::Line2
            | Self::Line3 => TokenGroup::Surfaces,
            Self::Fg | Self::Fg1 | Self::Fg2 | Self::Fg3 => TokenGroup::Text,
            Self::Accent | Self::AccentInk | Self::AccentText | Self::Ring => TokenGroup::Accent,
            Self::Success
            | Self::Amber
            | Self::Danger
            | Self::Info
            | Self::Violet
            | Self::Cyan
            | Self::Rose => TokenGroup::Tones,
            Self::Bar
            | Self::Chart1
            | Self::Chart2
            | Self::Chart3
            | Self::Chart4
            | Self::Chart5 => TokenGroup::Charts,
        }
    }

    /// The token called `name` (`ink-0`), exactly.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|token| token.name() == name)
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// One mode's colours: a value for every [`Token`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Palette([Rgb; TOKEN_COUNT]);

impl Palette {
    /// The colour of `token`.
    pub const fn get(&self, token: Token) -> Rgb {
        self.0[token.index()]
    }

    /// Sets the colour of `token`.
    pub fn set(&mut self, token: Token, colour: Rgb) {
        self.0[token.index()] = colour;
    }

    /// This palette with `token` set to `colour`.
    #[must_use]
    pub fn with(mut self, token: Token, colour: Rgb) -> Self {
        self.set(token, colour);
        self
    }

    /// Every token and its colour, in [`Token::ALL`]'s order.
    pub fn iter(&self) -> impl Iterator<Item = (Token, Rgb)> + '_ {
        Token::ALL.into_iter().map(|token| (token, self.get(token)))
    }

    /// A palette from a colour per token, in [`Token::ALL`]'s order.
    pub const fn from_array(colours: [Rgb; TOKEN_COUNT]) -> Self {
        Self(colours)
    }

    /// Sets the accent to `fill` and derives what reads on and around it: the
    /// text on it (white where white reads, else a deep shade of its hue), the
    /// accent as text (moved toward the page's text until it reaches 4.5:1 on
    /// every surface) and the focus ring (3:1 on every surface).
    pub fn derive_accent(&mut self, fill: Rgb) {
        let toward = self.get(Token::Fg);
        let surfaces = pairs::SURFACES.map(|surface| self.get(surface));
        self.set(Token::Accent, fill);
        self.set(Token::AccentInk, fill.ink());
        self.set(
            Token::AccentText,
            fill.until_contrast(&surfaces, 4.5, toward),
        );
        self.set(Token::Ring, fill.until_contrast(&surfaces, 3.0, toward));
    }

    /// What the accent is mixed toward on hover: away from the text on it, so the
    /// text reads better on hover, never worse. Black under light text, white
    /// under dark text.
    pub fn hover_mix(&self) -> Rgb {
        if self.get(Token::AccentInk).luminance() > 0.5 {
            Rgb::BLACK
        } else {
            Rgb::WHITE
        }
    }

    /// Moves `token` toward `toward` until every text pair it draws reaches
    /// `ratio` in this palette, as `side`.
    fn settle(&mut self, token: Token, ratio: f64, toward: Rgb, side: Side) {
        let start = self.get(token);
        let reads = |palette: &Self| {
            pairs::pairs()
                .iter()
                .filter(|pair| pair.kind == PairKind::Text)
                .filter(|pair| pairs::draws(pair.text, token) || pairs::draws(pair.on, token))
                .all(|pair| pair.ratio(palette, side) >= ratio)
        };
        for step in 0..=50 {
            let mut next = *self;
            next.set(token, start.mix(toward, f64::from(step) / 50.0));
            if reads(&next) {
                *self = next;
                return;
            }
        }
        self.set(token, toward);
    }
}

/// Which palette: what `light-dark()` picks between.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    /// The light palette.
    Light,
    /// The dark palette.
    Dark,
}

impl Side {
    /// Both, light first.
    pub const BOTH: [Self; 2] = [Self::Light, Self::Dark];

    /// `light` or `dark`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// The corners: controls (buttons, inputs, badges) and surfaces (cards,
/// popovers, dialogs). Never a pill: controls go to 8 px at most.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Radius {
    control: u8,
    surface: u8,
}

/// Why a length was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LengthError {
    /// Not a whole number of pixels, such as `4` or `4px`.
    Format,
    /// Outside what the toolkit allows.
    Range,
}

impl std::fmt::Display for LengthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Format => "Use a whole number of pixels, such as 4px.",
            Self::Range => {
                "Controls take 0 to 8 px, surfaces 0 to 12 px and no less than controls."
            }
        })
    }
}

impl std::error::Error for LengthError {}

impl Radius {
    /// Square corners.
    pub const SHARP: Self = Self::new_unchecked(0, 0);
    /// 2 px controls, 4 px surfaces.
    pub const SMALL: Self = Self::new_unchecked(2, 4);
    /// 4 px controls, 6 px surfaces: the toolkit's own.
    pub const DEFAULT: Self = Self::new_unchecked(4, 6);
    /// 6 px controls, 8 px surfaces.
    pub const ROUND: Self = Self::new_unchecked(6, 8);
    /// 8 px controls, 12 px surfaces.
    pub const ROUNDER: Self = Self::new_unchecked(8, 12);
    /// The named steps, smallest first, with their ids and labels.
    pub const STEPS: [(&'static str, &'static str, Self); 5] = [
        ("sharp", "Sharp", Self::SHARP),
        ("small", "Small", Self::SMALL),
        ("default", "Default", Self::DEFAULT),
        ("round", "Round", Self::ROUND),
        ("rounder", "Rounder", Self::ROUNDER),
    ];

    const fn new_unchecked(control: u8, surface: u8) -> Self {
        Self { control, surface }
    }

    /// Corners of `control` px on controls and `surface` px on surfaces.
    pub fn new(control: u8, surface: u8) -> Result<Self, LengthError> {
        if control > 8 || surface > 12 || surface < control {
            return Err(LengthError::Range);
        }
        Ok(Self { control, surface })
    }

    /// Reads one length: a whole number of pixels, `4` or `4px`.
    pub fn parse(text: &str) -> Result<u8, LengthError> {
        let text = text.trim_matches(|c: char| c.is_ascii_whitespace());
        let digits = text.strip_suffix("px").unwrap_or(text);
        if digits.is_empty() || digits.len() > 2 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(LengthError::Format);
        }
        digits.parse::<u8>().map_err(|_| LengthError::Format)
    }

    /// The controls' corner, in px.
    pub const fn control(self) -> u8 {
        self.control
    }

    /// The surfaces' corner, in px.
    pub const fn surface(self) -> u8 {
        self.surface
    }

    /// The step's id, when it is one of [`STEPS`](Self::STEPS).
    pub fn step(self) -> Option<&'static str> {
        Self::STEPS
            .iter()
            .find(|(_, _, radius)| *radius == self)
            .map(|(id, _, _)| *id)
    }
}

impl Default for Radius {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How tightly controls and space are packed. Text keeps its size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Density {
    /// Seven eighths: 28 px controls.
    Compact,
    /// 32 px controls.
    #[default]
    Default,
    /// Nine eighths: 36 px controls.
    Comfortable,
}

impl Density {
    /// Every density.
    pub const ALL: [Self; 3] = [Self::Compact, Self::Default, Self::Comfortable];

    /// The `data-density` value.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Default => "default",
            Self::Comfortable => "comfortable",
        }
    }

    /// The name shown.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Default => "Default",
            Self::Comfortable => "Comfortable",
        }
    }

    /// The density with the id `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|density| density.id() == id)
    }
}

/// The text face, from a safe list: Geist ships with the toolkit; the others
/// are the system's own fonts and download nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Font {
    /// Geist, self-hosted.
    #[default]
    Geist,
    /// The operating system's interface font.
    System,
    /// A humanist sans: Seravek, Gill Sans Nova, Ubuntu, Calibri.
    Humanist,
    /// A geometric sans: Avenir, Montserrat, Corbel.
    Geometric,
    /// A serif for reading: Charter, Sitka, Cambria.
    Serif,
}

impl Font {
    /// Every font.
    pub const ALL: [Self; 5] = [
        Self::Geist,
        Self::System,
        Self::Humanist,
        Self::Geometric,
        Self::Serif,
    ];

    /// The id stored and posted.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Geist => "geist",
            Self::System => "system",
            Self::Humanist => "humanist",
            Self::Geometric => "geometric",
            Self::Serif => "serif",
        }
    }

    /// The name shown.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Geist => "Geist",
            Self::System => "System",
            Self::Humanist => "Humanist",
            Self::Geometric => "Geometric",
            Self::Serif => "Serif",
        }
    }

    /// The `font-family` list. Provenance: the system stacks follow Modern Font
    /// Stacks (CC0).
    pub const fn stack(self) -> &'static str {
        match self {
            Self::Geist => {
                "Geist,ui-sans-serif,system-ui,-apple-system,\"Segoe UI\",Roboto,sans-serif"
            }
            Self::System => {
                "system-ui,-apple-system,\"Segoe UI\",Roboto,\"Helvetica Neue\",Arial,sans-serif"
            }
            Self::Humanist => {
                "Seravek,\"Gill Sans Nova\",Ubuntu,Calibri,\"DejaVu Sans\",source-sans-pro,sans-serif"
            }
            Self::Geometric => "Avenir,Montserrat,Corbel,\"URW Gothic\",source-sans-pro,sans-serif",
            Self::Serif => "Charter,\"Bitstream Charter\",\"Sitka Text\",Cambria,serif",
        }
    }

    /// The font with the id `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|font| font.id() == id)
    }
}

/// Everything a theme sets: both palettes, the corners, the density and the font.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ThemeSpec {
    /// The dark palette.
    pub dark: Palette,
    /// The light palette.
    pub light: Palette,
    /// The corners.
    pub radius: Radius,
    /// The density.
    pub density: Density,
    /// The text face.
    pub font: Font,
}

impl ThemeSpec {
    /// The palette for `side`.
    pub const fn palette(&self, side: Side) -> &Palette {
        match side {
            Side::Light => &self.light,
            Side::Dark => &self.dark,
        }
    }

    /// The palette for `side`, to change.
    pub fn palette_mut(&mut self, side: Side) -> &mut Palette {
        match side {
            Side::Light => &mut self.light,
            Side::Dark => &mut self.dark,
        }
    }

    /// The pairs that fall short, in both palettes. Empty means every text pair
    /// reaches WCAG AA and every graphic pair 3:1.
    pub fn shortfalls(&self) -> Vec<Shortfall> {
        Side::BOTH
            .into_iter()
            .flat_map(|side| {
                let palette = *self.palette(side);
                pairs().iter().filter_map(move |pair| {
                    let ratio = pair.ratio(&palette, side);
                    (ratio < pair.min).then_some(Shortfall {
                        side,
                        pair: *pair,
                        ratio,
                    })
                })
            })
            .collect()
    }

    /// Whether every text pair reaches WCAG AA in both palettes: what saving and
    /// exporting need.
    pub fn text_passes(&self) -> bool {
        self.shortfalls()
            .iter()
            .all(|shortfall| shortfall.pair.kind != PairKind::Text)
    }
}

/// A pair that falls short in one palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shortfall {
    /// The palette.
    pub side: Side,
    /// The pair.
    pub pair: Pair,
    /// The ratio it has.
    pub ratio: f64,
}

/// A theme: an id, a name and a [`ThemeSpec`]. The built-in ones are
/// associated constants; [`Theme::custom`] makes your own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme(Repr);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Repr {
    Builtin(Builtin),
    Custom(Arc<Custom>),
}

#[derive(Debug, PartialEq, Eq)]
struct Custom {
    id: ThemeId,
    name: String,
    spec: ThemeSpec,
    /// The canonical short form: the stylesheet's URL and the cookie's value.
    form: String,
    /// `c` and the short form's hash: the `data-theme` value.
    key: String,
}

impl Theme {
    /// The toolkit's own look: green on near-black, or on white.
    pub const NAVREE: Self = Self(Repr::Builtin(Builtin::Navree));
    /// Plain greys, with a white or black accent.
    pub const NEUTRAL: Self = Self(Repr::Builtin(Builtin::Neutral));
    /// Slate greys and a blue accent.
    pub const OCEAN: Self = Self(Repr::Builtin(Builtin::Ocean));
    /// Cool greys and a violet accent.
    pub const VIOLET: Self = Self(Repr::Builtin(Builtin::Violet));
    /// Warm greys and an orange accent.
    pub const SUNSET: Self = Self(Repr::Builtin(Builtin::Sunset));
    /// Neutral greys and a rose accent.
    pub const ROSE: Self = Self(Repr::Builtin(Builtin::Rose));
    /// Text at 7:1 or more and strong borders.
    pub const CONTRAST: Self = Self(Repr::Builtin(Builtin::Contrast));

    /// Every built-in theme, the default first.
    pub const BUILT_IN: [Self; 7] = [
        Self::NAVREE,
        Self::NEUTRAL,
        Self::OCEAN,
        Self::VIOLET,
        Self::SUNSET,
        Self::ROSE,
        Self::CONTRAST,
    ];

    /// A theme of your own, refused when any text pair fails WCAG AA in either
    /// palette. The name is only ever shown as text.
    pub fn custom(
        id: ThemeId,
        name: impl Into<String>,
        spec: ThemeSpec,
    ) -> Result<Self, ImportError> {
        if !spec.text_passes() {
            return Err(ImportError::Contrast(spec.shortfalls()));
        }
        Ok(Self::unchecked(id, name.into(), spec))
    }

    /// A theme whose contrast isn't checked: the builder's draft.
    pub(crate) fn unchecked(id: ThemeId, name: String, spec: ThemeSpec) -> Self {
        let form = codec::to_form(&spec);
        let key = format!("c{}", crate::assets::content_hash(form.as_bytes()));
        let name: String = name.chars().filter(|c| !c.is_control()).take(64).collect();
        Self(Repr::Custom(Arc::new(Custom {
            id,
            name,
            spec,
            form,
            key,
        })))
    }

    /// The built-in theme with the id `id`.
    pub fn builtin(id: &str) -> Option<Self> {
        Self::BUILT_IN.into_iter().find(|theme| theme.id() == id)
    }

    /// The theme's id: a built-in's name, or yours.
    pub fn id(&self) -> &str {
        match &self.0 {
            Repr::Builtin(builtin) => builtin.id(),
            Repr::Custom(custom) => custom.id.as_str(),
        }
    }

    /// The name shown.
    pub fn name(&self) -> &str {
        match &self.0 {
            Repr::Builtin(builtin) => builtin.name(),
            Repr::Custom(custom) => &custom.name,
        }
    }

    /// One line about it: the built-ins have one.
    pub fn description(&self) -> Option<&'static str> {
        match &self.0 {
            Repr::Builtin(builtin) => Some(builtin.description()),
            Repr::Custom(_) => None,
        }
    }

    /// The colours, corners, density and font.
    pub fn spec(&self) -> &ThemeSpec {
        match &self.0 {
            Repr::Builtin(builtin) => builtin.spec(),
            Repr::Custom(custom) => &custom.spec,
        }
    }

    /// Whether it ships in the toolkit's stylesheet.
    pub fn is_builtin(&self) -> bool {
        matches!(self.0, Repr::Builtin(_))
    }

    /// The `data-theme` value: a built-in's id, or `c` and twelve hex digits of
    /// the theme's hash. Never text you gave.
    pub fn key(&self) -> &str {
        match &self.0 {
            Repr::Builtin(builtin) => builtin.id(),
            Repr::Custom(custom) => &custom.key,
        }
    }

    /// The theme's stylesheet, for a theme of your own; a built-in needs none.
    pub fn stylesheet(&self) -> Option<String> {
        match &self.0 {
            Repr::Builtin(_) => None,
            Repr::Custom(custom) => Some(format!("{}/{}.css", css::SHEET_ROOT, custom.form)),
        }
    }

    /// The stylesheet of a theme of your own, as served at
    /// [`stylesheet`](Self::stylesheet); `None` for a built-in, which is in the
    /// toolkit's own.
    pub fn css(&self) -> Option<String> {
        (!self.is_builtin()).then(|| css::sheet(self))
    }

    /// The short form a cookie or a URL carries: a built-in's id, or `v1-` and the
    /// theme's values.
    pub fn short_form(&self) -> &str {
        match &self.0 {
            Repr::Builtin(builtin) => builtin.id(),
            Repr::Custom(custom) => &custom.form,
        }
    }

    /// The theme a short form names: a built-in's id, or a theme's values. A
    /// theme from its values is called "Custom" and has the id `custom`; its
    /// contrast is checked.
    pub fn from_short_form(form: &str) -> Option<Self> {
        if let Some(theme) = Self::builtin(form) {
            return Some(theme);
        }
        let spec = codec::from_form(form)?;
        Self::custom(ThemeId::custom(), "Custom", spec).ok()
    }

    /// The same look under another id and name.
    pub fn renamed(&self, id: ThemeId, name: impl Into<String>) -> Self {
        Self::unchecked(id, name.into(), *self.spec())
    }

    /// The family of greys a built-in theme is made of; `None` for one with a
    /// table of its own (high contrast, or a theme of yours).
    pub fn neutral(&self) -> Option<Neutral> {
        match &self.0 {
            Repr::Builtin(Builtin::Navree) => Some(Neutral::Navree),
            Repr::Builtin(Builtin::Neutral | Builtin::Rose) => Some(Neutral::Neutral),
            Repr::Builtin(Builtin::Ocean) => Some(Neutral::Slate),
            Repr::Builtin(Builtin::Violet) => Some(Neutral::Zinc),
            Repr::Builtin(Builtin::Sunset) => Some(Neutral::Stone),
            Repr::Builtin(Builtin::Contrast) | Repr::Custom(_) => None,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::NAVREE
    }
}

/// The built-in themes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Builtin {
    Navree,
    Neutral,
    Ocean,
    Violet,
    Sunset,
    Rose,
    Contrast,
}

impl Builtin {
    const fn id(self) -> &'static str {
        match self {
            Self::Navree => "navree",
            Self::Neutral => "neutral",
            Self::Ocean => "ocean",
            Self::Violet => "violet",
            Self::Sunset => "sunset",
            Self::Rose => "rose",
            Self::Contrast => "contrast",
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Navree => "Navree",
            Self::Neutral => "Neutral",
            Self::Ocean => "Ocean",
            Self::Violet => "Violet",
            Self::Sunset => "Sunset",
            Self::Rose => "Rose",
            Self::Contrast => "High contrast",
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Navree => "The toolkit's own look: green on near-black, or on white.",
            Self::Neutral => "Plain greys, with a white or black accent.",
            Self::Ocean => "Slate greys and a blue accent.",
            Self::Violet => "Cool greys and a violet accent.",
            Self::Sunset => "Warm greys and an orange accent.",
            Self::Rose => "Neutral greys and a rose accent.",
            Self::Contrast => "Text at 7:1 or more, strong borders and a bright accent.",
        }
    }

    fn spec(self) -> &'static ThemeSpec {
        static SPECS: LazyLock<[ThemeSpec; 7]> = LazyLock::new(|| {
            [
                Builtin::Navree,
                Builtin::Neutral,
                Builtin::Ocean,
                Builtin::Violet,
                Builtin::Sunset,
                Builtin::Rose,
                Builtin::Contrast,
            ]
            .map(build)
        });
        &SPECS[self as usize]
    }
}

/// A family of neutral greys: the toolkit's own, or one at a hue and chroma.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Neutral {
    /// The toolkit's own, a faint green.
    Navree,
    /// No hue.
    Neutral,
    /// A faint blue.
    Slate,
    /// A faint warm brown.
    Stone,
    /// A faint violet-blue.
    Zinc,
}

impl Neutral {
    /// Every family.
    pub const ALL: [Self; 5] = [
        Self::Navree,
        Self::Neutral,
        Self::Slate,
        Self::Stone,
        Self::Zinc,
    ];

    /// The id posted and stored.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Navree => "navree",
            Self::Neutral => "neutral",
            Self::Slate => "slate",
            Self::Stone => "stone",
            Self::Zinc => "zinc",
        }
    }

    /// The name shown.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Navree => "Navree",
            Self::Neutral => "Neutral",
            Self::Slate => "Slate",
            Self::Stone => "Stone",
            Self::Zinc => "Zinc",
        }
    }

    /// The family with the id `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|neutral| neutral.id() == id)
    }

    /// The hue (degrees) and the most chroma, or `None` for the toolkit's own table.
    const fn tint(self) -> Option<(f64, f64)> {
        match self {
            Self::Navree => None,
            Self::Neutral => Some((0.0, 0.0)),
            Self::Slate => Some((255.0, 0.022)),
            Self::Stone => Some((60.0, 0.010)),
            Self::Zinc => Some((286.0, 0.010)),
        }
    }

    /// Puts this family's greys into both palettes: the toolkit's own lightness
    /// at every step, in this family's hue.
    pub fn apply(self, spec: &mut ThemeSpec) {
        let base = navree();
        for side in Side::BOTH {
            let from = *base.palette(side);
            let palette = spec.palette_mut(side);
            for token in Token::NEUTRALS {
                let colour = match self.tint() {
                    None => from.get(token),
                    Some((hue, most)) => {
                        let [l, c, _] = from.get(token).to_oklch();
                        // Text keeps less tint than surfaces, so it stays crisp.
                        let chroma = if most == 0.0 {
                            0.0
                        } else {
                            most.min(c.max(most / 2.0))
                        };
                        Rgb::from_oklch(l, chroma, hue)
                    }
                };
                palette.set(token, colour);
            }
        }
    }
}

/// An accent the builder offers by name: a fill for each palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Swatch {
    /// The id posted.
    pub id: &'static str,
    /// The name shown.
    pub label: &'static str,
    /// The fill in the dark palette.
    pub dark: Rgb,
    /// The fill in the light palette.
    pub light: Rgb,
}

/// The accents the builder offers by name.
pub const SWATCHES: [Swatch; 8] = [
    Swatch {
        id: "green",
        label: "Green",
        dark: Rgb::hex(0x3dff8f),
        light: Rgb::hex(0x0b7a3e),
    },
    Swatch {
        id: "blue",
        label: "Blue",
        dark: Rgb::hex(0x5aa2ff),
        light: Rgb::hex(0x1f55d6),
    },
    Swatch {
        id: "violet",
        label: "Violet",
        dark: Rgb::hex(0xac8fff),
        light: Rgb::hex(0x6a2fd6),
    },
    Swatch {
        id: "orange",
        label: "Orange",
        dark: Rgb::hex(0xff9a4d),
        light: Rgb::hex(0xb8460e),
    },
    Swatch {
        id: "rose",
        label: "Rose",
        dark: Rgb::hex(0xff7a93),
        light: Rgb::hex(0xc0234a),
    },
    Swatch {
        id: "cyan",
        label: "Cyan",
        dark: Rgb::hex(0x4fd6e6),
        light: Rgb::hex(0x0a6b78),
    },
    Swatch {
        id: "amber",
        label: "Amber",
        dark: Rgb::hex(0xffc35a),
        light: Rgb::hex(0x8a5a00),
    },
    Swatch {
        id: "mono",
        label: "Mono",
        dark: Rgb::hex(0xebebeb),
        light: Rgb::hex(0x1a1a1a),
    },
];

impl Swatch {
    /// The swatch with the id `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        SWATCHES.into_iter().find(|swatch| swatch.id == id)
    }

    /// Sets this accent in both palettes, with what derives from it.
    pub fn apply(self, spec: &mut ThemeSpec) {
        spec.dark.derive_accent(self.dark);
        spec.light.derive_accent(self.light);
    }
}

/// The toolkit's own theme, from [`crate::tokens::TOKENS`].
fn navree() -> ThemeSpec {
    let read = |side: Side| {
        let mut colours = [Rgb::BLACK; TOKEN_COUNT];
        for token in Token::ALL {
            let entry = crate::tokens::TOKENS
                .iter()
                .find(|entry| entry.name == token.name());
            let value = entry.map(|entry| match side {
                Side::Light => entry.light,
                Side::Dark => entry.dark,
            });
            colours[token.index()] = value
                .and_then(|value| Rgb::parse(value).ok())
                .unwrap_or(Rgb::BLACK);
        }
        Palette(colours)
    };
    ThemeSpec {
        dark: read(Side::Dark),
        light: read(Side::Light),
        radius: Radius::DEFAULT,
        density: Density::Default,
        font: Font::Geist,
    }
}

/// The success green of every theme whose accent isn't green.
const SUCCESS: (Rgb, Rgb) = (Rgb::hex(0x5ee08f), Rgb::hex(0x106836));

/// Sets the charts to the colours of `tones`, in order.
fn charts(spec: &mut ThemeSpec, tones: [Token; 5]) {
    let series = [
        Token::Chart1,
        Token::Chart2,
        Token::Chart3,
        Token::Chart4,
        Token::Chart5,
    ];
    for side in Side::BOTH {
        let palette = spec.palette_mut(side);
        for (chart, tone) in series.into_iter().zip(tones) {
            palette.set(chart, palette.get(tone));
        }
    }
}

fn build(builtin: Builtin) -> ThemeSpec {
    let mut spec = navree();
    let accent = |spec: &mut ThemeSpec, dark: u32, light: u32| {
        spec.dark.derive_accent(Rgb::hex(dark));
        spec.light.derive_accent(Rgb::hex(light));
        spec.dark.set(Token::Success, SUCCESS.0);
        spec.light.set(Token::Success, SUCCESS.1);
    };
    match builtin {
        Builtin::Navree => {}
        Builtin::Neutral => {
            Neutral::Neutral.apply(&mut spec);
            accent(&mut spec, 0xebebeb, 0x1a1a1a);
            charts(
                &mut spec,
                [
                    Token::Info,
                    Token::Success,
                    Token::Violet,
                    Token::Amber,
                    Token::Rose,
                ],
            );
        }
        Builtin::Ocean => {
            Neutral::Slate.apply(&mut spec);
            accent(&mut spec, 0x5aa2ff, 0x1f55d6);
            charts(
                &mut spec,
                [
                    Token::Accent,
                    Token::Cyan,
                    Token::Violet,
                    Token::Amber,
                    Token::Rose,
                ],
            );
        }
        Builtin::Violet => {
            Neutral::Zinc.apply(&mut spec);
            accent(&mut spec, 0xac8fff, 0x6a2fd6);
            charts(
                &mut spec,
                [
                    Token::Accent,
                    Token::Cyan,
                    Token::Success,
                    Token::Amber,
                    Token::Rose,
                ],
            );
        }
        Builtin::Sunset => {
            Neutral::Stone.apply(&mut spec);
            accent(&mut spec, 0xff9a4d, 0xb8460e);
            charts(
                &mut spec,
                [
                    Token::Accent,
                    Token::Info,
                    Token::Violet,
                    Token::Success,
                    Token::Rose,
                ],
            );
        }
        Builtin::Rose => {
            Neutral::Neutral.apply(&mut spec);
            accent(&mut spec, 0xff7a93, 0xc0234a);
            charts(
                &mut spec,
                [
                    Token::Accent,
                    Token::Info,
                    Token::Violet,
                    Token::Amber,
                    Token::Cyan,
                ],
            );
        }
        Builtin::Contrast => contrast(&mut spec),
    }
    spec
}

/// High contrast: black and white, every text pair at 7:1 or more, borders at
/// 3:1, a bright yellow accent in dark and a deep blue in light.
fn contrast(spec: &mut ThemeSpec) {
    let set = |palette: &mut Palette, pairs: &[(Token, u32)]| {
        for (token, value) in pairs {
            palette.set(*token, Rgb::hex(*value));
        }
    };
    set(
        &mut spec.dark,
        &[
            (Token::Ink0, 0x000000),
            (Token::Ink1, 0x0a0a0a),
            (Token::Ink2, 0x121212),
            (Token::Ink3, 0x1c1c1c),
            (Token::Line, 0x6e6e6e),
            (Token::Line2, 0x8c8c8c),
            (Token::Line3, 0xa8a8a8),
            (Token::Fg, 0xffffff),
            (Token::Fg1, 0xf5f5f5),
            (Token::Fg2, 0xe0e0e0),
            (Token::Fg3, 0xc8c8c8),
            (Token::Bar, 0xa8a8a8),
            (Token::Success, 0x6ee79b),
            (Token::Amber, 0xffc266),
            (Token::Danger, 0xff9a9a),
            (Token::Info, 0x92caff),
            (Token::Violet, 0xcdb8ff),
            (Token::Cyan, 0x7ee6f2),
            (Token::Rose, 0xffa8bc),
        ],
    );
    set(
        &mut spec.light,
        &[
            (Token::Ink0, 0xffffff),
            (Token::Ink1, 0xffffff),
            (Token::Ink2, 0xf2f2f2),
            (Token::Ink3, 0xe8e8e8),
            (Token::Line, 0x767676),
            (Token::Line2, 0x5e5e5e),
            (Token::Line3, 0x474747),
            (Token::Fg, 0x000000),
            (Token::Fg1, 0x0d0d0d),
            (Token::Fg2, 0x262626),
            (Token::Fg3, 0x3d3d3d),
            (Token::Bar, 0x5e5e5e),
        ],
    );
    spec.dark.derive_accent(Rgb::hex(0xffe066));
    spec.light.derive_accent(Rgb::hex(0x0a3fd0));
    // Every text colour, then every tone and the accent as text, moved until
    // each text pair it draws reads at 7:1: deeper in light, brighter in dark.
    let text = [Token::Fg1, Token::Fg2, Token::Fg3];
    for token in text
        .into_iter()
        .chain(pairs::TONES)
        .chain([Token::AccentText])
    {
        spec.light.settle(token, 7.0, Rgb::BLACK, Side::Light);
        spec.dark.settle(token, 7.0, Rgb::WHITE, Side::Dark);
    }
    charts(
        spec,
        [
            Token::Accent,
            Token::Info,
            Token::Violet,
            Token::Success,
            Token::Rose,
        ],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_token_has_a_unique_name_and_is_in_the_table() {
        let mut names: Vec<&str> = Token::ALL.iter().map(|token| token.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TOKEN_COUNT);
        for (at, token) in Token::ALL.iter().enumerate() {
            assert_eq!(token.index(), at, "{token:?} is out of order");
            assert_eq!(Token::from_name(token.name()), Some(*token));
            let entry = crate::tokens::TOKENS
                .iter()
                .find(|entry| entry.name == token.name())
                .unwrap_or_else(|| panic!("{} is not in tokens::TOKENS", token.name()));
            assert!(
                Rgb::parse(entry.dark).is_ok() && Rgb::parse(entry.light).is_ok(),
                "{}",
                token.name()
            );
        }
        assert_eq!(Token::from_name("ink-9"), None);
        assert_eq!(Token::from_name("INK-0"), None);
    }

    #[test]
    fn the_default_theme_is_the_token_table() {
        let spec = Theme::NAVREE.spec();
        for entry in crate::tokens::TOKENS {
            let Some(token) = Token::from_name(entry.name) else {
                continue;
            };
            assert_eq!(spec.dark.get(token).to_hex(), entry.dark, "{}", entry.name);
            assert_eq!(
                spec.light.get(token).to_hex(),
                entry.light,
                "{}",
                entry.name
            );
        }
        assert_eq!(spec.radius, Radius::DEFAULT);
    }

    #[test]
    fn every_built_in_theme_passes_every_pair_in_both_palettes() {
        for theme in Theme::BUILT_IN {
            let shortfalls = theme.spec().shortfalls();
            assert!(
                shortfalls.is_empty(),
                "{}: {:?}",
                theme.id(),
                shortfalls
                    .iter()
                    .map(|s| format!(
                        "{} {} on {} is {:.2}:1",
                        s.side.as_str(),
                        s.pair.text.describe(),
                        s.pair.on.describe(),
                        s.ratio
                    ))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn high_contrast_reaches_7_to_1_for_text_and_3_to_1_for_borders() {
        let spec = Theme::CONTRAST.spec();
        for side in Side::BOTH {
            let palette = spec.palette(side);
            for pair in pairs().iter().filter(|pair| pair.kind == PairKind::Text) {
                let ratio = pair.ratio(palette, side);
                assert!(
                    ratio >= 7.0,
                    "{} {} on {}: {ratio:.2}",
                    side.as_str(),
                    pair.text.describe(),
                    pair.on.describe()
                );
            }
            for line in [Token::Line, Token::Line2] {
                let ratio = palette.get(line).contrast(palette.get(Token::Ink1));
                assert!(
                    ratio >= 3.0,
                    "{} {}: {ratio:.2}",
                    side.as_str(),
                    line.name()
                );
            }
        }
    }

    #[test]
    fn built_in_ids_are_unique_and_found() {
        for theme in Theme::BUILT_IN {
            assert_eq!(Theme::builtin(theme.id()), Some(theme.clone()));
            assert_eq!(theme.key(), theme.id());
            assert!(theme.stylesheet().is_none());
            assert!(theme.description().is_some());
        }
        assert_eq!(Theme::builtin("Ocean"), None);
        assert_eq!(Theme::builtin("custom"), None);
    }

    #[test]
    fn a_neutral_family_keeps_the_lightness_and_changes_the_hue() {
        let mut spec = *Theme::NAVREE.spec();
        Neutral::Slate.apply(&mut spec);
        let ours = Theme::NAVREE.spec();
        for token in Token::NEUTRALS {
            let (a, b) = (
                spec.dark.get(token).to_oklch()[0],
                ours.dark.get(token).to_oklch()[0],
            );
            assert!((a - b).abs() < 0.01, "{}", token.name());
        }
        assert_ne!(spec.dark.get(Token::Ink1), ours.dark.get(Token::Ink1));
        let mut grey = *Theme::NAVREE.spec();
        Neutral::Neutral.apply(&mut grey);
        let ink = grey.light.get(Token::Ink2);
        assert!(ink.r == ink.g && ink.g == ink.b, "no hue: {ink}");
    }

    #[test]
    fn a_derived_accent_always_reads() {
        for value in (0..0x0100_0000).step_by(0x2_9a1b) {
            let mut spec = *Theme::NAVREE.spec();
            spec.dark.derive_accent(Rgb::hex(value));
            spec.light.derive_accent(Rgb::hex(value));
            for side in Side::BOTH {
                let palette = spec.palette(side);
                let fill = palette.get(Token::Accent);
                assert!(palette.get(Token::AccentInk).contrast(fill) >= 4.5);
                for surface in [Token::Ink0, Token::Ink1] {
                    assert!(
                        palette
                            .get(Token::AccentText)
                            .contrast(palette.get(surface))
                            >= 4.5,
                        "{} {fill}",
                        side.as_str()
                    );
                    assert!(palette.get(Token::Ring).contrast(palette.get(surface)) >= 3.0);
                }
            }
        }
    }

    #[test]
    fn a_custom_theme_that_fails_contrast_is_refused() {
        let mut spec = *Theme::NAVREE.spec();
        spec.light.set(Token::Fg3, Rgb::hex(0xcccccc));
        let refused = Theme::custom(ThemeId::parse("pale").unwrap(), "Pale", spec);
        assert!(matches!(refused, Err(ImportError::Contrast(ref list)) if !list.is_empty()));
        let fine = Theme::custom(
            ThemeId::parse("acme").unwrap(),
            "Acme",
            *Theme::OCEAN.spec(),
        )
        .unwrap();
        assert_eq!(fine.id(), "acme");
        assert!(
            fine.key().starts_with('c') && fine.key().len() == 13,
            "{}",
            fine.key()
        );
        assert!(
            fine.stylesheet()
                .unwrap()
                .starts_with("/_navree/ui/theme/v1-")
        );
    }

    #[test]
    fn radius_lengths_are_parsed_strictly() {
        assert_eq!(Radius::parse("4px"), Ok(4));
        assert_eq!(Radius::parse(" 12 "), Ok(12));
        for bad in [
            "", "px", "4.5px", "-1px", "4em", "4 px", "100", "4px;", "1e1", "+4",
        ] {
            assert_eq!(Radius::parse(bad), Err(LengthError::Format), "{bad:?}");
        }
        assert!(Radius::new(9, 12).is_err());
        assert!(Radius::new(4, 13).is_err());
        assert!(Radius::new(6, 4).is_err());
        assert_eq!(Radius::new(4, 6), Ok(Radius::DEFAULT));
        assert_eq!(Radius::DEFAULT.step(), Some("default"));
        assert_eq!(Radius::new(3, 5).unwrap().step(), None);
    }
}
