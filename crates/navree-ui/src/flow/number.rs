//! [`Decimal`]: a number read from text without a float, so a slider's step of
//! `0.1` lands exactly, and nothing a person posts (`NaN`, `1e999`, 40 digits)
//! can panic or overflow.

use std::cmp::Ordering;
use std::fmt;

/// How many digits after the point a [`Decimal`] keeps.
pub const FRACTION_DIGITS: u32 = 9;
const ONE: i128 = 1_000_000_000;
/// The most digits before the point: enough for every `i64` and `u64`.
const INTEGER_DIGITS: usize = 20;
/// The largest magnitude, in units of 10^-9.
const LIMIT: i128 = 100_000_000_000_000_000_000 * ONE - 1;
/// The longest text [`Decimal::from_browser`] reads, and the largest exponent in it.
const BROWSER_BYTES: usize = 128;
const BROWSER_EXPONENT: i32 = 400;

/// Why a text is not a [`Decimal`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecimalError {
    /// Nothing there.
    Empty,
    /// Not digits with an optional sign and point: `0x10`, `12px`, `NaN`, spaces and the
    /// like. (`from_str` refuses `1e3` as well; [`Decimal::from_browser`] reads it.)
    Malformed,
    /// More than 20 digits before the point, or more than 9 after it.
    TooLong,
}

impl fmt::Display for DecimalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "There is no number.",
            Self::Malformed => "That is not a number.",
            Self::TooLong => "That number has too many digits.",
        })
    }
}

impl std::error::Error for DecimalError {}

/// A number with up to nine decimals, held as an integer count of billionths.
///
/// ```
/// use navree_ui::flow::Decimal;
///
/// let step: Decimal = "0.1".parse().unwrap();
/// let three = step.times(3).unwrap();
/// assert_eq!(three.to_string(), "0.3");
/// assert!("1e3".parse::<Decimal>().is_err());
/// assert_eq!(Decimal::from_browser("1e-7").unwrap().to_string(), "0.0000001");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Decimal(i128);

impl Decimal {
    /// Zero.
    pub const ZERO: Self = Self(0);
    /// One.
    pub const ONE: Self = Self(ONE);

    /// A whole number.
    pub const fn from_i64(value: i64) -> Self {
        Self(value as i128 * ONE)
    }

    /// A whole number.
    pub const fn from_u64(value: u64) -> Self {
        Self(value as i128 * ONE)
    }

    /// The nearest number with nine decimals. `NaN` reads as 0, and anything of 20
    /// digits or more before the point is held to the largest number there is.
    pub fn from_f64(value: f64) -> Self {
        if value.is_nan() {
            return Self::ZERO;
        }
        let scaled = value * ONE as f64;
        if scaled >= LIMIT as f64 {
            return Self(LIMIT);
        }
        if scaled <= -(LIMIT as f64) {
            return Self(-LIMIT);
        }
        Self(scaled.round() as i128)
    }

    /// A number of billionths, held inside the limit.
    pub(crate) fn from_units(units: i128) -> Self {
        Self(units.clamp(-LIMIT, LIMIT))
    }

    /// The units of 10^-9. Two numbers are equal when these are.
    pub const fn units(self) -> i128 {
        self.0
    }

    /// Whether this is below zero.
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// Whether this is zero.
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Adds, or `None` past the limit.
    pub fn plus(self, other: Self) -> Option<Self> {
        let sum = self.0.checked_add(other.0)?;
        (sum.abs() <= LIMIT).then_some(Self(sum))
    }

    /// Subtracts, or `None` past the limit.
    pub fn minus(self, other: Self) -> Option<Self> {
        let difference = self.0.checked_sub(other.0)?;
        (difference.abs() <= LIMIT).then_some(Self(difference))
    }

    /// Multiplies by a whole number, or `None` past the limit.
    pub fn times(self, count: i64) -> Option<Self> {
        let product = self.0.checked_mul(i128::from(count))?;
        (product.abs() <= LIMIT).then_some(Self(product))
    }

    /// How many whole `step`s fit in this (rounding down, for a positive step).
    /// `None` for a step of zero.
    pub fn steps_of(self, step: Self) -> Option<i128> {
        (step.0 != 0).then(|| self.0.div_euclid(step.0))
    }

    /// Whether this is a whole number of `step`s. A step of zero holds nothing.
    pub fn is_multiple_of(self, step: Self) -> bool {
        step.0 != 0 && self.0.rem_euclid(step.0) == 0
    }

    /// The nearest multiple of `step` above `origin`, rounding halves up. A step of
    /// zero changes nothing.
    pub fn snapped(self, origin: Self, step: Self) -> Self {
        if step.0 <= 0 {
            return self;
        }
        let offset = self.0 - origin.0;
        let below = offset.div_euclid(step.0);
        let rest = offset.rem_euclid(step.0);
        let count = if rest * 2 >= step.0 { below + 1 } else { below };
        Self(origin.0 + count * step.0)
    }

    /// How many decimals this needs to be written exactly: 0 to 9.
    pub fn decimals(self) -> u32 {
        let mut units = self.0.unsigned_abs() % ONE as u128;
        if units == 0 {
            return 0;
        }
        let mut digits = FRACTION_DIGITS;
        while units.is_multiple_of(10) {
            units /= 10;
            digits -= 1;
        }
        digits
    }

    /// The number as text with exactly `decimals` digits after the point (at most
    /// 9), rounded.
    pub fn fixed(self, decimals: u32) -> String {
        let decimals = decimals.min(FRACTION_DIGITS);
        let unit = 10_i128.pow(FRACTION_DIGITS - decimals);
        let rounded = (self.0.abs() + unit / 2) / unit * unit;
        let sign = if self.0 < 0 && rounded != 0 { "-" } else { "" };
        let whole = rounded / ONE;
        if decimals == 0 {
            return format!("{sign}{whole}");
        }
        let fraction = (rounded % ONE) / unit;
        format!(
            "{sign}{whole}.{fraction:0width$}",
            width = decimals as usize
        )
    }
}

impl Decimal {
    /// Reads a number the way a browser writes one: what `from_str` reads, and also a fraction
    /// of any length, which is rounded to nine places (half away from zero), and an exponent
    /// (`1e-7`, `2.5E3`). A range input that takes any value posts `0.5033333333333333`, and
    /// one with a tiny step posts `1e-7`, and neither is wrong. Still nothing else is a number:
    /// no spaces, no separators, no `NaN` or `inf`, no hex; a text over 128 bytes, an exponent
    /// past 400, or a number past 20 digits before the point is `TooLong`.
    ///
    /// ```
    /// use navree_ui::flow::Decimal;
    ///
    /// let read = |text: &str| Decimal::from_browser(text).map(|d| d.to_string());
    /// assert_eq!(read("0.5033333333333333").unwrap(), "0.503333333");
    /// assert_eq!(read("0.30000000000000004").unwrap(), "0.3");
    /// assert_eq!(read("1E3").unwrap(), "1000");
    /// assert!(read("0x10").is_err() && read("NaN").is_err() && read("1 ").is_err());
    /// ```
    pub fn from_browser(text: &str) -> Result<Self, DecimalError> {
        if text.is_empty() {
            return Err(DecimalError::Empty);
        }
        if text.len() > BROWSER_BYTES {
            return Err(DecimalError::TooLong);
        }
        let (mantissa, exponent) = match text.find(['e', 'E']) {
            Some(at) => (&text[..at], Some(&text[at + 1..])),
            None => (text, None),
        };
        let (negative, rest) = match mantissa.as_bytes().first() {
            Some(b'-') => (true, &mantissa[1..]),
            Some(b'+') => (false, &mantissa[1..]),
            _ => (false, mantissa),
        };
        let (whole, fraction) = rest.split_once('.').unwrap_or((rest, ""));
        let digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
        if (whole.is_empty() && fraction.is_empty()) || !digits(whole) || !digits(fraction) {
            return Err(DecimalError::Malformed);
        }
        let shift: i32 = match exponent {
            None => 0,
            Some(power) => {
                let (up, body) = match power.as_bytes().first() {
                    Some(b'-') => (false, &power[1..]),
                    Some(b'+') => (true, &power[1..]),
                    _ => (true, power),
                };
                if body.is_empty() || !digits(body) {
                    return Err(DecimalError::Malformed);
                }
                // At most 3 digits: the text is short, and 999 is past the limit anyway.
                let trimmed = body.trim_start_matches('0');
                let magnitude: i32 = if trimmed.is_empty() {
                    0
                } else {
                    trimmed.parse().map_err(|_| DecimalError::TooLong)?
                };
                if magnitude > BROWSER_EXPONENT {
                    return Err(DecimalError::TooLong);
                }
                if up { magnitude } else { -magnitude }
            }
        };
        // The digits in one run, and where the point falls among them once the exponent has moved it.
        let all = format!("{whole}{fraction}");
        let point = i64::try_from(whole.len()).unwrap_or(i64::MAX) + i64::from(shift);
        let len = i64::try_from(all.len()).unwrap_or(i64::MAX);
        let digit_at = |index: i64| -> i128 {
            if index < 0 || index >= len {
                0
            } else {
                i128::from(all.as_bytes()[index as usize] - b'0')
            }
        };
        // The whole part is the digits before the point (zeros when the exponent pushed it past the end).
        let mut units: i128 = 0;
        let first = point.max(0);
        // Leading zeros before the first non-zero digit do not count towards the limit.
        let mut seen = 0;
        for index in 0..first {
            let digit = digit_at(index);
            if digit == 0 && seen == 0 {
                continue;
            }
            seen += 1;
            if seen > INTEGER_DIGITS {
                return Err(DecimalError::TooLong);
            }
            units = units * 10 + digit;
        }
        units *= ONE;
        // Nine places after the point, then the tenth decides the rounding.
        let mut place = ONE / 10;
        for index in point..point + i64::from(FRACTION_DIGITS) {
            units += digit_at(index) * place;
            place /= 10;
        }
        if digit_at(point + i64::from(FRACTION_DIGITS)) >= 5 {
            units += 1;
        }
        if units > LIMIT {
            return Err(DecimalError::TooLong);
        }
        Ok(Self(if negative { -units } else { units }))
    }
}

impl std::ops::Neg for Decimal {
    type Output = Self;

    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl fmt::Display for Decimal {
    /// The shortest exact text: no trailing zeros, no exponent.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.fixed(self.decimals()))
    }
}

impl fmt::Debug for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Decimal({self})")
    }
}

impl std::str::FromStr for Decimal {
    type Err = DecimalError;

    /// Reads `-12`, `+3.5`, `.5` and `7.` as numbers. Nothing else is one: no
    /// spaces, no exponent, no separators, no more than 20 digits before the point
    /// or 9 after it.
    fn from_str(text: &str) -> Result<Self, DecimalError> {
        if text.is_empty() {
            return Err(DecimalError::Empty);
        }
        let (negative, rest) = match text.as_bytes()[0] {
            b'-' => (true, &text[1..]),
            b'+' => (false, &text[1..]),
            _ => (false, text),
        };
        let (whole, fraction) = rest.split_once('.').unwrap_or((rest, ""));
        let digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
        if (whole.is_empty() && fraction.is_empty()) || !digits(whole) || !digits(fraction) {
            return Err(DecimalError::Malformed);
        }
        let whole = whole.trim_start_matches('0');
        if whole.len() > INTEGER_DIGITS || fraction.len() > FRACTION_DIGITS as usize {
            return Err(DecimalError::TooLong);
        }
        let mut units: i128 = 0;
        for digit in whole.bytes() {
            units = units * 10 + i128::from(digit - b'0');
        }
        units *= ONE;
        let mut place = ONE / 10;
        for digit in fraction.bytes() {
            units += i128::from(digit - b'0') * place;
            place /= 10;
        }
        if units > LIMIT {
            return Err(DecimalError::TooLong);
        }
        Ok(Self(if negative { -units } else { units }))
    }
}

macro_rules! from_whole {
    ($($type:ty => $via:ident as $wide:ty),+ $(,)?) => {
        $(impl From<$type> for Decimal {
            fn from(value: $type) -> Self {
                Self::$via(<$wide>::from(value))
            }
        })+
    };
}
from_whole!(i8 => from_i64 as i64, i16 => from_i64 as i64, i32 => from_i64 as i64, i64 => from_i64 as i64,
    u8 => from_i64 as i64, u16 => from_i64 as i64, u32 => from_i64 as i64, u64 => from_u64 as u64);

impl From<f64> for Decimal {
    fn from(value: f64) -> Self {
        Self::from_f64(value)
    }
}

impl From<f32> for Decimal {
    fn from(value: f32) -> Self {
        // The shortest text of the float, as a person writes it: 0.1f32 is 0.1.
        value
            .to_string()
            .parse()
            .unwrap_or_else(|_| Self::from_f64(f64::from(value)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(text: &str) -> Decimal {
        text.parse().unwrap()
    }

    #[test]
    fn reads_and_writes_the_shortest_text() {
        for (text, back) in [
            ("0", "0"),
            ("-0", "0"),
            ("007", "7"),
            ("+3.50", "3.5"),
            (".5", "0.5"),
            ("7.", "7"),
            ("-12.000000001", "-12.000000001"),
            ("18446744073709551615", "18446744073709551615"),
            ("-9223372036854775808", "-9223372036854775808"),
        ] {
            assert_eq!(d(text).to_string(), back, "{text}");
        }
    }

    #[test]
    fn refuses_what_is_not_a_plain_number() {
        for text in [
            "", " 1", "1 ", "1e3", "1E3", "0x10", "NaN", "inf", "-", "+", ".", "1.2.3", "1,5",
            "1_000", "١٢", "--1", "1-", "12px", "\u{0}", "1\n",
        ] {
            assert!(text.parse::<Decimal>().is_err(), "{text:?}");
        }
        assert_eq!("".parse::<Decimal>(), Err(DecimalError::Empty));
        assert_eq!(
            "1.0000000001".parse::<Decimal>(),
            Err(DecimalError::TooLong)
        );
        assert_eq!(
            "100000000000000000000".parse::<Decimal>(),
            Err(DecimalError::TooLong)
        );
        assert!("99999999999999999999.999999999".parse::<Decimal>().is_ok());
    }

    #[test]
    fn floats_in_are_held_inside_and_nan_is_zero() {
        assert_eq!(Decimal::from(f64::NAN), Decimal::ZERO);
        assert_eq!(
            Decimal::from(f64::INFINITY).to_string(),
            "99999999999999999999.999999999"
        );
        assert_eq!(
            Decimal::from(f64::NEG_INFINITY).to_string(),
            "-99999999999999999999.999999999"
        );
        assert_eq!(Decimal::from(f64::MAX), Decimal::from(f64::INFINITY));
        assert_eq!(Decimal::from(0.1_f64).to_string(), "0.1");
        assert_eq!(Decimal::from(0.1_f32).to_string(), "0.1");
        assert_eq!(Decimal::from(1e-12_f64), Decimal::ZERO);
        assert_eq!(Decimal::from(i64::MIN).to_string(), "-9223372036854775808");
        assert_eq!(Decimal::from(u64::MAX).to_string(), "18446744073709551615");
    }

    #[test]
    fn arithmetic_is_exact_and_stops_at_the_limit() {
        assert_eq!(d("0.1").times(3).unwrap(), d("0.3"));
        assert_eq!(d("0.3").minus(d("0.1")).unwrap(), d("0.2"));
        let top = d("99999999999999999999.999999999");
        assert!(top.plus(d("0.000000001")).is_none());
        assert!(top.times(2).is_none());
        assert!((-top).minus(Decimal::ONE).is_none());
        assert!(top.times(i64::MAX).is_none());
    }

    #[test]
    fn multiples_and_snapping() {
        assert!(d("0.3").is_multiple_of(d("0.1")));
        assert!(!d("0.35").is_multiple_of(d("0.1")));
        assert!(!d("1").is_multiple_of(Decimal::ZERO));
        assert_eq!(d("7").snapped(d("0"), d("5")), d("5"));
        assert_eq!(d("7.5").snapped(d("0"), d("5")), d("10"));
        assert_eq!(d("-7").snapped(d("-10"), d("4")), d("-6"));
        assert_eq!(d("3").snapped(d("0"), Decimal::ZERO), d("3"));
        assert_eq!(d("12").steps_of(d("5")), Some(2));
        assert_eq!(d("12").steps_of(Decimal::ZERO), None);
    }

    #[test]
    fn fixed_rounds_and_pads() {
        assert_eq!(d("0.5").fixed(2), "0.50");
        assert_eq!(d("2.675").fixed(2), "2.68");
        assert_eq!(d("-0.004").fixed(2), "0.00");
        assert_eq!(d("-1.5").fixed(0), "-2");
        assert_eq!(d("1").fixed(12), "1.000000000");
        assert_eq!(d("0.25").decimals(), 2);
        assert_eq!(d("3").decimals(), 0);
        assert_eq!(d("0.000000001").decimals(), 9);
    }

    /// A small generator, so the properties below run the same everywhere.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    #[test]
    fn any_text_parses_or_errors_and_what_parses_reads_back() {
        let alphabet: Vec<char> = "0123456789+-.eE xX_,\u{0}\u{661}NaIn".chars().collect();
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut parsed = 0;
        for _ in 0..200_000 {
            let len = (rng.next() % 40) as usize;
            let text: String = (0..len)
                .map(|_| alphabet[(rng.next() % alphabet.len() as u64) as usize])
                .collect();
            if let Ok(value) = text.parse::<Decimal>() {
                parsed += 1;
                let written = value.to_string();
                assert_eq!(
                    written.parse::<Decimal>(),
                    Ok(value),
                    "{text:?} -> {written}"
                );
                assert!(!written.contains(['e', 'E']), "{written}");
            }
        }
        assert!(parsed > 1000, "the generator should find numbers: {parsed}");
    }

    #[test]
    fn any_grid_value_is_a_multiple_and_snapping_lands_on_the_grid() {
        let mut rng = Rng(42);
        for _ in 0..50_000 {
            let min = Decimal::from_f64((rng.next() % 2_000_000) as f64 / 1000.0 - 1000.0);
            let step = Decimal(1 + (rng.next() % 5_000_000_000) as i128);
            let value = Decimal::from_f64((rng.next() % 4_000_000) as f64 / 1000.0 - 2000.0);
            let snapped = value.snapped(min, step);
            assert!(
                snapped.minus(min).unwrap().is_multiple_of(step),
                "{min} {step} {value}"
            );
            // No farther than half a step.
            assert!(
                (snapped.units() - value.units()).abs() * 2 <= step.units(),
                "{min} {step} {value}"
            );
        }
    }

    #[test]
    fn a_browser_may_write_a_long_fraction_or_an_exponent() {
        let read = |text: &str| Decimal::from_browser(text).map(|d| d.to_string());
        for (text, back) in [
            ("0.5033333333333333", "0.503333333"),
            ("0.30000000000000004", "0.3"),
            ("0.123456789", "0.123456789"),
            ("0.1234567895", "0.12345679"),
            ("0.9999999999", "1"),
            ("-0.9999999999", "-1"),
            ("1e-7", "0.0000001"),
            ("1E-7", "0.0000001"),
            ("1.5e+3", "1500"),
            ("2.5e-1", "0.25"),
            ("-1e-10", "0"),
            ("5e-10", "0.000000001"),
            ("4e-10", "0"),
            ("0.000000000000001e9", "0.000001"),
            ("12345e-3", "12.345"),
            ("1e0", "1"),
            ("0e5", "0"),
            (".5e1", "5"),
            ("5.e0", "5"),
            ("+7", "7"),
            ("10000000000000000000", "10000000000000000000"),
            ("0.00000000000000000000000000000001e32", "1"),
        ] {
            assert_eq!(read(text).as_deref(), Ok(back), "{text}");
        }
        for text in [
            "", "e5", "1e", "1e+", "1e-", "1e1.5", "1ee1", "0x10", "NaN", "inf", "-inf",
            "Infinity", " 1", "1 ", "1,5", "1_000", "--1", "+-1", "1.2.3", "\u{661}", "1\u{0}",
            ".", "-", "+", "-.", ".e1",
        ] {
            assert!(read(text).is_err(), "{text:?} should not be a number");
        }
        // Too big to hold, however it is written.
        for text in [
            "1e30",
            "1e21",
            "123456789012345678901",
            "1e400",
            "1e401",
            "1e99999999999",
            &"9".repeat(129),
        ] {
            assert_eq!(read(text), Err(DecimalError::TooLong), "{text}");
        }
        assert!(read("1e20").is_err(), "the 21st digit is past the limit");
        assert_eq!(read("1e19").as_deref(), Ok("10000000000000000000"));
    }

    #[test]
    fn any_text_is_read_by_a_browser_or_refused_and_what_is_read_reads_back() {
        let alphabet: Vec<char> = "0123456789+-.eE xX_,\u{0}\u{661}NaIn".chars().collect();
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        let mut read = 0;
        for _ in 0..200_000 {
            let len = (rng.next() % 60) as usize;
            let text: String = (0..len)
                .map(|_| alphabet[(rng.next() % alphabet.len() as u64) as usize])
                .collect();
            if let Ok(value) = Decimal::from_browser(&text) {
                read += 1;
                // Never an exponent once read, and the shortest text reads back as itself.
                let written = value.to_string();
                assert!(!written.contains(['e', 'E']), "{text:?} -> {written}");
                assert_eq!(
                    written.parse::<Decimal>(),
                    Ok(value),
                    "{text:?} -> {written}"
                );
            }
        }
        assert!(read > 1000, "the generator should find numbers: {read}");
        // What `from_str` reads, `from_browser` reads the same.
        for text in ["0", "-12.5", "+3.25", ".5", "7.", "000123", "-0.000000001"] {
            assert_eq!(
                Decimal::from_browser(text),
                text.parse::<Decimal>(),
                "{text}"
            );
        }
    }
}
