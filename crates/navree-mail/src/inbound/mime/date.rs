//! `Date` values (RFC 5322 section 3.3, with the obsolete forms of
//! section 4.3): `Tue, 27 Oct 2026 09:14:00 +0000`, a day name optional,
//! seconds optional, two-digit years, named zones, comments anywhere.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

/// The date in UTC. `None` when the value isn't a date. A missing zone is
/// read as UTC; a day past the end of its month rolls over into the next.
pub(super) fn parse(value: &str) -> Option<DateTime<Utc>> {
    let text = without_comments(value);
    let mut tokens = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|token| !token.is_empty())
        .skip_while(|token| !token.starts_with(|c: char| c.is_ascii_digit()));

    let day: u32 = number(tokens.next()?, 2)?;
    let month_token = tokens.next()?;
    let month = month(month_token).or_else(|| number(month_token, 2))?;
    let year = year(tokens.next()?)?;

    // `09:14:00`, perhaps with the zone stuck to it.
    let time_token = tokens.next()?;
    let (time, glued_zone) = match time_token.find(['+', '-']) {
        Some(at) if at > 0 => (&time_token[..at], Some(&time_token[at..])),
        _ => (time_token, None),
    };
    let mut clock = time.split(':');
    let hour: u32 = number(clock.next()?, 2)?;
    let minute: u32 = number(clock.next()?, 2)?;
    let second: u32 = match clock.next() {
        Some(second) => number(second.split('.').next().unwrap_or(second), 2)?,
        None => 0,
    };
    if clock.next().is_some() {
        return None;
    }
    let offset = match glued_zone.or_else(|| tokens.next()) {
        Some(zone) => zone_offset(zone),
        None => 0,
    };

    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let days = NaiveDate::from_ymd_opt(year, month, 1)?
        .signed_duration_since(NaiveDate::from_ymd_opt(1970, 1, 1)?)
        .num_days()
        + i64::from(day - 1);
    let seconds = days * 86_400 + i64::from(hour * 3600 + minute * 60 + second) - offset;
    Utc.timestamp_opt(seconds, 0).single()
}

/// The value with its comments (nested, with escapes) replaced by spaces.
fn without_comments(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut depth = 0usize;
    let mut escaped = false;
    for c in value.chars() {
        if depth > 0 {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            continue;
        }
        if c == '(' {
            depth = 1;
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

/// A number of at most `digits` digits.
fn number(token: &str, digits: usize) -> Option<u32> {
    if token.is_empty() || token.len() > digits || !token.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    token.parse().ok()
}

/// A month by its name (the first three letters are enough).
fn month(token: &str) -> Option<u32> {
    let prefix = token.get(..3)?.to_ascii_lowercase();
    let index = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ]
    .iter()
    .position(|name| *name == prefix)?;
    u32::try_from(index + 1).ok()
}

/// A year: two digits are 1950 to 2049, three digits count from 1900.
fn year(token: &str) -> Option<i32> {
    let year = i32::try_from(number(token, 4)?).ok()?;
    Some(match token.len() {
        1 | 2 if year < 50 => year + 2000,
        1 | 2 => year + 1900,
        3 => year + 1900,
        _ => year,
    })
}

/// The zone's offset from UTC in seconds: `+hhmm`, `-hhmm`, or a name
/// (`GMT`, `EST`, ...). Names it doesn't know, and military letters,
/// count as UTC.
fn zone_offset(zone: &str) -> i64 {
    let sign = match zone.as_bytes().first() {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => {
            return match zone.to_ascii_uppercase().as_str() {
                "EDT" => -4 * 3600,
                "EST" | "CDT" => -5 * 3600,
                "CST" | "MDT" => -6 * 3600,
                "MST" | "PDT" => -7 * 3600,
                "PST" => -8 * 3600,
                _ => 0,
            };
        }
    };
    let digits: String = zone[1..]
        .chars()
        .take_while(char::is_ascii_digit)
        .take(4)
        .collect();
    let Ok(value) = digits.parse::<i64>() else {
        return 0;
    };
    let (hours, minutes) = if digits.len() <= 2 {
        (value, 0)
    } else {
        (value / 100, value % 100)
    };
    sign * ((hours % 24) * 3600 + (minutes % 60) * 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> Option<DateTime<Utc>> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).single()
    }

    #[test]
    fn dates_in_every_form_mail_uses() {
        for (value, expected) in [
            (
                "Tue, 27 Oct 2026 09:14:00 +0000",
                at(2026, 10, 27, 9, 14, 0),
            ),
            (
                "Tue, 27 Oct 2026 13:00:00 +0100",
                at(2026, 10, 27, 12, 0, 0),
            ),
            ("27 Oct 2026 09:14 -0530", at(2026, 10, 27, 14, 44, 0)),
            ("Tue,27 oct 26 09:14:00 GMT", at(2026, 10, 27, 9, 14, 0)),
            ("Fri, 21 Nov 97 09:55:06 -0600", at(1997, 11, 21, 15, 55, 6)),
            (
                "Thu, 13 Feb 1969 23:32 -0330 (Newfoundland Time)",
                at(1969, 2, 14, 3, 2, 0),
            ),
            ("Mon, 1 Jan 2001 00:00:00 EST", at(2001, 1, 1, 5, 0, 0)),
            ("Mon, 01 January 2001 10:00:00 Z", at(2001, 1, 1, 10, 0, 0)),
            (" 1 Jan 2001 10:00:00", at(2001, 1, 1, 10, 0, 0)),
            ("1 Jan 2001 10:00:00+0200", at(2001, 1, 1, 8, 0, 0)),
            (
                "(c) 1 (x) Jan 2001 10:00:00.123 +0000",
                at(2001, 1, 1, 10, 0, 0),
            ),
            ("31 Feb 2026 10:00:00 +0000", at(2026, 3, 3, 10, 0, 0)),
            ("1 Jan 101 00:00:00 +0000", at(2001, 1, 1, 0, 0, 0)),
        ] {
            assert_eq!(parse(value), expected, "{value:?}");
        }
    }

    #[test]
    fn what_is_not_a_date() {
        for value in [
            "",
            "yesterday",
            "Tue Oct 27 09:14:00 2026",
            "27 Foo 2026 09:14:00 +0000",
            "27 Oct 2026",
            "32 Oct 2026 09:14:00 +0000",
            "27 Oct 2026 24:00:00 +0000",
            "27 Oct 2026 09:14:00:00 +0000",
        ] {
            assert_eq!(parse(value), None, "{value:?}");
        }
    }
}
