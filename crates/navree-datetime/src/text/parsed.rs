//! What a text said, part by part, and the value those parts make.
//!
//! Reading has two halves. The scanners put each part they meet into a
//! [`Parsed`]: a year, a month's name, an hour, an offset. Then the type
//! that was asked for takes what it needs: a day, a time of day, a day and a
//! time, or an instant. Parts that say the same thing twice have to agree,
//! and parts the type has no place for are left where they are.

use super::scan::{Reason, Scan};
use super::{days_since, quarter, weeks_from};
use crate::{Date, DateTime, Offset, Span, Time, Timestamp, Weekday, civil};

/// The parts of a date and a time a text can name. A part the text did not
/// name is `None`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Parsed {
    year: Option<i32>,
    year_div_100: Option<i32>,
    year_mod_100: Option<i32>,
    iso_year: Option<i32>,
    iso_year_mod_100: Option<i32>,
    quarter: Option<u32>,
    month: Option<u32>,
    week_from_sun: Option<u32>,
    week_from_mon: Option<u32>,
    iso_week: Option<u32>,
    weekday: Option<Weekday>,
    ordinal: Option<u32>,
    day: Option<u32>,
    /// 0 before noon and 1 from noon.
    hour_div_12: Option<u32>,
    /// The hour of a 12-hour clock, with 12 as 0.
    hour_mod_12: Option<u32>,
    minute: Option<u32>,
    /// 0 to 60: a text may name second 60.
    second: Option<u32>,
    nanosecond: Option<u32>,
    /// Seconds from the epoch.
    timestamp: Option<i64>,
    /// Seconds east of UTC, as large as the text said.
    offset: Option<i32>,
}

/// Keeps `new` in `old`, unless `old` already holds something else.
fn set_if_consistent<T: PartialEq>(old: &mut Option<T>, new: T) -> Scan<()> {
    match old {
        Some(old) if *old != new => Err(Reason::Impossible),
        _ => {
            *old = Some(new);
            Ok(())
        }
    }
}

impl Parsed {
    pub(crate) fn set_year(&mut self, value: i64) -> Scan<()> {
        let value = i32::try_from(value).map_err(|_| Reason::OutOfRange)?;
        set_if_consistent(&mut self.year, value)
    }

    pub(crate) fn set_year_div_100(&mut self, value: i64) -> Scan<()> {
        if !(0..=i64::from(i32::MAX)).contains(&value) {
            return Err(Reason::OutOfRange);
        }
        set_if_consistent(&mut self.year_div_100, value as i32)
    }

    pub(crate) fn set_year_mod_100(&mut self, value: i64) -> Scan<()> {
        if !(0..100).contains(&value) {
            return Err(Reason::OutOfRange);
        }
        set_if_consistent(&mut self.year_mod_100, value as i32)
    }

    pub(crate) fn set_iso_year(&mut self, value: i64) -> Scan<()> {
        let value = i32::try_from(value).map_err(|_| Reason::OutOfRange)?;
        set_if_consistent(&mut self.iso_year, value)
    }

    pub(crate) fn set_iso_year_mod_100(&mut self, value: i64) -> Scan<()> {
        if !(0..100).contains(&value) {
            return Err(Reason::OutOfRange);
        }
        set_if_consistent(&mut self.iso_year_mod_100, value as i32)
    }

    pub(crate) fn set_quarter(&mut self, value: i64) -> Scan<()> {
        if !(1..=4).contains(&value) {
            return Err(Reason::OutOfRange);
        }
        set_if_consistent(&mut self.quarter, value as u32)
    }

    pub(crate) fn set_month(&mut self, value: i64) -> Scan<()> {
        if !(1..=12).contains(&value) {
            return Err(Reason::OutOfRange);
        }
        set_if_consistent(&mut self.month, value as u32)
    }

    pub(crate) fn set_week_from_sun(&mut self, value: i64) -> Scan<()> {
        if !(0..=53).contains(&value) {
            return Err(Reason::OutOfRange);
        }
        set_if_consistent(&mut self.week_from_sun, value as u32)
    }

    pub(crate) fn set_week_from_mon(&mut self, value: i64) -> Scan<()> {
        if !(0..=53).contains(&value) {
            return Err(Reason::OutOfRange);
        }
        set_if_consistent(&mut self.week_from_mon, value as u32)
    }

    pub(crate) fn set_iso_week(&mut self, value: i64) -> Scan<()> {
        if !(1..=53).contains(&value) {
            return Err(Reason::OutOfRange);
        }
        set_if_consistent(&mut self.iso_week, value as u32)
    }

    pub(crate) fn set_weekday(&mut self, value: Weekday) -> Scan<()> {
        set_if_consistent(&mut self.weekday, value)
    }

    /// The weekday from its number counted from Sunday as 0.
    pub(crate) fn set_weekday_from_sunday(&mut self, value: i64) -> Scan<()> {
        self.set_weekday(match value {
            0 => Weekday::Sunday,
            1 => Weekday::Monday,
            2 => Weekday::Tuesday,
            3 => Weekday::Wednesday,
            4 => Weekday::Thursday,
            5 => Weekday::Friday,
            6 => Weekday::Saturday,
            _ => return Err(Reason::OutOfRange),
        })
    }

    /// The weekday from its number counted from Monday as 1.
    pub(crate) fn set_weekday_from_monday(&mut self, value: i64) -> Scan<()> {
        self.set_weekday(match value {
            1 => Weekday::Monday,
            2 => Weekday::Tuesday,
            3 => Weekday::Wednesday,
            4 => Weekday::Thursday,
            5 => Weekday::Friday,
            6 => Weekday::Saturday,
            7 => Weekday::Sunday,
            _ => return Err(Reason::OutOfRange),
        })
    }

    pub(crate) fn set_ordinal(&mut self, value: i64) -> Scan<()> {
        if !(1..=366).contains(&value) {
            return Err(Reason::OutOfRange);
        }
        set_if_consistent(&mut self.ordinal, value as u32)
    }

    pub(crate) fn set_day(&mut self, value: i64) -> Scan<()> {
        if !(1..=31).contains(&value) {
            return Err(Reason::OutOfRange);
        }
        set_if_consistent(&mut self.day, value as u32)
    }

    /// Before noon (`false`) or from noon (`true`).
    pub(crate) fn set_ampm(&mut self, pm: bool) -> Scan<()> {
        set_if_consistent(&mut self.hour_div_12, u32::from(pm))
    }

    /// The hour of a 12-hour clock: 1 to 12.
    pub(crate) fn set_hour12(&mut self, mut value: i64) -> Scan<()> {
        if !(1..=12).contains(&value) {
            return Err(Reason::OutOfRange);
        }
        if value == 12 {
            value = 0;
        }
        set_if_consistent(&mut self.hour_mod_12, value as u32)
    }

    /// The hour of a 24-hour clock, which also says which half of the day
    /// it is.
    pub(crate) fn set_hour(&mut self, value: i64) -> Scan<()> {
        let (hour_div_12, hour_mod_12) = match value {
            hour @ 0..=11 => (0, hour as u32),
            hour @ 12..=23 => (1, hour as u32 - 12),
            _ => return Err(Reason::OutOfRange),
        };
        set_if_consistent(&mut self.hour_div_12, hour_div_12)?;
        set_if_consistent(&mut self.hour_mod_12, hour_mod_12)
    }

    pub(crate) fn set_minute(&mut self, value: i64) -> Scan<()> {
        if !(0..=59).contains(&value) {
            return Err(Reason::OutOfRange);
        }
        set_if_consistent(&mut self.minute, value as u32)
    }

    /// The second: 0 to 60.
    pub(crate) fn set_second(&mut self, value: i64) -> Scan<()> {
        if !(0..=60).contains(&value) {
            return Err(Reason::OutOfRange);
        }
        set_if_consistent(&mut self.second, value as u32)
    }

    pub(crate) fn set_nanosecond(&mut self, value: i64) -> Scan<()> {
        if !(0..=999_999_999).contains(&value) {
            return Err(Reason::OutOfRange);
        }
        set_if_consistent(&mut self.nanosecond, value as u32)
    }

    pub(crate) fn set_timestamp(&mut self, value: i64) -> Scan<()> {
        set_if_consistent(&mut self.timestamp, value)
    }

    pub(crate) fn set_offset(&mut self, value: i64) -> Scan<()> {
        let value = i32::try_from(value).map_err(|_| Reason::OutOfRange)?;
        set_if_consistent(&mut self.offset, value)
    }

    /// The day the parts name.
    ///
    /// It is found from the first of these the text gave, with a year: a
    /// month and a day; a day of the year; a week counted from Sunday or
    /// from Monday and a weekday; or the ISO 8601 year, week and weekday.
    /// Every other part of a date the text gave is then held against that
    /// day. A year can be whole, a century and two digits, or two digits
    /// alone: 70 to 99 are then 1970 to 1999 and 0 to 69 are 2000 to 2069.
    pub(crate) fn to_date(&self) -> Scan<Date> {
        /// The year of a whole year `y`, a century `q` and two digits `r`,
        /// as far as they were given and agree.
        fn resolve_year(y: Option<i32>, q: Option<i32>, r: Option<i32>) -> Scan<Option<i32>> {
            match (y, q, r) {
                (y, None, None) => Ok(y),

                // A whole year beside a century or two digits: those are
                // only for years from 0, and have to be this year's.
                (Some(y), q, r @ Some(0..=99)) | (Some(y), q, r @ None) => {
                    if y < 0 {
                        return Err(Reason::Impossible);
                    }
                    let q_ = y / 100;
                    let r_ = y % 100;
                    if q.unwrap_or(q_) == q_ && r.unwrap_or(r_) == r_ {
                        Ok(Some(y))
                    } else {
                        Err(Reason::Impossible)
                    }
                }

                (None, Some(q), Some(r @ 0..=99)) => {
                    if q < 0 {
                        return Err(Reason::Impossible);
                    }
                    let y = q.checked_mul(100).and_then(|v| v.checked_add(r));
                    Ok(Some(y.ok_or(Reason::OutOfRange)?))
                }

                // Two digits alone.
                (None, None, Some(r @ 0..=99)) => Ok(Some(r + if r < 70 { 2000 } else { 1900 })),

                (None, Some(_), None) => Err(Reason::NotEnough),
                (_, _, Some(_)) => Err(Reason::OutOfRange),
            }
        }

        let given_year = resolve_year(self.year, self.year_div_100, self.year_mod_100)?;
        let given_iso_year = resolve_year(self.iso_year, None, self.iso_year_mod_100)?;

        // Whether the year, the month and the day that were given are
        // `date`'s.
        let verify_ymd = |date: Date| {
            let year = date.year();
            let (year_div_100, year_mod_100) = if year >= 0 {
                (Some(year / 100), Some(year % 100))
            } else {
                // A year below 0 has neither.
                (None, None)
            };
            let month = u32::from(date.month_number());
            let day = u32::from(date.day());
            self.year.unwrap_or(year) == year
                && self.year_div_100.or(year_div_100) == year_div_100
                && self.year_mod_100.or(year_mod_100) == year_mod_100
                && self.month.unwrap_or(month) == month
                && self.day.unwrap_or(day) == day
        };

        // Whether the ISO 8601 year, week and weekday that were given are
        // `date`'s.
        let verify_iso_week_date = |date: Date| {
            let (iso_year, iso_week) = date.iso_week();
            let iso_week = u32::from(iso_week);
            let weekday = date.weekday();
            let iso_year_mod_100 = if iso_year >= 0 {
                Some(iso_year % 100)
            } else {
                None
            };
            self.iso_year.unwrap_or(iso_year) == iso_year
                && self.iso_year_mod_100.or(iso_year_mod_100) == iso_year_mod_100
                && self.iso_week.unwrap_or(iso_week) == iso_week
                && self.weekday.unwrap_or(weekday) == weekday
        };

        // Whether the day of the year and the weeks that were given are
        // `date`'s.
        let verify_ordinal = |date: Date| {
            let ordinal = u32::from(date.ordinal());
            let week_from_sun = weeks_from(date, Weekday::Sunday);
            let week_from_mon = weeks_from(date, Weekday::Monday);
            self.ordinal.unwrap_or(ordinal) == ordinal
                && self.week_from_sun.map_or(week_from_sun, |v| v as i32) == week_from_sun
                && self.week_from_mon.map_or(week_from_mon, |v| v as i32) == week_from_mon
        };

        let (verified, date) = match (given_year, given_iso_year, self) {
            (
                Some(year),
                _,
                &Parsed {
                    month: Some(month),
                    day: Some(day),
                    ..
                },
            ) => {
                // The month is 1 to 12 and the day 1 to 31.
                let date =
                    Date::new(year, month as u8, day as u8).map_err(|_| Reason::OutOfRange)?;
                (verify_iso_week_date(date) && verify_ordinal(date), date)
            }

            (
                Some(year),
                _,
                &Parsed {
                    ordinal: Some(ordinal),
                    ..
                },
            ) => {
                // The day of the year is 1 to 366.
                let date =
                    Date::from_ordinal(year, ordinal as u16).map_err(|_| Reason::OutOfRange)?;
                (
                    verify_ymd(date) && verify_iso_week_date(date) && verify_ordinal(date),
                    date,
                )
            }

            (
                Some(year),
                _,
                &Parsed {
                    week_from_sun: Some(week),
                    weekday: Some(weekday),
                    ..
                },
            ) => {
                let date = resolve_week_date(year, week, weekday, Weekday::Sunday)?;
                (
                    verify_ymd(date) && verify_iso_week_date(date) && verify_ordinal(date),
                    date,
                )
            }

            (
                Some(year),
                _,
                &Parsed {
                    week_from_mon: Some(week),
                    weekday: Some(weekday),
                    ..
                },
            ) => {
                let date = resolve_week_date(year, week, weekday, Weekday::Monday)?;
                (
                    verify_ymd(date) && verify_iso_week_date(date) && verify_ordinal(date),
                    date,
                )
            }

            (
                _,
                Some(iso_year),
                &Parsed {
                    iso_week: Some(iso_week),
                    weekday: Some(weekday),
                    ..
                },
            ) => {
                let date =
                    from_iso_week_date(iso_year, iso_week, weekday).ok_or(Reason::OutOfRange)?;
                (verify_ymd(date) && verify_ordinal(date), date)
            }

            (_, _, _) => return Err(Reason::NotEnough),
        };

        if !verified {
            return Err(Reason::Impossible);
        } else if let Some(given) = self.quarter
            && given != quarter(date)
        {
            return Err(Reason::Impossible);
        }

        Ok(date)
    }

    /// The time of day the parts name, and whether the text named second
    /// 60.
    ///
    /// It needs an hour of 24, or an hour of 12 and its half of the day,
    /// and a minute. A missing second is 0, and a fraction needs a second.
    ///
    /// A minute has 60 seconds here, so second 60 is no time of day. A text
    /// that names it is read as the last nanosecond of its minute, whatever
    /// fraction follows.
    pub(crate) fn to_time(&self) -> Scan<(Time, bool)> {
        let hour_div_12 = match self.hour_div_12 {
            Some(v @ 0..=1) => v,
            Some(_) => return Err(Reason::OutOfRange),
            None => return Err(Reason::NotEnough),
        };
        let hour_mod_12 = match self.hour_mod_12 {
            Some(v @ 0..=11) => v,
            Some(_) => return Err(Reason::OutOfRange),
            None => return Err(Reason::NotEnough),
        };
        let hour = hour_div_12 * 12 + hour_mod_12;

        let minute = match self.minute {
            Some(v @ 0..=59) => v,
            Some(_) => return Err(Reason::OutOfRange),
            None => return Err(Reason::NotEnough),
        };

        let (second, sixty) = match self.second.unwrap_or(0) {
            v @ 0..=59 => (v, false),
            60 => (59, true),
            _ => return Err(Reason::OutOfRange),
        };
        let nanosecond = match self.nanosecond {
            Some(v @ 0..=999_999_999) if self.second.is_some() => v,
            // A fraction of a second that was not given.
            Some(0..=999_999_999) => return Err(Reason::NotEnough),
            Some(_) => return Err(Reason::OutOfRange),
            None => 0,
        };
        let nanosecond = if sixty { 999_999_999 } else { nanosecond };

        // The hour is 0 to 23, the minute and the second 0 to 59.
        Time::new(hour as u8, minute as u8, second as u8)
            .and_then(|time| time.with_nanos(nanosecond))
            .map(|time| (time, sixty))
            .map_err(|_| Reason::OutOfRange)
    }

    /// The day and the time of day the parts name, on a clock `offset`
    /// seconds east of UTC, and whether the text named second 60.
    ///
    /// They come from the parts of a date and of a time, or, where those
    /// are not enough, from seconds since the epoch (`%s`). Where the text
    /// gave both, they have to be the same moment.
    pub(crate) fn to_date_time(&self, offset: i32) -> Scan<(DateTime, bool)> {
        let date = self.to_date();
        let time = self.to_time();
        if let (Ok(date), Ok((time, sixty))) = (date, time) {
            let date_time = date.at(time);

            // The second the clock shows: for second 60, the one before.
            let timestamp = date_time.in_utc().as_secs() - i64::from(offset);
            if let Some(given) = self.timestamp
                && given != timestamp
                && !(sixty && given == timestamp + 1)
            {
                return Err(Reason::Impossible);
            }

            Ok((date_time, sixty))
        } else if let Some(timestamp) = self.timestamp {
            // What was given of the date or the time is wrong in itself.
            match (date, time) {
                (Err(Reason::OutOfRange), _) | (_, Err(Reason::OutOfRange)) => {
                    return Err(Reason::OutOfRange);
                }
                (Err(Reason::Impossible), _) | (_, Err(Reason::Impossible)) => {
                    return Err(Reason::Impossible);
                }
                (_, _) => {}
            }

            // The day and the time of the seconds since the epoch.
            let seconds = timestamp
                .checked_add(i64::from(offset))
                .ok_or(Reason::OutOfRange)?;
            let mut date_time = Timestamp::from_secs(seconds)
                .map_err(|_| Reason::OutOfRange)?
                .date_time_utc();

            // Fill in the year, the day of the year, the hour, the minute
            // and the second from them. What the text gave besides has to
            // agree, and with it the day and the time are whole.
            let mut parsed = self.clone();
            if parsed.second == Some(60) {
                match date_time.time().second() {
                    // The seconds since the epoch count second 60 as the
                    // second before it, or as the one after.
                    59 => {}
                    0 => {
                        date_time = date_time
                            .add(Span::seconds(-1))
                            .ok_or(Reason::OutOfRange)?;
                    }
                    _ => return Err(Reason::Impossible),
                }
            } else {
                parsed.set_second(i64::from(date_time.time().second()))?;
            }
            parsed.set_year(i64::from(date_time.date().year()))?;
            parsed.set_ordinal(i64::from(date_time.date().ordinal()))?;
            parsed.set_hour(i64::from(date_time.time().hour()))?;
            parsed.set_minute(i64::from(date_time.time().minute()))?;

            let date = parsed.to_date()?;
            let (time, sixty) = parsed.to_time()?;
            Ok((date.at(time), sixty))
        } else {
            // The day's own refusal, or else the time's.
            let date = date?;
            let (time, sixty) = time?;
            Ok((date.at(time), sixty))
        }
    }

    /// The instant the parts name, and the offset from UTC the text was
    /// written with.
    ///
    /// It needs an offset. Seconds since the epoch (`%s`) are counted in
    /// UTC, so with them and no offset the offset is zero.
    ///
    /// An offset of 24 hours or more is refused: no clock is that far from
    /// UTC, and no text Navree has written holds one.
    pub(crate) fn to_instant(&self) -> Scan<(Timestamp, Offset)> {
        let seconds_east = match (self.offset, self.timestamp) {
            (Some(offset), _) => offset,
            (None, Some(_)) => 0,
            (None, None) => return Err(Reason::NotEnough),
        };
        let (local, _) = self.to_date_time(seconds_east)?;
        let offset = offset_within_a_day(seconds_east)?;

        // The local time is in the range; the instant it is can be just
        // past either end.
        let seconds = local.in_utc().as_secs() - i64::from(seconds_east);
        let instant = Timestamp::from_secs_nanos(seconds, local.time().nanosecond())
            .map_err(|_| Reason::Impossible)?;
        Ok((instant, offset))
    }
}

/// The offset `seconds_east` of UTC, where it is less than 24 hours either
/// way.
pub(crate) fn offset_within_a_day(seconds_east: i32) -> Scan<Offset> {
    if -86_400 < seconds_east && seconds_east < 86_400 {
        Offset::east(seconds_east).map_err(|_| Reason::OutOfRange)
    } else {
        Err(Reason::OutOfRange)
    }
}

/// The day of `year` that is `weekday` of week `week`, where week 1 starts
/// on the year's first `week_starts_on` and the days before it are week 0.
/// A week 0 or 53 whose weekday is in another year is refused.
fn resolve_week_date(
    year: i32,
    week: u32,
    weekday: Weekday,
    week_starts_on: Weekday,
) -> Scan<Date> {
    if week > 53 {
        return Err(Reason::OutOfRange);
    }

    let first_day_of_year = Date::from_ordinal(year, 1).map_err(|_| Reason::OutOfRange)?;
    // The day of the year week 1 starts on.
    let first_week_start = 1 + days_since(week_starts_on, first_day_of_year.weekday()) as i32;
    // The weekday's place in its week, from 0.
    let weekday = days_since(weekday, week_starts_on) as i32;
    let ordinal = first_week_start + (week as i32 - 1) * 7 + weekday;
    if ordinal <= 0 {
        return Err(Reason::Impossible);
    }
    Date::from_ordinal(year, ordinal as u16).map_err(|_| Reason::Impossible)
}

/// The day that is `weekday` of week `week` of the ISO 8601 year
/// `iso_year`, where that year has such a week and the day is one a
/// [`Date`] holds.
fn from_iso_week_date(iso_year: i32, week: u32, weekday: Weekday) -> Option<Date> {
    if !(1..=53).contains(&week) {
        return None;
    }
    // 4 January is always in week 1, and a week starts on Monday.
    let fourth = civil::days_from_civil(iso_year, 1, 4);
    let week_1 = fourth - i64::from(civil::weekday_from_days(fourth).days_from_monday());
    let days = week_1 + (i64::from(week) - 1) * 7 + i64::from(weekday.days_from_monday());
    let date = Date::from_epoch_days(days).ok()?;
    // A week 53 of a year with 52 is week 1 of the year after.
    (date.iso_week() == (iso_year, week as u8)).then_some(date)
}
