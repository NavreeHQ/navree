//! From text to a country calling code and a national significant number,
//! following libphonenumber's parsing steps: the possible number in the
//! text, its extension, the country calling code (after a plus sign, after
//! the international prefix dialled from the default region, or the
//! default region's own), then the national prefix.

use crate::ParseError;
use crate::meta::{self, Territory};
use crate::normalize::{
    extension_start, fold_digits, is_plus, is_viable, national_for_parsing, normalize,
};

/// Longer text is not a phone number (the reference library's limit).
pub(crate) const MAX_INPUT_CHARS: usize = 250;

/// The fewest digits of a national significant number.
const MIN_NSN: usize = 2;

/// The most digits of a national significant number.
pub(crate) const MAX_NSN: usize = 17;

/// The most digits of a country calling code.
const MAX_CODE: usize = 3;

/// The most digits handled at once: a calling code, the longest national
/// number, an international prefix and a national prefix with a carrier
/// code. Text with more digits than this is too long.
const MAX_DIGITS: usize = 40;

/// What parsing found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Parsed {
    pub(crate) code: u16,
    pub(crate) nsn: String,
    pub(crate) extension: String,
}

/// How a number's length compares with its territory's lengths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Length {
    Possible,
    LocalOnly,
    TooShort,
    TooLong,
    Invalid,
}

/// The length check of the reference library, on the territory's lengths
/// for any type.
pub(crate) fn test_length(nsn: &[u8], territory: &Territory) -> Length {
    let lengths = territory.general.lengths;
    if lengths == 0 {
        return Length::Invalid;
    }
    let len = nsn.len();
    if meta::has_length(territory.general.local, len) {
        return Length::LocalOnly;
    }
    let min = lengths.trailing_zeros() as usize;
    let max = 31 - lengths.leading_zeros() as usize;
    if len == min {
        Length::Possible
    } else if len < min {
        Length::TooShort
    } else if len > max {
        Length::TooLong
    } else if meta::has_length(lengths, len) {
        Length::Possible
    } else {
        Length::Invalid
    }
}

/// Parses `text`, reading a number without a country calling code in
/// `region`.
pub(crate) fn parse(text: &str, region: Option<&'static Territory>) -> Result<Parsed, ParseError> {
    if text.len() > 4 * MAX_INPUT_CHARS || text.chars().nth(MAX_INPUT_CHARS).is_some() {
        return Err(ParseError::TooLong);
    }
    let folded = fold_digits(text);
    if folded.bytes().filter(u8::is_ascii_digit).count() > MAX_DIGITS {
        return Err(ParseError::TooLong);
    }
    let national = national_for_parsing(&folded).ok_or(ParseError::NotANumber)?;
    if !is_viable(&national) {
        return Err(ParseError::NotANumber);
    }
    if region.is_none() && !national.starts_with(is_plus) {
        return Err(ParseError::NoRegion);
    }
    let (national, extension) = match extension_start(&national) {
        Some((start, digits)) if is_viable(national.get(..start).unwrap_or("")) => {
            (national.get(..start).unwrap_or("").to_owned(), digits)
        }
        _ => (national, String::new()),
    };

    let (mut code, mut nsn) = match extract_country_code(&national, region) {
        Ok(found) => found,
        Err(ParseError::InvalidCountryCode) if national.starts_with(is_plus) => {
            // A plus sign before something else: read what follows it again.
            let after = national.trim_start_matches(is_plus);
            match extract_country_code(after, region)? {
                (0, _) => return Err(ParseError::InvalidCountryCode),
                found => found,
            }
        }
        Err(error) => return Err(error),
    };
    // A number with its own calling code follows that code's main plan.
    let mut plan = region;
    if code != 0 {
        plan = meta::main_for(code);
    } else {
        nsn = normalize(&national);
        match region {
            Some(region) => code = region.code,
            None => return Err(ParseError::NoRegion),
        }
    }
    if nsn.len() < MIN_NSN {
        return Err(ParseError::TooShort);
    }
    if let Some(plan) = plan {
        let mut stripped = nsn.clone();
        strip_national_prefix(&mut stripped, plan);
        if !matches!(
            test_length(stripped.as_bytes(), plan),
            Length::TooShort | Length::LocalOnly | Length::Invalid
        ) {
            nsn = stripped;
        }
    }
    if nsn.len() < MIN_NSN {
        return Err(ParseError::TooShort);
    }
    if nsn.len() > MAX_NSN {
        return Err(ParseError::TooLong);
    }
    Ok(Parsed {
        code,
        nsn,
        extension,
    })
}

/// Where a country calling code came from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Plus,
    Idd,
    Default,
}

/// The country calling code and the rest of the number, when the number
/// holds one; `(0, "")` when it doesn't.
fn extract_country_code(
    number: &str,
    region: Option<&'static Territory>,
) -> Result<(u16, String), ParseError> {
    if number.is_empty() {
        return Ok((0, String::new()));
    }
    let (source, full) = strip_international_prefix(number, region);
    if source != Source::Default {
        if full.len() <= MIN_NSN {
            return Err(ParseError::TooShortAfterIdd);
        }
        return match split_country_code(&full) {
            Some((code, rest)) => Ok((code, rest.to_owned())),
            None => Err(ParseError::InvalidCountryCode),
        };
    }
    if let Some(region) = region {
        // The default region's own code, written without a plus sign: kept
        // when the number reads better without it.
        let own = region.code.to_string();
        if let Some(rest) = full.strip_prefix(own.as_str()) {
            let mut rest = rest.to_owned();
            strip_national_prefix(&mut rest, region);
            let general = region.general.pattern;
            if (!general.matches(full.as_bytes()) && general.matches(rest.as_bytes()))
                || test_length(full.as_bytes(), region) == Length::TooLong
            {
                return Ok((region.code, rest));
            }
        }
    }
    Ok((0, String::new()))
}

/// Strips a leading plus sign, or the international prefix dialled from
/// `region`, and normalizes the rest to digits.
fn strip_international_prefix(
    number: &str,
    region: Option<&'static Territory>,
) -> (Source, String) {
    if number.starts_with(is_plus) {
        return (Source::Plus, normalize(number.trim_start_matches(is_plus)));
    }
    let digits = normalize(number);
    if let Some(region) = region
        && let Some(end) = region.idd.looking_at(digits.as_bytes())
    {
        // Calling codes never start with 0: a 0 after the prefix means it
        // was something else.
        let rest = digits.get(end..).unwrap_or("");
        if !rest.starts_with('0') {
            return (Source::Idd, rest.to_owned());
        }
    }
    (Source::Default, digits)
}

/// A calling code at the start of `digits`, and what follows it.
pub(crate) fn split_country_code(digits: &str) -> Option<(u16, &str)> {
    if digits.starts_with('0') {
        return None;
    }
    (1..=MAX_CODE.min(digits.len())).find_map(|len| {
        let code: u16 = digits.get(..len)?.parse().ok()?;
        meta::is_calling_code(code).then(|| (code, digits.get(len..).unwrap_or("")))
    })
}

/// Strips the national prefix (and a carrier code) of `territory` from the
/// start of `number`, rewriting it by the territory's rule where it has one,
/// unless that turns a number that fits the plan into one that doesn't.
pub(crate) fn strip_national_prefix(number: &mut String, territory: &Territory) -> bool {
    let npp = territory.npp;
    if number.is_empty() || !npp.exists() {
        return false;
    }
    let Some(captures) = npp.captures(number.as_bytes(), false) else {
        return false;
    };
    let general = territory.general.pattern;
    let viable = general.matches(number.as_bytes());
    let transform = territory.npt.get();
    let end = captures.end();
    let rest = number.get(end..).unwrap_or("").to_owned();
    if transform.is_empty() || captures.get(npp.groups()).is_none() {
        if viable && !general.matches(rest.as_bytes()) {
            return false;
        }
        *number = rest;
        return true;
    }
    let mut transformed = expand(transform, &captures, number.as_bytes());
    transformed.push_str(&rest);
    if viable && !general.matches(transformed.as_bytes()) {
        return false;
    }
    *number = transformed;
    true
}

/// `template` with each `$n` replaced by group `n` of a match on `input`.
pub(crate) fn expand(template: &str, captures: &crate::regex::Captures, input: &[u8]) -> String {
    let mut out = String::with_capacity(template.len() + input.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$'
            && let Some(group) = chars.peek().and_then(|d| d.to_digit(10))
        {
            chars.next();
            if let Some((start, end)) = captures.get(group as usize)
                && let Some(text) = input.get(start..end)
            {
                out.extend(text.iter().map(|b| char::from(*b)));
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The territory whose number `nsn` (of calling code `code`) is, among those
/// sharing the code: the one whose leading digits it starts with, else the
/// first whose plan it fits.
pub(crate) fn territory_of(code: u16, nsn: &[u8]) -> Option<&'static Territory> {
    let first = meta::territories_for(code).next()?;
    if meta::territories_for(code).nth(1).is_none() {
        return Some(first);
    }
    meta::territories_for(code).find(|territory| {
        if territory.leading.exists() {
            territory.leading.looking_at(nsn).is_some()
        } else {
            crate::number::kind_of(territory, nsn).is_some()
        }
    })
}
