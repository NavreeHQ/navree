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
//! - [`UiModule`]: serves the files and picks ops or a page for each reply.
//!
//! The browser side lives in `runtime/`: plain ES modules with JSDoc types.
//! Its build output is committed, so `cargo build` never needs Node.
#![forbid(unsafe_code)]

pub mod assets;
mod assets_gen;
pub mod cmd;
pub mod html;
mod module;
pub mod node;
pub mod op;
pub mod render;
pub(crate) mod shell;
pub mod tokens;
pub mod view;

pub use cmd::{Cmd, Commands, Trigger};
pub use html::{Href, HrefError, SafeHtml};
pub use module::UiModule;
pub use node::{Attr, AttrValue, Element, IntoNodes, Node, Uses, el};
pub use op::{Envelope, Op, Reply, ToastTone, UrlMode};
pub use render::{Measure, measure, render, render_into};
pub use view::{PAGE_CSP, View};
