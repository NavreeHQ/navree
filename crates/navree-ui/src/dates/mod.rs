//! Dates and times: [`Calendar`], [`DatePicker`], [`DateRangePicker`] and
//! [`TimePicker`], with the logic they share.
//!
//! A day is a year, a month and a day, and a time is hours and minutes: nowhere in
//! this module, on the server or in the browser, does a day pass through a zone.
//! The server decides what is allowed and checks every value that comes back; the
//! browser only helps the person get there. See [`zoned`] for turning a time on a
//! wall into an instant.

mod data;
mod format;
pub mod grid;
mod locale;
mod parse;
mod range;
mod rules;
mod time;
pub mod zoned;

pub use locale::{Clock, DateLabels, DateLocale, DateOrder, LocaleData};
pub use parse::{DateParseError, TimeParseError};
pub use range::{DatePreset, DateRange, DateRangeQuery};
pub use rules::{DateError, DateRules};
pub use time::{TimeError, TimeRules};
