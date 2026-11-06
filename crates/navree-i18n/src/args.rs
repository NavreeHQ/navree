//! Message arguments: [`Args`], [`Arg`], [`Money`], and dates as Fluent
//! values formatted in the bundle's locale.

use std::borrow::Cow;
use std::fmt;

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use fluent_bundle::types::{FluentNumber, FluentNumberOptions, FluentType};
use fluent_bundle::{FluentArgs, FluentValue};
use intl_memoizer::Memoizable;
use unic_langid::LanguageIdentifier;

use crate::format::{DateStyle, Formats, TimeStyle};

/// The arguments of a message: `name = value` pairs, built with
/// [`args!`](crate::args) (or [`Args::new`] and [`with`](Args::with)).
///
/// ```
/// use navree_i18n::{Args, Money, args};
///
/// let args = args!(count = 3, owner = "Anu", amount = Money::new(4_200_000, "EUR"));
/// assert_eq!(args.len(), 3);
/// let same = Args::new().with("count", 3).with("owner", "Anu");
/// assert_eq!(same.len(), 2);
/// ```
#[derive(Default)]
pub struct Args(pub(crate) FluentArgs<'static>);

impl Args {
    /// No arguments.
    pub fn new() -> Args {
        Args(FluentArgs::new())
    }

    /// Sets argument `name` (`$name` in the message).
    pub fn set(&mut self, name: &str, value: impl Into<Arg>) -> &mut Args {
        self.0.set(name.to_owned(), value.into().0);
        self
    }

    /// With argument `name` set.
    #[must_use]
    pub fn with(mut self, name: &str, value: impl Into<Arg>) -> Args {
        self.set(name, value);
        self
    }

    /// How many arguments there are.
    pub fn len(&self) -> usize {
        self.0.iter().count()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The arguments for a lookup (`None` when there are none).
    pub(crate) fn fluent(&self) -> Option<&FluentArgs<'static>> {
        (!self.is_empty()).then_some(&self.0)
    }
}

impl fmt::Debug for Args {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.0.iter()).finish()
    }
}

/// One argument value: a number (any integer or float type), a string, a
/// [`Money`] amount, a chrono date or date-time, a `bool` (as `"true"` or
/// `"false"`, for selectors), or an `Option` of those (`None` prints
/// nothing).
///
/// Numbers select plural variants and print with the locale's separators
/// (`1,234` in `en`, `1 234` in `fr`); dates print in the locale's medium
/// style unless `DATETIME()` asks for another.
#[derive(Debug)]
pub struct Arg(pub(crate) FluentValue<'static>);

macro_rules! number_args {
    ($($ty:ty),*) => {$(
        impl From<$ty> for Arg {
            #[allow(clippy::cast_precision_loss, clippy::cast_lossless)]
            fn from(value: $ty) -> Arg {
                Arg(FluentValue::Number(FluentNumber::new(
                    value as f64,
                    FluentNumberOptions::default(),
                )))
            }
        }
    )*};
}

number_args!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64);

impl From<&str> for Arg {
    fn from(value: &str) -> Arg {
        Arg(FluentValue::String(Cow::Owned(value.to_owned())))
    }
}

impl From<String> for Arg {
    fn from(value: String) -> Arg {
        Arg(FluentValue::String(Cow::Owned(value)))
    }
}

impl From<&String> for Arg {
    fn from(value: &String) -> Arg {
        Arg::from(value.as_str())
    }
}

impl From<Cow<'_, str>> for Arg {
    fn from(value: Cow<'_, str>) -> Arg {
        Arg::from(value.into_owned())
    }
}

impl From<bool> for Arg {
    fn from(value: bool) -> Arg {
        Arg::from(if value { "true" } else { "false" })
    }
}

impl From<Money> for Arg {
    fn from(value: Money) -> Arg {
        Arg(FluentValue::Custom(Box::new(value)))
    }
}

impl From<&Money> for Arg {
    fn from(value: &Money) -> Arg {
        Arg::from(value.clone())
    }
}

impl From<NaiveDate> for Arg {
    fn from(value: NaiveDate) -> Arg {
        Arg(FluentValue::Custom(Box::new(FluentDate::new(value, None))))
    }
}

impl From<NaiveDateTime> for Arg {
    fn from(value: NaiveDateTime) -> Arg {
        Arg(FluentValue::Custom(Box::new(FluentDate::new(
            value.date(),
            Some(value.time()),
        ))))
    }
}

impl<Tz: TimeZone> From<DateTime<Tz>> for Arg {
    /// The date and time in the value's own time zone.
    fn from(value: DateTime<Tz>) -> Arg {
        Arg::from(value.naive_local())
    }
}

impl<T: Into<Arg>> From<Option<T>> for Arg {
    fn from(value: Option<T>) -> Arg {
        value.map_or(Arg(FluentValue::None), Into::into)
    }
}

/// An amount in minor units (cents) of a currency, as a message argument
/// or for [`Locale::format_money`](crate::Locale::format_money): it prints
/// with the currency's decimals and symbol where the locale puts it
/// (`€42,000.00` in `en`, `42 000,00 €` in `fr`).
///
/// ```text
/// deal-amount = Amount: { $amount }
/// ```
///
/// ```ignore
/// t!("deal-amount", amount = Money::new(4_200_000, "EUR"))
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Money {
    /// The amount in the currency's minor units (cents).
    pub minor_units: i64,
    /// The ISO 4217 currency code (`EUR`).
    pub currency: String,
}

impl Money {
    /// `minor_units` of `currency` (an ISO 4217 code).
    pub fn new(minor_units: i64, currency: impl Into<String>) -> Money {
        Money {
            minor_units,
            currency: currency.into(),
        }
    }
}

impl FluentType for Money {
    fn duplicate(&self) -> Box<dyn FluentType + Send> {
        Box::new(self.clone())
    }

    fn as_string(&self, intls: &intl_memoizer::IntlLangMemoizer) -> Cow<'static, str> {
        intls
            .with_try_get::<FormatsMemo, _, _>((), |formats| {
                formats.0.money(self.minor_units, &self.currency)
            })
            .map_or_else(|()| self.fallback(), Cow::Owned)
    }

    fn as_string_threadsafe(
        &self,
        intls: &intl_memoizer::concurrent::IntlLangMemoizer,
    ) -> Cow<'static, str> {
        intls
            .with_try_get::<FormatsMemo, _, _>((), |formats| {
                formats.0.money(self.minor_units, &self.currency)
            })
            .map_or_else(|()| self.fallback(), Cow::Owned)
    }
}

impl Money {
    fn fallback(&self) -> Cow<'static, str> {
        Cow::Owned(format!("{} {}", self.minor_units, self.currency))
    }
}

/// A date (and maybe a time) in a message, with the style `DATETIME()`
/// chose.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FluentDate {
    date: NaiveDate,
    time: Option<NaiveTime>,
    style: DateStyle,
    time_style: Option<TimeStyle>,
}

impl FluentDate {
    fn new(date: NaiveDate, time: Option<NaiveTime>) -> FluentDate {
        FluentDate {
            date,
            time,
            style: DateStyle::Medium,
            time_style: None,
        }
    }

    fn format(&self, formats: &Formats) -> String {
        formats.date(self.date, self.time, self.style, self.time_style)
    }
}

impl FluentType for FluentDate {
    fn duplicate(&self) -> Box<dyn FluentType + Send> {
        Box::new(self.clone())
    }

    fn as_string(&self, intls: &intl_memoizer::IntlLangMemoizer) -> Cow<'static, str> {
        intls
            .with_try_get::<FormatsMemo, _, _>((), |formats| self.format(&formats.0))
            .map_or_else(|()| Cow::Owned(self.date.to_string()), Cow::Owned)
    }

    fn as_string_threadsafe(
        &self,
        intls: &intl_memoizer::concurrent::IntlLangMemoizer,
    ) -> Cow<'static, str> {
        intls
            .with_try_get::<FormatsMemo, _, _>((), |formats| self.format(&formats.0))
            .map_or_else(|()| Cow::Owned(self.date.to_string()), Cow::Owned)
    }
}

/// The formatters of a bundle's locale, memoized by fluent-bundle.
pub(crate) struct FormatsMemo(pub(crate) Formats);

impl Memoizable for FormatsMemo {
    type Args = ();
    type Error = ();

    fn construct(lang: LanguageIdentifier, _args: ()) -> Result<Self, ()> {
        Ok(FormatsMemo(Formats::new(&lang)))
    }
}

/// The bundle formatter: numbers with the locale's separators and their
/// `NUMBER()` options; other values as fluent-bundle prints them.
pub(crate) fn format_value(
    value: &FluentValue<'_>,
    intls: &intl_memoizer::concurrent::IntlLangMemoizer,
) -> Option<String> {
    match value {
        FluentValue::Number(number) => intls
            .with_try_get::<FormatsMemo, _, _>((), |formats| formats.0.fluent_number(number))
            .ok()
            .flatten(),
        _ => None,
    }
}

/// `DATETIME($date, dateStyle: "long", timeStyle: "short")`: a date
/// argument, an RFC 3339 string, a `YYYY-MM-DD` string or a Unix timestamp
/// in seconds (UTC), in the given styles (`dateStyle` `short`, `medium`
/// (the default), `long` or `full`; `timeStyle` `short` or `medium` adds
/// the time).
pub(crate) fn datetime<'a>(positional: &[FluentValue<'a>], named: &FluentArgs) -> FluentValue<'a> {
    let Some(mut date) = positional.first().and_then(to_date) else {
        return FluentValue::Error;
    };
    if let Some(FluentValue::String(style)) = named.get("dateStyle") {
        match DateStyle::parse(style) {
            Some(style) => date.style = style,
            None => return FluentValue::Error,
        }
    }
    if let Some(FluentValue::String(style)) = named.get("timeStyle") {
        match TimeStyle::parse(style) {
            Some(style) => date.time_style = Some(style),
            None => return FluentValue::Error,
        }
        if date.time.is_none() {
            date.time = Some(NaiveTime::MIN);
        }
    }
    FluentValue::Custom(Box::new(date))
}

/// The date a `DATETIME()` argument stands for.
fn to_date(value: &FluentValue<'_>) -> Option<FluentDate> {
    match value {
        FluentValue::Custom(custom) => custom.as_any().downcast_ref::<FluentDate>().cloned(),
        FluentValue::String(text) => parse_date(text),
        FluentValue::Number(number) => {
            #[allow(clippy::cast_possible_truncation)]
            let seconds = number.value as i64;
            DateTime::from_timestamp(seconds, 0)
                .map(|at| FluentDate::new(at.date_naive(), Some(at.time())))
        }
        _ => None,
    }
}

/// An RFC 3339 date-time (in its own offset), a date-time without offset
/// or a `YYYY-MM-DD` date.
pub(crate) fn parse_date(text: &str) -> Option<FluentDate> {
    let text = text.trim();
    if let Ok(at) = DateTime::<FixedOffset>::parse_from_rfc3339(text) {
        let local = at.naive_local();
        return Some(FluentDate::new(local.date(), Some(local.time())));
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(at) = NaiveDateTime::parse_from_str(text, format) {
            return Some(FluentDate::new(at.date(), Some(at.time())));
        }
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .map(|day| FluentDate::new(day, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_convert_to_fluent_values() {
        assert!(matches!(Arg::from(3_usize).0, FluentValue::Number(n) if n.value == 3.0));
        assert!(matches!(Arg::from(-2.5_f64).0, FluentValue::Number(n) if n.value == -2.5));
        assert!(matches!(Arg::from("Anu").0, FluentValue::String(s) if s == "Anu"));
        assert!(matches!(Arg::from(true).0, FluentValue::String(s) if s == "true"));
        assert!(matches!(Arg::from(None::<&str>).0, FluentValue::None));
        assert!(matches!(Arg::from(Some(1_u8)).0, FluentValue::Number(_)));
        assert!(matches!(
            Arg::from(Money::new(1, "EUR")).0,
            FluentValue::Custom(_)
        ));
        let day = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
        assert!(matches!(Arg::from(day).0, FluentValue::Custom(_)));
    }

    #[test]
    fn dates_parse_from_strings_and_timestamps() {
        let date = parse_date("2026-10-26T14:30:00+02:00").unwrap();
        assert_eq!(date.date, NaiveDate::from_ymd_opt(2026, 10, 26).unwrap());
        assert_eq!(date.time, NaiveTime::from_hms_opt(14, 30, 0));
        assert_eq!(parse_date("2026-10-26").unwrap().time, None);
        assert!(parse_date("26/10/2026").is_none());
        let from_number = to_date(&FluentValue::Number(FluentNumber::new(
            0.0,
            Default::default(),
        )))
        .unwrap();
        assert_eq!(
            from_number.date,
            NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()
        );
    }

    #[test]
    fn datetime_applies_the_styles() {
        let mut named = FluentArgs::new();
        named.set("dateStyle", "long");
        named.set("timeStyle", "short");
        let value = datetime(&[FluentValue::from("2026-10-26")], &named);
        let FluentValue::Custom(custom) = value else {
            panic!("not a date: {value:?}");
        };
        let date = custom.as_any().downcast_ref::<FluentDate>().unwrap();
        assert_eq!(date.style, DateStyle::Long);
        assert_eq!(date.time_style, Some(TimeStyle::Short));
        assert_eq!(date.time, Some(NaiveTime::MIN));

        let mut bad = FluentArgs::new();
        bad.set("dateStyle", "huge");
        assert!(matches!(
            datetime(&[FluentValue::from("2026-10-26")], &bad),
            FluentValue::Error
        ));
        assert!(matches!(
            datetime(&[], &FluentArgs::new()),
            FluentValue::Error
        ));
    }
}
