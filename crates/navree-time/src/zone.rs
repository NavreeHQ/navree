//! [`Zone`]: a validated IANA time zone, named the way every PostgreSQL
//! accepts it.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use navree_datetime::{DateTime, Disambiguation, Offset, Timestamp, Zoned};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

use crate::LocalDay;
use crate::tz::Tz;

/// The release of the IANA time zone database compiled into this build
/// (`2025b`). Zones, offsets, aliases and DST rules all come from it.
pub fn tzdata_version() -> &'static str {
    navree_datetime::Zone::tzdata_version()
}

/// A time zone of the IANA database (`Europe/Paris`, `Asia/Kolkata`,
/// `UTC`), checked and normalized.
///
/// [`Zone::parse`] trims the name, matches it without regard to case and
/// replaces legacy aliases with their current names (`Asia/Calcutta` is
/// `Asia/Kolkata`, `US/Eastern` is `America/New_York`, `Etc/UTC` and `GMT`
/// are `UTC`). Browsers still report some legacy names, and PostgreSQL
/// servers whose system tz data leaves out the backward-compatible links
/// reject them; a `Zone` always names a zone those servers accept. Names
/// that are still country locations (`Europe/Oslo`, `Africa/Accra`) are
/// kept as they are.
///
/// A `Zone` is `Copy`, compares and sorts by name, serializes as its name
/// and deserializes through [`Zone::parse`].
///
/// ```
/// use navree_time::Zone;
///
/// let zone = Zone::parse("Asia/Calcutta")?;
/// assert_eq!(zone.name(), "Asia/Kolkata");
/// assert_eq!(Zone::for_country("AE").unwrap().name(), "Asia/Dubai");
/// assert!(Zone::parse("Mars/Olympus_Mons").is_err());
/// # Ok::<(), navree_time::ZoneError>(())
/// ```
///
/// The names, the legacy aliases, the country zones and the rule for a
/// wall-clock time are [`navree_datetime::Zone`]'s: a `Zone` is a checked
/// named zone of the database.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Zone(navree_datetime::Zone);

/// A name that is not a time zone of the IANA database.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("unknown time zone `{0}`")]
pub struct ZoneError(String);

impl ZoneError {
    pub(crate) fn new(name: &str) -> Self {
        ZoneError(name.to_owned())
    }

    /// The rejected name.
    pub fn name(&self) -> &str {
        &self.0
    }
}

impl Zone {
    /// Coordinated Universal Time.
    pub const UTC: Zone = Zone(navree_datetime::Zone::UTC);

    /// The zone called `name` (trimmed, in any case; legacy aliases are
    /// replaced by their current names).
    pub fn parse(name: &str) -> Result<Zone, ZoneError> {
        navree_datetime::Zone::named(name)
            .map(Zone)
            .map_err(|_| ZoneError(name.to_owned()))
    }

    /// The current name of the zone called `name`, or `None` when `name`
    /// is no zone: `Zone::canonical_name("asia/calcutta")` is
    /// `Some("Asia/Kolkata")`.
    pub fn canonical_name(name: &str) -> Option<&'static str> {
        navree_datetime::Zone::canonical_name(name)
    }

    /// The zone's name (`Asia/Kolkata`).
    pub fn name(&self) -> &'static str {
        self.0.name()
    }

    /// The underlying [`navree_datetime::Zone`].
    pub const fn as_datetime_zone(&self) -> navree_datetime::Zone {
        self.0
    }

    /// The [`Tz`] entry with the same name in the compiled tz database.
    pub fn tz(&self) -> Tz {
        // A zone of this crate is always one of the database.
        Tz::of(self.0).unwrap_or(Tz::UTC)
    }

    /// Every zone, sorted by name, without legacy aliases.
    pub fn all() -> impl Iterator<Item = Zone> {
        navree_datetime::Zone::all().map(Zone)
    }

    /// The primary zone of `country`, an ISO 3166-1 alpha-2 code in any
    /// case (`IN` gives `Asia/Kolkata`, `US` gives `America/New_York`);
    /// `None` for an unknown code.
    pub fn for_country(country: &str) -> Option<Zone> {
        navree_datetime::Zone::for_country(country).map(Zone)
    }

    /// Every zone of `country`, its primary zone first; empty for an
    /// unknown code.
    pub fn in_country(country: &str) -> Vec<Zone> {
        navree_datetime::Zone::in_country(country)
            .into_iter()
            .map(Zone)
            .collect()
    }

    /// The ISO 3166-1 alpha-2 codes that have zones, sorted.
    pub fn countries_with_zones() -> impl Iterator<Item = &'static str> {
        navree_datetime::Zone::countries_with_zones()
    }

    /// The countries that use this zone (`["IN"]` for `Asia/Kolkata`);
    /// empty for zones such as `UTC`.
    pub fn countries(&self) -> Vec<&'static str> {
        self.0.countries()
    }

    /// `at` on this zone's clock.
    pub fn local(&self, at: Timestamp) -> Zoned {
        at.in_zone(self.0)
    }

    /// The UTC offset in force at `at`.
    pub fn offset_at(&self, at: Timestamp) -> Offset {
        self.0.offset_at(at)
    }

    /// The UTC offset in force at `at`, written `+05:30` or `-04:00`.
    pub fn offset_label(&self, at: Timestamp) -> String {
        offset_label(self.offset_at(at).seconds())
    }

    /// The instant at which this zone's clocks show `local`, using `how` for
    /// skipped or repeated wall-clock times.
    pub fn to_timestamp(
        &self,
        local: DateTime,
        how: Disambiguation,
    ) -> navree_datetime::Result<Timestamp> {
        self.0.to_timestamp(local, how)
    }

    /// The instant at which this zone's clocks show `local`.
    ///
    /// A time that happens twice (when clocks go back) is the earlier one;
    /// a time that is skipped (when clocks go forward) moves forward by the
    /// length of the gap, so 02:30 on the night clocks jump from 02:00 to
    /// 03:00 is 03:30.
    pub fn to_utc(&self, local: DateTime) -> Timestamp {
        self.0
            .to_timestamp(local, Disambiguation::Compatible)
            .unwrap_or_else(|_| local.in_utc())
    }

    /// The local day `now` falls on in this zone.
    pub fn today(&self, now: Timestamp) -> LocalDay {
        LocalDay::of(now, self)
    }
}

/// `+05:30` / `-04:00` for an offset in seconds east of UTC.
pub(crate) fn offset_label(seconds: i32) -> String {
    let sign = if seconds < 0 { '-' } else { '+' };
    let minutes = seconds.unsigned_abs() / 60;
    format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

/// Whether `country` (any case) is a country with zones.
pub(crate) fn is_country(country: &str) -> bool {
    navree_datetime::Zone::for_country(country).is_some()
}

impl Default for Zone {
    /// [`Zone::UTC`].
    fn default() -> Self {
        Zone::UTC
    }
}

impl PartialOrd for Zone {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Zone {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name().cmp(other.name())
    }
}

impl fmt::Display for Zone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl fmt::Debug for Zone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Zone").field(&self.name()).finish()
    }
}

impl FromStr for Zone {
    type Err = ZoneError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Zone::parse(s)
    }
}

impl From<Zone> for Tz {
    fn from(zone: Zone) -> Self {
        zone.tz()
    }
}

impl From<Zone> for navree_datetime::Zone {
    fn from(zone: Zone) -> Self {
        zone.0
    }
}

impl From<&Zone> for navree_datetime::Zone {
    fn from(zone: &Zone) -> Self {
        zone.0
    }
}

impl From<&Zone> for Zone {
    fn from(zone: &Zone) -> Self {
        *zone
    }
}

impl Serialize for Zone {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for Zone {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Zone::parse(&name).map_err(de::Error::custom)
    }
}

impl navree_openapi::ToSchema for Zone {
    fn schema() -> navree_openapi::Schema {
        navree_openapi::Schema::string()
            .description("An IANA time zone name")
            .examples(vec![serde_json::json!("Europe/Paris")])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_labels_have_a_sign_hours_and_minutes() {
        assert_eq!(offset_label(0), "+00:00");
        assert_eq!(offset_label(19_800), "+05:30");
        assert_eq!(offset_label(-14_400), "-04:00");
        assert_eq!(offset_label(-34_200), "-09:30");
        assert_eq!(offset_label(50_400), "+14:00");
    }

    #[test]
    fn every_country_zone_is_canonical_and_countries_are_sorted() {
        let countries: Vec<&str> = Zone::countries_with_zones().collect();
        assert!(countries.windows(2).all(|w| w[0] < w[1]));
        for country in countries {
            assert!(is_country(country), "{country}");
            assert!(is_country(&country.to_ascii_lowercase()), "{country}");
            for zone in Zone::in_country(country) {
                assert_eq!(
                    Zone::canonical_name(zone.name()),
                    Some(zone.name()),
                    "{country}: {zone}"
                );
                assert!(zone.countries().contains(&country), "{country}: {zone}");
            }
        }
        assert!(!is_country("XX"));
    }

    #[test]
    fn factory_is_not_a_zone() {
        assert!(Zone::parse("Factory").is_err());
        assert!(Zone::all().all(|zone| zone.name() != "Factory"));
    }
}
