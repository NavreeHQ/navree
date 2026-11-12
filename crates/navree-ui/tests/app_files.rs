//! The app's own stylesheet and fonts: served by the toolkit under its mount, linked by a
//! page after the toolkit's files and its theme, and a brand theme that keeps the
//! toolkit's contrast checks.

use navree_http::HttpModule;
use navree_kernel::{Module, ModuleBuilder};
use navree_testing::{TestApp, TestAppBuilder};
use navree_ui::theme::{Appearance, Mode, Rgb, Side, Theme, ThemeId, ThemeSpec};
use navree_ui::{
    AppFont, AppSheet, FontDisplay, NavGroup, NavItem, PAGE_CSP, Shell, UiModule, View, el, icons,
};

static BRAND: AppSheet = AppSheet::new(
    "brand",
    b".hero{background:linear-gradient(135deg,var(--accent),var(--ink-2))}",
    4 * 1024,
);
static DISPLAY: AppFont = AppFont::new("display", "Fraunces", b"wOF2\0\0\0\x20fontbytes", 4 * 1024)
    .weights(400, 700)
    .display(FontDisplay::Swap);

struct Brand;

impl Module for Brand {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(BRAND);
        m.contribute(DISPLAY);
    }
}

fn builder() -> TestAppBuilder {
    TestApp::builder()
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(HttpModule::for_root(Default::default()))
}

#[tokio::test]
async fn the_apps_sheet_and_font_are_served_by_hash_under_the_toolkits_mount() {
    let app = builder().module(UiModule).module(Brand).build().await;

    let sheet = app
        .client()
        .get(&BRAND.url())
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        sheet.header("content-type"),
        Some("text/css; charset=utf-8")
    );
    assert_eq!(
        sheet.header("cache-control"),
        Some("public, max-age=31536000, immutable")
    );
    assert!(sheet.text().contains("linear-gradient"));

    let font = app
        .client()
        .get(&DISPLAY.url())
        .send()
        .await
        .assert_status(200);
    assert_eq!(font.header("content-type"), Some("font/woff2"));

    let rule = app
        .client()
        .get(&DISPLAY.face_url())
        .send()
        .await
        .assert_status(200);
    let rule = rule.text();
    assert!(
        rule.starts_with("@font-face{font-family:\"Fraunces\""),
        "{rule}"
    );
    assert!(rule.contains(&DISPLAY.url()) && rule.contains("font-display:swap"));

    // Only what the app contributed, by name; nothing a path can reach out of the mount.
    for path in [
        "/_navree/ui/app/other-000000000000.css",
        "/_navree/ui/app/brand.css",
        "/_navree/ui/app/brand-000000000000.js",
    ] {
        app.client().get(path).send().await.assert_status(404);
    }
    app.client()
        .get("/_navree/ui/app/..%2Fcore.js")
        .send()
        .await
        .assert_status(400);
}

#[tokio::test]
async fn an_app_that_contributes_nothing_serves_nothing() {
    let app = builder().module(UiModule).build().await;
    app.client()
        .get(&BRAND.url())
        .send()
        .await
        .assert_status(404);
}

/// The policy a page ships with allows exactly what the files are: a stylesheet and a font from
/// the page's own origin, and nothing inline.
#[test]
fn the_policy_allows_the_apps_files_and_nothing_inline() {
    assert!(PAGE_CSP.contains("style-src 'self'"));
    assert!(PAGE_CSP.contains("font-src 'self'"));
    assert!(BRAND.url().starts_with('/') && DISPLAY.url().starts_with('/'));
    let html = View::new("x")
        .stylesheet(&BRAND)
        .font(&DISPLAY)
        .body(el("p").child("hi"))
        .to_html();
    assert!(
        !html.contains("<style") && !html.contains(" style="),
        "{html}"
    );
}

#[test]
fn a_shell_links_its_files_on_every_page_and_a_page_adds_its_own() {
    static EXTRA: AppSheet = AppSheet::new("extra", b".x{color:red}", 1024);
    let shell = Shell::new("Fitness")
        .stylesheet(&BRAND)
        .font(&DISPLAY)
        .group(NavGroup::new("Today").item(NavItem::new("Home", "/").icon(icons::HOUSE)));
    let plain = shell.page("Home", "/").body("hi").to_view().to_html();
    assert!(plain.contains(&BRAND.url()) && plain.contains(&DISPLAY.face_url()));
    assert!(!plain.contains(&EXTRA.url()));
    let extra = shell
        .page("Home", "/")
        .stylesheet(&EXTRA)
        .stylesheet(&BRAND)
        .body("hi")
        .to_view()
        .to_html();
    assert_eq!(extra.matches(BRAND.url().as_str()).count(), 1);
    assert!(extra.find(&BRAND.url()) < extra.find(&EXTRA.url()));
}

/// A brand's accent can be any colour: `with_accent` derives the text on it, the accent as text
/// and the focus ring, so the theme keeps the toolkit's WCAG AA text pairs in light and dark.
/// Whatever a brand picks, a theme either passes or is refused; none ships failing.
#[test]
fn a_brand_theme_keeps_the_toolkits_contrast_checks() {
    let accents = [
        (0x2dd4bf, 0x0f766e), // teal
        (0xff7a45, 0xc2410c), // orange
        (0xffe14d, 0x92400e), // a pale yellow, where white text fails
        (0x1e1b4b, 0x1e1b4b), // a deep indigo that is dark in both modes
        (0xf472b6, 0xbe185d), // pink
        (0x7c5cff, 0x6d28d9),
    ];
    for (at, (dark, light)) in accents.into_iter().enumerate() {
        let spec: ThemeSpec = Theme::NAVREE
            .spec()
            .with_accent(Rgb::hex(dark), Rgb::hex(light));
        let theme = Theme::custom(
            ThemeId::parse(&format!("brand-{at}")).unwrap(),
            "Brand",
            spec,
        );
        let theme = theme.unwrap_or_else(|e| panic!("accent {dark:06x}/{light:06x}: {e:?}"));
        assert!(theme.spec().text_passes());
        // The accent is what was asked for, and what reads on it is derived.
        assert_eq!(
            theme
                .spec()
                .palette(Side::Dark)
                .get(navree_ui::theme::Token::Accent),
            Rgb::hex(dark)
        );
        // The look reaches the page through the theme's own sheet.
        let html = View::new("x")
            .appearance(Appearance::new(theme.clone(), Mode::System))
            .stylesheet(&BRAND)
            .to_html();
        assert!(html.contains(&theme.stylesheet().unwrap()), "{html}");
        assert!(
            html.find(&theme.stylesheet().unwrap()) < html.find(&BRAND.url()),
            "the app's sheet follows the theme's"
        );
    }
    // A palette that fails is refused, not shipped.
    let mut spec = *Theme::NAVREE.spec();
    spec.dark
        .set(navree_ui::theme::Token::Fg2, Rgb::hex(0x1b1b1b));
    assert!(Theme::custom(ThemeId::parse("bad").unwrap(), "Bad", spec).is_err());
}
