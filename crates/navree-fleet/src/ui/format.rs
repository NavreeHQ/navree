//! How the dashboard writes numbers, durations and times.

use chrono::{DateTime, SecondsFormat, Utc};
use navree_ui::{Node, el};

use crate::FleetStatus;

/// `1,620`: a count with a comma between thousands.
pub(super) fn number(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// `42 s`, `3 min 5 s`, `2 h 10 min`.
pub(super) fn duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs} s")
    } else if secs < 3600 {
        format!("{} min {} s", secs / 60, secs % 60)
    } else {
        format!("{} h {} min", secs / 3600, secs % 3600 / 60)
    }
}

/// `850 ms`, `1.20 s`, `12.3 s`.
pub(super) fn millis(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 10_000 {
        format!("{}.{:02} s", ms / 1000, ms % 1000 / 10)
    } else {
        format!("{}.{} s", ms / 1000, ms % 1000 / 100)
    }
}

/// `09:41`: the minute a chart's bar stands for.
pub(super) fn minute(at: DateTime<Utc>) -> String {
    at.format("%H:%M").to_string()
}

/// A moment as a `<time>`: `2026-11-06 09:41:07 UTC`, with the exact instant
/// in `datetime`.
pub(super) fn when(at: DateTime<Utc>) -> Node {
    el("time")
        .attr("datetime", at.to_rfc3339_opts(SecondsFormat::Secs, true))
        .child(at.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .into()
}

/// `3 min ago`, from `now`; `just now` for a moment that is not yet past.
pub(super) fn ago(now: DateTime<Utc>, at: DateTime<Utc>) -> String {
    match u64::try_from((now - at).num_seconds()) {
        Ok(secs) if secs > 0 => format!("{} ago", duration(secs)),
        _ => "just now".to_owned(),
    }
}

/// `1 master`, `2 masters`.
pub(super) fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The first line of `text`.
pub(super) fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}

/// Running, paused or inactive, in words.
pub(super) fn status(status: FleetStatus) -> &'static str {
    match status {
        FleetStatus::Running => "Running",
        FleetStatus::Paused => "Paused",
        FleetStatus::Inactive => "Inactive",
    }
}

/// `id` as one path segment: everything but letters, digits and `-._~` is
/// written as `%XX`.
pub(super) fn segment(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `text` with places to break in a long run without a space: after a `.`,
/// `:`, `_` or `/` once the run is 16 characters long, and after 40
/// characters whatever they are. A job type or a path then wraps in a narrow
/// column instead of widening the page. The breaks are invisible, and text
/// with room to wrap already is left as it is.
pub(super) fn breakable(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    let mut run = 0;
    for c in text.chars() {
        out.push(c);
        if c.is_whitespace() {
            run = 0;
            continue;
        }
        run += 1;
        if (matches!(c, '.' | ':' | '_' | '/') && run >= 16) || run >= 40 {
            out.push('\u{200b}');
            run = 0;
        }
    }
    out
}

/// `text` as a query value: escaped as a path segment is.
pub(super) fn query_value(text: &str) -> String {
    segment(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_have_commas_between_thousands() {
        assert_eq!(number(0), "0");
        assert_eq!(number(999), "999");
        assert_eq!(number(1_000), "1,000");
        assert_eq!(number(1_620), "1,620");
        assert_eq!(number(12_345_678), "12,345,678");
    }

    #[test]
    fn durations_read_in_the_largest_unit_that_fits() {
        assert_eq!(duration(0), "0 s");
        assert_eq!(duration(59), "59 s");
        assert_eq!(duration(185), "3 min 5 s");
        assert_eq!(duration(7_800), "2 h 10 min");
    }

    #[test]
    fn runtimes_read_in_milliseconds_then_seconds() {
        assert_eq!(millis(850), "850 ms");
        assert_eq!(millis(1_200), "1.20 s");
        assert_eq!(millis(12_340), "12.3 s");
    }

    #[test]
    fn segments_and_query_values_are_escaped() {
        assert_eq!(segment("01J-abc_9.x~"), "01J-abc_9.x~");
        assert_eq!(segment("a/b c"), "a%2Fb%20c");
        assert_eq!(query_value("mail server&x=1"), "mail%20server%26x%3D1");
    }

    #[test]
    fn long_names_get_places_to_break() {
        assert_eq!(breakable("contacts.export"), "contacts.export");
        assert_eq!(breakable("plain text"), "plain text");
        assert_eq!(
            breakable("events.listener.workspace.member_added"),
            "events.listener.\u{200b}workspace.member_\u{200b}added"
        );
        let unbroken = "x".repeat(45);
        assert_eq!(
            breakable(&unbroken),
            format!("{}\u{200b}xxxxx", "x".repeat(40))
        );
    }

    #[test]
    fn the_first_line_of_an_error_is_kept() {
        assert_eq!(first_line("timed out\nstack trace"), "timed out");
        assert_eq!(first_line(""), "");
    }
}
