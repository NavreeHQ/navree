//! The browser front end: the app shell and the profile page. Signing in and
//! out is `AuthPages` and live updates are the app's live hub, both registered
//! in `src/app.rs`.
//!
//! ```text
//! ui/
//! ├── mod.rs      # UiAppModule: registers the shell and the profile page
//! ├── auth.rs     # who is signed in (CurrentAccount) and the demo account
//! ├── shell.rs    # the sidebar, the top bar, the user menu and the Desk extractor
//! ├── live.rs     # the topic and the stream that tell open pages to fetch again
//! ├── pages.rs    # /profile
//! └── format.rs   # rupees, phone numbers and days the way people here read them
//! ```
//!
//! Feature modules (`src/modules/customers/`) draw their own pages: a handler
//! takes the [`Desk`](shell::Desk) extractor and returns the page it builds.
//! Every page route says who may see it (`.authenticated()`) and that it is a
//! page (`.page()`), so a guest is sent to `/sign-in` and back.

pub mod auth;
pub mod format;
pub mod live;
mod pages;
pub mod shell;

use navree::prelude::*;
use navree::ui::Shell;

/// The front end's providers and pages. It is global: every module's pages
/// find the shell without importing it.
pub struct UiAppModule;

impl Module for UiAppModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global();
        m.factory(Factory::<Shell>::new(|_| async {
            Ok(Arc::new(shell::shell()))
        }))
        .exports::<Shell>();
        m.controller::<pages::AccountPages>();
    }
}
