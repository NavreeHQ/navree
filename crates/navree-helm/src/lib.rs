//! # navree-helm
//!
//! `HelmModule`: an admin panel generated from your entities, served by
//! your app at `/admin`.
//!
//! You describe each entity once, as a [`Resource`]: its [`Field`]s (what
//! to show and edit), [`Filter`]s and search columns, [`Action`]s (on one
//! record or on the rows ticked on the list) and [`Metric`]s (cards above
//! the list and on the dashboard). Register it in its module with
//! `m.resource::<DealResource>()` ([`ModuleBuilderHelmExt`]) and Helm
//! serves the list, detail, create and edit screens, bulk actions and
//! metric cards, plus a JSON API under `/admin/api`.
//!
//! - **Your entities.** A resource wraps a SeaORM entity; writes go
//!   through `navree_db::Repo` in the request's unit of work (or through
//!   your services when you override [`Resource::create`] and
//!   [`Resource::update`]), so observers run and the audit log records who
//!   changed what. Every action run is audited as `helm.action`.
//! - **Your access rules.** Entering the panel needs `view` on [`Helm`]
//!   (`helm.view`); every screen, row, field and action is then checked
//!   against the user's abilities on the resource's subject (navree-access),
//!   with the same SQL filters the API uses. Hiding a button is never the
//!   only protection: the JSON API checks the same rules.
//! - **Sign-in.** The panel's sign-in screen uses the app's users through
//!   navree-auth's `AuthService` (password, then a two-factor code; the
//!   login throttle of `POST /api/auth/login`). The session is an
//!   encrypted, httpOnly, `SameSite=Strict` cookie scoped to the panel's
//!   path; every change it makes carries a CSRF token. Bearer tokens work
//!   too (tests, or a backend-for-frontend).
//! - **Pages.** Server-rendered with navree-view templates and a small
//!   stylesheet and script embedded in the crate, under a strict Content
//!   Security Policy (no inline scripts or styles).
//!
//! ```ignore
//! use navree::helm::{Field, Resource};
//! use navree::prelude::*;
//!
//! #[injectable]
//! pub struct DealResource;
//!
//! impl Resource for DealResource {
//!     type Entity = deal::Entity;
//!     const LABEL: &'static str = "Deals";
//!     const TITLE: &'static str = "title";
//!     const SEARCH: &'static [&'static str] = &["title"];
//!
//!     fn fields(&self) -> Vec<Field> {
//!         vec![Field::text("title").required().sortable()]
//!     }
//! }
//! ```
#![forbid(unsafe_code)]

mod action;
mod api;
mod context;
mod controller;
mod core;
mod cx;
mod field;
mod filter;
mod handle;
mod icons;
mod job;
mod markdown;
mod metric;
mod module;
mod money;
mod options;
mod page;
mod panel;
mod present;
mod query;
mod registry;
mod resource;
mod save;
mod session;
mod templates;
mod ui;
mod values;

pub use action::{Action, ActionResult};
pub use context::{ActionContext, PageContext, SaveContext};
pub use controller::HelmController;
pub use field::{DEFAULT_MAX_FILE_BYTES, Field};
pub use filter::Filter;
pub use job::HelmActionJob;
pub use metric::{Metric, MetricRange};
pub use module::{Helm, HelmModule, ModuleBuilderHelmExt};
pub use options::HelmOptions;
pub use page::HelmPage;
pub use resource::{FormData, ModelOf, Resource};
pub use session::SESSION_COOKIE;

/// The action that lets a user into the panel: `view` on [`Helm`].
pub const VIEW: &str = "view";

/// The subject of the panel itself (`Helm`).
pub const HELM_SUBJECT: &str = "Helm";

/// `Mark as won` as `mark-as-won`: lower-case ASCII letters and digits,
/// words joined by `-`.
pub(crate) fn slug(text: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in text.trim().chars() {
        if c.is_ascii_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(c.to_ascii_lowercase());
        } else if c == '_' && !out.is_empty() && !dash {
            out.push('_');
        } else {
            dash = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("Open pipeline"), "open-pipeline");
        assert_eq!(slug("mark-won"), "mark-won");
        assert_eq!(slug("  Deals by stage!  "), "deals-by-stage");
        assert_eq!(slug("pipeline_board"), "pipeline_board");
    }
}
