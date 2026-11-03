//! The app shell, Breadcrumb, Pagination.

use navree::ui::*;

use crate::catalog::{Example, gallery, page, single};

const SOURCE: &str = include_str!("navigation.rs");

pub fn all() -> Vec<Example> {
    vec![
        page!("app-shell", "thumb", "The app shell", app_shell_thumb)
            .fill()
            .height(160),
        page!("app-shell", "default", "The app shell", app_shell_default)
            .fill()
            .height(560),
        single!("breadcrumb", "default", "A trail", breadcrumb_default),
        single!("pagination", "default", "Page 5 of 12", pagination_default),
        gallery!(
            "pagination",
            "edges",
            "First, middle and last",
            pagination_edges
        )
        .wide(),
    ]
}

fn app_shell_thumb() -> AppPage {
    Shell::new("Acme")
        .group(
            NavGroup::new("Sales")
                .item(NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE))
                .item(NavItem::new("Contacts", "/contacts").icon(icons::USERS)),
        )
        .page("Deals", "/deals")
}

fn app_shell_default() -> AppPage {
    let shell = Shell::new("Acme CRM")
        .context("Production")
        .group(
            NavGroup::new("Sales")
                .item(NavItem::new("Overview", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(
                    NavItem::new("Deals", "/deals")
                        .icon(icons::HANDSHAKE)
                        .count("312"),
                )
                .item(NavItem::new("Contacts", "/contacts").icon(icons::USERS))
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
            UserMenu::new("Ada Lovelace", "ada@example.com")
                .link("Profile", "/profile")
                .action("Sign out", "/sign-out"),
        );
    shell
        .page("Deals", "/deals")
        .actions(
            Button::primary("New deal")
                .icon(icons::PLUS)
                .href("/deals/new"),
        )
        .body(Grid::cols(3).gap(4).children((
            Stat::new("Open deals", "312").trend("+12 this week", true),
            Stat::new("Pipeline value", "$4.2M").trend("+3.1%", true),
            Stat::new("Won this month", "$640K").trend("-2.4%", false),
        )))
}

fn breadcrumb_default() -> Breadcrumb {
    Breadcrumb::new()
        .link("Deals", "/deals")
        .link("Acme", "/companies/acme")
        .current("Acme renewal")
}

fn pagination_default() -> Pagination {
    Pagination::new(5, 12, |page| format!("/deals?page={page}"))
}

fn pagination_edges() -> impl IntoIterator<Item = (&'static str, Pagination)> {
    [
        (
            "Page 1 of 12",
            Pagination::new(1, 12, |page| format!("?page={page}")),
        ),
        (
            "Page 6 of 12",
            Pagination::new(6, 12, |page| format!("?page={page}")),
        ),
        (
            "Page 12 of 12",
            Pagination::new(12, 12, |page| format!("?page={page}")),
        ),
        (
            "One page",
            Pagination::new(1, 1, |page| format!("?page={page}")),
        ),
    ]
}
