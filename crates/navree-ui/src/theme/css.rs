//! Themes written as CSS, by this module alone.
//!
//! Every rule is at zero specificity (`:where(...)`), so order decides and any
//! rule of an app's own beats them. A theme is one block whose colours are
//! `light-dark(light, dark)`; the mode only sets `color-scheme`, so system mode
//! is the browser's and needs no script and no media query for colours. The tick
//! and the chevron are images, which `light-dark()` can't take, so they have a
//! block per mode.
//!
//! The only text that reaches a rule is a token's name from [`Token`], a
//! colour written by [`Rgb::write_hex`], a length in px from [`Radius`], a font
//! stack from [`Font`], or a built-in id: never anything given.

use std::fmt::Write as _;
use std::sync::LazyLock;

use bytes::Bytes;
use http::StatusCode;
use http::header::{
    ACCEPT_ENCODING, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_TYPE, ETAG, HeaderMap, HeaderValue,
    IF_NONE_MATCH, VARY,
};
use navree_http::response::{IntoResponse, Response};

use super::{Density, Rgb, Side, Theme, ThemeSpec, Token};
use crate::tokens::TOKENS;

/// Where a theme of your own is served, under the toolkit's files.
pub(crate) const SHEET_ROOT: &str = "/_navree/ui/theme";

/// The lengths and fonts every theme shares, from the token table.
const FIXED: [&str; 5] = [
    "sidebar-w",
    "sidebar-w-icon",
    "topbar-h",
    "page-pad",
    "font-mono",
];

/// The scrim behind a modal: a light page dims less.
const SCRIM: &str = "light-dark(rgb(10 14 12/.4),rgb(1 3 2/.72))";

/// The stylesheet's token layer, written to `styles/tokens.css`: the default
/// theme on the root and on every theme scope, the modes, the densities, then
/// every other built-in theme.
pub fn token_layer() -> String {
    let base = Theme::NAVREE.spec();
    let mut out = String::with_capacity(16 * 1024);
    out.push_str(":where(:root,[data-theme]){color-scheme:dark");
    colours(&mut out, base, None);
    corners_and_font(&mut out, base, None);
    for name in FIXED {
        let value = TOKENS
            .iter()
            .find(|token| token.name == name)
            .map_or("", |token| token.dark);
        let _ = write!(out, ";--{name}:{value}");
    }
    hover_mix(&mut out, base);
    let _ = write!(out, ";--scrim:{SCRIM}");
    images(&mut out, base, Side::Dark);
    out.push_str("}\n");
    out.push_str(":where([data-mode=dark]){color-scheme:dark}\n");
    out.push_str(":where([data-mode=light]){color-scheme:light");
    images(&mut out, base, Side::Light);
    out.push_str("}\n");
    out.push_str(":where([data-mode=system]){color-scheme:light dark}\n");
    out.push_str("@media (prefers-color-scheme:light){:where([data-mode=system]){");
    images_bare(&mut out, base, Side::Light);
    out.push_str("}}\n");
    for density in Density::ALL {
        let scale = match density {
            Density::Compact => "87.5%",
            Density::Default => continue,
            Density::Comfortable => "112.5%",
        };
        let _ = writeln!(
            out,
            ":root[data-density={}]{{font-size:{scale}}}",
            density.id()
        );
    }
    for theme in Theme::BUILT_IN.iter().skip(1) {
        theme_rules(&mut out, theme.key(), theme.spec(), Some(base));
    }
    // The theme menu's swatches.
    for token in [Token::Ink1, Token::Accent, Token::Fg] {
        let name = token.name();
        let _ = writeln!(
            out,
            ":where([data-token={name}]){{--token-bg:var(--{name})}}"
        );
    }
    out
}

/// The builder stylesheet's generated part, written to
/// `styles/extra/theme-builder-tokens.css`: what the builder draws a token
/// with, with no style attribute. `data-token` paints a swatch, `data-fg`
/// colours text, `data-swatch` is an accent the builder offers, `data-font` a
/// face from the list and `data-radius` a corner step.
pub fn builder_layer() -> String {
    let mut out = String::with_capacity(4096);
    helpers(&mut out);
    out
}

fn helpers(out: &mut String) {
    for token in Token::ALL {
        let name = token.name();
        let _ = writeln!(
            out,
            ":where([data-token={name}]){{--token-bg:var(--{name})}}"
        );
    }
    for token in Token::ALL {
        let name = token.name();
        let _ = writeln!(out, ":where([data-fg={name}]){{color:var(--{name})}}");
    }
    for swatch in super::SWATCHES {
        let _ = write!(
            out,
            ":where([data-swatch={}]){{--swatch:light-dark(",
            swatch.id
        );
        swatch.light.write_hex(out);
        out.push(',');
        swatch.dark.write_hex(out);
        out.push_str(
            ")}
",
        );
    }
    // A pair's swatch with a tint or mix on its surface: the tint's token and share.
    for token in Token::ALL {
        let name = token.name();
        let _ = writeln!(out, ":where([data-tint={name}]){{--tint:var(--{name})}}");
    }
    let mut shares: Vec<u8> = super::pairs()
        .iter()
        .filter_map(|pair| match pair.on {
            super::Paint::Tint(_, percent, _) | super::Paint::Mix(_, percent, _) => Some(percent),
            _ => None,
        })
        .collect();
    shares.sort_unstable();
    shares.dedup();
    for percent in shares {
        let _ = writeln!(out, ":where([data-pct=\"{percent}\"]){{--pct:{percent}%}}");
    }
    // A swatch of each family of greys: its strongest border, which shows the tint.
    for neutral in super::Neutral::ALL {
        let mut spec = *Theme::NAVREE.spec();
        neutral.apply(&mut spec);
        let _ = write!(
            out,
            ":where([data-neutral={}]){{--swatch:light-dark(",
            neutral.id()
        );
        spec.light.get(Token::Line3).write_hex(out);
        out.push(',');
        spec.dark.get(Token::Line3).write_hex(out);
        out.push_str(")}\n");
    }
    for font in super::Font::ALL {
        let _ = writeln!(
            out,
            ":where([data-font={}]){{font-family:{}}}",
            font.id(),
            font.stack()
        );
    }
    for (id, _, radius) in super::Radius::STEPS {
        let _ = writeln!(
            out,
            ":where([data-radius={id}]){{--corner:{}px}}",
            radius.surface()
        );
    }
}

/// The stylesheet of a theme of your own: every token, scoped to its key.
pub(crate) fn sheet(theme: &Theme) -> String {
    let mut out = String::with_capacity(4096);
    theme_rules(&mut out, theme.key(), theme.spec(), None);
    out
}

/// A theme's rules: its block, then the images of the light palette for light
/// and for system mode. With `base`, only what differs from it.
fn theme_rules(out: &mut String, key: &str, spec: &ThemeSpec, base: Option<&ThemeSpec>) {
    let _ = write!(out, ":where([data-theme={key}]){{");
    let start = out.len();
    colours(out, spec, base);
    corners_and_font(out, spec, base);
    if base.is_none_or(|base| {
        base.light.hover_mix() != spec.light.hover_mix()
            || base.dark.hover_mix() != spec.dark.hover_mix()
    }) {
        hover_mix(out, spec);
    }
    let dark_images = base.is_none_or(|base| images_differ(spec, base, Side::Dark));
    if dark_images {
        images(out, spec, Side::Dark);
    }
    // The block's first declaration has a `;` before it: drop it.
    if out[start..].starts_with(';') {
        out.remove(start);
    }
    out.push_str("}\n");
    if base.is_none_or(|base| images_differ(spec, base, Side::Light) || dark_images) {
        let _ = write!(out, ":where([data-theme={key}][data-mode=light]){{");
        images_bare(out, spec, Side::Light);
        out.push_str("}\n");
        let _ = write!(
            out,
            "@media (prefers-color-scheme:light){{:where([data-theme={key}][data-mode=system]){{"
        );
        images_bare(out, spec, Side::Light);
        out.push_str("}}\n");
    }
}

/// `;--name:light-dark(light,dark)` for every colour, or the one colour when both
/// palettes agree. With `base`, only the tokens that differ from it.
fn colours(out: &mut String, spec: &ThemeSpec, base: Option<&ThemeSpec>) {
    for token in Token::ALL {
        let (light, dark) = (spec.light.get(token), spec.dark.get(token));
        if base.is_some_and(|base| base.light.get(token) == light && base.dark.get(token) == dark) {
            continue;
        }
        let _ = write!(out, ";--{}:", token.name());
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
}

/// `;--hover-mix:light-dark(light,dark)`: away from the text on the accent.
fn hover_mix(out: &mut String, spec: &ThemeSpec) {
    out.push_str(";--hover-mix:light-dark(");
    spec.light.hover_mix().write_hex(out);
    out.push(',');
    spec.dark.hover_mix().write_hex(out);
    out.push(')');
}

fn corners_and_font(out: &mut String, spec: &ThemeSpec, base: Option<&ThemeSpec>) {
    if base.is_none_or(|base| base.radius != spec.radius) {
        let _ = write!(
            out,
            ";--radius:{}px;--radius-lg:{}px",
            spec.radius.control(),
            spec.radius.surface()
        );
    }
    if base.is_none_or(|base| base.font != spec.font) {
        let _ = write!(out, ";--font-sans:{}", spec.font.stack());
    }
}

/// Whether a built-in theme needs images of its own: only when its tick would
/// be light where the default's is dark, or the other way round. A near-black
/// tick reads on every light accent and a white one on every dark accent, and a
/// chevron's grey is the same to the eye in every built-in, so most share the
/// default's and the stylesheet stays small.
fn images_differ(spec: &ThemeSpec, base: &ThemeSpec, side: Side) -> bool {
    let light = |colour: Rgb| colour.luminance() > 0.18;
    light(spec.palette(side).get(Token::AccentInk))
        != light(base.palette(side).get(Token::AccentInk))
}

/// `;--ui-tick:url(...);--ui-chevron:url(...)` in `side`'s colours.
fn images(out: &mut String, spec: &ThemeSpec, side: Side) {
    out.push(';');
    images_bare(out, spec, side);
}

fn images_bare(out: &mut String, spec: &ThemeSpec, side: Side) {
    let palette = spec.palette(side);
    out.push_str("--ui-tick:");
    tick(out, palette.get(Token::AccentInk));
    out.push_str(";--ui-chevron:");
    chevron(out, palette.get(Token::Fg3));
}

/// The tick of a checked box, in `colour`.
fn tick(out: &mut String, colour: Rgb) {
    out.push_str("url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' fill='none' stroke='%23");
    colour.write_digits(out);
    out.push_str("' stroke-width='3' stroke-linecap='round' stroke-linejoin='round' viewBox='0 0 24 24'%3E%3Cpath d='M20 6 9 17l-5-5'/%3E%3C/svg%3E\")");
}

/// The chevron of a native select, in `colour`.
fn chevron(out: &mut String, colour: Rgb) {
    out.push_str("url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='16' height='16' fill='none' stroke='%23");
    colour.write_digits(out);
    out.push_str("' stroke-width='1.75' stroke-linecap='round' stroke-linejoin='round' viewBox='0 0 24 24'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E\")");
}

/// The build theme stylesheets belong to: the toolkit's own and what this
/// version writes for a sample theme, as 12 hex digits. It is in each sheet's
/// URL, so a sheet is cached for good only while what it holds can't change.
pub(crate) fn sheet_build() -> &'static str {
    static BUILD: LazyLock<String> = LazyLock::new(|| {
        let mut spec = *Theme::SUNSET.spec();
        spec.radius = super::Radius::SHARP;
        spec.density = Density::Compact;
        spec.font = super::Font::System;
        let sample = Theme::unchecked(super::ThemeId::custom(), String::new(), spec);
        let mut text = String::from(crate::assets::BUILD);
        text.push_str(&sheet(&sample));
        crate::assets::content_hash(text.as_bytes())
    });
    &BUILD
}

/// Whether `text` is a build: 12 lower-case hex digits.
fn is_build(text: &str) -> bool {
    text.len() == 12
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Answers `GET /_navree/ui/theme/{file}`: the stylesheet of the theme whose
/// short form the file is named by, then a dash and the build, or 404 for
/// anything else. A sheet of this build is cached for good; one named by
/// another build, or by its short form alone, is served as this build writes
/// it and checked again each time, as the toolkit's other files are.
pub(crate) fn respond(file: &str, request: &HeaderMap) -> Response {
    let Some(name) = file.strip_suffix(".css") else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let built = name
        .rsplit_once('-')
        .filter(|(_, build)| is_build(build))
        .and_then(|(form, build)| {
            super::codec::from_form(form).map(|spec| (spec, build == sheet_build()))
        });
    let Some((spec, current)) =
        built.or_else(|| super::codec::from_form(name).map(|spec| (spec, false)))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let theme = Theme::unchecked(super::ThemeId::custom(), String::new(), spec);
    let css = sheet(&theme);
    let accept = request
        .get(ACCEPT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let brotli = crate::compress::worth_brotli(css.len(), accept);
    // Each encoding is its own representation, with its own tag.
    let etag = HeaderValue::from_str(&format!(
        "\"{}-{}{}\"",
        theme.key(),
        sheet_build(),
        if brotli { "-br" } else { "" }
    ))
    .unwrap_or(HeaderValue::from_static("\"theme\""));
    let cache = HeaderValue::from_static(if current {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    });
    let vary = HeaderValue::from_static("accept-encoding");
    if request
        .get(IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|tags| matches_tag(tags, &etag))
    {
        return (
            StatusCode::NOT_MODIFIED,
            [(ETAG, etag), (CACHE_CONTROL, cache), (VARY, vary)],
        )
            .into_response();
    }
    let mut response = (
        [
            (
                CONTENT_TYPE,
                HeaderValue::from_static("text/css; charset=utf-8"),
            ),
            (CACHE_CONTROL, cache),
            (ETAG, etag),
            (VARY, vary),
        ],
        if brotli {
            Bytes::from(crate::compress::brotli(css.as_bytes()))
        } else {
            Bytes::from(css)
        },
    )
        .into_response();
    if brotli {
        response
            .headers_mut()
            .insert(CONTENT_ENCODING, HeaderValue::from_static("br"));
    }
    response
}

/// Whether an `If-None-Match` list names `etag`, weak or strong, or is `*`.
fn matches_tag(tags: &str, etag: &HeaderValue) -> bool {
    let etag = etag.to_str().unwrap_or_default();
    tags.split(',')
        .map(str::trim)
        .any(|tag| tag == "*" || tag == etag || tag.strip_prefix("W/") == Some(etag))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeId;

    /// What a rule may hold: the characters of names, colours, lengths, the
    /// font stacks and the two SVG images.
    fn allowed(css: &str) -> bool {
        css.chars()
            .all(|c| c.is_ascii_alphanumeric() || " \n#%(),-.:;=[]{}/'\"@_+*".contains(c))
    }

    #[test]
    fn the_layer_scopes_the_default_and_every_built_in() {
        let css = token_layer();
        assert!(
            css.starts_with(
                ":where(:root,[data-theme]){color-scheme:dark;--ink-0:light-dark(#ffffff,#040605)"
            ),
            "{css:.120}"
        );
        for theme in Theme::BUILT_IN.iter().skip(1) {
            assert!(
                css.contains(&format!(":where([data-theme={}]){{", theme.id())),
                "{}",
                theme.id()
            );
        }
        assert!(css.contains(":where([data-mode=system]){color-scheme:light dark}"));
        assert!(css.contains(":root[data-density=compact]{font-size:87.5%}"));
        assert!(
            !css.contains("data-theme=navree"),
            "the default needs no block of its own"
        );
        assert!(allowed(&css));
        // Every rule is at zero specificity but the densities, which only the root takes.
        for rule in css.lines() {
            assert!(
                rule.starts_with(":where(")
                    || rule.starts_with("@media (prefers-color-scheme:light){:where(")
                    || rule.starts_with(":root[data-density="),
                "{rule:.80}"
            );
        }
    }

    #[test]
    fn a_built_in_block_holds_only_what_differs_from_the_default() {
        let css = token_layer();
        let ocean = css
            .lines()
            .find(|line| line.starts_with(":where([data-theme=ocean]){"))
            .unwrap();
        assert!(
            ocean.contains("--accent:light-dark(#1f55d6,#5aa2ff)"),
            "{ocean}"
        );
        assert!(
            !ocean.contains("--amber:"),
            "the same as the default: {ocean}"
        );
        assert!(!ocean.contains("--radius:"), "{ocean}");
        assert!(!ocean.contains("{;"), "{ocean}");
    }

    #[test]
    fn a_custom_sheet_sets_every_token_and_nothing_else() {
        let theme = Theme::custom(
            ThemeId::parse("acme").unwrap(),
            "x}body{display:none",
            *Theme::ROSE.spec(),
        )
        .unwrap();
        let css = sheet(&theme);
        let key = theme.key();
        assert!(
            css.starts_with(&format!(":where([data-theme={key}]){{--ink-0:")),
            "{css:.80}"
        );
        for token in Token::ALL {
            assert!(
                css.contains(&format!("--{}:", token.name())),
                "{}",
                token.name()
            );
        }
        assert!(!css.contains("display:none"), "the name is never CSS");
        assert!(allowed(&css));
        assert!(css.len() < 4096, "{}", css.len());
    }

    #[test]
    fn the_sheet_route_serves_a_form_and_refuses_everything_else() {
        let theme = Theme::custom(
            ThemeId::parse("acme").unwrap(),
            "Acme",
            *Theme::VIOLET.spec(),
        )
        .unwrap();
        let url = theme.stylesheet().unwrap();
        let file = url.strip_prefix(&format!("{SHEET_ROOT}/")).unwrap();
        assert_eq!(
            file,
            format!("{}-{}.css", theme.short_form(), sheet_build()),
            "the build is in the name"
        );
        let response = respond(file, &HeaderMap::new());
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        assert_eq!(response.headers()[CONTENT_TYPE], "text/css; charset=utf-8");
        let etag = response.headers()[ETAG].clone();
        let mut again = HeaderMap::new();
        again.insert(IF_NONE_MATCH, etag.clone());
        assert_eq!(respond(file, &again).status(), StatusCode::NOT_MODIFIED);
        again.insert(IF_NONE_MATCH, HeaderValue::from_static("\"x\", W/\"y\""));
        assert_eq!(respond(file, &again).status(), StatusCode::OK);
        // Another build's name, or the short form alone: this build's sheet, not kept.
        let form = theme.short_form();
        for stale in [format!("{form}-000000000000.css"), format!("{form}.css")] {
            let response = respond(&stale, &HeaderMap::new());
            assert_eq!(response.status(), StatusCode::OK, "{stale:.40}");
            assert_eq!(response.headers()[CACHE_CONTROL], "no-cache", "{stale:.40}");
            assert_eq!(response.headers()[ETAG], etag, "the same sheet");
        }
        // A sheet is small: a client that takes gzip gets it from the pipeline's
        // gzip; brotli only for one that takes nothing else, with its own tag.
        let mut both = HeaderMap::new();
        both.insert(ACCEPT_ENCODING, HeaderValue::from_static("gzip, br"));
        let plain = respond(file, &both);
        assert!(!plain.headers().contains_key(CONTENT_ENCODING));
        assert_eq!(plain.headers()[ETAG], etag);
        let mut br = HeaderMap::new();
        br.insert(ACCEPT_ENCODING, HeaderValue::from_static("br"));
        let compressed = respond(file, &br);
        assert_eq!(compressed.headers()[CONTENT_ENCODING], "br");
        assert_eq!(compressed.headers()[VARY], "accept-encoding");
        let br_tag = compressed.headers()[ETAG].clone();
        assert_ne!(br_tag, etag, "brotli and identity are told apart");
        br.insert(IF_NONE_MATCH, etag.clone());
        assert_eq!(
            respond(file, &br).status(),
            StatusCode::OK,
            "the identity tag doesn't answer for brotli"
        );
        br.insert(IF_NONE_MATCH, br_tag);
        assert_eq!(respond(file, &br).status(), StatusCode::NOT_MODIFIED);
        assert!(crate::compress::worth_brotli(
            crate::compress::MIN_SIZE,
            "gzip, br"
        ));
        for bad in [
            "",
            ".css",
            "navree.css",
            "ocean.css",
            &file.replace(".css", ".js"),
            &file.to_ascii_uppercase(),
            &file.replacen("v1", "v9", 1),
            &file.replace(sheet_build(), "00000000000g"),
            &file.replace(&format!("-{}", sheet_build()), "-"),
            "../../etc/passwd",
            "v1-0406-d-g-;}body{x}.css",
        ] {
            assert_eq!(
                respond(bad, &HeaderMap::new()).status(),
                StatusCode::NOT_FOUND,
                "{bad:.40}"
            );
        }
    }
}
