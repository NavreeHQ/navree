//! The log file of an installed `LoggerModule`: lines reach the file, and
//! app shutdown flushes them. Every test boots with the same options (the
//! process installs the subscriber once), so they share one file and each
//! looks for its own marker.

use std::path::PathBuf;
use std::sync::OnceLock;

use navree_kernel::Navree;
use navree_log::{FileLog, LogFormat, LogOptions, LoggerModule, Rotation};
use tempfile::TempDir;

fn log_path() -> PathBuf {
    static DIR: OnceLock<TempDir> = OnceLock::new();
    DIR.get_or_init(|| tempfile::tempdir().expect("tempdir"))
        .path()
        .join("logs")
        .join("navree.log")
}

fn options() -> LogOptions {
    LogOptions {
        format: LogFormat::Json,
        file: Some(FileLog {
            path: log_path(),
            rotation: Rotation::Never,
            keep_days: 1,
        }),
        ..LogOptions::default()
    }
}

fn file_contents() -> String {
    std::fs::read_to_string(log_path()).expect("log file")
}

#[tokio::test]
async fn file_appender_writes() {
    let app = Navree::app()
        .module(LoggerModule::for_root(options()))
        .boot()
        .await
        .expect("boot");
    tracing::info!(
        marker = "file-appender-writes",
        token = "t0p",
        "to the file"
    );
    app.shutdown().await.expect("shutdown flushes the file");
    let contents = file_contents();
    assert!(contents.contains("file-appender-writes"), "{contents}");
    assert!(
        !contents.contains("t0p"),
        "redacted in files too: {contents}"
    );
}

#[tokio::test]
async fn file_logging_continues_after_an_app_shuts_down() {
    let first = Navree::app()
        .module(LoggerModule::for_root(options()))
        .boot()
        .await
        .expect("first boot");
    first.shutdown().await.expect("first shutdown");

    let second = Navree::app()
        .module(LoggerModule::for_root(options()))
        .boot()
        .await
        .expect("second boot");
    tracing::info!(marker = "after-first-shutdown", "still logging");
    second.shutdown().await.expect("second shutdown");
    let contents = file_contents();
    assert!(contents.contains("after-first-shutdown"), "{contents}");
}
