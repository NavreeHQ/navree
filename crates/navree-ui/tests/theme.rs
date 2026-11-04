//! Themes from outside, in volume: random and mutated colours, short forms,
//! settings, stored themes and builder queries through every parser, with the
//! invariants the fuzz target (`fuzz/fuzz_targets/ui_theme.rs`) checks for
//! longer. And pages: what a View, a shell layout and the scope levels write.

use http::header::{COOKIE, HeaderMap, HeaderValue};
use navree_ui::theme::{
    Appearance, BuilderDraft, Mode, Rgb, Theme, ThemeId, ThemeMenu, ThemeSetting,
};
use navree_ui::{
    Collapse, NavGroup, NavItem, Shell, ShellLayout, UserMenu, View, el, icons, render,
};

/// A small, seeded generator: the same inputs on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// Text that is mostly the alphabet parsers care about, with some hostile pieces.
    fn text(&mut self, most: usize) -> String {
        const PIECES: &[&str] = &[
            "#",
            "0",
            "9",
            "a",
            "f",
            "F",
            "g",
            "z",
            "rgb(",
            ")",
            ",",
            " ",
            "-",
            ".",
            "v1",
            "v1-",
            "0406",
            "d",
            "g",
            ";",
            "}",
            "{",
            "<",
            ">",
            "\"",
            "'",
            "\\",
            "/",
            "%",
            "=",
            "&",
            "é",
            "\u{0}",
            "\n",
            "ocean",
            "navree",
            "light",
            "dark",
            "system",
            "custom",
            "px",
            "url(",
            "javascript:",
            "</style>",
            "<script>",
            "__proto__",
            "255",
            "256",
            "-1",
            "1.5",
            "ink-0",
            "d.",
            "l.",
            "fg-3",
            "accent",
            "base=",
        ];
        let len = self.below(most + 1);
        let mut out = String::new();
        for _ in 0..len {
            out.push_str(PIECES[self.below(PIECES.len())]);
        }
        out
    }

    /// `text` with a few bytes changed, cut or doubled.
    fn mutate(&mut self, text: &str) -> String {
        let mut chars: Vec<char> = text.chars().collect();
        for _ in 0..=self.below(3) {
            if chars.is_empty() {
                break;
            }
            let at = self.below(chars.len());
            match self.below(4) {
                0 => chars[at] = char::from(b"0f9ag-#;}<\" "[self.below(12)]),
                1 => {
                    chars.remove(at);
                }
                2 => chars.insert(at, char::from(b"0f-v."[self.below(5)])),
                _ => chars.truncate(at),
            }
        }
        chars.into_iter().collect()
    }
}

/// What a theme's stylesheet may hold.
fn clean_css(css: &str) -> bool {
    css.chars()
        .all(|c| c.is_ascii_alphanumeric() || " \n#%(),-.:;=[]{}/'\"@_+*".contains(c))
        && !css.contains("</")
}

fn check_theme(theme: &Theme) {
    let back = Theme::from_short_form(theme.short_form()).expect("reads back");
    assert_eq!(back.spec(), theme.spec());
    if let Some(css) = theme.css() {
        assert!(clean_css(&css), "{css}");
    }
    assert!(theme.key().bytes().all(|b| b.is_ascii_alphanumeric()));
}

#[test]
fn colours_from_anything_never_panic_and_read_back_when_they_parse() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    const HEX: &[u8] = b"0123456789abcdefABCDEFgz;} ";
    let mut parsed = 0;
    for round in 0..60_000 {
        let text = match round % 3 {
            0 => rng.text(8),
            1 => {
                let len = rng.below(9);
                let mut out = String::from(if rng.below(8) == 0 { "" } else { "#" });
                for _ in 0..len {
                    out.push(char::from(HEX[rng.below(HEX.len())]));
                }
                out
            }
            _ => {
                let part = |rng: &mut Rng| match rng.below(5) {
                    0 => String::new(),
                    1 => "256".to_owned(),
                    2 => "-1".to_owned(),
                    _ => rng.below(256).to_string(),
                };
                let sep = [", ", " ", ",", " , "][rng.below(4)];
                format!(
                    "rgb({}{sep}{}{sep}{})",
                    part(&mut rng),
                    part(&mut rng),
                    part(&mut rng)
                )
            }
        };
        if let Ok(colour) = Rgb::parse(&text) {
            parsed += 1;
            let hex = colour.to_hex();
            assert_eq!(Rgb::parse(&hex), Ok(colour), "{text:?}");
            assert!(
                hex.len() == 7
                    && hex[1..]
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            );
        }
    }
    assert!(
        parsed > 1_000,
        "the generator reaches valid colours: {parsed}"
    );
}

#[test]
fn short_forms_and_settings_from_anything_and_from_mutations() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let forms: Vec<String> = Theme::BUILT_IN
        .iter()
        .map(|theme| {
            theme
                .renamed(ThemeId::parse("x").unwrap(), "X")
                .short_form()
                .to_owned()
        })
        .collect();
    let mut kept = 0;
    for round in 0..40_000 {
        let text = if round % 2 == 0 {
            rng.text(12)
        } else {
            let at = rng.below(forms.len());
            rng.mutate(&forms[at])
        };
        if let Some(theme) = Theme::from_short_form(&text) {
            kept += 1;
            assert_eq!(theme.short_form(), text, "one spelling per theme");
            check_theme(&theme);
        }
        if let Some(setting) = ThemeSetting::parse(&format!(
            "{}.{text}",
            ["light", "dark", "system", "x"][round % 4]
        )) {
            if let (Some(theme), Some(mode)) = (setting.theme, setting.mode) {
                let appearance = Appearance::new(theme, mode);
                assert_eq!(
                    Appearance::parse(&appearance.to_value()).map(|a| *a.theme().spec()),
                    Some(*appearance.theme().spec())
                );
            }
        }
    }
    assert!(kept > 0, "some mutations are still themes");
}

#[test]
fn stored_themes_from_mutations_never_panic_and_read_back() {
    let mut rng = Rng(0x0123_4567_89ab_cdef);
    let good = Theme::custom(
        ThemeId::parse("acme").unwrap(),
        "Acme",
        *Theme::OCEAN.spec(),
    )
    .unwrap()
    .to_json();
    let mut kept = 0;
    for round in 0..4_000 {
        let text = if round % 4 == 0 {
            rng.text(40)
        } else {
            rng.mutate(&good)
        };
        if let Ok(theme) = Theme::from_json(&text) {
            kept += 1;
            let again = Theme::from_json(&theme.to_json()).unwrap();
            assert_eq!(again.spec(), theme.spec());
            check_theme(&theme);
        }
    }
    assert!(kept > 0, "some mutations are still themes");
}

#[test]
fn builder_queries_from_anything_read_back_from_their_own_query() {
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    let good = "base=ocean&accent=custom&accent_hex=%23ff8800&neutral=stone&radius=round&density=compact&font=serif&layout=inset&mode=light&d.fg-3=%23aabbcc&l.accent=%23123456&id=acme&name=Acme";
    for round in 0..20_000 {
        let text = if round % 2 == 0 {
            rng.text(16)
        } else {
            rng.mutate(good)
        };
        let draft = BuilderDraft::from_query(&text);
        let spec = draft.spec();
        assert_eq!(
            BuilderDraft::from_query(&draft.to_query()).spec(),
            spec,
            "{text:?}"
        );
        if let Some(css) = draft.draft().css() {
            assert!(clean_css(&css));
        }
        if let Ok(theme) = draft.to_theme() {
            check_theme(&theme);
        }
    }
}

fn shell() -> Shell {
    Shell::new("Acme")
        .group(NavGroup::plain().item(NavItem::new("Home", "/").icon(icons::HOUSE)))
        .group(
            NavGroup::new("Sales")
                .item(NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE))
                .item(
                    NavItem::new("Reports", "/reports")
                        .icon(icons::CHART_COLUMN)
                        .sub(NavItem::new("Pipeline", "/reports/pipeline")),
                ),
        )
        .user(UserMenu::new("Ada", "ada@example.com").action("Sign out", "/sign-out"))
}

#[test]
fn a_page_with_nothing_set_is_as_it_always_was() {
    let html = View::new("x").body(el("main").child("hi")).to_html();
    assert!(
        html.starts_with(r#"<!doctype html><html lang="en"><head>"#),
        "{html}"
    );
    assert!(html.contains(r#"<meta name="color-scheme" content="dark">"#));
    assert!(!html.contains("data-ui-theme"));
    let page = shell().page("Deals", "/deals").to_view().to_html();
    assert!(
        page.starts_with(r#"<!doctype html><html lang="en" data-build="#),
        "{page:.120}"
    );
}

#[test]
fn a_page_carries_its_theme_mode_density_and_sheet_from_the_first_byte() {
    let mine = Theme::custom(ThemeId::parse("acme").unwrap(), "Acme", {
        let mut spec = *Theme::ROSE.spec();
        spec.density = navree_ui::theme::Density::Compact;
        spec.font = navree_ui::theme::Font::Serif;
        spec
    })
    .unwrap();
    let html = View::new("x")
        .appearance(Appearance::new(mine.clone(), Mode::System))
        .body(el("main").child("hi"))
        .to_html();
    let head = html.split("</head>").next().unwrap();
    assert!(head.starts_with(&format!(r#"<!doctype html><html lang="en" data-theme="{}" data-mode="system" data-density="compact">"#, mine.key())), "{head}");
    assert!(
        head.contains(r#"<meta name="color-scheme" content="light dark">"#),
        "{head}"
    );
    assert!(
        head.contains(&format!(
            r#"<link rel="stylesheet" href="{}" data-ui-theme>"#,
            mine.stylesheet().unwrap()
        )),
        "{head}"
    );
    assert!(
        !head.contains("geist"),
        "no Geist preload for a serif theme: {head}"
    );
    assert!(!html.contains("<script"), "a theme needs no script");
    // A built-in needs no sheet of its own.
    let ocean = View::new("x")
        .appearance(Appearance::new(Theme::OCEAN, Mode::Light))
        .to_html();
    assert!(ocean.contains(r#"data-theme="ocean" data-mode="light""#));
    assert!(!ocean.contains("data-ui-theme"));
}

#[test]
fn each_layout_and_folding_is_written_on_the_wrapper() {
    for (layout, attr) in [
        (ShellLayout::Sidebar, None),
        (ShellLayout::Inset, Some(r#"data-layout="inset""#)),
        (ShellLayout::Floating, Some(r#"data-layout="floating""#)),
        (ShellLayout::Header, Some(r#"data-layout="header""#)),
    ] {
        let html = shell()
            .layout(layout)
            .page("Deals", "/deals")
            .to_view()
            .to_html();
        match attr {
            Some(attr) => assert!(html.contains(attr), "{layout:?}"),
            None => assert!(!html.contains("data-layout"), "{layout:?}"),
        }
    }
    let html = shell()
        .collapse(Collapse::Offcanvas)
        .page("x", "/")
        .to_view()
        .to_html();
    assert!(html.contains(r#"data-collapse="offcanvas""#));
    // A sidebar that never folds ignores the folded cookie.
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_static("navree_ui_sidebar=collapsed"),
    );
    let html = shell()
        .collapse(Collapse::Never)
        .for_request("/", &headers)
        .page("x")
        .to_view()
        .to_html();
    assert!(html.contains(r#"data-collapse="never">"#), "{html:.300}");
}

#[test]
fn the_header_layout_puts_a_labelled_group_in_one_menu_and_the_user_in_the_top_bar() {
    let html = shell()
        .layout(ShellLayout::Header)
        .page("Pipeline", "/reports/pipeline")
        .to_view()
        .to_html();
    let topbar = html
        .split(r#"data-slot="topbar""#)
        .nth(1)
        .unwrap()
        .split("</header>")
        .next()
        .unwrap();
    assert!(
        topbar.contains(r#"<nav data-slot="topnav" aria-label="Main">"#),
        "{topbar}"
    );
    assert!(
        topbar.contains(r#"<a data-slot="topnav-item" href="/""#),
        "an unlabelled group's link stands in the bar: {topbar}"
    );
    assert!(
        topbar.contains(r#"popovertarget="ui-topnav-1" data-active"#),
        "the group with the current page is marked: {topbar}"
    );
    assert!(
        topbar.contains(r#"href="/reports/pipeline" aria-current="page""#),
        "{topbar}"
    );
    assert!(topbar.contains(r#"data-slot="topbar-user""#), "{topbar}");
    assert!(
        !html.contains(r#"data-slot="sidebar-footer""#),
        "the user is in the top bar, not the sheet"
    );
}

#[test]
fn the_levels_lay_over_each_other_and_a_cookie_or_url_picks_only_what_is_offered() {
    let offered = shell().themes([Theme::NAVREE, Theme::OCEAN, Theme::ROSE]);
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_static("navree_ui_theme=light.rose"),
    );
    let look = |shell: navree_ui::AppShell| shell.appearance();
    assert_eq!(
        look(offered.for_request("/", &headers)),
        Appearance::new(Theme::ROSE, Mode::Light)
    );
    assert_eq!(
        look(offered.for_request("/?theme=ocean&mode=system", &headers)),
        Appearance::new(Theme::OCEAN, Mode::System),
        "the URL over the cookie"
    );
    assert_eq!(
        look(offered.for_request("/?theme=violet", &headers)),
        Appearance::new(Theme::ROSE, Mode::Light),
        "a theme the app doesn't offer is ignored"
    );
    let locked = offered
        .for_request("/?theme=ocean", &headers)
        .workspace(ThemeSetting::theme(Theme::SUNSET).locked());
    assert_eq!(
        look(locked),
        Appearance::new(Theme::SUNSET, Mode::Light),
        "locked: the workspace's theme, the reader's mode"
    );
    let stored = offered
        .for_request("/", &headers)
        .user(ThemeSetting::appearance(Appearance::new(
            Theme::CONTRAST,
            Mode::Dark,
        )));
    assert_eq!(
        look(stored),
        Appearance::new(Theme::CONTRAST, Mode::Dark),
        "the app's store over the cookie, offered or not"
    );
}

#[test]
fn a_shell_with_the_theme_menu_links_the_sheets_of_the_themes_it_offers() {
    let mine = Theme::custom(
        ThemeId::parse("acme").unwrap(),
        "Acme",
        *Theme::VIOLET.spec(),
    )
    .unwrap();
    let html = shell()
        .theme_menu()
        .themes([Theme::NAVREE, mine.clone()])
        .page("x", "/")
        .to_view()
        .to_html();
    assert!(html.contains(&format!(
        r#"href="{}" data-ui-theme"#,
        mine.stylesheet().unwrap()
    )));
    assert!(html.contains(r#"popovertarget="ui-theme-menu""#));
    let menu = render(&ThemeMenu::new(&Appearance::default(), &[Theme::NAVREE]).into());
    assert!(!menu.contains("style="));
}
