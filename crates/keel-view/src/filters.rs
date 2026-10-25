//! The `money` and `date` template filters.
//!
//! Error messages describe what is wrong but never echo the value: template
//! contexts may hold secrets.

use std::fmt::Write as _;

use chrono::format::StrftimeItems;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime};
use minijinja::value::{Value, ValueKind};
use minijinja::{Environment, Error, ErrorKind};

/// Registers every Keel filter on `env`.
pub(crate) fn register(env: &mut Environment<'_>) {
    env.add_filter("money", money);
    env.add_filter("date", date);
}

/// `{{ cents|money("USD") }}`: an integer amount in minor units, formatted
/// with the currency's symbol (or ISO code), thousands separators and its
/// number of decimals: `123456|money("USD")` is `$1,234.56`,
/// `150000|money("JPY")` is `¥150,000`, `1999|money("CHF")` is `CHF 19.99`.
/// `none` renders as an empty string.
fn money(value: Value, currency: &str) -> Result<String, Error> {
    if value.is_none() {
        return Ok(String::new());
    }
    let cents = integer(&value).ok_or_else(|| {
        invalid("money filter: the amount must be an integer number of minor units (cents)")
    })?;
    let code = currency.to_ascii_uppercase();
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(invalid(
            "money filter: the currency must be a 3-letter ISO 4217 code",
        ));
    }
    Ok(format_money(cents, &code))
}

/// Formats `cents` of currency `code` (an upper-case ISO 4217 code).
fn format_money(cents: i64, code: &str) -> String {
    let decimals = minor_units(code);
    let divisor = 10_u64.pow(decimals);
    let amount = cents.unsigned_abs();
    let mut out = String::new();
    if cents < 0 {
        out.push('-');
    }
    match symbol(code) {
        Some(symbol) => out.push_str(symbol),
        None => {
            out.push_str(code);
            out.push(' ');
        }
    }
    out.push_str(&group_thousands(amount / divisor));
    if decimals > 0 {
        let width = decimals as usize;
        let _ = write!(out, ".{:0width$}", amount % divisor);
    }
    out
}

/// The symbol written before the amount, for the most common currencies.
fn symbol(code: &str) -> Option<&'static str> {
    Some(match code {
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        "JPY" => "¥",
        "INR" => "₹",
        _ => return None,
    })
}

/// The number of decimals (ISO 4217 minor units) of `code`; 2 unless listed.
fn minor_units(code: &str) -> u32 {
    match code {
        "BIF" | "CLP" | "DJF" | "GNF" | "ISK" | "JPY" | "KMF" | "KRW" | "PYG" | "RWF" | "UGX"
        | "UYI" | "VND" | "VUV" | "XAF" | "XOF" | "XPF" => 0,
        "BHD" | "IQD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND" => 3,
        _ => 2,
    }
}

/// `1234567` as `1,234,567`.
fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// `{{ value|date("%d %b %Y") }}`: formats an RFC 3339 date-time (kept in
/// its own offset), a date-time without offset (`2026-10-25T14:30:00` or
/// with a space), a date (`2026-10-25`) or a Unix timestamp in seconds (UTC)
/// with a chrono `strftime` format, `%Y-%m-%d` by default. `none` renders as
/// an empty string.
fn date(value: Value, format: Option<&str>) -> Result<String, Error> {
    if value.is_none() {
        return Ok(String::new());
    }
    let moment = Moment::parse(&value).ok_or_else(|| {
        invalid("date filter: the value is not a date, a date-time or a Unix timestamp")
    })?;
    let items = StrftimeItems::new(format.unwrap_or("%Y-%m-%d"))
        .parse()
        .map_err(|_| invalid("date filter: invalid format string"))?;
    let mut out = String::new();
    let written = match moment {
        Moment::Zoned(at) => write!(out, "{}", at.format_with_items(items.iter())),
        Moment::Naive(at) => write!(out, "{}", at.format_with_items(items.iter())),
        Moment::Date(day) => write!(out, "{}", day.format_with_items(items.iter())),
    };
    written.map_err(|_| {
        invalid(
            "date filter: the format asks for fields the value does not have (a time or offset)",
        )
    })?;
    Ok(out)
}

/// A value the `date` filter understands.
enum Moment {
    Zoned(DateTime<FixedOffset>),
    Naive(NaiveDateTime),
    Date(NaiveDate),
}

impl Moment {
    fn parse(value: &Value) -> Option<Moment> {
        if let Some(text) = value.as_str() {
            let text = text.trim();
            return DateTime::parse_from_rfc3339(text)
                .map(Moment::Zoned)
                .or_else(|_| {
                    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f").map(Moment::Naive)
                })
                .or_else(|_| {
                    NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f").map(Moment::Naive)
                })
                .or_else(|_| NaiveDate::parse_from_str(text, "%Y-%m-%d").map(Moment::Date))
                .ok();
        }
        let seconds = integer(value)?;
        DateTime::from_timestamp(seconds, 0).map(|at| Moment::Zoned(at.fixed_offset()))
    }
}

/// `value` as an `i64` when it is an integer number (not `42.0`, not `"42"`).
fn integer(value: &Value) -> Option<i64> {
    (value.kind() == ValueKind::Number && value.is_integer())
        .then(|| value.as_i64())
        .flatten()
}

fn invalid(message: &'static str) -> Error {
    Error::new(ErrorKind::InvalidOperation, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1_000), "1,000");
        assert_eq!(group_thousands(12_345_678), "12,345,678");
        assert_eq!(group_thousands(u64::MAX), "18,446,744,073,709,551,615");
    }
}
