//! MySQL and MariaDB `Type`, `Encode` and `Decode` implementations.

use std::sync::LazyLock;

use ::sqlx::TypeInfo as _;
use ::sqlx::decode::Decode;
use ::sqlx::encode::{Encode, IsNull};
use ::sqlx::error::{BoxDynError, UnexpectedNullError};
use ::sqlx::mysql::types::{MySqlTime, MySqlTimeSign};
use ::sqlx::mysql::{MySql, MySqlTypeInfo, MySqlValueRef};
use ::sqlx::types::Type;

use crate::{Date, DateTime, Offset, Time, Timestamp, Zone, Zoned};

static TIMESTAMP_TYPE_INFO: LazyLock<MySqlTypeInfo> =
    LazyLock::new(|| mysql_binary_type_info("Timestamp"));
static DATETIME_TYPE_INFO: LazyLock<MySqlTypeInfo> =
    LazyLock::new(|| mysql_binary_type_info("Datetime"));
static DATE_TYPE_INFO: LazyLock<MySqlTypeInfo> = LazyLock::new(|| mysql_binary_type_info("Date"));

fn mysql_binary_type_info(column_type: &str) -> MySqlTypeInfo {
    let json = format!(r#"{{"type":"{column_type}","flags":"BINARY","collation":63}}"#);
    serde_json::from_str(&json).expect("valid MySqlTypeInfo descriptor")
}

fn date_time_encoded_len(dt: DateTime) -> u8 {
    let time = dt.time();
    match (
        time.hour(),
        time.minute(),
        time.second(),
        time.nanosecond(),
    ) {
        (0, 0, 0, 0) => 4,
        (_, _, _, 0) => 7,
        _ => 11,
    }
}

fn time_encoded_len(time: Time) -> u8 {
    if time.nanosecond() == 0 { 8 } else { 12 }
}

fn encode_date(date: Date, buf: &mut Vec<u8>) -> Result<(), BoxDynError> {
    let year = u16::try_from(date.year())
        .map_err(|_| format!("NaiveDateTime out of range for Mysql: {date}"))?;
    buf.extend_from_slice(&year.to_le_bytes());
    buf.push(date.month_number());
    buf.push(date.day());
    Ok(())
}

fn encode_time(time: Time, include_micros: bool, buf: &mut Vec<u8>) {
    buf.push(time.hour());
    buf.push(time.minute());
    buf.push(time.second());
    if include_micros {
        buf.extend_from_slice(&(time.nanosecond() / 1_000).to_le_bytes());
    }
}

fn decode_binary_date(buf: &[u8]) -> Result<Option<Date>, BoxDynError> {
    match buf.len() {
        0 => Ok(None),
        4.. => {
            let year = i32::from(u16::from_le_bytes([buf[0], buf[1]]));
            let month = buf[2];
            let day = buf[3];
            let date = Date::new(year, month, day)
                .map_err(|_| format!("server returned invalid date: {year}/{month}/{day}"))?;
            Ok(Some(date))
        }
        len => Err(format!("expected at least 4 bytes for date, got {len}").into()),
    }
}

fn decode_binary_time(len: u8, buf: &[u8]) -> Result<Time, BoxDynError> {
    if buf.len() < 3 {
        return Err(format!("expected at least 3 bytes for time, got {}", buf.len()).into());
    }
    let hour = buf[0];
    let minute = buf[1];
    let second = buf[2];
    let micros = if len > 3 {
        let rest = &buf[3..];
        if rest.is_empty() || rest.len() > 8 {
            return Err(format!("unexpected microseconds byte length {}", rest.len()).into());
        }
        let mut raw = [0u8; 8];
        raw[..rest.len()].copy_from_slice(rest);
        let value = u64::from_le_bytes(raw);
        u32::try_from(value)
            .map_err(|_| format!("server returned microseconds out of range: {value}"))?
    } else {
        0
    };
    let nanos = micros
        .checked_mul(1_000)
        .ok_or_else(|| format!("server returned microseconds out of range: {micros}"))?;
    Time::new(hour, minute, second)
        .and_then(|t| t.with_nanos(nanos))
        .map_err(|_| {
            format!(
                "server returned invalid time: {hour:02}:{minute:02}:{second:02}; micros: {micros}"
            )
            .into()
        })
}

fn is_binary_date_packet(buf: &[u8]) -> bool {
    matches!(buf.first(), Some(&(0 | 4))) && buf.len() == usize::from(buf[0]) + 1
}

fn is_binary_date_time_packet(buf: &[u8]) -> bool {
    matches!(buf.first(), Some(&(0 | 4 | 7 | 11))) && buf.len() == usize::from(buf[0]) + 1
}

#[doc(hidden)]
pub fn decode_date_bytes(buf: &[u8]) -> Result<Date, BoxDynError> {
    if buf.is_empty() {
        return Err("empty buffer".into());
    }
    if is_binary_date_packet(buf) {
        return decode_binary_date(&buf[1..])?.ok_or_else(|| UnexpectedNullError.into());
    }
    let s = std::str::from_utf8(buf)?;
    Ok(Date::parse("%Y-%m-%d", s)?)
}

#[doc(hidden)]
pub fn decode_date_time_bytes(buf: &[u8]) -> Result<DateTime, BoxDynError> {
    if buf.is_empty() {
        return Err("empty buffer".into());
    }
    if is_binary_date_time_packet(buf) {
        let len = buf[0];
        let date = decode_binary_date(&buf[1..])?.ok_or(UnexpectedNullError)?;
        let time = if len > 4 {
            decode_binary_time(len - 4, &buf[5..])?
        } else {
            Time::MIDNIGHT
        };
        return Ok(date.at(time));
    }
    let s = std::str::from_utf8(buf)?;
    Ok(DateTime::parse("%Y-%m-%d %H:%M:%S%.f", s)?)
}

#[doc(hidden)]
pub fn decode_timestamp_bytes(buf: &[u8]) -> Result<Timestamp, BoxDynError> {
    Ok(decode_date_time_bytes(buf)?.in_utc())
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl Type<MySql> for Timestamp {
    fn type_info() -> MySqlTypeInfo {
        TIMESTAMP_TYPE_INFO.clone()
    }

    fn compatible(ty: &MySqlTypeInfo) -> bool {
        matches!(ty.name(), "DATETIME" | "TIMESTAMP")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl Encode<'_, MySql> for Timestamp {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        Encode::<MySql>::encode(self.date_time_utc(), buf)
    }

    fn size_hint(&self) -> usize {
        usize::from(date_time_encoded_len(self.date_time_utc())) + 1
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl<'r> Decode<'r, MySql> for Timestamp {
    fn decode(value: MySqlValueRef<'r>) -> Result<Self, BoxDynError> {
        let bytes = <&'r [u8] as Decode<'r, MySql>>::decode(value)?;
        decode_timestamp_bytes(bytes)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl Type<MySql> for Zoned {
    fn type_info() -> MySqlTypeInfo {
        TIMESTAMP_TYPE_INFO.clone()
    }

    fn compatible(ty: &MySqlTypeInfo) -> bool {
        matches!(ty.name(), "DATETIME" | "TIMESTAMP")
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl Encode<'_, MySql> for Zoned {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        Encode::<MySql>::encode(self.timestamp().date_time_utc(), buf)
    }

    fn size_hint(&self) -> usize {
        usize::from(date_time_encoded_len(self.timestamp().date_time_utc())) + 1
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl<'r> Decode<'r, MySql> for Zoned {
    fn decode(value: MySqlValueRef<'r>) -> Result<Self, BoxDynError> {
        let ts = <Timestamp as Decode<'r, MySql>>::decode(value)?;
        Ok(ts.in_zone(Zone::fixed(Offset::UTC)))
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl Type<MySql> for DateTime {
    fn type_info() -> MySqlTypeInfo {
        DATETIME_TYPE_INFO.clone()
    }

    fn compatible(ty: &MySqlTypeInfo) -> bool {
        ty.name() == "DATETIME"
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl Encode<'_, MySql> for DateTime {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        let len = date_time_encoded_len(*self);
        buf.push(len);
        encode_date(self.date(), buf)?;
        if len > 4 {
            encode_time(self.time(), len > 7, buf);
        }
        Ok(IsNull::No)
    }

    fn size_hint(&self) -> usize {
        usize::from(date_time_encoded_len(*self)) + 1
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl<'r> Decode<'r, MySql> for DateTime {
    fn decode(value: MySqlValueRef<'r>) -> Result<Self, BoxDynError> {
        let bytes = <&'r [u8] as Decode<'r, MySql>>::decode(value)?;
        decode_date_time_bytes(bytes)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl Type<MySql> for Date {
    fn type_info() -> MySqlTypeInfo {
        DATE_TYPE_INFO.clone()
    }

    fn compatible(ty: &MySqlTypeInfo) -> bool {
        ty.name() == "DATE"
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl Encode<'_, MySql> for Date {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.push(4);
        encode_date(*self, buf)?;
        Ok(IsNull::No)
    }

    fn size_hint(&self) -> usize {
        5
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl<'r> Decode<'r, MySql> for Date {
    fn decode(value: MySqlValueRef<'r>) -> Result<Self, BoxDynError> {
        let bytes = <&'r [u8] as Decode<'r, MySql>>::decode(value)?;
        decode_date_bytes(bytes)
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl Type<MySql> for Time {
    fn type_info() -> MySqlTypeInfo {
        <MySqlTime as Type<MySql>>::type_info()
    }

    fn compatible(ty: &MySqlTypeInfo) -> bool {
        ty.name() == "TIME"
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl Encode<'_, MySql> for Time {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        let len = time_encoded_len(*self);
        buf.push(len);
        buf.push(0);
        buf.extend_from_slice(&[0_u8; 4]);
        encode_time(*self, len > 8, buf);
        Ok(IsNull::No)
    }

    fn size_hint(&self) -> usize {
        usize::from(time_encoded_len(*self)) + 1
    }
}

#[cfg_attr(docsrs, doc(cfg(feature = "sqlx-mysql")))]
impl<'r> Decode<'r, MySql> for Time {
    fn decode(value: MySqlValueRef<'r>) -> Result<Self, BoxDynError> {
        let time = MySqlTime::decode(value)?;
        if time.sign() != MySqlTimeSign::Positive {
            return Err(format!("Cannot convert negative `MySqlTime` value to `Time`: {time}").into());
        }
        let hour = u8::try_from(time.hours())
            .map_err(|_| format!("Cannot convert `MySqlTime` value to `Time`: {time}"))?;
        let nanos = time
            .microseconds()
            .checked_mul(1_000)
            .ok_or_else(|| format!("Cannot convert `MySqlTime` value to `Time`: {time}"))?;
        Time::new(hour, time.minutes(), time.seconds())
            .and_then(|t| t.with_nanos(nanos))
            .map_err(|_| format!("Cannot convert `MySqlTime` value to `Time`: {time}").into())
    }
}
