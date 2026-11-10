//! [`fold_digits`] and [`Folded`]: digits of any script as ASCII `0`-`9`,
//! before a value is checked or parsed.

use std::borrow::Cow;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use crate::rules::{Length, Measure, Range, RuleInput, Unit};

/// The digit zero of every run of ten decimal digits (Unicode general
/// category `Nd`) outside ASCII, in code point order. Each run holds the
/// digits 0 to 9 in order; the test below checks the table against the
/// Unicode character database.
const ZEROS: &[u32] = &[
    0x0660,  // Arabic-Indic
    0x06F0,  // Extended Arabic-Indic (Persian, Urdu)
    0x07C0,  // NKo
    0x0966,  // Devanagari
    0x09E6,  // Bengali
    0x0A66,  // Gurmukhi
    0x0AE6,  // Gujarati
    0x0B66,  // Oriya
    0x0BE6,  // Tamil
    0x0C66,  // Telugu
    0x0CE6,  // Kannada
    0x0D66,  // Malayalam
    0x0DE6,  // Sinhala Lith
    0x0E50,  // Thai
    0x0ED0,  // Lao
    0x0F20,  // Tibetan
    0x1040,  // Myanmar
    0x1090,  // Myanmar Shan
    0x17E0,  // Khmer
    0x1810,  // Mongolian
    0x1946,  // Limbu
    0x19D0,  // New Tai Lue
    0x1A80,  // Tai Tham Hora
    0x1A90,  // Tai Tham Tham
    0x1B50,  // Balinese
    0x1BB0,  // Sundanese
    0x1C40,  // Lepcha
    0x1C50,  // Ol Chiki
    0xA620,  // Vai
    0xA8D0,  // Saurashtra
    0xA900,  // Kayah Li
    0xA9D0,  // Javanese
    0xA9F0,  // Myanmar Tai Laing
    0xAA50,  // Cham
    0xABF0,  // Meetei Mayek
    0xFF10,  // Fullwidth
    0x104A0, // Osmanya
    0x10D30, // Hanifi Rohingya
    0x10D40, // Garay
    0x11066, // Brahmi
    0x110F0, // Sora Sompeng
    0x11136, // Chakma
    0x111D0, // Sharada
    0x112F0, // Khudawadi
    0x11450, // Newa
    0x114D0, // Tirhuta
    0x11650, // Modi
    0x116C0, // Takri
    0x116D0, // Myanmar Pao
    0x116DA, // Myanmar Eastern Pwo Karen
    0x11730, // Ahom
    0x118E0, // Warang Citi
    0x11950, // Dives Akuru
    0x11BF0, // Sunuwar
    0x11C50, // Bhaiksuki
    0x11D50, // Masaram Gondi
    0x11DA0, // Gunjala Gondi
    0x11DE0, // Tolong Siki
    0x11F50, // Kawi
    0x16130, // Gurung Khema
    0x16A60, // Mro
    0x16AC0, // Tangsa
    0x16B50, // Pahawh Hmong
    0x16D70, // Kirat Rai
    0x1CCF0, // Outlined digits
    0x1D7CE, // Mathematical bold
    0x1D7D8, // Mathematical double-struck
    0x1D7E2, // Mathematical sans-serif
    0x1D7EC, // Mathematical sans-serif bold
    0x1D7F6, // Mathematical monospace
    0x1E140, // Nyiakeng Puachue Hmong
    0x1E2F0, // Wancho
    0x1E4F0, // Nag Mundari
    0x1E5F1, // Ol Onal
    0x1E950, // Adlam
    0x1FBF0, // Segmented digits
];

/// The ASCII digit for `c`, when `c` is a decimal digit of another script.
fn ascii_digit(c: char) -> Option<char> {
    let code = u32::from(c);
    if code < 0x0660 {
        return None;
    }
    let run = ZEROS.partition_point(|&zero| zero <= code).checked_sub(1)?;
    let value = code - ZEROS[run];
    (value < 10).then(|| char::from(b'0' + value as u8))
}

/// `text` with every decimal digit written in another script (Arabic-Indic
/// `٤٢`, Persian `۴۲`, Devanagari `४२`, Bengali, Thai, fullwidth `４２` and
/// every other Unicode decimal digit) replaced by its ASCII digit. Nothing
/// else changes; borrowed when there is nothing to fold.
///
/// ```
/// use navree_validation::fold_digits;
///
/// assert_eq!(fold_digits("٠٥٠ ١٢٣ ٤٥٦٧"), "050 123 4567");
/// assert_eq!(fold_digits("पिन: १२३४"), "पिन: 1234");
/// assert!(matches!(fold_digits("abc 123"), std::borrow::Cow::Borrowed(_)));
/// ```
pub fn fold_digits(text: &str) -> Cow<'_, str> {
    let Some(first) = text.find(|c| ascii_digit(c).is_some()) else {
        return Cow::Borrowed(text);
    };
    let mut folded = String::with_capacity(text.len());
    folded.push_str(&text[..first]);
    folded.extend(text[first..].chars().map(|c| ascii_digit(c).unwrap_or(c)));
    Cow::Owned(folded)
}

/// A value whose digits are folded to ASCII ([`fold_digits`]) before it is
/// parsed, so rules see `123456` whether the user typed `١٢٣٤٥٦`, `۱۲۳۴۵۶`
/// or `१२३४५६`.
///
/// - `Folded<String>` (the default) keeps the folded text. The text rules
///   (`length`, `pattern`, `ascii`, `contains`, ...) and the
///   [`rules`](crate::rules) for `custom` work on it directly.
/// - `Folded<T>` for any `T: FromStr` (`u32`, `i64`, `f64`, `Decimal`, ...)
///   parses the folded text, so `"٤٢"` is `42`; a JSON number is accepted
///   too. `range` works on folded numbers.
///
/// It derefs to `T`, serializes as `T` and converts with
/// [`into_inner`](Folded::into_inner). A value that does not parse fails
/// like `T` does (`quantity must be an integer number`).
///
/// ```
/// use navree_validation::Validate;
/// use navree_validation::Folded;
/// use serde::Deserialize;
///
/// #[derive(Deserialize, Validate)]
/// struct Verify {
///     #[validate(length(min = 6, max = 6), pattern(r"^[0-9]+$"))]
///     code: Folded<String>,
///     #[validate(range(min = 1, max = 99))]
///     quantity: Option<Folded<u32>>,
/// }
///
/// let input: Verify =
///     serde_json::from_str(r#"{"code": "٦٥٤٣٢١", "quantity": "٤٢"}"#).unwrap();
/// assert_eq!(*input.code, "654321");
/// assert_eq!(input.quantity.map(Folded::into_inner), Some(42));
/// assert!(input.validate().is_ok());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Folded<T = String>(pub T);

impl<T> Folded<T> {
    /// The parsed value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for Folded<T> {
    fn from(value: T) -> Self {
        Folded(value)
    }
}

impl<T> Deref for Folded<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Folded<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: fmt::Display> fmt::Display for Folded<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: FromStr> FromStr for Folded<T> {
    type Err = T::Err;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        fold_digits(text).parse().map(Folded)
    }
}

impl<T: Serialize> Serialize for Folded<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de, T> Deserialize<'de> for Folded<T>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FoldedVisitor<T>(std::marker::PhantomData<fn() -> T>);

        impl<T> FoldedVisitor<T>
        where
            T: FromStr,
            T::Err: fmt::Display,
        {
            fn parse<E: de::Error>(text: &str) -> Result<Folded<T>, E> {
                text.parse().map_err(E::custom)
            }
        }

        impl<T> Visitor<'_> for FoldedVisitor<T>
        where
            T: FromStr,
            T::Err: fmt::Display,
        {
            type Value = Folded<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string")
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<Self::Value, E> {
                Self::parse(text)
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Self::parse(&value.to_string())
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Self::parse(&value.to_string())
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                Self::parse(&value.to_string())
            }
        }

        deserializer.deserialize_any(FoldedVisitor(std::marker::PhantomData))
    }
}

impl<T: RuleInput> RuleInput for Folded<T> {
    fn rule_str(&self) -> Option<&str> {
        self.0.rule_str()
    }
}

impl<T: Length> Length for Folded<T> {
    fn measure(&self, unit: Unit) -> Option<Measure> {
        self.0.measure(unit)
    }
}

impl<T: Range> Range for Folded<T> {
    type Bound = T::Bound;

    fn value(&self) -> Option<&T::Bound> {
        self.0.value()
    }
}

#[cfg(test)]
mod tests {
    use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

    use super::*;

    #[test]
    fn every_unicode_decimal_digit_folds() {
        assert!(ZEROS.windows(2).all(|w| w[1] >= w[0] + 10), "sorted runs");
        let mut missed = Vec::new();
        for code in 0..=0x10FFFF_u32 {
            let Some(c) = char::from_u32(code) else {
                continue;
            };
            let decimal = c.general_category() == GeneralCategory::DecimalNumber;
            match ascii_digit(c) {
                Some(digit) => {
                    assert!(decimal, "U+{code:04X} folds but is not a decimal digit");
                    // Each run starts at its zero.
                    let zero = char::from_u32(code - u32::from(digit as u8 - b'0')).unwrap();
                    assert_eq!(zero.general_category(), GeneralCategory::DecimalNumber);
                }
                None if decimal && !c.is_ascii_digit() => missed.push(format!("U+{code:04X}")),
                None => {}
            }
        }
        assert!(
            missed.is_empty(),
            "decimal digits the table misses: {missed:?}"
        );
    }

    #[test]
    fn folding_keeps_everything_else() {
        assert_eq!(fold_digits("+٩٧١ (٥٠) ١٢٣-٤٥٦٧"), "+971 (50) 123-4567");
        assert_eq!(fold_digits("۱۴۰۳/۰۵/۱۲"), "1403/05/12");
        assert_eq!(fold_digits("१२३ abc ১২৩ ๑๒๓ ４５"), "123 abc 123 123 45");
        // Digit-like characters that aren't decimal digits stay.
        assert_eq!(fold_digits("² ½ Ⅻ ①"), "² ½ Ⅻ ①");
        assert!(matches!(fold_digits("نام"), Cow::Borrowed("نام")));
    }

    #[test]
    fn folded_values_parse_from_strings_and_numbers() {
        let code: Folded = serde_json::from_str(r#""٠٠٧""#).unwrap();
        assert_eq!(code, Folded("007".to_owned()));
        let n: Folded<u32> = serde_json::from_str(r#""४२""#).unwrap();
        assert_eq!(*n, 42);
        let n: Folded<u32> = serde_json::from_str("42").unwrap();
        assert_eq!(n.into_inner(), 42);
        let x: Folded<f64> = serde_json::from_str(r#""٣.٥""#).unwrap();
        assert_eq!(*x, 3.5);
        let error = serde_json::from_str::<Folded<u32>>(r#""٤x""#).unwrap_err();
        assert!(error.to_string().contains("invalid digit"), "{error}");
        assert_eq!("٤٢".parse::<Folded<u8>>(), Ok(Folded(42)));
        assert_eq!(serde_json::to_string(&Folded(42)).unwrap(), "42");
    }
}
