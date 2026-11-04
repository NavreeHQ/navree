//! Context menus, menu bars and the navigation menu in the states the browser suite runs
//! against: `/menubar`, the page a link goes to, and the post that raises a toast.
//!
//! | Page | Shows |
//! |---|---|
//! | `/menubar` | a context menu on a card, one on links and a field, one with hostile text, two menu bars, two navigation menus |
//! | `/menubar/go?to=` | where a link of a menu went |
//! | `/menubar/echo` | what a post item sent, as a toast |

use std::sync::Arc;

use http::Method;
use navree::http::extract::{RawQuery, urlencoded};
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::*;

/// The first value of each key in a query or a form body.
fn pairs(input: &str) -> Vec<(String, String)> {
    urlencoded::parse(input.as_bytes())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

fn section(id: &'static str, title: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("id", id)
        .children((Heading::h2(title), Stack::gap(3).children(content)))
}

/// An order card: the area a right click opens the menu on.
fn order_area(id: &'static str) -> Element {
    el("div")
        .attr("id", id)
        .attr("data-area", "")
        .child(Card::titled("Order 2041").body((
            Text::new("Gurpreet Singh, Ludhiana. ₹1,24,500 by UPI."),
            Text::new("Right click, long press, or press Shift+F10."),
        )))
}

fn order_menu() -> ContextMenu {
    ContextMenu::new("ctx-order", "Order actions")
        .content(order_area("ctx-order-area"))
        .focusable()
        .header("Order 2041")
        .item(MenuItem::link("Open order", "/menubar/go?to=open").shortcut("Enter"))
        .item(MenuItem::action("Copy the order number").icon(icons::COPY))
        .item(MenuItem::post("Mark as paid", "/menubar/echo?what=paid"))
        .item(MenuItem::action("Refund").disabled(true))
        .separator()
        .checkbox(MenuCheckbox::new("flag", "Flag for follow-up").checked(true))
        .radios(
            MenuRadios::new("warehouse", "Ship from")
                .option("ldh", "Ludhiana")
                .option("asr", "Amritsar")
                .option("chd", "Chandigarh")
                .value("ldh"),
        )
        .separator()
        .submenu(
            MenuSub::new("Move to")
                .item(MenuItem::link("Jalandhar", "/menubar/go?to=jalandhar"))
                .item(MenuItem::link("Patiala", "/menubar/go?to=patiala"))
                .submenu(
                    MenuSub::new("Zones")
                        .item(MenuItem::link("Doaba", "/menubar/go?to=doaba"))
                        .item(MenuItem::link("Malwa", "/menubar/go?to=malwa")),
                ),
        )
        .separator()
        .item(MenuItem::post("Cancel order", "/menubar/echo?what=cancel").destructive())
}

/// Links and a field inside a region: the keys reach the menu from any of them, and the
/// browser's own menu stays on the field.
fn mixed_menu() -> ContextMenu {
    ContextMenu::new("ctx-mixed", "Contact actions")
        .content(
            Card::titled("Harleen Kaur").body((
                el("p").child(
                    el("a")
                        .attr("id", "ctx-link")
                        .href("/menubar/go?to=profile")
                        .child("Open the profile"),
                ),
                Input::text("notes")
                    .attr("id", "ctx-input")
                    .attr("aria-label", "Notes"),
            )),
        )
        .item(MenuItem::action("Call +91 98765 43210"))
        .item(MenuItem::link("Write an email", "/menubar/go?to=email"))
}

fn hostile_menu() -> ContextMenu {
    ContextMenu::new("ctx-hostile", "<img src=x onerror=alert(1)> actions")
        .content(
            el("div")
                .attr("id", "ctx-hostile-area")
                .child("<img src=x onerror=alert(1)> Pneumonoultramicroscopic"),
        )
        .header("<script>alert(1)</script>")
        .item(MenuItem::link(
            "<img src=x onerror=alert(1)> and a very long label that goes on and on and on and on and on and on",
            "javascript:alert(1)",
        ))
        .item(MenuItem::action("<b>bold</b>").shortcut("<kbd>"))
}

fn contexts() -> Element {
    section(
        "contexts",
        "Context menu",
        (order_menu(), mixed_menu(), hostile_menu()),
    )
}

fn bar(id: &'static str) -> Menubar {
    Menubar::new(id, "Application")
        .menu(
            MenubarMenu::new("File")
                .item(MenuItem::link("New invoice", "/menubar/go?to=new").shortcut("Ctrl+N"))
                .item(MenuItem::action("Open recent"))
                .item(MenuItem::post("Delete draft", "/menubar/echo?what=draft").destructive())
                .separator()
                .submenu(
                    MenuSub::new("Export")
                        .item(MenuItem::link("PDF", "/menubar/go?to=pdf"))
                        .item(MenuItem::link("CSV", "/menubar/go?to=csv")),
                ),
        )
        .menu(
            MenubarMenu::new("View")
                .checkbox(MenuCheckbox::new("grid", "Show the grid").checked(true))
                .radios(
                    MenuRadios::new("zoom", "Zoom")
                        .option("100", "100%")
                        .option("125", "125%")
                        .option("150", "150%")
                        .value("100"),
                ),
        )
        .menu(
            MenubarMenu::new("Reports")
                .item(MenuItem::link("Sales in Ludhiana", "/menubar/go?to=ldh"))
                .item(MenuItem::link("Sales in Amritsar", "/menubar/go?to=asr")),
        )
        .menu(MenubarMenu::new("Admin").disabled(true))
        .link("Help", "/menubar/go?to=help")
}

fn menubars() -> Element {
    section(
        "menubars",
        "Menu bar",
        (
            bar("bar"),
            el("div").attr("dir", "rtl").attr("id", "rtl-bar").child(
                Menubar::new("bar-rtl", "التطبيق")
                    .menu(
                        MenubarMenu::new("ملف")
                            .item(MenuItem::link("جديد", "/menubar/go?to=new"))
                            .item(MenuItem::link("فتح", "/menubar/go?to=open")),
                    )
                    .menu(
                        MenubarMenu::new("عرض")
                            .item(MenuItem::link("تكبير", "/menubar/go?to=zoom")),
                    )
                    .menu(
                        MenubarMenu::new("مساعدة")
                            .item(MenuItem::link("حول", "/menubar/go?to=about")),
                    ),
            ),
        ),
    )
}

fn site(id: &'static str) -> NavigationMenu {
    NavigationMenu::new(id, "Main")
        .item(
            NavMenuItem::panel("Products")
                .entry(
                    NavMenuLink::new("Invoices", "/menubar/go?to=invoices")
                        .description("Send GST invoices from Ludhiana to Kochi in one click."),
                )
                .entry(
                    NavMenuLink::new("Payments", "/menubar/go?to=payments")
                        .description("Collect by UPI, card or bank transfer.")
                        .current(true),
                )
                .entry(NavMenuLink::new("Payroll", "/menubar/go?to=payroll"))
                .featured(
                    Card::titled("New: UPI collect").body("Ask a customer to pay in seconds."),
                ),
        )
        .item(
            NavMenuItem::panel("Company")
                .entry(NavMenuLink::new("About", "/menubar/go?to=about"))
                .entry(NavMenuLink::new(
                    "Careers in Chandigarh",
                    "/menubar/go?to=careers",
                )),
        )
        .item(NavMenuItem::link("Pricing", "/menubar/go?to=pricing"))
        .item(NavMenuItem::link("Docs", "/menubar/go?to=docs"))
}

fn navigation() -> Element {
    section(
        "navigation",
        "Navigation menu",
        (
            site("site-nav"),
            el("div").attr("dir", "rtl").attr("id", "rtl-nav").child(
                NavigationMenu::new("site-nav-rtl", "الرئيسية")
                    .item(
                        NavMenuItem::panel("المنتجات")
                            .entry(NavMenuLink::new("الفواتير", "/menubar/go?to=invoices"))
                            .entry(NavMenuLink::new("المدفوعات", "/menubar/go?to=payments")),
                    )
                    .item(NavMenuItem::link("الأسعار", "/menubar/go?to=pricing")),
            ),
        ),
    )
}

fn page(shell: &AppShell) -> AppPage {
    shell
        .page("Menu bars")
        .body(Stack::gap(8).children((contexts(), menubars(), navigation())))
}

/// The routes of the proof page.
pub struct BarPages;

impl Injectable for BarPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for BarPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/menubar", |shell: AppShell| async move {
                page(&shell)
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/menubar/go",
                |shell: AppShell, RawQuery(query): RawQuery| async move {
                    let all = pairs(query.as_deref().unwrap_or_default());
                    let to = all
                        .iter()
                        .find(|(name, _)| name == "to")
                        .map_or("nowhere", |(_, value)| value.as_str());
                    shell
                        .page("Went somewhere")
                        .body(Text::new(format!("You went to {to}.")))
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/menubar/echo",
                |RawQuery(query): RawQuery| async move {
                    let all = pairs(query.as_deref().unwrap_or_default());
                    let what = all
                        .iter()
                        .find(|(name, _)| name == "what")
                        .map_or("nothing", |(_, value)| value.as_str());
                    Reply::ops_or_redirect("/menubar")
                        .toast(ToastTone::Success, format!("Posted: {what}"))
                },
            )
            .public(),
        ]
    }
}
