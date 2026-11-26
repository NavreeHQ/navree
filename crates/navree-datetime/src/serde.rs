//! `Serialize` and `Deserialize` for every date, time, zone and span type,
//! keeping the wire forms Navree has written since 1.0.

use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use ::serde::de::{self, Visitor};
use ::serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    Date, DateTime, Month, Offset, Precision, Span, Time, Timestamp, Weekday, Zone, Zoned,
};

struct FromStrVisitor<T> {
    expecting: &'static str,
    marker: PhantomData<fn() -> T>,
}

impl<T> FromStrVisitor<T> {
    const fn new(expecting: &'static str) -> Self {
        Self {
            expecting,
            marker: PhantomData,
        }
    }
}

impl<'de, T> Visitor<'de> for FromStrVisitor<T>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.expecting)
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<Self::Value, E> {
        text.parse().map_err(de::Error::custom)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_rfc3339_with(Precision::Auto, true))
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(FromStrVisitor::new("an RFC 3339 timestamp"))
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl Serialize for Zoned {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_rfc3339_with(Precision::Auto, true))
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl<'de> Deserialize<'de> for Zoned {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(FromStrVisitor::new(
            "an RFC 3339 date and time with an offset",
        ))
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl Serialize for Zone {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl<'de> Deserialize<'de> for Zone {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(FromStrVisitor::new("an IANA time zone name"))
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl Serialize for Date {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(FromStrVisitor::new("a calendar date (YYYY-MM-DD)"))
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl Serialize for Time {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl<'de> Deserialize<'de> for Time {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(FromStrVisitor::new("a time of day (HH:MM:SS)"))
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl Serialize for DateTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("{self:?}"))
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl<'de> Deserialize<'de> for DateTime {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(FromStrVisitor::new("a date and time (YYYY-MM-DDTHH:MM:SS)"))
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl Serialize for Offset {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl<'de> Deserialize<'de> for Offset {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(FromStrVisitor::new("a UTC offset (+HH:MM)"))
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl Serialize for Span {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (self.secs_floor(), self.nanos_above() as i32).serialize(serializer)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl<'de> Deserialize<'de> for Span {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (secs, nanos) = <(i64, i32)>::deserialize(deserializer)?;
        u32::try_from(nanos)
            .ok()
            .and_then(|nanos| Span::from_parts(secs, nanos))
            .ok_or_else(|| de::Error::custom("span out of range"))
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl Serialize for Weekday {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl<'de> Deserialize<'de> for Weekday {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(FromStrVisitor::new("a day of the week"))
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl Serialize for Month {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl<'de> Deserialize<'de> for Month {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(FromStrVisitor::new("a month of the year"))
    }
}
