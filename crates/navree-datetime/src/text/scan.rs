//! The small scanners every parser of this crate is built from: a number of
//! so many digits, a month's or a weekday's name, an offset from UTC.
//!
//! Each takes the text that is left and gives back what is left after it,
//! with what it read. None of them looks past what it needs, so a text can
//! be read piece by piece with no separators (`20261125`).

use crate::{Error, Weekday};

/// Why a text was refused.
///
/// Each reason has one message, the same for every scanner, so the text of
/// a refusal does not depend on which scanner met it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    /// A number names nothing: month 13, hour 24, a year no date holds.
    OutOfRange,
    /// Two parts of the text contradict each other: a Sunday that is a
    /// Monday, `13:00 AM`.
    Impossible,
    /// The text was read, and it does not say enough: a day with no year, a
    /// time with no minute, an instant with no offset.
    NotEnough,
    /// A character that does not belong where it is.
    Invalid,
    /// The text ended before the value did.
    TooShort,
    /// The value ended before the text did.
    TooLong,
    /// The pattern itself is wrong.
    BadPattern,
}

impl Reason {
    /// The message of an error for this reason.
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Reason::OutOfRange => "input is out of range",
            Reason::Impossible => "no possible date and time matching input",
            Reason::NotEnough => "input is not enough for unique date and time",
            Reason::Invalid => "input contains invalid characters",
            Reason::TooShort => "premature end of input",
            Reason::TooLong => "trailing input",
            Reason::BadPattern => "bad or unsupported format string",
        }
    }
}

impl From<Reason> for Error {
    fn from(reason: Reason) -> Error {
        Error::parse(reason.message())
    }
}

/// What a scanner returns.
pub(crate) type Scan<T> = Result<T, Reason>;

/// A number of `min` to `max` digits, with no sign.
///
/// It reads as many digits as there are, up to `max`, and stops at the
/// first character that is not one. Fewer than `min` is a refusal, and so
/// is a number an `i64` cannot hold.
pub(crate) fn number(s: &str, min: usize, max: usize) -> Scan<(&str, i64)> {
    // Only ASCII digits count, so the text is read as bytes. The cuts below
    // are always after ASCII digits, which is between two characters.
    let bytes = s.as_bytes();
    if bytes.len() < min {
        return Err(Reason::TooShort);
    }

    let mut n = 0i64;
    for (i, c) in bytes.iter().take(max).copied().enumerate() {
        if !c.is_ascii_digit() {
            if i < min {
                return Err(Reason::Invalid);
            }
            return Ok((&s[i..], n));
        }
        n = match n
            .checked_mul(10)
            .and_then(|n| n.checked_add(i64::from(c - b'0')))
        {
            Some(n) => n,
            None => return Err(Reason::OutOfRange),
        };
    }

    Ok((&s[max.min(bytes.len())..], n))
}

/// What one digit in each place of a fraction of a second is worth, in
/// nanoseconds: `SCALE[3]` is the third digit after the dot.
const SCALE: [u32; 10] = [
    0,
    100_000_000,
    10_000_000,
    1_000_000,
    100_000,
    10_000,
    1_000,
    100,
    10,
    1,
];

/// The digits of a fraction of a second, one or more, as nanoseconds. Digits
/// after the ninth are read and dropped.
pub(crate) fn nanosecond(s: &str) -> Scan<(&str, u32)> {
    let before = s.len();
    let (s, v) = number(s, 1, 9)?;
    let digits = before - s.len();
    // At most nine digits were read, so the number is below 1,000,000,000.
    let v = (v as u32)
        .checked_mul(SCALE[digits])
        .ok_or(Reason::OutOfRange)?;
    let s = s.trim_start_matches(|c: char| c.is_ascii_digit());
    Ok((s, v))
}

/// Exactly `digits` digits (3, 6 or 9) of a fraction of a second, as
/// nanoseconds.
pub(crate) fn nanosecond_fixed(s: &str, digits: usize) -> Scan<(&str, i64)> {
    let (s, v) = number(s, digits, digits)?;
    let v = v
        .checked_mul(i64::from(SCALE[digits]))
        .ok_or(Reason::OutOfRange)?;
    Ok((s, v))
}

/// A month by the first three letters of its English name, in any case:
/// 0 for January to 11 for December.
pub(crate) fn short_month0(s: &str) -> Scan<(&str, u8)> {
    if s.len() < 3 {
        return Err(Reason::TooShort);
    }
    let buf = s.as_bytes();
    // `| 32` turns an ASCII capital into its small letter and leaves a small
    // letter alone; nothing else becomes a letter by it.
    let month0 = match (buf[0] | 32, buf[1] | 32, buf[2] | 32) {
        (b'j', b'a', b'n') => 0,
        (b'f', b'e', b'b') => 1,
        (b'm', b'a', b'r') => 2,
        (b'a', b'p', b'r') => 3,
        (b'm', b'a', b'y') => 4,
        (b'j', b'u', b'n') => 5,
        (b'j', b'u', b'l') => 6,
        (b'a', b'u', b'g') => 7,
        (b's', b'e', b'p') => 8,
        (b'o', b'c', b't') => 9,
        (b'n', b'o', b'v') => 10,
        (b'd', b'e', b'c') => 11,
        _ => return Err(Reason::Invalid),
    };
    Ok((&s[3..], month0))
}

/// A weekday by the first three letters of its English name, in any case.
pub(crate) fn short_weekday(s: &str) -> Scan<(&str, Weekday)> {
    if s.len() < 3 {
        return Err(Reason::TooShort);
    }
    let buf = s.as_bytes();
    let weekday = match (buf[0] | 32, buf[1] | 32, buf[2] | 32) {
        (b'm', b'o', b'n') => Weekday::Monday,
        (b't', b'u', b'e') => Weekday::Tuesday,
        (b'w', b'e', b'd') => Weekday::Wednesday,
        (b't', b'h', b'u') => Weekday::Thursday,
        (b'f', b'r', b'i') => Weekday::Friday,
        (b's', b'a', b't') => Weekday::Saturday,
        (b's', b'u', b'n') => Weekday::Sunday,
        _ => return Err(Reason::Invalid),
    };
    Ok((&s[3..], weekday))
}

/// A month by its English name or the name's first three letters, in any
/// case: 0 for January to 11 for December. The whole name is read where it
/// is there.
pub(crate) fn short_or_long_month0(s: &str) -> Scan<(&str, u8)> {
    // The names in small letters, without their first three.
    const REST: [&[u8]; 12] = [
        b"uary", b"ruary", b"ch", b"il", b"", b"e", b"y", b"ust", b"tember", b"ober", b"ember",
        b"ember",
    ];

    let (mut s, month0) = short_month0(s)?;
    let rest = REST[usize::from(month0)];
    if s.len() >= rest.len() && s.as_bytes()[..rest.len()].eq_ignore_ascii_case(rest) {
        s = &s[rest.len()..];
    }
    Ok((s, month0))
}

/// A weekday by its English name or the name's first three letters, in any
/// case. The whole name is read where it is there.
pub(crate) fn short_or_long_weekday(s: &str) -> Scan<(&str, Weekday)> {
    // The names in small letters, without their first three, from Monday.
    const REST: [&[u8]; 7] = [
        b"day", b"sday", b"nesday", b"rsday", b"day", b"urday", b"day",
    ];

    let (mut s, weekday) = short_weekday(s)?;
    let rest = REST[usize::from(weekday.days_from_monday())];
    if s.len() >= rest.len() && s.as_bytes()[..rest.len()].eq_ignore_ascii_case(rest) {
        s = &s[rest.len()..];
    }
    Ok((s, weekday))
}

/// Exactly the ASCII character `c1`.
pub(crate) fn char(s: &str, c1: u8) -> Scan<&str> {
    match s.as_bytes().first() {
        Some(&c) if c == c1 => Ok(&s[1..]),
        Some(_) => Err(Reason::Invalid),
        None => Err(Reason::TooShort),
    }
}

/// One or more white space characters.
pub(crate) fn space(s: &str) -> Scan<&str> {
    let s_ = s.trim_start();
    if s_.len() < s.len() {
        Ok(s_)
    } else if s.is_empty() {
        Err(Reason::TooShort)
    } else {
        Err(Reason::Invalid)
    }
}

/// What may stand between the hours and the minutes of an offset in a
/// pattern: any number of colons and white space, or nothing.
pub(crate) fn colon_or_space(s: &str) -> Scan<&str> {
    Ok(s.trim_start_matches(|c: char| c == ':' || c.is_whitespace()))
}

/// What must stand between the hours and the minutes of an offset in
/// RFC 3339: one colon.
pub(crate) fn colon(s: &str) -> Scan<&str> {
    char(s, b':')
}

/// What stands between the hours and the minutes of an offset in RFC 2822:
/// nothing.
pub(crate) fn nothing(s: &str) -> Scan<&str> {
    Ok(s)
}

/// An offset from UTC, in seconds east: a sign, two digits of hours, what
/// `between` reads, and two digits of minutes (00 to 59).
///
/// `allow_z` also reads `Z` or `z` as no offset. `allow_hours_alone` lets
/// the minutes be missing, at the end of the text only. `allow_minus_sign`
/// also reads the minus sign U+2212 for the hyphen.
///
/// The hours go up to 99: whether so large an offset is one a value can
/// have is for the caller to say.
pub(crate) fn timezone_offset(
    mut s: &str,
    between: fn(&str) -> Scan<&str>,
    allow_z: bool,
    allow_hours_alone: bool,
    allow_minus_sign: bool,
) -> Scan<(&str, i32)> {
    if allow_z && let Some(&b'Z' | &b'z') = s.as_bytes().first() {
        return Ok((&s[1..], 0));
    }

    fn two(s: &str) -> Scan<(u8, u8)> {
        let b = s.as_bytes();
        if b.len() < 2 {
            Err(Reason::TooShort)
        } else {
            Ok((b[0], b[1]))
        }
    }

    let negative = match s.chars().next() {
        Some('+') => {
            s = &s['+'.len_utf8()..];
            false
        }
        Some('-') => {
            s = &s['-'.len_utf8()..];
            true
        }
        Some('\u{2212}') => {
            if !allow_minus_sign {
                return Err(Reason::Invalid);
            }
            s = &s['\u{2212}'.len_utf8()..];
            true
        }
        Some(_) => return Err(Reason::Invalid),
        None => return Err(Reason::TooShort),
    };

    // Hours: 00 to 99.
    let hours = match two(s)? {
        (h1 @ b'0'..=b'9', h2 @ b'0'..=b'9') => i32::from((h1 - b'0') * 10 + (h2 - b'0')),
        _ => return Err(Reason::Invalid),
    };
    s = &s[2..];

    s = between(s)?;

    // Minutes: 00 to 59. Where two more characters follow they have to be
    // the minutes.
    let minutes = if let Ok(pair) = two(s) {
        match pair {
            (m1 @ b'0'..=b'5', m2 @ b'0'..=b'9') => i32::from((m1 - b'0') * 10 + (m2 - b'0')),
            (b'6'..=b'9', b'0'..=b'9') => return Err(Reason::OutOfRange),
            _ => return Err(Reason::Invalid),
        }
    } else if allow_hours_alone {
        0
    } else {
        return Err(Reason::TooShort);
    };
    s = match s.len() {
        len if len >= 2 => &s[2..],
        0 => s,
        _ => return Err(Reason::TooShort),
    };

    let seconds = hours * 3_600 + minutes * 60;
    Ok((s, if negative { -seconds } else { seconds }))
}

/// The zone of an RFC 2822 date: a sign and four digits, or one of the
/// names the RFC keeps from RFC 822 (`GMT`, `UT`, `Z`, and the North
/// American `EST` to `PDT`). A single other letter is a military zone, read
/// and taken as no offset, as the RFC says to.
pub(crate) fn timezone_offset_2822(s: &str) -> Scan<(&str, i32)> {
    let upto = s
        .as_bytes()
        .iter()
        .position(|&c| !c.is_ascii_alphabetic())
        .unwrap_or(s.len());
    if upto > 0 {
        let name = &s.as_bytes()[..upto];
        let s = &s[upto..];
        let hours = |hours: i32| Ok((s, hours * 3_600));
        if name.eq_ignore_ascii_case(b"gmt")
            || name.eq_ignore_ascii_case(b"ut")
            || name.eq_ignore_ascii_case(b"z")
        {
            return hours(0);
        } else if name.eq_ignore_ascii_case(b"edt") {
            return hours(-4);
        } else if name.eq_ignore_ascii_case(b"est") || name.eq_ignore_ascii_case(b"cdt") {
            return hours(-5);
        } else if name.eq_ignore_ascii_case(b"cst") || name.eq_ignore_ascii_case(b"mdt") {
            return hours(-6);
        } else if name.eq_ignore_ascii_case(b"mst") || name.eq_ignore_ascii_case(b"pdt") {
            return hours(-7);
        } else if name.eq_ignore_ascii_case(b"pst") {
            return hours(-8);
        } else if name.len() == 1
            && let b'a'..=b'i' | b'k'..=b'y' | b'A'..=b'I' | b'K'..=b'Y' = name[0]
        {
            return Ok((s, 0));
        }
        Err(Reason::Invalid)
    } else {
        timezone_offset(s, nothing, false, false, false)
    }
}

/// A comment of an RFC 2822 date, with the white space before it: text in
/// round brackets, which can hold more brackets and `\` before a character
/// that is to be taken as it is.
pub(crate) fn comment_2822(s: &str) -> Scan<&str> {
    enum State {
        Start,
        Next(usize),
        Escape(usize),
    }

    let s = s.trim_start();

    let mut state = State::Start;
    for (i, c) in s.bytes().enumerate() {
        state = match (state, c) {
            (State::Start, b'(') => State::Next(1),
            (State::Next(1), b')') => return Ok(&s[i + 1..]),
            (State::Next(depth), b'\\') => State::Escape(depth),
            (State::Next(depth), b'(') => State::Next(depth + 1),
            (State::Next(depth), b')') => State::Next(depth - 1),
            (State::Next(depth), _) | (State::Escape(depth), _) => State::Next(depth),
            _ => return Err(Reason::Invalid),
        };
    }

    Err(Reason::TooShort)
}
