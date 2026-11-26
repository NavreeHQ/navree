//! [`Offset`]: a fixed distance from UTC.

use crate::{Error, Result};

/// The furthest an offset goes from UTC, either way: 25 hours, 59 minutes
/// and 59 seconds.
const LIMIT_SECS: i32 = 25 * 3_600 + 59 * 60 + 59;

/// A fixed distance from UTC, in seconds: what a clock in some place shows
/// ahead of UTC, or behind it.
///
/// It is kept to the second, not the minute, since the local mean times
/// zones kept before railways are off by odd seconds. An offset east of UTC
/// is ahead of it and counts up; one west of it is behind and counts down.
///
/// ```
/// use navree_datetime::Offset;
///
/// # fn main() -> navree_datetime::Result<()> {
/// let india = Offset::east(5 * 3_600 + 30 * 60)?;
/// assert_eq!(india.seconds(), 19_800);
/// assert_eq!(Offset::west(5 * 3_600)?.seconds(), -18_000);
/// assert_eq!(Offset::UTC.seconds(), 0);
/// assert!(Offset::east(26 * 3_600).is_err());
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Offset {
    /// Seconds east of UTC, below zero for west.
    secs: i32,
}

impl Offset {
    /// No distance: UTC itself.
    pub const UTC: Offset = Offset { secs: 0 };

    /// The offset `seconds` east of UTC (ahead of it), or west for a number
    /// below zero.
    ///
    /// # Errors
    ///
    /// An offset of more than 25:59:59 either way is out of range.
    pub fn east(seconds: i32) -> Result<Offset> {
        if seconds.unsigned_abs() > LIMIT_SECS.unsigned_abs() {
            return Err(Error::out_of_range(format!(
                "an offset of {seconds} seconds is more than 25:59:59 from UTC"
            )));
        }
        Ok(Offset { secs: seconds })
    }

    /// The offset `seconds` west of UTC (behind it), or east for a number
    /// below zero.
    ///
    /// # Errors
    ///
    /// An offset of more than 25:59:59 either way is out of range.
    pub fn west(seconds: i32) -> Result<Offset> {
        // The opposite of a number that passes the check is always there.
        Offset::east(seconds).map(|offset| Offset { secs: -offset.secs })
    }

    /// Seconds east of UTC: above zero ahead of it, below zero behind.
    pub const fn seconds(self) -> i32 {
        self.secs
    }

    /// The offset `seconds` east of UTC, for a number known to be within
    /// 25:59:59 of UTC. It is not checked: the offsets of the compiled time
    /// zone tables come through here, and a test of the tables holds each
    /// of them to the limit.
    pub(crate) const fn from_seconds_unchecked(seconds: i32) -> Offset {
        Offset { secs: seconds }
    }
}
