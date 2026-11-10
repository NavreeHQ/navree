//! The UTC date and time used to name migrations (no time-zone database
//! needed: migration names use UTC).

use std::time::{SystemTime, UNIX_EPOCH};

/// A UTC date and time, to the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    /// The year, e.g. 2026.
    pub year: i64,
    /// The month, 1 to 12.
    pub month: u32,
    /// The day of the month, 1 to 31.
    pub day: u32,
    /// The hour, 0 to 23.
    pub hour: u32,
    /// The minute, 0 to 59.
    pub minute: u32,
    /// The second, 0 to 59.
    pub second: u32,
}

impl Timestamp {
    /// The current time (the Unix epoch if the system clock is before it).
    pub fn now() -> Self {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self::from_unix(i64::try_from(secs).unwrap_or(i64::MAX))
    }

    /// The UTC time `secs` seconds after the Unix epoch.
    pub fn from_unix(secs: i64) -> Self {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        // `rem` is below 86 400, so the casts are lossless.
        Self {
            year,
            month,
            day,
            hour: (rem / 3_600) as u32,
            minute: (rem % 3_600 / 60) as u32,
            second: (rem % 60) as u32,
        }
    }

    /// `m20261025_143005`: the prefix of migration names, which sort by it.
    pub fn migration_prefix(&self) -> String {
        format!(
            "m{:04}{:02}{:02}_{:02}{:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

/// Howard Hinnant's `civil_from_days`: the proleptic Gregorian date of a
/// day count relative to 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_unix_seconds_to_utc_fields() {
        assert_eq!(
            Timestamp::from_unix(0),
            Timestamp {
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0
            }
        );
        // 2026-10-25T14:30:05Z
        let t = Timestamp::from_unix(1_792_938_605);
        assert_eq!((t.year, t.month, t.day), (2026, 10, 25));
        assert_eq!((t.hour, t.minute, t.second), (14, 30, 5));
        // A leap day.
        let leap = Timestamp::from_unix(951_782_400);
        assert_eq!((leap.year, leap.month, leap.day), (2000, 2, 29));
    }

    #[test]
    fn migration_prefix_is_sortable() {
        let t = Timestamp::from_unix(1_792_938_605);
        assert_eq!(t.migration_prefix(), "m20261025_143005");
    }
}
