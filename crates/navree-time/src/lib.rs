//! # navree-time
//!
//! `TimeModule`: time zones for tenants and people. Dates are stored in
//! UTC; days, reports and messages are counted on the clock of the person
//! or workspace they are for.
//!
//! - **Zones.** [`Zone`] is a checked IANA zone. It parses names in any
//!   case and replaces legacy aliases with current names
//!   (`Asia/Calcutta` is `Asia/Kolkata`), so every PostgreSQL accepts what
//!   it stores. [`Zone::for_country`] gives a country's primary zone.
//! - **The request's zone.** [`CurrentZone`] resolves the signed-in
//!   user's zone, else the workspace's, else the organization's, else
//!   `time.default_zone`, through the `dyn ZoneSource` multi-bindings of
//!   navree-contracts (navree-workspace registers the workspace and
//!   organization levels). [`Zones`] does the
//!   same for jobs: [`Zones::zone_of`] and [`Zones::owners`].
//! - **Local days.** [`LocalDay`] (`yyyy-MM-dd`) and [`DayRange`] (from and
//!   to, both included) turn into the UTC instants they span in a zone,
//!   including the 23- and 25-hour days of DST changes.
//! - **Send windows.** [`SendWindow`] (`weekdays().between("09:00",
//!   "17:00")`) finds the next slot on the recipient's clock, skipping
//!   [`Holidays`] and following each country's weekend
//!   ([`weekend_days`]).
//! - **SQL.** [`Zone::sql`] gives `AT TIME ZONE` expressions with a
//!   checked zone, for reports that group rows by local day in the
//!   database ([`ZoneSql`]).
//! - **Responses.** [`WithZone`] adds `timeZone` to a response body.
//! - **Routes and commands.** `GET /api/time/zones` (public reference data
//!   for zone pickers, unless `time.routes` is false) and `time:zones`.
//!
//! ```no_run
//! use navree_config::{ConfigModule, ConfigSources};
//! use navree_http::HttpModule;
//! use navree_kernel::Navree;
//! use navree_time::TimeModule;
//!
//! # fn main() -> Result<(), navree_kernel::BoxError> {
//! Navree::app()
//!     .module(ConfigModule::for_root(ConfigSources::defaults()))
//!     .module(HttpModule::from_config())
//!     .module(TimeModule::from_config()) // section `time`
//!     .run()
//! # }
//! ```
#![forbid(unsafe_code)]

mod commands;
mod controller;
mod current;
mod day;
mod fake;
mod holidays;
mod module;
mod options;
pub mod rules;
mod sql;
mod tables;
mod window;
mod zone;
mod zones;

pub use commands::zones_listing;
pub use controller::{TimeController, ZoneListQuery, ZoneListResource, ZoneResource};
pub use current::{CurrentZone, TIME_ZONE_KEY, WithZone};
pub use day::{DayRange, DayRangeError, LocalDay, LocalDayError};
pub use fake::ZonesFake;
pub use holidays::{
    BuiltInHolidays, FixedHolidays, Holiday, HolidayCalendar, Holidays, easter_sunday,
};
pub use module::{ModuleBuilderTimeExt, TimeModule};
pub use navree_contracts::{ZoneLevel, ZoneOwner, ZoneRequest, ZoneSource};
pub use options::{HolidayOptions, TimeOptions};
pub use sql::ZoneSql;
pub use window::{SendWindow, WindowError, weekend_days};
pub use zone::{Zone, ZoneError, tzdata_version};
pub use zones::Zones;
