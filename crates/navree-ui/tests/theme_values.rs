//! What a theme says beyond its spec, and that a theme which says none of it writes exactly
//! what it wrote before these values existed.
//!
//! The pins below were taken from the toolkit at 1.7.0, before a theme could say more than
//! its spec: the token layer (the default theme and every built-in), a theme of your own's
//! short form, key, stylesheet, stored form and Rust, and the markup of a page. A change to
//! any of them is a change to every app that sets nothing new, so each is held here.

use navree_ui::theme::{
    CardSizes, ControlSizes, Corner, Corners, Face, FieldSizes, ImportError, Inverse, PageSizes,
    Rgb, Roles, Shape, SidebarSizes, Sizes, TextStyle, Theme, ThemeId, Token, TopbarSizes,
    TypeScale, builder_layer, token_layer,
};
use navree_ui::{
    AppFont, Badge, Button, ButtonVariant, Card, Input, NavGroup, NavItem, Shell, Stat, UserMenu,
    View, el, icons,
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

/// A light design's spec: Neutral, with a tinted sidebar (`ink-1`) under white cards.
fn tinted() -> Theme {
    let mut spec = *Theme::NEUTRAL.spec();
    spec.light.set(Token::Ink1, Rgb::hex(0xfafaf9));
    Theme::custom(ThemeId::parse("agra").unwrap(), "Agra", spec).unwrap()
}

/// White cards and fields, a ghost button in the text's colour, the sidebar's quieter text,
/// and a near-black inverse surface with white text on a light page.
fn design_roles() -> Roles {
    Roles::new()
        .card(Rgb::hex(0xffffff), Rgb::hex(0x121212))
        .field(Rgb::hex(0xffffff), Rgb::hex(0x121212))
        .ghost_text(Rgb::hex(0x0a0a0a), Rgb::hex(0xf2f2f2))
        .shell_muted(Rgb::hex(0x5e5e5a), Rgb::hex(0xb7b7b2))
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
fn roles_set_white_cards_on_a_tinted_sidebar_and_an_inverse_surface_whose_text_reads() {
    let plain = tinted();
    let theme = plain.clone().with_roles(design_roles()).unwrap();
    assert_eq!(theme.spec(), plain.spec(), "the palettes are untouched");
    assert_eq!(theme.roles(), &design_roles());
    assert!(plain.roles().is_default() && Theme::NAVREE.roles().is_default());
    let said = said(&theme);
    for (name, value) in [
        ("card", "light-dark(#ffffff,#121212)"),
        ("field", "light-dark(#ffffff,#121212)"),
        ("ghost-fg", "light-dark(#0a0a0a,#f2f2f2)"),
        ("shell-muted", "light-dark(#5e5e5a,#b7b7b2)"),
    ] {
        assert!(
            said.contains(&(name.to_owned(), value.to_owned())),
            "{name}: {said:?}"
        );
    }
    let css = theme.css().unwrap();
    assert!(
        css.contains(";--ink-1:light-dark(#fafaf9,"),
        "the sidebar keeps its tint"
    );
    // The inverse surface is drawn on the other palette with the theme's colours in place:
    // its scheme is flipped, so each colour is written in the other order.
    let scope = css
        .lines()
        .find(|line| {
            line.starts_with(&format!(
                ":where([data-theme={}] [data-inverse]){{",
                theme.key()
            ))
        })
        .expect("the inverse surface's block");
    for part in [
        "--ink-1:light-dark(#ffffff,#0a0a0a)",
        "--card:light-dark(#ffffff,#0a0a0a)",
        "--fg:light-dark(#0a0a0a,#ffffff)",
        "--fg-2:light-dark(#3d3d3a,#b5b5b0)",
        "--line:light-dark(#c4c4bf,#2a2a28)",
        "--accent:light-dark(#0a0a0a,#ffffff)",
        "--accent-ink:light-dark(#ffffff,#0a0a0a)",
    ] {
        assert!(scope.contains(part), "{part}: {scope}");
    }
    // A part of the page drawn in another theme loses the roles too.
    assert!(
        css.lines().next().unwrap().contains("--card:initial"),
        "{css:.200}"
    );
    // The short form, the stored theme and the Rust export carry them.
    let back = Theme::from_short_form(theme.short_form()).unwrap();
    assert_eq!(back.roles(), theme.roles());
    assert_eq!(back.short_form(), theme.short_form());
    assert!(theme.short_form().starts_with(plain.short_form()));
    let stored = Theme::from_json(&theme.to_json()).unwrap();
    assert_eq!((stored.roles(), stored.key()), (theme.roles(), theme.key()));
    let rust = theme.to_rust("navree::ui");
    assert!(
        rust.contains("Inverse, Palette, Radius, Rgb, Roles, Theme"),
        "{rust}"
    );
    assert!(
        rust.contains(".card(rgb(0xffffff), rgb(0x121212))"),
        "{rust}"
    );
    assert!(rust.contains(".inverse(Inverse::new()"), "{rust}");
    assert!(rust.contains("        .with_roles(roles)\n"), "{rust}");
    assert_eq!(
        theme
            .renamed(ThemeId::parse("kesar").unwrap(), "Kesar")
            .roles(),
        theme.roles()
    );
    // A shape and roles go together.
    let both = theme.with_shape(every_corner()).unwrap();
    assert_eq!(
        (both.roles(), both.shape()),
        (&design_roles(), &every_corner())
    );
    let back = Theme::from_short_form(both.short_form()).unwrap();
    assert_eq!((back.roles(), back.shape()), (both.roles(), both.shape()));
}

#[test]
fn a_role_whose_text_falls_short_is_refused_in_light_and_in_dark() {
    let refused = |roles: Roles| match tinted().with_roles(roles) {
        Err(ImportError::Fields(fields)) => fields,
        other => panic!("{other:?}"),
    };
    // Cards too dark for the light palette's hints.
    let fields = refused(Roles::new().card(Rgb::hex(0xa0a0a0), Rgb::hex(0x121212)));
    assert!(
        fields.iter().any(|(at, what)| at == "card"
            && what.contains(" on card is ")
            && what.contains(" in light;")),
        "{fields:?}"
    );
    // A ghost button too dim to read on a dark page.
    let fields = refused(Roles::new().ghost_text(Rgb::hex(0x0a0a0a), Rgb::hex(0x404040)));
    assert!(
        fields
            .iter()
            .any(|(at, what)| at == "ghost-fg" && what.contains(" in dark;")),
        "{fields:?}"
    );
    // Text on the inverse surface that doesn't read, on a dark page.
    let fields = refused(
        Roles::new().inverse(
            Inverse::new()
                .surface(Rgb::hex(0x0a0a0a), Rgb::hex(0xffffff))
                .text(Rgb::hex(0xffffff), Rgb::hex(0xdddddd)),
        ),
    );
    assert!(
        fields.iter().any(|(at, what)| at == "inverse-fg"
            && what.starts_with("inverse-fg on inverse is ")
            && what.contains(" in dark, on the inverse surface;")),
        "{fields:?}"
    );
    // A built-in theme ships as it is.
    assert!(Theme::NEUTRAL.with_roles(design_roles()).is_err());
}

/// A page title of 32 px in the display face, 15 px card titles and stat figures in the mono
/// face.
fn a_type_scale() -> TypeScale {
    TypeScale::new()
        .page_title(
            TextStyle::px(32)
                .line(1.1)
                .weight(600)
                .tracking(-0.03)
                .face(Face::Display),
        )
        .brand(TextStyle::px(18).weight(600).tracking(-0.03))
        .card_title(TextStyle::px(15).line(1.4).tracking(-0.01))
        .stat_value(TextStyle::px(22).weight(500).face(Face::Mono))
        .h4(TextStyle::new().weight(500))
}

#[test]
fn a_type_scale_sets_each_kind_of_text_and_travels_with_the_theme() {
    let theme = agra(&Theme::NAVREE)
        .with_shape(Shape::new().type_scale(a_type_scale()))
        .unwrap();
    let said = said(&theme);
    for (name, value) in [
        ("text-page-title-size", "32px"),
        ("text-page-title-line", "1.1"),
        ("text-page-title-weight", "600"),
        ("text-page-title-tracking", "-0.03em"),
        (
            "text-page-title-face",
            "var(--font-display,var(--font-sans))",
        ),
        ("text-brand-size", "18px"),
        ("text-card-title-line", "1.4"),
        ("text-stat-value-face", "var(--font-mono)"),
        ("text-h4-weight", "500"),
    ] {
        assert!(
            said.contains(&(name.to_owned(), value.to_owned())),
            "{name}: {said:?}"
        );
    }
    assert_eq!(said.len(), 15, "{said:?}");
    let back = Theme::from_short_form(theme.short_form()).unwrap();
    assert_eq!(back.shape(), theme.shape());
    let stored = Theme::from_json(&theme.to_json()).unwrap();
    assert_eq!(stored.shape(), theme.shape());
    let rust = theme.to_rust("navree::ui");
    assert!(
        rust.contains(
            ".page_title(TextStyle::px(32).line(1.1).weight(600).tracking(-0.03).face(Face::Display))"
        ),
        "{rust}"
    );
    assert!(rust.contains(".h4(TextStyle::new().weight(500))"), "{rust}");
    assert!(
        rust.contains("Face, Font, Palette, Radius, Rgb, Shape, TextStyle, Theme, ThemeId, ThemeSpec, TypeScale}"),
        "{rust}"
    );
    // A size out of range is refused by its name.
    let big = Shape::new().type_scale(TypeScale::new().page_title(TextStyle::px(80)));
    match agra(&Theme::NAVREE).with_shape(big) {
        Err(ImportError::Fields(fields)) => assert_eq!(
            fields,
            vec![(
                "text-page-title-size".to_owned(),
                "A text size takes 10 to 72 px.".to_owned()
            )]
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_kind_of_text_is_read_by_the_stylesheet_and_headings_and_the_brand_take_the_display_face() {
    let mut every = TypeScale::new();
    let style = TextStyle::px(16)
        .line(1.5)
        .weight(500)
        .tracking(0.01)
        .face(Face::Text);
    for step in [
        TypeScale::page_title,
        TypeScale::h1,
        TypeScale::h2,
        TypeScale::h3,
        TypeScale::h4,
        TypeScale::card_title,
        TypeScale::brand,
        TypeScale::body,
        TypeScale::label,
        TypeScale::button,
        TypeScale::badge,
        TypeScale::input,
        TypeScale::breadcrumb,
        TypeScale::stat_value,
        TypeScale::table_head,
        TypeScale::sidebar_user,
    ] {
        every = step(every, style);
    }
    let theme = agra(&Theme::NAVREE)
        .with_shape(Shape::new().type_scale(every))
        .unwrap();
    let names = said(&theme);
    assert_eq!(names.len(), 16 * 5);
    let css = sources();
    for (name, _) in &names {
        assert!(
            css.contains(&format!("var(--{name}")),
            "{name} is read by no rule"
        );
    }
    // With no display face given, these draw in the text face, as before; with one, in it.
    for rule in [
        "[data-slot=page-title]{",
        "h1[data-slot=heading]{",
        "h2[data-slot=heading]{",
        "h3[data-slot=heading]{",
        "[data-slot=sidebar-brand-name]{",
        "[data-slot=site-brand]{",
    ] {
        let at = css.find(rule).unwrap_or_else(|| panic!("{rule}"));
        let body = &css[at..at + css[at..].find('}').unwrap()];
        assert!(body.contains("var(--font-display)"), "{rule}: {body}");
    }
    let at = css.find("h4[data-slot=heading]{").unwrap();
    assert!(!css[at..at + css[at..].find('}').unwrap()].contains("font-display"));
}

#[test]
fn a_font_of_the_apps_own_can_be_the_display_text_or_mono_face() {
    static PLAIN: AppFont =
        AppFont::new("tight", "Inter Tight", b"wOF2....", 1024).weights(500, 700);
    static TIGHT: AppFont = AppFont::new("tight", "Inter Tight", b"wOF2....", 1024)
        .weights(500, 700)
        .as_face(Face::Display);
    static BODY: AppFont = AppFont::new("body", "Inter", b"wOF2....", 1024).as_face(Face::Text);
    static FIGURES: AppFont =
        AppFont::new("figures", "Geist Mono", b"wOF2....", 1024).as_face(Face::Mono);
    // A font that is no face of the page keeps the rule and the address it always had.
    assert!(!PLAIN.face().contains(":root"), "{}", PLAIN.face());
    assert_eq!(PLAIN.role(), None);
    assert_eq!(TIGHT.url(), PLAIN.url(), "one file");
    assert_ne!(TIGHT.face_url(), PLAIN.face_url(), "its own rule");
    assert!(TIGHT.face().starts_with(&PLAIN.face()), "{}", TIGHT.face());
    assert!(TIGHT.face().ends_with(
        r#":root{--font-display:"Inter Tight",ui-sans-serif,system-ui,-apple-system,"Segoe UI",Roboto,sans-serif}"#
    ));
    assert!(BODY.face().contains(r#":root{--font-sans:"Inter","#));
    assert!(FIGURES.face().contains(
        r#":root{--font-mono:"Geist Mono",ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}"#
    ));
    // A page links its faces' rules; Geist is not asked for when the text face is the app's.
    let display = View::new("x").font(&TIGHT).to_html();
    assert!(display.contains(&TIGHT.face_url()));
    assert!(
        display.contains("/_navree/ui/geist-"),
        "the text is still Geist"
    );
    let text = View::new("x").font(&BODY).to_html();
    assert!(text.contains(&BODY.face_url()));
    assert!(!text.contains("/_navree/ui/geist-"), "{text:.400}");
}

/// Controls and fields 44 px high, 22 px badges, a sidebar 248 px wide with 16 px and 12 px
/// insets, a top bar 60 px high, a page padded 28, 32 and 40 px and at most 1240 px wide, and
/// cards padded 18 px.
fn shell_sizes() -> Sizes {
    Sizes::new()
        .controls(ControlSizes::new().sm(32).md(44).lg(48))
        .fields(FieldSizes::new().md(44).lg(48).padding_x(14))
        .badge_height(22)
        .sidebar(
            SidebarSizes::new()
                .width(248)
                .inset_y(16)
                .inset_x(12)
                .gap(18)
                .item_height(36),
        )
        .topbar(TopbarSizes::new().height(60).inset_x(32))
        .page(
            PageSizes::new()
                .padding_top(28)
                .padding_x(32)
                .padding_bottom(40)
                .gap(24)
                .max_width(1240),
        )
        .cards(
            CardSizes::new()
                .padding(18)
                .stat_padding_y(16)
                .stat_padding_x(18),
        )
}

#[test]
fn sizes_set_controls_fields_the_shell_and_the_page_in_pixels() {
    let theme = agra(&Theme::NAVREE)
        .with_shape(Shape::new().sizes(shell_sizes()))
        .unwrap();
    let said = said(&theme);
    for (name, value) in [
        ("control-md", "44px"),
        ("field-md", "44px"),
        ("field-px", "14px"),
        ("badge-h", "22px"),
        ("sidebar-w", "248px"),
        ("sidebar-pad-y", "16px"),
        ("sidebar-pad-x", "12px"),
        ("sidebar-gap", "18px"),
        ("sidebar-item-h", "36px"),
        ("topbar-h", "60px"),
        ("topbar-px", "32px"),
        ("page-pad-top", "28px"),
        ("page-max", "1240px"),
        ("card-pad", "18px"),
        ("stat-pad-x", "18px"),
    ] {
        assert!(
            said.contains(&(name.to_owned(), value.to_owned())),
            "{name}: {said:?}"
        );
    }
    // The token layer writes the sidebar's width and the top bar's height for every theme, so
    // a part of the page drawn in another theme gets its own from there.
    let reset = theme.css().unwrap().lines().next().unwrap().to_owned();
    assert!(reset.contains("--control-md:initial"), "{reset}");
    assert!(
        !reset.contains("--sidebar-w") && !reset.contains("--topbar-h"),
        "{reset}"
    );
    let back = Theme::from_short_form(theme.short_form()).unwrap();
    assert_eq!(back.shape(), theme.shape());
    let stored = Theme::from_json(&theme.to_json()).unwrap();
    assert_eq!(stored.shape(), theme.shape());
    let rust = theme.to_rust("navree::ui");
    for call in [
        ".controls(ControlSizes::new().sm(32).md(44).lg(48))",
        ".fields(FieldSizes::new().md(44).lg(48).padding_x(14))",
        "\n                .badge_height(22)",
        ".sidebar(SidebarSizes::new().width(248).inset_y(16).inset_x(12).gap(18).item_height(36))",
        ".topbar(TopbarSizes::new().height(60).inset_x(32))",
        ".cards(CardSizes::new().padding(18).stat_padding_y(16).stat_padding_x(18))",
        "PageSizes, Palette, Radius, Rgb, Shape, SidebarSizes, Sizes, Theme",
    ] {
        assert!(rust.contains(call), "{call}: {rust}");
    }
    // A size out of range is refused by its name.
    let narrow = Shape::new().sizes(Sizes::new().sidebar(SidebarSizes::new().width(100)));
    match agra(&Theme::NAVREE).with_shape(narrow) {
        Err(ImportError::Fields(fields)) => assert_eq!(
            fields,
            vec![(
                "sidebar-w".to_owned(),
                "This takes 160 to 400 px.".to_owned()
            )]
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_size_is_read_by_the_stylesheet() {
    let theme = agra(&Theme::NAVREE)
        .with_shape(
            Shape::new().sizes(
                shell_sizes()
                    .controls(ControlSizes::new().xs(24).sm(32).md(44).lg(48))
                    .fields(FieldSizes::new().xs(24).sm(32).md(44).lg(48).padding_x(14)),
            ),
        )
        .unwrap();
    let names = said(&theme);
    assert_eq!(names.len(), 25, "{names:?}");
    let css = sources();
    for (name, _) in &names {
        assert!(
            css.contains(&format!("var(--{name}")),
            "{name} is read by no rule"
        );
    }
}
