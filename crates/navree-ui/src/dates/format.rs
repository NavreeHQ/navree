//! Writing dates and times the way a locale writes them.

use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};

use super::locale::{Clock, DateLocale, DateOrder};

/// Fills `{y}`, `{m}`, `{d}`, `{M}` and `{w}` in a pattern. An unknown letter is
/// written as it is.
fn expand(pattern: &str, values: &[(char, &str)]) -> String {
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        if c != '{' {
            out.push(c);
            continue;
        }
        let key = chars.next();
        let close = chars.next();
        match (key, close) {
            (Some(key), Some('}')) => match values.iter().find(|(name, _)| *name == key) {
                Some((_, text)) => out.push_str(text),
                None => {
                    out.push('{');
                    out.push(key);
                    out.push('}');
                }
            },
            (key, close) => {
                out.push('{');
                out.extend(key);
                out.extend(close);
            }
        }
    }
    out
}

impl DateLocale {
    /// The date as it is typed here: `11/03/2026`, `03.11.2026`, `2026-11-03`.
    /// Day and month have two digits and the year four, so the text is the same
    /// width for every date and cannot be misread.
    ///
    /// ```
    /// use navree_ui::dates::DateLocale;
    /// use chrono::NaiveDate;
    ///
    /// let day = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
    /// assert_eq!(DateLocale::of("en").format_date(day), "11/03/2026");
    /// assert_eq!(DateLocale::of("de").format_date(day), "03.11.2026");
    /// assert_eq!(DateLocale::of("sv").format_date(day), "2026-11-03");
    /// ```
    pub fn format_date(&self, date: NaiveDate) -> String {
        let (year, month, day) = (
            format!("{:04}", date.year()),
            format!("{:02}", date.month()),
            format!("{:02}", date.day()),
        );
        let parts: [&str; 3] = match self.order {
            DateOrder::Mdy => [&month, &day, &year],
            DateOrder::Dmy => [&day, &month, &year],
            DateOrder::Ymd => [&year, &month, &day],
        };
        let mut out = parts.join(&self.separator);
        out.push_str(&self.suffix);
        out
    }

    /// The date with its weekday and month name, as a screen reader should say it:
    /// `Tuesday, November 3, 2026`.
    pub fn format_long(&self, date: NaiveDate) -> String {
        expand(
            &self.long_pattern,
            &[
                ('w', self.weekday_name(date.weekday())),
                ('M', self.month_in_date(date.month())),
                ('d', &date.day().to_string()),
                ('m', &date.month().to_string()),
                ('y', &date.year().to_string()),
            ],
        )
    }

    /// The heading of a month: `November 2026`.
    pub fn format_caption(&self, year: i32, month: u32) -> String {
        expand(
            &self.caption_pattern,
            &[
                ('M', self.month_name(month)),
                ('m', &month.to_string()),
                ('y', &year.to_string()),
            ],
        )
    }

    /// A short date: `Nov 3`, or `Nov 3, 2026` with the year.
    pub fn format_short(&self, date: NaiveDate, with_year: bool) -> String {
        let pattern = if with_year {
            &self.short_year_pattern
        } else {
            &self.short_pattern
        };
        expand(
            pattern,
            &[
                ('M', self.month_short(date.month())),
                ('d', &date.day().to_string()),
                ('m', &date.month().to_string()),
                ('y', &date.year().to_string()),
            ],
        )
    }

    /// A date to show in a hint: `11/03/2026` or the like, for a fixed day.
    pub fn example_date(&self) -> String {
        self.format_date(NaiveDate::from_ymd_opt(2026, 11, 30).unwrap_or(NaiveDate::MIN))
    }

    /// A time to show in a hint, for 2:30 in the afternoon.
    pub fn example_time(&self) -> String {
        self.format_time(
            NaiveTime::from_hms_opt(14, 30, 0).unwrap_or(NaiveTime::MIN),
            false,
        )
    }

    /// The time as it is typed here: `14:30`, or `2:30 PM` on a 12 hour clock.
    ///
    /// ```
    /// use navree_ui::dates::DateLocale;
    /// use chrono::NaiveTime;
    ///
    /// let t = NaiveTime::from_hms_opt(0, 5, 0).unwrap();
    /// assert_eq!(DateLocale::of("en").format_time(t, false), "12:05 AM");
    /// assert_eq!(DateLocale::of("de").format_time(t, false), "00:05");
    /// assert_eq!(DateLocale::of("ja").format_time(t, true), "00:05:00");
    /// ```
    pub fn format_time(&self, time: NaiveTime, seconds: bool) -> String {
        let (hour, minute, second) = (time.hour(), time.minute(), time.second());
        let tail = if seconds {
            format!("{minute:02}:{second:02}")
        } else {
            format!("{minute:02}")
        };
        match self.clock {
            Clock::H24 => format!("{hour:02}:{tail}"),
            Clock::H12 => {
                let shown = (hour + 11) % 12 + 1;
                let marker = if hour < 12 { &self.am } else { &self.pm };
                if self.marker_first {
                    format!("{marker}{shown}:{tail}")
                } else {
                    format!("{shown}:{tail}{marker}")
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn typed_dates_follow_the_locale() {
        let d = day(2026, 11, 3);
        for (tag, text) in [
            ("en", "11/03/2026"),
            ("en-GB", "03/11/2026"),
            ("en-CA", "2026-11-03"),
            ("de", "03.11.2026"),
            ("fr", "03/11/2026"),
            ("nl", "03-11-2026"),
            ("sv", "2026-11-03"),
            ("ja", "2026/11/03"),
            ("zh", "2026/11/03"),
            ("ko", "2026. 11. 03."),
            ("ar", "03/11/2026"),
            ("he", "03.11.2026"),
        ] {
            assert_eq!(DateLocale::of(tag).format_date(d), text, "{tag}");
        }
    }

    #[test]
    fn years_before_1000_keep_four_digits() {
        assert_eq!(
            DateLocale::of("de").format_date(day(99, 1, 2)),
            "02.01.0099"
        );
    }

    #[test]
    fn read_out_dates_name_the_weekday_and_month() {
        let d = day(2026, 11, 3);
        assert_eq!(
            DateLocale::of("en").format_long(d),
            "Tuesday, November 3, 2026"
        );
        assert_eq!(
            DateLocale::of("en-GB").format_long(d),
            "Tuesday 3 November 2026"
        );
        assert_eq!(
            DateLocale::of("de").format_long(d),
            "Dienstag, 3. November 2026"
        );
        assert_eq!(
            DateLocale::of("ru").format_long(d),
            "вторник, 3 ноября 2026 г."
        );
        assert_eq!(DateLocale::of("ja").format_long(d), "2026年11月3日火曜日");
        assert_eq!(
            DateLocale::of("pl").format_long(d),
            "wtorek, 3 listopada 2026"
        );
    }

    #[test]
    fn captions_use_the_name_on_its_own() {
        assert_eq!(DateLocale::of("ru").format_caption(2026, 11), "ноябрь 2026");
        assert_eq!(DateLocale::of("ja").format_caption(2026, 11), "2026年11月");
        assert_eq!(
            DateLocale::of("en").format_caption(2026, 2),
            "February 2026"
        );
    }

    #[test]
    fn short_dates_drop_the_year_unless_asked() {
        let d = day(2026, 4, 3);
        let en = DateLocale::of("en");
        assert_eq!(en.format_short(d, false), "Apr 3");
        assert_eq!(en.format_short(d, true), "Apr 3, 2026");
        assert_eq!(DateLocale::of("de").format_short(d, false), "3. Apr.");
    }

    #[test]
    fn times_follow_the_clock() {
        let t = |h, m| NaiveTime::from_hms_opt(h, m, 0).unwrap();
        let en = DateLocale::of("en");
        assert_eq!(en.format_time(t(0, 0), false), "12:00 AM");
        assert_eq!(en.format_time(t(0, 5), false), "12:05 AM");
        assert_eq!(en.format_time(t(9, 30), false), "9:30 AM");
        assert_eq!(en.format_time(t(12, 0), false), "12:00 PM");
        assert_eq!(en.format_time(t(23, 59), false), "11:59 PM");
        assert_eq!(en.format_time(t(13, 0), true), "1:00:00 PM");
        let de = DateLocale::of("de");
        assert_eq!(de.format_time(t(0, 5), false), "00:05");
        assert_eq!(de.format_time(t(23, 59), true), "23:59:00");
        assert_eq!(
            DateLocale::of("ko").format_time(t(15, 5), false),
            "오후 3:05"
        );
        assert_eq!(
            DateLocale::of("en-AU").format_time(t(15, 5), false),
            "3:05 pm"
        );
    }

    #[test]
    fn a_pattern_with_an_unknown_letter_is_kept() {
        assert_eq!(expand("{q} {y} {", &[('y', "2026")]), "{q} 2026 {");
        assert_eq!(expand("{y", &[('y', "2026")]), "{y");
    }
}
