//! SQLite `Type`, `Encode` and `Decode` implementations.

use std::sync::LazyLock;

use ::sqlx::TypeInfo as _;
use ::sqlx::ValueRef as _;
use ::sqlx::decode::Decode;
use ::sqlx::encode::{Encode, IsNull};
use ::sqlx::error::BoxDynError;
use ::sqlx::sqlite::{Sqlite, SqliteArgumentsBuffer, SqliteTypeInfo, SqliteValueRef};
use ::sqlx::types::Type;

use crate::{Date, DateTime, Offset, Precision, Time, Timestamp, Zone, Zoned};

static DATETIME_TYPE_INFO: LazyLock<SqliteTypeInfo> =
    LazyLock::new(|| sqlite_type_info("Datetime"));
static DATE_TYPE_INFO: LazyLock<SqliteTypeInfo> = LazyLock::new(|| sqlite_type_info("Date"));
static TIME_TYPE_INFO: LazyLock<SqliteTypeInfo> = LazyLock::new(|| sqlite_type_info("Time"));

fn sqlite_type_info(data_type: &str) -> SqliteTypeInfo {
    let json = format!(r#""{data_type}""#);
    serde_json::from_str(&json).expect("valid SqliteTypeInfo descriptor")
}

const SQLITE_DATETIME_FORMATS: &[&str] = &[
    "%F %T%.f",
    "%F %R",
    "%F %RZ",
    "%F %R%:z",
    "%F %T%.fZ",
    "%F %T%.f%:z",
    "%FT%R",
    "%FT%RZ",
    "%FT%R%:z",
    "%FT%T%.f",
    "%FT%T%.fZ",
    "%FT%T%.f%:z",
];

const SQLITE_TIME_FORMATS: &[&str] = &["%T.f", "%T%.f", "%R", "%RZ", "%T%.fZ", "%R%:z", "%T%.f%:z"];

#[doc(hidden)]
pub fn decode_zoned_text(value: &str) -> Result<Zoned, BoxDynError> {
    if let Ok(zoned) = Zoned::parse_rfc3339(value) {
        return Ok(zoned);
    }

    for format in SQLITE_DATETIME_FORMATS {
        if let Ok(zoned) = Zoned::parse(format, value) {
            return Ok(zoned);
        }
        if let Ok(dt) = DateTime::parse(format, value) {
            return Ok(dt.in_utc().in_zone(Zone::fixed(Offset::UTC)));
        }
    }

    if let Ok(zoned) = value.parse::<Zoned>() {
        return Ok(zoned);
    }

    Err(format!("invalid datetime: {value}").into())
}

#[doc(hidden)]
pub fn decode_timestamp_text(value: &str) -> Result<Timestamp, BoxDynError> {
    decode_zoned_text(value).map(Zoned::timestamp)
}

#[doc(hidden)]
pub fn decode_date_time_text(value: &str) -> Result<DateTime, BoxDynError> {
    decode_zoned_text(value).map(Zoned::date_time)
}

#[doc(hidden)]
pub fn decode_date_text(value: &str) -> Result<Date, BoxDynError> {
    Ok(Date::parse("%F", value)?)
}

#[doc(hidden)]
pub fn decode_time_text(value: &str) -> Result<Time, BoxDynError> {
    for format in SQLITE_TIME_FORMATS {
        if let Ok(time) = Time::parse(format, value) {
            return Ok(time);
        }
    }
    Err(format!("invalid time: {value}").into())
}

fn decode_zoned_from_int(secs: i64) -> Result<Zoned, BoxDynError> {
    Ok(Timestamp::from_secs(secs)?.in_zone(Zone::fixed(Offset::UTC)))
}

fn decode_zoned_from_float(julian_days: f64) -> Result<Zoned, BoxDynError> {
    const EPOCH_IN_JULIAN_DAYS: f64 = 2_440_587.5;
    const SECONDS_IN_DAY: f64 = 86_400.0;

    let timestamp = (julian_days - EPOCH_IN_JULIAN_DAYS) * SECONDS_IN_DAY;
    if !timestamp.is_finite() || timestamp < i64::MIN as f64 || timestamp >= i64::MAX as f64 {
        return Err(format!("invalid Julian datetime: {julian_days}").into());
    }

    let whole = timestamp.floor();
    let secs = whole as i64;
    let nanos = ((timestamp - whole) * 1e9) as u32;
    let nanos = nanos.min(999_999_999);
    Ok(Timestamp::from_secs_nanos(secs, nanos)?.in_zone(Zone::fixed(Offset::UTC)))
}

fn decode_sqlite_zoned<'r>(value: SqliteValueRef<'r>) -> Result<Zoned, BoxDynError> {
    match value.type_info().name() {
        "INTEGER" => decode_zoned_from_int(<i64 as Decode<'r, Sqlite>>::decode(value)?),
        "REAL" => decode_zoned_from_float(<f64 as Decode<'r, Sqlite>>::decode(value)?),
        _ => decode_zoned_text(<&'r str as Decode<'r, Sqlite>>::decode(value)?),
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl Type<Sqlite> for Timestamp {
    fn type_info() -> SqliteTypeInfo {
        DATETIME_TYPE_INFO.clone()
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        matches!(ty.name(), "DATETIME" | "TEXT" | "INTEGER" | "REAL")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl Encode<'_, Sqlite> for Timestamp {
    fn encode_by_ref(&self, buf: &mut SqliteArgumentsBuffer) -> Result<IsNull, BoxDynError> {
        Encode::<Sqlite>::encode(self.to_rfc3339_with(Precision::Auto, false), buf)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl<'r> Decode<'r, Sqlite> for Timestamp {
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        Ok(decode_sqlite_zoned(value)?.timestamp())
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl Type<Sqlite> for Zoned {
    fn type_info() -> SqliteTypeInfo {
        DATETIME_TYPE_INFO.clone()
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        matches!(ty.name(), "DATETIME" | "TEXT" | "INTEGER" | "REAL")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl Encode<'_, Sqlite> for Zoned {
    fn encode_by_ref(&self, buf: &mut SqliteArgumentsBuffer) -> Result<IsNull, BoxDynError> {
        Encode::<Sqlite>::encode(self.to_rfc3339_with(Precision::Auto, false), buf)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl<'r> Decode<'r, Sqlite> for Zoned {
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        decode_sqlite_zoned(value)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl Type<Sqlite> for DateTime {
    fn type_info() -> SqliteTypeInfo {
        DATETIME_TYPE_INFO.clone()
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        matches!(ty.name(), "DATETIME" | "TEXT" | "INTEGER" | "REAL")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl Encode<'_, Sqlite> for DateTime {
    fn encode_by_ref(&self, buf: &mut SqliteArgumentsBuffer) -> Result<IsNull, BoxDynError> {
        Encode::<Sqlite>::encode(self.format("%F %T%.f").to_string(), buf)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl<'r> Decode<'r, Sqlite> for DateTime {
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        Ok(decode_sqlite_zoned(value)?.date_time())
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl Type<Sqlite> for Date {
    fn type_info() -> SqliteTypeInfo {
        DATE_TYPE_INFO.clone()
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        matches!(ty.name(), "DATE" | "TEXT")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl Encode<'_, Sqlite> for Date {
    fn encode_by_ref(&self, buf: &mut SqliteArgumentsBuffer) -> Result<IsNull, BoxDynError> {
        Encode::<Sqlite>::encode(self.format("%F").to_string(), buf)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl<'r> Decode<'r, Sqlite> for Date {
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        decode_date_text(<&'r str as Decode<'r, Sqlite>>::decode(value)?)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl Type<Sqlite> for Time {
    fn type_info() -> SqliteTypeInfo {
        TIME_TYPE_INFO.clone()
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        matches!(ty.name(), "TIME" | "TEXT")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl Encode<'_, Sqlite> for Time {
    fn encode_by_ref(&self, buf: &mut SqliteArgumentsBuffer) -> Result<IsNull, BoxDynError> {
        Encode::<Sqlite>::encode(self.format("%T%.f").to_string(), buf)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-sqlite")))]
impl<'r> Decode<'r, Sqlite> for Time {
    fn decode(value: SqliteValueRef<'r>) -> Result<Self, BoxDynError> {
        decode_time_text(<&'r str as Decode<'r, Sqlite>>::decode(value)?)
    }
}
