//! When held deliveries go out: [`Digest`] frequencies and [`QuietHours`],
//! counted on the recipient's clock.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, NaiveTime, TimeDelta, Timelike, Utc, Weekday};
use navree_time::{LocalDay, SendWindow, Zone};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How often the mail of a notification is batched into one digest mail.
///
/// A notification declares its own with
/// [`Notification::DIGEST`](crate::Notification::DIGEST) (or
/// `#[notification(digest = "daily")]`); people and workspaces choose
/// theirs per category in their [`Preferences`](crate::Preferences). Only
/// the mail channel is batched: the other channels deliver as usual.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Digest {
    /// No digest: each mail goes out on its own (the default).
    #[default]
    Off,
    /// One mail at the top of every hour.
    Hourly,
    /// One mail a day at `notify.digest.at` (09:00).
    Daily,
    /// One mail a week, on `notify.digest.weekly_on` (Monday) at
    /// `notify.digest.at`.
    Weekly,
}

impl Digest {
    /// Every frequency.
    pub const ALL: [Digest; 4] = [Digest::Off, Digest::Hourly, Digest::Daily, Digest::Weekly];

    /// `off`, `hourly`, `daily` or `weekly`.
    pub fn as_str(self) -> &'static str {
        match self {
            Digest::Off => "off",
            Digest::Hourly => "hourly",
            Digest::Daily => "daily",
            Digest::Weekly => "weekly",
        }
    }

    /// Whether mail is batched (any frequency but [`Digest::Off`]).
    pub fn is_on(self) -> bool {
        self != Digest::Off
    }

    /// When the digest that collects a mail sent at `now` goes out, on
    /// `zone`'s clock: the next top of the hour, the next `at` (daily), or
    /// the next `weekly_on` at `at` (weekly). Always after `now`; `None`
    /// for [`Digest::Off`].
    pub fn next_at(
        self,
        now: DateTime<Utc>,
        zone: &Zone,
        at: NaiveTime,
        weekly_on: Weekday,
    ) -> Option<DateTime<Utc>> {
        let today = LocalDay::of(now, zone);
        match self {
            Digest::Off => None,
            Digest::Hourly => {
                let local = zone.local(now).naive_local();
                let hour = local
                    .with_minute(0)
                    .and_then(|t| t.with_second(0))
                    .and_then(|t| t.with_nanosecond(0))
                    .unwrap_or(local);
                // The next local hour that starts after `now` (a DST change
                // can make two local hours map to one instant).
                (1..=3)
                    .map(|hours| zone.to_utc(hour + TimeDelta::hours(hours)))
                    .find(|at| *at > now)
            }
            Digest::Daily => (0..=2)
                .map(|days| today.add_days(days).at(at, zone))
                .find(|instant| *instant > now),
            Digest::Weekly => (0..=8)
                .map(|days| today.add_days(days))
                .filter(|day| day.weekday() == weekly_on)
                .map(|day| day.at(at, zone))
                .find(|instant| *instant > now),
        }
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Digest {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Digest::ALL
            .into_iter()
            .find(|digest| digest.as_str() == s.trim())
            .ok_or_else(|| {
                format!("unknown digest `{s}`: use `off`, `hourly`, `daily` or `weekly`")
            })
    }
}

/// The local hours in which a recipient wants no notifications that
/// interrupt (mail, Slack, Teams, SMS, WhatsApp, web push): deliveries
/// over those channels wait until the quiet hours end. `22:00`–`07:00`
/// spans midnight; a start equal to the end means no quiet hours (a
/// person's way to have none when their workspace sets some).
///
/// ```
/// use navree_notify::QuietHours;
/// use navree_time::Zone;
///
/// let quiet = QuietHours::new("22:00", "07:00").unwrap();
/// let kolkata = Zone::parse("Asia/Kolkata").unwrap();
/// // 23:30 in Kolkata: quiet until 07:00 there (01:30 UTC).
/// let late = "2026-10-29T18:00:00Z".parse().unwrap();
/// assert!(quiet.contains(late, &kolkata));
/// assert_eq!(quiet.ends_after(late, &kolkata).to_rfc3339(), "2026-10-30T01:30:00+00:00");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct QuietHours {
    start: NaiveTime,
    end: NaiveTime,
}

impl QuietHours {
    /// Quiet from `start` to `end`, local 24-hour `HH:MM` times.
    pub fn new(start: &str, end: &str) -> Result<QuietHours, String> {
        Ok(QuietHours {
            start: parse_hh_mm(start)?,
            end: parse_hh_mm(end)?,
        })
    }

    /// Quiet from `start` to `end`.
    pub fn between(start: NaiveTime, end: NaiveTime) -> QuietHours {
        QuietHours { start, end }
    }

    /// When the quiet hours start.
    pub fn start(&self) -> NaiveTime {
        self.start
    }

    /// When they end.
    pub fn end(&self) -> NaiveTime {
        self.end
    }

    /// Whether the start equals the end (no quiet hours at all).
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Whether `at` falls in the quiet hours on `zone`'s clock.
    pub fn contains(&self, at: DateTime<Utc>, zone: &Zone) -> bool {
        self.ends_after(at, zone) != at
    }

    /// When the quiet hours around `at` end on `zone`'s clock, or `at`
    /// itself when it is not in quiet hours.
    pub fn ends_after(&self, at: DateTime<Utc>, zone: &Zone) -> DateTime<Utc> {
        if self.is_empty() {
            return at;
        }
        let today = LocalDay::of(at, zone);
        // The periods that can hold `at`: the one that started yesterday
        // (spanning midnight) and today's.
        for start_day in [today.pred(), today] {
            let starts = start_day.at(self.start, zone);
            let end_day = if self.start < self.end {
                start_day
            } else {
                start_day.succ()
            };
            let ends = end_day.at(self.end, zone);
            if starts <= at && at < ends {
                return ends;
            }
        }
        at
    }
}

impl Serialize for QuietHours {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Raw {
            start: String,
            end: String,
        }
        Raw {
            start: self.start.format("%H:%M").to_string(),
            end: self.end.format("%H:%M").to_string(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for QuietHours {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            start: String,
            end: String,
        }
        let raw = Raw::deserialize(deserializer)?;
        QuietHours::new(&raw.start, &raw.end).map_err(serde::de::Error::custom)
    }
}

/// A local 24-hour `HH:MM` time (`7:05` and `07:05` alike).
pub(crate) fn parse_hh_mm(text: &str) -> Result<NaiveTime, String> {
    let invalid = || format!("`{text}` is not a 24-hour HH:MM time");
    let (hour, minute) = text.trim().split_once(':').ok_or_else(invalid)?;
    let digits = |s: &str, max: usize| {
        (!s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u32>().ok())
            .flatten()
    };
    let hour = digits(hour, 2).ok_or_else(invalid)?;
    let minute = digits(minute, 2)
        .filter(|_| minute.len() == 2)
        .ok_or_else(invalid)?;
    NaiveTime::from_hms_opt(hour, minute, 0).ok_or_else(invalid)
}

/// A weekday by its English name (`monday`, `Mon`).
pub(crate) fn parse_weekday(text: &str) -> Result<Weekday, String> {
    text.trim()
        .parse::<Weekday>()
        .map_err(|_| format!("`{text}` is not a day of the week"))
}

/// The first instant at or after `from` that is inside `window` (when
/// given) and outside `quiet` (when given), on `zone`'s clock.
pub(crate) fn next_open(
    from: DateTime<Utc>,
    zone: &Zone,
    window: Option<&SendWindow>,
    quiet: Option<&QuietHours>,
) -> DateTime<Utc> {
    let mut at = from;
    // Each step only moves forward; a window and quiet hours settle in a
    // few steps (a window that always falls in quiet hours never would).
    for _ in 0..16 {
        let mut next = at;
        if let Some(window) = window {
            next = window.next_slot(next, zone);
        }
        if let Some(quiet) = quiet {
            next = quiet.ends_after(next, zone);
        }
        if next == at {
            return at;
        }
        at = next;
    }
    tracing::warn!(
        target: "navree::notify",
        zone = %zone,
        "the send window never opens outside the quiet hours; using the window alone"
    );
    window.map_or(from, |window| window.next_slot(from, zone))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    fn zone(name: &str) -> Zone {
        Zone::parse(name).unwrap()
    }

    fn nine() -> NaiveTime {
        NaiveTime::from_hms_opt(9, 0, 0).unwrap()
    }

    #[test]
    fn digests_round_trip_through_strings_and_serde() {
        for digest in Digest::ALL {
            assert_eq!(digest.as_str().parse::<Digest>(), Ok(digest));
            let json = serde_json::to_value(digest).unwrap();
            assert_eq!(json, digest.as_str());
        }
        assert!("monthly".parse::<Digest>().is_err());
        assert!(!Digest::Off.is_on());
        assert!(Digest::Daily.is_on());
        assert_eq!(Digest::default(), Digest::Off);
    }

    #[test]
    fn daily_digests_go_out_at_the_next_local_nine() {
        let kolkata = zone("Asia/Kolkata");
        // 08:00 in Kolkata: today 09:00 there.
        let next =
            Digest::Daily.next_at(at("2026-10-29T02:30:00Z"), &kolkata, nine(), Weekday::Mon);
        assert_eq!(next, Some(at("2026-10-29T03:30:00Z")));
        // 09:00 exactly: tomorrow.
        let next =
            Digest::Daily.next_at(at("2026-10-29T03:30:00Z"), &kolkata, nine(), Weekday::Mon);
        assert_eq!(next, Some(at("2026-10-30T03:30:00Z")));
        // The same instant is still yesterday evening in New York.
        let new_york = zone("America/New_York");
        let next =
            Digest::Daily.next_at(at("2026-10-29T02:30:00Z"), &new_york, nine(), Weekday::Mon);
        assert_eq!(next, Some(at("2026-10-29T13:00:00Z")));
        assert_eq!(
            Digest::Off.next_at(at("2026-10-29T02:30:00Z"), &kolkata, nine(), Weekday::Mon),
            None
        );
    }

    #[test]
    fn daily_digests_follow_daylight_saving_changes() {
        // Europe/Paris leaves summer time on Sunday 25 October 2026.
        let paris = zone("Europe/Paris");
        let saturday_noon = at("2026-10-24T10:00:00Z");
        let next = Digest::Daily.next_at(saturday_noon, &paris, nine(), Weekday::Mon);
        // 09:00 CET on Sunday is 08:00 UTC (it was 07:00 UTC on Saturday).
        assert_eq!(next, Some(at("2026-10-25T08:00:00Z")));
    }

    #[test]
    fn hourly_digests_go_out_at_the_next_local_hour() {
        let kolkata = zone("Asia/Kolkata");
        // 10:15 in Kolkata (+05:30): 11:00 there, 05:30 UTC.
        let next =
            Digest::Hourly.next_at(at("2026-10-29T04:45:00Z"), &kolkata, nine(), Weekday::Mon);
        assert_eq!(next, Some(at("2026-10-29T05:30:00Z")));
        let utc = Zone::UTC;
        let next = Digest::Hourly.next_at(at("2026-10-29T04:00:00Z"), &utc, nine(), Weekday::Mon);
        assert_eq!(next, Some(at("2026-10-29T05:00:00Z")));
    }

    #[test]
    fn weekly_digests_go_out_on_the_next_chosen_day() {
        let utc = Zone::UTC;
        // Thursday 29 October 2026: Monday 2 November 09:00.
        let next = Digest::Weekly.next_at(at("2026-10-29T12:00:00Z"), &utc, nine(), Weekday::Mon);
        assert_eq!(next, Some(at("2026-11-02T09:00:00Z")));
        // Monday 08:00: the same day.
        let next = Digest::Weekly.next_at(at("2026-11-02T08:00:00Z"), &utc, nine(), Weekday::Mon);
        assert_eq!(next, Some(at("2026-11-02T09:00:00Z")));
        // Monday 10:00: the next Monday.
        let next = Digest::Weekly.next_at(at("2026-11-02T10:00:00Z"), &utc, nine(), Weekday::Mon);
        assert_eq!(next, Some(at("2026-11-09T09:00:00Z")));
    }

    #[test]
    fn quiet_hours_span_midnight_on_the_recipients_clock() {
        let quiet = QuietHours::new("22:00", "07:00").unwrap();
        let tokyo = zone("Asia/Tokyo");
        // 23:00 in Tokyo: quiet until 07:00 there (22:00 UTC).
        assert_eq!(
            quiet.ends_after(at("2026-10-29T14:00:00Z"), &tokyo),
            at("2026-10-29T22:00:00Z")
        );
        // 03:00 in Tokyo: quiet until 07:00 the same morning.
        assert_eq!(
            quiet.ends_after(at("2026-10-29T18:00:00Z"), &tokyo),
            at("2026-10-29T22:00:00Z")
        );
        // 12:00 in Tokyo: not quiet.
        let noon = at("2026-10-29T03:00:00Z");
        assert!(!quiet.contains(noon, &tokyo));
        assert_eq!(quiet.ends_after(noon, &tokyo), noon);
        // The end itself is not quiet; the start is.
        assert!(!quiet.contains(at("2026-10-29T22:00:00Z"), &tokyo));
        assert!(quiet.contains(at("2026-10-29T13:00:00Z"), &tokyo));
    }

    #[test]
    fn quiet_hours_within_a_day_and_empty_ones() {
        let lunch = QuietHours::new("12:00", "13:30").unwrap();
        let utc = Zone::UTC;
        assert_eq!(
            lunch.ends_after(at("2026-10-29T12:45:00Z"), &utc),
            at("2026-10-29T13:30:00Z")
        );
        assert!(!lunch.contains(at("2026-10-29T11:59:00Z"), &utc));
        let none = QuietHours::new("08:00", "8:00").unwrap();
        assert!(none.is_empty());
        assert!(!none.contains(at("2026-10-29T08:00:00Z"), &utc));
    }

    #[test]
    fn quiet_hours_parse_strict_times_and_serialize_as_hh_mm() {
        let quiet: QuietHours =
            serde_json::from_value(serde_json::json!({"start": "22:00", "end": "7:00"})).unwrap();
        assert_eq!(
            serde_json::to_value(quiet).unwrap(),
            serde_json::json!({"start": "22:00", "end": "07:00"})
        );
        for bad in [
            serde_json::json!({"start": "24:00", "end": "07:00"}),
            serde_json::json!({"start": "22:00"}),
            serde_json::json!({"start": "22:00", "end": "7"}),
            serde_json::json!({"start": "22:00", "end": "07:00", "days": []}),
        ] {
            assert!(
                serde_json::from_value::<QuietHours>(bad.clone()).is_err(),
                "{bad}"
            );
        }
        assert_eq!(parse_weekday("monday"), Ok(Weekday::Mon));
        assert_eq!(parse_weekday("Fri"), Ok(Weekday::Fri));
        assert!(parse_weekday("someday").is_err());
    }

    #[test]
    fn windows_and_quiet_hours_combine() {
        let utc = Zone::UTC;
        let window = SendWindow::weekdays().between("09:00", "17:00");
        let quiet = QuietHours::new("08:00", "10:00").unwrap();
        // Thursday 06:00: the window opens at 09:00, quiet until 10:00.
        let open = next_open(
            at("2026-10-29T06:00:00Z"),
            &utc,
            Some(&window),
            Some(&quiet),
        );
        assert_eq!(open, at("2026-10-29T10:00:00Z"));
        // Friday 18:00: Monday 09:00, then quiet until 10:00.
        let open = next_open(
            at("2026-10-30T18:00:00Z"),
            &utc,
            Some(&window),
            Some(&quiet),
        );
        assert_eq!(open, at("2026-11-02T10:00:00Z"));
        // Open now.
        let now = at("2026-10-29T11:00:00Z");
        assert_eq!(next_open(now, &utc, Some(&window), Some(&quiet)), now);
        assert_eq!(next_open(now, &utc, None, None), now);
        // Quiet all window long: the window alone decides.
        let always_quiet = QuietHours::new("08:00", "18:00").unwrap();
        let open = next_open(now, &utc, Some(&window), Some(&always_quiet));
        assert_eq!(open, now);
    }
}
