//! [`Date`]: a day of the calendar.

use crate::{DateTime, Error, Month, Result, Time, Weekday, civil};

/// The first year a [`Date`] holds.
pub(crate) const MIN_YEAR: i32 = -262_143;
/// The last year a [`Date`] holds.
pub(crate) const MAX_YEAR: i32 = 262_142;
/// The day count of [`Date::MIN`].
const MIN_DAYS: i64 = civil::days_from_civil(MIN_YEAR, 1, 1);
/// The day count of [`Date::MAX`].
const MAX_DAYS: i64 = civil::days_from_civil(MAX_YEAR, 12, 31);

/// A day of the calendar: a year, a month and a day, with no time of day
/// and no zone.
///
/// The calendar is the proleptic Gregorian one: today's calendar carried
/// back before 1582 by its own rules, with a year 0 before year 1. Dates run
/// from [`Date::MIN`], 1 January of the year -262,143, to [`Date::MAX`],
/// 31 December of the year 262,142.
///
/// Days compare in the order of the calendar. A step along it is a method
/// with a name: [`add_days`](Date::add_days) counts days,
/// [`add_months`](Date::add_months) and [`add_years`](Date::add_years) keep
/// the day of the month where the month has it.
///
/// ```
/// use navree_datetime::{Date, Month, Weekday};
///
/// # fn main() -> navree_datetime::Result<()> {
/// let day = Date::new(2026, 11, 25)?;
/// assert_eq!((day.year(), day.month(), day.day()), (2026, Month::November, 25));
/// assert_eq!(day.weekday(), Weekday::Wednesday);
/// assert_eq!(day.tomorrow(), Some(Date::new(2026, 11, 26)?));
/// assert_eq!(day.last_of_month(), Date::new(2026, 11, 30)?);
/// assert_eq!(day.days_until(Date::new(2027, 1, 1)?), 37);
/// assert!(Date::new(2026, 2, 30).is_err());
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: i32,
    /// 1 to 12.
    month: u8,
    /// 1 to the month's last day.
    day: u8,
}

impl Date {
    /// The first day: 1 January of the year -262,143.
    pub const MIN: Date = Date {
        year: MIN_YEAR,
        month: 1,
        day: 1,
    };

    /// The last day: 31 December of the year 262,142.
    pub const MAX: Date = Date {
        year: MAX_YEAR,
        month: 12,
        day: 31,
    };

    /// The day `day` of month `month` (1 to 12) of `year`.
    ///
    /// # Errors
    ///
    /// A year outside -262,143 to 262,142 is out of range. A month that is
    /// not 1 to 12, or a day the month does not have (30 February, 31
    /// April, 29 February of a year that is not a leap year), is invalid.
    pub fn new(year: i32, month: u8, day: u8) -> Result<Date> {
        if !(MIN_YEAR..=MAX_YEAR).contains(&year) {
            return Err(year_out_of_range(year));
        }
        if !(1..=12).contains(&month) {
            return Err(Error::invalid(format!("there is no month {month}")));
        }
        if day == 0 || day > civil::days_in_month(year, month) {
            return Err(Error::invalid(format!(
                "{year}-{month:02} has no day {day}"
            )));
        }
        Ok(Date { year, month, day })
    }

    /// The `ordinal`th day of `year`: 1 is 1 January, and 365 or 366 is
    /// 31 December.
    ///
    /// # Errors
    ///
    /// A year outside -262,143 to 262,142 is out of range. A day the year
    /// does not have (0, or 366 in a year that is not a leap year) is
    /// invalid.
    pub fn from_ordinal(year: i32, ordinal: u16) -> Result<Date> {
        if !(MIN_YEAR..=MAX_YEAR).contains(&year) {
            return Err(year_out_of_range(year));
        }
        match civil::month_and_day(year, ordinal) {
            Some((month, day)) => Ok(Date { year, month, day }),
            None => Err(Error::invalid(format!(
                "the year {year} has no day {ordinal}"
            ))),
        }
    }

    /// The day `days` days from 1 January 1970, which is day 0; a day
    /// before it has a count below zero.
    ///
    /// # Errors
    ///
    /// A day before [`Date::MIN`] or after [`Date::MAX`] is out of range.
    pub fn from_epoch_days(days: i64) -> Result<Date> {
        match Date::from_epoch_days_checked(days) {
            Some(date) => Ok(date),
            None => Err(Error::out_of_range(format!(
                "day {days} from 1970-01-01 is outside the years {MIN_YEAR} to {MAX_YEAR}"
            ))),
        }
    }

    /// Days from 1 January 1970 to this day: 0 for that day, below zero
    /// before it.
    pub const fn epoch_days(self) -> i64 {
        civil::days_from_civil(self.year, self.month, self.day)
    }

    /// The year: -262,143 to 262,142. The year before year 1 is year 0.
    pub const fn year(self) -> i32 {
        self.year
    }

    /// The month.
    pub const fn month(self) -> Month {
        match Month::from_number(self.month) {
            Some(month) => month,
            // A date holds a month of 1 to 12.
            None => Month::January,
        }
    }

    /// The month's number: 1 for January to 12 for December.
    pub const fn month_number(self) -> u8 {
        self.month
    }

    /// The day of the month: 1 to 31.
    pub const fn day(self) -> u8 {
        self.day
    }

    /// The day of the year: 1 for 1 January, 365 or 366 for 31 December.
    pub const fn ordinal(self) -> u16 {
        civil::day_of_year(self.year, self.month, self.day)
    }

    /// The day of the week.
    pub const fn weekday(self) -> Weekday {
        civil::weekday_from_days(self.epoch_days())
    }

    /// The ISO 8601 week the day is in: its year and its number, 1 to 52 or
    /// 53.
    ///
    /// A week runs from Monday to Sunday and belongs to the year that holds
    /// its Thursday, so the first days of January can be in the last week of
    /// the year before and the last days of December in week 1 of the year
    /// after.
    pub const fn iso_week(self) -> (i32, u8) {
        // The week's Thursday is in the year; its place in the year, in
        // whole weeks, is the week's number.
        let week = (self.ordinal() as i32 - self.weekday().number_from_monday() as i32 + 10) / 7;
        if week < 1 {
            (self.year - 1, iso_weeks_in(self.year - 1))
        } else if week > iso_weeks_in(self.year) as i32 {
            (self.year + 1, 1)
        } else {
            (self.year, week as u8)
        }
    }

    /// The first day of the day's month.
    #[must_use]
    pub const fn first_of_month(self) -> Date {
        Date {
            year: self.year,
            month: self.month,
            day: 1,
        }
    }

    /// The last day of the day's month: the 28th to the 31st.
    #[must_use]
    pub const fn last_of_month(self) -> Date {
        Date {
            year: self.year,
            month: self.month,
            day: civil::days_in_month(self.year, self.month),
        }
    }

    /// The day `days` days later, or earlier for a count below zero. `None`
    /// past [`Date::MIN`] or [`Date::MAX`].
    #[must_use]
    pub const fn add_days(self, days: i64) -> Option<Date> {
        match self.epoch_days().checked_add(days) {
            Some(days) => Date::from_epoch_days_checked(days),
            None => None,
        }
    }

    /// The same day of the month `months` months later, or earlier for a
    /// count below zero. A day the month does not have becomes the month's
    /// last day: a month after 31 January is 28 or 29 February. `None` past
    /// [`Date::MIN`] or [`Date::MAX`].
    #[must_use]
    pub const fn add_months(self, months: i32) -> Option<Date> {
        // Months counted from January of year 0.
        let months = match (self.year * 12 + self.month as i32 - 1).checked_add(months) {
            Some(months) => months,
            None => return None,
        };
        Date::clamped(
            months.div_euclid(12),
            months.rem_euclid(12) as u8 + 1,
            self.day,
        )
    }

    /// The same day of the same month `years` years later, or earlier for a
    /// count below zero. 29 February becomes 28 February in a year that is
    /// not a leap year. `None` past [`Date::MIN`] or [`Date::MAX`].
    #[must_use]
    pub const fn add_years(self, years: i32) -> Option<Date> {
        match self.year.checked_add(years) {
            Some(year) => Date::clamped(year, self.month, self.day),
            None => None,
        }
    }

    /// The day after, or `None` after [`Date::MAX`].
    #[must_use]
    pub const fn tomorrow(self) -> Option<Date> {
        self.add_days(1)
    }

    /// The day before, or `None` before [`Date::MIN`].
    #[must_use]
    pub const fn yesterday(self) -> Option<Date> {
        self.add_days(-1)
    }

    /// The `n`th `weekday` of a month, counted from 1: the 2nd Friday of
    /// March 2017 is `Date::nth_weekday_of_month(2017, 3, Weekday::Friday, 2)`.
    ///
    /// `None` when the month has no such day (a 5th Monday in a month with
    /// four), for `n` of 0, for a month that is not 1 to 12 and for a year
    /// out of range.
    pub const fn nth_weekday_of_month(
        year: i32,
        month: u8,
        weekday: Weekday,
        n: u8,
    ) -> Option<Date> {
        if n == 0 || year < MIN_YEAR || year > MAX_YEAR || month < 1 || month > 12 {
            return None;
        }
        let first = Date {
            year,
            month,
            day: 1,
        }
        .weekday();
        // Days from the 1st to the month's first `weekday`: 0 to 6.
        let to_first = (7 + weekday.days_from_monday() - first.days_from_monday()) % 7;
        let day = (n as u16 - 1) * 7 + to_first as u16 + 1;
        if day > civil::days_in_month(year, month) as u16 {
            return None;
        }
        Some(Date {
            year,
            month,
            day: day as u8,
        })
    }

    /// Days from this day to `other`: above zero when `other` is later,
    /// below zero when it is earlier.
    pub const fn days_until(self, other: Date) -> i64 {
        other.epoch_days() - self.epoch_days()
    }

    /// The day at a time of day.
    pub const fn at(self, time: Time) -> DateTime {
        DateTime::new(self, time)
    }

    /// The day of a day count, or `None` for a count outside the range.
    const fn from_epoch_days_checked(days: i64) -> Option<Date> {
        if days < MIN_DAYS || days > MAX_DAYS {
            return None;
        }
        Some(Date::from_epoch_days_unchecked(days))
    }

    /// The day of a day count that is in the range or a day or two past
    /// either end of it.
    ///
    /// A wall clock ahead of UTC shows a day after [`Date::MAX`] in the last
    /// hours of the range, and one behind UTC a day before [`Date::MIN`] in
    /// its first hours. Such a day can be shown and read, field by field. It
    /// must not be stepped along the calendar or turned back into an
    /// instant: the answers past the range are not checked.
    pub(crate) const fn from_epoch_days_unchecked(days: i64) -> Date {
        let (year, month, day) = civil::civil_from_days_in_range(days);
        Date { year, month, day }
    }

    /// `day` of `month` (1 to 12) of `year`, or the month's last day where
    /// the month is shorter. `None` for a year out of range.
    const fn clamped(year: i32, month: u8, day: u8) -> Option<Date> {
        if year < MIN_YEAR || year > MAX_YEAR {
            return None;
        }
        let last = civil::days_in_month(year, month);
        Some(Date {
            year,
            month,
            day: if day > last { last } else { day },
        })
    }
}

/// 1 January 1970.
impl Default for Date {
    fn default() -> Date {
        Date {
            year: 1970,
            month: 1,
            day: 1,
        }
    }
}

/// How many ISO 8601 weeks `year` has: 53 when it starts on a Thursday, or
/// on a Wednesday and is a leap year, and 52 otherwise.
const fn iso_weeks_in(year: i32) -> u8 {
    match civil::weekday_from_days(civil::days_from_civil(year, 1, 1)) {
        Weekday::Thursday => 53,
        Weekday::Wednesday if civil::is_leap_year(year) => 53,
        _ => 52,
    }
}

fn year_out_of_range(year: i32) -> Error {
    Error::out_of_range(format!(
        "the year {year} is outside {MIN_YEAR} to {MAX_YEAR}"
    ))
}
