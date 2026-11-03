//! Reading what people type: digits of any script, the start and end of a
//! number in surrounding text, `tel:` URIs, extensions, letters on a keypad,
//! and whether the text could be a phone number at all.
//!
//! These follow libphonenumber's rules for the same steps, so text reads
//! the way it does in the reference library. Every step is linear in its
//! input, or bounded by a small constant where it looks back.

use std::borrow::Cow;

/// The digit zero of every run of ten decimal digits (Unicode general
/// category `Nd`) outside ASCII, in code point order. Each run holds the
/// digits 0 to 9 in order; a test checks the table against the Unicode
/// character database.
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
    let value = code.checked_sub(*ZEROS.get(run)?)?;
    u8::try_from(value)
        .ok()
        .filter(|value| *value < 10)
        .map(|value| char::from(b'0' + value))
}

/// `text` with every decimal digit of another script (Arabic-Indic `٤٢`,
/// Devanagari `४२`, fullwidth `４２`, ...) written as its ASCII digit.
pub(crate) fn fold_digits(text: &str) -> Cow<'_, str> {
    let Some(first) = text.find(|c| ascii_digit(c).is_some()) else {
        return Cow::Borrowed(text);
    };
    let mut folded = String::with_capacity(text.len());
    folded.push_str(text.get(..first).unwrap_or(""));
    folded.extend(
        text.get(first..)
            .unwrap_or("")
            .chars()
            .map(|c| ascii_digit(c).unwrap_or(c)),
    );
    Cow::Owned(folded)
}

/// The plus signs a number may start with.
pub(crate) fn is_plus(c: char) -> bool {
    matches!(c, '+' | '\u{FF0B}')
}

/// Punctuation people write between digits: dashes, spaces, brackets, dots,
/// slashes and tildes, in their ASCII, fullwidth and typographic forms, and
/// `x`.
pub(crate) fn is_punctuation(c: char) -> bool {
    matches!(
        c,
        '-' | 'x'
            | 'X'
            | '\u{2010}'..='\u{2015}'
            | '\u{2212}'
            | '\u{30FC}'
            | '\u{FF0D}'..='\u{FF0F}'
            | ' '
            | '\u{00A0}'
            | '\u{00AD}'
            | '\u{200B}'
            | '\u{2060}'
            | '\u{3000}'
            | '('
            | ')'
            | '\u{FF08}'
            | '\u{FF09}'
            | '\u{FF3B}'
            | '\u{FF3D}'
            | '.'
            | '['
            | ']'
            | '/'
            | '~'
            | '\u{2053}'
            | '\u{223C}'
            | '\u{FF5E}'
    )
}

/// What could be a number in `text` (with folded digits): from the first
/// plus sign or digit to the last letter, digit or `#`, cut before a second
/// number written as `/ x...` or `\ x...`.
pub(crate) fn extract(text: &str) -> &str {
    let Some(start) = text.find(|c: char| is_plus(c) || c.is_ascii_digit()) else {
        return "";
    };
    let text = text.get(start..).unwrap_or("");
    let end = text
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_alphanumeric() || *c == '#')
        .map_or(0, |(at, c)| at + c.len_utf8());
    let text = text.get(..end).unwrap_or("");
    // `/ x` or `\ x`: the start of another number.
    let bytes = text.as_bytes();
    for (at, byte) in bytes.iter().enumerate() {
        if matches!(byte, b'/' | b'\\') {
            let rest = bytes.get(at + 1..).unwrap_or(&[]);
            let spaces = rest.iter().take_while(|b| **b == b' ').count();
            if rest.get(spaces) == Some(&b'x') {
                return text.get(..at).unwrap_or("");
            }
        }
    }
    text
}

/// RFC 3966: `tel:` and `;phone-context=`. The national number of `text`
/// for parsing, or `None` when its phone context is malformed.
pub(crate) fn national_for_parsing(text: &str) -> Option<String> {
    const CONTEXT: &str = ";phone-context=";
    let mut out = match text.find(CONTEXT) {
        Some(at) => {
            let after = text.get(at + CONTEXT.len()..).unwrap_or("");
            let context = after.split(';').next().unwrap_or("");
            if !is_valid_context(context) {
                return None;
            }
            let mut out = String::with_capacity(text.len());
            // A context that is a number prefix comes first; a domain adds nothing.
            if context.starts_with('+') {
                out.push_str(context);
            }
            let start = text.find("tel:").map_or(0, |tel| tel + 4).min(at);
            out.push_str(text.get(start..at).unwrap_or(""));
            out
        }
        None => extract(text).to_owned(),
    };
    // An ISDN subaddress and everything after it.
    if let Some(at) = out.find(";isub=").filter(|at| *at > 0) {
        out.truncate(at);
    }
    Some(out)
}

/// A `phone-context` is a global number (`+` then digits and visual
/// separators) or a domain name.
fn is_valid_context(context: &str) -> bool {
    if context.is_empty() {
        return false;
    }
    if let Some(digits) = context.strip_prefix('+') {
        return digits.bytes().any(|b| b.is_ascii_digit())
            && digits
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'-' | b'.' | b'(' | b')'));
    }
    let context = context.strip_suffix('.').unwrap_or(context);
    let label_ok = |label: &str| {
        !label.is_empty()
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    };
    let top_ok = context
        .rsplit('.')
        .next()
        .and_then(|top| top.bytes().next())
        .is_some_and(|b| b.is_ascii_alphabetic());
    top_ok && context.split('.').all(label_ok)
}

/// Whether `text` could be a phone number: exactly two digits, or plus
/// signs, then at least three digits among punctuation, then punctuation,
/// letters and digits, optionally followed by an extension.
pub(crate) fn is_viable(text: &str) -> bool {
    if text.chars().nth(1).is_none() {
        return false;
    }
    is_main_part(text)
        || extension_start(text)
            .is_some_and(|(start, _)| is_main_part(text.get(..start).unwrap_or("")))
}

fn is_main_part(text: &str) -> bool {
    if text.len() == 2 && text.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    let rest = text.trim_start_matches(is_plus);
    let mut digits = 0;
    for c in rest.chars() {
        if c.is_ascii_digit() {
            digits += 1;
        } else if !(is_punctuation(c) || c == '*' || (digits >= 3 && c.is_ascii_alphabetic())) {
            return false;
        }
    }
    digits >= 3
}

/// The most characters looked at before an extension's digits for its
/// label and separators. Real ones use a few; the cap keeps the search
/// linear.
const EXTENSION_LOOKBACK: usize = 40;

/// The extension at the end of `text`: the byte where it starts (its label
/// or separator) and its digits. The start is the leftmost from which an
/// extension pattern reaches the end, within [`EXTENSION_LOOKBACK`]
/// characters of the digits.
pub(crate) fn extension_start(text: &str) -> Option<(usize, String)> {
    // Only the end of the text can hold an extension.
    let tail_start = text
        .char_indices()
        .rev()
        .nth(EXTENSION_LOOKBACK + 24)
        .map_or(0, |(at, _)| at);
    let tail = text.get(tail_start..).unwrap_or("");
    let chars: Vec<(usize, char)> = tail.char_indices().collect();
    // The digits at the end, before an optional `#`.
    let mut end = chars.len();
    if chars
        .last()
        .is_some_and(|(_, c)| matches!(c, '#' | '\u{FF03}'))
    {
        end -= 1;
    }
    let digits_start = chars
        .get(..end)?
        .iter()
        .rposition(|(_, c)| !c.is_ascii_digit())
        .map_or(0, |at| at + 1);
    if digits_start == end || digits_start == 0 {
        return None;
    }
    let lowest = digits_start.saturating_sub(EXTENSION_LOOKBACK);
    let lower: Vec<char> = chars.iter().map(|(_, c)| lower(*c)).collect();
    for start in lowest..digits_start {
        if let Some(digits) = extension_at(&lower, start) {
            let byte = chars.get(start).map_or(tail.len(), |(at, _)| *at);
            return Some((tail_start + byte, digits));
        }
    }
    None
}

/// `c` in lower case, when that is one character.
fn lower(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(one), None) => one,
        _ => c,
    }
}

/// The extension's digits, when an extension pattern matches `chars` from
/// `start` to the end.
fn extension_at(chars: &[char], start: usize) -> Option<String> {
    let rest = chars.get(start..)?;
    // `;ext=` then up to 20 digits.
    if let Some(digits) = rest.strip_prefix(&[';', 'e', 'x', 't', '='])
        && let Some(found) = digits_to_end(digits, 20, false)
    {
        return Some(found);
    }
    // Separators, then an explicit label (`ext`, `extension`, `anexo`, ...).
    let after_separators = skip(rest, |c| matches!(c, ' ' | '\u{00A0}' | '\t' | ','));
    if let Some(after) = explicit_label(after_separators)
        && let Some(found) = after_label(after, 20)
    {
        return Some(found);
    }
    // Separators, then an ambiguous label (`x`, `#`, `~`, `int`, ...).
    if let Some(after) = ambiguous_label(after_separators)
        && let Some(found) = after_label(after, 9)
    {
        return Some(found);
    }
    // The American style: `- 503#`.
    let after_dashes = skip(rest, |c| matches!(c, '-' | ' '));
    if after_dashes.len() < rest.len()
        && let Some(found) = digits_to_end(after_dashes, 6, true)
    {
        return Some(found);
    }
    // Auto-dialling: `,,` or `;` before the extension.
    let no_comma = skip(rest, |c| matches!(c, ' ' | '\u{00A0}' | '\t'));
    let auto = no_comma
        .strip_prefix(&[',', ','])
        .or_else(|| no_comma.strip_prefix(&[';']));
    if let Some(after) = auto
        && let Some(found) = after_label(after, 15)
    {
        return Some(found);
    }
    // Commas alone.
    let commas = skip(no_comma, |c| c == ',');
    if commas.len() < no_comma.len()
        && let Some(found) = after_label(commas, 9)
    {
        return Some(found);
    }
    None
}

fn skip(chars: &[char], keep: impl Fn(char) -> bool) -> &[char] {
    let at = chars.iter().take_while(|c| keep(**c)).count();
    chars.get(at..).unwrap_or(&[])
}

/// Takes `c` from the front of `rest`.
fn take(rest: &mut &[char], c: char) -> bool {
    match rest.split_first() {
        Some((first, tail)) if *first == c => {
            *rest = tail;
            true
        }
        _ => false,
    }
}

/// `e?xt(?:ensi(?:ó?|ó))?n?`, `ｅ?ｘｔｎ?`, `доб` or `anexo`.
fn explicit_label(chars: &[char]) -> Option<&[char]> {
    let mut rest = chars;
    take(&mut rest, 'e');
    if take(&mut rest, 'x') && take(&mut rest, 't') {
        let mut long = rest;
        if take(&mut long, 'e')
            && take(&mut long, 'n')
            && take(&mut long, 's')
            && take(&mut long, 'i')
        {
            if take(&mut long, 'o') {
                take(&mut long, '\u{0301}');
                rest = long;
            } else if take(&mut long, '\u{00F3}') {
                rest = long;
            }
        }
        take(&mut rest, 'n');
        return Some(rest);
    }
    let mut rest = chars;
    take(&mut rest, '\u{FF45}');
    if take(&mut rest, '\u{FF58}') && take(&mut rest, '\u{FF54}') {
        take(&mut rest, '\u{FF4E}');
        return Some(rest);
    }
    chars
        .strip_prefix(&['\u{0434}', '\u{043E}', '\u{0431}'])
        .or_else(|| chars.strip_prefix(&['a', 'n', 'e', 'x', 'o']))
}

/// `x`, `ｘ`, `#`, `＃`, `~`, `～`, `int` or `ｉｎｔ`.
fn ambiguous_label(chars: &[char]) -> Option<&[char]> {
    match chars.first()? {
        'x' | '\u{FF58}' | '#' | '\u{FF03}' | '~' | '\u{FF5E}' => chars.get(1..),
        _ => chars
            .strip_prefix(&['i', 'n', 't'])
            .or_else(|| chars.strip_prefix(&['\u{FF49}', '\u{FF4E}', '\u{FF54}'])),
    }
}

/// After a label: an optional `:` or `.`, separators, then the digits.
fn after_label(chars: &[char], limit: usize) -> Option<String> {
    let rest = match chars.first() {
        Some(':' | '.' | '\u{FF0E}') => chars.get(1..).unwrap_or(&[]),
        _ => chars,
    };
    let rest = skip(rest, |c| matches!(c, ' ' | '\u{00A0}' | '\t' | ',' | '-'));
    digits_to_end(rest, limit, false)
}

/// 1 to `limit` digits, then `#` (required or optional), then the end.
fn digits_to_end(chars: &[char], limit: usize, hash_required: bool) -> Option<String> {
    let count = chars.iter().take_while(|c| c.is_ascii_digit()).count();
    if count == 0 || count > limit {
        return None;
    }
    let ok = match chars.get(count..).unwrap_or(&[]) {
        [] => !hash_required,
        ['#' | '\u{FF03}'] => true,
        _ => false,
    };
    ok.then(|| chars.iter().take(count).collect())
}

/// The digits of `text`, with letters read as keypad digits when there are
/// at least three of them (`1-800-FLOWERS`); everything else dropped.
pub(crate) fn normalize(text: &str) -> String {
    let letters = text.chars().filter(char::is_ascii_alphabetic).count();
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_digit() {
            out.push(c);
        } else if letters >= 3 && c.is_ascii_alphabetic() {
            out.push(keypad(c));
        }
    }
    out
}

/// The keypad digit of an ASCII letter.
fn keypad(c: char) -> char {
    match c.to_ascii_uppercase() {
        'A'..='C' => '2',
        'D'..='F' => '3',
        'G'..='I' => '4',
        'J'..='L' => '5',
        'M'..='O' => '6',
        'P'..='S' => '7',
        'T'..='V' => '8',
        _ => '9',
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
                Some(_) => assert!(decimal, "U+{code:04X} folds but is not a decimal digit"),
                None if decimal && !c.is_ascii_digit() => missed.push(format!("U+{code:04X}")),
                None => {}
            }
        }
        assert!(
            missed.is_empty(),
            "decimal digits the table misses: {missed:?}"
        );
        assert_eq!(fold_digits("٠٥٠ ١٢٣ ٤٥٦٧"), "050 123 4567");
        assert!(matches!(fold_digits("+44 7400"), Cow::Borrowed(_)));
    }

    #[test]
    fn extraction_finds_the_number_in_text() {
        assert_eq!(extract("Tel: +44 (0)20 7946 0018."), "+44 (0)20 7946 0018");
        assert_eq!(extract("call 0800 FLOWERS!"), "0800 FLOWERS");
        assert_eq!(extract("020 7946 0018 / x 0019"), "020 7946 0018 ");
        assert_eq!(extract("020 7946 0018 #"), "020 7946 0018 #");
        assert_eq!(extract("no digits"), "");
    }

    #[test]
    fn rfc3966_and_phone_context() {
        assert_eq!(
            national_for_parsing("tel:7946-0018;phone-context=+44-20").as_deref(),
            Some("+44-207946-0018")
        );
        assert_eq!(
            national_for_parsing("tel:2079460018;phone-context=example.com").as_deref(),
            Some("2079460018")
        );
        assert_eq!(
            national_for_parsing("tel:+442079460018;isub=12").as_deref(),
            Some("+442079460018")
        );
        for bad in [
            "tel:123;phone-context=",
            "tel:123;phone-context=+",
            "tel:1;phone-context=-x.",
            ";phone-context=+44tel:",
        ] {
            assert_eq!(national_for_parsing(bad), None, "{bad}");
        }
    }

    #[test]
    fn viability() {
        for good in [
            "12",
            "+44 20 7946",
            "1-800-FLOWERS",
            "020 7946 0018 ext. 12",
            "(020) 7946.0018",
            "**123",
        ] {
            assert!(is_viable(good), "{good}");
        }
        for bad in ["1", "+1", "a12", "1a2", "12a", "+44 +44", "ext 12"] {
            assert!(!is_viable(bad), "{bad}");
        }
    }

    #[test]
    fn extensions() {
        let ext =
            |text: &str| extension_start(text).map(|(at, digits)| (text[..at].to_owned(), digits));
        for (input, number, digits) in [
            ("020 7946 0018 ext. 12", "020 7946 0018", "12"),
            ("020 7946 0018 EXT 12", "020 7946 0018", "12"),
            ("020 7946 0018 x12", "020 7946 0018", "12"),
            ("020 7946 0018 extensión 12", "020 7946 0018", "12"),
            ("020 7946 0018 доб. 12", "020 7946 0018", "12"),
            ("020 7946 0018;ext=12", "020 7946 0018", "12"),
            ("020 7946 0018,,12", "020 7946 0018", "12"),
            ("020 7946 0018, 12", "020 7946 0018", "12"),
            ("2015550123 - 503#", "2015550123", "503"),
            ("020 7946 0018 #12#", "020 7946 0018", "12"),
        ] {
            assert_eq!(
                ext(input),
                Some((number.to_owned(), digits.to_owned())),
                "{input}"
            );
        }
        for none in [
            "020 7946 0018",
            "x",
            "020 7946 0018 x1234567890",
            "2015550123 - 503",
            "12",
        ] {
            assert_eq!(ext(none), None, "{none}");
        }
    }

    #[test]
    fn letters_on_a_keypad() {
        assert_eq!(normalize("1-800-FLOWERS"), "18003569377");
        assert_eq!(normalize("+44 (0) 20 ab"), "44020");
    }
}
