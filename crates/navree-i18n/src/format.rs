//! Numbers, money and dates formatted with a locale's conventions (ICU4X
//! with compiled data): decimal and grouping separators, the currency
//! symbol's place and the date pattern.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};
use fixed_decimal::Decimal;
use fluent_bundle::types::{FluentNumber, FluentNumberStyle};
use icu_calendar::Date;
use icu_calendar::cal::Gregorian;
use icu_datetime::FixedCalendarDateTimeFormatter;
use icu_datetime::fieldsets::{YMD, YMDT};
use icu_datetime::input::{DateTime, Time};
use icu_decimal::DecimalFormatter;
use icu_decimal::options::{DecimalFormatterOptions, GroupingStrategy};
use unic_langid::LanguageIdentifier;

/// How long a formatted date is: `short` (`10/26/26`), `medium` (`Oct 26,
/// 2026`, the default) or `long` (`October 26, 2026`). Fluent's
/// `dateStyle: "full"` is `long`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DateStyle {
    /// Numeric: `10/26/26`, `26/10/2026`.
    Short,
    /// Abbreviated month: `Oct 26, 2026`, `26 oct. 2026`.
    #[default]
    Medium,
    /// Full month name: `October 26, 2026`, `26 octobre 2026`.
    Long,
}

impl DateStyle {
    /// `short`, `medium`, `long` or `full` (as `long`).
    pub(crate) fn parse(value: &str) -> Option<DateStyle> {
        match value {
            "short" => Some(DateStyle::Short),
            "medium" => Some(DateStyle::Medium),
            "long" | "full" => Some(DateStyle::Long),
            _ => None,
        }
    }
}

/// The time part of a formatted date-time: hours and minutes (`short`) or
/// with seconds (`medium`, `long`, `full`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TimeStyle {
    Short,
    Medium,
}

impl TimeStyle {
    pub(crate) fn parse(value: &str) -> Option<TimeStyle> {
        match value {
            "short" => Some(TimeStyle::Short),
            "medium" | "long" | "full" => Some(TimeStyle::Medium),
            _ => None,
        }
    }
}

/// A date formatter for one style.
enum DateFormatter {
    Date(FixedCalendarDateTimeFormatter<Gregorian, YMD>),
    DateTime(FixedCalendarDateTimeFormatter<Gregorian, YMDT>),
}

/// Date formatters by style, built on first use (`None`: no data).
type DateFormatters = HashMap<(DateStyle, Option<TimeStyle>), Option<Arc<DateFormatter>>>;

/// The formatters of one locale, built on first use and kept.
pub(crate) struct Formats {
    langid: LanguageIdentifier,
    icu: Option<icu_locale_core::Locale>,
    grouped: OnceLock<Option<DecimalFormatter>>,
    ungrouped: OnceLock<Option<DecimalFormatter>>,
    dates: Mutex<DateFormatters>,
}

impl Formats {
    /// The formatters of `langid` (ICU4X falls back to the language, then
    /// to the root locale, for data it lacks).
    pub(crate) fn new(langid: &LanguageIdentifier) -> Formats {
        Formats {
            langid: langid.clone(),
            icu: icu_locale_core::Locale::try_from_str(&langid.to_string()).ok(),
            grouped: OnceLock::new(),
            ungrouped: OnceLock::new(),
            dates: Mutex::new(HashMap::new()),
        }
    }

    fn decimal_formatter(&self, grouping: bool) -> Option<&DecimalFormatter> {
        let (cell, strategy) = if grouping {
            (&self.grouped, GroupingStrategy::Auto)
        } else {
            (&self.ungrouped, GroupingStrategy::Never)
        };
        cell.get_or_init(|| {
            let locale = self.icu.clone()?;
            let mut options = DecimalFormatterOptions::default();
            options.grouping_strategy = Some(strategy);
            DecimalFormatter::try_new(locale.into(), options).ok()
        })
        .as_ref()
    }

    /// `decimal` with the locale's separators.
    pub(crate) fn decimal(&self, decimal: &Decimal, grouping: bool) -> String {
        match self.decimal_formatter(grouping) {
            Some(formatter) => formatter.format(decimal).to_string(),
            None => decimal.to_string(),
        }
    }

    /// An integer or a float with at most three fraction digits:
    /// `1234.5` is `1,234.5` in `en` and `1 234,5` in `fr`.
    pub(crate) fn number(&self, value: f64) -> String {
        let Some(mut decimal) = to_decimal(value) else {
            return value.to_string();
        };
        decimal.round(-3);
        decimal.absolute.trim_end();
        self.decimal(&decimal, true)
    }

    /// A Fluent number with its `NUMBER()` options: `style` (`decimal`,
    /// `percent`, `currency` with `currency`), `useGrouping`,
    /// `minimumIntegerDigits`, `minimumFractionDigits` and
    /// `maximumFractionDigits`.
    pub(crate) fn fluent_number(&self, number: &FluentNumber) -> Option<String> {
        let options = &number.options;
        let value = match options.style {
            FluentNumberStyle::Percent => number.value * 100.0,
            _ => number.value,
        };
        let mut decimal = to_decimal(value)?;
        let currency = match options.style {
            FluentNumberStyle::Currency => Some(options.currency.as_deref()?.to_ascii_uppercase()),
            _ => None,
        };
        let (default_min, default_max) = match (&currency, options.style) {
            (Some(code), _) => {
                let units = minor_units(code) as usize;
                (units, units)
            }
            (None, FluentNumberStyle::Percent) => (0, 0),
            (None, _) => (0, 3),
        };
        let mut max = options.maximum_fraction_digits.unwrap_or(default_max);
        let min = options
            .minimum_fraction_digits
            .unwrap_or_else(|| default_min.min(max));
        max = max.max(min);
        decimal.round(-to_position(max));
        decimal.absolute.trim_end();
        decimal.absolute.pad_end(-to_position(min));
        if let Some(digits) = options.minimum_integer_digits {
            decimal.absolute.pad_start(to_position(digits));
        }
        let formatted = self.decimal(&decimal, options.use_grouping);
        Some(match (currency, options.style) {
            (Some(code), _) => self.place_currency(&formatted, &code),
            (None, FluentNumberStyle::Percent) => format!("{formatted}{}", self.percent_sign()),
            (None, _) => formatted,
        })
    }

    /// `minor_units` of `currency` (an ISO 4217 code) with its decimals and
    /// symbol where the locale puts it: `format_money(4_200_000, "EUR")` is
    /// `€42,000.00` in `en` and `42 000,00 €` in `fr`.
    pub(crate) fn money(&self, minor_units: i64, currency: &str) -> String {
        let code = currency.trim().to_ascii_uppercase();
        let decimals = self::minor_units(&code);
        let mut decimal = Decimal::from(minor_units);
        decimal.multiply_pow10(-to_position(decimals as usize));
        decimal.absolute.pad_end(-to_position(decimals as usize));
        let formatted = self.decimal(&decimal, true);
        self.place_currency(&formatted, &code)
    }

    /// The currency symbol (or code) before or after `amount`, as the
    /// locale writes it.
    fn place_currency(&self, amount: &str, code: &str) -> String {
        let (sign, digits) = match amount.strip_prefix(['-', '\u{2212}']) {
            Some(rest) => (&amount[..amount.len() - rest.len()], rest),
            None => ("", amount),
        };
        let symbol = currency_symbol(code);
        match placement(&self.langid) {
            Placement::Before if symbol.is_some() => {
                format!("{sign}{}{digits}", symbol.unwrap_or(code))
            }
            Placement::Before | Placement::BeforeSpace => {
                format!("{sign}{}\u{a0}{digits}", symbol.unwrap_or(code))
            }
            Placement::After => format!("{sign}{digits}\u{a0}{}", symbol.unwrap_or(code)),
        }
    }

    /// `%`, after a no-break space in languages that space it.
    fn percent_sign(&self) -> &'static str {
        match self.langid.language.as_str() {
            "fr" | "de" | "es" | "sv" | "nb" | "nn" | "no" | "fi" | "da" | "cs" | "sk" | "ru"
            | "uk" | "pl" => "\u{a0}%",
            _ => "%",
        }
    }

    /// `date` (and `time`) in `style`, in the Gregorian calendar.
    pub(crate) fn date(
        &self,
        date: NaiveDate,
        time: Option<NaiveTime>,
        style: DateStyle,
        time_style: Option<TimeStyle>,
    ) -> String {
        let time_style = time.and(time_style);
        let formatter = self.date_formatter(style, time_style);
        let fallback = || match (time, time_style) {
            (Some(time), Some(_)) => {
                format!("{} {}", date.format("%Y-%m-%d"), time.format("%H:%M"))
            }
            _ => date.format("%Y-%m-%d").to_string(),
        };
        let Some(formatter) = formatter else {
            return fallback();
        };
        let Ok(icu_date) = Date::try_new_gregorian(
            date.year(),
            u8::try_from(date.month()).unwrap_or(1),
            u8::try_from(date.day()).unwrap_or(1),
        ) else {
            return fallback();
        };
        match (&*formatter, time) {
            (DateFormatter::Date(formatter), _) => formatter.format(&icu_date).to_string(),
            (DateFormatter::DateTime(formatter), Some(time)) => {
                let Ok(icu_time) = Time::try_new(
                    u8::try_from(time.hour()).unwrap_or(0),
                    u8::try_from(time.minute()).unwrap_or(0),
                    u8::try_from(time.second()).unwrap_or(0),
                    0,
                ) else {
                    return fallback();
                };
                formatter
                    .format(&DateTime {
                        date: icu_date,
                        time: icu_time,
                    })
                    .to_string()
            }
            (DateFormatter::DateTime(_), None) => fallback(),
        }
    }

    fn date_formatter(
        &self,
        style: DateStyle,
        time: Option<TimeStyle>,
    ) -> Option<Arc<DateFormatter>> {
        let mut dates = self.dates.lock().unwrap_or_else(PoisonError::into_inner);
        dates
            .entry((style, time))
            .or_insert_with(|| {
                let locale = self.icu.clone()?;
                let fields = match style {
                    DateStyle::Short => YMD::short(),
                    DateStyle::Medium => YMD::medium(),
                    DateStyle::Long => YMD::long(),
                };
                let formatter = match time {
                    None => DateFormatter::Date(
                        FixedCalendarDateTimeFormatter::try_new(locale.into(), fields).ok()?,
                    ),
                    Some(TimeStyle::Short) => DateFormatter::DateTime(
                        FixedCalendarDateTimeFormatter::try_new(
                            locale.into(),
                            fields.with_time_hm(),
                        )
                        .ok()?,
                    ),
                    Some(TimeStyle::Medium) => DateFormatter::DateTime(
                        FixedCalendarDateTimeFormatter::try_new(
                            locale.into(),
                            fields.with_time_hms(),
                        )
                        .ok()?,
                    ),
                };
                Some(Arc::new(formatter))
            })
            .clone()
    }
}

/// The date of anything chrono can give a calendar date of.
pub(crate) fn naive_date(date: &impl Datelike) -> NaiveDate {
    NaiveDate::from_ymd_opt(date.year(), date.month(), date.day()).unwrap_or_default()
}

/// `usize` digits as an ICU4X magnitude.
fn to_position(digits: usize) -> i16 {
    i16::try_from(digits.min(20)).unwrap_or(20)
}

/// `value` as a decimal (`None` for NaN and the infinities).
fn to_decimal(value: f64) -> Option<Decimal> {
    if !value.is_finite() {
        return None;
    }
    // Rust prints floats in their shortest round-trip form, never with an
    // exponent.
    Decimal::from_str(&value.to_string()).ok()
}

/// Where a locale writes the currency symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placement {
    /// `$42,000.00` (a code is followed by a space: `CHF 42,000.00`).
    Before,
    /// `€ 42.000,00`, `R$ 42.000,00`.
    BeforeSpace,
    /// `42 000,00 €`.
    After,
}

fn placement(langid: &LanguageIdentifier) -> Placement {
    match langid.language.as_str() {
        "en" | "ja" | "zh" | "ko" | "th" | "tr" | "hi" | "id" | "ms" | "fil" | "vi" | "ga"
        | "cy" | "mt" | "sw" | "zu" | "ta" | "te" | "bn" | "gu" | "kn" | "ml" | "mr" | "pa"
        | "ne" | "si" | "my" | "km" | "am" => Placement::Before,
        "nl" => Placement::BeforeSpace,
        "pt" if langid.region.is_none_or(|region| region.as_str() == "BR") => {
            Placement::BeforeSpace
        }
        _ => Placement::After,
    }
}

/// The symbol of the most common currencies; others are written with
/// their code.
fn currency_symbol(code: &str) -> Option<&'static str> {
    Some(match code {
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        "JPY" => "¥",
        "INR" => "₹",
        "BRL" => "R$",
        "KRW" => "₩",
        "CNY" => "CN¥",
        "ILS" => "₪",
        "VND" => "₫",
        "NGN" => "₦",
        "PHP" => "₱",
        "UAH" => "₴",
        "PLN" => "zł",
        "TRY" => "₺",
        _ => return None,
    })
}

/// The number of decimals (ISO 4217 minor units) of `code`; 2 unless listed.
pub(crate) fn minor_units(code: &str) -> u32 {
    match code {
        "BIF" | "CLP" | "DJF" | "GNF" | "ISK" | "JPY" | "KMF" | "KRW" | "PYG" | "RWF" | "UGX"
        | "UYI" | "VND" | "VUV" | "XAF" | "XOF" | "XPF" => 0,
        "BHD" | "IQD" | "JOD" | "KWD" | "LYD" | "OMR" | "TND" => 3,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use fluent_bundle::types::FluentNumberOptions;

    use super::*;

    fn formats(tag: &str) -> Formats {
        Formats::new(&tag.parse().unwrap())
    }

    #[test]
    fn numbers_use_the_locale_separators() {
        assert_eq!(formats("en").number(1234.5), "1,234.5");
        assert_eq!(formats("de").number(1234.5), "1.234,5");
        assert_eq!(formats("fr").number(1234.5), "1\u{202f}234,5");
        assert_eq!(formats("en").number(2.0 / 3.0), "0.667");
        assert_eq!(formats("en").number(42.0), "42");
    }

    #[test]
    fn money_places_the_symbol_like_the_locale() {
        assert_eq!(formats("en").money(4_200_000, "EUR"), "€42,000.00");
        assert_eq!(formats("en").money(4_200_000, "usd"), "$42,000.00");
        assert_eq!(
            formats("fr").money(4_200_000, "EUR"),
            "42\u{202f}000,00\u{a0}€"
        );
        assert_eq!(formats("de").money(1999, "EUR"), "19,99\u{a0}€");
        assert_eq!(formats("nl").money(1999, "EUR"), "€\u{a0}19,99");
        assert_eq!(formats("pt-BR").money(1999, "BRL"), "R$\u{a0}19,99");
        assert_eq!(formats("en").money(150_000, "JPY"), "¥150,000");
        assert_eq!(formats("en").money(1999, "CHF"), "CHF\u{a0}19.99");
        assert_eq!(formats("en").money(-1999, "USD"), "-$19.99");
        assert_eq!(formats("en").money(5, "KWD"), "KWD\u{a0}0.005");
    }

    #[test]
    fn fluent_numbers_follow_their_options() {
        let number = |value, options| FluentNumber::new(value, options);
        let en = formats("en");
        assert_eq!(
            en.fluent_number(&number(
                42000.0,
                FluentNumberOptions {
                    style: FluentNumberStyle::Currency,
                    currency: Some("EUR".into()),
                    ..FluentNumberOptions::default()
                }
            )),
            Some("€42,000.00".to_owned())
        );
        assert_eq!(
            en.fluent_number(&number(
                0.256,
                FluentNumberOptions {
                    style: FluentNumberStyle::Percent,
                    ..FluentNumberOptions::default()
                }
            )),
            Some("26%".to_owned())
        );
        assert_eq!(
            en.fluent_number(&number(
                2026.0,
                FluentNumberOptions {
                    use_grouping: false,
                    ..FluentNumberOptions::default()
                }
            )),
            Some("2026".to_owned())
        );
        assert_eq!(
            en.fluent_number(&number(
                3.0,
                FluentNumberOptions {
                    minimum_fraction_digits: Some(2),
                    ..FluentNumberOptions::default()
                }
            )),
            Some("3.00".to_owned())
        );
        assert_eq!(
            en.fluent_number(&number(
                7.0,
                FluentNumberOptions {
                    minimum_integer_digits: Some(3),
                    ..FluentNumberOptions::default()
                }
            )),
            Some("007".to_owned())
        );
        assert_eq!(
            formats("fr").fluent_number(&number(
                0.5,
                FluentNumberOptions {
                    style: FluentNumberStyle::Percent,
                    ..FluentNumberOptions::default()
                }
            )),
            Some("50\u{a0}%".to_owned())
        );
    }

    #[test]
    fn dates_follow_the_locale_pattern() {
        let day = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
        assert_eq!(
            formats("en").date(day, None, DateStyle::Medium, None),
            "Oct 26, 2026"
        );
        assert_eq!(
            formats("fr").date(day, None, DateStyle::Medium, None),
            "26 oct. 2026"
        );
        assert_eq!(
            formats("de").date(day, None, DateStyle::Long, None),
            "26. Oktober 2026"
        );
        assert_eq!(
            formats("en").date(day, None, DateStyle::Short, None),
            "10/26/26"
        );
        let at = NaiveTime::from_hms_opt(14, 30, 0).unwrap();
        assert_eq!(
            formats("en").date(day, Some(at), DateStyle::Medium, Some(TimeStyle::Short)),
            "Oct 26, 2026, 2:30\u{202f}PM"
        );
        assert_eq!(
            formats("fr").date(day, Some(at), DateStyle::Medium, Some(TimeStyle::Short)),
            "26 oct. 2026, 14:30"
        );
        // A time is shown only when a time style asks for it.
        assert_eq!(
            formats("en").date(day, Some(at), DateStyle::Medium, None),
            "Oct 26, 2026"
        );
    }
}
