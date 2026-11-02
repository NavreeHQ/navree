//! The design tokens' one source. [`css`] writes `styles/tokens.css`, a test
//! fails when the committed file differs, and the runtime's build step bundles
//! it into the stylesheet. The `@navree/theme` package's colours and radii are
//! generated from the same table ([`SITE`], [`theme_css`], [`theme_ts`]), and a
//! test holds them equal to the website's own stylesheet.
//!
//! After changing a token, write every generated file:
//!
//! ```sh
//! cargo test -p navree-ui --lib tokens::tests::write_generated_files -- --ignored
//! ```
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

/// The tokens `@navree/theme` and the website share, in order: the colours.
pub const SITE: &[&str] = &[
    "ink-0",
    "ink-1",
    "ink-2",
    "ink-3",
    "line",
    "line-2",
    "fg",
    "fg-1",
    "fg-2",
    "fg-3",
    "accent",
    "accent-ink",
    "amber",
    "danger",
    "bar",
];

/// The value of token `name` in the dark theme.
pub fn dark(name: &str) -> Option<&'static str> {
    TOKENS
        .iter()
        .find(|token| token.name == name)
        .map(|token| token.dark)
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

/// The marker lines that fence generated text inside a hand-written file.
pub const BEGIN: &str = "tokens:begin";
/// See [`BEGIN`].
pub const END: &str = "tokens:end";

/// The `--color-*` declarations of [`SITE`] for a stylesheet, and the radii when
/// `radii` is set.
pub fn theme_css(radii: bool) -> String {
    let mut out = String::new();
    for name in SITE {
        out.push_str(&format!(
            "  --color-{name}: {};
",
            dark(name).unwrap_or_default()
        ));
    }
    if radii {
        out.push_str(&format!(
            "  --radius-control: {};
",
            dark("radius").unwrap_or_default()
        ));
        out.push_str(&format!(
            "  --radius-surface: {};
",
            dark("radius-lg").unwrap_or_default()
        ));
    }
    out
}

/// The entries of the `colors` object in `@navree/theme`'s `index.ts`.
pub fn theme_ts() -> String {
    let mut out = String::new();
    for name in SITE {
        let key = if name.contains('-') {
            format!("\"{name}\"")
        } else {
            (*name).to_owned()
        };
        out.push_str(&format!(
            "  {key}: \"{}\",
",
            dark(name).unwrap_or_default()
        ));
    }
    out
}

/// `text` with the lines between its two marker lines replaced by `block`. The
/// marker lines stay. `None` when a marker is missing or they are out of order.
pub fn splice(text: &str, block: &str) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let begin = lines.iter().position(|line| line.contains(BEGIN))?;
    let end = lines.iter().position(|line| line.contains(END))?;
    if end <= begin {
        return None;
    }
    let mut out = String::with_capacity(text.len() + block.len());
    lines[..=begin].iter().for_each(|line| out.push_str(line));
    out.push_str(block);
    lines[end..].iter().for_each(|line| out.push_str(line));
    Some(out)
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
        Some(if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        })
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

    const WRITE: &str =
        "rerun: cargo test -p navree-ui --lib tokens::tests::write_generated_files -- --ignored";

    /// The generated files and what each is made from: the stylesheet's tokens, and
    /// the blocks of the `@navree/theme` package.
    fn generated() -> Vec<(std::path::PathBuf, String)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let theme = root.join("../../packages/js/theme");
        let read = |path: &std::path::Path| {
            std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
                .replace("\r\n", "\n")
        };
        let spliced = |file: &str, block: String| {
            let path = theme.join(file);
            let text = splice(&read(&path), &block)
                .unwrap_or_else(|| panic!("{file} has no `{BEGIN}`/`{END}` lines"));
            (path, text)
        };
        vec![
            (root.join("styles/tokens.css"), css()),
            spliced("css/theme.css", theme_css(false)),
            spliced("css/tokens.css", theme_css(true)),
            spliced("src/index.ts", theme_ts()),
        ]
    }

    #[test]
    fn the_committed_files_made_from_the_tokens_are_current() {
        for (path, text) in generated() {
            let committed = std::fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n");
            assert_eq!(committed, text, "{} is stale. {WRITE}", path.display());
        }
    }

    /// Not a check: writes the files the test above compares.
    #[test]
    #[ignore = "writes files"]
    fn write_generated_files() {
        for (path, text) in generated() {
            std::fs::write(&path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }

    #[test]
    fn splice_keeps_the_marker_lines_and_the_rest() {
        let text = "a\n  /* tokens:begin x */\n  old\n  /* tokens:end */\nb\n";
        assert_eq!(
            splice(text, "  new\n").unwrap(),
            "a\n  /* tokens:begin x */\n  new\n  /* tokens:end */\nb\n"
        );
        assert_eq!(splice("no markers", "x"), None);
        assert_eq!(splice("tokens:end\ntokens:begin\n", "x"), None);
    }
}
