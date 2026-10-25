//! The rotating log file: same lines as the main output, flushed on demand.

use std::path::Path;

use navree_log::{FileLog, LogFormat, LogOptions, Rotation};
use serde_json::Value;

use crate::support::{capture, json, pretty};

fn with_file(base: LogOptions, path: &Path, rotation: Rotation) -> LogOptions {
    LogOptions {
        file: Some(FileLog {
            path: path.to_owned(),
            rotation,
            keep_days: 2,
        }),
        ..base
    }
}

/// The names of the files in `dir`.
fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn file_gets_the_same_redacted_lines_after_flush() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("app.log");
    let options = with_file(json(), &path, Rotation::Never);
    let out = capture(&options, |control| {
        tracing::info!(password = "hunter2", "to-file");
        control.flush().expect("flush");
    });
    assert!(out.contains("to-file"), "{out}");
    let written = std::fs::read_to_string(&path).expect("log file");
    let line: Value = serde_json::from_str(written.trim_end()).expect("one JSON line");
    assert_eq!(line["fields"]["message"], "to-file", "{written}");
    assert_eq!(line["fields"]["password"], "[redacted]", "{written}");
    assert_eq!(options.format, LogFormat::Json);
}

#[test]
fn file_lines_are_plain_text_in_pretty_format() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("app.log");
    capture(&with_file(pretty(), &path, Rotation::Never), |control| {
        tracing::warn!(attempt = 2, "retrying");
        control.flush().expect("flush");
    });
    let written = std::fs::read_to_string(&path).expect("log file");
    assert!(written.contains("WARN"), "{written}");
    assert!(written.contains("retrying attempt=2"), "{written}");
    assert!(
        !written.contains('\u{1b}'),
        "no ANSI escapes in files: {written:?}"
    );
}

#[test]
fn daily_files_carry_the_date_before_the_extension() {
    let dir = tempfile::tempdir().expect("tempdir");
    let options = with_file(pretty(), &dir.path().join("navree.log"), Rotation::Daily);
    capture(&options, |control| {
        tracing::info!("dated");
        control.flush().expect("flush");
    });
    let names = file_names(dir.path());
    assert_eq!(names.len(), 1, "{names:?}");
    let name = &names[0];
    // navree.YYYY-MM-DD.log
    assert!(
        name.starts_with("navree.") && name.ends_with(".log"),
        "{name}"
    );
    assert_eq!(name.len(), "navree.2026-10-25.log".len(), "{name}");
}

#[test]
fn missing_log_directory_is_created() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("storage").join("logs").join("navree.log");
    capture(&with_file(pretty(), &path, Rotation::Never), |control| {
        tracing::info!("nested");
        control.flush().expect("flush");
    });
    let written = std::fs::read_to_string(&path).expect("log file");
    assert!(written.contains("nested"), "{written}");
}

#[test]
fn flush_without_a_file_is_a_no_op() {
    capture(&pretty(), |control| control.flush().expect("flush"));
}

#[test]
fn no_file_lines_are_dropped_when_the_writer_keeps_up() {
    let dir = tempfile::tempdir().expect("tempdir");
    let options = with_file(pretty(), &dir.path().join("app.log"), Rotation::Never);
    capture(&options, |control| {
        for i in 0..100 {
            tracing::info!(i, "line");
        }
        control.flush().expect("flush");
        assert_eq!(control.dropped_file_lines(), 0);
    });
    let written = std::fs::read_to_string(dir.path().join("app.log")).expect("log file");
    assert_eq!(written.lines().count(), 100);
    capture(&pretty(), |control| {
        assert_eq!(control.dropped_file_lines(), 0)
    });
}
