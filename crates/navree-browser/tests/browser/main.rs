//! navree-browser integration tests: a fixture app served on a real port
//! and driven by headless Chrome. Without a browser on the machine they
//! print a notice and pass (see `support`).

mod actions;
mod assertions;
mod docs_examples;
mod docs_surface;
mod failures;
mod frontend;
mod launch;
mod reading;
mod server;
mod support;
mod targets;
mod viewports;
mod waits;
