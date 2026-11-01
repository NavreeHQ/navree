//! The design tokens' one source. [`css`] writes `styles/tokens.css`, a test
//! fails when the committed file differs, and the runtime's build step bundles
//! it into the stylesheet.
//!
//! The dark theme ships. Each token holds its light value too, so the light
//! theme is a matter of writing it out.

/// One colour or length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    /// The CSS custom property, without `--`.
    pub name: &'static str,
    /// The value in the dark theme.
    pub dark: &'static str,
    /// The value in the light theme.
    pub light: &'static str,
}

const fn t(name: &'static str, dark: &'static str, light: &'static str) -> Token {
    Token { name, dark, light }
}

/// Every token, in the order written.
pub const TOKENS: &[Token] = &[
    t("ink-0", "#040605", "#ffffff"),
    t("ink-1", "#0a0e0c", "#f7f8f7"),
    t("ink-2", "#111714", "#eff1f0"),
    t("ink-3", "#16201b", "#e5e8e6"),
    t("line", "#1c2420", "#dfe3e0"),
    t("line-2", "#2a3530", "#c8cecb"),
    t("line-3", "#3a4741", "#aeb6b2"),
    t("fg", "#e9efeb", "#0a0e0c"),
    t("fg-1", "#cfd8d3", "#1c2420"),
    t("fg-2", "#a9b5ae", "#3f4a44"),
    t("fg-3", "#8a968f", "#55615b"),
    t("accent", "#3dff8f", "#0b7a3e"),
    t("accent-ink", "#03130a", "#ffffff"),
    t("amber", "#ffbf5e", "#8a5a00"),
    t("danger", "#ff6b6b", "#b3261e"),
    t("bar", "#5b6862", "#8a968f"),
    t("radius", "4px", "4px"),
    t("radius-lg", "6px", "6px"),
    t("sidebar-w", "16rem", "16rem"),
    t("sidebar-w-icon", "3rem", "3rem"),
    t("topbar-h", "3rem", "3rem"),
    t("page-pad", "1.5rem", "1.5rem"),
    t(
        "font-sans",
        "Geist,ui-sans-serif,system-ui,-apple-system,\"Segoe UI\",Roboto,sans-serif",
        "Geist,ui-sans-serif,system-ui,-apple-system,\"Segoe UI\",Roboto,sans-serif",
    ),
    t(
        "font-mono",
        "ui-monospace,SFMono-Regular,Menlo,Consolas,monospace",
        "ui-monospace,SFMono-Regular,Menlo,Consolas,monospace",
    ),
];

/// Text colour and background pairs that must reach WCAG AA (4.5:1).
pub const TEXT_PAIRS: &[(&str, &str)] = &[
    ("fg", "ink-0"),
    ("fg", "ink-1"),
    ("fg", "ink-3"),
    ("fg-1", "ink-1"),
    ("fg-2", "ink-1"),
    ("fg-2", "ink-3"),
    ("fg-3", "ink-0"),
    ("fg-3", "ink-1"),
    ("fg-3", "ink-3"),
    ("accent-ink", "accent"),
    ("danger", "ink-1"),
    ("amber", "ink-1"),
    ("accent", "ink-1"),
];

/// The value of token `name` in the dark theme.
pub fn dark(name: &str) -> Option<&'static str> {
    TOKENS.iter().find(|token| token.name == name).map(|token| token.dark)
}

/// `styles/tokens.css`, as it must read.
pub fn css() -> String {
    let mut out = String::from(":root{color-scheme:dark");
    for token in TOKENS {
        out.push_str(";--");
        out.push_str(token.name);
        out.push(':');
        out.push_str(token.dark);
    }
    out.push_str("}\n");
    out
}

/// The WCAG contrast ratio of two `#rrggbb` colours; `None` for anything else.
pub fn contrast(a: &str, b: &str) -> Option<f64> {
    let (a, b) = (luminance(a)?, luminance(b)?);
    let (light, dark) = if a > b { (a, b) } else { (b, a) };
    Some((light + 0.05) / (dark + 0.05))
}

fn luminance(hex: &str) -> Option<f64> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let channel = |at: usize| -> Option<f64> {
        let value = f64::from(u8::from_str_radix(hex.get(at..at + 2)?, 16).ok()?) / 255.0;
        Some(if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) })
    };
    Some(0.2126 * channel(0)? + 0.7152 * channel(2)? + 0.0722 * channel(4)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_pair_reaches_aa() {
        for theme in [|t: &Token| t.dark, |t: &Token| t.light] {
            for (text, background) in TEXT_PAIRS {
                let value = |name: &str| TOKENS.iter().find(|t| t.name == name).map(theme).unwrap();
                let ratio = contrast(value(text), value(background)).unwrap();
                assert!(ratio >= 4.5, "{text} on {background} is {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn contrast_is_the_wcag_formula() {
        let ratio = contrast("#000000", "#ffffff").unwrap();
        assert!((ratio - 21.0).abs() < 0.01);
        assert_eq!(contrast("red", "#ffffff"), None);
    }

    #[test]
    fn the_committed_stylesheet_tokens_are_current() {
        let committed = include_str!("../styles/tokens.css").replace("\r\n", "\n");
        assert_eq!(committed, css(), "rerun: cargo test -p navree-ui --test tokens -- --ignored write");
    }
}
