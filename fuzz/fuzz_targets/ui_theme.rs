//! `navree_ui::theme`: every parser of a theme from outside: a colour, a short
//! form (a stylesheet's name, a cookie), a setting, a stored theme (JSON) and
//! the builder's query.
//!
//! Input: a selector byte, then the text.
//!
//! Invariants: nothing panics. A colour that parses is written back as `#` and
//! six lower-case hex digits and parses to itself. A theme read from a short
//! form or a stored theme writes the same short form or JSON back, and its
//! stylesheet holds only the characters of names, colours, lengths, the font
//! stacks and the two images. A theme's name holds no control character and
//! nothing that reorders text, with no space at either end. A builder draft
//! reads back from its own query to the same theme.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_ui::theme::{Appearance, BuilderDraft, Rgb, Theme, ThemeSetting};

/// What a theme's stylesheet may hold.
fn clean_css(css: &str) {
    assert!(
        css.chars()
            .all(|c| c.is_ascii_alphanumeric() || " \n#%(),-.:;=[]{}/'\"@_+*".contains(c)),
        "{css}"
    );
    assert!(!css.contains("</"), "{css}");
}

fn check_theme(theme: &Theme) {
    let back = Theme::from_short_form(theme.short_form()).expect("a theme's short form reads back");
    assert_eq!(back.spec(), theme.spec());
    if let Some(url) = theme.stylesheet() {
        assert!(
            url.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b)),
            "{url}"
        );
    }
    if let Some(css) = theme.css() {
        clean_css(&css);
    }
    assert!(
        theme.key().bytes().all(|b| b.is_ascii_alphanumeric()),
        "{}",
        theme.key()
    );
    // A name has no control character and nothing that reorders text, and no
    // space at either end.
    let name = theme.name();
    assert_eq!(name, name.trim(), "{name:?}");
    assert!(
        !name.chars().any(|c| c.is_control()
            || matches!(c, '\u{61c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')),
        "{name:?}"
    );
}

fuzz_target!(|data: &[u8]| {
    let Some((&which, rest)) = data.split_first() else {
        return;
    };
    let text = String::from_utf8_lossy(rest);
    match which % 5 {
        0 => {
            if let Ok(colour) = Rgb::parse(&text) {
                let hex = colour.to_hex();
                assert_eq!(hex.len(), 7);
                assert!(
                    hex.starts_with('#')
                        && hex[1..]
                            .bytes()
                            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                );
                assert_eq!(Rgb::parse(&hex), Ok(colour));
            }
        }
        1 => {
            if let Some(theme) = Theme::from_short_form(&text) {
                assert_eq!(theme.short_form(), text.as_ref(), "one spelling per theme");
                check_theme(&theme);
            }
        }
        2 => {
            if let Some(setting) = ThemeSetting::parse(&text) {
                if let (Some(theme), Some(mode)) = (setting.theme.clone(), setting.mode) {
                    let appearance = Appearance::new(theme, mode);
                    let again = Appearance::parse(&appearance.to_value()).expect("reads back");
                    assert_eq!(again.theme().spec(), appearance.theme().spec());
                    check_theme(appearance.theme());
                }
            }
        }
        3 => {
            if let Ok(theme) = Theme::from_json(&text) {
                let again = Theme::from_json(&theme.to_json()).expect("a stored theme reads back");
                assert_eq!(again.spec(), theme.spec());
                assert_eq!(again.name(), theme.name());
                check_theme(&theme);
            }
        }
        _ => {
            let draft = BuilderDraft::from_query(&text);
            let spec = draft.spec();
            let again = BuilderDraft::from_query(&draft.to_query());
            assert_eq!(again.spec(), spec, "a draft reads back from its own query");
            let shown = draft.draft();
            if let Some(css) = shown.css() {
                clean_css(&css);
            }
            if let Ok(theme) = draft.to_theme() {
                check_theme(&theme);
            }
        }
    }
});
