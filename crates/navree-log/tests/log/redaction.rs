//! Fields whose name contains a redact key are written as `[redacted]`.

use navree_log::LogOptions;

use crate::support::{capture, pretty};

#[test]
fn redacts_configured_fields() {
    let out = capture(&pretty(), |_| {
        tracing::info!(
            user = "bob",
            password = "hunter2",
            api_token = "tok-123",
            "signed in"
        );
    });
    assert!(out.contains("signed in"), "{out}");
    assert!(out.contains("user=\"bob\""), "{out}");
    assert!(out.contains("password=[redacted]"), "{out}");
    assert!(out.contains("api_token=[redacted]"), "{out}");
    assert!(!out.contains("hunter2"), "{out}");
    assert!(!out.contains("tok-123"), "{out}");
}

#[test]
fn redacts_span_fields() {
    let out = capture(&pretty(), |_| {
        let span = tracing::info_span!("request", session_cookie = "abc123", path = "/login");
        let _entered = span.enter();
        tracing::info!("handled");
    });
    assert!(out.contains("handled"), "{out}");
    assert!(out.contains("session_cookie=[redacted]"), "{out}");
    assert!(out.contains("path=\"/login\""), "{out}");
    assert!(!out.contains("abc123"), "{out}");
}

#[test]
fn redacts_fields_recorded_on_a_span_later() {
    let out = capture(&pretty(), |_| {
        let span = tracing::info_span!("login", client_secret = tracing::field::Empty);
        span.record("client_secret", "shh-later");
        let _entered = span.enter();
        tracing::info!("checked");
    });
    assert!(out.contains("client_secret=[redacted]"), "{out}");
    assert!(!out.contains("shh-later"), "{out}");
}

#[test]
fn redaction_ignores_ascii_case_and_value_type() {
    let out = capture(&pretty(), |_| {
        tracing::info!(
            Authorization = "Bearer abc",
            csrf_TOKEN = 4242_u64,
            secret_flag = true,
            cookie_jar = ?vec!["c1"],
            "request"
        );
    });
    assert!(out.contains("Authorization=[redacted]"), "{out}");
    assert!(out.contains("csrf_TOKEN=[redacted]"), "{out}");
    assert!(out.contains("secret_flag=[redacted]"), "{out}");
    assert!(out.contains("cookie_jar=[redacted]"), "{out}");
    assert!(!out.contains("Bearer abc"), "{out}");
    assert!(!out.contains("4242"), "{out}");
    assert!(!out.contains("c1"), "{out}");
}

#[test]
fn redacts_error_values() {
    let error = std::io::Error::other("db password is pa55");
    let out = capture(&pretty(), |_| {
        tracing::error!(
            password_error = &error as &(dyn std::error::Error + 'static),
            "failed"
        );
    });
    assert!(out.contains("password_error=[redacted]"), "{out}");
    assert!(!out.contains("pa55"), "{out}");
}

#[test]
fn custom_redact_list_replaces_defaults() {
    let options = LogOptions {
        redact: vec!["SSN".to_owned()],
        ..pretty()
    };
    let out = capture(&options, |_| {
        tracing::info!(customer_ssn = "123-45-6789", password = "visible", "stored");
    });
    assert!(out.contains("customer_ssn=[redacted]"), "{out}");
    assert!(out.contains("password=\"visible\""), "{out}");
}

#[test]
fn blank_redact_keys_are_ignored() {
    let options = LogOptions {
        redact: vec![String::new(), "  ".to_owned()],
        ..pretty()
    };
    let out = capture(&options, |_| tracing::info!(user = "bob", "plain"));
    assert!(out.contains("user=\"bob\""), "{out}");
}
