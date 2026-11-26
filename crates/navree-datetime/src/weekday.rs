//! [`Weekday`]: the days of the week.

use crate::{Error, Result};

/// A day of the week.
///
/// A week has no first day of its own: a locale starts it on Monday, Sunday
/// or Saturday. So a weekday has no order, and every number it gives says
/// which day it counts from.
///
/// ```
/// use navree_datetime::Weekday;
///
/// assert_eq!(Weekday::Wednesday.number_from_monday(), 3);
/// assert_eq!(Weekday::Wednesday.days_from_sunday(), 3);
/// assert_eq!(Weekday::Sunday.succ(), Weekday::Monday);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weekday {
    /// Monday.
    Monday,
    /// Tuesday.
    Tuesday,
    /// Wednesday.
    Wednesday,
    /// Thursday.
    Thursday,
    /// Friday.
    Friday,
    /// Saturday.
    Saturday,
    /// Sunday.
    Sunday,
}

impl Weekday {
    /// The day's number in a week that starts on Monday: Monday is 1 and
    /// Sunday is 7 (the ISO 8601 weekday number).
    pub const fn number_from_monday(self) -> u8 {
        self.days_from_monday() + 1
    }

    /// The day's number in a week that starts on Sunday: Sunday is 1 and
    /// Saturday is 7.
    pub const fn number_from_sunday(self) -> u8 {
        self.days_from_sunday() + 1
    }

    /// How many days after Monday the day is: Monday is 0 and Sunday is 6.
    /// It is the place of the day in a table of seven that starts on Monday.
    pub const fn days_from_monday(self) -> u8 {
        self as u8
    }

    /// How many days after Sunday the day is: Sunday is 0 and Saturday is 6.
    /// It is the place of the day in a table of seven that starts on Sunday.
    pub const fn days_from_sunday(self) -> u8 {
        (self as u8 + 1) % 7
    }

    /// The day after: Monday after Sunday.
    #[must_use]
    pub const fn succ(self) -> Weekday {
        Weekday::from_days_from_monday((self as u8 + 1) % 7)
    }

    /// The day before: Sunday before Monday.
    #[must_use]
    pub const fn pred(self) -> Weekday {
        Weekday::from_days_from_monday((self as u8 + 6) % 7)
    }

    /// The day that is `days` days after Monday, counted round the week.
    pub(crate) const fn from_days_from_monday(days: u8) -> Weekday {
        match days % 7 {
            0 => Weekday::Monday,
            1 => Weekday::Tuesday,
            2 => Weekday::Wednesday,
            3 => Weekday::Thursday,
            4 => Weekday::Friday,
            5 => Weekday::Saturday,
            _ => Weekday::Sunday,
        }
    }
}

/// A weekday from the number [`days_from_monday`](Weekday::days_from_monday)
/// gives: 0 is Monday and 6 is Sunday. Any other number is an error.
impl TryFrom<u8> for Weekday {
    type Error = Error;

    fn try_from(days_from_monday: u8) -> Result<Weekday> {
        if days_from_monday < 7 {
            Ok(Weekday::from_days_from_monday(days_from_monday))
        } else {
            Err(Error::invalid(format!(
                "{days_from_monday} is not a weekday: 0 is Monday and 6 is Sunday"
            )))
        }
    }
}
