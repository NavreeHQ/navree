//! ContextMenu, Menubar and NavigationMenu.

use navree::ui::*;

use crate::catalog::{Example, single};

const SOURCE: &str = include_str!("menu_bars.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("context-menu", "thumb", "A context menu", context_thumb)
            .show()
            .height(250),
        single!(
            "context-menu",
            "default",
            "Right click a card",
            context_default
        )
        .show()
        .height(430),
        single!(
            "context-menu",
            "choices",
            "Checkboxes and radios",
            context_choices
        )
        .show()
        .height(430),
        single!("context-menu", "submenu", "A submenu", context_submenu).height(200),
        single!(
            "context-menu",
            "destructive",
            "Destructive and disabled",
            context_destructive
        )
        .show()
        .height(330),
        single!(
            "context-menu",
            "focusable",
            "Reached by the keyboard",
            context_focusable
        )
        .height(190),
        single!("menubar", "thumb", "A menu bar", menubar_thumb)
            .start()
            .height(110),
        single!("menubar", "default", "A menu bar", menubar_default)
            .start()
            .height(110),
        single!("menubar", "open", "A menu open", menubar_open)
            .show()
            .height(250),
        single!(
            "menubar",
            "choices",
            "Checkboxes, radios and a submenu",
            menubar_choices
        )
        .start()
        .height(110),
        single!("menubar", "link", "A button with no menu", menubar_link)
            .start()
            .height(110),
        single!(
            "navigation-menu",
            "thumb",
            "A navigation menu",
            navigation_thumb
        )
        .start()
        .height(110),
        single!(
            "navigation-menu",
            "default",
            "Panels of links",
            navigation_default
        )
        .show()
        .height(380),
        single!("navigation-menu", "links", "Links only", navigation_links)
            .start()
            .height(110),
        single!(
            "navigation-menu",
            "current",
            "The current page",
            navigation_current
        )
        .show()
        .height(300),
    ]
}

fn context_thumb() -> ContextMenu {
    ContextMenu::new("order-thumb", "Order actions")
        .content(Text::muted("Order 2041 · Gurpreet Singh, Ludhiana"))
        .item(MenuItem::link("Open order", "/orders/2041"))
        .item(MenuItem::action("Copy the order number").icon(icons::COPY))
}

fn context_default() -> ContextMenu {
    ContextMenu::new("order-menu", "Order actions")
        .content(Card::titled("Order 2041").body((
            Text::new("Gurpreet Singh, Ludhiana. ₹1,24,500 by UPI."),
            Text::muted("Right click, press and hold, or press Shift+F10."),
        )))
        .item(MenuItem::link("Open order", "/orders/2041").icon(icons::EYE))
        .item(MenuItem::action("Copy the order number").icon(icons::COPY))
        .item(MenuItem::post("Mark as paid", "/orders/2041/paid"))
        .separator()
        .item(
            MenuItem::post("Cancel order", "/orders/2041/cancel")
                .icon(icons::TRASH)
                .destructive(),
        )
}

fn context_choices() -> ContextMenu {
    ContextMenu::new("ship-menu", "Shipping")
        .content(Card::titled("Ship from").body(Text::muted("Right click to choose a warehouse.")))
        .checkbox(MenuCheckbox::new("flag", "Flag for follow-up").checked(true))
        .separator()
        .radios(
            MenuRadios::new("warehouse", "Warehouse")
                .option("ldh", "Ludhiana")
                .option("asr", "Amritsar")
                .option("chd", "Chandigarh")
                .value("ldh"),
        )
}

fn context_submenu() -> ContextMenu {
    ContextMenu::new("move-menu", "Move the contact")
        .content(
            Card::titled("Harleen Kaur").body(Text::muted("Right click to move her to a list.")),
        )
        .item(MenuItem::link("Open the profile", "/contacts/12"))
        .submenu(
            MenuSub::new("Move to")
                .item(MenuItem::action("Hot leads"))
                .item(MenuItem::action("Follow up in Jalandhar"))
                .item(MenuItem::action("Customers")),
        )
}

fn context_destructive() -> ContextMenu {
    ContextMenu::new("invoice-menu", "Invoice actions")
        .content(Card::titled("Invoice 1187").body(Text::muted("₹58,300 due on 15 December")))
        .item(MenuItem::action("Send a reminder"))
        .item(MenuItem::action("Write off").disabled(true))
        .separator()
        .item(
            MenuItem::post("Delete invoice", "/invoices/1187/delete")
                .icon(icons::TRASH)
                .destructive(),
        )
}

fn context_focusable() -> ContextMenu {
    ContextMenu::new("file-menu", "Shared file actions")
        .content(Card::titled("Fee schedule.pdf").body(Text::muted(
            "Tab to this card, then press Shift+F10 or the menu key.",
        )))
        .focusable()
        .item(MenuItem::link("Download", "/files/88/download"))
        .item(MenuItem::action("Rename"))
}

fn menubar_thumb() -> Menubar {
    Menubar::new("bar-thumb", "Application")
        .menu(MenubarMenu::new("File").item(MenuItem::link("New invoice", "/invoices/new")))
        .menu(MenubarMenu::new("View").item(MenuItem::action("Zoom in")))
        .menu(MenubarMenu::new("Help").item(MenuItem::link("Guides", "/help")))
}

fn menubar_default() -> Menubar {
    Menubar::new("bar-default", "Application")
        .menu(
            MenubarMenu::new("File")
                .item(MenuItem::link("New invoice", "/invoices/new").shortcut("Ctrl+N"))
                .item(MenuItem::action("Open recent"))
                .separator()
                .item(MenuItem::link("Export as PDF", "/invoices/export.pdf")),
        )
        .menu(
            MenubarMenu::new("Customers")
                .item(MenuItem::link("Amandeep Sandhu", "/customers/4"))
                .item(MenuItem::link("Navdeep Grewal", "/customers/9")),
        )
        .menu(MenubarMenu::new("Reports").item(MenuItem::link("Sales in Ludhiana", "/reports/ldh")))
}

fn menubar_open() -> Menubar {
    Menubar::new("bar-open", "Application").menu(
        MenubarMenu::new("File")
            .item(MenuItem::link("New invoice", "/invoices/new").shortcut("Ctrl+N"))
            .item(MenuItem::action("Open recent").icon(icons::FILE))
            .separator()
            .item(MenuItem::post("Delete draft", "/drafts/9/delete").destructive()),
    )
}

fn menubar_choices() -> Menubar {
    Menubar::new("bar-choices", "Application")
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
        .menu(
            MenubarMenu::new("File").submenu(
                MenuSub::new("Export")
                    .item(MenuItem::link("PDF", "/invoices/export.pdf"))
                    .item(MenuItem::link("CSV", "/invoices/export.csv")),
            ),
        )
}

fn menubar_link() -> Menubar {
    Menubar::new("bar-link", "Application")
        .menu(MenubarMenu::new("File").item(MenuItem::link("New invoice", "/invoices/new")))
        .menu(MenubarMenu::new("Admin").disabled(true))
        .link("Help", "/help")
}

fn navigation_thumb() -> NavigationMenu {
    NavigationMenu::new("nav-thumb", "Main")
        .item(
            NavMenuItem::panel("Products")
                .entry(NavMenuLink::new("Invoices", "/invoices"))
                .entry(NavMenuLink::new("Payments", "/payments")),
        )
        .item(NavMenuItem::link("Pricing", "/pricing"))
        .item(NavMenuItem::link("Docs", "/docs"))
}

fn navigation_default() -> NavigationMenu {
    NavigationMenu::new("nav-default", "Main")
        .item(
            NavMenuItem::panel("Products")
                .featured(
                    Card::titled("New: UPI collect")
                        .body(Text::muted("Ask a customer to pay in seconds.")),
                )
                .entry(
                    NavMenuLink::new("Invoices", "/invoices")
                        .description("Send GST invoices from Ludhiana to Kochi in one click."),
                )
                .entry(
                    NavMenuLink::new("Payments", "/payments")
                        .description("Collect by UPI, card or bank transfer."),
                )
                .entry(NavMenuLink::new("Payroll", "/payroll")),
        )
        .item(
            NavMenuItem::panel("Company")
                .entry(NavMenuLink::new("About us", "/about"))
                .entry(NavMenuLink::new("Careers in Chandigarh", "/careers")),
        )
        .item(NavMenuItem::link("Pricing", "/pricing"))
}

fn navigation_links() -> NavigationMenu {
    NavigationMenu::new("nav-links", "Main")
        .item(NavMenuItem::link("Home", "/"))
        .item(NavMenuItem::link("Pricing", "/pricing"))
        .item(NavMenuItem::link("Docs", "/docs"))
        .item(NavMenuItem::link("Contact", "/contact"))
}

fn navigation_current() -> NavigationMenu {
    NavigationMenu::new("nav-current", "Main")
        .item(
            NavMenuItem::panel("Products")
                .entry(NavMenuLink::new("Invoices", "/invoices"))
                .entry(NavMenuLink::new("Payments", "/payments").current(true)),
        )
        .item(NavMenuItem::link("Pricing", "/pricing"))
        .item(NavMenuItem::link("Docs", "/docs").current(true))
}
