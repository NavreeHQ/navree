//! Templates in the current locale: the `t` and `locale` functions and the
//! locale-aware `money` and `date` filters navree-i18n adds to navree-view.

use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};

use chrono::format::StrftimeItems;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime};
use navree_template::{Environment, Error, ErrorKind, Kwargs, Value, ValueKind};

use crate::locale::Locale;
use crate::{Args, I18n};

/// The app's `I18n`, once it is built.
pub(crate) type Slot = Arc<OnceLock<Arc<I18n>>>;

/// The current locale, as a locale of the app in `slot` when it has one.
pub(crate) fn current(slot: &Slot) -> Locale {
    let current = Locale::current();
    match slot.get() {
        Some(i18n) if !current.belongs_to(&i18n.shared) => i18n.locale(current.tag()),
        _ => current,
    }
}

/// Adds the functions and filters to `env`:
///
/// - `t(id, name=value, ...)`: message `id` in the current locale;
/// - `locale()`: the current locale's tag (`<html lang="{{ locale() }}">`);
/// - `money(currency)`: an integer amount in minor units, formatted for
///   the current locale (`{{ 4200000 | money("EUR") }}` is `42 000,00 €`
///   in `fr`);
/// - `date(format?)`: a date, a date-time, an RFC 3339 string or a Unix
///   timestamp in the locale's medium style (`26 oct. 2026`), or with a
///   chrono `strftime` format.
pub(crate) fn configure(env: &mut Environment, slot: &Slot) {
    let for_t = slot.clone();
    env.add_function(
        "t",
        move |id: &str, kwargs: Kwargs| -> Result<String, Error> {
            let args = to_args(&kwargs)?;
            current(&for_t)
                .translate(id, &args)
                .map_err(|error| Error::new(ErrorKind::InvalidOperation, error.to_string()))
        },
    );
    let for_locale = slot.clone();
    env.add_function("locale", move || current(&for_locale).tag().to_owned());
    let for_money = slot.clone();
    env.add_filter(
        "money",
        move |value: Value, currency: &str| -> Result<String, Error> {
            money(&current(&for_money), &value, currency)
        },
    );
    let for_date = slot.clone();
    env.add_filter(
        "date",
        move |value: Value, format: Option<&str>| -> Result<String, Error> {
            date(&current(&for_date), &value, format)
        },
    );
}

/// The keyword arguments of `t(...)` as message arguments.
fn to_args(kwargs: &Kwargs) -> Result<Args, Error> {
    let mut args = Args::new();
    for name in kwargs.args() {
        let value: Value = kwargs.get(name)?;
        match value.kind() {
            ValueKind::Undefined | ValueKind::None => args.set(name, None::<&str>),
            ValueKind::Bool => args.set(name, value.is_true()),
            ValueKind::Number => match (value.as_i64(), f64::try_from(value.clone())) {
                (Some(integer), _) if value.is_integer() => args.set(name, integer),
                (_, Ok(float)) => args.set(name, float),
                _ => args.set(name, value.to_string()),
            },
            ValueKind::String => args.set(name, value.as_str().unwrap_or_default()),
            _ => args.set(name, value.to_string()),
        };
    }
    kwargs.assert_all_used()?;
    Ok(args)
}

fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::InvalidOperation, message.to_owned())
}

/// `{{ cents | money("EUR") }}`.
fn money(locale: &Locale, value: &Value, currency: &str) -> Result<String, Error> {
    if value.is_none() || value.is_undefined() {
        return Ok(String::new());
    }
    let cents = (value.kind() == ValueKind::Number && value.is_integer())
        .then(|| value.as_i64())
        .flatten()
        .ok_or_else(|| {
            invalid("money filter: the amount must be an integer number of minor units (cents)")
        })?;
    let code = currency.trim();
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_alphabetic()) {
        return Err(invalid(
            "money filter: the currency must be a 3-letter ISO 4217 code",
        ));
    }
    Ok(locale.format_money(cents, code))
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
        let seconds = (value.kind() == ValueKind::Number && value.is_integer())
            .then(|| value.as_i64())
            .flatten()?;
        DateTime::from_timestamp(seconds, 0).map(|at| Moment::Zoned(at.fixed_offset()))
    }

    fn date(&self) -> NaiveDate {
        match self {
            Moment::Zoned(at) => at.date_naive(),
            Moment::Naive(at) => at.date(),
            Moment::Date(day) => *day,
        }
    }
}

/// `{{ value | date }}` or `{{ value | date("%d/%m/%Y") }}`.
fn date(locale: &Locale, value: &Value, format: Option<&str>) -> Result<String, Error> {
    if value.is_none() || value.is_undefined() {
        return Ok(String::new());
    }
    let moment = Moment::parse(value).ok_or_else(|| {
        invalid("date filter: the value is not a date, a date-time or a Unix timestamp")
    })?;
    let Some(format) = format else {
        return Ok(locale.format_date(moment.date()));
    };
    let items = StrftimeItems::new(format)
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
