//! Reading dates and times typed by a person.
//!
//! The reader is forgiving about how a date is written and strict about what it
//! is. Any separator works (`/ . - ,` or a space), digits of any script count, direction
//! marks are ignored, the order is the locale's unless the first number has four
//! digits (then it is year, month, day), and a month may be a name. A two-digit
//! year lands within 50 years of today. Nothing is guessed: `3/11` has no year and
//! is refused, as is the 31st of April. The browser module reads text by these
//! same rules, and one list of cases holds the two together.

use std::fmt;

use chrono::{Datelike, NaiveDate, NaiveTime};

use super::locale::{DateLocale, DateOrder};

/// Why text is not a date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DateParseError {
    /// There was nothing to read.
    Empty,
    /// It doesn't look like a date.
    Unreadable,
    /// It has a day and a month but no year.
    MissingYear,
    /// The year has the wrong number of digits, or is year 0.
    BadYear,
    /// The numbers are fine but there is no such day, such as the 31st of April.
    NotADate,
}

impl DateParseError {
    /// The code both sides of the wire use for it.
    pub fn code(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Unreadable => "unreadable",
            Self::MissingYear => "missing_year",
            Self::BadYear => "bad_year",
            Self::NotADate => "not_a_date",
        }
    }
}

impl fmt::Display for DateParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "enter a date",
            Self::Unreadable => "that is not a date",
            Self::MissingYear => "the date needs a year",
            Self::BadYear => "the year needs four digits",
            Self::NotADate => "there is no such day",
        })
    }
}

impl std::error::Error for DateParseError {}

/// Why text is not a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TimeParseError {
    /// There was nothing to read.
    Empty,
    /// It doesn't look like a time.
    Unreadable,
    /// An hour, minute or second is out of range.
    OutOfRange,
}

impl TimeParseError {
    /// The code both sides of the wire use for it.
    pub fn code(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Unreadable => "unreadable",
            Self::OutOfRange => "out_of_range",
        }
    }
}

impl fmt::Display for TimeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "enter a time",
            Self::Unreadable => "that is not a time",
            Self::OutOfRange => "there is no such time",
        })
    }
}

impl std::error::Error for TimeParseError {}

/// The code points of the digit zero in the scripts that have their own digits.
const ZEROS: [u32; 17] = [
    0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6, 0x0D66,
    0x0E50, 0x0ED0, 0x0F20, 0x1040, 0xFF10,
];

/// The value of a decimal digit in any of the common scripts. The browser module
/// holds the same list of zeros.
pub(crate) fn digit(c: char) -> Option<u8> {
    if c.is_ascii_digit() {
        return Some(c as u8 - b'0');
    }
    let code = c as u32;
    if code < ZEROS[0] {
        return None;
    }
    ZEROS
        .iter()
        .find_map(|zero| code.checked_sub(*zero).filter(|d| *d <= 9))
        .and_then(|d| u8::try_from(d).ok())
}

/// Characters that mark a part of a date in Chinese, Japanese and Korean. They
/// separate, and mean nothing more.
fn is_date_marker(c: char) -> bool {
    matches!(c, '年' | '月' | '日' | '년' | '월' | '일')
}

/// Characters that separate parts of a time and are letters.
fn is_time_marker(c: char) -> bool {
    matches!(
        c,
        'h' | 'u' | '時' | '点' | '分' | '秒' | '시' | '분' | '초'
    )
}

/// Letters with accents and the plain letters they are typed as. The browser module
/// holds the same table, so `fevrier` finds `février` on both sides.
const PLAIN: [(&str, char); 22] = [
    ("àáâãäåāăą", 'a'),
    ("çćč", 'c'),
    ("ďđ", 'd'),
    ("èéêëēėęěẽ", 'e'),
    ("ğ", 'g'),
    ("ìíîïīıį", 'i'),
    ("ł", 'l'),
    ("ñńň", 'n'),
    ("òóôõöøōő", 'o'),
    ("ř", 'r'),
    ("śşšß", 's'),
    ("ţť", 't'),
    ("ùúûüūůűų", 'u'),
    ("ýÿ", 'y'),
    ("źżž", 'z'),
    ("æ", 'e'),
    ("œ", 'e'),
    ("þ", 't'),
    ("ð", 'd'),
    ("ħ", 'h'),
    ("ĳ", 'i'),
    ("ŋ", 'n'),
];

fn plain(c: char) -> char {
    PLAIN
        .iter()
        .find(|(accented, _)| accented.contains(c))
        .map_or(c, |(_, plain)| *plain)
}

/// Lower case, accents off the Latin letters, and everything but letters and digits
/// removed: `Sept.` is `sept` and `Février` is `fevrier`.
pub(crate) fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .map(plain)
        .collect()
}

/// Words that sit between the parts of a date and mean nothing: `3rd of November`,
/// `3 de noviembre de 2026`, `2026 г.`.
const CONNECTORS: [&str; 10] = ["de", "del", "of", "the", "st", "nd", "rd", "th", "г", "um"];

enum Token {
    Number(String),
    Word(String),
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut number = String::new();
    let mut word = String::new();
    for c in text.chars() {
        if let Some(value) = digit(c) {
            if !word.is_empty() {
                out.push(Token::Word(std::mem::take(&mut word)));
            }
            number.push(char::from(b'0' + value));
        } else if c.is_alphabetic() && !is_date_marker(c) {
            if !number.is_empty() {
                out.push(Token::Number(std::mem::take(&mut number)));
            }
            word.push(c);
        } else {
            if !number.is_empty() {
                out.push(Token::Number(std::mem::take(&mut number)));
            }
            if !word.is_empty() {
                out.push(Token::Word(std::mem::take(&mut word)));
            }
        }
    }
    if !number.is_empty() {
        out.push(Token::Number(number));
    }
    if !word.is_empty() {
        out.push(Token::Word(word));
    }
    out
}

enum MonthMatch {
    One(u32),
    Ambiguous,
    None,
}

/// The year for digits: four digits as they are, two within 50 years of `today`.
fn year_of(digits: &str, today: NaiveDate) -> Result<i32, DateParseError> {
    let value: i32 = digits.parse().map_err(|_| DateParseError::BadYear)?;
    let year = match digits.len() {
        4 => value,
        2 => {
            let mut year = today.year() / 100 * 100 + value;
            if year > today.year() + 50 {
                year -= 100;
            } else if year <= today.year() - 50 {
                year += 100;
            }
            year
        }
        _ => return Err(DateParseError::BadYear),
    };
    if (1..=9999).contains(&year) {
        Ok(year)
    } else {
        Err(DateParseError::BadYear)
    }
}

fn part(digits: &str) -> Result<u32, DateParseError> {
    if digits.is_empty() || digits.len() > 2 {
        return Err(DateParseError::Unreadable);
    }
    digits.parse().map_err(|_| DateParseError::Unreadable)
}

fn build(year: i32, month: u32, day: u32) -> Result<NaiveDate, DateParseError> {
    NaiveDate::from_ymd_opt(year, month, day).ok_or(DateParseError::NotADate)
}

impl DateLocale {
    fn month_match(&self, word: &str) -> MonthMatch {
        let long_enough = word.chars().count() >= 3;
        let mut found = None;
        for month in 1..=12 {
            let hit = [
                self.month_name(month),
                self.month_in_date(month),
                self.month_short(month),
            ]
            .iter()
            .any(|name| {
                let name = fold(name);
                name == word || (long_enough && name.starts_with(word))
            });
            if hit {
                if found.is_some() {
                    return MonthMatch::Ambiguous;
                }
                found = Some(month);
            }
        }
        found.map_or(MonthMatch::None, MonthMatch::One)
    }

    fn is_weekday_word(&self, word: &str) -> bool {
        self.weekdays
            .iter()
            .chain(self.weekdays_short.iter())
            .any(|name| {
                let name = fold(name);
                name == word || (word.chars().count() >= 3 && name.starts_with(word))
            })
    }

    /// Reads a date typed by a person, by this locale's rules. `today` anchors a
    /// two-digit year.
    ///
    /// ```
    /// use navree_ui::dates::DateLocale;
    /// use chrono::NaiveDate;
    ///
    /// let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
    /// let de = DateLocale::of("de");
    /// assert_eq!(de.parse_date("3.11.2026", today), Ok(today));
    /// assert_eq!(de.parse_date("3. November 26", today), Ok(today));
    /// assert_eq!(de.parse_date("2026-11-03", today), Ok(today));
    /// assert!(de.parse_date("31.4.2026", today).is_err());
    /// ```
    pub fn parse_date(&self, text: &str, today: NaiveDate) -> Result<NaiveDate, DateParseError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(DateParseError::Empty);
        }
        if text.chars().count() > 64 {
            return Err(DateParseError::Unreadable);
        }
        let mut numbers: Vec<String> = Vec::new();
        let mut month = None;
        for token in tokenize(text) {
            match token {
                Token::Number(number) => {
                    if number.len() > 8 || numbers.len() == 3 {
                        return Err(DateParseError::Unreadable);
                    }
                    numbers.push(number);
                }
                Token::Word(word) => {
                    let word = fold(&word);
                    if word.is_empty() {
                        continue;
                    }
                    match self.month_match(&word) {
                        MonthMatch::One(found) => {
                            if month.replace(found).is_some() {
                                return Err(DateParseError::Unreadable);
                            }
                        }
                        MonthMatch::Ambiguous => return Err(DateParseError::Unreadable),
                        MonthMatch::None => {
                            if !CONNECTORS.contains(&word.as_str()) && !self.is_weekday_word(&word)
                            {
                                return Err(DateParseError::Unreadable);
                            }
                        }
                    }
                }
            }
        }
        let numbers: Vec<&str> = numbers.iter().map(String::as_str).collect();
        match (numbers.as_slice(), month) {
            ([a, b, c], None) => self.numeric(a, b, c, today),
            ([a, b], Some(month)) => self.named(a, b, month, today),
            ([only], None) => self.compact(only, today),
            ([_, _], None) => Err(DateParseError::MissingYear),
            _ => Err(DateParseError::Unreadable),
        }
    }

    fn numeric(
        &self,
        a: &str,
        b: &str,
        c: &str,
        today: NaiveDate,
    ) -> Result<NaiveDate, DateParseError> {
        let (year, month, day) = if a.len() == 4 {
            (a, b, c)
        } else {
            match self.order {
                DateOrder::Mdy => (c, a, b),
                DateOrder::Dmy => (c, b, a),
                DateOrder::Ymd => (a, b, c),
            }
        };
        let (month, day) = (part(month)?, part(day)?);
        build(year_of(year, today)?, month, day)
    }

    fn named(
        &self,
        a: &str,
        b: &str,
        month: u32,
        today: NaiveDate,
    ) -> Result<NaiveDate, DateParseError> {
        let (year, day) = match (a.len(), b.len()) {
            (4, 4) => return Err(DateParseError::Unreadable),
            (4, _) => (a, b),
            (_, 4) => (b, a),
            _ if self.order == DateOrder::Ymd => (a, b),
            _ => (b, a),
        };
        build(year_of(year, today)?, month, part(day)?)
    }

    /// Eight digits are `yyyymmdd` when that is a date, else the locale's order
    /// with a four-digit year; six digits are the locale's order with two.
    fn compact(&self, digits: &str, today: NaiveDate) -> Result<NaiveDate, DateParseError> {
        let at = |from: usize, to: usize| digits.get(from..to).unwrap_or_default();
        match digits.len() {
            8 => {
                let iso = year_of(at(0, 4), today)
                    .and_then(|year| build(year, part(at(4, 6))?, part(at(6, 8))?));
                if iso.is_ok() {
                    return iso;
                }
                match self.order {
                    DateOrder::Mdy => {
                        build(year_of(at(4, 8), today)?, part(at(0, 2))?, part(at(2, 4))?)
                    }
                    DateOrder::Dmy => {
                        build(year_of(at(4, 8), today)?, part(at(2, 4))?, part(at(0, 2))?)
                    }
                    DateOrder::Ymd => iso,
                }
            }
            6 => match self.order {
                DateOrder::Mdy => {
                    build(year_of(at(4, 6), today)?, part(at(0, 2))?, part(at(2, 4))?)
                }
                DateOrder::Dmy => {
                    build(year_of(at(4, 6), today)?, part(at(2, 4))?, part(at(0, 2))?)
                }
                DateOrder::Ymd => {
                    build(year_of(at(0, 2), today)?, part(at(2, 4))?, part(at(4, 6))?)
                }
            },
            _ => Err(DateParseError::Unreadable),
        }
    }

    /// Reads a time typed by a person: `9`, `9:30`, `0930`, `9.30pm`, `9h30`,
    /// `21:30:15`. A marker (`am`, `pm`, `a`, `p`, or the locale's own) makes it a
    /// 12 hour time. With no marker the hour is read on the 24 hour clock, in
    /// every locale, so `9` is nine in the morning and `15` is three in the
    /// afternoon; the field then shows the time back in the locale's own form.
    ///
    /// ```
    /// use navree_ui::dates::DateLocale;
    /// use chrono::NaiveTime;
    ///
    /// let en = DateLocale::of("en");
    /// assert_eq!(en.parse_time("9:30 pm").unwrap(), NaiveTime::from_hms_opt(21, 30, 0).unwrap());
    /// assert!(en.parse_time("25:00").is_err());
    /// ```
    pub fn parse_time(&self, text: &str) -> Result<NaiveTime, TimeParseError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(TimeParseError::Empty);
        }
        if text.chars().count() > 32 {
            return Err(TimeParseError::Unreadable);
        }
        let lower = text.to_lowercase().replace("uhr", " ");
        let mut runs: Vec<String> = Vec::new();
        let mut run = String::new();
        let mut letters = String::new();
        for c in lower.chars() {
            if let Some(value) = digit(c) {
                run.push(char::from(b'0' + value));
            } else {
                if !run.is_empty() {
                    runs.push(std::mem::take(&mut run));
                }
                if c.is_alphabetic() && !is_time_marker(c) {
                    letters.push(c);
                }
            }
        }
        if !run.is_empty() {
            runs.push(run);
        }
        let letters = fold(&letters);
        let marker = if letters.is_empty() {
            None
        } else if letters == "a" || letters == "am" || letters == fold(&self.am) {
            Some(false)
        } else if letters == "p" || letters == "pm" || letters == fold(&self.pm) {
            Some(true)
        } else {
            return Err(TimeParseError::Unreadable);
        };
        let split = |text: &str, sizes: &[usize]| -> Vec<String> {
            let mut at = 0;
            sizes
                .iter()
                .map(|size| {
                    let part = text.get(at..at + size).unwrap_or_default().to_owned();
                    at += size;
                    part
                })
                .collect()
        };
        let parts: Vec<String> = match runs.as_slice() {
            [one] => match one.len() {
                1 | 2 => vec![one.clone()],
                3 => split(one, &[1, 2]),
                4 => split(one, &[2, 2]),
                5 => split(one, &[1, 2, 2]),
                6 => split(one, &[2, 2, 2]),
                _ => return Err(TimeParseError::Unreadable),
            },
            [h, m] => vec![h.clone(), m.clone()],
            [h, m, s] => vec![h.clone(), m.clone(), s.clone()],
            _ => return Err(TimeParseError::Unreadable),
        };
        if parts.iter().any(|part| part.is_empty() || part.len() > 2) {
            return Err(TimeParseError::Unreadable);
        }
        let number = |at: usize| -> u32 {
            parts
                .get(at)
                .and_then(|part| part.parse().ok())
                .unwrap_or_default()
        };
        let (mut hour, minute, second) = (number(0), number(1), number(2));
        if let Some(afternoon) = marker {
            if !(1..=12).contains(&hour) {
                return Err(TimeParseError::OutOfRange);
            }
            hour %= 12;
            if afternoon {
                hour += 12;
            }
        }
        NaiveTime::from_hms_opt(hour, minute, second).ok_or(TimeParseError::OutOfRange)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn today() -> NaiveDate {
        day(2026, 11, 3)
    }

    #[test]
    fn digits_of_other_scripts_count() {
        assert_eq!(digit('7'), Some(7));
        assert_eq!(digit('٣'), Some(3));
        assert_eq!(digit('۴'), Some(4));
        assert_eq!(digit('१'), Some(1));
        assert_eq!(digit('１'), Some(1));
        assert_eq!(digit('９'), Some(9));
        assert_eq!(digit('a'), None);
        assert_eq!(digit('é'), None);
    }

    #[test]
    fn the_shared_cases_hold() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/dates-parse.json")).unwrap();
        let today: NaiveDate = cases["today"].as_str().unwrap().parse().unwrap();
        let mut seen = 0;
        for case in cases["dates"].as_array().unwrap() {
            let locale = DateLocale::of(case["locale"].as_str().unwrap());
            let text = case["text"].as_str().unwrap();
            let got = locale.parse_date(text, today);
            match (case.get("ok"), case.get("err")) {
                (Some(ok), _) => {
                    let want: NaiveDate = ok.as_str().unwrap().parse().unwrap();
                    assert_eq!(got, Ok(want), "{} {text:?}", case["locale"]);
                }
                (_, Some(err)) => {
                    assert_eq!(
                        got.map_err(DateParseError::code),
                        Err(err.as_str().unwrap()),
                        "{} {text:?}",
                        case["locale"]
                    );
                }
                _ => panic!("a case needs ok or err: {case}"),
            }
            seen += 1;
        }
        for case in cases["times"].as_array().unwrap() {
            let locale = DateLocale::of(case["locale"].as_str().unwrap());
            let text = case["text"].as_str().unwrap();
            let got = locale.parse_time(text);
            match (case.get("ok"), case.get("err")) {
                (Some(ok), _) => {
                    let want: NaiveTime = ok.as_str().unwrap().parse().unwrap();
                    assert_eq!(got, Ok(want), "{} {text:?}", case["locale"]);
                }
                (_, Some(err)) => {
                    assert_eq!(
                        got.map_err(TimeParseError::code),
                        Err(err.as_str().unwrap()),
                        "{} {text:?}",
                        case["locale"]
                    );
                }
                _ => panic!("a case needs ok or err: {case}"),
            }
            seen += 1;
        }
        assert!(seen >= 100, "{seen} cases");
    }

    #[test]
    fn a_two_digit_year_lands_within_fifty_years() {
        let en = DateLocale::of("en");
        assert_eq!(en.parse_date("1/2/76", today()), Ok(day(2076, 1, 2)));
        assert_eq!(en.parse_date("1/2/77", today()), Ok(day(1977, 1, 2)));
        assert_eq!(en.parse_date("1/2/00", today()), Ok(day(2000, 1, 2)));
        assert_eq!(en.parse_date("1/2/26", today()), Ok(day(2026, 1, 2)));
    }

    #[test]
    fn leap_days_are_checked() {
        let en = DateLocale::of("en");
        assert_eq!(en.parse_date("2/29/2024", today()), Ok(day(2024, 2, 29)));
        assert_eq!(en.parse_date("2/29/2000", today()), Ok(day(2000, 2, 29)));
        assert_eq!(
            en.parse_date("2/29/2100", today()),
            Err(DateParseError::NotADate)
        );
        assert_eq!(
            en.parse_date("2/29/2025", today()),
            Err(DateParseError::NotADate)
        );
    }

    #[test]
    fn hostile_text_is_refused_without_panic() {
        let en = DateLocale::of("en");
        for text in [
            "",
            "   ",
            "\0",
            "<script>alert(1)</script>",
            "99999999999999999999",
            "1/2/3/4",
            "١٢٣٤٥٦٧٨٩٠١٢٣٤٥٦٧٨٩٠",
            &"9".repeat(10_000),
            &"/".repeat(10_000),
            "𝟙/𝟚/𝟛𝟘𝟚𝟞",
            "\u{202e}11/03/2026\u{202c}x",
        ] {
            let _ = en.parse_date(text, today());
            let _ = en.parse_time(text);
        }
    }

    #[test]
    fn direction_marks_are_ignored() {
        let ar = DateLocale::of("ar");
        assert_eq!(
            ar.parse_date("\u{200f}٠٣/١١/٢٠٢٦\u{200f}", today()),
            Ok(day(2026, 11, 3))
        );
    }

    #[test]
    fn times_without_a_marker_read_on_the_24_hour_clock() {
        let en = DateLocale::of("en");
        let t = |h, m, s| NaiveTime::from_hms_opt(h, m, s).unwrap();
        assert_eq!(en.parse_time("9"), Ok(t(9, 0, 0)));
        assert_eq!(en.parse_time("15"), Ok(t(15, 0, 0)));
        assert_eq!(en.parse_time("9:05"), Ok(t(9, 5, 0)));
        assert_eq!(en.parse_time("0930"), Ok(t(9, 30, 0)));
        assert_eq!(en.parse_time("930"), Ok(t(9, 30, 0)));
        assert_eq!(en.parse_time("21h30"), Ok(t(21, 30, 0)));
        assert_eq!(en.parse_time("21:30:15"), Ok(t(21, 30, 15)));
        assert_eq!(en.parse_time("24:00"), Err(TimeParseError::OutOfRange));
        assert_eq!(en.parse_time("9:60"), Err(TimeParseError::OutOfRange));
    }

    #[test]
    fn markers_make_a_12_hour_time() {
        let en = DateLocale::of("en");
        let t = |h, m| NaiveTime::from_hms_opt(h, m, 0).unwrap();
        assert_eq!(en.parse_time("12 am"), Ok(t(0, 0)));
        assert_eq!(en.parse_time("12:30 AM"), Ok(t(0, 30)));
        assert_eq!(en.parse_time("12 pm"), Ok(t(12, 0)));
        assert_eq!(en.parse_time("12:30 p.m."), Ok(t(12, 30)));
        assert_eq!(en.parse_time("9.30pm"), Ok(t(21, 30)));
        assert_eq!(en.parse_time("9p"), Ok(t(21, 0)));
        assert_eq!(en.parse_time("13 pm"), Err(TimeParseError::OutOfRange));
        assert_eq!(en.parse_time("0 am"), Err(TimeParseError::OutOfRange));
        assert_eq!(DateLocale::of("ja").parse_time("午後3:05"), Ok(t(15, 5)));
        assert_eq!(DateLocale::of("ko").parse_time("오전 12:05"), Ok(t(0, 5)));
        assert_eq!(DateLocale::of("ar").parse_time("٣:٣٠ م"), Ok(t(15, 30)));
        assert_eq!(DateLocale::of("de").parse_time("9 Uhr 30"), Ok(t(9, 30)));
    }
}
