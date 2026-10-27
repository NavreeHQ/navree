//! # navree-browser
//!
//! Browser testing for Navree apps: drive a real Chrome or Chromium from
//! Rust tests over the Chrome DevTools Protocol, in the same `cargo test`
//! run as everything else. No WebDriver server and no Node.js in the test
//! process.
//!
//! - [`Browser`] starts headless Chrome (one process shared by the tests of
//!   a binary that run at the same time) and gives each test its own
//!   browser context: separate cookies, storage and cache.
//! - [`Page`] and [`Element`] act like a user (`type_into`, `press`,
//!   `click`, `select`, `check`, `drag`, `keys`) and assert on what a user
//!   sees (`assert_see`, `assert_path_is`, `assert_count`, ...). Every step
//!   waits for the page, up to a timeout, so tests need no sleeps.
//! - A failing step saves a screenshot, the page's HTML and its console
//!   log under `<target>/navree-browser/<test name>/`, and the error names
//!   them ([`BrowserError`]).
//! - [`Target`] finds elements by CSS (`#id`, `.class`, `[attr]`,
//!   `css=...`), test id (`@deal-card`) or the text users see (labels,
//!   button text, accessible names).
//! - [`TestServer`] serves an app's real router on a free port;
//!   [`FrontEnd`] starts a web front end once and points it at the app of
//!   the test holding its [`WebSession`].
//! - [`Viewport`] emulates phones and tablets.
//!
//! ```no_run
//! use navree_browser::{Browser, BrowserError, TestServer};
//!
//! # async fn demo(ctx: &navree_kernel::AppContext) -> Result<(), BrowserError> {
//! let server = TestServer::start(ctx).await?;
//! let browser = Browser::launch().await?;
//! let page = browser.visit(server.url("/login")).await?;
//! page.type_into("Email", "rep1@navree.test").await?;
//! page.type_into("Password", "password123").await?;
//! page.press("Sign in").await?;
//! page.assert_path_is("/").await?;
//! page.assert_see("Pipeline by stage").await?;
//! # Ok(())
//! # }
//! ```
//!
//! Settings come from `NAVREE_BROWSER_CHROME`, `NAVREE_BROWSER_HEADLESS`,
//! `NAVREE_BROWSER_SLOW_MO`, `NAVREE_BROWSER_TIMEOUT_MS` and
//! `NAVREE_BROWSER_ARTIFACTS`, or [`Browser::builder`]. Run the tests in a
//! Tokio runtime (`#[tokio::test]`).
#![forbid(unsafe_code)]

mod artifacts;
mod browser;
mod cdp;
mod chrome;
mod error;
mod frontend;
mod keys;
mod page;
mod process;
mod runtime;
mod server;
mod settings;
mod target;
mod viewport;

pub use browser::{Browser, BrowserBuilder};
pub use error::{BrowserError, FailureFiles, StepFailure};
pub use frontend::{FrontEnd, WebSession};
pub use page::{Cookie, Element, Page, PageRequest};
pub use server::TestServer;
pub use target::Target;
pub use viewport::Viewport;
