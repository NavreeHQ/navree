//! The browser front end: sign-in, the app shell and the live stream, with the
//! pages that belong to no feature (sign in, sign out, profile).
//!
//! ```text
//! ui/
//! ├── mod.rs      # UiAppModule: registers the shell, the live hub and sign-in
//! ├── auth.rs     # how a browser signs in: the one file that knows (read its top)
//! ├── shell.rs    # the sidebar, the top bar and the user menu
//! ├── live.rs     # server-sent events that tell open pages to fetch again
//! ├── pages.rs    # /sign-in, /sign-out and /profile
//! └── format.rs   # rupees, phone numbers and days the way people here read them
//! ```
//!
//! Feature modules (`src/modules/customers/`) draw their own pages: a handler
//! takes the `AppShell` extractor and returns the page it builds. Every page
//! route says who may see it (`.authenticated()`) and that it is a page
//! (`.page()`), so a guest is sent to `/sign-in` and back.

pub mod auth;
pub mod format;
pub mod live;
mod pages;
pub mod shell;

use navree::prelude::*;
use navree::ui::Shell;

use live::Hub;

/// The front end's providers and pages. It is global: every module's pages
/// find the shell and the hub without importing it.
pub struct UiAppModule;

impl Module for UiAppModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global();
        m.factory(Factory::<Shell>::new(|_| async {
            Ok(Arc::new(shell::shell()))
        }))
        .exports::<Shell>();
        m.value(Hub::new()).exports::<Hub>();
        // Lets `.authenticated()` and `CurrentUser` see a signed-in browser.
        m.strategy::<auth::BrowserStrategy>();
        m.controller::<pages::AccountPages>();
    }
}
