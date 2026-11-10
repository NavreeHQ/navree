//! [`SendWindow`]: the local hours and days in which people-facing messages
//! may go out, and [`weekend_days`].

use std::fmt;
use std::time::Duration;

use chrono::{DateTime, NaiveTime, Utc, Weekday};

use crate::tables::WEEKENDS;
use crate::{Holidays, LocalDay, Zone};

/// How far ahead [`SendWindow::next_slot`] looks for an open day.
const HORIZON_DAYS: i64 = 3 * 366;

/// When people-facing work (a digest, a reminder, a campaign mail) may
/// reach someone: on some days of the week, between two local times,
/// outside holidays, on the recipient's clock.
///
/// ```
/// use navree_time::{SendWindow, Zone};
///
/// let window = SendWindow::weekdays().between("09:00", "17:00");
/// let kolkata = Zone::parse("Asia/Kolkata").unwrap();
/// // Friday 30 October 2026, 17:30 in Kolkata: the next slot is Monday 09:00.
/// let now = "2026-10-30T12:00:00Z".parse().unwrap();
/// let slot = window.next_slot(now, &kolkata);
/// assert_eq!(slot.to_rfc3339(), "2026-11-02T03:30:00+00:00");
/// ```
///
/// The window is half-open: it opens at its start and closes at its end
/// (`"24:00"` closes at midnight). Each day's opening and closing are
/// computed on that day's clock, so DST changes move them with the zone;
/// an opening time skipped by a DST change opens as far after the gap as
/// it was into it.
#[derive(Clone)]
pub struct SendWindow {
    /// Open days, indexed from Monday.
    days: [bool; 7],
    start: NaiveTime,
    /// `None`: midnight at the end of the day.
    end: Option<NaiveTime>,
    holidays: Option<Holidays>,
}

/// A send window that cannot be built.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct WindowError(String);

impl SendWindow {
    /// Every day, all day.
    pub fn always() -> SendWindow {
        SendWindow::on(&ALL_DAYS)
    }

    /// Every day, all day (narrow it with [`between`](SendWindow::between)).
    pub fn every_day() -> SendWindow {
        SendWindow::always()
    }

    /// Monday to Friday, all day.
    pub fn weekdays() -> SendWindow {
        SendWindow::on(&[
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
        ])
    }

    /// The days of the week that are not the weekend in `country`
    /// ([`weekend_days`]): Sunday to Thursday in Saudi Arabia, Monday to
    /// Saturday in India, Monday to Friday where the weekend is Saturday
    /// and Sunday.
    pub fn working_days_in(country: &str) -> SendWindow {
        let weekend = weekend_days(country);
        let days: Vec<Weekday> = ALL_DAYS
            .iter()
            .copied()
            .filter(|day| !weekend.contains(day))
            .collect();
        SendWindow::on(&days)
    }

    /// The given days of the week, all day.
    ///
    /// # Panics
    ///
    /// When `days` is empty: a window must open on some day.
    pub fn on(days: &[Weekday]) -> SendWindow {
        assert!(
            !days.is_empty(),
            "a send window needs at least one day of the week"
        );
        let mut open = [false; 7];
        for day in days {
            open[day.num_days_from_monday() as usize] = true;
        }
        SendWindow {
            days: open,
            start: NaiveTime::MIN,
            end: None,
            holidays: None,
        }
    }

    /// Opens at `start` and closes at `end`, local 24-hour `HH:MM` times
    /// (`end` may be `"24:00"`).
    ///
    /// # Panics
    ///
    /// When a time is malformed or `end` is not after `start`; use
    /// [`try_between`](SendWindow::try_between) for times that come from
    /// input.
    pub fn between(self, start: &str, end: &str) -> SendWindow {
        self.try_between(start, end)
            .unwrap_or_else(|error| panic!("invalid send window: {error}"))
    }

    /// Like [`between`](SendWindow::between), returning an error instead of
    /// panicking.
    pub fn try_between(self, start: &str, end: &str) -> Result<SendWindow, WindowError> {
        let start_time = parse_time(start, false)?
            .ok_or_else(|| WindowError(format!("`{start}` is not a 24-hour HH:MM time")))?;
        let end_time = parse_time(end, true)?;
        self.between_times(start_time, end_time).map_err(|_| {
            WindowError(format!(
                "`{start}`-`{end}`: a send window must end after it starts"
            ))
        })
    }

    /// Opens at `start` and closes at `end` (`None`: midnight).
    pub fn between_times(
        mut self,
        start: NaiveTime,
        end: Option<NaiveTime>,
    ) -> Result<SendWindow, WindowError> {
        if end.is_some_and(|end| end <= start) {
            return Err(WindowError(format!(
                "`{start}`-`{}`: a send window must end after it starts",
                end.map(|e| e.to_string()).unwrap_or_default()
            )));
        }
        self.start = start;
        self.end = end;
        Ok(self)
    }

    /// Closed on the days of `holidays`.
    pub fn except(mut self, holidays: Holidays) -> SendWindow {
        self.holidays = Some(holidays);
        self
    }

    /// The open days of the week, Monday first.
    pub fn days(&self) -> Vec<Weekday> {
        ALL_DAYS
            .iter()
            .copied()
            .filter(|day| self.days[day.num_days_from_monday() as usize])
            .collect()
    }

    /// The opening time.
    pub fn start(&self) -> NaiveTime {
        self.start
    }

    /// The closing time; `None` for midnight.
    pub fn end(&self) -> Option<NaiveTime> {
        self.end
    }

    /// Whether `at` is inside the window in `zone`.
    pub fn contains(&self, at: DateTime<Utc>, zone: &Zone) -> bool {
        let day = LocalDay::of(at, zone);
        self.span(day, zone)
            .is_some_and(|(open, close)| open <= at && at < close)
    }

    /// The first instant at or after `now` inside the window in `zone`:
    /// `now` itself when the window is open.
    ///
    /// When no day in the next three years is open (every one of them a
    /// holiday), it logs a warning and returns `now`.
    pub fn next_slot(&self, now: DateTime<Utc>, zone: &Zone) -> DateTime<Utc> {
        let today = LocalDay::of(now, zone);
        for offset in 0..HORIZON_DAYS {
            let day = today.add_days(offset);
            if let Some((open, close)) = self.span(day, zone)
                && now < close
            {
                return open.max(now);
            }
        }
        tracing::warn!(
            target: "navree::time",
            zone = %zone,
            "the send window has no open day in the next three years; sending now"
        );
        now
    }

    /// How long to wait from `now` until [`next_slot`](SendWindow::next_slot)
    /// (zero inside the window): the delay for `dispatch_later`.
    pub fn delay(&self, now: DateTime<Utc>, zone: &Zone) -> Duration {
        (self.next_slot(now, zone) - now)
            .to_std()
            .unwrap_or_default()
    }

    /// The opening and closing instants of `day` in `zone`, if the window
    /// opens that day.
    fn span(&self, day: LocalDay, zone: &Zone) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        if !self.days[day.weekday().num_days_from_monday() as usize] {
            return None;
        }
        if self
            .holidays
            .as_ref()
            .is_some_and(|holidays| holidays.is_holiday(day))
        {
            return None;
        }
        let open = day.at(self.start, zone);
        let close = match self.end {
            Some(end) => day.at(end, zone),
            None => day.end_in(zone),
        };
        (open < close).then_some((open, close))
    }
}

impl fmt::Debug for SendWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SendWindow")
            .field("days", &self.days())
            .field("start", &self.start)
            .field("end", &self.end)
            .field(
                "holidays",
                &self.holidays.as_ref().map(|h| h.country().unwrap_or("-")),
            )
            .finish()
    }
}

const ALL_DAYS: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];

/// Parses a 24-hour `HH:MM` (`24:00` when `allow_midnight`, as `None`).
fn parse_time(text: &str, allow_midnight: bool) -> Result<Option<NaiveTime>, WindowError> {
    let invalid = || WindowError(format!("`{text}` is not a 24-hour HH:MM time"));
    let trimmed = text.trim();
    if allow_midnight && trimmed == "24:00" {
        return Ok(None);
    }
    let (hour, minute) = trimmed.split_once(':').ok_or_else(invalid)?;
    let digits = |s: &str, max_len: usize| {
        (!s.is_empty() && s.len() <= max_len && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u32>().ok())
            .flatten()
    };
    let hour = digits(hour, 2).ok_or_else(invalid)?;
    let minute = digits(minute, 2)
        .filter(|_| minute.len() == 2)
        .ok_or_else(invalid)?;
    NaiveTime::from_hms_opt(hour, minute, 0)
        .map(Some)
        .ok_or_else(invalid)
}

/// The weekend of `country` (an ISO 3166-1 alpha-2 code, any case), from
/// the Unicode CLDR week data: Saturday and Sunday in most countries,
/// Friday and Saturday in much of the Middle East, Sunday alone in India.
/// Unknown codes get Saturday and Sunday.
pub fn weekend_days(country: &str) -> Vec<Weekday> {
    let upper = country.trim().to_ascii_uppercase();
    let numbers: &[u8] = WEEKENDS
        .binary_search_by(|(code, _)| (*code).cmp(upper.as_str()))
        .map(|index| WEEKENDS[index].1)
        .unwrap_or(&[6, 7]);
    numbers
        .iter()
        .filter_map(|n| ALL_DAYS.get(usize::from(*n).wrapping_sub(1)).copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_strict_hh_mm() {
        assert_eq!(
            parse_time("09:00", false).unwrap(),
            NaiveTime::from_hms_opt(9, 0, 0)
        );
        assert_eq!(
            parse_time("7:05", false).unwrap(),
            NaiveTime::from_hms_opt(7, 5, 0)
        );
        assert_eq!(parse_time("24:00", true).unwrap(), None);
        for bad in ["24:00", "12:5", "12", "noon", "", "12:60", "-1:00"] {
            assert!(parse_time(bad, false).is_err(), "{bad}");
        }
    }

    #[test]
    fn weekends_are_sorted_tables() {
        assert!(WEEKENDS.windows(2).all(|w| w[0].0 < w[1].0));
    }
}
