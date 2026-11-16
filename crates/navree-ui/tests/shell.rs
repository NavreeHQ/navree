//! The application shell's options, and that a shell which uses none of them draws exactly
//! what it drew before they existed.
//!
//! The pins below were taken before the brand row, the sidebar's foot, the top bar and the
//! page header took options of their own: the markup of the repository's own example shells
//! (the store, the board, the inbox, the theme demo, the assistant demo, the proof app and
//! the docs' CRM), in each layout and fold, with a request's user, slot, token, toast and
//! cookies. A change to any of them is a change to every app that sets nothing new.

use http::HeaderMap;
use http::header::{COOKIE, HeaderValue};
use navree_ui::theme::{Appearance, Density, Mode, Theme};
use navree_ui::{
    AppPage, Avatar, AvatarGroup, Badge, BadgeVariant, Button, Card, Collapse, NavGroup, NavItem,
    Shell, ShellLayout, Size, Stat, ToastPosition, UserMenu, el, icons,
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

/// The store demo's shell.
fn store() -> Shell {
    Shell::new("Rangla Store")
        .context("Store operations")
        .group(
            NavGroup::new("Store")
                .item(NavItem::new("Dashboard", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(NavItem::new("Orders", "/orders").icon(icons::SHOPPING_BAG))
                .item(NavItem::new("Inventory", "/inventory").icon(icons::BOXES)),
        )
        .user(
            UserMenu::new("Ravneet Kaur", "ravneet.kaur@example.com")
                .link("Profile", "/")
                .action("Sign out", "/sign-out"),
        )
}

/// The board demo's shell.
fn board() -> Shell {
    Shell::new("Rangla Looms")
        .context("Product")
        .group(
            NavGroup::new("Spring collection")
                .item(NavItem::new("Board", "/").icon(icons::KANBAN))
                .item(NavItem::new("Due this week", "/?due=week").icon(icons::CALENDAR_CLOCK))
                .item(NavItem::new("Bugs", "/?label=bug").icon(icons::BUG)),
        )
        .user(
            UserMenu::new("Rangla Looms product team", "team@example.com")
                .action("Sign out", "/sign-out"),
        )
}

/// The inbox demo's shell: a live group of counted views, the current one said by the
/// app, a home of its own and toasts at the top.
fn inbox() -> Shell {
    let menu = UserMenu::new("Priya Verma", "priya@example.com")
        .action("Work as Harjot Singh", "/as/2")
        .action("Work as Simran Arora", "/as/3");
    let group = NavGroup::new("Inbox")
        .item(
            NavItem::new("All", "/inbox?view=all")
                .icon(icons::INBOX)
                .current(false)
                .count("1,240"),
        )
        .item(
            NavItem::new("Mine", "/inbox?view=mine")
                .icon(icons::USER_ROUND)
                .current(true)
                .count("12"),
        )
        .item(
            NavItem::new("Closed", "/inbox?view=closed")
                .icon(icons::ARCHIVE)
                .current(false),
        )
        .live(
            "views",
            "/inbox/fragments/views",
            "/inbox/events",
            Some("conversations"),
        );
    Shell::new("Sutlej Support")
        .context("Support")
        .home("/inbox")
        .toasts(ToastPosition::TopCenter)
        .group(group)
        .user(menu)
}

/// The theme demo's shell: the theme menu, a home of its own and an item with items below it.
fn theme_demo() -> Shell {
    Shell::new("Sutlej CRM")
        .context("Theme demo")
        .home("/themes/builder")
        .theme_menu()
        .group(
            NavGroup::new("Look")
                .item(NavItem::new("Theme builder", "/themes/builder").icon(icons::SWATCH_BOOK))
                .item(NavItem::new("Themes", "/themes").icon(icons::PALETTE))
                .item(
                    NavItem::new("Layouts", "/themes/layouts/sidebar")
                        .icon(icons::PANEL_LEFT)
                        .sub(NavItem::new("Sidebar", "/themes/layouts/sidebar"))
                        .sub(NavItem::new("Inset", "/themes/layouts/inset"))
                        .sub(NavItem::new("Floating", "/themes/layouts/floating"))
                        .sub(NavItem::new("Header", "/themes/layouts/header")),
                ),
        )
}

/// The assistant demo's shell: a group with no label.
fn assistant() -> Shell {
    Shell::new("Kesar Foods")
        .context("Back office")
        .group(
            NavGroup::plain()
                .item(NavItem::new("Assistant", "/assistant").icon(icons::BOT))
                .item(NavItem::new("Orders", "/orders").icon(icons::RECEIPT)),
        )
        .user(UserMenu::new("Account", "Signed in").action("Sign out", "/sign-out"))
}

/// The proof app's shell: two groups, a count and a link fetched ahead.
fn proof() -> Shell {
    Shell::new("Navree CRM")
        .context("Proof pages")
        .group(
            NavGroup::new("Sales")
                .item(NavItem::new("Overview", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(
                    NavItem::new("Deals", "/deals?per_page=1000")
                        .icon(icons::HANDSHAKE)
                        .count("1000"),
                )
                .item(NavItem::new("New deal", "/deals/new").icon(icons::CIRCLE_PLUS)),
        )
        .group(
            NavGroup::new("System")
                .item(NavItem::new("Plain page", "/plain").icon(icons::FILE))
                .item(
                    NavItem::new("Gallery", "/gallery")
                        .icon(icons::LAYOUT_GRID)
                        .prefetch(),
                ),
        )
        .user(
            UserMenu::new("Anu Lakhotia", "anu@navree.test")
                .link("Settings", "/plain")
                .action("Sign out", "/sign-out"),
        )
}

/// The proof app's own shell: the app's mark, the links in the top bar, one theme, dark, the
/// theme menu and a title template.
fn own() -> Shell {
    Shell::new("Kesar")
        .logo("/brand/kesar.svg")
        .layout(ShellLayout::Header)
        .themes([Theme::OCEAN])
        .appearance(Appearance::new(Theme::OCEAN, Mode::Dark))
        .theme_menu()
        .title_template("{} · Kesar")
        .group(
            NavGroup::plain()
                .item(NavItem::new("Today", "/app-blocks/shell").icon(icons::HOUSE))
                .item(NavItem::new("Sleep", "/app-blocks?case=dock").icon(icons::MOON)),
        )
        .group(
            NavGroup::new("More")
                .item(NavItem::new("Heart", "/app-blocks?case=tiles").icon(icons::HEART_PULSE)),
        )
}

/// The docs' CRM, with a header slot.
fn crm() -> Shell {
    Shell::new("Cloudworks CRM")
        .context("Production")
        .prefetch_home()
        .header_slot(el("span").child("Every workspace"))
        .search(
            el("input")
                .attr("type", "search")
                .attr("aria-label", "Search"),
        )
        .group(
            NavGroup::new("Sales")
                .item(NavItem::new("Overview", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(
                    NavItem::new("Deals", "/deals")
                        .icon(icons::HANDSHAKE)
                        .count("312"),
                )
                .item(
                    NavItem::new("Reports", "/reports")
                        .icon(icons::CHART_COLUMN)
                        .sub(NavItem::new("Pipeline", "/reports/pipeline"))
                        .sub(NavItem::new("Forecast", "/reports/forecast")),
                ),
        )
        .group(
            NavGroup::new("Workspace")
                .item(NavItem::new("Settings", "/settings").icon(icons::SETTINGS)),
        )
        .user(
            UserMenu::new("Gurpreet Singh", "gurpreet@example.com")
                .link("Profile", "/profile")
                .action("Sign out", "/sign-out"),
        )
}

/// What a page holds: a card, a stat, badges and avatars.
fn body(page: AppPage) -> AppPage {
    page.body((
        Card::titled("Today").body(Stat::new("Orders", "1,240")),
        Badge::new("Paid").variant(BadgeVariant::Success),
        Badge::new("Draft"),
        Avatar::placeholder("Simran Kaur").size(Size::Sm),
        Avatar::image("/avatars/harjot.webp", "Harjot Singh"),
        AvatarGroup::new("Reviewers").max(2).children((
            Avatar::placeholder("Gurpreet Kaur"),
            Avatar::placeholder("Harjot Singh"),
            Avatar::placeholder("Simran Arora"),
        )),
    ))
}

/// A request with the sidebar folded, a toast left for it and the session's token.
fn request() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_static(
            "navree_ui_sidebar=collapsed; navree_ui_flash=warning:Almost%20full; XSRF-TOKEN=tok",
        ),
    );
    headers
}

/// Every example page, by name.
fn examples() -> Vec<(&'static str, String)> {
    let html = |page: AppPage| page.to_view().to_html();
    let mut out = vec![
        (
            "store",
            html(body(
                store()
                    .page("Orders", "/orders")
                    .crumb("Dashboard", "/")
                    .actions(Button::primary("Export").href("/orders.csv")),
            )),
        ),
        ("board", html(body(board().page("Board", "/?due=week")))),
        (
            "inbox",
            html(body(
                inbox()
                    .for_request("/inbox?view=mine", &request())
                    .page("Mine"),
            )),
        ),
        (
            "assistant",
            html(assistant().page("Assistant", "/assistant")),
        ),
        (
            "proof",
            html(body(
                proof()
                    .page("Deals", "/deals")
                    .crumb("Overview", "/")
                    .prefetch_crumbs()
                    .actions((
                        Button::primary("New deal").href("/deals/new"),
                        Button::new("Export"),
                    )),
            )),
        ),
        (
            "own",
            html(
                own()
                    .for_request("/app-blocks/shell", &HeaderMap::new())
                    .page("Good evening, Simran")
                    .document_title("Today")
                    .crumb("Home", "/")
                    .topbar(el("nav").attr("aria-label", "Day").child("Tue 11 Nov")),
            ),
        ),
        (
            "crm",
            html(body(
                crm()
                    .for_request("/deals/12?stage=won&theme=ocean", &HeaderMap::new())
                    .user_menu(
                        UserMenu::new("Harleen Kaur", "harleen@example.com")
                            .action("Sign out", "/sign-out"),
                    )
                    .header_slot(el("span").child("Ludhiana desk"))
                    .csrf("tok")
                    .page("Deal 12")
                    .crumb("Deals", "/deals")
                    .density(Density::Compact),
            )),
        ),
        (
            "crm, not found",
            html(
                crm()
                    .page("Not found", "/nowhere")
                    .status(http::StatusCode::NOT_FOUND),
            ),
        ),
    ];
    for layout in ShellLayout::ALL {
        out.push((
            layout.id(),
            html(body(
                theme_demo()
                    .for_request("/themes/layouts/inset", &request())
                    .page("Layouts")
                    .layout(layout)
                    .crumb("Themes", "/themes"),
            )),
        ));
    }
    for (name, collapse) in [
        ("never", Collapse::Never),
        ("offcanvas", Collapse::Offcanvas),
    ] {
        out.push((
            name,
            html(body(
                store()
                    .collapse(collapse)
                    .for_request("/inventory", &request())
                    .page("Inventory"),
            )),
        ));
    }
    out
}

/// The SHA-256 of each example page's markup, its asset hashes masked, taken before the
/// shell's new options.
const PINS: [(&str, &str); 14] = [
    (
        "store",
        "d99e05fb2f54b38c4c435accb663ee27a9ce0ab478de68e09edb50927acbbc1a",
    ),
    (
        "board",
        "328c70b97b08fa0ed5a7371d66975fc94fd91f3aab78543ce7e6ca5adf8908fc",
    ),
    (
        "inbox",
        "bd962cde8c536d7c6dbcf9aa496317c2d6882a714f92dfb1b79c7ff1283713ad",
    ),
    (
        "assistant",
        "c6b8951ffd27d561b157280fbf7e84e5779f504ae53b823f61883dfa1ce12f85",
    ),
    (
        "proof",
        "1196679e55a1cf5115bb060d19451c107c81f776792273f0540114acd5784bb2",
    ),
    (
        "own",
        "2c554dc6e9dfb3dc0e3fb1461ad4218a6bd3363b264157276e40a56bdf2989aa",
    ),
    (
        "crm",
        "b1fcebc4ab9962e2cafb92563947b768f11a0507712d34312d3472793d07b5a5",
    ),
    (
        "crm, not found",
        "6ea47147524074f369362c70148ea0aa18c676c987eaf554caa72a6c49398795",
    ),
    (
        "sidebar",
        "0899737bca94431c661f6af82354bdc04fa8c318a3328355bcada47cd2753105",
    ),
    (
        "inset",
        "98d258881a45f1f14c83b15b23ecc59cf7b092612303b02b05c1a50b4e3db1de",
    ),
    (
        "floating",
        "1e685b434ef143499bc74dbc8f1a10828f6269a67518dab10a9741f46011ee27",
    ),
    (
        "header",
        "57a8c31ffd8cc33ab6a6608011fd96f3e92b0412d9da5d2e716df6d9b2524121",
    ),
    (
        "never",
        "5557f7a01e8c36df7c4839dc8a5cf66acbed7bd65237d6f8b99f3e90235e5756",
    ),
    (
        "offcanvas",
        "99f1373e207b3c0bc63c948e04e10e75d77351e563fab2f549d636e2ed690977",
    ),
];

#[test]
fn a_shell_that_uses_no_new_option_draws_what_it_drew_before() {
    let examples = examples();
    assert_eq!(examples.len(), PINS.len());
    let mut changed = Vec::new();
    for ((name, html), (pinned, sum)) in examples.iter().zip(PINS) {
        assert_eq!(*name, pinned);
        let markup = unhashed(html);
        if sha(&markup) != sum {
            changed.push(format!("(\"{name}\", \"{}\"):\n{markup}", sha(&markup)));
        }
    }
    assert!(changed.is_empty(), "{}", changed.join("\n\n"));
}

/// A page of `shell`, as HTML.
fn drawn(shell: &Shell) -> String {
    shell.page("Deals", "/deals").to_view().to_html()
}

/// What a page of `shell` loads.
fn loads(shell: &Shell) -> navree_ui::Uses {
    shell.page("Deals", "/deals").to_view().uses()
}

/// The source of the shell parts stylesheet.
fn shell_parts() -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles/41-shell-parts.css"),
    )
    .unwrap()
    .replace("\r\n", "\n")
}

// The brand row

#[test]
fn a_bare_mark_has_no_tile_and_brings_the_shell_parts() {
    let bare = store()
        .logo("/brand/rangla.svg")
        .logo_style(navree_ui::LogoStyle::Bare);
    let html = drawn(&bare);
    assert!(
        html.contains(r#"<a data-slot="sidebar-brand" href="/" title="Rangla Store" data-logo="bare"><span data-slot="sidebar-logo"><img src="/brand/rangla.svg" alt=""></span>"#),
        "{html}"
    );
    assert!(loads(&bare).has(navree_ui::Uses::SHELL_PARTS));
    let tile = store().logo_style(navree_ui::LogoStyle::Tile);
    assert!(!drawn(&tile).contains("data-logo"));
    assert!(!loads(&tile).has(navree_ui::Uses::SHELL_PARTS));
    assert_eq!(navree_ui::LogoStyle::default(), navree_ui::LogoStyle::Tile);
    let css = shell_parts();
    let at = css
        .find("[data-slot=sidebar-brand][data-logo=bare]{")
        .unwrap();
    let rule = &css[at..at + css[at..].find('\n').unwrap()];
    for part in [
        "gap:.625rem",
        "padding:.25rem .5rem",
        "background:none",
        "width:1.375rem;height:1.375rem",
    ] {
        assert!(rule.contains(part), "{part} in {rule}");
    }
}

#[test]
fn a_dark_mark_is_drawn_in_dark_and_chosen_by_the_device_when_the_page_follows_it() {
    let shell = |mode: Mode| {
        store()
            .logo("/brand/light.svg")
            .logo_dark("/brand/dark.svg")
            .appearance(Appearance::new(Theme::NAVREE, mode))
    };
    let light = drawn(&shell(Mode::Light));
    assert!(
        light.contains(
            r#"<span data-slot="sidebar-logo"><img src="/brand/light.svg" alt=""></span>"#
        ),
        "{light}"
    );
    assert!(!light.contains("dark.svg"), "{light}");
    let dark = drawn(&shell(Mode::Dark));
    assert!(
        dark.contains(
            r#"<span data-slot="sidebar-logo"><img src="/brand/dark.svg" alt=""></span>"#
        ),
        "{dark}"
    );
    assert!(!dark.contains("light.svg"), "{dark}");
    assert!(!loads(&shell(Mode::Dark)).has(navree_ui::Uses::SHELL_PARTS));
    let system = drawn(&shell(Mode::System));
    assert!(
        system.contains(concat!(
            r#"<span data-slot="sidebar-logo"><picture><source media="(prefers-color-scheme: dark)" srcset="/brand/dark.svg">"#,
            r#"<img src="/brand/light.svg" alt=""></picture></span>"#
        )),
        "{system}"
    );
    assert!(loads(&shell(Mode::System)).has(navree_ui::Uses::SHELL_PARTS));
    assert!(shell_parts().contains("[data-slot=sidebar-logo] > picture{display:contents}"));
    // The reader's choice of mode on the request picks the mark too.
    let chosen = shell(Mode::Light)
        .for_request("/deals?mode=dark", &HeaderMap::new())
        .page("Deals")
        .to_view()
        .to_html();
    assert!(
        chosen.contains(r#"<img src="/brand/dark.svg" alt="">"#),
        "{chosen}"
    );
    // A dark mark alone draws Navree's mark; a hostile one goes nowhere; a space or a comma
    // can't split the candidate.
    let alone = drawn(&store().logo_dark("/brand/dark.svg"));
    assert!(!alone.contains("dark.svg"), "{alone}");
    let hostile = drawn(
        &store()
            .logo("/brand/light.svg")
            .logo_dark("javascript:alert(1)")
            .appearance(Appearance::new(Theme::NAVREE, Mode::System)),
    );
    assert!(!hostile.contains("javascript"), "{hostile}");
    let spaced = drawn(
        &store()
            .logo("/brand/light.svg")
            .logo_dark("/brand/dark mark,2x.svg")
            .appearance(Appearance::new(Theme::NAVREE, Mode::System)),
    );
    assert!(
        spaced.contains(r#"srcset="/brand/dark%20mark%2C2x.svg""#),
        "{spaced}"
    );
}

#[test]
fn a_badge_ends_the_brand_row_in_place_of_the_context_line() {
    let internal = || Badge::new("Internal").variant(BadgeVariant::Solid);
    let shell = store().badge(internal());
    let html = drawn(&shell);
    assert!(
        html.contains(concat!(
            r#"<span data-slot="sidebar-brand-text"><span data-slot="sidebar-brand-name">Rangla Store</span></span>"#,
            r#"<span data-slot="badge" data-variant="solid">Internal</span></a>"#
        )),
        "{html}"
    );
    assert!(!html.contains("Store operations"), "{html}");
    let uses = loads(&shell);
    assert!(uses.has(navree_ui::Uses::SHELL_PARTS) && uses.has(navree_ui::Uses::PARTS_STYLES));
    // A request's badge in place of the shell's; none, and the context line is back.
    let request = store()
        .badge(Badge::new("Staging"))
        .for_request("/", &HeaderMap::new())
        .badge(internal())
        .page("Home")
        .to_view()
        .to_html();
    assert!(
        request.contains(">Internal</span></a>") && !request.contains("Staging"),
        "{request}"
    );
    assert!(
        drawn(&store())
            .contains(r#"<span data-slot="sidebar-brand-context">Store operations</span>"#)
    );
    // In the top bar of the Header layout, the badge goes with the brand.
    let header = drawn(&store().layout(ShellLayout::Header).badge(internal()));
    let bar = &header[header.find(r#"data-slot="topbar-brand""#).unwrap()..];
    assert!(
        bar[..bar.find("</a>").unwrap()].contains("Internal"),
        "{header}"
    );
    // It hides with the app's name when the sidebar folds.
    assert!(shell_parts().contains(
        "@media (min-width:768px){[data-collapsed] > [data-slot=sidebar] [data-slot=sidebar-brand] > [data-slot=badge]{display:none}}"
    ));
}

#[test]
fn a_solid_badge_is_filled_with_the_accent_and_comes_with_the_parts_styles() {
    let node: navree_ui::Node = Badge::new("Internal").variant(BadgeVariant::Solid).into();
    assert_eq!(
        navree_ui::render(&node),
        r#"<span data-slot="badge" data-variant="solid">Internal</span>"#
    );
    assert_eq!(
        navree_ui::measure(&node).uses,
        navree_ui::Uses::PARTS_STYLES
    );
    let parts = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles/extra/parts.css"),
    )
    .unwrap();
    assert!(parts.contains(
        "[data-slot=badge][data-variant=solid]{background:var(--accent);color:var(--accent-ink)}"
    ));
    let plain: navree_ui::Node = Badge::new("Won").variant(BadgeVariant::Success).into();
    assert!(navree_ui::measure(&plain).uses.is_none());
}
