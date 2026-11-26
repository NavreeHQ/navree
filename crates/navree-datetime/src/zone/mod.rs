//! [`Zone`]: a time zone, and [`Disambiguation`]: which instant a time read
//! off a zone's clocks is, where the clocks skipped it or showed it twice.

use std::collections::HashMap;
use std::fmt;
use std::sync::LazyLock;

use crate::{Date, DateTime, Error, ErrorKind, Offset, Result, Timestamp};

mod rules;
mod tables;
mod zoned;

pub use rules::{Entry, LocalPeriods, Period, PeriodSpan};
pub use zoned::Zoned;

use tables::{ALIASES, COUNTRY_ZONES, TZDATA_VERSION};

const SECS_PER_DAY: i64 = 86_400;

/// Names in the tz database that are not places: never a zone.
const NOT_ZONES: &[&str] = &["Factory"];

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

/// A time zone: what says which offset from UTC the clocks of a place keep
/// at each instant.
///
/// A zone is one of two things:
///
/// - A zone of the IANA time zone database, by name: [`Zone::named`], and
///   [`Zone::UTC`]. Its offset changes over the years and, where the place
///   keeps daylight saving time, twice a year. The database is compiled into
///   this crate ([`Zone::tzdata_version`]); no file of the system is read.
/// - One offset that never changes: [`Zone::fixed`].
///
/// [`Zone::named`] trims the name, matches it without regard to case and
/// replaces a legacy name with the current one (`Asia/Calcutta` is
/// `Asia/Kolkata`, `US/Eastern` is `America/New_York`, `Etc/UTC` and `GMT`
/// are `UTC`), so a zone always has the name a database server knows it by.
/// Names that are still country locations (`Europe/Oslo`, `Africa/Accra`)
/// are kept as they are.
///
/// A zone is a small `Copy` handle. Two named zones are equal when they
/// have the same name, and two fixed ones when they have the same offset; a
/// fixed zone is never equal to a named one, so `Zone::fixed(Offset::UTC)`
/// is not [`Zone::UTC`].
///
/// ```
/// use navree_datetime::{Offset, Timestamp, Zone};
///
/// # fn main() -> navree_datetime::Result<()> {
/// let kolkata = Zone::named("Asia/Calcutta")?;
/// assert_eq!(kolkata.name(), "Asia/Kolkata");
/// assert_eq!(Zone::for_country("IN"), Some(kolkata));
///
/// let noon = Timestamp::utc(2026, 1, 15, 12, 0, 0)?;
/// assert_eq!(kolkata.offset_at(noon), Offset::east(19_800)?);
/// assert_eq!(kolkata.abbreviation_at(noon), "IST");
///
/// // A zone that keeps daylight saving time has two offsets a year.
/// let new_york = Zone::named("America/New_York")?;
/// let july = Timestamp::utc(2026, 7, 15, 12, 0, 0)?;
/// assert_eq!(new_york.offset_at(noon), Offset::west(5 * 3_600)?);
/// assert_eq!(new_york.offset_at(july), Offset::west(4 * 3_600)?);
///
/// assert!(Zone::named("Mars/Olympus_Mons").is_err());
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Zone(Kind);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    /// A zone of the database, under its current name.
    Named(Entry),
    /// One offset at every instant.
    Fixed(Offset),
}

/// Which instant a time read off a zone's clocks is, where it is not
/// exactly one.
///
/// Where a zone moves its clocks forward, the times it jumps over are never
/// shown: a gap. Where it moves them back, the times it goes back over are
/// shown twice: a fold. Every other time is one instant, whatever is asked
/// for here.
///
/// | | In a fold (shown twice) | In a gap (never shown) |
/// |---|---|---|
/// | [`Compatible`](Disambiguation::Compatible) | the earlier instant | forward by the length of the gap |
/// | [`Earlier`](Disambiguation::Earlier) | the earlier instant | back by the length of the gap |
/// | [`Later`](Disambiguation::Later) | the later instant | forward by the length of the gap |
/// | [`Reject`](Disambiguation::Reject) | an error | an error |
///
/// ```
/// use navree_datetime::{Date, Disambiguation, ErrorKind, Time, Timestamp, Zone};
///
/// # fn main() -> navree_datetime::Result<()> {
/// let new_york = Zone::named("America/New_York")?;
///
/// // 8 March 2026: the clocks jump from 02:00 to 03:00, and 02:30 is not shown.
/// let skipped = Date::new(2026, 3, 8)?.at(Time::new(2, 30, 0)?);
/// assert_eq!(
///     new_york.to_timestamp(skipped, Disambiguation::Compatible)?,
///     Timestamp::utc(2026, 3, 8, 7, 30, 0)?, // 03:30, an hour on
/// );
/// assert_eq!(
///     new_york.to_timestamp(skipped, Disambiguation::Earlier)?,
///     Timestamp::utc(2026, 3, 8, 6, 30, 0)?, // 01:30, an hour back
/// );
/// let refused = new_york.to_timestamp(skipped, Disambiguation::Reject);
/// assert_eq!(refused.unwrap_err().kind(), ErrorKind::Gap);
///
/// // 1 November 2026: the clocks go back from 02:00 to 01:00, and 01:30 is shown twice.
/// let repeated = Date::new(2026, 11, 1)?.at(Time::new(1, 30, 0)?);
/// assert_eq!(
///     new_york.to_timestamp(repeated, Disambiguation::Compatible)?,
///     Timestamp::utc(2026, 11, 1, 5, 30, 0)?, // the first 01:30
/// );
/// assert_eq!(
///     new_york.to_timestamp(repeated, Disambiguation::Later)?,
///     Timestamp::utc(2026, 11, 1, 6, 30, 0)?, // the second
/// );
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Disambiguation {
    /// The rule Navree's zones have read a local time by since 1.0, so the
    /// start of a local day and the opening of a send window are the
    /// instants they always were.
    ///
    /// A time shown twice is the earlier of its two instants.
    ///
    /// A time never shown is read with the offset in force before the gap,
    /// which moves it forward by the length of the gap: on the night the
    /// clocks jump from 02:00 to 03:00, 02:30 is 03:30. That offset is
    /// looked for on the wall clock: the time is stepped back an hour at a
    /// time, for up to 48 hours, to the first time the clocks show, and
    /// where they show that one twice the later of its offsets is taken. A
    /// zone that shows none of the 48 is read as UTC.
    #[default]
    Compatible,
    /// A time shown twice is the earlier of its two instants. A time never
    /// shown is read with the offset in force after the gap, which moves it
    /// back by the length of the gap: 02:30 is 01:30.
    Earlier,
    /// A time shown twice is the later of its two instants. A time never
    /// shown moves forward by the length of the gap, as with
    /// [`Compatible`](Disambiguation::Compatible): 02:30 is 03:30.
    Later,
    /// A time shown twice is an error of the kind
    /// [`Ambiguous`](ErrorKind::Ambiguous), and a time never shown an error
    /// of the kind [`Gap`](ErrorKind::Gap).
    Reject,
}

impl Zone {
    /// Coordinated Universal Time: the zone of the database called `UTC`.
    pub const UTC: Zone = Zone(Kind::Named(Entry::UTC));

    /// The zone that keeps `offset` at every instant. It has no name and no
    /// abbreviation.
    pub const fn fixed(offset: Offset) -> Zone {
        Zone(Kind::Fixed(offset))
    }

    /// The zone of the IANA database called `name`. The name is trimmed and
    /// matched in any case, and a legacy name is replaced by the current
    /// one, so `Zone::named(" asia/calcutta ")` is `Asia/Kolkata`.
    ///
    /// # Errors
    ///
    /// A name the database does not have is invalid. So is `Factory`, the
    /// database's stand-in for a clock nobody has set.
    pub fn named(name: &str) -> Result<Zone> {
        match lookup(name) {
            Some(entry) => Ok(Zone(Kind::Named(entry))),
            None => Err(Error::invalid(format!("unknown time zone `{name}`"))),
        }
    }

    /// The current name of the zone called `name`, or `None` when `name` is
    /// no zone: `Zone::canonical_name("asia/calcutta")` is
    /// `Some("Asia/Kolkata")`.
    pub fn canonical_name(name: &str) -> Option<&'static str> {
        lookup(name).map(Entry::name)
    }

    /// The zone's name in the IANA database (`Asia/Kolkata`, `UTC`). A
    /// [fixed](Zone::fixed) zone is not in the database and has no name: the
    /// answer is empty.
    pub const fn name(self) -> &'static str {
        match self.0 {
            Kind::Named(entry) => entry.name(),
            Kind::Fixed(_) => "",
        }
    }

    /// Whether the zone is one offset that never changes
    /// ([`Zone::fixed`]), not a zone of the database. [`Zone::UTC`] is a
    /// zone of the database.
    pub const fn is_fixed(self) -> bool {
        matches!(self.0, Kind::Fixed(_))
    }

    /// Every zone of the database under its current name, sorted by name:
    /// no legacy name and no fixed zone is among them.
    pub fn all() -> impl Iterator<Item = Zone> {
        Entry::all()
            .filter(|entry| {
                !NOT_ZONES.contains(&entry.name()) && alias_target(entry.name()).is_none()
            })
            .map(|entry| Zone(Kind::Named(entry)))
    }

    /// The primary zone of `country`, an ISO 3166-1 alpha-2 code in any
    /// case (`IN` gives `Asia/Kolkata`, `US` gives `America/New_York`);
    /// `None` for an unknown code.
    pub fn for_country(country: &str) -> Option<Zone> {
        country_zones(country)
            .iter()
            .find_map(|name| Zone::named(name).ok())
    }

    /// Every zone of `country`, its primary zone first; empty for an
    /// unknown code.
    pub fn in_country(country: &str) -> Vec<Zone> {
        country_zones(country)
            .iter()
            .filter_map(|name| Zone::named(name).ok())
            .collect()
    }

    /// The ISO 3166-1 alpha-2 codes that have zones, sorted.
    pub fn countries_with_zones() -> impl Iterator<Item = &'static str> {
        COUNTRY_ZONES.iter().map(|(country, _)| *country)
    }

    /// The countries that use this zone (`["IN"]` for `Asia/Kolkata`);
    /// empty for a zone such as `UTC` and for a fixed one.
    pub fn countries(self) -> Vec<&'static str> {
        COUNTRIES_OF.get(self.name()).cloned().unwrap_or_default()
    }

    /// The offset from UTC the zone keeps at the instant `at`.
    pub fn offset_at(self, at: Timestamp) -> Offset {
        match self.0 {
            Kind::Named(entry) => {
                Offset::from_seconds_unchecked(entry.span_at(at.as_secs()).period.offset())
            }
            Kind::Fixed(offset) => offset,
        }
    }

    /// The zone's abbreviation at the instant `at` (`IST`, `EST`, `EDT`,
    /// `UTC`). A zone the database gives no letters has its offset there
    /// (`+0530`, `-03`). A [fixed](Zone::fixed) zone has no abbreviation:
    /// the answer is empty, and what there is to show is its offset.
    pub fn abbreviation_at(self, at: Timestamp) -> &'static str {
        match self.0 {
            Kind::Named(entry) => entry.span_at(at.as_secs()).period.abbreviation(),
            Kind::Fixed(_) => "",
        }
    }

    /// The instant at which the zone's clocks show `local`.
    ///
    /// Nearly every time is exactly one instant. `how` says what to make
    /// of the others: a time the clocks skip, and a time they show twice
    /// ([`Disambiguation`]). In a fixed zone there are no others.
    ///
    /// # Errors
    ///
    /// With [`Disambiguation::Reject`], a time the clocks skip is an error
    /// of the kind [`ErrorKind::Gap`] and a time they show twice one of the
    /// kind [`ErrorKind::Ambiguous`]. With any `how`, an instant before
    /// [`Timestamp::MIN`] or after [`Timestamp::MAX`] is out of range.
    pub fn to_timestamp(self, local: DateTime, how: Disambiguation) -> Result<Timestamp> {
        let wall = wall_second(local);
        let offset = match self.0 {
            Kind::Fixed(offset) => offset.seconds(),
            Kind::Named(entry) => match entry.offset_of_local(wall, how) {
                Ok(offset) => offset,
                Err(ErrorKind::Ambiguous) => {
                    return Err(Error::new(
                        ErrorKind::Ambiguous,
                        format!(
                            "{} comes twice in {}: the clocks go back over it",
                            wall_text(local),
                            entry.name()
                        ),
                    ));
                }
                Err(kind) => {
                    return Err(Error::new(
                        kind,
                        format!(
                            "{} does not exist in {}: the clocks go forward over it",
                            wall_text(local),
                            entry.name()
                        ),
                    ));
                }
            },
        };
        Timestamp::from_secs_nanos(wall - i64::from(offset), local.time().nanosecond())
    }

    /// The day the zone's clocks show now, by the system's clock.
    pub fn today(self) -> Date {
        Timestamp::now().in_zone(self).date()
    }

    /// The release of the IANA time zone database compiled into this build
    /// (`2025b`). Zones, offsets, legacy names and daylight saving rules all
    /// come from it.
    pub const fn tzdata_version() -> &'static str {
        TZDATA_VERSION
    }

    /// The second, from the epoch, at which the zone's clocks show the
    /// wall-clock second `wall`, by [`Disambiguation::Compatible`].
    pub(crate) fn second_of_wall(self, wall: i64) -> i64 {
        let offset = match self.0 {
            Kind::Fixed(offset) => offset.seconds(),
            // `Compatible` takes every time.
            Kind::Named(entry) => entry
                .offset_of_local(wall, Disambiguation::Compatible)
                .unwrap_or(0),
        };
        wall.saturating_sub(i64::from(offset))
    }
}

impl Entry {
    /// The entry of a zone of the database; `None` for a fixed zone.
    pub fn of(zone: Zone) -> Option<Entry> {
        match zone.0 {
            Kind::Named(entry) => Some(entry),
            Kind::Fixed(_) => None,
        }
    }
}

/// [`Zone::UTC`].
impl Default for Zone {
    fn default() -> Zone {
        Zone::UTC
    }
}

/// A named zone as `Zone("Asia/Kolkata")`, a fixed one by its offset.
impl fmt::Debug for Zone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut zone = f.debug_tuple("Zone");
        match &self.0 {
            Kind::Named(entry) => zone.field(&entry.name()),
            Kind::Fixed(offset) => zone.field(offset),
        };
        zone.finish()
    }
}

/// A day and a time on a wall clock as seconds, counted as though the
/// clock were UTC's.
pub(crate) fn wall_second(local: DateTime) -> i64 {
    local.date().epoch_days() * SECS_PER_DAY + i64::from(local.time().second_of_day())
}

/// A day and a time on a wall clock, to the second, for a message.
fn wall_text(local: DateTime) -> String {
    let (date, time) = (local.date(), local.time());
    format!(
        "{}-{:02}-{:02} {:02}:{:02}:{:02}",
        date.year(),
        date.month_number(),
        date.day(),
        time.hour(),
        time.minute(),
        time.second()
    )
}

/// The zone names of `country` (any case).
fn country_zones(country: &str) -> &'static [&'static str] {
    let code = country.trim();
    COUNTRY_ZONES
        .binary_search_by(|(known, _)| {
            known
                .bytes()
                .cmp(code.bytes().map(|byte| byte.to_ascii_uppercase()))
        })
        .map(|index| COUNTRY_ZONES[index].1)
        .unwrap_or(&[])
}

/// The zone a legacy name stands for, where `name` (in any case) is one.
fn alias_target(name: &str) -> Option<Entry> {
    let at = ALIASES
        .binary_search_by(|(alias, _)| rules::cmp_ignoring_ascii_case(alias, name))
        .ok()?;
    Entry::find(ALIASES[at].1)
}

/// The zone called `name`: trimmed, in any case, a legacy name replaced by
/// the current one.
fn lookup(name: &str) -> Option<Entry> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    if let Some(target) = alias_target(name) {
        return Some(target);
    }
    let entry = Entry::find(name)?;
    (!NOT_ZONES.contains(&entry.name())).then_some(entry)
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::tables::{ENTRIES, PERIODS, TRANSITIONS, ZONES};
    use super::*;

    #[test]
    fn a_zone_is_small_and_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<Zone>();
        assert_copy::<Zoned>();
        assert_copy::<Entry>();
        assert_copy::<Period>();
        assert!(std::mem::size_of::<Zone>() <= 8);
        assert!(std::mem::size_of::<Entry>() <= 2);
        assert!(std::mem::size_of::<Period>() <= 2);
    }

    #[test]
    fn the_tables_name_the_tz_database_release_they_were_made_from() {
        assert_eq!(TZDATA_VERSION, Zone::tzdata_version());
        assert!(TZDATA_VERSION.len() >= 5, "{TZDATA_VERSION}");
        assert!(
            TZDATA_VERSION.as_bytes()[..4]
                .iter()
                .all(u8::is_ascii_digit),
            "{TZDATA_VERSION}"
        );
        assert!(
            TZDATA_VERSION.as_bytes()[4..]
                .iter()
                .all(u8::is_ascii_lowercase),
            "{TZDATA_VERSION}"
        );
    }

    #[test]
    fn every_offset_of_the_tables_is_one_an_offset_holds() {
        // `Zone::offset_at` makes an `Offset` from a period without a check.
        for (index, period) in PERIODS.iter().enumerate() {
            let seconds = period.std_offset + period.dst_offset;
            assert_eq!(
                Offset::east(seconds).map(Offset::seconds).ok(),
                Some(seconds),
                "period {index}"
            );
        }
    }

    #[test]
    fn every_zone_of_the_tables_has_its_changes_in_order() {
        for (index, zone) in ZONES.iter().enumerate() {
            let transitions = &TRANSITIONS[zone.start as usize..zone.end as usize];
            assert!(
                transitions.windows(2).all(|pair| pair[0].0 < pair[1].0),
                "zone {index}"
            );
            assert!(usize::from(zone.initial) < PERIODS.len(), "zone {index}");
        }
        for (name, zone) in ENTRIES {
            assert!(usize::from(*zone) < ZONES.len(), "{name}");
        }
    }

    #[test]
    fn the_alias_table_is_in_the_order_it_is_searched_in() {
        assert!(ALIASES.windows(2).all(|pair| {
            rules::cmp_ignoring_ascii_case(pair[0].0, pair[1].0) == Ordering::Less
        }));
    }

    #[test]
    fn every_alias_stands_for_a_current_zone() {
        for (alias, target) in ALIASES {
            let zone = Zone::named(alias).unwrap_or_else(|error| panic!("{alias}: {error}"));
            assert_eq!(zone.name(), *target, "{alias}");
            assert_eq!(
                Zone::named(&alias.to_ascii_lowercase()).ok(),
                Some(zone),
                "{alias}"
            );
            assert!(
                alias_target(target).is_none(),
                "{alias} -> {target} is itself an alias"
            );
        }
    }

    #[test]
    fn every_country_zone_is_current_and_countries_are_sorted() {
        assert!(COUNTRY_ZONES.windows(2).all(|pair| pair[0].0 < pair[1].0));
        for (country, zones) in COUNTRY_ZONES {
            assert!(
                country.bytes().all(|byte| byte.is_ascii_uppercase()),
                "{country}"
            );
            for zone in *zones {
                assert_eq!(Zone::canonical_name(zone), Some(*zone), "{country}: {zone}");
            }
        }
    }

    #[test]
    fn factory_is_not_a_zone() {
        assert!(Entry::find("Factory").is_some());
        assert!(Zone::named("Factory").is_err());
        assert!(Zone::all().all(|zone| zone.name() != "Factory"));
    }
}
