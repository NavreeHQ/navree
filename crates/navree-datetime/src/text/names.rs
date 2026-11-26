//! The English names of the months and of the days of the week.
//!
//! The text of a date is written with these names whatever the language of
//! the machine or of the person reading: `%b` is `Nov` everywhere. Showing a
//! date in someone's own language is a matter for the code that shows it.

use crate::{Month, Weekday};

/// The months' names, from January.
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The first three letters of the months' names, from January.
const SHORT_MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The weekdays' names, from Monday.
const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// The first three letters of the weekdays' names, from Monday.
const SHORT_WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

impl Month {
    /// The month's English name: `January` to `December`.
    ///
    /// ```
    /// use navree_datetime::Month;
    ///
    /// assert_eq!(Month::November.name(), "November");
    /// ```
    pub const fn name(self) -> &'static str {
        MONTHS[self.number() as usize - 1]
    }

    /// The first three letters of the month's English name: `Jan` to `Dec`.
    ///
    /// ```
    /// use navree_datetime::Month;
    ///
    /// assert_eq!(Month::November.short_name(), "Nov");
    /// ```
    pub const fn short_name(self) -> &'static str {
        SHORT_MONTHS[self.number() as usize - 1]
    }
}

impl Weekday {
    /// The day's English name: `Monday` to `Sunday`.
    ///
    /// ```
    /// use navree_datetime::Weekday;
    ///
    /// assert_eq!(Weekday::Wednesday.name(), "Wednesday");
    /// ```
    pub const fn name(self) -> &'static str {
        WEEKDAYS[self.days_from_monday() as usize]
    }

    /// The first three letters of the day's English name: `Mon` to `Sun`.
    /// It is what the day is shown as.
    ///
    /// ```
    /// use navree_datetime::Weekday;
    ///
    /// assert_eq!(Weekday::Wednesday.short_name(), "Wed");
    /// assert_eq!(Weekday::Wednesday.to_string(), "Wed");
    /// ```
    pub const fn short_name(self) -> &'static str {
        SHORT_WEEKDAYS[self.days_from_monday() as usize]
    }
}
