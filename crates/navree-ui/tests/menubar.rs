//! Context menus, menu bars and the navigation menu: roles, names, the ids that tie a
//! button to its menu, what the page loads, JavaScript off, and hostile text.

use navree_ui::assets::{
    CORE_URL, MENUBAR_STYLES_URL, MENUBAR_URL, MENUS_URL, OVERLAY_URL, TABLE_URL,
};
use navree_ui::{
    Button, Card, ContextMenu, MenuCheckbox, MenuGroup, MenuItem, MenuRadios, MenuSub, Menubar,
    MenubarMenu, NavMenuItem, NavMenuLink, NavigationMenu, Node, Uses, View, icons, render,
};

/// The values of every `name="..."` attribute in `html`.
fn values(html: &str, name: &str) -> Vec<String> {
    let needle = format!(" {name}=\"");
    let mut found = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(&needle) {
        rest = &rest[at + needle.len()..];
        let end = rest.find('"').unwrap_or(rest.len());
        found.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    found
}

/// Every id is used once, and every `aria-labelledby` and `aria-controls` names an id that exists.
fn ids_are_unique_and_tied(html: &str) {
    let ids = values(html, "id");
    for (at, id) in ids.iter().enumerate() {
        assert!(!ids[..at].contains(id), "{id} is used twice in {html}");
    }
    for name in ["aria-labelledby", "aria-controls", "popovertarget"] {
        for target in values(html, name) {
            for one in target.split(' ') {
                assert!(
                    ids.iter().any(|id| id == one),
                    "{name}=\"{one}\" names nothing in {html}"
                );
            }
        }
    }
}

fn page(body: impl Into<Node>) -> View {
    View::new("x").body(body)
}

// ---- ContextMenu ---------------------------------------------------------

fn orders_menu() -> ContextMenu {
    ContextMenu::new("order-menu", "Order actions")
        .content(Card::titled("Order 2041").body("Gurpreet Singh, Ludhiana, ₹1,24,500"))
        .item(MenuItem::link("Open order", "/orders/2041").shortcut("Enter"))
        .item(MenuItem::post("Cancel order", "/orders/2041/cancel").destructive())
        .separator()
        .checkbox(MenuCheckbox::new("flag", "Flag for follow-up").checked(true))
        .radios(
            MenuRadios::new("ship", "Ship from")
                .option("ldh", "Ludhiana")
                .option("asr", "Amritsar")
                .value("ldh"),
        )
}

// UI-GENUI-019
#[test]
fn a_context_menu_is_a_region_and_a_menu_that_the_module_opens() {
    let html = render(&orders_menu().into());
    // The region names the menu it opens, and is not a button or a link.
    assert!(
        html.contains(r#"data-slot="context-menu" data-ui-context="order-menu""#),
        "{html}"
    );
    // The menu is a manual popover: the browser would close an automatic one on the
    // mouse-up that follows a right click, so the module closes it itself.
    assert!(
        html.contains(r#"id="order-menu" popover="manual" role="menu""#),
        "{html}"
    );
    assert!(html.contains(r#"aria-label="Order actions""#), "{html}");
    assert!(html.contains("data-ui-menu"), "{html}");
    assert!(html.contains("data-ui-context-menu"), "{html}");
    // Nothing opens it, so nothing is claimed to be expanded.
    assert!(!html.contains("aria-expanded"), "{html}");
    // The menu is named by its own label, not by a button: the one it has isn't a button's.
    let start = html.find(r#"id="order-menu" popover"#).expect("the menu");
    let tag = &html[start..start + html[start..].find('>').expect("its end")];
    assert!(!tag.contains("aria-labelledby"), "{tag}");
    assert!(tag.contains(r#"aria-label="Order actions""#), "{tag}");
    assert!(html.contains(r#"role="menuitemcheckbox""#), "{html}");
    assert_eq!(html.matches(r#"role="menuitemradio""#).count(), 2, "{html}");
    assert!(html.contains("data-destructive"), "{html}");
    assert!(html.contains(r#"data-slot="menu-shortcut""#), "{html}");
    assert!(html.contains("Gurpreet Singh"), "{html}");
    ids_are_unique_and_tied(&html);
}

// UI-GENUI-019
#[test]
fn a_context_menu_region_takes_the_keyboard_only_when_asked() {
    let plain = render(&orders_menu().into());
    assert!(!plain.contains("tabindex=\"0\""), "{plain}");
    let focusable = render(&orders_menu().focusable().into());
    assert!(focusable.contains(r#"tabindex="0""#), "{focusable}");
    // A focusable region is named, so assistive technology says what it is.
    assert!(focusable.contains(r#"role="group""#), "{focusable}");
    assert!(
        focusable.contains(r#"aria-label="Order actions""#),
        "{focusable}"
    );
}

// UI-GENUI-019
#[test]
fn a_context_menu_loads_the_menus_module_and_its_own_and_only_what_it_uses() {
    let plain = page(orders_menu());
    let uses = plain.uses();
    assert!(uses.has(Uses::MENUS) && uses.has(Uses::MENUBAR), "{uses:?}");
    // It places itself at the pointer: the overlay module comes only with a submenu.
    assert!(!uses.has(Uses::OVERLAY), "{uses:?}");
    // The item that posts goes through the core runtime, as in a dropdown menu.
    assert!(uses.has(Uses::CORE), "{uses:?}");
    let html = plain.to_html();
    assert!(html.contains(MENUBAR_URL), "{html}");
    assert!(html.contains(MENUS_URL), "{html}");
    assert!(html.contains(MENUBAR_STYLES_URL), "{html}");
    assert!(!html.contains(TABLE_URL), "{html}");

    let links_only = page(
        ContextMenu::new("m", "Actions")
            .content("Area")
            .item(MenuItem::link("Open", "/o")),
    );
    assert!(!links_only.uses().has(Uses::CORE));
    assert!(!links_only.uses().has(Uses::OVERLAY));
    assert!(!links_only.to_html().contains(CORE_URL));

    let with_sub = page(
        ContextMenu::new("m", "Actions")
            .content("Area")
            .submenu(MenuSub::new("Move to").item(MenuItem::link("Delhi", "/d"))),
    );
    assert!(with_sub.uses().has(Uses::OVERLAY));
    assert!(with_sub.to_html().contains(OVERLAY_URL));
}

// UI-GENUI-019
#[test]
fn a_page_without_these_menus_loads_none_of_their_module() {
    let plain = page(Button::primary("Go").href("/x"));
    assert!(!plain.uses().has(Uses::MENUBAR));
    let html = plain.to_html();
    assert!(
        !html.contains(MENUBAR_URL) && !html.contains(MENUBAR_STYLES_URL),
        "{html}"
    );
}

// UI-GENUI-019
#[test]
fn a_context_menu_has_the_entries_of_a_dropdown_menu() {
    let html = render(
        &ContextMenu::new("m", "Actions")
            .content("Area")
            .header("Harleen Kaur")
            .group(MenuGroup::new("Share").item(MenuItem::action("Copy link").disabled(true)))
            .submenu(
                MenuSub::new("Move to")
                    .icon(icons::MAP_PIN)
                    .item(MenuItem::link("Chandigarh", "/c"))
                    .submenu(MenuSub::new("Zones").item(MenuItem::link("North", "/n"))),
            )
            .into(),
    );
    assert!(html.contains(r#"data-slot="menu-header""#), "{html}");
    assert!(html.contains(r#"role="group""#), "{html}");
    assert!(html.contains(r#"aria-disabled="true""#), "{html}");
    assert!(html.contains("data-sub-trigger"), "{html}");
    assert!(
        html.contains(r#"id="m-s2" popover="auto" role="menu""#),
        "{html}"
    );
    ids_are_unique_and_tied(&html);
}

// UI-GENUI-019
#[test]
fn a_context_menu_escapes_every_text_and_refuses_an_unsafe_link() {
    let html = render(
        &ContextMenu::new("m", "<b>Actions</b>")
            .content(Card::titled("<script>alert(1)</script>").body("x"))
            .header("<i>Navdeep</i>")
            .item(MenuItem::link("<u>Open</u>", "javascript:alert(1)").shortcut("<k>"))
            .into(),
    );
    for bad in ["<b>", "<i>", "<u>", "<k>"] {
        assert!(!html.contains(bad), "{bad}: {html}");
    }
    assert!(!html.contains("<script>"), "{html}");
    assert!(!html.contains("javascript"), "{html}");
}

// UI-GENUI-019
#[test]
fn a_context_menu_of_five_hundred_items_draws_them_all() {
    let mut menu = ContextMenu::new("big", "Cities").content("Area");
    for n in 0..500 {
        menu = menu.item(MenuItem::link(format!("City {n}"), format!("/cities/{n}")));
    }
    let html = render(&menu.into());
    assert_eq!(html.matches(r#"role="menuitem""#).count(), 500);
}

// ---- Menubar -------------------------------------------------------------

fn editor_bar() -> Menubar {
    Menubar::new("editor-bar", "Application")
        .menu(
            MenubarMenu::new("File")
                .item(MenuItem::link("New invoice", "/invoices/new").shortcut("Ctrl+N"))
                .item(MenuItem::post("Delete draft", "/drafts/9/delete").destructive())
                .separator()
                .submenu(
                    MenuSub::new("Export")
                        .item(MenuItem::link("PDF", "/x.pdf"))
                        .item(MenuItem::link("CSV", "/x.csv")),
                ),
        )
        .menu(
            MenubarMenu::new("View")
                .checkbox(MenuCheckbox::new("grid", "Show the grid").checked(true))
                .radios(
                    MenuRadios::new("zoom", "Zoom")
                        .option("100", "100%")
                        .option("125", "125%")
                        .value("100"),
                ),
        )
        .menu(MenubarMenu::new("Reports").disabled(true))
        .link("Help", "/help")
}

// UI-GENUI-019
#[test]
fn a_menubar_is_a_menubar_of_menu_buttons_each_tied_to_its_menu() {
    let html = render(&editor_bar().into());
    assert!(
        html.contains(r#"role="menubar" aria-label="Application""#),
        "{html}"
    );
    // The pattern: the bar holds menuitems, and each one that has a menu says so.
    assert_eq!(
        html.matches(r#"aria-haspopup="menu""#).count(),
        2 + 1,
        "{html}"
    );
    for (button, menu) in [
        ("editor-bar-m1-trigger", "editor-bar-m1"),
        ("editor-bar-m2-trigger", "editor-bar-m2"),
    ] {
        assert!(html.contains(&format!(r#"id="{button}""#)), "{html}");
        assert!(
            html.contains(&format!(r#"popovertarget="{menu}""#)),
            "{html}"
        );
        assert!(
            html.contains(&format!(r#"id="{menu}" popover="auto" role="menu""#)),
            "{html}"
        );
        assert!(
            html.contains(&format!(r#"aria-labelledby="{button}""#)),
            "{html}"
        );
    }
    assert!(html.contains(r#"data-slot="menubar-trigger""#), "{html}");
    // Each menu and its button sit in a wrapper that has no role, as the pattern's example does.
    assert!(html.contains(r#"role="none""#), "{html}");
    // The menu inside a menu is an ordinary submenu.
    assert!(html.contains(r#"id="editor-bar-m1-s1""#), "{html}");
    ids_are_unique_and_tied(&html);
}

// UI-GENUI-019
#[test]
fn a_menubar_works_without_script_every_button_is_a_tab_stop_and_opens_its_popover() {
    let html = render(&editor_bar().into());
    // The module makes the bar one tab stop; with no script every button can be reached.
    assert!(!html.contains("tabindex=\"0\""), "{html}");
    assert!(html.contains("popovertarget"), "{html}");
    // A button that opens a menu starts not expanded, and the module keeps it right.
    assert!(html.contains(r#"aria-expanded="false""#), "{html}");
    assert!(
        html.contains(r#"data-ui-keep-attrs="aria-expanded""#),
        "{html}"
    );
}

// UI-GENUI-019
#[test]
fn a_disabled_menubar_menu_stays_in_the_bar_and_opens_nothing() {
    let html = render(&editor_bar().into());
    let at = html.find("Reports").expect("the button");
    let before = &html[..at];
    let start = before.rfind("<button").expect("its tag");
    let tag = &html[start..at];
    assert!(tag.contains(r#"aria-disabled="true""#), "{tag}");
    assert!(!tag.contains("popovertarget"), "{tag}");
    assert!(!html.contains(r#"id="editor-bar-m3""#), "{html}");
}

// UI-GENUI-019
#[test]
fn a_bar_level_link_is_a_menuitem_that_goes_to_its_page() {
    let html = render(&editor_bar().into());
    assert!(
        html.contains(r#"<a data-slot="menubar-trigger" role="menuitem" href="/help">Help</a>"#),
        "{html}"
    );
}

// UI-GENUI-019
#[test]
fn a_menubar_loads_the_menus_overlay_and_its_own_module_and_not_the_core_runtime() {
    let bar = page(
        Menubar::new("b", "App").menu(MenubarMenu::new("File").item(MenuItem::link("New", "/n"))),
    );
    let uses = bar.uses();
    for module in [Uses::MENUS, Uses::OVERLAY, Uses::MENUBAR] {
        assert!(uses.has(module), "{uses:?}");
    }
    assert!(!uses.has(Uses::CORE), "{uses:?}");
    let html = bar.to_html();
    assert!(
        html.contains(MENUBAR_URL) && html.contains(MENUBAR_STYLES_URL),
        "{html}"
    );
    assert!(!html.contains(CORE_URL), "{html}");
    // A post item in a menu brings the core runtime that sends it.
    assert!(page(editor_bar()).uses().has(Uses::CORE));
}

// UI-GENUI-019
#[test]
fn a_menubar_escapes_every_text_and_refuses_an_unsafe_link() {
    let html = render(
        &Menubar::new("b", "<b>App</b>")
            .menu(
                MenubarMenu::new("<i>File</i>")
                    .item(MenuItem::link("<u>Open</u>", "javascript:alert(1)")),
            )
            .link("<s>Help</s>", "data:text/html,<script>alert(1)</script>")
            .into(),
    );
    for bad in ["<b>", "<i>", "<u>", "<s>", "<script>"] {
        assert!(!html.contains(bad), "{bad}: {html}");
    }
    assert!(!html.contains("javascript"), "{html}");
    assert!(!html.contains("data:text"), "{html}");
}

// UI-GENUI-019
#[test]
fn a_menubar_with_nothing_in_it_and_a_very_wide_one_both_draw() {
    let empty = render(&Menubar::new("e", "Nothing").into());
    assert!(empty.contains(r#"role="menubar""#), "{empty}");
    let mut wide = Menubar::new("w", "Wide");
    for m in 0..50 {
        let mut menu = MenubarMenu::new(format!("Menu {m}"));
        for n in 0..10 {
            menu = menu.item(MenuItem::link(format!("Item {n}"), format!("/{m}/{n}")));
        }
        wide = wide.menu(menu);
    }
    let html = render(&wide.into());
    assert_eq!(html.matches(r#"role="menu""#).count(), 50);
    assert_eq!(html.matches(r#"role="menuitem""#).count(), 50 + 500);
    ids_are_unique_and_tied(&html);
}

// ---- NavigationMenu ------------------------------------------------------

fn site_menu() -> NavigationMenu {
    NavigationMenu::new("site-nav", "Main")
        .item(
            NavMenuItem::panel("Products")
                .entry(
                    NavMenuLink::new("Invoices", "/invoices")
                        .description("Send GST invoices from Ludhiana to Kochi in one click."),
                )
                .entry(NavMenuLink::new("Payments", "/payments").current(true))
                .featured(
                    Card::titled("New: UPI collect").body("Ask a customer to pay in seconds."),
                ),
        )
        .item(
            NavMenuItem::panel("Company")
                .entry(NavMenuLink::new("About", "/about"))
                .entry(NavMenuLink::new("Careers in Chandigarh", "/careers")),
        )
        .item(NavMenuItem::link("Pricing", "/pricing"))
        .item(NavMenuItem::link("Docs", "/docs").current(true))
}

// UI-GENUI-019
#[test]
fn a_navigation_menu_is_a_nav_of_links_and_disclosure_buttons_not_a_menu() {
    let html = render(&site_menu().into());
    assert!(html.starts_with("<nav"), "{html}");
    assert!(html.contains(r#"aria-label="Main""#), "{html}");
    assert!(html.contains(r#"data-slot="navigation-menu""#), "{html}");
    // The disclosure pattern: buttons that show and hide panels of links, in lists.
    assert!(html.contains("<ul"), "{html}");
    assert!(html.contains(r#"aria-controls="site-nav-p1""#), "{html}");
    assert!(html.contains(r#"popovertarget="site-nav-p1""#), "{html}");
    assert!(
        html.contains(r#"id="site-nav-p1" popover="auto""#),
        "{html}"
    );
    // It is not a menu: no menu roles, no claim that a button opens one.
    for refused in [
        "role=\"menu\"",
        "role=\"menuitem\"",
        "role=\"menubar\"",
        "aria-haspopup",
    ] {
        assert!(!html.contains(refused), "{refused}: {html}");
    }
    // The module writes aria-expanded when it runs; with no script the browser's own
    // state for a button that opens a popover is the true one.
    assert!(
        !html.contains(r#"aria-expanded="false" aria-controls"#),
        "{html}"
    );
    ids_are_unique_and_tied(&html);
}

// UI-GENUI-019
#[test]
fn a_navigation_menu_marks_the_current_page_and_the_section_that_holds_it() {
    let html = render(&site_menu().into());
    assert_eq!(html.matches(r#"aria-current="page""#).count(), 2, "{html}");
    // The button of the panel that holds the current page says so.
    let trigger = html.find("Products").expect("the trigger");
    let start = html[..trigger].rfind("<button").expect("its tag");
    assert!(html[start..trigger].contains("data-active"), "{html}");
    let company = html.find("Company").expect("the second trigger");
    let start = html[..company].rfind("<button").expect("its tag");
    assert!(!html[start..company].contains("data-active"), "{html}");
}

// UI-GENUI-019
#[test]
fn a_navigation_menu_panel_holds_links_with_their_words_and_a_featured_block() {
    let html = render(&site_menu().into());
    assert!(
        html.contains(r#"data-slot="navigation-menu-content""#),
        "{html}"
    );
    assert!(
        html.contains(r#"data-slot="navigation-menu-link-description""#),
        "{html}"
    );
    assert!(
        html.contains("Send GST invoices from Ludhiana to Kochi"),
        "{html}"
    );
    assert!(
        html.contains(r#"data-slot="navigation-menu-featured""#),
        "{html}"
    );
    assert!(html.contains("New: UPI collect"), "{html}");
    // The panel is placed under its button by the overlay module.
    assert!(
        html.contains(r#"data-side="bottom""#) && html.contains("data-ui-place"),
        "{html}"
    );
}

// UI-GENUI-019
#[test]
fn a_navigation_menu_opens_its_panels_with_no_script_and_loads_its_own_module() {
    let page = page(site_menu());
    let uses = page.uses();
    assert!(
        uses.has(Uses::MENUBAR) && uses.has(Uses::OVERLAY),
        "{uses:?}"
    );
    assert!(!uses.has(Uses::MENUS) && !uses.has(Uses::CORE), "{uses:?}");
    let html = page.to_html();
    assert!(
        html.contains(MENUBAR_URL) && html.contains(OVERLAY_URL),
        "{html}"
    );
    assert!(
        !html.contains(MENUS_URL) && !html.contains(CORE_URL),
        "{html}"
    );
    // Every panel is a popover and every button names it: that is all that opening takes.
    assert_eq!(
        html.matches("popover=\"auto\"").count(),
        html.matches("popovertarget=").count()
    );
    // A menu of links only is drawn with the module's stylesheet and needs no overlay.
    let links = page_of_links();
    assert!(links.uses().has(Uses::MENUBAR), "{:?}", links.uses());
    assert!(!links.uses().has(Uses::OVERLAY), "{:?}", links.uses());
}

fn page_of_links() -> View {
    page(NavigationMenu::new("n", "Links").item(NavMenuItem::link("Home", "/")))
}

// UI-GENUI-019
#[test]
fn a_navigation_menu_escapes_every_text_and_refuses_an_unsafe_link() {
    let html = render(
        &NavigationMenu::new("n", "<b>Main</b>")
            .item(
                NavMenuItem::panel("<i>Products</i>")
                    .entry(
                        NavMenuLink::new("<u>Invoices</u>", "javascript:alert(1)")
                            .description("<s>words</s>"),
                    )
                    .featured(Card::titled("<script>alert(1)</script>").body("x")),
            )
            .item(NavMenuItem::link("<q>Docs</q>", "vbscript:x"))
            .into(),
    );
    for bad in ["<b>", "<i>", "<u>", "<s>", "<q>", "<script>"] {
        assert!(!html.contains(bad), "{bad}: {html}");
    }
    assert!(
        !html.contains("javascript") && !html.contains("vbscript"),
        "{html}"
    );
}

// UI-GENUI-019
#[test]
fn a_navigation_menu_of_many_panels_keeps_every_id_apart() {
    let mut menu = NavigationMenu::new("big", "Big");
    for p in 0..40 {
        let mut item = NavMenuItem::panel(format!("Section {p}"));
        for n in 0..20 {
            item = item.entry(NavMenuLink::new(format!("Page {n}"), format!("/{p}/{n}")));
        }
        menu = menu.item(item);
    }
    let html = render(&menu.into());
    assert_eq!(html.matches("popover=\"auto\"").count(), 40);
    ids_are_unique_and_tied(&html);
}
