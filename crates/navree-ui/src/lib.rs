//! # navree-ui
//!
//! The UI kit of Navree's server-rendered tools (Helm, Lighthouse, Fleet,
//! Logbook and Navigator's pages): an app shell and components as
//! templates for [navree-view](navree_view), and embedded, versioned CSS, JS and fonts.
//!
//! - [`UiModule`] contributes the `ui/*` templates to the app's
//!   [`Views`](navree_view::Views), serves the [`assets`] at
//!   `GET {mount}/{version}/{*file}` with year-long immutable caching, and
//!   (in development) the component gallery at `GET {mount}/gallery`.
//! - Pages extend `ui/app.html` (the shell: a collapsible sidebar with the
//!   app switcher, groups, items and the user menu; a top bar with the
//!   breadcrumb and search; the command palette; toasts) or `ui/auth.html`
//!   (a centered panel, for sign-in), and import the components:
//!   `{% import "ui/components.html" as ui %}`.
//! - A page's context holds a [`Shell`] as `shell`; [`render`] adds it and
//!   returns an [`HtmlPage`], served with a strict content security policy
//!   ([`PAGE_CSP`]: no inline scripts or styles; the kit needs none).
//! - Data-heavy components take prepared values: a [`Table`] (sortable
//!   headers, selection, row actions, pagination in Navree's page shape
//!   [`PageInfo`], empty and loading states), a [`Chart`] or [`Sparkline`],
//!   and [`FormErrors`] in Navree's validation error shape.
//!
//! ```
//! use navree_ui::{Cell, Column, Crumb, NavGroup, NavItem, PageInfo, Row, Shell, Table, render};
//! use serde_json::json;
//!
//! # fn main() -> Result<(), navree_view::ViewError> {
//! // A tool's own template, next to the kit's (an app gets both from ViewModule).
//! let views = navree_ui::views_with([(
//!     "helm/users.html",
//!     r#"{% extends "ui/app.html" %}
//! {% import "ui/components.html" as ui %}
//! {% block content %}
//!   {% call ui.page_header("Users", "Everyone with access to Acme.") %}
//!     {{ ui.button("Invite user", variant="primary", icon="user-plus", href="/helm/users/new") }}
//!   {% endcall %}
//!   {{ ui.data_table(users) }}
//! {% endblock %}"#,
//! )])?;
//!
//! let shell = Shell::new("Helm")
//!     .home("/helm")
//!     .group(NavGroup::new("Records").item(NavItem::new("Users", "/helm/users").icon("users")))
//!     .crumb(Crumb::page("Users"))
//!     .title("Users")
//!     .current("/helm/users");
//! let users = Table::new("users")
//!     .column(Column::new("name", "Name"))
//!     .row(Row::new("1").cell(Cell::strong("Ada Lovelace")))
//!     .page(PageInfo::new(1, 15, 1), "/helm/users?page={page}");
//!
//! let page = render(&views, "helm/users", &shell, json!({ "users": users }))?;
//! assert!(page.0.contains("<title>Users · Helm</title>"));
//! assert!(page.0.contains("Ada Lovelace"));
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

pub mod assets;
mod chart;
pub mod gallery;
mod icons;
mod module;
mod page;
mod shell;
mod table;

pub use chart::{Chart, ChartSize, SeriesTone, Sparkline};
pub use icons::ICONS;
pub use module::{Ui, UiModule, UiOptions};
pub use page::{HtmlPage, PAGE_CSP, SIDEBAR_COOKIE, render, sidebar_collapsed};
pub use shell::{
    Command, CommandGroup, Crumb, MenuItem, NavGroup, NavItem, NavLink, Shell, Toast, UserMenu,
};
pub use table::{Action, Cell, Column, Empty, FormErrors, PageInfo, Row, Table, Tone};

use navree_view::{ViewError, Views};

/// The kit's templates as (name, source) pairs, what [`UiModule`] contributes
/// to the app's [`Views`]:
///
/// - `ui/base.html`: the document (head, assets); blocks `title`, `head`, `body`;
/// - `ui/app.html`: the shell; blocks `content`, `actions` (top bar, right),
///   `dialogs` (after the shell), `main_class`;
/// - `ui/auth.html`: a centered panel with the app's mark; block `content`;
/// - `ui/components.html`: the components, as macros;
/// - `ui/icons.html`: `icon(name)` and `mark()`.
pub const TEMPLATES: &[(&str, &str)] = &[
    ("ui/base.html", include_str!("../templates/ui/base.html")),
    ("ui/app.html", include_str!("../templates/ui/app.html")),
    ("ui/auth.html", include_str!("../templates/ui/auth.html")),
    (
        "ui/components.html",
        include_str!("../templates/ui/components.html"),
    ),
    ("ui/icons.html", include_str!("../templates/ui/icons.html")),
];

/// [`Views`] holding the kit's templates and `templates` (name, source
/// pairs; a later pair replaces an earlier one): for tests, static pages and
/// tools rendering without `ViewModule`.
pub fn views_with<I, N, S>(templates: I) -> Result<Views, ViewError>
where
    I: IntoIterator<Item = (N, S)>,
    N: Into<String>,
    S: Into<String>,
{
    let kit = TEMPLATES
        .iter()
        .map(|&(name, source)| (name.to_owned(), source.to_owned()));
    let extra = templates
        .into_iter()
        .map(|(name, source)| (name.into(), source.into()));
    Views::from_templates(kit.chain(extra))
}

/// Whether `name` is one of the kit's [`ICONS`].
pub fn is_icon(name: &str) -> bool {
    ICONS.binary_search(&name).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_are_sorted_and_known() {
        assert!(ICONS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(is_icon("users"));
        assert!(is_icon("chevrons-up-down"));
        assert!(!is_icon("not-an-icon"));
    }

    #[test]
    fn kit_templates_compile() {
        views_with(std::iter::empty::<(String, String)>()).expect("the kit's templates compile");
    }
}
