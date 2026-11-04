//! Numbers as a reader of a language writes them: [`NumberLocale`] holds the marks,
//! [`NumberFormat`] says what a number is (a count, a share, money, a duration).
//!
//! Digits are always Latin. A chart formats what the server computed and never
//! rounds a figure it then adds up: the table twin prints every value in full.

use std::borrow::Cow;

/// How digits are grouped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Grouping {
    /// Every three digits: 1,234,567.
    #[default]
    Thousands,
    /// Three, then every two: 12,34,567 (India).
    Indian,
    /// No grouping.
    None,
}

/// One step of a compact number: `power` of ten and what it is written as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactStep {
    /// The power of ten the step divides by: 3 for thousands.
    pub power: u8,
    /// What follows the number: `K`, ` Mio.`, `万`.
    pub suffix: Cow<'static, str>,
}

const fn step(power: u8, suffix: &'static str) -> CompactStep {
    CompactStep {
        power,
        suffix: Cow::Borrowed(suffix),
    }
}

/// The marks a language writes numbers with.
///
/// ```
/// use navree_ui::charts::{NumberFormat, NumberLocale};
///
/// let n = NumberFormat::number();
/// assert_eq!(n.format(1234567.5, &NumberLocale::of("en")), "1,234,567.5");
/// assert_eq!(n.format(1234567.5, &NumberLocale::of("de")), "1.234.567,5");
/// assert_eq!(n.format(1234567.0, &NumberLocale::of("en-IN")), "12,34,567");
/// assert_eq!(NumberFormat::percent().format(0.125, &NumberLocale::of("fr")), "12,5\u{a0}%");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumberLocale {
    /// The decimal mark.
    pub decimal: Cow<'static, str>,
    /// The mark between groups of digits.
    pub group: Cow<'static, str>,
    /// How digits are grouped.
    pub grouping: Grouping,
    /// The fewest digits before the first group mark: 2 writes 1234 but 12,345.
    pub min_grouping: u8,
    /// What stands between a number and `%`: nothing, or a no-break space.
    pub percent_gap: Cow<'static, str>,
    /// The steps of compact numbers, smallest first.
    pub compact: Cow<'static, [CompactStep]>,
    /// The minus sign.
    pub minus: Cow<'static, str>,
}

const EN_COMPACT: &[CompactStep] = &[step(3, "K"), step(6, "M"), step(9, "B"), step(12, "T")];
const IN_COMPACT: &[CompactStep] = &[step(3, "K"), step(5, "L"), step(7, "Cr")];
const HI_COMPACT: &[CompactStep] = &[
    step(3, "\u{a0}हज़ार"),
    step(5, "\u{a0}लाख"),
    step(7, "\u{a0}क॰"),
];
const DE_COMPACT: &[CompactStep] = &[
    step(3, "\u{a0}Tsd."),
    step(6, "\u{a0}Mio."),
    step(9, "\u{a0}Mrd."),
    step(12, "\u{a0}Bio."),
];
const FR_COMPACT: &[CompactStep] = &[
    step(3, "\u{a0}k"),
    step(6, "\u{a0}M"),
    step(9, "\u{a0}Md"),
    step(12, "\u{a0}Bn"),
];
const ES_COMPACT: &[CompactStep] = &[
    step(3, "\u{a0}mil"),
    step(6, "\u{a0}M"),
    step(9, "\u{a0}mil\u{a0}M"),
    step(12, "\u{a0}B"),
];
const IT_COMPACT: &[CompactStep] = &[
    step(6, "\u{a0}Mln"),
    step(9, "\u{a0}Mrd"),
    step(12, "\u{a0}Bln"),
];
const PT_COMPACT: &[CompactStep] = &[
    step(3, "\u{a0}mil"),
    step(6, "\u{a0}mi"),
    step(9, "\u{a0}bi"),
    step(12, "\u{a0}tri"),
];
const NL_COMPACT: &[CompactStep] = &[
    step(3, "K"),
    step(6, "\u{a0}mln."),
    step(9, "\u{a0}mld."),
    step(12, "\u{a0}bln."),
];
const RU_COMPACT: &[CompactStep] = &[
    step(3, "\u{a0}тыс."),
    step(6, "\u{a0}млн"),
    step(9, "\u{a0}млрд"),
    step(12, "\u{a0}трлн"),
];
const AR_COMPACT: &[CompactStep] = &[
    step(3, "\u{a0}ألف"),
    step(6, "\u{a0}مليون"),
    step(9, "\u{a0}مليار"),
    step(12, "\u{a0}ترليون"),
];
const JA_COMPACT: &[CompactStep] = &[step(4, "万"), step(8, "億"), step(12, "兆")];
const ZH_COMPACT: &[CompactStep] = &[step(4, "万"), step(8, "亿"), step(12, "万亿")];
const KO_COMPACT: &[CompactStep] = &[step(3, "천"), step(4, "만"), step(8, "억"), step(12, "조")];
const TR_COMPACT: &[CompactStep] = &[
    step(3, "\u{a0}B"),
    step(6, "\u{a0}Mn"),
    step(9, "\u{a0}Mr"),
    step(12, "\u{a0}Tn"),
];
const PL_COMPACT: &[CompactStep] = &[
    step(3, "\u{a0}tys."),
    step(6, "\u{a0}mln"),
    step(9, "\u{a0}mld"),
    step(12, "\u{a0}bln"),
];
const SV_COMPACT: &[CompactStep] = &[
    step(3, "\u{a0}tn"),
    step(6, "\u{a0}mn"),
    step(9, "\u{a0}md"),
    step(12, "\u{a0}bn"),
];

impl Default for NumberLocale {
    fn default() -> Self {
        Self::english()
    }
}

impl NumberLocale {
    /// English: `1,234.5`, `12.5%`, `1.2K`.
    pub const fn english() -> Self {
        Self {
            decimal: Cow::Borrowed("."),
            group: Cow::Borrowed(","),
            grouping: Grouping::Thousands,
            min_grouping: 1,
            percent_gap: Cow::Borrowed(""),
            compact: Cow::Borrowed(EN_COMPACT),
            minus: Cow::Borrowed("-"),
        }
    }

    /// The marks for a tag such as `en`, `en-IN`, `de-CH` or `pt_BR`. A language the
    /// table doesn't hold writes English marks.
    pub fn of(tag: &str) -> Self {
        let tag = tag.replace('_', "-").to_ascii_lowercase();
        let mut parts = tag.split('-');
        let language = parts.next().unwrap_or_default();
        let region = parts.find(|part| part.len() == 2);
        let marks = |decimal: &'static str, group: &'static str| Self {
            decimal: Cow::Borrowed(decimal),
            group: Cow::Borrowed(group),
            ..Self::english()
        };
        let mut out = match language {
            "en" if region == Some("in") => Self {
                grouping: Grouping::Indian,
                compact: Cow::Borrowed(IN_COMPACT),
                ..Self::english()
            },
            "hi" | "mr" | "bn" | "ta" | "te" | "gu" | "kn" | "ml" | "pa" => Self {
                grouping: Grouping::Indian,
                compact: Cow::Borrowed(HI_COMPACT),
                ..Self::english()
            },
            "de" if region == Some("ch") => Self {
                group: Cow::Borrowed("’"),
                compact: Cow::Borrowed(DE_COMPACT),
                percent_gap: Cow::Borrowed(""),
                ..Self::english()
            },
            "de" => Self {
                compact: Cow::Borrowed(DE_COMPACT),
                percent_gap: Cow::Borrowed("\u{a0}"),
                ..marks(",", ".")
            },
            "fr" => Self {
                compact: Cow::Borrowed(FR_COMPACT),
                percent_gap: Cow::Borrowed("\u{a0}"),
                ..marks(",", "\u{202f}")
            },
            "es" => Self {
                min_grouping: 2,
                compact: Cow::Borrowed(ES_COMPACT),
                percent_gap: Cow::Borrowed("\u{a0}"),
                ..marks(",", ".")
            },
            "it" => Self {
                compact: Cow::Borrowed(IT_COMPACT),
                ..marks(",", ".")
            },
            "pt" if region == Some("pt") => Self {
                min_grouping: 2,
                compact: Cow::Borrowed(PT_COMPACT),
                ..marks(",", "\u{a0}")
            },
            "pt" => Self {
                compact: Cow::Borrowed(PT_COMPACT),
                ..marks(",", ".")
            },
            "nl" => Self {
                compact: Cow::Borrowed(NL_COMPACT),
                ..marks(",", ".")
            },
            "ru" | "uk" => Self {
                compact: Cow::Borrowed(RU_COMPACT),
                percent_gap: Cow::Borrowed("\u{a0}"),
                ..marks(",", "\u{a0}")
            },
            "pl" => Self {
                min_grouping: 2,
                compact: Cow::Borrowed(PL_COMPACT),
                percent_gap: Cow::Borrowed("\u{a0}"),
                ..marks(",", "\u{a0}")
            },
            "sv" | "nb" | "no" | "da" | "fi" => Self {
                compact: Cow::Borrowed(SV_COMPACT),
                percent_gap: Cow::Borrowed("\u{a0}"),
                minus: Cow::Borrowed("\u{2212}"),
                ..marks(",", "\u{a0}")
            },
            "tr" => Self {
                compact: Cow::Borrowed(TR_COMPACT),
                ..marks(",", ".")
            },
            "ar" | "fa" | "ur" | "he" => Self {
                compact: Cow::Borrowed(AR_COMPACT),
                ..Self::english()
            },
            "ja" => Self {
                compact: Cow::Borrowed(JA_COMPACT),
                ..Self::english()
            },
            "zh" => Self {
                compact: Cow::Borrowed(ZH_COMPACT),
                ..Self::english()
            },
            "ko" => Self {
                compact: Cow::Borrowed(KO_COMPACT),
                ..Self::english()
            },
            _ => Self::english(),
        };
        if language == "he" {
            out.compact = Cow::Borrowed(EN_COMPACT);
        }
        out
    }
}

/// What kind of number a value is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Kind {
    #[default]
    Number,
    /// A fraction shown as a percentage: 0.125 is 12.5%.
    Percent,
    /// A value already on 0 to 100, shown with `%`.
    Points,
}

/// When a number is written short.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Compact {
    /// On an axis that reaches 10,000; never in a table or a tooltip.
    #[default]
    Auto,
    /// Always short: `12K`.
    Always,
    /// Always in full.
    Never,
}

/// How a chart writes its values.
///
/// ```
/// use navree_ui::charts::{NumberFormat, NumberLocale};
///
/// let en = NumberLocale::english();
/// assert_eq!(NumberFormat::number().format(1234.0, &en), "1,234");
/// assert_eq!(NumberFormat::number().format(0.125, &en), "0.13");
/// assert_eq!(NumberFormat::decimals(1).format(2.0, &en), "2");
/// assert_eq!(NumberFormat::decimals(1).fixed().format(2.0, &en), "2.0");
/// assert_eq!(NumberFormat::currency("$").format(-1200.5, &en), "-$1,200.5");
/// assert_eq!(NumberFormat::number().suffix(" ms").format(85.0, &en), "85 ms");
/// assert_eq!(NumberFormat::number().format_compact(12_345.0, &en), "12.3K");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct NumberFormat {
    kind: Kind,
    max_decimals: Option<u8>,
    fixed: bool,
    prefix: Cow<'static, str>,
    suffix: Cow<'static, str>,
    compact: Compact,
}

/// The most decimal places any format writes.
const MAX_DECIMALS: u8 = 12;

impl NumberFormat {
    /// A plain number: whole numbers as they are, others with up to two decimals.
    pub fn number() -> Self {
        Self::default()
    }

    /// A count: whole numbers, rounded.
    pub fn integer() -> Self {
        Self {
            max_decimals: Some(0),
            ..Self::default()
        }
    }

    /// Up to `places` decimals, trailing zeros dropped (see [`fixed`](Self::fixed)).
    pub fn decimals(places: u8) -> Self {
        Self {
            max_decimals: Some(places.min(MAX_DECIMALS)),
            ..Self::default()
        }
    }

    /// A fraction as a percentage: `0.125` is `12.5%`. One decimal at most unless set.
    pub fn percent() -> Self {
        Self {
            kind: Kind::Percent,
            max_decimals: Some(1),
            ..Self::default()
        }
    }

    /// A value already from 0 to 100, written with `%`.
    pub fn percent_of_100() -> Self {
        Self {
            kind: Kind::Points,
            max_decimals: Some(1),
            ..Self::default()
        }
    }

    /// Money: `symbol` before the number, two decimals at most.
    pub fn currency(symbol: impl Into<Cow<'static, str>>) -> Self {
        Self {
            prefix: symbol.into(),
            max_decimals: Some(2),
            ..Self::default()
        }
    }

    /// Writes exactly the set number of decimals, keeping trailing zeros.
    pub fn fixed(mut self) -> Self {
        self.fixed = true;
        self
    }

    /// Sets the most decimal places.
    pub fn max_decimals(mut self, places: u8) -> Self {
        self.max_decimals = Some(places.min(MAX_DECIMALS));
        self
    }

    /// Text before the number, after any sign: `$`.
    pub fn prefix(mut self, text: impl Into<Cow<'static, str>>) -> Self {
        self.prefix = text.into();
        self
    }

    /// Text after the number: ` ms`, ` GB`.
    pub fn suffix(mut self, text: impl Into<Cow<'static, str>>) -> Self {
        self.suffix = text.into();
        self
    }

    /// Sets when the number is written short.
    pub fn compact(mut self, compact: Compact) -> Self {
        self.compact = compact;
        self
    }

    /// Whether an axis reaching `top` writes its ticks short.
    pub(crate) fn axis_compact(&self, top: f64) -> bool {
        match self.compact {
            Compact::Always => true,
            Compact::Never => false,
            Compact::Auto => self.kind == Kind::Number && top.abs() >= 10_000.0,
        }
    }

    /// Whether a value in a table or a tooltip is written short.
    pub(crate) fn always_compact(&self) -> bool {
        self.compact == Compact::Always
    }

    /// Whether this writes whole numbers only, so an axis steps by one at least.
    pub(crate) fn whole(&self) -> bool {
        self.max_decimals == Some(0) && self.kind == Kind::Number
    }

    /// Whether this writes percentages.
    pub(crate) fn is_percent(&self) -> bool {
        matches!(self.kind, Kind::Percent | Kind::Points)
    }

    /// `value` in full.
    pub fn format(&self, value: f64, locale: &NumberLocale) -> String {
        if self.always_compact() {
            return self.format_compact(value, locale);
        }
        self.format_with(value, locale, None)
    }

    /// `value` with exactly `places` decimals at most: an axis writes every tick
    /// with the decimals its step needs.
    pub(crate) fn format_places(&self, value: f64, locale: &NumberLocale, places: u8) -> String {
        if self.always_compact() {
            return self.format_compact(value, locale);
        }
        self.format_with(value, locale, Some(places))
    }

    fn format_with(&self, value: f64, locale: &NumberLocale, places: Option<u8>) -> String {
        if !value.is_finite() {
            return "–".to_owned();
        }
        let shown = self.shown(value);
        let places = places.or(self.max_decimals);
        let body = digits(shown.abs(), places, self.fixed, locale);
        self.wrap(
            shown < 0.0 && body.bytes().any(|b| matches!(b, b'1'..=b'9')),
            &body,
            locale,
        )
    }

    /// `value` short: `1.2K`, `12K`, `120K`, `1.2M`.
    pub fn format_compact(&self, value: f64, locale: &NumberLocale) -> String {
        if !value.is_finite() {
            return "–".to_owned();
        }
        let shown = self.shown(value);
        let size = shown.abs();
        let steps = &locale.compact;
        let mut chosen: Option<usize> = None;
        for (at, step) in steps.iter().enumerate() {
            if size >= 10f64.powi(i32::from(step.power)) {
                chosen = Some(at);
            }
        }
        let Some(mut at) = chosen else {
            return self.format(value, locale);
        };
        // Past a thousand of the largest step, an exponent says more than more digits.
        if size >= 10f64.powi(i32::from(steps[steps.len() - 1].power) + 3) {
            let text = format!("{size:.1e}");
            let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
            let body = format!("{}E{exponent}", mantissa.replace('.', &locale.decimal));
            return self.wrap(shown < 0.0, &body, locale);
        }
        loop {
            let scaled = size / 10f64.powi(i32::from(steps[at].power));
            let places = if scaled < 100.0 { 1 } else { 0 };
            let rounded = round_to(scaled, places);
            // 999,950 is 1M, not 1000K.
            if let Some(next) = steps.get(at + 1) {
                let gap = i32::from(next.power) - i32::from(steps[at].power);
                if rounded >= 10f64.powi(gap) {
                    at += 1;
                    continue;
                }
            }
            let mut body = digits(scaled, Some(places), false, locale);
            body.push_str(&steps[at].suffix);
            let negative = shown < 0.0 && body.bytes().any(|b| matches!(b, b'1'..=b'9'));
            return self.wrap(negative, &body, locale);
        }
    }

    /// The number written: a fraction as a percentage, never past the largest float.
    fn shown(&self, value: f64) -> f64 {
        let shown = match self.kind {
            Kind::Percent => value * 100.0,
            Kind::Number | Kind::Points => value,
        };
        if shown.is_finite() {
            shown
        } else {
            f64::MAX.copysign(shown)
        }
    }

    fn wrap(&self, negative: bool, body: &str, locale: &NumberLocale) -> String {
        let mut out = String::with_capacity(body.len() + self.prefix.len() + self.suffix.len() + 4);
        if negative {
            out.push_str(&locale.minus);
        }
        out.push_str(&self.prefix);
        out.push_str(body);
        if self.is_percent() {
            out.push_str(&locale.percent_gap);
            out.push('%');
        }
        out.push_str(&self.suffix);
        out
    }
}

/// `value` rounded to `places` decimals.
fn round_to(value: f64, places: u8) -> f64 {
    let factor = 10f64.powi(i32::from(places));
    (value * factor).round() / factor
}

/// A non-negative `value` as digits with the locale's marks: `places` decimals at most
/// (two when not given, none for a whole number), trailing zeros dropped unless `fixed`.
fn digits(value: f64, places: Option<u8>, fixed: bool, locale: &NumberLocale) -> String {
    // A number this large has no digits worth grouping: write it with an exponent.
    if value >= 1e21 {
        let text = format!("{value:.2e}");
        let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        return format!("{}E{exponent}", mantissa.replace('.', &locale.decimal));
    }
    let digits_after = places.unwrap_or(if value.fract() == 0.0 { 0 } else { 2 });
    // Halves round away from zero, as people round: 1.25 is 1.3, 2.5 is 3.
    let rounded = round_to(value, digits_after);
    let value = if rounded.is_finite() { rounded } else { value };
    let places = usize::from(digits_after);
    let text = format!("{value:.places$}");
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let fraction = if fixed {
        fraction
    } else {
        fraction.trim_end_matches('0')
    };
    let mut out = group(whole, locale);
    if !fraction.is_empty() {
        out.push_str(&locale.decimal);
        out.push_str(fraction);
    }
    out
}

/// The whole part with group marks.
fn group(whole: &str, locale: &NumberLocale) -> String {
    let len = whole.len();
    if locale.grouping == Grouping::None
        || len < 4
        || len < 4 + usize::from(locale.min_grouping) - 1
    {
        return whole.to_owned();
    }
    // Where the marks go, counted from the right.
    let mut cuts = Vec::new();
    let mut at = 3;
    while at < len {
        cuts.push(len - at);
        at += if locale.grouping == Grouping::Indian {
            2
        } else {
            3
        };
    }
    let mut out = String::with_capacity(len + cuts.len() * locale.group.len());
    for (i, c) in whole.char_indices() {
        if cuts.contains(&i) {
            out.push_str(&locale.group);
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_write_their_own_marks() {
        let n = NumberFormat::number();
        let f = |tag: &str, v: f64| n.format(v, &NumberLocale::of(tag));
        assert_eq!(f("en", 1234567.25), "1,234,567.25");
        assert_eq!(f("en-GB", 1234.5), "1,234.5");
        assert_eq!(f("en-IN", 12345678.0), "1,23,45,678");
        assert_eq!(f("hi", 138000.0), "1,38,000");
        assert_eq!(f("de", 1234.5), "1.234,5");
        assert_eq!(f("de-CH", 1234.5), "1’234.5");
        assert_eq!(f("fr", 1234.5), "1\u{202f}234,5");
        assert_eq!(f("es", 1234.0), "1234");
        assert_eq!(f("es", 12345.0), "12.345");
        assert_eq!(f("pl", 1234.0), "1234");
        assert_eq!(f("ar", 1234.5), "1,234.5");
        assert_eq!(f("sv", -1234.0), "\u{2212}1\u{a0}234");
        assert_eq!(f("xx", 1234.0), "1,234");
        assert_eq!(f("en", 999.0), "999");
    }

    #[test]
    fn percentages_follow_the_language() {
        let p = NumberFormat::percent();
        assert_eq!(p.format(0.125, &NumberLocale::of("en")), "12.5%");
        assert_eq!(p.format(0.125, &NumberLocale::of("fr")), "12,5\u{a0}%");
        assert_eq!(p.format(0.125, &NumberLocale::of("de")), "12,5\u{a0}%");
        assert_eq!(p.format(1.0, &NumberLocale::of("en")), "100%");
        assert_eq!(
            NumberFormat::percent_of_100().format(42.0, &NumberLocale::of("en")),
            "42%"
        );
    }

    #[test]
    fn compact_numbers_move_up_a_step_when_rounding_reaches_it() {
        let en = NumberLocale::english();
        let n = NumberFormat::number();
        assert_eq!(n.format_compact(999.0, &en), "999");
        assert_eq!(n.format_compact(1_000.0, &en), "1K");
        assert_eq!(n.format_compact(1_250.0, &en), "1.3K");
        assert_eq!(n.format_compact(123_456.0, &en), "123K");
        assert_eq!(n.format_compact(999_950.0, &en), "1M");
        assert_eq!(n.format_compact(-2_500_000.0, &en), "-2.5M");
        assert_eq!(n.format_compact(1e14, &en), "100T");
        assert_eq!(n.format_compact(1e15, &en), "1.0E15");
        assert_eq!(
            n.format_compact(150_000.0, &NumberLocale::of("en-IN")),
            "1.5L"
        );
        assert_eq!(
            n.format_compact(25_000_000.0, &NumberLocale::of("en-IN")),
            "2.5Cr"
        );
        assert_eq!(n.format_compact(12_000.0, &NumberLocale::of("ja")), "1.2万");
        assert_eq!(
            n.format_compact(1_500_000.0, &NumberLocale::of("de")),
            "1,5\u{a0}Mio."
        );
    }

    #[test]
    fn extreme_values_never_panic_and_stay_readable() {
        let en = NumberLocale::english();
        let n = NumberFormat::number();
        assert_eq!(n.format(f64::NAN, &en), "–");
        assert_eq!(NumberFormat::percent().format(f64::MIN, &en), "-1.8E308%");
        assert_eq!(n.format(f64::INFINITY, &en), "–");
        assert_eq!(n.format(f64::MAX, &en), "1.8E308");
        assert_eq!(n.format(-1e25, &en), "-1E25");
        assert_eq!(n.format(1e-12, &en), "0");
        assert_eq!(n.format(-0.001, &en), "0", "no minus on a zero");
        assert_eq!(n.format_compact(f64::MAX, &en), "1.8E308");
        assert_eq!(
            NumberFormat::decimals(12).format(1e-12, &en),
            "0.000000000001"
        );
        assert_eq!(NumberFormat::decimals(200).format(1.5, &en), "1.5");
        assert_eq!(n.format(1e20, &en), "100,000,000,000,000,000,000");
    }

    #[test]
    fn fixed_decimals_and_affixes() {
        let en = NumberLocale::english();
        assert_eq!(NumberFormat::decimals(2).fixed().format(3.0, &en), "3.00");
        assert_eq!(NumberFormat::integer().format(2.5, &en), "3");
        assert_eq!(NumberFormat::currency("€").format(19.999, &en), "€20");
        assert_eq!(
            NumberFormat::number()
                .prefix("+")
                .suffix(" pts")
                .format(4.0, &en),
            "+4 pts"
        );
        assert_eq!(NumberFormat::number().format_places(2.5, &en, 1), "2.5");
        assert_eq!(NumberFormat::number().format_places(2.0, &en, 1), "2");
    }
}
