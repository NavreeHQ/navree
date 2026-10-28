//! # evidence-app
//!
//! The Navree evidence app: one application that wires the framework's
//! packages and tools, with a few domain models and seeders so every tool
//! has something to show. The e2e harness (`e2e/`) builds it into one
//! image, runs it as web, worker and scheduler against the compose
//! services, and drives it with HTTP checks, browser screenshots and k6
//! load (see `e2e/README.md`).
//!
//! [`app`] assembles it: [`platform`] (the framework modules, configured
//! once) and the domain modules in [`modules`].
#![forbid(unsafe_code)]

pub mod abilities;
pub mod modules;
pub mod seed;

mod app;
mod platform;

pub use app::app;
pub use platform::platform;
