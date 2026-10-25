//! The `money` and `date` filters.

use keel_view::{ViewError, Views};
use serde_json::{Value, json};

/// Renders `source` as a `.txt` template with `ctx`.
fn render(source: &str, ctx: Value) -> Result<String, ViewError> {
    Views::from_templates([("t.txt", source)])
        .expect("views")
        .render("t", ctx)
}

#[test]
fn money_filter() {
    let cases = [
        (123_456, "USD", "$1,234.56"),
        (-500, "EUR", "-€5.00"),
        (0, "GBP", "£0.00"),
        (5, "USD", "$0.05"),
        (123_456_789, "INR", "₹1,234,567.89"),
        (150_000, "JPY", "¥150,000"),
        (1_234_567, "KWD", "KWD 1,234.567"),
        (1_999, "chf", "CHF 19.99"),
        (i64::MIN, "USD", "-$92,233,720,368,547,758.08"),
    ];
    for (cents, currency, expected) in cases {
        let out = render(
            "{{ cents|money(currency) }}",
            json!({ "cents": cents, "currency": currency }),
        )
        .expect("render");
        assert_eq!(out, expected, "{cents} {currency}");
    }
}

#[test]
fn money_filter_renders_none_as_empty() {
    let out = render("[{{ cents|money('USD') }}]", json!({ "cents": null })).expect("render");

    assert_eq!(out, "[]");
}

#[test]
fn money_filter_rejects_bad_input() {
    for (cents, currency) in [
        (json!(12.5), json!("USD")),
        (json!("12"), json!("USD")),
        (json!(100), json!("US")),
        (json!(100), json!("U$D")),
    ] {
        let result = render(
            "{{ cents|money(currency) }}",
            json!({ "cents": cents, "currency": currency }),
        );
        let error = result.expect_err("bad money input");
        assert!(matches!(error, ViewError::Template { .. }), "{error:?}");
        assert!(
            error.to_string().contains("money filter:"),
            "{cents} {currency}: {error}"
        );
    }
}

#[test]
fn date_filter_formats_dates_and_times() {
    let cases = [
        (
            json!("2026-10-25T14:30:00Z"),
            "%d %b %Y %H:%M",
            "25 Oct 2026 14:30",
        ),
        (
            json!("2026-10-25T14:30:00.123456789Z"),
            "%Y-%m-%d %H:%M:%S",
            "2026-10-25 14:30:00",
        ),
        (
            json!("2026-10-25T14:30:00+05:30"),
            "%H:%M %z",
            "14:30 +0530",
        ),
        (json!("2026-10-25T14:30:00"), "%H:%M", "14:30"),
        (json!("2026-10-25 14:30:00.5"), "%H:%M:%S", "14:30:00"),
        (json!("2026-10-25"), "%B %-d, %Y", "October 25, 2026"),
        (json!(0), "%Y-%m-%d %H:%M:%S", "1970-01-01 00:00:00"),
    ];
    for (value, format, expected) in cases {
        let out = render(
            "{{ value|date(format) }}",
            json!({ "value": value, "format": format }),
        )
        .expect("render");
        assert_eq!(out, expected, "{value} {format}");
    }
}

#[test]
fn date_filter_defaults_to_iso_date() {
    let out = render(
        "{{ value|date }}",
        json!({ "value": "2026-10-25T23:59:59Z" }),
    )
    .expect("render");

    assert_eq!(out, "2026-10-25");
}

#[test]
fn date_filter_accepts_chrono_values() {
    let at = chrono::DateTime::parse_from_rfc3339("2026-10-25T08:05:00Z")
        .expect("timestamp")
        .with_timezone(&chrono::Utc);
    let day = chrono::NaiveDate::from_ymd_opt(2026, 1, 2).expect("date");

    let out = render(
        "{{ at|date('%H:%M') }} {{ day|date('%d/%m/%Y') }}",
        json!({ "at": at, "day": day }),
    )
    .expect("render");

    assert_eq!(out, "08:05 02/01/2026");
}

#[test]
fn date_filter_renders_none_as_empty() {
    let out = render("[{{ value|date }}]", json!({ "value": null })).expect("render");

    assert_eq!(out, "[]");
}

#[test]
fn date_filter_rejects_bad_input() {
    for (value, format) in [
        (json!("not a date"), "%Y"),
        (json!(true), "%Y"),
        (json!("2026-10-25"), "%H:%M"),
        (json!("2026-10-25"), "%Q"),
    ] {
        let result = render(
            "{{ value|date(format) }}",
            json!({ "value": value, "format": format }),
        );
        let error = result.expect_err("bad date input");
        assert!(matches!(error, ViewError::Template { .. }), "{error:?}");
        assert!(
            error.to_string().contains("date filter:"),
            "{value} {format}: {error}"
        );
    }
}

#[test]
fn filters_reject_undefined_values() {
    for source in ["{{ missing|money('USD') }}", "{{ missing|date }}"] {
        let result = render(source, json!({}));

        assert!(
            matches!(result, Err(ViewError::Template { .. })),
            "{source}: {result:?}"
        );
    }
}
