//! Pages at a phone's width: what the grid, its spans and the app shell do below
//! 768 px.

use navree::ui::*;

use crate::catalog::{Example, page, single};

const SOURCE: &str = include_str!("page_layouts.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!(
            "layout",
            "span-phone",
            "The same page on a phone",
            layout_span_phone
        )
        .start()
        .width(390)
        .height(520),
        page!(
            "app-shell",
            "phone",
            "The shell on a phone",
            app_shell_phone
        )
        .fill()
        .width(390)
        .height(640),
    ]
}

fn layout_span_phone() -> Grid {
    Grid::cols(3).gap(3).children((
        Card::titled("Revenue").span(2).body(Bars::new(
            "Revenue by month, rising from 12 to 61",
            &[12, 18, 15, 24, 31, 28, 36, 41, 39, 48, 55, 61],
        )),
        Card::titled("Activity").body(Text::muted(
            "Gurpreet moved Sutlej Supply renewal to Proposal",
        )),
    ))
}

fn app_shell_phone() -> AppPage {
    let shell = Shell::new("Sutlej CRM")
        .group(
            NavGroup::new("Sales")
                .item(
                    NavItem::new("Deals", "/deals")
                        .icon(icons::HANDSHAKE)
                        .count("312"),
                )
                .item(NavItem::new("Contacts", "/contacts").icon(icons::USERS)),
        )
        .user(
            UserMenu::new("Gurpreet Singh", "gurpreet@example.com")
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
        .body(Stack::gap(4).children((
            Stat::new("Open deals", "312").trend("+12 this week", true),
            Stat::new("Pipeline value", "₹4.2 crore").trend("+3.1%", true),
        )))
}
