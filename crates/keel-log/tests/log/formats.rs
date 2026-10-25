//! Output formats: one JSON object per line, or human-readable lines.

use serde_json::Value;

use crate::support::{capture, json, pretty};

/// Parses every captured line as JSON.
fn json_lines(out: &str) -> Vec<Value> {
    out.lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}")))
        .collect()
}

#[test]
fn json_format_has_level_target_fields() {
    let out = capture(&json(), |_| {
        tracing::info!(target: "keel::test", answer = 42, ok = true, ratio = 0.5, "hello");
    });
    let lines = json_lines(&out);
    assert_eq!(lines.len(), 1, "{out}");
    let line = &lines[0];
    assert_eq!(line["level"], "INFO", "{out}");
    assert_eq!(line["target"], "keel::test", "{out}");
    assert_eq!(line["fields"]["message"], "hello", "{out}");
    assert_eq!(line["fields"]["answer"], 42, "{out}");
    assert_eq!(line["fields"]["ok"], true, "{out}");
    assert_eq!(line["fields"]["ratio"], 0.5, "{out}");
    let timestamp = line["timestamp"].as_str().expect("timestamp string");
    assert!(timestamp.ends_with('Z') && timestamp.contains('T'), "{out}");
}

#[test]
fn json_lines_carry_span_context() {
    let out = capture(&json(), |_| {
        let request = tracing::info_span!(
            "request",
            request_id = "r-1",
            user_id = tracing::field::Empty
        );
        request.record("user_id", 7);
        let _request = request.enter();
        let step = tracing::info_span!("step", n = 2);
        let _step = step.enter();
        tracing::warn!("inside");
    });
    let lines = json_lines(&out);
    let line = &lines[0];
    assert_eq!(line["level"], "WARN", "{out}");
    assert_eq!(line["span"]["name"], "step", "{out}");
    assert_eq!(line["span"]["n"], 2, "{out}");
    assert_eq!(line["spans"][0]["name"], "request", "{out}");
    assert_eq!(line["spans"][0]["request_id"], "r-1", "{out}");
    assert_eq!(line["spans"][0]["user_id"], 7, "{out}");
    assert_eq!(line["spans"][1]["name"], "step", "{out}");
}

#[test]
fn json_events_outside_spans_have_no_span_keys() {
    let out = capture(&json(), |_| tracing::info!("bare"));
    let line = &json_lines(&out)[0];
    assert!(line.get("span").is_none(), "{out}");
    assert!(line.get("spans").is_none(), "{out}");
}

#[test]
fn json_format_redacts_event_and_span_fields() {
    let out = capture(&json(), |_| {
        let span = tracing::info_span!("request", authorization = "Bearer xyz");
        let _entered = span.enter();
        tracing::info!(password = "hunter2", count = 3_u64, "login");
    });
    assert!(!out.contains("hunter2"), "{out}");
    assert!(!out.contains("Bearer xyz"), "{out}");
    let line = &json_lines(&out)[0];
    assert_eq!(line["fields"]["password"], "[redacted]", "{out}");
    assert_eq!(line["fields"]["count"], 3, "{out}");
    assert_eq!(line["spans"][0]["authorization"], "[redacted]", "{out}");
}

#[test]
fn json_errors_include_their_source_chain() {
    #[derive(Debug)]
    struct Outer(std::io::Error);
    impl std::fmt::Display for Outer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("query failed")
        }
    }
    impl std::error::Error for Outer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }
    let error = Outer(std::io::Error::other("connection reset"));
    let out = capture(&json(), |_| {
        tracing::error!(error = &error as &(dyn std::error::Error + 'static), "boom");
    });
    let line = &json_lines(&out)[0];
    assert_eq!(
        line["fields"]["error"], "query failed: connection reset",
        "{out}"
    );
}

#[test]
fn pretty_format_writes_one_readable_line() {
    let out = capture(&pretty(), |_| {
        tracing::info!(target: "keel::test", answer = 42, "hello");
    });
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(out.contains("INFO"), "{out}");
    assert!(out.contains("keel::test"), "{out}");
    assert!(out.contains("hello answer=42"), "{out}");
    assert!(
        !out.contains('\u{1b}'),
        "no ANSI escapes in captured output: {out}"
    );
}
