//! A theme written down three ways, each read back by a strict parser:
//!
//! - the **short form**, `v1-0406-d-g-<dark>-<light>`, only `[0-9a-z-]`: the
//!   theme's stylesheet URL and what a cookie carries;
//! - the **stored theme**, JSON with a schema name, for a database column or a
//!   file to share;
//! - **Rust**, a function returning the [`Theme`], to paste into an app.

use std::fmt::Write as _;

use serde_json::{Map, Value};

use super::{
    Density, Font, LengthError, Palette, Radius, Rgb, Shortfall, Side, TOKEN_COUNT, Theme,
    ThemeSpec, Token,
};

/// A theme's id: `[a-z][a-z0-9-]*`, at most 32 characters, no `--` and no
/// trailing `-`. The built-in themes' ids are taken.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ThemeId(String);

impl ThemeId {
    /// The longest id.
    pub const MAX: usize = 32;

    /// Reads an id.
    pub fn parse(text: &str) -> Result<Self, String> {
        let bytes = text.as_bytes();
        let shaped = !bytes.is_empty()
            && bytes.len() <= Self::MAX
            && bytes[0].is_ascii_lowercase()
            && bytes
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
            && !text.ends_with('-')
            && !text.contains("--");
        if !shaped {
            return Err(
                "Use lower-case letters, digits and single hyphens, starting with a letter, up to 32 characters."
                    .to_owned(),
            );
        }
        if text == "custom" || Theme::builtin(text).is_some() {
            return Err("That id belongs to a built-in theme.".to_owned());
        }
        Ok(Self(text.to_owned()))
    }

    /// The id of a theme read from its values alone.
    pub(crate) fn custom() -> Self {
        Self("custom".to_owned())
    }

    /// The id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ThemeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a stored theme was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum ImportError {
    /// The text is not a stored theme: each entry is where and what.
    Fields(Vec<(String, String)>),
    /// It reads, but text fails WCAG AA in these pairs.
    Contrast(Vec<Shortfall>),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fields(fields) => {
                let mut first = true;
                for (at, what) in fields {
                    if !first {
                        f.write_str("; ")?;
                    }
                    first = false;
                    write!(f, "{at}: {what}")?;
                }
                Ok(())
            }
            Self::Contrast(list) => {
                write!(f, "{} pairs fall short of WCAG AA", list.len())
            }
        }
    }
}

impl std::error::Error for ImportError {}

/// The schema a stored theme names.
pub const SCHEMA: &str = "navree-theme/1";

/// The most text [`Theme::from_json`] reads.
pub const JSON_MOST: usize = 16 * 1024;

const PALETTE_DIGITS: usize = TOKEN_COUNT * 6;

/// The length of every short form.
pub(crate) const FORM_LEN: usize =
    2 + 1 + 4 + 1 + 1 + 1 + 1 + 1 + PALETTE_DIGITS + 1 + PALETTE_DIGITS;

const fn density_char(density: Density) -> char {
    match density {
        Density::Compact => 'c',
        Density::Default => 'd',
        Density::Comfortable => 'l',
    }
}

const fn font_char(font: Font) -> char {
    match font {
        Font::Geist => 'g',
        Font::System => 's',
        Font::Humanist => 'h',
        Font::Geometric => 'm',
        Font::Serif => 'r',
    }
}

/// The short form of `spec`.
pub(crate) fn to_form(spec: &ThemeSpec) -> String {
    let mut out = String::with_capacity(FORM_LEN);
    let _ = write!(
        out,
        "v1-{:02}{:02}-{}-{}-",
        spec.radius.control(),
        spec.radius.surface(),
        density_char(spec.density),
        font_char(spec.font)
    );
    for (_, colour) in spec.dark.iter() {
        colour.write_digits(&mut out);
    }
    out.push('-');
    for (_, colour) in spec.light.iter() {
        colour.write_digits(&mut out);
    }
    out
}

/// The spec a short form holds: `None` for anything that isn't exactly one,
/// in its one canonical spelling.
pub(crate) fn from_form(form: &str) -> Option<ThemeSpec> {
    if form.len() != FORM_LEN || !form.is_ascii() {
        return None;
    }
    let mut parts = form.split('-');
    let (version, corners, density, font, dark, light) = (
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
        parts.next()?,
    );
    if parts.next().is_some() || version != "v1" || corners.len() != 4 {
        return None;
    }
    if !corners.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let radius = Radius::new(corners[..2].parse().ok()?, corners[2..].parse().ok()?).ok()?;
    let density = Density::ALL
        .into_iter()
        .find(|d| density.len() == 1 && density.starts_with(density_char(*d)))?;
    let font = Font::ALL
        .into_iter()
        .find(|f| font.len() == 1 && font.starts_with(font_char(*f)))?;
    Some(ThemeSpec {
        dark: palette_from_digits(dark)?,
        light: palette_from_digits(light)?,
        radius,
        density,
        font,
    })
}

fn palette_from_digits(digits: &str) -> Option<Palette> {
    if digits.len() != PALETTE_DIGITS
        || !digits
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut colours = [Rgb::BLACK; TOKEN_COUNT];
    for (at, colour) in colours.iter_mut().enumerate() {
        let value = u32::from_str_radix(digits.get(at * 6..at * 6 + 6)?, 16).ok()?;
        *colour = Rgb::hex(value);
    }
    Some(Palette::from_array(colours))
}

impl Theme {
    /// The stored theme: JSON naming its schema, with every colour of both
    /// palettes. [`Theme::from_json`] reads it back.
    pub fn to_json(&self) -> String {
        let spec = self.spec();
        let palette = |palette: &Palette| {
            let mut map = Map::new();
            for (token, colour) in palette.iter() {
                map.insert(token.name().to_owned(), Value::String(colour.to_hex()));
            }
            Value::Object(map)
        };
        let mut radius = Map::new();
        radius.insert(
            "control".to_owned(),
            Value::String(format!("{}px", spec.radius.control())),
        );
        radius.insert(
            "surface".to_owned(),
            Value::String(format!("{}px", spec.radius.surface())),
        );
        let mut root = Map::new();
        root.insert("schema".to_owned(), Value::String(SCHEMA.to_owned()));
        root.insert("id".to_owned(), Value::String(self.id().to_owned()));
        root.insert("name".to_owned(), Value::String(self.name().to_owned()));
        root.insert("radius".to_owned(), Value::Object(radius));
        root.insert(
            "density".to_owned(),
            Value::String(spec.density.id().to_owned()),
        );
        root.insert("font".to_owned(), Value::String(spec.font.id().to_owned()));
        root.insert("dark".to_owned(), palette(&spec.dark));
        root.insert("light".to_owned(), palette(&spec.light));
        serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default()
    }

    /// Reads a stored theme. Every field is checked: the schema, an id, a name,
    /// the corners in px, a density and a font from the lists, and a colour for
    /// every token of both palettes, each through [`Rgb::parse`]. Unknown fields
    /// are refused, so a misspelt one isn't lost. Then the contrast is checked.
    pub fn from_json(text: &str) -> Result<Self, ImportError> {
        if text.len() > JSON_MOST {
            return Err(fields("theme", "This file is larger than a theme."));
        }
        let value: Value =
            serde_json::from_str(text).map_err(|_| fields("theme", "This is not JSON."))?;
        let Value::Object(root) = value else {
            return Err(fields("theme", "A theme is a JSON object."));
        };
        let mut errors: Vec<(String, String)> = Vec::new();
        // What is missing comes after what is wrong, so a short list shows the worst first.
        let mut missing: Vec<(String, String)> = Vec::new();
        let mut error = |at: &str, what: &str| errors.push((at.to_owned(), what.to_owned()));
        for key in root.keys() {
            if !matches!(
                key.as_str(),
                "schema" | "id" | "name" | "radius" | "density" | "font" | "dark" | "light"
            ) {
                error(&clip(key), "This field is not part of a theme.");
            }
        }
        let text_of = |key: &str| root.get(key).and_then(Value::as_str);
        if text_of("schema") != Some(SCHEMA) {
            error("schema", "Expected \"navree-theme/1\".");
        }
        let id = match text_of("id").map(ThemeId::parse) {
            Some(Ok(id)) => Some(id),
            Some(Err(why)) => {
                error("id", &why);
                None
            }
            None => {
                error("id", "Missing.");
                None
            }
        };
        let name = match text_of("name").map(|name| (clean_text(name, usize::MAX), name)) {
            Some((_, name)) if name.chars().any(reorders) => {
                error("name", "A name can't hold characters that reorder text.");
                None
            }
            Some((name, _)) if !name.is_empty() && name.chars().count() <= 64 => Some(name),
            Some(_) => {
                error("name", "Give a name of 1 to 64 characters.");
                None
            }
            None => {
                error("name", "Missing.");
                None
            }
        };
        let radius = match root.get("radius") {
            Some(Value::Object(corners)) => {
                let length = |key: &str| -> Result<u8, LengthError> {
                    corners
                        .get(key)
                        .and_then(Value::as_str)
                        .ok_or(LengthError::Format)
                        .and_then(Radius::parse)
                };
                let extra = corners
                    .keys()
                    .find(|key| !matches!(key.as_str(), "control" | "surface"));
                match (length("control"), length("surface"), extra) {
                    (_, _, Some(key)) => {
                        error(
                            &format!("radius.{}", clip(key)),
                            "This field is not part of a theme.",
                        );
                        None
                    }
                    (Ok(control), Ok(surface), None) => match Radius::new(control, surface) {
                        Ok(radius) => Some(radius),
                        Err(why) => {
                            error("radius", &why.to_string());
                            None
                        }
                    },
                    (control, surface, None) => {
                        if let Err(why) = control {
                            error("radius.control", &why.to_string());
                        }
                        if let Err(why) = surface {
                            error("radius.surface", &why.to_string());
                        }
                        None
                    }
                }
            }
            _ => {
                error(
                    "radius",
                    "Give control and surface corners, such as \"4px\".",
                );
                None
            }
        };
        let density = text_of("density").and_then(Density::from_id);
        if density.is_none() {
            error("density", "Use compact, default or comfortable.");
        }
        let font = text_of("font").and_then(Font::from_id);
        if font.is_none() {
            error("font", "Use geist, system, humanist, geometric or serif.");
        }
        let mut palettes = [Palette::from_array([Rgb::BLACK; TOKEN_COUNT]); 2];
        for (side, palette) in Side::BOTH.into_iter().zip(palettes.iter_mut()) {
            let Some(Value::Object(colours)) = root.get(side.as_str()) else {
                error(side.as_str(), "Give a colour for every token.");
                continue;
            };
            for key in colours.keys() {
                if Token::from_name(key).is_none() {
                    error(
                        &format!("{}.{}", side.as_str(), clip(key)),
                        "This is not a token.",
                    );
                }
            }
            for token in Token::ALL {
                let at = format!("{}.{}", side.as_str(), token.name());
                match colours.get(token.name()).and_then(Value::as_str) {
                    Some(text) => match Rgb::parse(text) {
                        Ok(colour) => palette.set(token, colour),
                        Err(why) => error(&at, &why.to_string()),
                    },
                    None => missing.push((at, "Missing.".to_owned())),
                }
            }
        }
        errors.append(&mut missing);
        if !errors.is_empty() {
            return Err(ImportError::Fields(errors));
        }
        let (Some(id), Some(name), Some(radius), Some(density), Some(font)) =
            (id, name, radius, density, font)
        else {
            return Err(fields("theme", "This is not a theme."));
        };
        let [light, dark] = palettes;
        Self::custom(
            id,
            name,
            ThemeSpec {
                dark,
                light,
                radius,
                density,
                font,
            },
        )
    }

    /// The theme as Rust: a function returning it, for an app's code. `path` is
    /// where the toolkit is, `navree::ui` in an app.
    pub fn to_rust(&self, path: &str) -> String {
        let spec = self.spec();
        // `_theme` keeps a keyword id (`type`, `match`) from naming the function.
        let function = format!("{}_theme", self.id().replace('-', "_"));
        let mut out = String::with_capacity(4096);
        let _ = writeln!(
            out,
            "use {path}::theme::{{Density, Font, Palette, Radius, Rgb, Theme, ThemeId, ThemeSpec}};\n"
        );
        let name = self.name().replace('\\', "\\\\").replace('"', "\\\"");
        let _ = writeln!(out, "/// The \"{}\" theme.", name.replace('\n', " "));
        let _ = writeln!(out, "pub fn {function}() -> Theme {{");
        out.push_str("    let rgb = Rgb::hex;\n    let spec = ThemeSpec {\n");
        for (side, palette) in [("dark", &spec.dark), ("light", &spec.light)] {
            let _ = writeln!(out, "        {side}: Palette::from_array([");
            for (token, colour) in palette.iter() {
                let _ = writeln!(
                    out,
                    "            rgb(0x{:06x}), // {}",
                    colour.to_u32(),
                    token.name()
                );
            }
            out.push_str("        ]),\n");
        }
        let radius = match spec.radius.step() {
            Some(step) => format!("Radius::{}", step.to_ascii_uppercase()),
            None => format!(
                "Radius::new({}, {}).expect(\"corners in range\")",
                spec.radius.control(),
                spec.radius.surface()
            ),
        };
        let _ = writeln!(out, "        radius: {radius},");
        let _ = writeln!(out, "        density: Density::{:?},", spec.density);
        let _ = writeln!(out, "        font: Font::{:?},", spec.font);
        out.push_str("    };\n");
        let _ = writeln!(
            out,
            "    Theme::custom(ThemeId::parse(\"{}\").expect(\"a theme id\"), \"{name}\", spec)\n        .expect(\"every text pair reaches WCAG AA\")",
            self.id()
        );
        out.push_str("}\n");
        out
    }
}

/// One error for the whole file.
fn fields(at: &str, what: &str) -> ImportError {
    ImportError::Fields(vec![(at.to_owned(), what.to_owned())])
}

/// A key from the file, cut short and stripped of control characters, for a message.
fn clip(key: &str) -> String {
    clean_text(key, 40)
}

/// Whether `c` reorders the text around it: a bidirectional embedding,
/// override, isolate or mark. Text holding one can show one thing and be another.
pub(crate) fn reorders(c: char) -> bool {
    matches!(
        c,
        '\u{61c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

/// `text` without control characters or characters that reorder text, then
/// without spaces at either end, at most `most` characters.
pub(crate) fn clean_text(text: &str, most: usize) -> String {
    let kept: String = text
        .chars()
        .filter(|c| !c.is_control() && !reorders(*c))
        .collect();
    let cut: String = kept.trim().chars().take(most).collect();
    cut.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_short_form_round_trips_every_built_in() {
        for theme in Theme::BUILT_IN {
            let form = to_form(theme.spec());
            assert_eq!(form.len(), FORM_LEN);
            assert!(
                form.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "{form}"
            );
            assert_eq!(from_form(&form).as_ref(), Some(theme.spec()));
        }
    }

    #[test]
    fn a_short_form_has_one_spelling() {
        let form = to_form(Theme::OCEAN.spec());
        for bad in [
            form.to_ascii_uppercase(),
            form.replacen("v1", "v2", 1),
            form.replacen("-d-", "-x-", 1),
            form.replacen("-g-", "-z-", 1),
            form.replacen("0406", "0904", 1),
            form.replacen("0406", "0604", 1),
            form.replacen("0406", "040a", 1),
            format!("{form}-"),
            form[1..].to_owned(),
            form.replacen('-', "_", 1),
            String::new(),
            "navree".to_owned(),
        ] {
            assert_eq!(from_form(&bad), None, "{bad}");
        }
        // A digit swapped for a non-hex letter, anywhere in the palettes.
        let mut bytes = form.clone().into_bytes();
        let at = FORM_LEN - 3;
        bytes[at] = b'g';
        assert_eq!(from_form(&String::from_utf8(bytes).unwrap()), None);
        // Multi-byte text of the right length.
        let wide = "é".repeat(FORM_LEN / 2);
        assert_eq!(from_form(&wide), None);
    }

    #[test]
    fn json_round_trips_and_names_its_schema() {
        let theme = Theme::custom(
            ThemeId::parse("acme").unwrap(),
            "Acme \"CRM\"",
            *Theme::SUNSET.spec(),
        )
        .unwrap();
        let json = theme.to_json();
        assert!(json.contains("\"schema\": \"navree-theme/1\""), "{json}");
        let back = Theme::from_json(&json).unwrap();
        assert_eq!(back.spec(), theme.spec());
        assert_eq!(back.name(), "Acme \"CRM\"");
        assert_eq!(back.id(), "acme");
        assert_eq!(back.key(), theme.key());
    }

    #[test]
    fn a_stored_theme_is_checked_field_by_field() {
        let good = Theme::custom(
            ThemeId::parse("acme").unwrap(),
            "Acme",
            *Theme::OCEAN.spec(),
        )
        .unwrap();
        let mut value: Value = serde_json::from_str(&good.to_json()).unwrap();
        value["dark"]["ink-0"] = Value::String("red;}body{display:none".into());
        value["light"]["fg"] = Value::String("url(javascript:alert(1))".into());
        value["radius"]["control"] = Value::String("40px".into());
        value["font"] = Value::String("Comic Sans".into());
        value["evil"] = Value::String("x".into());
        value["dark"]["--x"] = Value::String("#fff".into());
        value["id"] = Value::String("ocean".into());
        let Err(ImportError::Fields(errors)) = Theme::from_json(&value.to_string()) else {
            panic!("refused");
        };
        let at: Vec<&str> = errors.iter().map(|(at, _)| at.as_str()).collect();
        for expected in [
            "dark.ink-0",
            "light.fg",
            "radius",
            "font",
            "evil",
            "dark.--x",
            "id",
        ] {
            assert!(at.contains(&expected), "{expected} in {at:?}");
        }
        for bad in [
            "",
            "[]",
            "null",
            "{",
            "\"theme\"",
            &"x".repeat(JSON_MOST + 1),
        ] {
            assert!(
                matches!(Theme::from_json(bad), Err(ImportError::Fields(_))),
                "{bad:.20}"
            );
        }
        // A missing token is named.
        let mut missing: Value = serde_json::from_str(&good.to_json()).unwrap();
        missing["light"].as_object_mut().unwrap().remove("ring");
        let Err(ImportError::Fields(errors)) = Theme::from_json(&missing.to_string()) else {
            panic!("refused");
        };
        assert_eq!(
            errors,
            vec![("light.ring".to_owned(), "Missing.".to_owned())]
        );
    }

    #[test]
    fn a_stored_theme_whose_text_fails_is_refused_with_the_pairs() {
        let good = Theme::custom(
            ThemeId::parse("acme").unwrap(),
            "Acme",
            *Theme::NAVREE.spec(),
        )
        .unwrap();
        let mut value: Value = serde_json::from_str(&good.to_json()).unwrap();
        value["light"]["fg-3"] = Value::String("#dddddd".into());
        match Theme::from_json(&value.to_string()) {
            Err(ImportError::Contrast(list)) => {
                assert!(
                    list.iter()
                        .any(|s| s.pair.text == crate::theme::Paint::Token(Token::Fg3)
                            && s.side == Side::Light)
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn ids_are_strict() {
        for good in ["acme", "a", "acme-2", "x1"] {
            assert!(ThemeId::parse(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "Acme",
            "1acme",
            "-acme",
            "acme-",
            "ac--me",
            "ac me",
            "acme!",
            "ACME",
            "navree",
            "ocean",
            "custom",
            &"a".repeat(33),
            "acmé",
            "a\"b",
            "a<b",
        ] {
            assert!(ThemeId::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rust_export_names_every_token_and_quotes_the_name() {
        let theme = Theme::custom(
            ThemeId::parse("acme-crm").unwrap(),
            "Acme \"CRM\"\\",
            *Theme::VIOLET.spec(),
        )
        .unwrap();
        let rust = theme.to_rust("navree::ui");
        assert!(
            rust.contains("pub fn acme_crm_theme() -> Theme {"),
            "{rust}"
        );
        assert!(rust.contains("use navree::ui::theme::{"), "{rust}");
        assert!(rust.contains(r#""Acme \"CRM\"\\""#), "{rust}");
        assert_eq!(rust.matches("), // ").count(), TOKEN_COUNT * 2, "{rust}");
        assert!(rust.contains("radius: Radius::DEFAULT,"), "{rust}");
    }
}
