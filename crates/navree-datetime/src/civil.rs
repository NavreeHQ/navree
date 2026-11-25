//! The calendar as arithmetic on day counts.
//!
//! A day count is the number of days from 1 January 1970, which is day 0,
//! in the proleptic Gregorian calendar: today's calendar carried back before
//! 1582 by its own rules, with a year 0 before year 1. [`days_from_civil`]
//! and [`civil_from_days`] are the one place where a year, a month and a day
//! become a day count and a day count becomes them again; every date and
//! every instant of this crate goes through them.
//!
//! The method is Howard Hinnant's. It counts a year from 1 March, so that a
//! leap day is the last day of its year and never moves the months before
//! it, and it works in whole cycles of 400 years: 146,097 days, after which
//! the calendar repeats exactly.
//!
//! ```
//! use navree_datetime::civil;
//!
//! assert_eq!(civil::days_from_civil(1970, 1, 1), 0);
//! assert_eq!(civil::days_from_civil(2026, 11, 25), 20_782);
//! assert_eq!(civil::civil_from_days(20_782), Some((2026, 11, 25)));
//! assert_eq!(civil::days_in_month(2024, 2), 29);
//! ```

use crate::Weekday;

/// Days in 400 years.
const DAYS_PER_CYCLE: i64 = 146_097;
/// Days from 1 March of year 0 to 1 January 1970.
const DAYS_TO_EPOCH: i64 = 719_468;
/// The first day count whose year fits in `i32`.
const FIRST_DAY: i64 = days_from_civil(i32::MIN, 1, 1);
/// The last day count whose year fits in `i32`.
const LAST_DAY: i64 = days_from_civil(i32::MAX, 12, 31);
/// Days of a year that is not a leap year before each month's first day.
const DAYS_BEFORE_MONTH: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];

/// Whether `year` has a 29 February: every fourth year, but of the years
/// that end a century only those that divide by 400.
pub const fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// How many days `year` has: 365, or 366 in a leap year.
pub const fn days_in_year(year: i32) -> u16 {
    if is_leap_year(year) { 366 } else { 365 }
}

/// How many days `month` (1 to 12) of `year` has. A number that is no month
/// has no days: the answer is 0.
pub const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// The day count of a year, a month (1 to 12) and a day of that month.
///
/// The year can be any `i32`. The month and the day are not checked:
/// [`Date::new`](crate::Date::new) checks them. A day past the month's end
/// counts on into the months after it, so day 30 of February is 1 or 2
/// March.
pub const fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    // The year that starts on 1 March: January and February end the one
    // before.
    let year = year as i64 - if month <= 2 { 1 } else { 0 };
    let cycle = year.div_euclid(400);
    let year_of_cycle = year.rem_euclid(400);
    let month_from_march = if month > 2 {
        month as i64 - 3
    } else {
        month as i64 + 9
    };
    // March to July and August to December each go 31, 30, 31, 30, 31 days:
    // 153 days in five months.
    let day_of_year = (153 * month_from_march + 2) / 5 + day as i64 - 1;
    let day_of_cycle = year_of_cycle * 365 + year_of_cycle / 4 - year_of_cycle / 100 + day_of_year;
    cycle * DAYS_PER_CYCLE + day_of_cycle - DAYS_TO_EPOCH
}

/// The year, the month (1 to 12) and the day of a day count, or `None` for a
/// count whose year does not fit in `i32`.
pub const fn civil_from_days(days: i64) -> Option<(i32, u8, u8)> {
    if days < FIRST_DAY || days > LAST_DAY {
        return None;
    }
    Some(civil_from_days_in_range(days))
}

/// [`civil_from_days`] for a count known to have a year that fits in `i32`:
/// every count a `Date` or a `Timestamp` holds does.
pub(crate) const fn civil_from_days_in_range(days: i64) -> (i32, u8, u8) {
    let days = days + DAYS_TO_EPOCH;
    let cycle = days.div_euclid(DAYS_PER_CYCLE);
    let day_of_cycle = days.rem_euclid(DAYS_PER_CYCLE);
    // Without its leap days the cycle is 400 years of 365 days. A leap day
    // has gone by after each 1,460 days, but not at the end of a century of
    // 36,524 days, save on the last day of the cycle.
    let year_of_cycle = (day_of_cycle - day_of_cycle / 1_460 + day_of_cycle / 36_524
        - day_of_cycle / 146_096)
        / 365;
    let day_of_year =
        day_of_cycle - (365 * year_of_cycle + year_of_cycle / 4 - year_of_cycle / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = cycle * 400 + year_of_cycle + if month <= 2 { 1 } else { 0 };
    (year as i32, month as u8, day as u8)
}

/// The weekday of a day count. 1 January 1970, day 0, was a Thursday.
pub const fn weekday_from_days(days: i64) -> Weekday {
    Weekday::from_days_from_monday((days.rem_euclid(7) as u8 + 3) % 7)
}

/// The day of the year, from 1, of a month (1 to 12) and a day of `year`.
pub(crate) const fn day_of_year(year: i32, month: u8, day: u8) -> u16 {
    days_before_month(year, month) + day as u16
}

/// The month and the day of the `ordinal`th day of `year`, counted from 1,
/// or `None` when the year has no such day.
pub(crate) const fn month_and_day(year: i32, ordinal: u16) -> Option<(u8, u8)> {
    if ordinal == 0 || ordinal > days_in_year(year) {
        return None;
    }
    let mut month = 12;
    while ordinal <= days_before_month(year, month) {
        month -= 1;
    }
    Some((month, (ordinal - days_before_month(year, month)) as u8))
}

/// Days of `year` before the first day of `month` (1 to 12).
const fn days_before_month(year: i32, month: u8) -> u16 {
    let leap_day = if month > 2 && is_leap_year(year) {
        1
    } else {
        0
    };
    DAYS_BEFORE_MONTH[month as usize - 1] + leap_day
}
