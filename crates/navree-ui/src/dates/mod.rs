//! Dates and times: [`Calendar`], [`DatePicker`], [`DateRangePicker`] and
//! [`TimePicker`], with the logic they share.
//!
//! A day is a year, a month and a day, and a time is hours and minutes: nowhere in
//! this module, on the server or in the browser, does a day pass through a zone.
//! The server decides what is allowed and checks every value that comes back; the
//! browser only helps the person get there. See [`zoned`] for turning a time on a
//! wall into an instant.

mod calendar;
mod data;
mod format;
pub mod grid;
mod locale;
mod parse;
mod picker;
mod range;
mod rules;
mod time;
mod time_picker;
pub mod zoned;

pub use calendar::{Calendar, CalendarMode, CaptionLayout};
pub use locale::{Clock, DateLabels, DateLocale, DateOrder, LocaleData};
pub use parse::{DateParseError, TimeParseError};
pub use picker::{DatePicker, DateRangePicker, PickerStyle};
pub use range::{DatePreset, DateRange, DateRangeQuery, parse_list};
pub use rules::{DateError, DateRules};
pub use time::{TimeError, TimeRules};
pub use time_picker::TimePicker;
