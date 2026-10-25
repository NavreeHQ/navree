//! Integration tests for the keel-log subscriber: formats, redaction and
//! level filters. Every test builds its own subscriber with
//! `build_subscriber` and a `CaptureWriter`, and runs it as the thread's
//! default, so tests never touch the process-wide subscriber.

mod file;
mod formats;
mod levels;
mod options;
mod redaction;
mod support;
