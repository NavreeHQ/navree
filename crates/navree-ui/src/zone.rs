//! The reader's time zone (the `time` feature): today's day for the date pickers and the
//! zone the times of a timeline and a feed are shown in, read from navree-time's
//! [`CurrentZone`] instead of passed by hand.
//!
//! A day in the toolkit never passes through a zone, which is right: a day is a day. What
//! needs the person's or the workspace's zone is *today* (the day a calendar marks, the
//! day every preset counts from) and the clock times of an event. The app reads
//! [`CurrentZone`] in the handler and gives it to the component:
//!
//! ```
//! # fn page(zone: &navree_time::CurrentZone) {
//! use navree_ui::dates::DateRangePicker;
//!
//! let picker = DateRangePicker::new("from", "to").today_in(zone);
//! # let _ = picker;
//! # }
//! ```

use chrono::Offset as _;
use navree_time::CurrentZone;

use crate::components::{ActivityFeed, Timeline};
use crate::dates::{Calendar, DatePicker, DateRangePicker};

/// The zone's name and its offset from UTC in seconds at this moment, as a timeline and a
/// feed take them.
fn name_and_offset(zone: &CurrentZone) -> (String, i32) {
    let offset = zone.now().offset().fix().local_minus_utc();
    (zone.name().to_owned(), offset)
}

impl DatePicker {
    /// Today in `zone`, on the app's clock: the day the calendar marks. Without it,
    /// today is the day in UTC.
    pub fn today_in(self, zone: &CurrentZone) -> Self {
        self.today(zone.today().date())
    }
}

impl DateRangePicker {
    /// Today in `zone`, on the app's clock: every preset is counted from it. Without it,
    /// today is the day in UTC.
    pub fn today_in(self, zone: &CurrentZone) -> Self {
        self.today(zone.today().date())
    }
}

impl Calendar {
    /// Today in `zone`, on the app's clock: the day the calendar marks. Without it,
    /// today is the day in UTC.
    pub fn today_in(self, zone: &CurrentZone) -> Self {
        self.today(zone.today().date())
    }
}

impl Timeline {
    /// Shows the times in `zone`: its name and its offset from UTC now, as
    /// [`time_zone`](Self::time_zone) takes them.
    pub fn zone(self, zone: &CurrentZone) -> Self {
        let (name, offset) = name_and_offset(zone);
        self.time_zone(name, offset)
    }
}

impl ActivityFeed {
    /// Shows the times in `zone`: its name and its offset from UTC now, as
    /// [`time_zone`](Self::time_zone) takes them.
    pub fn zone(self, zone: &CurrentZone) -> Self {
        let (name, offset) = name_and_offset(zone);
        self.time_zone(name, offset)
    }
}
