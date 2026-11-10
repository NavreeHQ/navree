//! [`Zone`]: a validated IANA time zone, named the way every PostgreSQL
//! accepts it.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

use chrono::{DateTime, FixedOffset, LocalResult, NaiveDateTime, Offset, TimeDelta, TimeZone, Utc};
use chrono_tz::{TZ_VARIANTS, Tz};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

use crate::LocalDay;
use crate::tables::{ALIASES, COUNTRY_ZONES, TZDATA_VERSION};

/// Names in the tz database that are not places: never accepted.
const NOT_ZONES: &[&str] = &["Factory"];

/// Every name chrono-tz knows (zones and links), by lower-cased name.
static BY_LOWER_NAME: LazyLock<HashMap<String, Tz>> = LazyLock::new(|| {
    TZ_VARIANTS
        .iter()
        .filter(|tz| !NOT_ZONES.contains(&tz.name()))
        .map(|tz| (tz.name().to_ascii_lowercase(), *tz))
        .collect()
});

/// Legacy names, lower-cased, with the zone each stands for.
static ALIAS_TARGETS: LazyLock<HashMap<String, Tz>> = LazyLock::new(|| {
    ALIASES
        .iter()
        .filter_map(|(alias, target)| {
            let tz = Tz::from_str(target).ok()?;
            Some((alias.to_ascii_lowercase(), tz))
        })
        .collect()
});

/// Every canonical zone, sorted by name.
static ALL: LazyLock<Vec<Zone>> = LazyLock::new(|| {
    let mut zones: Vec<Zone> = TZ_VARIANTS
        .iter()
        .filter(|tz| {
            !NOT_ZONES.contains(&tz.name())
                && !ALIAS_TARGETS.contains_key(&tz.name().to_ascii_lowercase())
        })
        .map(|tz| Zone(*tz))
        .collect();
    zones.sort();
    zones.dedup();
    zones
});

/// Zone name → the countries that use it.
static COUNTRIES_OF: LazyLock<HashMap<&'static str, Vec<&'static str>>> = LazyLock::new(|| {
    let mut map: HashMap<&'static str, Vec<&'static str>> = HashMap::new();
    for (country, zones) in COUNTRY_ZONES {
        for zone in *zones {
            map.entry(zone).or_default().push(country);
        }
    }
    map
});

/// The release of the IANA time zone database compiled into this build
/// (`2025b`). Zones, offsets, aliases and DST rules all come from it.
pub fn tzdata_version() -> &'static str {
    TZDATA_VERSION
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
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Zone(Tz);

/// A name that is not a time zone of the IANA database.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("unknown time zone `{0}`")]
pub struct ZoneError(String);

impl ZoneError {
    /// The rejected name.
    pub fn name(&self) -> &str {
        &self.0
    }
}

impl Zone {
    /// Coordinated Universal Time.
    pub const UTC: Zone = Zone(Tz::UTC);

    /// The zone called `name` (trimmed, in any case; legacy aliases are
    /// replaced by their current names).
    pub fn parse(name: &str) -> Result<Zone, ZoneError> {
        lookup(name).ok_or_else(|| ZoneError(name.to_owned()))
    }

    /// The current name of the zone called `name`, or `None` when `name`
    /// is no zone: `Zone::canonical_name("asia/calcutta")` is
    /// `Some("Asia/Kolkata")`.
    pub fn canonical_name(name: &str) -> Option<&'static str> {
        lookup(name).map(|zone| zone.name())
    }

    /// The zone's name (`Asia/Kolkata`).
    pub fn name(&self) -> &'static str {
        self.0.name()
    }

    /// The chrono-tz zone, for chrono's own conversions.
    pub fn tz(&self) -> Tz {
        self.0
    }

    /// Every zone, sorted by name, without legacy aliases.
    pub fn all() -> impl Iterator<Item = Zone> {
        ALL.iter().copied()
    }

    /// The primary zone of `country`, an ISO 3166-1 alpha-2 code in any
    /// case (`IN` gives `Asia/Kolkata`, `US` gives `America/New_York`);
    /// `None` for an unknown code.
    pub fn for_country(country: &str) -> Option<Zone> {
        Zone::in_country(country).first().copied()
    }

    /// Every zone of `country`, its primary zone first; empty for an
    /// unknown code.
    pub fn in_country(country: &str) -> Vec<Zone> {
        country_zones(country)
            .iter()
            .filter_map(|name| Zone::parse(name).ok())
            .collect()
    }

    /// The ISO 3166-1 alpha-2 codes that have zones, sorted.
    pub fn countries_with_zones() -> impl Iterator<Item = &'static str> {
        COUNTRY_ZONES.iter().map(|(country, _)| *country)
    }

    /// The countries that use this zone (`["IN"]` for `Asia/Kolkata`);
    /// empty for zones such as `UTC`.
    pub fn countries(&self) -> Vec<&'static str> {
        COUNTRIES_OF.get(self.name()).cloned().unwrap_or_default()
    }

    /// `at` on this zone's clock.
    pub fn local(&self, at: DateTime<Utc>) -> DateTime<Tz> {
        at.with_timezone(&self.0)
    }

    /// The UTC offset in force at `at`.
    pub fn offset_at(&self, at: DateTime<Utc>) -> FixedOffset {
        self.0.offset_from_utc_datetime(&at.naive_utc()).fix()
    }

    /// The UTC offset in force at `at`, written `+05:30` or `-04:00`.
    pub fn offset_label(&self, at: DateTime<Utc>) -> String {
        offset_label(self.offset_at(at).local_minus_utc())
    }

    /// The instant at which this zone's clocks show `local`.
    ///
    /// A time that happens twice (when clocks go back) is the earlier one;
    /// a time that is skipped (when clocks go forward) moves forward by the
    /// length of the gap, so 02:30 on the night clocks jump from 02:00 to
    /// 03:00 is 03:30.
    pub fn to_utc(&self, local: NaiveDateTime) -> DateTime<Utc> {
        match self.0.from_local_datetime(&local) {
            LocalResult::Single(at) => at.with_timezone(&Utc),
            LocalResult::Ambiguous(earlier, _) => earlier.with_timezone(&Utc),
            LocalResult::None => {
                // The offset in force before the gap, applied to the wall
                // clock time, lands as far after the gap as the time was
                // into it.
                let before = (1..=48)
                    .find_map(|hours| {
                        match self
                            .0
                            .offset_from_local_datetime(&(local - TimeDelta::hours(hours)))
                        {
                            LocalResult::Single(offset) => Some(offset.fix()),
                            LocalResult::Ambiguous(_, later) => Some(later.fix()),
                            LocalResult::None => None,
                        }
                    })
                    .unwrap_or_else(|| FixedOffset::east_opt(0).expect("zero offset"));
                let utc = local - TimeDelta::seconds(i64::from(before.local_minus_utc()));
                Utc.from_utc_datetime(&utc)
            }
        }
    }

    /// The local day `now` falls on in this zone.
    pub fn today(&self, now: DateTime<Utc>) -> LocalDay {
        LocalDay::of(now, self)
    }
}

/// `+05:30` / `-04:00` for an offset in seconds east of UTC.
pub(crate) fn offset_label(seconds: i32) -> String {
    let sign = if seconds < 0 { '-' } else { '+' };
    let minutes = seconds.unsigned_abs() / 60;
    format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

/// The zone names of `country` (any case).
fn country_zones(country: &str) -> &'static [&'static str] {
    let upper = country.trim().to_ascii_uppercase();
    COUNTRY_ZONES
        .binary_search_by(|(code, _)| (*code).cmp(upper.as_str()))
        .map(|index| COUNTRY_ZONES[index].1)
        .unwrap_or(&[])
}

/// Whether `country` (any case) is a country with zones.
pub(crate) fn is_country(country: &str) -> bool {
    !country_zones(country).is_empty()
}

fn lookup(name: &str) -> Option<Zone> {
    let lower = name.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }
    ALIAS_TARGETS
        .get(&lower)
        .or_else(|| BY_LOWER_NAME.get(&lower))
        .map(|tz| Zone(*tz))
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
    fn every_alias_target_is_a_canonical_zone() {
        for (alias, target) in ALIASES {
            let zone = Zone::parse(alias).unwrap_or_else(|e| panic!("{alias}: {e}"));
            assert_eq!(zone.name(), *target, "{alias}");
            assert!(
                !ALIAS_TARGETS.contains_key(&target.to_ascii_lowercase()),
                "{alias} -> {target} is itself an alias"
            );
        }
    }

    #[test]
    fn every_country_zone_is_canonical_and_countries_are_sorted() {
        assert!(COUNTRY_ZONES.windows(2).all(|w| w[0].0 < w[1].0));
        for (country, zones) in COUNTRY_ZONES {
            for zone in *zones {
                assert_eq!(Zone::canonical_name(zone), Some(*zone), "{country}: {zone}");
            }
        }
    }

    #[test]
    fn factory_is_not_a_zone() {
        assert!(Zone::parse("Factory").is_err());
        assert!(Zone::all().all(|zone| zone.name() != "Factory"));
    }
}
