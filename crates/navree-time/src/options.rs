//! [`TimeOptions`]: the `time` configuration section.

use std::fmt;

use navree_config::ConfigSection;
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};

use crate::{Holidays, LocalDay, Zone};

/// The settings of `TimeModule`: the `time` configuration section. Every
/// key is optional.
///
/// ```toml
/// [time]
/// default_zone = "UTC"          # the zone when no user, workspace or organization has one
/// routes = true                 # GET /api/time/zones (public reference data)
///
/// [time.holidays]
/// country = "GB"                # the app's public holidays (Zones::holidays)
/// days = ["2026-12-24"]         # extra closing days; or "2026-12-24,2026-12-31"
/// ```
///
/// From the environment: `APP_TIME__DEFAULT_ZONE=Asia/Kolkata`,
/// `APP_TIME__HOLIDAYS__COUNTRY=IN`,
/// `APP_TIME__HOLIDAYS__DAYS=2026-12-24,2026-12-31`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimeOptions {
    /// The app's zone: the zone of requests and owners no source gives one
    /// (default `UTC`). Legacy names are accepted and normalized
    /// (`Asia/Calcutta` is `Asia/Kolkata`).
    pub default_zone: String,
    /// Mount `GET {api_prefix}/time/zones` (default `true`).
    pub routes: bool,
    /// The app's holidays.
    pub holidays: HolidayOptions,
}

/// The `time.holidays` section.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HolidayOptions {
    /// The country whose public holidays the app observes, an ISO 3166-1
    /// alpha-2 code (any case); none when unset.
    pub country: Option<String>,
    /// Extra closing days (`yyyy-MM-dd`): a list, or a comma-separated
    /// string from an environment variable.
    #[serde(deserialize_with = "days_list_or_csv")]
    pub days: Vec<LocalDay>,
}

impl Default for TimeOptions {
    fn default() -> Self {
        TimeOptions {
            default_zone: "UTC".to_owned(),
            routes: true,
            holidays: HolidayOptions::default(),
        }
    }
}

impl TimeOptions {
    /// The default zone (UTC when `default_zone` is not a zone; the
    /// module refuses to boot with such a value).
    pub fn zone(&self) -> Zone {
        Zone::parse(&self.default_zone).unwrap_or(Zone::UTC)
    }

    /// The app's holidays: `holidays.country`'s public holidays plus
    /// `holidays.days`.
    pub fn holidays(&self) -> Holidays {
        let base = match self.holidays.country() {
            Some(country) => Holidays::for_country(country),
            None => Holidays::none(),
        };
        base.with_days(self.holidays.days.iter().copied())
    }

    /// Checks the values; the message names the key.
    pub fn check(&self) -> Result<(), String> {
        if let Err(error) = Zone::parse(&self.default_zone) {
            return Err(format!(
                "time.default_zone: {error}; use an IANA name such as `Europe/Paris`"
            ));
        }
        if let Some(country) = self.holidays.country()
            && !crate::zone::is_country(country)
        {
            return Err(format!(
                "time.holidays.country: `{country}` is not an ISO 3166-1 alpha-2 country code"
            ));
        }
        Ok(())
    }
}

impl HolidayOptions {
    /// The country, trimmed; `None` when unset or empty.
    pub fn country(&self) -> Option<&str> {
        self.country
            .as_deref()
            .map(str::trim)
            .filter(|country| !country.is_empty())
    }
}

impl ConfigSection for TimeOptions {
    const KEY: &'static str = "time";

    fn validate(&self) -> Result<(), String> {
        self.check()
    }
}

/// A list of days, or one comma-separated string.
fn days_list_or_csv<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<LocalDay>, D::Error> {
    struct Days;

    impl<'de> Visitor<'de> for Days {
        type Value = Vec<LocalDay>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a list of yyyy-MM-dd days or a comma-separated string")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            value
                .split(',')
                .map(str::trim)
                .filter(|day| !day.is_empty())
                .map(|day| LocalDay::parse(day).map_err(E::custom))
                .collect()
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut days = Vec::new();
            while let Some(day) = seq.next_element::<LocalDay>()? {
                days.push(day);
            }
            Ok(days)
        }
    }

    deserializer.deserialize_any(Days)
}
