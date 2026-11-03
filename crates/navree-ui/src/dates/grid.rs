//! The month grid: which days stand in which row.

use chrono::{Datelike, Duration, NaiveDate, Weekday};

/// The first year the calendar draws.
pub const FIRST_YEAR: i32 = 1;
/// The last year the calendar draws.
pub const LAST_YEAR: i32 = 9999;

/// The earliest day a calendar shows: 1 January of year 1.
pub fn earliest() -> NaiveDate {
    NaiveDate::from_ymd_opt(FIRST_YEAR, 1, 1).unwrap_or(NaiveDate::MIN)
}

/// The latest day a calendar shows: 31 December of year 9999.
pub fn latest() -> NaiveDate {
    NaiveDate::from_ymd_opt(LAST_YEAR, 12, 31).unwrap_or(NaiveDate::MAX)
}

/// Moves a day into the range the calendar can show.
pub fn clamp(date: NaiveDate) -> NaiveDate {
    date.clamp(earliest(), latest())
}

/// How many days month `month` of `year` has.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month >= 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    match (
        NaiveDate::from_ymd_opt(year, month, 1),
        NaiveDate::from_ymd_opt(next_year, next_month, 1),
    ) {
        (Some(first), Some(next)) => u32::try_from((next - first).num_days()).unwrap_or(31),
        _ => 31,
    }
}

/// A month: `(year, month)` moved by `delta` months, or `None` outside the years
/// the calendar draws.
pub fn add_months(year: i32, month: u32, delta: i32) -> Option<(i32, u32)> {
    let index = year
        .checked_mul(12)?
        .checked_add(i32::try_from(month).ok()? - 1)?;
    let moved = index.checked_add(delta)?;
    let (year, month) = (
        moved.div_euclid(12),
        u32::try_from(moved.rem_euclid(12)).ok()? + 1,
    );
    (FIRST_YEAR..=LAST_YEAR)
        .contains(&year)
        .then_some((year, month))
}

/// One month as rows of seven days, the first starting on `week_start`. Days of the
/// neighbouring months fill the first and last rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonthGrid {
    /// The year.
    pub year: i32,
    /// The month, 1 to 12.
    pub month: u32,
    /// The weeks.
    pub rows: Vec<[NaiveDate; 7]>,
}

/// The grid of a month. `fixed_weeks` always gives six rows.
///
/// ```
/// use navree_ui::dates::grid::month_grid;
/// use chrono::Weekday;
///
/// let grid = month_grid(2026, 2, Weekday::Sun, false);
/// // 1 February 2026 is a Sunday and the month has 28 days: four rows exactly.
/// assert_eq!(grid.rows.len(), 4);
/// assert_eq!(month_grid(2026, 2, Weekday::Sun, true).rows.len(), 6);
/// ```
pub fn month_grid(year: i32, month: u32, week_start: Weekday, fixed_weeks: bool) -> MonthGrid {
    let first = NaiveDate::from_ymd_opt(year, month, 1)
        .or_else(|| NaiveDate::from_ymd_opt(year, 1, 1))
        .unwrap_or_else(earliest);
    let lead = (first.weekday().num_days_from_sunday() + 7 - week_start.num_days_from_sunday()) % 7;
    let used = lead + days_in_month(first.year(), first.month());
    let rows_needed = if fixed_weeks { 6 } else { used.div_ceil(7) };
    let start = first - Duration::days(i64::from(lead));
    let rows = (0..rows_needed)
        .map(|row| {
            std::array::from_fn(|column| {
                let offset = i64::from(row * 7) + i64::try_from(column).unwrap_or(0);
                start + Duration::days(offset)
            })
        })
        .collect();
    MonthGrid {
        year: first.year(),
        month: first.month(),
        rows,
    }
}

/// The ISO-8601 week number of a row: the week of the row's Thursday. A row starting
/// on Monday gets its ISO week; a row that starts on Sunday or Saturday gets the ISO
/// week that most of its days belong to.
pub fn week_number(row: &[NaiveDate; 7]) -> u32 {
    row.iter()
        .find(|day| day.weekday() == Weekday::Thu)
        .map_or(0, |day| day.iso_week().week())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    const STARTS: [Weekday; 7] = [
        Weekday::Sun,
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
    ];

    #[test]
    fn month_lengths_are_right() {
        for (year, month, days) in [
            (2024, 2, 29),
            (2025, 2, 28),
            (2000, 2, 29),
            (1900, 2, 28),
            (2100, 2, 28),
            (2026, 1, 31),
            (2026, 4, 30),
            (2026, 12, 31),
            (9999, 12, 31),
            (1, 1, 31),
        ] {
            assert_eq!(days_in_month(year, month), days, "{year}-{month}");
        }
    }

    #[test]
    fn every_month_from_1583_to_2400_fills_its_rows() {
        for year in (1583..=2400).step_by(1) {
            for month in 1..=12 {
                for start in STARTS {
                    let grid = month_grid(year, month, start, false);
                    let rows = grid.rows.len();
                    assert!((4..=6).contains(&rows), "{year}-{month} {start}: {rows}");
                    // The first column is the week's first day; days run on without a gap.
                    let mut previous = None;
                    let mut in_month = 0;
                    for row in &grid.rows {
                        assert_eq!(row[0].weekday(), start);
                        for day in row {
                            if let Some(previous) = previous {
                                assert_eq!(*day - previous, Duration::days(1));
                            }
                            previous = Some(*day);
                            if day.month() == month && day.year() == year {
                                in_month += 1;
                            }
                        }
                    }
                    assert_eq!(in_month, days_in_month(year, month));
                    // The first of the month is in the first row, the last in the last.
                    assert!(
                        grid.rows[0]
                            .iter()
                            .any(|d| d.day() == 1 && d.month() == month)
                    );
                    assert!(
                        grid.rows[rows - 1]
                            .iter()
                            .any(|d| d.day() == days_in_month(year, month) && d.month() == month)
                    );
                    assert_eq!(month_grid(year, month, start, true).rows.len(), 6);
                }
            }
        }
    }

    #[test]
    fn february_can_take_four_rows() {
        // 1 February 2026 is a Sunday.
        assert_eq!(month_grid(2026, 2, Weekday::Sun, false).rows.len(), 4);
        assert_eq!(month_grid(2026, 2, Weekday::Mon, false).rows.len(), 5);
        // A 31-day month starting on the last column needs six.
        assert_eq!(month_grid(2026, 8, Weekday::Sun, false).rows.len(), 6);
    }

    #[test]
    fn the_edges_of_the_calendar_do_not_panic() {
        let first = month_grid(1, 1, Weekday::Mon, true);
        assert_eq!(first.rows.len(), 6);
        let last = month_grid(9999, 12, Weekday::Sat, true);
        assert_eq!(last.rows.len(), 6);
        // A month that doesn't exist falls back to January rather than panic.
        assert_eq!(month_grid(2026, 13, Weekday::Mon, false).month, 1);
    }

    #[test]
    fn months_move_across_years() {
        assert_eq!(add_months(2026, 12, 1), Some((2027, 1)));
        assert_eq!(add_months(2026, 1, -1), Some((2025, 12)));
        assert_eq!(add_months(2026, 11, -23), Some((2024, 12)));
        assert_eq!(add_months(2026, 3, 24), Some((2028, 3)));
        assert_eq!(add_months(1, 1, -1), None);
        assert_eq!(add_months(9999, 12, 1), None);
        assert_eq!(add_months(i32::MAX, 12, 1), None);
    }

    #[test]
    fn week_numbers_follow_iso_for_monday_rows() {
        let weeks = |year, month| -> Vec<u32> {
            month_grid(year, month, Weekday::Mon, false)
                .rows
                .iter()
                .map(week_number)
                .collect()
        };
        // 2026-01-01 is a Thursday: week 1. 2020-12-31 was a Thursday of week 53.
        assert_eq!(weeks(2026, 1), vec![1, 2, 3, 4, 5]);
        assert_eq!(weeks(2020, 12), vec![49, 50, 51, 52, 53]);
        // 2021-01-01 is a Friday: still week 53 of 2020.
        assert_eq!(weeks(2021, 1)[0], 53);
        assert_eq!(weeks(2024, 12), vec![48, 49, 50, 51, 52, 1]);
        // 2015 had 53 weeks, 2016 begins in week 53.
        assert_eq!(weeks(2016, 1)[0], 53);
    }

    #[test]
    fn a_sunday_row_takes_the_week_most_of_it_belongs_to() {
        let rows = month_grid(2026, 1, Weekday::Sun, false).rows;
        // Sun 28 Dec 2025 to Sat 3 Jan 2026: its Thursday is 1 Jan, week 1.
        assert_eq!(rows[0][0], day(2025, 12, 28));
        assert_eq!(week_number(&rows[0]), 1);
    }
}
