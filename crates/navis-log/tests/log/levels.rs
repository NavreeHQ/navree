//! Level filters: a default level, per-target overrides from the options,
//! and runtime changes through `LogControl`.

use std::collections::BTreeMap;

use navis_log::{CaptureWriter, LogError, LogOptions, build_subscriber};

use crate::support::{capture, pretty};

fn with_levels(level: &str, levels: &[(&str, &str)]) -> LogOptions {
    LogOptions {
        level: level.to_owned(),
        levels: levels
            .iter()
            .map(|(target, level)| ((*target).to_owned(), (*level).to_owned()))
            .collect::<BTreeMap<_, _>>(),
        ..pretty()
    }
}

#[test]
fn default_level_filters_lower_levels() {
    let out = capture(&with_levels("info", &[]), |_| {
        tracing::debug!(target: "app", "debug-hidden");
        tracing::info!(target: "app", "info-shown");
    });
    assert!(out.contains("info-shown"), "{out}");
    assert!(!out.contains("debug-hidden"), "{out}");
}

#[test]
fn level_override_per_target() {
    let options = with_levels("info", &[("chatty", "debug"), ("noisy::db", "warn")]);
    let out = capture(&options, |_| {
        tracing::debug!(target: "chatty", "chatty-debug");
        tracing::debug!(target: "chatty::inner", "chatty-inner-debug");
        tracing::info!(target: "noisy::db", "db-info");
        tracing::warn!(target: "noisy::db", "db-warn");
        tracing::info!(target: "noisy::http", "http-info");
        tracing::debug!(target: "other", "other-debug");
        tracing::info!(target: "other", "other-info");
    });
    assert!(out.contains("chatty-debug"), "{out}");
    assert!(out.contains("chatty-inner-debug"), "{out}");
    assert!(!out.contains("db-info"), "{out}");
    assert!(out.contains("db-warn"), "{out}");
    assert!(out.contains("http-info"), "{out}");
    assert!(!out.contains("other-debug"), "{out}");
    assert!(out.contains("other-info"), "{out}");
}

#[test]
fn level_names_are_case_insensitive() {
    let out = capture(&with_levels("WARN", &[("app", "Debug")]), |_| {
        tracing::info!(target: "other", "other-info");
        tracing::debug!(target: "app", "app-debug");
    });
    assert!(!out.contains("other-info"), "{out}");
    assert!(out.contains("app-debug"), "{out}");
}

#[test]
fn set_level_at_runtime() {
    let out = capture(&with_levels("info", &[]), |control| {
        tracing::debug!(target: "billing", "before-change");
        control.set_level("billing", "debug").expect("set level");
        tracing::debug!(target: "billing", "after-change");
        tracing::debug!(target: "other", "other-debug");
        control.set_level("billing", "off").expect("set level");
        tracing::error!(target: "billing", "after-off");
    });
    assert!(!out.contains("before-change"), "{out}");
    assert!(out.contains("after-change"), "{out}");
    assert!(!out.contains("other-debug"), "{out}");
    assert!(!out.contains("after-off"), "{out}");
}

#[test]
fn set_default_level_at_runtime() {
    let out = capture(&with_levels("info", &[("pinned", "info")]), |control| {
        control
            .set_default_level("trace")
            .expect("set default level");
        tracing::trace!(target: "anything", "anything-trace");
        tracing::debug!(target: "pinned", "pinned-debug");
    });
    assert!(out.contains("anything-trace"), "{out}");
    assert!(!out.contains("pinned-debug"), "{out}");
}

#[test]
fn directives_show_default_then_targets() {
    let options = with_levels("info", &[("b", "warn"), ("a::x", "debug")]);
    capture(&options, |control| {
        assert_eq!(control.directives(), "info,a::x=debug,b=warn");
        control
            .set_level("navis::request", "trace")
            .expect("set level");
        assert_eq!(
            control.directives(),
            "info,a::x=debug,b=warn,navis::request=trace"
        );
    });
}

#[test]
fn set_level_rejects_unknown_level() {
    capture(&pretty(), |control| {
        let error = control.set_level("app", "loud").expect_err("invalid level");
        assert!(
            matches!(&error, LogError::InvalidLevel { level } if level == "loud"),
            "{error:?}"
        );
        assert_eq!(control.directives(), "info", "unchanged after an error");
    });
}

#[test]
fn set_level_rejects_invalid_targets() {
    capture(&pretty(), |control| {
        for target in ["", "a b", "a=b", "a,b", "a[span]", "a{x}"] {
            let error = control
                .set_level(target, "debug")
                .expect_err("invalid target");
            assert!(
                matches!(&error, LogError::InvalidTarget { target: t } if t == target),
                "{target:?}: {error:?}"
            );
        }
        assert_eq!(control.directives(), "info");
    });
}

#[test]
fn invalid_levels_in_options_fail_the_build() {
    let error = build_subscriber(&with_levels("chatty", &[]), CaptureWriter::new())
        .expect_err("invalid default level");
    assert!(
        matches!(&error, LogError::InvalidLevel { level } if level == "chatty"),
        "{error:?}"
    );
    let error = build_subscriber(
        &with_levels("info", &[("a b", "debug")]),
        CaptureWriter::new(),
    )
    .expect_err("invalid target");
    assert!(matches!(error, LogError::InvalidTarget { .. }), "{error:?}");
}
