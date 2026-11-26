//! `Retry-After` (RFC 9110, section 10.2.3): how long a response asks the
//! client to wait before trying again.

use std::time::{Duration, SystemTime};

use navree_datetime::{DateTime, Timestamp};

/// The wait a `Retry-After` value asks for: a number of seconds, or an HTTP
/// date (a date in the past asks for no wait). `None` when the value is
/// neither.
pub(crate) fn parse_retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        // More digits than a u64 holds is "very long": saturate.
        let secs = value.parse::<u64>().unwrap_or(u64::MAX);
        return Some(Duration::from_secs(secs));
    }
    let date = http_date(value)?;
    Some(
        (date - Timestamp::from(now))
            .to_std()
            .unwrap_or(Duration::ZERO),
    )
}

/// An HTTP date: the preferred IMF-fixdate (`Sun, 06 Nov 1994 08:49:37
/// GMT`), and the obsolete RFC 850 (`Sunday, 06-Nov-94 08:49:37 GMT`) and
/// asctime (`Sun Nov  6 08:49:37 1994`) forms recipients must accept.
fn http_date(value: &str) -> Option<Timestamp> {
    Timestamp::parse_rfc2822(value)
        .ok()
        .or_else(|| {
            DateTime::parse("%A, %d-%b-%y %H:%M:%S GMT", value)
                .ok()
                .map(DateTime::in_utc)
        })
        .or_else(|| {
            DateTime::parse("%a %b %e %H:%M:%S %Y", value)
                .ok()
                .map(DateTime::in_utc)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    /// 1994-11-06T08:49:37Z, the date of RFC 9110's examples.
    const RFC_DATE: u64 = 784_111_777;

    #[test]
    fn seconds_are_read_as_given() {
        let now = at(0);
        assert_eq!(
            parse_retry_after("120", now),
            Some(Duration::from_secs(120))
        );
        assert_eq!(parse_retry_after(" 0 ", now), Some(Duration::ZERO));
        assert_eq!(
            parse_retry_after("99999999999999999999999", now),
            Some(Duration::from_secs(u64::MAX))
        );
    }

    #[test]
    fn the_three_date_forms_are_read() {
        let now = at(RFC_DATE - 30);
        for value in [
            "Sun, 06 Nov 1994 08:49:37 GMT",
            "Sunday, 06-Nov-94 08:49:37 GMT",
            "Sun Nov  6 08:49:37 1994",
        ] {
            assert_eq!(
                parse_retry_after(value, now),
                Some(Duration::from_secs(30)),
                "{value}"
            );
        }
    }

    #[test]
    fn a_date_in_the_past_asks_for_no_wait() {
        assert_eq!(
            parse_retry_after("Sun, 06 Nov 1994 08:49:37 GMT", at(RFC_DATE + 60)),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn anything_else_is_not_a_retry_after() {
        for value in ["", "soon", "-5", "1.5", "5s", "Sun, 06 Nov"] {
            assert_eq!(parse_retry_after(value, at(0)), None, "{value}");
        }
    }
}
