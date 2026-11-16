//! What a theme says beyond its spec, and that a theme which says none of it writes exactly
//! what it wrote before these values existed.
//!
//! The pins below were taken from the toolkit at 1.7.0, before a theme could say more than
//! its spec: the token layer (the default theme and every built-in), a theme of your own's
//! short form, key, stylesheet, stored form and Rust, and the markup of a page. A change to
//! any of them is a change to every app that sets nothing new, so each is held here.

use navree_ui::theme::{
    Corner, Corners, ImportError, Shape, Theme, ThemeId, builder_layer, token_layer,
};
use navree_ui::{
    Badge, Button, ButtonVariant, Card, Input, NavGroup, NavItem, Shell, Stat, UserMenu, View, el,
    icons,
};
use sha2::{Digest, Sha256};

fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `html` with every run of exactly twelve hex digits (an asset's or a build's hash) made
/// `HASH`: what a page links changes with the toolkit's files, its markup must not.
fn unhashed(html: &str) -> String {
    let bytes = html.as_bytes();
    let hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    let word = |b: u8| b.is_ascii_alphanumeric();
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    while at < bytes.len() {
        let run = bytes[at..].iter().take_while(|b| hex(**b)).count();
        let before = at.checked_sub(1).is_some_and(|i| word(bytes[i]));
        let after = bytes.get(at + run).is_some_and(|b| word(*b));
        if run == 12 && !before && !after {
            out.push_str("HASH");
            at += run;
        } else if run > 0 {
            out.push_str(&html[at..at + run]);
            at += run;
        } else {
            let len = html[at..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&html[at..at + len]);
            at += len;
        }
    }
    out
}

fn agra(theme: &Theme) -> Theme {
    theme.renamed(ThemeId::parse("agra").unwrap(), "Agra")
}

fn page() -> String {
    Shell::new("Agra")
        .group(NavGroup::plain().item(NavItem::new("Home", "/").icon(icons::HOUSE)))
        .group(NavGroup::new("Sales").item(NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE)))
        .user(UserMenu::new("Anu", "anu@example.com").action("Sign out", "/sign-out"))
        .page("Deals", "/deals")
        .actions(Button::primary("New deal").href("/deals/new"))
        .body((
            Card::titled("Pipeline").body((
                Stat::new("Open", "12"),
                Badge::new("Won"),
                Input::text("q"),
                Button::new("Export").variant(ButtonVariant::Ghost),
            )),
            el("p").child("hi"),
        ))
        .to_view()
        .to_html()
}

/// Each built-in theme as a theme of your own called `agra`: the SHA-256 of its short form
/// (whose first 24 digits are its key) and of its stylesheet, at 1.7.0.
const BUILT_IN_SHEETS: [(&str, &str, &str); 7] = [
    (
        "navree",
        "96af204015e7e4113cd3cddabeea8cf202dd60faad9fdfd9acd4c86207629d42",
        "a120e9d3005c230a18db732e3b5974f69bfad7bd75bca39479c72e1cd2773f31",
    ),
    (
        "neutral",
        "765017eb859863fa7878f4c5e0db26188ec714bdadb7328fd562074e2c844644",
        "7f1d9a2ce30b2a163afabe2de66314105831b2a9a030985a8874023d8d666f53",
    ),
    (
        "ocean",
        "249ca3844aac7a072cb5cf6f3ed0a3cb8de4ef90760aa026af28e76b056d2608",
        "4a5adcbecc6261f56f8c43a049ac69ad5f7b96e6f49aad55020bef0defdbbb04",
    ),
    (
        "violet",
        "68543efcedc722abfacef75c58125cad6791f698ce15745d76a1cbe64616513d",
        "274e9e770b702f45d83c3a2c49830b963eb0da729061f39eecb71cde7813268b",
    ),
    (
        "sunset",
        "f58e0f80c1cdf4bf7b6095364e783ed2935dcf46588b26a99886557eb8a7cc92",
        "54081c00eaade8e77c1f59d7e9e935ec237b5eba8e7473d532ac384b52a60e78",
    ),
    (
        "rose",
        "6ea1085dfec06d31620185680802446d510955882a28661bbcff240c5c3208a9",
        "4feef0b5e5517cbd6dcfa80338eefca0b02cd9fe5dbe3e70301549467b9ae4e4",
    ),
    (
        "contrast",
        "f2d2787475d4972478e0acede9e8d74fab26f3d053474c9f43528a0ba761a5d1",
        "dc660f890aa98a6a0043120b3a27b20e8c4fa0864bb35b22bfaa43e1a846218d",
    ),
];

#[test]
fn a_theme_that_says_nothing_new_writes_what_it_wrote_at_1_7_0() {
    assert_eq!(
        sha(&token_layer()),
        "cc71fecc49cf231dd26786d08e5a60eb224e8667e32e6b897405b2c199dec046",
        "the token layer: the default theme, the modes and every built-in theme"
    );
    assert_eq!(
        sha(&builder_layer()),
        "4f31cefefd64f6e7f80e2899998c818fd602427a6e3ec0ac55e9e4affa505d7e",
        "the theme builder's helpers"
    );
    for ((id, form, css), theme) in BUILT_IN_SHEETS.iter().zip(Theme::BUILT_IN) {
        assert_eq!(theme.id(), *id);
        let mine = agra(&theme);
        assert_eq!(sha(mine.short_form()), *form, "{id}: the short form");
        assert_eq!(mine.key(), format!("c{}", &form[..24]), "{id}: the key");
        assert_eq!(sha(&mine.css().unwrap()), *css, "{id}: the stylesheet");
        assert!(theme.css().is_none(), "{id}: a built-in needs no sheet");
    }
    let mine = agra(&Theme::VIOLET);
    assert_eq!(
        sha(&mine.to_rust("navree::ui")),
        "a98d73d5f89a1e37cfcfdc8a23e932512287ea7bb28b53892a6eaa0897bd28ef",
        "the Rust export:\n{}",
        mine.to_rust("navree::ui")
    );
    assert_eq!(
        sha(&mine.to_json()),
        "23e54e09347cd5cd4c4af00a55e97865a3c1153047ce35efe26469ae160a74c6",
        "the stored theme:\n{}",
        mine.to_json()
    );
    assert_eq!(
        sha(&unhashed(
            &View::new("x").body(el("main").child("hi")).to_html()
        )),
        "4af8ccb150a4268b0e6d0b5cb917a599a9c3b36e53ed0d774fc4c44fbb36900c",
        "a page with nothing set"
    );
    assert_eq!(
        sha(&unhashed(&page())),
        "6ccab9194517b04898ec0fe17a26a139f5c1584a8176818de420cefa21bd5321",
        "a shell page with a card, a stat, a badge, an input and buttons:\n{}",
        unhashed(&page())
    );
}

/// The toolkit's stylesheet sources: the ones every page loads and each module's own.
fn sources() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&root)
        .unwrap()
        .chain(std::fs::read_dir(root.join("extra")).unwrap())
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "css"))
        .collect();
    files.sort();
    files
        .iter()
        .map(|path| std::fs::read_to_string(path).unwrap().replace("\r\n", "\n"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The custom properties a theme's block sets that a theme with nothing beyond its spec
/// doesn't, with their values: what it says beyond its spec.
fn said(theme: &Theme) -> Vec<(String, String)> {
    let plain = agra(&Theme::NAVREE).css().unwrap();
    let css = theme.css().unwrap();
    let block = css
        .lines()
        .find(|line| line.starts_with(&format!(":where([data-theme={}]){{", theme.key())))
        .unwrap();
    let body = &block[block.find('{').unwrap() + 1..block.len() - 1];
    body.split(';')
        .filter_map(|decl| decl.split_once(':'))
        .filter(|(name, _)| {
            !plain.contains(&format!(";{name}:")) && !plain.contains(&format!("{{{name}:"))
        })
        .map(|(name, value)| (name.trim_start_matches("--").to_owned(), value.to_owned()))
        .collect()
}

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
            .panel(Corner::Px(12))
            .overlay(Corner::Px(12)),
    )
}

#[test]
fn a_shape_sets_the_corner_of_each_part_and_travels_with_the_theme() {
    let plain = agra(&Theme::OCEAN);
    let shaped = plain.clone().with_shape(every_corner()).unwrap();
    assert_eq!(shaped.spec(), plain.spec(), "the spec is untouched");
    assert_eq!(shaped.shape(), &every_corner());
    assert!(plain.shape().is_default() && Theme::OCEAN.shape().is_default());
    let css = shaped.css().unwrap();
    for (name, value) in [
        ("radius-button", "999px"),
        ("radius-badge", "999px"),
        ("radius-field", "12px"),
        ("radius-nav", "10px"),
        ("radius-card", "16px"),
    ] {
        assert!(css.contains(&format!(";--{name}:{value}")), "{name}: {css}");
    }
    // A part of the page drawn in another theme is drawn as that theme alone.
    assert!(
        css.starts_with(&format!(
            ":where([data-theme={}] [data-theme]){{--radius-button:initial;",
            shaped.key()
        )),
        "{css:.120}"
    );
    // The short form keeps the spec's and adds the corners; it reads back to the same theme.
    let form = shaped.short_form();
    assert!(form.starts_with(plain.short_form()), "{form:.40}");
    assert_ne!(shaped.key(), plain.key());
    let back = Theme::from_short_form(form).unwrap();
    assert_eq!((back.spec(), back.shape()), (shaped.spec(), shaped.shape()));
    assert_eq!(back.short_form(), form);
    // The stored theme and the Rust export carry it; neither changes without it.
    let json = shaped.to_json();
    assert!(json.contains("\"radius-button\": \"pill\""), "{json}");
    let stored = Theme::from_json(&json).unwrap();
    assert_eq!(stored.shape(), shaped.shape());
    assert_eq!(stored.key(), shaped.key());
    let rust = shaped.to_rust("navree::ui");
    assert!(
        rust.contains("use navree::ui::theme::{Corner, Corners, Density, Font, Palette, Radius, Rgb, Shape, Theme, ThemeId, ThemeSpec};"),
        "{rust}"
    );
    assert!(
        rust.contains(".corners(Corners::new().button(Corner::Pill)"),
        "{rust}"
    );
    assert!(rust.contains(".card(Corner::Px(16))"), "{rust}");
    assert!(rust.contains("        .with_shape(shape)\n"), "{rust}");
    assert!(!plain.to_rust("navree::ui").contains("with_shape"));
    // Renamed, it keeps its shape.
    let renamed = shaped.renamed(ThemeId::parse("kesar").unwrap(), "Kesar");
    assert_eq!(renamed.shape(), shaped.shape());
}

#[test]
fn a_corner_out_of_range_or_a_built_in_theme_is_refused() {
    let card = |corner| Shape::new().corners(Corners::new().card(corner));
    match agra(&Theme::NAVREE).with_shape(card(Corner::Px(33))) {
        Err(ImportError::Fields(fields)) => assert_eq!(
            fields,
            vec![(
                "radius-card".to_owned(),
                "A corner takes 0 to 32 px, or a pill.".to_owned()
            )]
        ),
        other => panic!("{other:?}"),
    }
    assert!(
        agra(&Theme::NAVREE)
            .with_shape(card(Corner::Px(32)))
            .is_ok()
    );
    assert!(matches!(
        Theme::NAVREE.with_shape(every_corner()),
        Err(ImportError::Fields(_))
    ));
    // The default shape changes nothing.
    let same = agra(&Theme::ROSE).with_shape(Shape::new()).unwrap();
    assert_eq!(same, agra(&Theme::ROSE));
}

#[test]
fn every_corner_is_read_by_the_stylesheet_with_todays_corner_behind_it() {
    let shaped = agra(&Theme::NAVREE).with_shape(every_corner()).unwrap();
    let names = said(&shaped);
    assert_eq!(names.len(), 9, "{names:?}");
    let css = sources();
    for (name, _) in &names {
        let reads: Vec<&str> = css
            .match_indices(&format!("var(--{name},"))
            .map(|(at, _)| &css[at + name.len() + 7..])
            .collect();
        assert!(!reads.is_empty(), "{name} is read by no rule");
        for fallback in reads {
            assert!(
                fallback.starts_with("var(--radius)")
                    || fallback.starts_with("var(--radius-lg)")
                    || fallback.starts_with("0px)"),
                "{name} falls back to {fallback:.24}"
            );
        }
    }
    // A checkbox keeps the control corner: a round box reads as a radio.
    assert!(css.contains("[data-slot=checkbox]{border-radius:var(--radius);"));
}
