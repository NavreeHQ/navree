//! Helpers: run code under a capturing subscriber built from options.

use navree_log::{CaptureWriter, LogControl, LogFormat, LogOptions, build_subscriber};

/// Default options with the pretty (human-readable) format.
pub fn pretty() -> LogOptions {
    LogOptions {
        format: LogFormat::Pretty,
        ..LogOptions::default()
    }
}

/// Default options with the JSON format.
pub fn json() -> LogOptions {
    LogOptions {
        format: LogFormat::Json,
        ..LogOptions::default()
    }
}

/// Runs `f` with a subscriber built from `options` as the thread's default
/// and returns everything it wrote.
pub fn capture(options: &LogOptions, f: impl FnOnce(&LogControl)) -> String {
    let writer = CaptureWriter::new();
    let (dispatch, control) = build_subscriber(options, writer.clone()).expect("build subscriber");
    tracing::dispatcher::with_default(&dispatch, || f(&control));
    writer.contents()
}
