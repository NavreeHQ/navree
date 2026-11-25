//! [`Month`]: the months of the year.

use crate::{Error, Result, civil};

/// A month of the year.
///
/// ```
/// use navree_datetime::Month;
///
/// assert_eq!(Month::November.number(), 11);
/// assert_eq!(Month::try_from(2).unwrap(), Month::February);
/// assert_eq!(Month::February.days_in(2024), 29);
/// assert!(Month::March < Month::April);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Month {
    /// January.
    January = 1,
    /// February.
    February = 2,
    /// March.
    March = 3,
    /// April.
    April = 4,
    /// May.
    May = 5,
    /// June.
    June = 6,
    /// July.
    July = 7,
    /// August.
    August = 8,
    /// September.
    September = 9,
    /// October.
    October = 10,
    /// November.
    November = 11,
    /// December.
    December = 12,
}

impl Month {
    /// The month's number: January is 1 and December is 12.
    pub const fn number(self) -> u8 {
        self as u8
    }

    /// How many days the month has in `year`: 28 to 31. February has 29 in
    /// a leap year of the proleptic Gregorian calendar.
    pub const fn days_in(self, year: i32) -> u8 {
        civil::days_in_month(year, self as u8)
    }

    /// The month with this number, where there is one.
    pub(crate) const fn from_number(number: u8) -> Option<Month> {
        Some(match number {
            1 => Month::January,
            2 => Month::February,
            3 => Month::March,
            4 => Month::April,
            5 => Month::May,
            6 => Month::June,
            7 => Month::July,
            8 => Month::August,
            9 => Month::September,
            10 => Month::October,
            11 => Month::November,
            12 => Month::December,
            _ => return None,
        })
    }
}

/// A month from its number: 1 is January and 12 is December. Any other
/// number is an error.
impl TryFrom<u8> for Month {
    type Error = Error;

    fn try_from(number: u8) -> Result<Month> {
        Month::from_number(number)
            .ok_or_else(|| Error::invalid(format!("there is no month {number}")))
    }
}
