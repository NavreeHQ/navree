//! Patterns: how a date or a time is laid out as text.
//!
//! A pattern is text with specifiers in it. A specifier is `%` and a letter,
//! and stands for one part of a date or a time; everything else stands for
//! itself. `value.format(pattern)` writes a value by a pattern and
//! `Type::parse(pattern, text)` reads one:
//!
//! ```
//! use navree_datetime::{Date, Time};
//!
//! # fn main() -> navree_datetime::Result<()> {
//! let day = Date::new(2026, 11, 25)?;
//! assert_eq!(day.format("%d/%m/%Y").to_string(), "25/11/2026");
//! assert_eq!(day.format("%A, %-d %B %Y").to_string(), "Wednesday, 25 November 2026");
//! assert_eq!(Date::parse("%d/%m/%Y", "25/11/2026")?, day);
//!
//! let opens = Time::new(9, 30, 0)?;
//! assert_eq!(opens.format("%-I:%M %p").to_string(), "9:30 AM");
//! # Ok(())
//! # }
//! ```
//!
//! [`check`] says whether a pattern is one, without a value: a macro calls
//! it to refuse a wrong pattern while the program is compiled.
//!
//! # The specifiers
//!
//! The examples are of 9:05:07 and 26,490 microseconds in the evening of
//! Sunday 8 July 2001, at 5 hours 30 minutes east of UTC.
//!
//! | Specifier | Example | What it is |
//! |---|---|---|
//! | | | **The day** |
//! | `%Y` | `2001` | The year, with zeros before it to four digits. A year before 0 or after 9999 has its sign (`-0044`, `+12026`). |
//! | `%C` | `20` | The year divided by 100, two digits. Meant for years 0 to 9999. |
//! | `%y` | `01` | The year's last two digits. Read alone, 70 to 99 are 1970 to 1999 and 00 to 69 are 2000 to 2069. |
//! | `%q` | `3` | The quarter of the year: 1 to 4. |
//! | `%m` | `07` | The month's number: 01 to 12. |
//! | `%b` | `Jul` | The first three letters of the month's English name. |
//! | `%h` | `Jul` | The same as `%b`. |
//! | `%B` | `July` | The month's English name. Read, it may also be its first three letters. |
//! | `%d` | `08` | The day of the month: 01 to 31. |
//! | `%e` | ` 8` | The day of the month with a space before one digit. |
//! | `%a` | `Sun` | The first three letters of the weekday's English name. |
//! | `%A` | `Sunday` | The weekday's English name. Read, it may also be its first three letters. |
//! | `%w` | `0` | The weekday as a number from Sunday: Sunday is 0 and Saturday is 6. |
//! | `%u` | `7` | The weekday as a number from Monday: Monday is 1 and Sunday is 7. |
//! | `%U` | `27` | The week of the year, 00 to 53, where week 1 starts on the year's first Sunday. |
//! | `%W` | `27` | The week of the year, 00 to 53, where week 1 starts on the year's first Monday. |
//! | `%G` | `2001` | The year of the ISO 8601 week, written as `%Y` is. |
//! | `%g` | `01` | The last two digits of the year of the ISO 8601 week. |
//! | `%V` | `27` | The ISO 8601 week: 01 to 53. |
//! | `%j` | `189` | The day of the year: 001 to 366. |
//! | `%D` | `07/08/01` | The same as `%m/%d/%y`. |
//! | `%x` | `07/08/01` | The same as `%m/%d/%y`. |
//! | `%F` | `2001-07-08` | The same as `%Y-%m-%d`. |
//! | `%v` | ` 8-Jul-2001` | The same as `%e-%b-%Y`. |
//! | | | **The time of day** |
//! | `%H` | `21` | The hour: 00 to 23. |
//! | `%k` | `21` | The hour with a space before one digit. |
//! | `%I` | `09` | The hour of a 12-hour clock: 01 to 12. |
//! | `%l` | ` 9` | The hour of a 12-hour clock with a space before one digit. |
//! | `%P` | `pm` | `am` or `pm`. |
//! | `%p` | `PM` | `AM` or `PM`. |
//! | `%M` | `05` | The minute: 00 to 59. |
//! | `%S` | `07` | The second: 00 to 59. Read, 60 is taken too (see below). |
//! | `%f` | `026490000` | The nanoseconds after the second, as a number of nine digits. It is not a fraction: read, `5` is 5 nanoseconds. |
//! | `%.f` | `.026490` | The fraction of the second with its dot: nothing when there is none, else 3, 6 or 9 digits, the fewest that lose nothing. Read, any number of digits, or no dot and no fraction. |
//! | `%.3f` | `.026` | The fraction with its dot, cut to 3 digits. Read, exactly 3 digits after a dot, or no dot. |
//! | `%.6f` | `.026490` | The same with 6 digits. |
//! | `%.9f` | `.026490000` | The same with 9 digits. |
//! | `%3f` | `026` | The fraction cut to 3 digits, with no dot. |
//! | `%6f` | `026490` | The same with 6 digits. |
//! | `%9f` | `026490000` | The same with 9 digits. |
//! | `%R` | `21:05` | The same as `%H:%M`. |
//! | `%T` | `21:05:07` | The same as `%H:%M:%S`. |
//! | `%X` | `21:05:07` | The same as `%H:%M:%S`. |
//! | `%r` | `09:05:07 PM` | The same as `%I:%M:%S %p`. |
//! | | | **The zone** |
//! | `%Z` | `IST` | The zone's short name at that instant, `UTC` for an instant with no zone, or the offset (`+05:30`) where a value has only an offset. Read, it skips everything up to the next white space and says nothing about the offset. |
//! | `%z` | `+0530` | The offset from UTC, to the nearest minute. |
//! | `%:z` | `+05:30` | The offset with a colon. |
//! | `%::z` | `+05:30:00` | The offset with its seconds. |
//! | `%:::z` | `+05` | The offset's hours alone, the rest dropped. |
//! | `%#z` | | For reading only: an offset whose minutes may be missing (`+05`), or `Z`. |
//! | | | **The whole of it** |
//! | `%c` | `Sun Jul  8 21:05:07 2001` | The same as `%a %b %e %H:%M:%S %Y`. |
//! | `%+` | `2001-07-08T21:05:07.026490+05:30` | RFC 3339: the same as `%Y-%m-%dT%H:%M:%S%.f%:z`. Read, it takes what `"...".parse::<Timestamp>()` takes. |
//! | `%s` | `994606507` | The seconds from 1970-01-01T00:00:00Z, not padded. Read, it cannot be below zero. |
//! | | | **Text** |
//! | `%t` | | A tab. |
//! | `%n` | | A new line. |
//! | `%%` | `%` | A percent sign. |
//!
//! The names are English everywhere, whatever the machine's language.
//!
//! ## Padding
//!
//! A specifier that writes one number takes `-`, `_` or `0` after the `%`:
//!
//! | Written | Example | What it does |
//! |---|---|---|
//! | `%-d` | `8` | No padding. |
//! | `%_d` | ` 8` | Spaces for padding. |
//! | `%0e` | `08` | Zeros for padding. |
//!
//! They are for `%Y %C %y %q %m %d %e %w %u %U %W %G %g %V %j %H %k %I %l %M
//! %S %f %s` and for no other specifier. `%q`, `%u` and `%w` are one digit
//! with any of them, and `%Y` and `%G` keep four digits for the years 1000
//! to 9999.
//!
//! ## Reading
//!
//! Reading follows the pattern from left to right:
//!
//! - White space in the pattern stands for any white space, or none.
//! - A number may be shorter than its specifier writes it and may have
//!   white space before it, but is never longer: `%Y` reads at most four
//!   digits unless the year has a sign, which lets `%Y%m%d` read `20261125`.
//! - Names are read in any case.
//! - The whole text has to be used up by the whole pattern.
//! - Parts that say the same thing twice have to agree: a weekday that is
//!   not the date's is refused.
//! - A value needs enough to be one. A day needs its year, and then a month
//!   and a day, or the day of the year, or a week and a weekday. A time of
//!   day needs its hour and minute (a 12-hour `%I` needs `%p`), and a
//!   missing second is 0. An instant needs an offset or `%s`. Parts the
//!   value has no place for are read and dropped: a day read by a pattern
//!   with a time in it is that day.
//! - Second 60 is read as the last nanosecond of its minute. A minute has
//!   60 seconds here and no leap second, so `23:59:60` and any fraction
//!   after it is `23:59:59.999999999`: the nearest time there is.

use std::fmt;

use crate::Error;

/// What goes before a number that is shorter than its width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pad {
    /// Nothing.
    None,
    /// Zeros.
    Zero,
    /// Spaces.
    Space,
}

/// A part of a date or a time that is one number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Numeric {
    /// The year. Written to 4 digits; read to 4 digits, or any number after
    /// a sign.
    Year,
    /// The year divided by 100. Width 2.
    YearDiv100,
    /// The year's last two digits. Width 2.
    YearMod100,
    /// The year of the ISO 8601 week, as [`Numeric::Year`].
    IsoYear,
    /// The last two digits of the year of the ISO 8601 week. Width 2.
    IsoYearMod100,
    /// The quarter: 1 to 4. Width 1.
    Quarter,
    /// The month: 1 to 12. Width 2.
    Month,
    /// The day of the month. Width 2.
    Day,
    /// The week of the year where week 1 starts on the first Sunday. Width
    /// 2.
    WeekFromSun,
    /// The week of the year where week 1 starts on the first Monday. Width
    /// 2.
    WeekFromMon,
    /// The ISO 8601 week. Width 2.
    IsoWeek,
    /// The weekday counted from Sunday as 0. Width 1.
    DaysFromSun,
    /// The weekday counted from Monday as 1. Width 1.
    WeekdayFromMon,
    /// The day of the year. Width 3.
    Ordinal,
    /// The hour of 24. Width 2.
    Hour,
    /// The hour of 12. Width 2.
    Hour12,
    /// The minute. Width 2.
    Minute,
    /// The second. Width 2.
    Second,
    /// The nanoseconds after the second. Width 9.
    Nanosecond,
    /// The seconds from the epoch. Written at least 1 digit (9 when padded);
    /// read to any number of digits.
    Timestamp,
}

/// A part of a date or a time that is not one number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fixed {
    /// `Jul`.
    ShortMonthName,
    /// `July`. Read, also `Jul`.
    LongMonthName,
    /// `Sun`.
    ShortWeekdayName,
    /// `Sunday`. Read, also `Sun`.
    LongWeekdayName,
    /// `am` or `pm`.
    LowerAmPm,
    /// `AM` or `PM`.
    UpperAmPm,
    /// A dot and the fraction of the second, in as many digits as it needs
    /// of 3, 6 and 9; nothing when there is none.
    Fraction,
    /// A dot and 3 digits of the fraction.
    Fraction3,
    /// A dot and 6 digits of the fraction.
    Fraction6,
    /// A dot and 9 digits of the fraction.
    Fraction9,
    /// 3 digits of the fraction and no dot.
    Digits3,
    /// 6 digits of the fraction and no dot.
    Digits6,
    /// 9 digits of the fraction and no dot.
    Digits9,
    /// The zone's short name. Read, anything up to white space.
    ZoneName,
    /// `+0530`.
    Offset,
    /// `+05:30`.
    OffsetColon,
    /// `+05:30:00`.
    OffsetSeconds,
    /// `+05`.
    OffsetHours,
    /// Read only: `+0530`, `+05:30`, `+05` or `Z`.
    OffsetPermissive,
    /// `2001-07-08T21:05:07.026490+05:30`.
    Rfc3339,
}

/// One step of a pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Item<'a> {
    /// Text that stands for itself.
    Literal(&'a str),
    /// White space: written as it is, read as any white space or none.
    Space(&'a str),
    /// A number and its padding. Reading takes any padding.
    Numeric(Numeric, Pad),
    /// A part that is not one number.
    Fixed(Fixed),
    /// A specifier that is not one: the pattern is wrong from here.
    Error,
}

const fn num(numeric: Numeric) -> Item<'static> {
    Item::Numeric(numeric, Pad::None)
}

const fn num0(numeric: Numeric) -> Item<'static> {
    Item::Numeric(numeric, Pad::Zero)
}

const fn nums(numeric: Numeric) -> Item<'static> {
    Item::Numeric(numeric, Pad::Space)
}

const fn fixed(fixed: Fixed) -> Item<'static> {
    Item::Fixed(fixed)
}

const fn lit(text: &'static str) -> Item<'static> {
    Item::Literal(text)
}

const fn space(text: &'static str) -> Item<'static> {
    Item::Space(text)
}

/// What follows `%` in a pattern, and the steps it stands for.
struct Specifier {
    /// The text after the `%` (and after a padding sign, where there is
    /// one).
    text: &'static str,
    /// What it stands for: one step, or a few for a specifier that is short
    /// for a pattern (`%F` for `%Y-%m-%d`).
    steps: &'static [Item<'static>],
}

macro_rules! specifiers {
    ($($text:literal => [$($step:expr),+ $(,)?]),+ $(,)?) => {
        &[$(Specifier { text: $text, steps: &[$($step),+] }),+]
    };
}

/// Every specifier there is. Writing, reading and [`check`] all go through
/// this one table, so the three cannot disagree about what a pattern means.
///
/// No specifier's text starts with another's, so at most one fits the text
/// after a `%` and the order is only for speed: the common ones are first.
static SPECIFIERS: &[Specifier] = specifiers![
    "Y" => [num0(Numeric::Year)],
    "m" => [num0(Numeric::Month)],
    "d" => [num0(Numeric::Day)],
    "H" => [num0(Numeric::Hour)],
    "M" => [num0(Numeric::Minute)],
    "S" => [num0(Numeric::Second)],
    ".f" => [fixed(Fixed::Fraction)],
    "b" => [fixed(Fixed::ShortMonthName)],
    "B" => [fixed(Fixed::LongMonthName)],
    "a" => [fixed(Fixed::ShortWeekdayName)],
    "A" => [fixed(Fixed::LongWeekdayName)],
    "e" => [nums(Numeric::Day)],
    "y" => [num0(Numeric::YearMod100)],
    "z" => [fixed(Fixed::Offset)],
    ":z" => [fixed(Fixed::OffsetColon)],
    "Z" => [fixed(Fixed::ZoneName)],
    "F" => [
        num0(Numeric::Year),
        lit("-"),
        num0(Numeric::Month),
        lit("-"),
        num0(Numeric::Day),
    ],
    "T" => [
        num0(Numeric::Hour),
        lit(":"),
        num0(Numeric::Minute),
        lit(":"),
        num0(Numeric::Second),
    ],
    "R" => [num0(Numeric::Hour), lit(":"), num0(Numeric::Minute)],
    ".3f" => [fixed(Fixed::Fraction3)],
    ".6f" => [fixed(Fixed::Fraction6)],
    ".9f" => [fixed(Fixed::Fraction9)],
    "3f" => [fixed(Fixed::Digits3)],
    "6f" => [fixed(Fixed::Digits6)],
    "9f" => [fixed(Fixed::Digits9)],
    "f" => [num0(Numeric::Nanosecond)],
    "j" => [num0(Numeric::Ordinal)],
    "I" => [num0(Numeric::Hour12)],
    "p" => [fixed(Fixed::UpperAmPm)],
    "P" => [fixed(Fixed::LowerAmPm)],
    "k" => [nums(Numeric::Hour)],
    "l" => [nums(Numeric::Hour12)],
    "h" => [fixed(Fixed::ShortMonthName)],
    "C" => [num0(Numeric::YearDiv100)],
    "q" => [num(Numeric::Quarter)],
    "w" => [num(Numeric::DaysFromSun)],
    "u" => [num(Numeric::WeekdayFromMon)],
    "U" => [num0(Numeric::WeekFromSun)],
    "W" => [num0(Numeric::WeekFromMon)],
    "G" => [num0(Numeric::IsoYear)],
    "g" => [num0(Numeric::IsoYearMod100)],
    "V" => [num0(Numeric::IsoWeek)],
    "s" => [num(Numeric::Timestamp)],
    "::z" => [fixed(Fixed::OffsetSeconds)],
    ":::z" => [fixed(Fixed::OffsetHours)],
    "#z" => [fixed(Fixed::OffsetPermissive)],
    "+" => [fixed(Fixed::Rfc3339)],
    "D" => [
        num0(Numeric::Month),
        lit("/"),
        num0(Numeric::Day),
        lit("/"),
        num0(Numeric::YearMod100),
    ],
    "x" => [
        num0(Numeric::Month),
        lit("/"),
        num0(Numeric::Day),
        lit("/"),
        num0(Numeric::YearMod100),
    ],
    "v" => [
        nums(Numeric::Day),
        lit("-"),
        fixed(Fixed::ShortMonthName),
        lit("-"),
        num0(Numeric::Year),
    ],
    "X" => [
        num0(Numeric::Hour),
        lit(":"),
        num0(Numeric::Minute),
        lit(":"),
        num0(Numeric::Second),
    ],
    "r" => [
        num0(Numeric::Hour12),
        lit(":"),
        num0(Numeric::Minute),
        lit(":"),
        num0(Numeric::Second),
        space(" "),
        fixed(Fixed::UpperAmPm),
    ],
    "c" => [
        fixed(Fixed::ShortWeekdayName),
        space(" "),
        fixed(Fixed::ShortMonthName),
        space(" "),
        nums(Numeric::Day),
        space(" "),
        num0(Numeric::Hour),
        lit(":"),
        num0(Numeric::Minute),
        lit(":"),
        num0(Numeric::Second),
        space(" "),
        num0(Numeric::Year),
    ],
    "t" => [space("\t")],
    "n" => [space("\n")],
    "%" => [lit("%")],
];

/// What is wrong with a specifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Problem {
    /// Nothing follows the `%`, or the `%` and its padding sign.
    Unfinished,
    /// What follows the `%` is not in the table.
    Unknown,
    /// A padding sign before a specifier that does not write one number.
    Padding,
}

/// Where a pattern went wrong: the bytes of the specifier and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Refused {
    start: usize,
    end: usize,
    problem: Problem,
}

/// The steps of a pattern, one after the other.
///
/// A specifier that is not one gives [`Item::Error`] and ends the steps
/// there; [`refusal`](Items::refusal) then says what it was.
#[derive(Clone, Debug)]
pub(crate) struct Items<'a> {
    pattern: &'a str,
    /// What is still to be read of the pattern.
    rest: &'a str,
    /// The steps still to give of a specifier that stands for several.
    queue: &'static [Item<'static>],
    refused: Option<Refused>,
}

impl<'a> Items<'a> {
    /// The steps of `pattern`.
    pub(crate) const fn new(pattern: &'a str) -> Items<'a> {
        Items {
            pattern,
            rest: pattern,
            queue: &[],
            refused: None,
        }
    }

    /// What was wrong with the pattern, once [`Item::Error`] has come out.
    pub(crate) fn refusal(&self) -> Option<PatternError> {
        self.refused.map(|refused| PatternError {
            position: refused.start,
            specifier: self.pattern[refused.start..refused.end].to_owned(),
            problem: refused.problem,
        })
    }

    /// The step of the specifier `self.rest` starts with.
    fn specifier(&mut self) -> Item<'a> {
        let start = self.pattern.len() - self.rest.len();
        // After the `%`.
        let mut rest = &self.rest[1..];

        let pad = match rest.as_bytes().first() {
            Some(b'-') => Some(Pad::None),
            Some(b'0') => Some(Pad::Zero),
            Some(b'_') => Some(Pad::Space),
            _ => None,
        };
        if pad.is_some() {
            rest = &rest[1..];
        }
        if rest.is_empty() {
            return self.refuse(start, self.pattern.len(), Problem::Unfinished);
        }

        let Some(specifier) = SPECIFIERS.iter().find(|s| rest.starts_with(s.text)) else {
            // Show the specifier as far as it could be one: through any of
            // the signs a longer specifier is made of, and the character
            // after them.
            let mut shown = 0;
            for c in rest.chars() {
                shown += c.len_utf8();
                if !matches!(c, '.' | ':' | '#' | '0'..='9') {
                    break;
                }
            }
            let end = self.pattern.len() - rest.len() + shown;
            return self.refuse(start, end, Problem::Unknown);
        };
        let end = self.pattern.len() - rest.len() + specifier.text.len();

        let step = match (pad, specifier.steps) {
            (None, [first, more @ ..]) => {
                self.queue = more;
                *first
            }
            // A padding sign is for one number.
            (Some(pad), [Item::Numeric(numeric, _)]) => Item::Numeric(*numeric, pad),
            _ => return self.refuse(start, end, Problem::Padding),
        };
        self.rest = &self.pattern[end..];
        step
    }

    fn refuse(&mut self, start: usize, end: usize, problem: Problem) -> Item<'a> {
        self.refused = Some(Refused {
            start,
            end,
            problem,
        });
        self.rest = "";
        self.queue = &[];
        Item::Error
    }
}

impl<'a> Iterator for Items<'a> {
    type Item = Item<'a>;

    fn next(&mut self) -> Option<Item<'a>> {
        if let Some((step, more)) = self.queue.split_first() {
            self.queue = more;
            return Some(*step);
        }

        let first = self.rest.chars().next()?;
        if first == '%' {
            return Some(self.specifier());
        }

        // A run of white space, or a run of anything else up to white space
        // or a specifier.
        let end = if first.is_whitespace() {
            self.rest.find(|c: char| !c.is_whitespace())
        } else {
            self.rest.find(|c: char| c.is_whitespace() || c == '%')
        }
        .unwrap_or(self.rest.len());
        let (run, rest) = self.rest.split_at(end);
        self.rest = rest;
        Some(if first.is_whitespace() {
            Item::Space(run)
        } else {
            Item::Literal(run)
        })
    }
}

/// Whether `pattern` is a pattern: every `%` in it starts a specifier of
/// [the table](self#the-specifiers).
///
/// It does not say whether a value can be written by it: a day has no hour
/// for `%H`, and [`Formatted`](crate::Formatted) says so when it is asked to
/// write one.
///
/// It keeps nothing and builds nothing, so a macro can call it on a pattern
/// written in the code and refuse a wrong one while the program is
/// compiled.
///
/// ```
/// use navree_datetime::pattern;
///
/// assert!(pattern::check("%d %b %Y, %H:%M").is_ok());
/// assert!(pattern::check("50%% off until %-d %B").is_ok());
///
/// let error = pattern::check("%Y-%Q").unwrap_err();
/// assert_eq!(error.position(), 3);
/// assert_eq!(error.specifier(), "%Q");
///
/// // A `%` with nothing after it, and padding for what is not a number.
/// assert!(pattern::check("100%").is_err());
/// assert!(pattern::check("%-B").is_err());
/// ```
///
/// # Errors
///
/// The first specifier that is not one: a `%` followed by something the
/// table does not have, a `%` at the end of the pattern, or one of the
/// padding signs `-`, `_` and `0` before a specifier that does not write
/// one number.
pub fn check(pattern: &str) -> Result<(), PatternError> {
    let mut items = Items::new(pattern);
    for _step in items.by_ref() {}
    match items.refusal() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// What is wrong with a pattern: which specifier, where, and why.
///
/// ```
/// use navree_datetime::pattern;
///
/// let error = pattern::check("%d %b %Y at %K").unwrap_err();
/// assert_eq!(error.position(), 12);
/// assert_eq!(error.specifier(), "%K");
/// assert_eq!(
///     error.to_string(),
///     "`%K` at byte 12 is not a specifier of a date or time pattern"
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternError {
    position: usize,
    specifier: String,
    problem: Problem,
}

impl PatternError {
    /// Where in the pattern the specifier starts: the place of its `%`,
    /// counted in bytes from 0.
    pub fn position(&self) -> usize {
        self.position
    }

    /// The specifier as the pattern has it, from its `%`.
    pub fn specifier(&self) -> &str {
        &self.specifier
    }
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let PatternError {
            position,
            specifier,
            problem,
        } = self;
        match problem {
            Problem::Unfinished => write!(
                f,
                "the pattern ends after `{specifier}` at byte {position}: a specifier follows `%`, and `%%` is a percent sign"
            ),
            Problem::Unknown => write!(
                f,
                "`{specifier}` at byte {position} is not a specifier of a date or time pattern"
            ),
            Problem::Padding => write!(
                f,
                "`{specifier}` at byte {position} has a padding sign, and `-`, `_` and `0` are only for a specifier that writes one number"
            ),
        }
    }
}

impl std::error::Error for PatternError {}

/// A wrong pattern as this crate's error, of the kind
/// [`Parse`](crate::ErrorKind::Parse): the pattern is text that could not be
/// read.
impl From<PatternError> for Error {
    fn from(error: PatternError) -> Error {
        Error::parse(error.to_string())
    }
}
