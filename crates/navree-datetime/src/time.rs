//! [`Time`]: a time of day.

use crate::{Error, Result};

const NANOS_PER_SEC: u32 = 1_000_000_000;

/// A time of day, to the nanosecond, as a clock on a wall shows it: no day
/// and no zone.
///
/// A minute always has 60 seconds: a time of day holds no leap second, so
/// `Time::new(23, 59, 60)` is an error.
///
/// ```
/// use navree_datetime::Time;
///
/// # fn main() -> navree_datetime::Result<()> {
/// let opens = Time::new(9, 30, 0)?;
/// assert_eq!((opens.hour(), opens.minute(), opens.second()), (9, 30, 0));
/// assert!(opens < Time::new(17, 0, 0)?);
/// assert_eq!(opens.with_nanos(250_000_000)?.nanosecond(), 250_000_000);
/// assert!(Time::new(24, 0, 0).is_err());
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Time {
    hour: u8,
    minute: u8,
    second: u8,
    /// Nanoseconds after the second: 0 to 999,999,999.
    nanos: u32,
}

impl Time {
    /// 00:00:00, the first moment of a day.
    pub const MIDNIGHT: Time = Time {
        hour: 0,
        minute: 0,
        second: 0,
        nanos: 0,
    };

    /// The time `hour`:`minute`:`second`, on the second.
    ///
    /// # Errors
    ///
    /// An hour past 23, or a minute or a second past 59, is invalid.
    pub fn new(hour: u8, minute: u8, second: u8) -> Result<Time> {
        if hour > 23 {
            return Err(Error::invalid(format!("a day has no hour {hour}")));
        }
        if minute > 59 {
            return Err(Error::invalid(format!("an hour has no minute {minute}")));
        }
        if second > 59 {
            return Err(Error::invalid(format!("a minute has no second {second}")));
        }
        Ok(Time {
            hour,
            minute,
            second,
            nanos: 0,
        })
    }

    /// The same hour, minute and second, `nanos` nanoseconds into the
    /// second.
    ///
    /// # Errors
    ///
    /// A count of 1,000,000,000 or more, a whole second, is invalid.
    pub fn with_nanos(self, nanos: u32) -> Result<Time> {
        if nanos >= NANOS_PER_SEC {
            return Err(Error::invalid(format!(
                "a second has no nanosecond {nanos}"
            )));
        }
        Ok(Time { nanos, ..self })
    }

    /// The same time in another hour.
    ///
    /// # Errors
    ///
    /// An hour past 23 is invalid.
    pub fn with_hour(self, hour: u8) -> Result<Time> {
        Ok(Time {
            nanos: self.nanos,
            ..Time::new(hour, self.minute, self.second)?
        })
    }

    /// The same time in another minute of its hour.
    ///
    /// # Errors
    ///
    /// A minute past 59 is invalid.
    pub fn with_minute(self, minute: u8) -> Result<Time> {
        Ok(Time {
            nanos: self.nanos,
            ..Time::new(self.hour, minute, self.second)?
        })
    }

    /// The same time in another second of its minute.
    ///
    /// # Errors
    ///
    /// A second past 59 is invalid.
    pub fn with_second(self, second: u8) -> Result<Time> {
        Ok(Time {
            nanos: self.nanos,
            ..Time::new(self.hour, self.minute, second)?
        })
    }

    /// The hour: 0 to 23.
    pub const fn hour(self) -> u8 {
        self.hour
    }

    /// The minute of the hour: 0 to 59.
    pub const fn minute(self) -> u8 {
        self.minute
    }

    /// The second of the minute: 0 to 59.
    pub const fn second(self) -> u8 {
        self.second
    }

    /// The nanoseconds after the second: 0 to 999,999,999.
    pub const fn nanosecond(self) -> u32 {
        self.nanos
    }

    /// Seconds from midnight to the time's second: 0 to 86,399.
    pub(crate) const fn second_of_day(self) -> u32 {
        self.hour as u32 * 3_600 + self.minute as u32 * 60 + self.second as u32
    }

    /// The time `second_of_day` seconds after midnight (below 86,400) and
    /// `nanos` nanoseconds (below a second) after that. Neither is checked.
    pub(crate) const fn from_second_of_day(second_of_day: u32, nanos: u32) -> Time {
        Time {
            hour: (second_of_day / 3_600) as u8,
            minute: (second_of_day / 60 % 60) as u8,
            second: (second_of_day % 60) as u8,
            nanos,
        }
    }
}

/// Midnight.
impl Default for Time {
    fn default() -> Time {
        Time::MIDNIGHT
    }
}
