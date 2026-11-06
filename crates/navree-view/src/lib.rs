//! # navree-view
//!
//! `ViewModule`: templates for mail and other text output (pages are drawn by
//! Navree UI, not by templates), on [navree-template](navree_template),
//! re-exported as [`template`].
//!
//! - [`ViewModule`] is global: it provides and exports [`Views`], built at
//!   boot from the template directories in [`ViewOptions`] (section `view`)
//!   and the templates modules embed with [`ModuleBuilderViewExt::templates`].
//! - [`Views::render`] renders a template by name: `mail/welcome` is
//!   `mail/welcome.html`, `.txt` or `.j2`, whichever exists first. A template
//!   file overrides an embedded template of the same name.
//! - `.html`, `.htm` and `.xml` templates (also with a trailing `.j2`) are
//!   HTML-escaped; `.txt` ones are not.
//! - Printing an undefined variable is an error, so a typo cannot send a
//!   blank email; `{% if title %}` still works for optional values.
//! - Filters: `{{ cents|money("USD") }}` (`$1,234.56`) and
//!   `{{ created_at|date("%d %b %Y") }}`.
//!
//! ```
//! use navree_view::Views;
//! use serde_json::json;
//!
//! # fn main() -> Result<(), navree_view::ViewError> {
//! let views = Views::from_templates([(
//!     "mail/invoice.html",
//!     "<p>{{ name }}, you owe {{ total|money(currency) }} by {{ due|date('%-d %B') }}.</p>",
//! )])?;
//!
//! let html = views.render(
//!     "mail/invoice",
//!     json!({ "name": "Ada & Co", "total": 123456, "currency": "EUR", "due": "2026-11-01" }),
//! )?;
//! assert_eq!(html, "<p>Ada &amp; Co, you owe €1,234.56 by 1 November.</p>");
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod error;
mod filters;
mod module;
mod options;
mod source;
mod views;

pub use error::ViewError;
pub use module::{ModuleBuilderViewExt, ViewModule};
pub use navree_template as template;
pub use options::ViewOptions;
pub use views::Views;
