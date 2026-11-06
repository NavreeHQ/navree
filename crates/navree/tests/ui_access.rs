//! Show or hide what a reader may not use, through the facade: the navigation, the bulk
//! actions and a row's menu of a Navree UI page follow navree-access's abilities. (The
//! routes keep their own guards: a post the reader may not make is refused whatever the
//! page showed, and navree-access's own suites prove that.)
//!
//! `cargo test -p navree --features ui,access --test ui_access`

use navree::access::{Ability, Rules};
use navree::ui::ability::{ByAbility, Requires};
use navree::ui::{BulkAction, NavGroup, NavItem, RowAction, Shell, render};

/// A sales clerk: reads and edits deals, reads reports.
fn clerk() -> Ability {
    let mut rules = Rules::new();
    rules.can("read", "Deal");
    rules.can("update", "Deal");
    rules.can("read", "Report");
    rules.build()
}

/// A manager: everything on deals, and nothing else.
fn manager() -> Ability {
    let mut rules = Rules::new();
    rules.can("manage", "Deal");
    rules.build()
}

#[test]
fn a_clerk_does_not_see_the_delete_bulk_action_and_a_manager_does() {
    let actions = |ability: &Ability| {
        ability.only([
            BulkAction::new("archive", "Archive").requires("update", "Deal"),
            BulkAction::new("delete", "Delete")
                .destructive()
                .requires("delete", "Deal"),
        ])
    };
    assert_eq!(actions(&clerk()).len(), 1);
    assert_eq!(actions(&manager()).len(), 2);
}

#[test]
fn the_menu_of_a_row_follows_the_abilities() {
    let row = |ability: &Ability| {
        ability.only([
            RowAction::link("Open", "/deals/7").requires("read", "Deal"),
            RowAction::post("Delete", "/deals/7/delete")
                .destructive()
                .requires("delete", "Deal"),
        ])
    };
    assert_eq!(row(&clerk()).len(), 1);
    assert_eq!(row(&manager()).len(), 2);
}

#[test]
fn the_navigation_follows_the_abilities() {
    let sidebar = |ability: &Ability| {
        let sales = ability.nav_group(
            NavGroup::new("Sales"),
            [
                NavItem::new("Deals", "/deals").requires("read", "Deal"),
                NavItem::new("Reports", "/reports").requires("read", "Report"),
            ],
        );
        let mut shell = Shell::new("Agra");
        if let Some(sales) = sales {
            shell = shell.group(sales);
        }
        render(shell.page("Home", "/").to_view().body_node())
    };
    let clerk_sees = sidebar(&clerk());
    assert!(
        clerk_sees.contains("Deals") && clerk_sees.contains("Reports"),
        "{clerk_sees}"
    );
    let manager_sees = sidebar(&manager());
    assert!(
        manager_sees.contains("Deals") && !manager_sees.contains("Reports"),
        "{manager_sees}"
    );
}
