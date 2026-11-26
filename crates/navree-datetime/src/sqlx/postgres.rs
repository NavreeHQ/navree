//! PostgreSQL `Type`, `PgHasArrayType`, `Encode` and `Decode` implementations.

use std::mem;

use ::sqlx::decode::Decode;
use ::sqlx::encode::{Encode, IsNull};
use ::sqlx::error::BoxDynError;
use ::sqlx::postgres::{
    PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueFormat, PgValueRef, Postgres,
};
use ::sqlx::types::Type;

use crate::{Date, DateTime, Offset, Time, Timestamp, Zone, Zoned};

/// Days from 1970-01-01 (`epoch_days == 0`) to PostgreSQL's epoch, 2000-01-01.
const PG_EPOCH_DAYS: i64 = 10_957;
/// Seconds from 1970-01-01T00:00:00Z to 2000-01-01T00:00:00Z.
const PG_EPOCH_SECS: i64 = PG_EPOCH_DAYS * 86_400;
const MICROS_PER_SEC: i64 = 1_000_000;
const MICROS_PER_DAY: i64 = 86_400 * MICROS_PER_SEC;

fn encode_timestamp_micros(ts: Timestamp) -> Result<i64, BoxDynError> {
    let total_nanos =
        i128::from(ts.as_secs() - PG_EPOCH_SECS) * 1_000_000_000 + i128::from(ts.subsec_nanos());
    i64::try_from(total_nanos / 1_000)
        .map_err(|_| format!("Timestamp out of range for PostgreSQL: {ts:?}").into())
}

fn encode_date_time_micros(dt: DateTime) -> Result<i64, BoxDynError> {
    let secs = dt.date().epoch_days() * 86_400 + i64::from(dt.time().second_of_day());
    let total_nanos =
        i128::from(secs - PG_EPOCH_SECS) * 1_000_000_000 + i128::from(dt.time().nanosecond());
    i64::try_from(total_nanos / 1_000)
        .map_err(|_| format!("DateTime out of range for PostgreSQL: {dt:?}").into())
}

#[doc(hidden)]
pub fn decode_timestamp_binary(bytes: &[u8]) -> Result<Timestamp, BoxDynError> {
    let raw: [u8; 8] = bytes
        .try_into()
        .map_err(|_| format!("expected 8 bytes for PostgreSQL timestamp, got {}", bytes.len()))?;
    let us = i64::from_be_bytes(raw);
    let secs = us
        .div_euclid(MICROS_PER_SEC)
        .checked_add(PG_EPOCH_SECS)
        .ok_or_else(|| format!("PostgreSQL timestamp {us}us overflows seconds"))?;
    let nanos = (us.rem_euclid(MICROS_PER_SEC) as u32) * 1_000;
    Ok(Timestamp::from_secs_nanos(secs, nanos)?)
}

#[doc(hidden)]
pub fn decode_date_time_binary(bytes: &[u8]) -> Result<DateTime, BoxDynError> {
    decode_timestamp_binary(bytes).map(Timestamp::date_time_utc)
}

#[doc(hidden)]
pub fn decode_date_binary(bytes: &[u8]) -> Result<Date, BoxDynError> {
    let raw: [u8; 4] = bytes
        .try_into()
        .map_err(|_| format!("expected 4 bytes for PostgreSQL date, got {}", bytes.len()))?;
    let days = i32::from_be_bytes(raw);
    Ok(Date::from_epoch_days(PG_EPOCH_DAYS + i64::from(days))?)
}

#[doc(hidden)]
pub fn decode_time_binary(bytes: &[u8]) -> Result<Time, BoxDynError> {
    let raw: [u8; 8] = bytes
        .try_into()
        .map_err(|_| format!("expected 8 bytes for PostgreSQL time, got {}", bytes.len()))?;
    let us = i64::from_be_bytes(raw);
    if !(0..MICROS_PER_DAY).contains(&us) {
        return Err(format!("PostgreSQL time {us}us is outside 00:00:00..24:00:00").into());
    }
    let second_of_day = (us / MICROS_PER_SEC) as u32;
    let nanos = ((us % MICROS_PER_SEC) as u32) * 1_000;
    Ok(Time::from_second_of_day(second_of_day, nanos))
}

fn decode_zoned_text(s: &str) -> Result<Zoned, BoxDynError> {
    if let Ok(zoned) = Zoned::parse("%Y-%m-%d %H:%M:%S%.f%#z", s) {
        return Ok(zoned);
    }
    if let Ok(zoned) = s.parse::<Zoned>() {
        return Ok(zoned);
    }
    let dt = DateTime::parse("%Y-%m-%d %H:%M:%S%.f", s)?;
    Ok(dt.in_utc().in_zone(Zone::fixed(Offset::UTC)))
}

fn decode_date_time_text(s: &str) -> Result<DateTime, BoxDynError> {
    if let Ok(dt) = DateTime::parse("%Y-%m-%d %H:%M:%S%.f", s) {
        return Ok(dt);
    }
    Ok(DateTime::parse("%Y-%m-%d %H:%M:%S%.f%#z", s)?)
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl Type<Postgres> for Timestamp {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("TIMESTAMPTZ")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl PgHasArrayType for Timestamp {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::array_of("TIMESTAMPTZ")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl Encode<'_, Postgres> for Timestamp {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        let micros = encode_timestamp_micros(*self)?;
        Encode::<Postgres>::encode(micros, buf)
    }

    fn size_hint(&self) -> usize {
        mem::size_of::<i64>()
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl<'r> Decode<'r, Postgres> for Timestamp {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.format() {
            PgValueFormat::Binary => decode_timestamp_binary(value.as_bytes()?),
            PgValueFormat::Text => Ok(decode_zoned_text(value.as_str()?)?.timestamp()),
        }
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl Type<Postgres> for Zoned {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("TIMESTAMPTZ")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl PgHasArrayType for Zoned {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::array_of("TIMESTAMPTZ")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl Encode<'_, Postgres> for Zoned {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        let micros = encode_timestamp_micros(self.timestamp())?;
        Encode::<Postgres>::encode(micros, buf)
    }

    fn size_hint(&self) -> usize {
        mem::size_of::<i64>()
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl<'r> Decode<'r, Postgres> for Zoned {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.format() {
            PgValueFormat::Binary => {
                let ts = decode_timestamp_binary(value.as_bytes()?)?;
                Ok(ts.in_zone(Zone::fixed(Offset::UTC)))
            }
            PgValueFormat::Text => decode_zoned_text(value.as_str()?),
        }
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl Type<Postgres> for DateTime {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("TIMESTAMP")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl PgHasArrayType for DateTime {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::array_of("TIMESTAMP")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl Encode<'_, Postgres> for DateTime {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        let micros = encode_date_time_micros(*self)?;
        Encode::<Postgres>::encode(micros, buf)
    }

    fn size_hint(&self) -> usize {
        mem::size_of::<i64>()
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl<'r> Decode<'r, Postgres> for DateTime {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.format() {
            PgValueFormat::Binary => decode_date_time_binary(value.as_bytes()?),
            PgValueFormat::Text => decode_date_time_text(value.as_str()?),
        }
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl Type<Postgres> for Date {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("DATE")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl PgHasArrayType for Date {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::array_of("DATE")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl Encode<'_, Postgres> for Date {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        let days = i32::try_from(self.epoch_days() - PG_EPOCH_DAYS)
            .map_err(|_| format!("value {self:?} would overflow binary encoding for Postgres DATE"))?;
        Encode::<Postgres>::encode(days, buf)
    }

    fn size_hint(&self) -> usize {
        mem::size_of::<i32>()
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl<'r> Decode<'r, Postgres> for Date {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.format() {
            PgValueFormat::Binary => decode_date_binary(value.as_bytes()?),
            PgValueFormat::Text => Ok(Date::parse("%Y-%m-%d", value.as_str()?)?),
        }
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl Type<Postgres> for Time {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("TIME")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl PgHasArrayType for Time {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::array_of("TIME")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl Encode<'_, Postgres> for Time {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        let micros =
            i64::from(self.second_of_day()) * MICROS_PER_SEC + i64::from(self.nanosecond() / 1_000);
        Encode::<Postgres>::encode(micros, buf)
    }

    fn size_hint(&self) -> usize {
        mem::size_of::<u64>()
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-postgres")))]
impl<'r> Decode<'r, Postgres> for Time {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.format() {
            PgValueFormat::Binary => decode_time_binary(value.as_bytes()?),
            PgValueFormat::Text => Ok(Time::parse("%H:%M:%S%.f", value.as_str()?)?),
        }
    }
}
