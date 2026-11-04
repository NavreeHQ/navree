//! The theme builder: a page where a developer or an app's admin builds a
//! theme, sees real components drawn in it in both modes, checks its contrast
//! and takes it away as Rust or as a stored theme.
//!
//! The builder is a plain `GET` form whose state is the URL, so a draft can be
//! shared, reloaded and walked back. With JavaScript on, the page follows each
//! change in place; with it off, the Preview button does the same. Every value
//! is read on the server by [`BuilderDraft::from_query`]: colours through
//! [`Rgb::parse`], the rest from closed lists. The draft's stylesheet is
//! written by Navree, so nothing typed reaches the page as CSS.

use navree_http::extract::urlencoded;

use super::{
    Appearance, Density, Font, Mode, Neutral, Paint, Pair, PairKind, Radius, Rgb, SWATCHES,
    Shortfall, Side, Swatch, Theme, ThemeId, ThemeSpec, Token, TokenGroup, pairs,
};
use crate::cmd::{Cmd, Trigger};
use crate::components::{
    Alert, AlertTone, Badge, BadgeVariant, Button, ButtonVariant, Card, Checkbox, Field, Input,
    NativeSelect, Size, Stat, Switch, Textarea, Tone, icon,
};
use crate::form::csrf_field;
use crate::html::Href;
use crate::icons;
use crate::node::{Element, Node, Uses, el};
use crate::shell::{AppPage, ShellLayout};

/// The id of the builder's region: what the page follows in place.
pub const REGION: &str = "theme-builder";

/// The most fields a draft reads from a query.
const MOST_FIELDS: usize = 256;

/// What the accent is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Accent {
    /// The base theme's own.
    Base,
    /// A named swatch.
    Swatch(Swatch),
    /// A colour of the reader's, in both palettes.
    Custom(Rgb),
}

/// A theme being built: the base it starts from and every choice on top, read
/// from the builder's query by a strict parser. Fields that don't parse are
/// kept as typed, with an error, and left out of the theme.
#[derive(Clone, Debug)]
pub struct BuilderDraft {
    base: Theme,
    accent: Accent,
    custom: Option<String>,
    neutral: Option<Neutral>,
    radius: Option<Radius>,
    density: Option<Density>,
    font: Option<Font>,
    layout: ShellLayout,
    mode: Mode,
    overrides: Vec<(Side, Token, Rgb)>,
    typed: Vec<(String, String)>,
    errors: Vec<(String, String)>,
    id: String,
    name: String,
}

impl Default for BuilderDraft {
    fn default() -> Self {
        Self::from_query("")
    }
}

/// The form field of a token's colour in one palette: `d.ink-0`, `l.ink-0`.
fn field_name(side: Side, token: Token) -> String {
    format!(
        "{}.{}",
        match side {
            Side::Dark => "d",
            Side::Light => "l",
        },
        token.name()
    )
}

impl BuilderDraft {
    /// Reads a draft from the builder's query string (or a posted body of the
    /// same fields). Nothing in it can fail: unknown fields are passed over,
    /// values from closed lists that aren't on them are ignored, and a colour
    /// that doesn't parse is kept as typed with an error.
    pub fn from_query(query: &str) -> Self {
        let mut draft = Self {
            base: Theme::NAVREE,
            accent: Accent::Base,
            custom: None,
            neutral: None,
            radius: None,
            density: None,
            font: None,
            layout: ShellLayout::Sidebar,
            mode: Mode::Dark,
            overrides: Vec::new(),
            typed: Vec::new(),
            errors: Vec::new(),
            id: String::new(),
            name: String::new(),
        };
        let mut accent = None;
        for (name, value) in urlencoded::parse(query.as_bytes()).take(MOST_FIELDS) {
            let value = value.trim();
            match &*name {
                "base" => {
                    if let Some(theme) = Theme::builtin(value) {
                        draft.base = theme;
                    }
                }
                "accent" => accent = Some(value.to_owned()),
                "accent_hex" => draft.custom = Some(clip(value, 32)),
                "neutral" => draft.neutral = Neutral::from_id(value),
                "radius" => {
                    draft.radius = Radius::STEPS
                        .iter()
                        .find(|(id, _, _)| *id == value)
                        .map(|(_, _, radius)| *radius);
                }
                "density" => draft.density = Density::from_id(value),
                "font" => draft.font = Font::from_id(value),
                "layout" => {
                    if let Some(layout) = ShellLayout::from_id(value) {
                        draft.layout = layout;
                    }
                }
                "mode" => {
                    if let Some(mode) = Mode::from_id(value) {
                        draft.mode = mode;
                    }
                }
                "id" => draft.id = clip(value, 64),
                "name" => draft.name = clip(value, 128),
                other => {
                    let Some((side, token)) = other.split_once('.').and_then(|(side, token)| {
                        let side = match side {
                            "d" => Side::Dark,
                            "l" => Side::Light,
                            _ => return None,
                        };
                        Some((side, Token::from_name(token)?))
                    }) else {
                        continue;
                    };
                    if value.is_empty() {
                        continue;
                    }
                    let key = field_name(side, token);
                    draft.typed.retain(|(name, _)| *name != key);
                    draft.typed.push((key.clone(), clip(value, 32)));
                    match Rgb::parse(value) {
                        Ok(colour) => {
                            draft
                                .overrides
                                .retain(|(s, t, _)| (*s, *t) != (side, token));
                            draft.overrides.push((side, token, colour));
                        }
                        Err(why) => draft.errors.push((key, why.to_string())),
                    }
                }
            }
        }
        draft.accent = match accent.as_deref() {
            Some("custom") => match draft.custom.as_deref().map(Rgb::parse) {
                Some(Ok(colour)) => Accent::Custom(colour),
                Some(Err(why)) => {
                    draft
                        .errors
                        .push(("accent_hex".to_owned(), why.to_string()));
                    Accent::Base
                }
                None => Accent::Base,
            },
            Some(id) => Swatch::from_id(id).map_or(Accent::Base, Accent::Swatch),
            None => Accent::Base,
        };
        if !draft.id.is_empty()
            && let Err(why) = ThemeId::parse(&draft.id)
        {
            draft.errors.push(("id".to_owned(), why));
        }
        draft
    }

    /// A draft that is `theme` exactly, every colour set: what an imported
    /// theme becomes, to keep working on it.
    pub fn from_theme(theme: &Theme) -> Self {
        let mut draft = Self::from_query("");
        let spec = theme.spec();
        for side in Side::BOTH {
            for (token, colour) in spec.palette(side).iter() {
                draft.overrides.push((side, token, colour));
                draft.typed.push((field_name(side, token), colour.to_hex()));
            }
        }
        draft.radius = Some(spec.radius);
        draft.density = Some(spec.density);
        draft.font = Some(spec.font);
        if !theme.is_builtin() && theme.id() != "custom" {
            draft.id = theme.id().to_owned();
        }
        draft.name = theme.name().to_owned();
        draft
    }

    /// The draft as the builder's query: what a link or a redirect to it carries.
    pub fn to_query(&self) -> String {
        let mut pairs: Vec<(String, String)> = vec![("base".into(), self.base.id().into())];
        match self.accent {
            Accent::Base => {}
            Accent::Swatch(swatch) => pairs.push(("accent".into(), swatch.id.into())),
            Accent::Custom(colour) => {
                pairs.push(("accent".into(), "custom".into()));
                pairs.push(("accent_hex".into(), colour.to_hex()));
            }
        }
        if let Some(neutral) = self.neutral {
            pairs.push(("neutral".into(), neutral.id().into()));
        }
        if let Some(step) = self.radius.and_then(Radius::step) {
            pairs.push(("radius".into(), step.into()));
        }
        if let Some(density) = self.density {
            pairs.push(("density".into(), density.id().into()));
        }
        if let Some(font) = self.font {
            pairs.push(("font".into(), font.id().into()));
        }
        pairs.push(("layout".into(), self.layout.id().into()));
        pairs.push(("mode".into(), self.mode.id().into()));
        for (side, token, colour) in &self.overrides {
            pairs.push((field_name(*side, *token), colour.to_hex()));
        }
        if !self.id.is_empty() {
            pairs.push(("id".into(), self.id.clone()));
        }
        if !self.name.is_empty() {
            pairs.push(("name".into(), self.name.clone()));
        }
        pairs
            .iter()
            .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
            .collect::<Vec<_>>()
            .join("&")
    }

    /// Everything the draft sets: the base, then the greys, the accent, the
    /// corners, density and font, then each colour set by hand.
    pub fn spec(&self) -> ThemeSpec {
        let mut spec = *self.base.spec();
        // The base's own greys are what it has: choosing them again changes nothing.
        if let Some(neutral) = self.neutral.filter(|n| Some(*n) != self.base.neutral()) {
            neutral.apply(&mut spec);
            // The accent's derived colours depend on the greys under them.
            let (dark, light) = (spec.dark.get(Token::Accent), spec.light.get(Token::Accent));
            spec.dark.derive_accent(dark);
            spec.light.derive_accent(light);
        }
        match self.accent {
            Accent::Base => {}
            Accent::Swatch(swatch) => swatch.apply(&mut spec),
            Accent::Custom(colour) => {
                spec.dark.derive_accent(colour);
                spec.light.derive_accent(colour);
            }
        }
        if !matches!(self.accent, Accent::Base) {
            for side in Side::BOTH {
                let palette = spec.palette_mut(side);
                palette.set(Token::Chart1, palette.get(Token::Accent));
            }
        }
        if let Some(radius) = self.radius {
            spec.radius = radius;
        }
        if let Some(density) = self.density {
            spec.density = density;
        }
        if let Some(font) = self.font {
            spec.font = font;
        }
        for (side, token, colour) in &self.overrides {
            spec.palette_mut(*side).set(*token, *colour);
        }
        spec
    }

    /// The theme's id: the one typed, or `my-theme`.
    fn theme_id(&self) -> ThemeId {
        ThemeId::parse(&self.id)
            .or_else(|_| ThemeId::parse("my-theme"))
            .unwrap_or_else(|_| ThemeId::custom())
    }

    /// The theme's name: the one typed, or "My theme".
    fn theme_name(&self) -> String {
        if self.name.trim().is_empty() {
            "My theme".to_owned()
        } else {
            self.name.trim().to_owned()
        }
    }

    /// The draft as a theme, whatever its contrast: what the preview draws. A
    /// draft that changes nothing is its base, which needs no stylesheet of its own.
    pub fn draft(&self) -> Theme {
        let spec = self.spec();
        if spec == *self.base.spec() {
            return self.base.clone();
        }
        Theme::unchecked(self.theme_id(), self.theme_name(), spec)
    }

    /// The pairs that fall short in either palette.
    pub fn shortfalls(&self) -> Vec<Shortfall> {
        self.spec().shortfalls()
    }

    /// The fields that don't parse, by field name, with why.
    pub fn errors(&self) -> &[(String, String)] {
        &self.errors
    }

    /// The finished theme: refused while a field doesn't parse or any text pair
    /// fails WCAG AA. What a save handler stores.
    pub fn to_theme(&self) -> Result<Theme, BuildError> {
        if !self.errors.is_empty() {
            return Err(BuildError::Fields(self.errors.clone()));
        }
        Theme::custom(self.theme_id(), self.theme_name(), self.spec()).map_err(
            |error| match error {
                super::ImportError::Contrast(list) => BuildError::Contrast(list),
                super::ImportError::Fields(list) => BuildError::Fields(list),
            },
        )
    }

    /// The mode the page previews in.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The layout the page previews.
    pub fn layout(&self) -> ShellLayout {
        self.layout
    }

    /// The base theme.
    pub fn base(&self) -> &Theme {
        &self.base
    }
}

/// Why a draft can't be saved or exported.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildError {
    /// Fields that don't parse, by name, with why.
    Fields(Vec<(String, String)>),
    /// Text pairs that fall short of WCAG AA.
    Contrast(Vec<Shortfall>),
}

/// Percent-encodes a query part: everything but unreserved characters.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            _ => {
                out.push('%');
                out.push(char::from(b"0123456789ABCDEF"[usize::from(byte >> 4)]));
                out.push(char::from(b"0123456789ABCDEF"[usize::from(byte & 15)]));
            }
        }
    }
    out
}

/// At most `most` characters of `text`, with no control characters.
fn clip(text: &str, most: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(most)
        .collect()
}

/// The builder: the form, the preview, the contrast report and the export.
///
/// ```ignore
/// async fn builder(shell: AppShell, RawQuery(query): RawQuery) -> AppPage {
///     let draft = BuilderDraft::from_query(query.as_deref().unwrap_or(""));
///     ThemeBuilder::new(&draft, "/settings/theme")
///         .save("/settings/theme/save")
///         .page(shell.page("Theme builder"))
/// }
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ThemeBuilder {
    draft: BuilderDraft,
    /// The draft's values and the theme it draws in, worked out once per render.
    spec: ThemeSpec,
    shown: Theme,
    action: Href,
    save: Option<Href>,
    import: Option<Href>,
    import_text: String,
    import_errors: Vec<String>,
    csrf: Option<String>,
    path: String,
}

impl ThemeBuilder {
    /// The builder for `draft`, on the page at `path`.
    pub fn new(draft: &BuilderDraft, path: &str) -> Self {
        let shown = draft.draft();
        Self {
            draft: draft.clone(),
            spec: *shown.spec(),
            shown,
            action: Href::from(path),
            save: None,
            import: None,
            import_text: String::new(),
            import_errors: Vec::new(),
            csrf: None,
            path: path.to_owned(),
        }
    }

    /// Adds a Save button posting the draft (`draft`, its query) to `href`.
    pub fn save(mut self, href: impl Into<Href>) -> Self {
        self.save = Some(href.into());
        self
    }

    /// Adds an import box posting a stored theme (`json`) to `href`.
    pub fn import(mut self, href: impl Into<Href>) -> Self {
        self.import = Some(href.into());
        self
    }

    /// Shows a refused import again: the text as pasted and why.
    pub fn import_failed(mut self, text: impl Into<String>, errors: Vec<String>) -> Self {
        self.import_text = text.into();
        self.import_errors = errors;
        self
    }

    /// Adds the session's CSRF token to the posts.
    pub fn csrf(mut self, token: impl Into<String>) -> Self {
        self.csrf = Some(token.into());
        self
    }

    /// `page` with the builder in it, drawn in the draft when its text passes
    /// (else in the base theme), in the draft's mode, layout and density.
    pub fn page(self, page: AppPage) -> AppPage {
        let draft = self.shown.clone();
        let passes = draft.spec().text_passes();
        let shown = if passes {
            draft.clone()
        } else {
            self.draft.base.clone()
        };
        page.appearance(Appearance::new(shown, self.draft.mode))
            .layout(self.draft.layout)
            .density(draft.spec().density)
            .theme_sheet(&draft)
            .body(self)
    }

    fn error(&self, field: &str) -> Option<String> {
        self.draft
            .errors
            .iter()
            .find(|(name, _)| name == field)
            .map(|(_, why)| why.clone())
    }

    fn typed(&self, field: &str) -> Option<String> {
        self.draft
            .typed
            .iter()
            .find(|(name, _)| name == field)
            .map(|(_, value)| value.clone())
    }

    /// A radio group of `(value, label)` posting as `name`.
    fn choices<'a>(
        name: &'static str,
        legend: &'static str,
        options: impl IntoIterator<Item = (&'a str, &'a str)>,
        chosen: &str,
        decorate: impl Fn(&str) -> Option<Element>,
    ) -> Element {
        el("fieldset").slot("theme-field").children((
            el("legend").slot("theme-field-legend").child(legend),
            el("div").slot("theme-choices").child(
                options
                    .into_iter()
                    .map(|(value, label)| {
                        el("label").slot("theme-choice").children((
                            el("input")
                                .slot("theme-choice-input")
                                .attr("type", "radio")
                                .attr("name", name)
                                .attr("value", value.to_owned())
                                .flag("checked", value == chosen),
                            decorate(value),
                            el("span").child(label.to_owned()),
                        ))
                    })
                    .collect::<Vec<_>>(),
            ),
        ))
    }

    fn bases(&self) -> Element {
        let cards: Vec<Element> = Theme::BUILT_IN
            .iter()
            .map(|theme| {
                let href = format!(
                    "{}?base={}&layout={}&mode={}",
                    self.path,
                    theme.id(),
                    self.draft.layout.id(),
                    self.draft.mode.id()
                );
                el("a")
                    .slot("theme-base")
                    .href(href)
                    .attr_opt(
                        "aria-current",
                        (theme.key() == self.draft.base.key()).then_some("true"),
                    )
                    .children((
                        el("span")
                            .slot("theme-swatches")
                            .attr("data-theme", theme.key().to_owned())
                            .attr("data-mode", self.draft.mode.id())
                            .attr("aria-hidden", "true")
                            .children((
                                el("span").slot("theme-swatch").attr("data-token", "ink-1"),
                                el("span").slot("theme-swatch").attr("data-token", "accent"),
                                el("span").slot("theme-swatch").attr("data-token", "fg"),
                            )),
                        el("span").slot("theme-card-name").child(theme.name()),
                    ))
            })
            .collect();
        el("nav")
            .slot("theme-bases")
            .attr("aria-label", "Start from")
            .children((
                el("h2").slot("theme-section-title").child("Start from"),
                el("div").slot("theme-base-list").child(cards),
            ))
    }

    fn form(&self) -> Element {
        let draft = &self.draft;
        let spec = self.spec;
        let key = self.shown.key();
        let accent_value = match draft.accent {
            Accent::Base => "base".to_owned(),
            Accent::Swatch(swatch) => swatch.id.to_owned(),
            Accent::Custom(_) => "custom".to_owned(),
        };
        let custom_value = draft
            .custom
            .clone()
            .unwrap_or_else(|| spec.light.get(Token::Accent).to_hex());
        let custom_valid = Rgb::parse(&custom_value)
            .map(Rgb::to_hex)
            .unwrap_or_else(|_| spec.light.get(Token::Accent).to_hex());
        let accents = el("fieldset").slot("theme-field").children((
            el("legend").slot("theme-field-legend").child("Accent"),
            el("div").slot("theme-choices").attr("data-variant", "swatches").child(
                std::iter::once(("base", "Base"))
                    .chain(SWATCHES.iter().map(|s| (s.id, s.label)))
                    .chain(std::iter::once(("custom", "Custom")))
                    .map(|(value, label)| {
                        el("label").slot("theme-choice").children((
                            el("input")
                                .slot("theme-choice-input")
                                .attr("type", "radio")
                                .attr("name", "accent")
                                .attr("value", value)
                                .flag("checked", value == accent_value),
                            el("span")
                                .slot("theme-dot")
                                .attr("data-swatch", value)
                                .attr("aria-hidden", "true"),
                            el("span").child(label),
                        ))
                    })
                    .collect::<Vec<_>>(),
            ),
            Field::new("theme-accent-hex", "Custom accent")
                .help("Any colour: the text on it, the accent as text and the focus ring are worked out to read.")
                .error_opt(self.error("accent_hex"))
                .control(
                    el("input")
                        .slot("theme-colour")
                        .attr("type", "color")
                        .attr("name", "accent_hex")
                        .attr("value", custom_valid),
                ),
        ));
        let neutral_value = draft.neutral.or_else(|| draft.base.neutral());
        let neutrals = Self::choices(
            "neutral",
            "Greys",
            Neutral::ALL.iter().map(|n| (n.id(), n.label())),
            neutral_value.map_or("", Neutral::id),
            |id| {
                Some(
                    el("span")
                        .slot("theme-dot")
                        .attr("data-neutral", id.to_owned())
                        .attr("aria-hidden", "true"),
                )
            },
        );
        let radius = Self::choices(
            "radius",
            "Corners",
            Radius::STEPS.iter().map(|(id, label, _)| (*id, *label)),
            spec.radius.step().unwrap_or(""),
            |id| {
                Some(
                    el("span")
                        .slot("theme-corner")
                        .attr("data-radius", id.to_owned())
                        .attr("aria-hidden", "true"),
                )
            },
        );
        let density = Self::choices(
            "density",
            "Density",
            Density::ALL.iter().map(|d| (d.id(), d.label())),
            spec.density.id(),
            |_| None,
        );
        let font = Self::choices(
            "font",
            "Font",
            Font::ALL.iter().map(|f| (f.id(), f.label())),
            spec.font.id(),
            |id| {
                Some(
                    el("span")
                        .slot("theme-font")
                        .attr("data-font", id.to_owned())
                        .attr("aria-hidden", "true")
                        .child("Ag"),
                )
            },
        );
        let layout = Self::choices(
            "layout",
            "Layout",
            ShellLayout::ALL.iter().map(|l| (l.id(), l.label())),
            draft.layout.id(),
            |_| None,
        );
        let mode = Self::choices(
            "mode",
            "Preview in",
            Mode::ALL.iter().map(|m| (m.id(), m.label())),
            draft.mode.id(),
            |_| None,
        );
        let mut groups: Vec<Element> = Vec::new();
        for group in [
            TokenGroup::Surfaces,
            TokenGroup::Text,
            TokenGroup::Accent,
            TokenGroup::Tones,
            TokenGroup::Charts,
        ] {
            let rows: Vec<Element> = Token::ALL
                .into_iter()
                .filter(|token| token.group() == group)
                .map(|token| {
                    let cells: Vec<Element> = [Side::Light, Side::Dark]
                        .into_iter()
                        .map(|side| {
                            let field = field_name(side, token);
                            let derived = spec.palette(side).get(token).to_hex();
                            let error = self.error(&field);
                            let id = format!("theme-{}", field.replace('.', "-"));
                            el("td").children((
                                el("span")
                                    .slot("theme-cell-swatch")
                                    .attr("data-theme", key.to_owned())
                                    .attr("data-mode", side.as_str())
                                    .attr("aria-hidden", "true")
                                    .child(
                                        el("span")
                                            .slot("theme-swatch")
                                            .attr("data-token", token.name()),
                                    ),
                                {
                                    let input = Input::text(field.clone())
                                        .value(self.typed(&field).unwrap_or_default())
                                        .placeholder(derived)
                                        .invalid(error.is_some())
                                        .attr("id", id.clone())
                                        .attr(
                                            "aria-label",
                                            format!("{} in {}", token.label(), side.as_str()),
                                        )
                                        .attr("spellcheck", "false")
                                        .attr("autocomplete", "off");
                                    if error.is_some() {
                                        input.attr("aria-describedby", format!("{id}-error"))
                                    } else {
                                        input
                                    }
                                },
                                error.map(|why| {
                                    el("p")
                                        .slot("field-error")
                                        .attr("id", format!("{id}-error"))
                                        .child(why)
                                }),
                            ))
                        })
                        .collect();
                    el("tr").children((
                        el("th").attr("scope", "row").children((
                            el("span").slot("theme-token-label").child(token.label()),
                            el("code")
                                .slot("theme-token-name")
                                .child(format!("--{}", token.name())),
                        )),
                        cells,
                    ))
                })
                .collect();
            groups.push(el("table").slot("theme-token-table").children((
                el("caption").child(group.label()),
                el("thead").child(el("tr").children((
                    el("th").attr("scope", "col").child("Token"),
                    el("th").attr("scope", "col").child("Light"),
                    el("th").attr("scope", "col").child("Dark"),
                ))),
                el("tbody").child(rows),
            )));
        }
        let has_override_error = draft.errors.iter().any(|(name, _)| name.contains('.'));
        let tokens = el("details")
            .slot("theme-tokens")
            .flag("open", has_override_error || !draft.overrides.is_empty())
            .children((
                el("summary").slot("theme-tokens-summary").child("Every colour, light and dark"),
                el("p")
                    .slot("theme-help")
                    .child("Leave a box empty to keep the colour shown. A colour is #rgb, #rrggbb or rgb(r, g, b)."),
                groups,
            ));
        let naming = el("div").slot("theme-naming").children((
            Field::new("theme-name", "Name")
                .help("Shown in the theme menu.")
                .control(
                    Input::text("name")
                        .value(draft.name.clone())
                        .placeholder("My theme")
                        .attr("autocomplete", "off"),
                ),
            Field::new("theme-id", "Id")
                .help("Lower-case letters, digits and hyphens: the theme's name in code.")
                .error_opt(self.error("id"))
                .control(
                    Input::text("id")
                        .value(draft.id.clone())
                        .placeholder("my-theme")
                        .attr("autocomplete", "off")
                        .attr("spellcheck", "false"),
                ),
        ));
        el("form")
            .slot("theme-form")
            .attr("id", "theme-builder-form")
            .attr("method", "get")
            .attr("action", self.action.clone())
            .attr("data-ui-target", REGION)
            .attr("data-ui-debounce", "150")
            .attr("aria-label", "Theme")
            .uses(Uses::CORE)
            .children((
                el("input")
                    .attr("type", "hidden")
                    .attr("name", "base")
                    .attr("value", draft.base.id().to_owned()),
                accents,
                neutrals,
                radius,
                density,
                font,
                layout,
                mode,
                tokens,
                naming,
                Button::new("Preview")
                    .variant(ButtonVariant::Outline)
                    .submit()
                    .attr("data-theme-apply", ""),
            ))
    }

    /// Real components, drawn in the draft in one palette.
    fn sample(key: &str, side: Side) -> Element {
        let suffix = side.as_str();
        el("section")
            .slot("theme-sample")
            .attr("data-theme", key.to_owned())
            .attr("data-mode", suffix)
            .attr("aria-label", format!("Preview, {suffix}"))
            .children((
                el("p").slot("theme-sample-title").child(match side {
                    Side::Light => "Light",
                    Side::Dark => "Dark",
                }),
                el("div").slot("theme-sample-row").children((
                    Button::primary("Save deal"),
                    Button::new("Cancel"),
                    Button::new("Export").variant(ButtonVariant::Outline),
                    Button::new("More").variant(ButtonVariant::Ghost),
                    Button::new("Delete").variant(ButtonVariant::Destructive),
                )),
                el("div").slot("theme-sample-row").children((
                    Badge::new("Won").variant(BadgeVariant::Success),
                    Badge::new("At risk").variant(BadgeVariant::Warning),
                    Badge::new("Lost").variant(BadgeVariant::Danger),
                    Badge::new("New").tone(Tone::Info),
                    Badge::new("Partner").tone(Tone::named("violet")),
                    Badge::new("Trial").tone(Tone::named("cyan")),
                    Badge::new("VIP").tone(Tone::named("rose")),
                )),
                Card::titled("Pipeline").description("This quarter").body((
                    el("div").slot("theme-sample-stats").children((
                        Stat::new("Won", "$84,200").trend("+12%", true),
                        Stat::new("Lost", "$9,800").trend("-3%", false),
                    )),
                    el("div")
                        .slot("theme-sample-chart")
                        .attr("role", "img")
                        .attr("aria-label", "Five series in the chart colours")
                        .child(
                            ["chart-1", "chart-2", "chart-3", "chart-4", "chart-5", "bar"]
                                .into_iter()
                                .map(|token| {
                                    el("span")
                                        .slot("theme-sample-bar")
                                        .attr("data-token", token)
                                })
                                .collect::<Vec<_>>(),
                        ),
                )),
                el("div").slot("theme-sample-form").children((
                    Field::new(format!("theme-sample-email-{suffix}"), "Email")
                        .help("We never share it.")
                        .control(Input::email("email").placeholder("ada@example.com")),
                    Field::new(format!("theme-sample-stage-{suffix}"), "Stage").control(
                        NativeSelect::new("stage")
                            .option("lead", "Lead")
                            .option("won", "Won")
                            .selected("won"),
                    ),
                    el("div").slot("theme-sample-row").children((
                        Checkbox::new("notify")
                            .checked(true)
                            .label("Notify the owner"),
                        Switch::new("public").checked(true).label("Public"),
                    )),
                )),
                Alert::new(AlertTone::Success).title("Deal saved"),
                Alert::new(AlertTone::Danger).title("Payment failed"),
                el("p").slot("theme-sample-text").children((
                    el("span").attr("data-fg", "fg").child("Headings, "),
                    el("span").attr("data-fg", "fg-1").child("body text, "),
                    el("span").attr("data-fg", "fg-2").child("secondary text, "),
                    el("span").attr("data-fg", "fg-3").child("hints "),
                    el("span")
                        .attr("data-fg", "accent-text")
                        .child("and the accent as text."),
                )),
                el("span").slot("theme-sample-focus").child("Focus ring"),
            ))
    }

    fn preview(&self) -> Element {
        let key = self.shown.key().to_owned();
        el("div").slot("theme-preview").children((
            el("h2").slot("theme-section-title").child("Preview"),
            el("p").slot("theme-help").attr("data-state", "note").child(
                "A picture of the draft: real components drawn in it, light and dark. The contrast report below says in words what it shows.",
            ),
            // A picture, not controls to use: out of the focus order and the
            // accessibility tree, so a failing draft's colours are shown, not read.
            el("div")
                .slot("theme-preview-pair")
                .flag("inert", true)
                .children((
                    Self::sample(&key, Side::Light),
                    Self::sample(&key, Side::Dark),
                )),
        ))
    }

    fn report(&self) -> Element {
        let draft = &self.shown;
        let spec = self.spec;
        let shortfalls = spec.shortfalls();
        let failing_text = shortfalls
            .iter()
            .filter(|s| s.pair.kind == PairKind::Text)
            .count();
        let warnings = shortfalls.len() - failing_text;
        let summary = if shortfalls.is_empty() {
            "Every pair passes: text at 4.5:1 or more, the focus ring and charts at 3:1 or more."
                .to_owned()
        } else if failing_text == 0 {
            format!(
                "All text passes. {warnings} {} under 3:1.",
                if warnings == 1 {
                    "graphic is"
                } else {
                    "graphics are"
                }
            )
        } else {
            format!(
                "{failing_text} text {} WCAG AA: the theme can't be saved or exported until {} fixed.",
                if failing_text == 1 {
                    "pair fails"
                } else {
                    "pairs fail"
                },
                if failing_text == 1 {
                    "it is"
                } else {
                    "they are"
                }
            )
        };
        let row = |side: Side, pair: &Pair| -> Element {
            let palette = spec.palette(side);
            let ratio = pair.ratio(palette, side);
            let ok = ratio >= pair.min;
            el("tr")
                .attr_opt(
                    "data-fails",
                    (!ok).then_some(match pair.kind {
                        PairKind::Text => "text",
                        PairKind::Graphic => "graphic",
                    }),
                )
                .children((
                    el("td")
                        .flag("inert", true)
                        .child(sample(draft.key(), side, pair)),
                    el("td").children((
                        el("code").child(pair.text.describe()),
                        " on ",
                        el("code").child(pair.on.describe()),
                    )),
                    el("td")
                        .slot("theme-contrast-ratio")
                        .child(format!("{ratio:.2}:1")),
                    el("td").child(if ok {
                        Badge::new("Passes").variant(BadgeVariant::Success)
                    } else if pair.kind == PairKind::Text {
                        Badge::new(format!("Needs {:.1}", pair.min)).variant(BadgeVariant::Danger)
                    } else {
                        Badge::new(format!("Under {:.0}:1", pair.min))
                            .variant(BadgeVariant::Warning)
                    }),
                ))
        };
        let head = || {
            el("thead").child(
                el("tr").children((
                    el("th")
                        .attr("scope", "col")
                        .child(el("span").slot("theme-sr").child("Sample")),
                    el("th").attr("scope", "col").child("Pair"),
                    el("th").attr("scope", "col").child("Ratio"),
                    el("th")
                        .attr("scope", "col")
                        .child(el("span").slot("theme-sr").child("Result")),
                )),
            )
        };
        let tables: Vec<Element> = Side::BOTH
            .into_iter()
            .map(|side| {
                let name = match side {
                    Side::Light => "Light",
                    Side::Dark => "Dark",
                };
                let failing: Vec<Element> = shortfalls
                    .iter()
                    .filter(|shortfall| shortfall.side == side)
                    .map(|shortfall| row(side, &shortfall.pair))
                    .collect();
                let all: Vec<Element> = pairs().iter().map(|pair| row(side, pair)).collect();
                el("div").slot("theme-contrast-side").children((
                    (!failing.is_empty()).then(|| {
                        el("table").slot("theme-contrast-table").children((
                            el("caption").child(format!("{name}: what falls short")),
                            head(),
                            el("tbody").child(failing),
                        ))
                    }),
                    el("details").slot("theme-contrast-all").children((
                        el("summary").child(format!("{name}: all {} pairs", pairs().len())),
                        el("table").slot("theme-contrast-table").children((
                            el("caption").child(format!("{name}: every pair")),
                            head(),
                            el("tbody").child(all),
                        )),
                    )),
                ))
            })
            .collect();
        el("section")
            .slot("theme-contrast")
            .attr("aria-labelledby", "theme-contrast-title")
            .children((
                el("h2")
                    .slot("theme-section-title")
                    .attr("id", "theme-contrast-title")
                    .child("Contrast"),
                el("p")
                    .slot("theme-contrast-summary")
                    .attr("role", "status")
                    .attr(
                        "data-state",
                        if failing_text > 0 {
                            "fails"
                        } else if warnings > 0 {
                            "warns"
                        } else {
                            "passes"
                        },
                    )
                    .children((
                        icon(if failing_text > 0 {
                            icons::CIRCLE_X
                        } else if warnings > 0 {
                            icons::TRIANGLE_ALERT
                        } else {
                            icons::CIRCLE_CHECK
                        }),
                        el("span").child(summary),
                    )),
                el("div").slot("theme-contrast-tables").child(tables),
            ))
    }

    fn export(&self) -> Element {
        let built = self.draft.to_theme();
        let body: Node = match &built {
            Ok(theme) => {
                let rust = theme.to_rust("navree::ui");
                let json = theme.to_json();
                let block = |title: &'static str, id: &'static str, text: String, glyph| {
                    el("div").slot("theme-export-block").children((
                        el("div").slot("theme-export-head").children((
                            el("h3")
                                .slot("theme-export-title")
                                .attr("id", id)
                                .child(title),
                            Button::new(Node::list((icon(icons::COPY), el("span").child("Copy"))))
                                .variant(ButtonVariant::Outline)
                                .size(Size::Sm)
                                .attr("aria-describedby", id)
                                .on(Trigger::Click, Cmd::copy(text.clone())),
                        )),
                        el("pre")
                            .slot("code")
                            .attr("data-variant", "export")
                            .attr("tabindex", 0_i64)
                            .attr("aria-labelledby", id)
                            .child(el("code").child(text)),
                        glyph,
                    ))
                };
                Node::list((
                    block("Rust", "theme-export-rust", rust, Option::<Node>::None),
                    block("Stored theme (JSON)", "theme-export-json", json, None),
                    self.save.clone().map(|href| {
                        el("form")
                            .slot("theme-save")
                            .attr("method", "post")
                            .attr("action", href)
                            .children((
                                self.csrf.clone().map(csrf_field),
                                el("input")
                                    .attr("type", "hidden")
                                    .attr("name", "draft")
                                    .attr("value", self.draft.to_query()),
                                Button::primary(Node::list((
                                    icon(icons::SAVE),
                                    el("span").child("Save theme"),
                                )))
                                .submit(),
                            ))
                    }),
                ))
            }
            Err(BuildError::Contrast(_)) => el("p")
                .slot("theme-help")
                .attr("data-state", "blocked")
                .child(
                    "Fix the text pairs that fail, under Contrast, to save or export this theme.",
                )
                .into(),
            Err(BuildError::Fields(_)) => el("p")
                .slot("theme-help")
                .attr("data-state", "blocked")
                .child("Fix the fields marked in the form to save or export this theme.")
                .into(),
        };
        let import = self.import.clone().map(|href| {
            el("form")
                .slot("theme-import")
                .attr("method", "post")
                .attr("action", href)
                .children((
                    self.csrf.clone().map(csrf_field),
                    Field::new("theme-import-json", "Import a stored theme")
                        .help("Paste the JSON of a theme saved here or exported from another app.")
                        .error_opt(
                            (!self.import_errors.is_empty()).then(|| self.import_errors.join(" ")),
                        )
                        .control(
                            Textarea::new("json")
                                .rows(4)
                                .value(self.import_text.clone()),
                        ),
                    Button::new(Node::list((
                        icon(icons::IMPORT),
                        el("span").child("Import"),
                    )))
                    .variant(ButtonVariant::Outline)
                    .submit(),
                ))
        });
        el("section")
            .slot("theme-export")
            .attr("aria-labelledby", "theme-export-title")
            .children((
                el("h2")
                    .slot("theme-section-title")
                    .attr("id", "theme-export-title")
                    .child("Export"),
                body,
                import,
            ))
    }
}

/// The swatch of a pair, drawn in the draft with no style attribute: the text in
/// its main token, on its surface, with a tint or mix laid on through the
/// builder stylesheet's `data-tint` and `data-pct` rules.
fn sample(key: &str, side: Side, pair: &Pair) -> Element {
    let main = |paint: Paint| match paint {
        Paint::Token(token) | Paint::Mix(token, _, _) | Paint::Hover(token, _) => token,
        Paint::Tint(token, _, _) => token,
    };
    let swatch = el("span")
        .slot("theme-contrast-sample")
        .attr("data-theme", key.to_owned())
        .attr("data-mode", side.as_str())
        .attr("data-fg", main(pair.text).name())
        .attr("aria-hidden", "true");
    let swatch = match pair.on {
        Paint::Token(token) | Paint::Hover(token, _) => swatch.attr("data-token", token.name()),
        Paint::Tint(tint, percent, under) | Paint::Mix(tint, percent, under) => swatch
            .attr("data-token", under.name())
            .attr("data-tint", tint.name())
            .attr("data-pct", percent.to_string()),
    };
    swatch.child("Aa")
}

impl From<ThemeBuilder> for Node {
    fn from(builder: ThemeBuilder) -> Self {
        el("div")
            .slot("theme-builder")
            .attr("id", REGION)
            .uses(Uses::THEME_BUILDER)
            .children((
                builder.bases(),
                el("div").slot("theme-builder-grid").children((
                    builder.form(),
                    el("div").slot("theme-builder-side").children((
                        builder.preview(),
                        builder.report(),
                        builder.export(),
                    )),
                )),
            ))
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    /// The proof app's own theme, built here and exported as Rust: the app
    /// compiles the export as it comes.
    const EXAMPLE: &str = "accent=custom&accent_hex=%237c5cff&neutral=zinc&radius=round&font=system&id=acme&name=Acme";

    fn example() -> (std::path::PathBuf, String) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/ui-proof/src/acme_theme.rs");
        let theme = BuilderDraft::from_query(EXAMPLE).to_theme().unwrap();
        let text = format!(
            "//! The \"Acme\" theme: the theme builder's Rust export, used as it comes. A\n//! test in navree-ui writes it and keeps it current.\n\n{}",
            theme.to_rust("navree::ui")
        );
        (path, text)
    }

    #[test]
    fn the_proof_apps_exported_theme_is_current() {
        let (path, text) = example();
        let committed = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert_eq!(
            committed,
            text,
            "{} is stale: cargo test -p navree-ui --lib theme::builder::tests::write_the_proof_apps_exported_theme -- --ignored",
            path.display()
        );
    }

    /// Not a check: writes the file the test above compares.
    #[test]
    #[ignore = "writes a file"]
    fn write_the_proof_apps_exported_theme() {
        let (path, text) = example();
        std::fs::write(&path, text).unwrap();
    }

    /// What the builder's form posts as it is first drawn for `base`: every choice
    /// as shown, nothing changed.
    fn untouched(base: &Theme) -> String {
        let html = render(
            &ThemeBuilder::new(
                &BuilderDraft::from_query(&format!("base={}", base.id())),
                "/b",
            )
            .into(),
        );
        let mut fields = vec![format!("base={}", base.id())];
        for chunk in html.split("<input ").skip(1) {
            let attr = |name: &str| {
                chunk
                    .split_once(&format!("{name}=\""))
                    .and_then(|(_, rest)| rest.split_once('"'))
                    .map(|(value, _)| value.to_owned())
            };
            let kind = attr("type").unwrap_or_default();
            let checked = chunk
                .split('>')
                .next()
                .unwrap_or_default()
                .contains(" checked");
            if let (Some(name), Some(value)) = (attr("name"), attr("value"))
                && ((kind == "radio" && checked)
                    || kind == "color"
                    || (kind == "text" && !value.is_empty()))
            {
                fields.push(format!("{name}={}", encode(&value)));
            }
        }
        fields.join("&")
    }

    #[test]
    fn the_form_as_first_drawn_is_its_base_exactly() {
        for base in Theme::BUILT_IN {
            let query = untouched(&base);
            let draft = BuilderDraft::from_query(&query);
            assert_eq!(draft.spec(), *base.spec(), "{}: {query}", base.id());
            assert_eq!(
                draft.draft().key(),
                base.key(),
                "{}: no stylesheet of its own",
                base.id()
            );
        }
    }

    #[test]
    fn a_draft_reads_the_query_strictly() {
        let draft = BuilderDraft::from_query(
            "base=ocean&accent=custom&accent_hex=%23ff8800&neutral=stone&radius=round&density=compact&font=serif&layout=inset&mode=light&d.fg-3=%23aabbcc&id=acme&name=Acme",
        );
        let spec = draft.spec();
        assert_eq!(spec.light.get(Token::Accent), Rgb::hex(0xff8800));
        assert_eq!(spec.dark.get(Token::Fg3), Rgb::hex(0xaabbcc));
        assert_eq!(spec.radius, Radius::ROUND);
        assert_eq!(spec.density, Density::Compact);
        assert_eq!(spec.font, Font::Serif);
        assert_eq!(draft.layout(), ShellLayout::Inset);
        assert_eq!(draft.mode(), Mode::Light);
        assert!(draft.errors().is_empty(), "{:?}", draft.errors());
        let again = BuilderDraft::from_query(&draft.to_query());
        assert_eq!(again.spec(), spec);
    }

    #[test]
    fn hostile_values_are_errors_or_nothing() {
        let draft = BuilderDraft::from_query(
            "base=%3Cscript%3E&accent=custom&accent_hex=red%3B%7Dbody%7Bdisplay%3Anone&neutral=x&radius=999px&font=Comic%20Sans&layout=%22&mode=dim&d.ink-0=url(javascript:alert(1))&l.fg=%23fff%3B%7D&d.--x=%23fff&z.ink-0=%23fff&id=%3Cb%3E",
        );
        assert_eq!(draft.base(), &Theme::NAVREE);
        let fields: Vec<&str> = draft
            .errors()
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        for expected in ["accent_hex", "d.ink-0", "l.fg", "id"] {
            assert!(fields.contains(&expected), "{expected} in {fields:?}");
        }
        assert_eq!(
            draft.spec(),
            *Theme::NAVREE.spec(),
            "nothing hostile reached the theme"
        );
        assert!(matches!(draft.to_theme(), Err(BuildError::Fields(_))));
        let html = render(&ThemeBuilder::new(&draft, "/b").into());
        assert!(!html.contains("<script"), "{html:.200}");
        assert!(!html.contains("<b>"), "{html:.200}");
        assert!(
            html.contains("url(javascript:alert(1))"),
            "kept as typed, escaped, in its box"
        );
        assert!(!html.contains("style="), "no style attribute anywhere");
    }

    #[test]
    fn a_failing_draft_can_be_previewed_but_not_saved() {
        let draft = BuilderDraft::from_query("l.fg-3=%23dddddd");
        assert!(matches!(draft.to_theme(), Err(BuildError::Contrast(_))));
        let html = render(&ThemeBuilder::new(&draft, "/b").save("/b/save").into());
        assert!(
            html.contains("can&#39;t be saved") || html.contains("can't be saved"),
            "{html:.100}"
        );
        assert!(
            !html.contains("Save theme"),
            "no save button while text fails"
        );
        assert!(html.contains(r#"data-fails="text""#));
    }

    #[test]
    fn a_grey_page_with_white_cards_is_refused_for_what_the_page_draws() {
        // The page's own text, a field's error and the destructive button all sit
        // on the page, not on a card: a grey page with a bright danger fails there.
        let draft = BuilderDraft::from_query(
            "base=navree&mode=light&l.ink-0=%23eceeed&l.ink-1=%23ffffff&l.danger=%23d92d20&id=grey",
        );
        let Err(BuildError::Contrast(list)) = draft.to_theme() else {
            panic!("the gate passed a draft whose danger text fails on the page");
        };
        let failing: Vec<String> = list
            .iter()
            .map(|s| format!("{} on {}", s.pair.text.describe(), s.pair.on.describe()))
            .collect();
        assert!(
            failing.iter().any(|f| f == "danger on ink-0"),
            "{failing:?}"
        );
        assert!(
            failing.iter().any(|f| f.starts_with("danger on danger ")),
            "the destructive button's own tint: {failing:?}"
        );
    }

    #[test]
    fn a_passing_draft_exports_rust_and_json_and_posts_its_query() {
        let draft = BuilderDraft::from_query("base=violet&id=acme&name=Acme");
        let html = render(
            &ThemeBuilder::new(&draft, "/b")
                .save("/b/save")
                .csrf("t")
                .into(),
        );
        assert!(
            html.contains("pub fn acme_theme() -&gt; Theme"),
            "{html:.100}"
        );
        assert!(html.contains("navree-theme/1"));
        assert!(
            html.contains(r#"name="draft" value="base=violet"#),
            "{html:.100}"
        );
        assert!(html.contains("Save theme"));
        let saved = draft.to_theme().unwrap();
        assert_eq!(saved.spec(), Theme::VIOLET.spec());
        assert_eq!(saved.id(), "acme");
    }

    #[test]
    fn an_imported_theme_becomes_a_draft_of_itself() {
        let theme = Theme::custom(
            ThemeId::parse("acme").unwrap(),
            "Acme",
            *Theme::SUNSET.spec(),
        )
        .unwrap();
        let draft = BuilderDraft::from_theme(&theme);
        assert_eq!(draft.spec(), *theme.spec());
        let back = BuilderDraft::from_query(&draft.to_query());
        assert_eq!(back.spec(), *theme.spec());
        assert_eq!(back.to_theme().unwrap().id(), "acme");
    }
}
