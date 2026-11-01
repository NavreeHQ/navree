//! Dates in a workbook: which number formats show a number as a date, a time
//! or a duration, and the date or time a number stands for.

use chrono::{NaiveDate, NaiveTime, TimeDelta};

use crate::Cell;

/// Whether the built-in number format `id` shows a date, a time or a
/// duration: the dates and times 14 to 22, 45 to 47, and the East Asian
/// ones, 27 to 36 and 50 to 58.
pub(super) fn builtin_shows_date(id: u16) -> bool {
    matches!(id, 14..=22 | 27..=36 | 45..=47 | 50..=58)
}

/// [`builtin_shows_date`] for an id as an XLSX file writes it (`14`; `014`
/// is no built-in format).
pub(super) fn builtin_id_shows_date(id: &str) -> bool {
    id.parse::<u16>()
        .is_ok_and(|number| builtin_shows_date(number) && number.to_string() == id)
}

/// Whether the number format `code` shows a date, a time or a duration.
///
/// Only the first section (before `;`) counts. Text in quotes, the
/// character after `\`, `_` or `*`, and anything in brackets (a colour, a
/// condition, a locale) are not date codes, except an elapsed time (`[h]`,
/// `[mm]`, `[ss]`). After an `a` (`AM/PM`, `a/p`), only `m`, `p` and `/`
/// still make it a date.
pub(super) fn shows_date(code: &str) -> bool {
    let mut escaped = false;
    let mut quoted = false;
    let mut brackets = 0u8;
    let mut previous = ' ';
    let mut elapsed = false;
    let mut after_a = false;
    for c in code.chars() {
        if escaped {
            escaped = false;
        } else if matches!(c, '_' | '\\' | '*') {
            escaped = true;
        } else if quoted {
            quoted = c != '"';
        } else if c == '"' {
            quoted = true;
        } else if c == ';' {
            return false;
        } else if c == '[' {
            brackets = brackets.wrapping_add(1);
        } else if c == ']' {
            if brackets == 1 && elapsed {
                return true;
            }
            brackets = brackets.saturating_sub(1);
        } else if brackets == 0 && !after_a && matches!(c, 'a' | 'A') {
            after_a = true;
        } else if brackets == 0
            && if after_a {
                matches!(c, 'p' | 'P' | 'm' | 'M' | '/')
            } else {
                matches!(c.to_ascii_lowercase(), 'd' | 'm' | 'h' | 'y' | 's')
            }
        {
            return true;
        } else if !(elapsed && c.eq_ignore_ascii_case(&previous)) {
            elapsed = previous == '[' && matches!(c.to_ascii_lowercase(), 'h' | 'm' | 's');
        }
        previous = c;
    }
    false
}

/// The days between 1900 and 1904, the two dates a workbook counts from.
const DAYS_1900_TO_1904: f64 = 1462.0;

/// The cell a number shown as a date stands for: the days since the
/// workbook's epoch (1899-12-30, or 1904-01-01 when `is_1904`), with the
/// time as the fraction, to the millisecond. A date at midnight is a
/// [`Cell::Date`]; one out of range stays the number.
///
/// The days before 1900-03-01 are counted with the day Excel puts on
/// 1900-02-29, which never was.
pub(crate) fn excel_date(serial: f64, is_1904: bool) -> Cell {
    let days = if is_1904 {
        serial + DAYS_1900_TO_1904
    } else {
        serial
    };
    let days = if days >= 60.0 { days } else { days + 1.0 };
    // A cast saturates (NaN is 0), as the milliseconds of a date in range
    // never do.
    let milliseconds = (days * 86_400_000.0).round() as i64;
    let at = NaiveDate::from_ymd_opt(1899, 12, 30)
        .map(|day| day.and_time(NaiveTime::MIN))
        .zip(TimeDelta::try_milliseconds(milliseconds))
        .and_then(|(epoch, since)| epoch.checked_add_signed(since));
    match at {
        Some(at) if at.time() == NaiveTime::MIN => Cell::Date(at.date()),
        Some(at) => Cell::DateTime(at),
        None => Cell::Float(serial),
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDateTime;

    use super::*;

    /// [`excel_date`] as a date and time.
    fn at(cell: Cell) -> Option<NaiveDateTime> {
        match cell {
            Cell::Date(day) => Some(day.and_time(NaiveTime::MIN)),
            Cell::DateTime(at) => Some(at),
            _ => None,
        }
    }

    #[test]
    fn custom_formats_that_show_dates() {
        for code in [
            "DD/MM/YY",
            "H:MM:SS;@",
            "m\"M\"d\"D\";@",
            "[h]:mm:ss",
            "[$-404]e\"\\xfc\"m\"\\xfc\"d\"\\xfc\"",
            "[h]",
            "[ss]",
            "[s].000",
            "[m]",
            "[mm]",
            "[Blue]\\+[h]:mm;[Red]\\-[h]:mm;[Green][h]:mm",
            "[>=100][Magenta][s].00",
            "[h]:mm;[=0]\\-",
            "ha/p\\\\m",
            "*-yyyy-mm-dd",
            "yyyy\\-mm\\-dd\\ hh:mm",
        ] {
            assert!(shows_date(code), "{code}");
        }
    }

    #[test]
    fn custom_formats_that_do_not_show_dates() {
        for code in [
            "#,##0\\ [$\\u20bd-46D]",
            "\"Y: \"0.00\"m\";\"Y: \"-0.00\"m\";\"Y: <num>m\";@",
            "#,##0\\ [$''u20bd-46D]",
            "\"$\"#,##0_);[Red](\"$\"#,##0)",
            "0_ ;[Red]\\-0\\ ",
            "\\Y000000",
            "#,##0.0####\" YMD\"",
            "[>=100][Magenta].00",
            "[>=100][Magenta]General",
            "#,##0.00\\ _M\"H\"_);[Red]#,##0.00\\ _M\"S\"_)",
            "#,##0*y",
            "0\"x\"*d",
            "*-#,##0",
            "0.00E+00",
            "General",
            "",
        ] {
            assert!(!shows_date(code), "{code}");
        }
    }

    #[test]
    fn built_in_formats_that_show_dates() {
        let dates: Vec<u16> = (0..200).filter(|id| builtin_shows_date(*id)).collect();
        let expected: Vec<u16> = (14..=22)
            .chain(27..=36)
            .chain(45..=47)
            .chain(50..=58)
            .collect();
        assert_eq!(dates, expected);
        assert!(builtin_id_shows_date("14"));
        assert!(!builtin_id_shows_date("014"));
        assert!(!builtin_id_shows_date("+14"));
        assert!(!builtin_id_shows_date("9"));
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn numbers_are_dates_counted_from_the_epoch() {
        assert_eq!(excel_date(46082.0, false), Cell::Date(day(2026, 3, 1)));
        assert_eq!(
            excel_date(46082.75, false),
            Cell::DateTime(day(2026, 3, 1).and_hms_opt(18, 0, 0).unwrap())
        );
        assert_eq!(excel_date(1000.0, true), Cell::Date(day(1906, 9, 27)));
        assert_eq!(excel_date(1000.0, false), Cell::Date(day(1902, 9, 26)));
        // Before 1900-03-01 a day is added for the 29th of February.
        assert_eq!(excel_date(61.0, false), Cell::Date(day(1900, 3, 1)));
        assert_eq!(excel_date(59.0, false), Cell::Date(day(1900, 2, 28)));
        assert_eq!(excel_date(60.0, false), Cell::Date(day(1900, 2, 28)));
        assert_eq!(excel_date(1.0, false), Cell::Date(day(1900, 1, 1)));
        // A time alone is a time on the first day.
        assert_eq!(
            at(excel_date(0.25, false)),
            day(1899, 12, 31).and_hms_opt(6, 0, 0)
        );
        // To the millisecond.
        assert_eq!(
            at(excel_date(45943.541, false)),
            day(2025, 10, 13).and_hms_milli_opt(12, 59, 2, 400)
        );
        // Out of range, the number stays.
        assert_eq!(excel_date(1e300, false), Cell::Float(1e300));
        assert_eq!(excel_date(-1e300, false), Cell::Float(-1e300));
        assert!(matches!(excel_date(f64::INFINITY, false), Cell::Float(n) if n.is_infinite()));
    }
}
