//! The background runtime that outlives each test's own runtime.
//!
//! `#[tokio::test]` gives every test a runtime of its own, dropped when the
//! test ends. The DevTools connections (shared by the tests of a binary)
//! and the front-end proxy run here instead, so one test ending never
//! stops another's browser.

use std::sync::LazyLock;

use tokio::runtime::{Builder, Handle, Runtime};

static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("navree-browser")
        .enable_all()
        .build()
        .expect("navree-browser cannot start its background runtime")
});

/// The background runtime's handle.
pub(crate) fn handle() -> Handle {
    RUNTIME.handle().clone()
}
