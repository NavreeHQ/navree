//! `async_graphql::ScalarType` implementations with Navree's GraphQL scalar
//! names (`DateTime`, `NaiveDate`, `NaiveTime`, `NaiveDateTime`, `TimeZone`).

use ::async_graphql::{InputValueError, InputValueResult, Scalar, ScalarType, Value};

use crate::{Date, DateTime, Precision, Time, Timestamp, Zone, Zoned};

/// An instant in RFC 3339 format.
#[Scalar(
    name = "DateTime",
    specified_by_url = "https://datatracker.ietf.org/doc/html/rfc3339"
)]
impl ScalarType for Timestamp {
    fn parse(value: Value) -> InputValueResult<Self> {
        match &value {
            Value::String(s) => Ok(s.parse::<Timestamp>()?),
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.to_rfc3339_with(Precision::Auto, true))
    }
}

/// A zoned instant in RFC 3339 format.
#[Scalar(
    name = "DateTime",
    specified_by_url = "https://datatracker.ietf.org/doc/html/rfc3339"
)]
impl ScalarType for Zoned {
    fn parse(value: Value) -> InputValueResult<Self> {
        match &value {
            Value::String(s) => Ok(s.parse::<Zoned>()?),
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.to_rfc3339())
    }
}

/// ISO 8601 calendar date without time zone (`%Y-%m-%d`).
#[Scalar(name = "NaiveDate")]
impl ScalarType for Date {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => Ok(Date::parse("%Y-%m-%d", &s)?),
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.format("%Y-%m-%d").to_string())
    }
}

/// ISO 8601 time of day without time zone (`%H:%M:%S%.f`).
#[Scalar(name = "NaiveTime")]
impl ScalarType for Time {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => Ok(Time::parse("%H:%M:%S%.f", &s)?),
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.format("%H:%M:%S%.f").to_string())
    }
}

/// ISO 8601 date and time without time zone (`%Y-%m-%dT%H:%M:%S%.f`).
#[Scalar(name = "NaiveDateTime")]
impl ScalarType for DateTime {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => Ok(DateTime::parse("%Y-%m-%dT%H:%M:%S%.f", &s)?),
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
    }
}

/// An IANA time zone name (`Asia/Kolkata`, `UTC`).
#[Scalar(name = "TimeZone", specified_by_url = "http://www.iana.org/time-zones")]
impl ScalarType for Zone {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => Ok(Zone::named(&s)?),
            _ => Err(InputValueError::expected_type(value)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.to_string())
    }
}
