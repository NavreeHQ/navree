//! Show or hide what a reader may not use (the `access` feature): a navigation item, a
//! bulk action, a row action, a button.
//!
//! Route guards and the scoped query protect every action: a post a reader may not make is
//! refused whatever the page showed. This is about the page. A button that answers "no" is
//! a button that should not have been drawn, so the page asks navree-access's
//! [`Abilities`](navree_access::Abilities) before it draws:
//!
//! ```
//! use navree_access::Rules;
//! use navree_ui::ability::{ByAbility, Requires};
//! use navree_ui::{BulkAction, NavItem};
//!
//! let mut rules = Rules::new();
//! rules.can("read", "Deal");
//! let ability = rules.build();
//!
//! let items = ability.only([
//!     NavItem::new("Deals", "/deals").requires("read", "Deal"),
//!     NavItem::new("Reports", "/reports").requires("read", "Report"),
//! ]);
//! assert_eq!(items.len(), 1);
//!
//! let actions = ability.only([
//!     BulkAction::new("archive", "Archive").requires("update", "Deal"),
//!     BulkAction::new("delete", "Delete").requires("delete", "Deal"),
//! ]);
//! assert!(actions.is_empty());
//! ```
//!
//! The check is on the kind of subject (a type-level check, as navree-access calls it): a
//! reader who may edit some deals sees the Edit button. The server still decides each
//! record.

use std::borrow::Cow;

use navree_access::Ability;

use crate::shell::{NavGroup, NavItem};

/// A part of a page that only a reader with an ability may see: the thing, and the action
/// on a kind of subject the reader needs (`"delete"` on `"Deal"`). Make one with
/// [`Requires::requires`].
#[derive(Clone, Debug)]
pub struct Gated<T> {
    item: T,
    action: Cow<'static, str>,
    subject: Cow<'static, str>,
}

impl<T> Gated<T> {
    /// The thing.
    pub fn item(&self) -> &T {
        &self.item
    }

    /// The action the reader needs.
    pub fn action(&self) -> &str {
        &self.action
    }

    /// The kind of subject the action is on.
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The thing, without the check.
    pub fn into_item(self) -> T {
        self.item
    }
}

/// `item.requires("delete", "Deal")`: the item is for readers who may delete deals. Any
/// component, navigation item or action can be gated.
pub trait Requires: Sized {
    /// Gates `self` on `action` over `subject`.
    fn requires(
        self,
        action: impl Into<Cow<'static, str>>,
        subject: impl Into<Cow<'static, str>>,
    ) -> Gated<Self> {
        Gated {
            item: self,
            action: action.into(),
            subject: subject.into(),
        }
    }
}

impl<T> Requires for T {}

/// What an [`Ability`] (or the [`Abilities`](navree_access::Abilities) extractor, which
/// derefs to one) does with gated parts of a page.
pub trait ByAbility {
    /// Whether the reader may see `gated`.
    fn allows<T>(&self, gated: &Gated<T>) -> bool;

    /// `gated`'s item when the reader may see it.
    fn show<T>(&self, gated: Gated<T>) -> Option<T> {
        self.allows(&gated).then(|| gated.into_item())
    }

    /// The items the reader may see, in order: for a navigation group's items, a table's
    /// bulk actions, a row's menu.
    fn only<T>(&self, items: impl IntoIterator<Item = Gated<T>>) -> Vec<T> {
        items
            .into_iter()
            .filter_map(|gated| self.show(gated))
            .collect()
    }

    /// `group` with the navigation items the reader may use; `None` when there are none,
    /// so a group with nothing in it is not drawn.
    fn nav_group(
        &self,
        group: NavGroup,
        items: impl IntoIterator<Item = Gated<NavItem>>,
    ) -> Option<NavGroup> {
        let allowed = self.only(items);
        if allowed.is_empty() {
            return None;
        }
        Some(allowed.into_iter().fold(group, NavGroup::item))
    }
}

impl ByAbility for Ability {
    fn allows<T>(&self, gated: &Gated<T>) -> bool {
        self.can(&gated.action, &gated.subject, None)
    }
}

#[cfg(test)]
mod tests {
    use navree_access::Rules;

    use super::*;
    use crate::{BulkAction, RowAction, render};

    fn ability() -> Ability {
        let mut rules = Rules::new();
        rules.can("read", "Deal");
        rules.can("update", "Deal");
        rules.build()
    }

    #[test]
    fn a_reader_sees_only_what_the_rules_allow() {
        let ability = ability();
        let actions = ability.only([
            BulkAction::new("archive", "Archive").requires("update", "Deal"),
            BulkAction::new("delete", "Delete").requires("delete", "Deal"),
        ]);
        assert_eq!(actions.len(), 1);
        let row = ability.only([
            RowAction::link("Open", "/deals/1").requires("read", "Deal"),
            RowAction::post("Delete", "/deals/1/delete").requires("delete", "Deal"),
        ]);
        assert_eq!(row.len(), 1);
        assert!(
            ability
                .show(NavItem::new("Reports", "/reports").requires("read", "Report"))
                .is_none()
        );
        assert!(
            ability
                .show(NavItem::new("Deals", "/deals").requires("read", "Deal"))
                .is_some()
        );
    }

    #[test]
    fn a_group_with_nothing_the_reader_may_use_is_not_drawn() {
        let ability = ability();
        let nothing = ability.nav_group(
            NavGroup::new("Admin"),
            [NavItem::new("Roles", "/roles").requires("manage", "Role")],
        );
        assert!(nothing.is_none());
        let group = ability
            .nav_group(
                NavGroup::new("Sales"),
                [
                    NavItem::new("Deals", "/deals").requires("read", "Deal"),
                    NavItem::new("Roles", "/roles").requires("manage", "Role"),
                ],
            )
            .expect("one item is allowed");
        let html = render(
            &crate::Shell::new("Acme")
                .group(group)
                .page("x", "/")
                .to_view()
                .body_node()
                .clone(),
        );
        assert!(html.contains("Deals") && !html.contains("Roles"), "{html}");
    }

    #[test]
    fn manage_and_all_cover_every_action() {
        let mut rules = Rules::new();
        rules.can("manage", "all");
        let ability = rules.build();
        assert!(ability.allows(&"x".requires("delete", "Anything")));
    }
}
