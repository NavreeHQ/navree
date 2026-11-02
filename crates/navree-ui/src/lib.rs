//! # navree-ui
//!
//! Navree's UI toolkit: typed Rust components render to HTML on the server, a
//! small owned runtime handles behaviour, and a page with nothing interactive
//! ships no JavaScript at all.
//!
//! - [`Node`], [`el`]: the tree every component builds. [`render`] writes it
//!   in one pass to one `String`.
//! - [`Cmd`]: what an element does in the browser with no request. [`Op`] and
//!   [`Reply`]: what the server sends back from an action.
//! - [`View`]: a page, with its strict content security policy.
//! - [`assets`]: one stylesheet and the runtime's modules, embedded with
//!   brotli and gzip forms, at content-hash URLs.
//! - [`tokens`]: the design tokens' one source.
//! - [`form`]: `#[derive(Form)]`, [`FormView`], [`Submit`]. [`table`]:
//!   `#[derive(TableRow)]`, [`DataTable`], [`TableQuery`], [`Region`].
//! - [`TestView`]: drive a page in a plain test, with no browser.
//! - [`UiModule`]: serves the files and picks ops or a page for each reply.
//!
//! The browser side lives in `runtime/`: plain ES modules with JSDoc types.
//! Its build output is committed, so `cargo build` never needs Node.
#![forbid(unsafe_code)]

/// What pages import: every component, the shell, forms, tables and the wire.
pub mod prelude {
    pub use crate::components::*;
    pub use crate::form::{
        Form as FormFields, FormView, Money, Options as OptionList, Rejected, Submit,
    };
    pub use crate::icons;
    pub use crate::op::{Op, Reply, ToastTone, UrlMode};
    pub use crate::table::{
        BulkAction, BulkRequest, DataTable, FilterOption, Region, RowAction, TableDensity,
        TableHeight, TableLabels, TablePage, TableQuery,
    };
    pub use crate::{
        AppPage, AppShell, Cmd, Element, FilterBar, Form, IntoNodes, NavGroup, NavItem, Node,
        Options, Shell, TableRow, TestView, UiModule, UserMenu, Validate, View, el,
    };
}

pub mod assets;
mod assets_gen;
pub mod cmd;
pub mod components;
pub mod compress;
pub mod form;
pub mod html;
pub mod icons;
mod module;
pub mod node;
pub mod op;
pub mod render;
pub mod shell;
pub mod table;
pub mod test_view;
pub mod tokens;
pub mod view;

pub use cmd::{Cmd, Commands, Trigger};
pub use components::*;
pub use form::{FormView, Money, Submit};
pub use html::{Href, HrefError, LocalHref, SafeHtml};
pub use module::{UiModule, UiNeed, UiOnDemand};
pub use navree_ui_macros::{FilterBar, Form, Options, TableRow};
pub use navree_validation::Validate;
pub use node::{Attr, AttrValue, Element, IntoNodes, Node, Uses, el};
pub use op::{Envelope, Op, Reply, ToastTone, UrlMode, is_prefetch};
pub use render::{Measure, measure, render, render_into};
pub use shell::{AppPage, AppShell, NavGroup, NavItem, Shell, UserMenu};
pub use table::{
    BulkAction, BulkRequest, DataTable, FilterOption, Region, RowAction, TableDensity, TableHeight,
    TableLabels, TablePage, TableQuery,
};
pub use test_view::TestView;
pub use view::{PAGE_CSP, View};
