//! [`DateLocale`]: what a calendar needs to know about a language and region:
//! the names of months and weekdays, the order and separators of a typed date,
//! the first day of the week, the 12 or 24 hour clock, the text direction and the
//! words on the controls.
//!
//! The data is owned here, so a page needs no ICU and the browser module reads
//! the same table the server drew from: a month's name never differs between the
//! first month a page shows and the next one the browser draws.

use std::borrow::Cow;
use std::ops::Deref;
use std::sync::Arc;

use chrono::Weekday;

use super::data;
use super::range::DatePreset;

/// The order of day, month and year in a typed date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DateOrder {
    /// Month, day, year (`11/03/2026`).
    Mdy,
    /// Day, month, year (`03/11/2026`).
    Dmy,
    /// Year, month, day (`2026-11-03`).
    Ymd,
}

impl DateOrder {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Mdy => "mdy",
            Self::Dmy => "dmy",
            Self::Ymd => "ymd",
        }
    }
}

/// How hours are counted on a clock face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Clock {
    /// 1 to 12 with a day or night marker.
    H12,
    /// 0 to 23.
    H24,
}

/// The words on the controls. English is the default; the built-in locales carry
/// their own. A page may replace any of them with [`DateLocale::labels`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateLabels {
    /// The previous-month button.
    pub prev_month: Cow<'static, str>,
    /// The next-month button.
    pub next_month: Cow<'static, str>,
    /// The button that picks, or goes to, today.
    pub today: Cow<'static, str>,
    /// The button that empties the value.
    pub clear: Cow<'static, str>,
    /// The name of the button that opens the calendar.
    pub choose_date: Cow<'static, str>,
    /// The name of the button that opens the list of times.
    pub choose_time: Cow<'static, str>,
    /// The name of the month list.
    pub month: Cow<'static, str>,
    /// The name of the year list.
    pub year: Cow<'static, str>,
    /// The heading of the week-number column.
    pub week: Cow<'static, str>,
    /// The start of a range.
    pub from: Cow<'static, str>,
    /// The end of a range.
    pub to: Cow<'static, str>,
    /// Applies a draft.
    pub apply: Cow<'static, str>,
    /// Drops a draft.
    pub cancel: Cow<'static, str>,
    /// Shown on an empty date button.
    pub pick_date: Cow<'static, str>,
    /// Shown on an empty range button.
    pub pick_range: Cow<'static, str>,
    /// Shown on an empty time button.
    pub pick_time: Cow<'static, str>,
    /// The message for text that is not a date; `{example}` is a date as typed here.
    pub invalid_date: Cow<'static, str>,
    /// The message for a date that cannot be chosen.
    pub unavailable_date: Cow<'static, str>,
    /// The message for text that is not a time; `{example}` is a time as typed here.
    pub invalid_time: Cow<'static, str>,
    /// The message for a time that cannot be chosen.
    pub unavailable_time: Cow<'static, str>,
    /// The entry of a range picker that sends a person to the calendar.
    pub custom_range: Cow<'static, str>,
    /// The name of the group of presets, for a screen reader.
    pub presets: Cow<'static, str>,
    /// The words on each preset's button, in the order of [`DatePreset::ALL`]; read them
    /// with [`DateLabels::preset`].
    pub preset_names: [Cow<'static, str>; 14],
}

impl DateLabels {
    /// Builds the labels from their texts in field order. After the twenty words the
    /// browser reads come the two of the range picker and the fourteen presets.
    pub(crate) const fn from_array(t: [&'static str; 36]) -> Self {
        let [
            prev_month,
            next_month,
            today,
            clear,
            choose_date,
            choose_time,
            month,
            year,
            week,
            from,
            to,
            apply,
            cancel,
            pick_date,
            pick_range,
            pick_time,
            invalid_date,
            unavailable_date,
            invalid_time,
            unavailable_time,
            custom_range,
            presets,
            p0,
            p1,
            p2,
            p3,
            p4,
            p5,
            p6,
            p7,
            p8,
            p9,
            p10,
            p11,
            p12,
            p13,
        ] = t;
        Self {
            prev_month: Cow::Borrowed(prev_month),
            next_month: Cow::Borrowed(next_month),
            today: Cow::Borrowed(today),
            clear: Cow::Borrowed(clear),
            choose_date: Cow::Borrowed(choose_date),
            choose_time: Cow::Borrowed(choose_time),
            month: Cow::Borrowed(month),
            year: Cow::Borrowed(year),
            week: Cow::Borrowed(week),
            from: Cow::Borrowed(from),
            to: Cow::Borrowed(to),
            apply: Cow::Borrowed(apply),
            cancel: Cow::Borrowed(cancel),
            pick_date: Cow::Borrowed(pick_date),
            pick_range: Cow::Borrowed(pick_range),
            pick_time: Cow::Borrowed(pick_time),
            invalid_date: Cow::Borrowed(invalid_date),
            unavailable_date: Cow::Borrowed(unavailable_date),
            invalid_time: Cow::Borrowed(invalid_time),
            unavailable_time: Cow::Borrowed(unavailable_time),
            custom_range: Cow::Borrowed(custom_range),
            presets: Cow::Borrowed(presets),
            preset_names: [
                Cow::Borrowed(p0),
                Cow::Borrowed(p1),
                Cow::Borrowed(p2),
                Cow::Borrowed(p3),
                Cow::Borrowed(p4),
                Cow::Borrowed(p5),
                Cow::Borrowed(p6),
                Cow::Borrowed(p7),
                Cow::Borrowed(p8),
                Cow::Borrowed(p9),
                Cow::Borrowed(p10),
                Cow::Borrowed(p11),
                Cow::Borrowed(p12),
                Cow::Borrowed(p13),
            ],
        }
    }

    /// The words on a preset's button.
    pub fn preset(&self, preset: DatePreset) -> &str {
        &self.preset_names[preset.index()]
    }

    /// The first twenty labels in field order, as the browser module reads them. The preset
    /// names are drawn by the server and stay out of the page's data.
    pub(crate) fn as_list(&self) -> [&str; 20] {
        [
            &self.prev_month,
            &self.next_month,
            &self.today,
            &self.clear,
            &self.choose_date,
            &self.choose_time,
            &self.month,
            &self.year,
            &self.week,
            &self.from,
            &self.to,
            &self.apply,
            &self.cancel,
            &self.pick_date,
            &self.pick_range,
            &self.pick_time,
            &self.invalid_date,
            &self.unavailable_date,
            &self.invalid_time,
            &self.unavailable_time,
        ]
    }
}

/// Everything about a locale. Build one by hand to teach the calendar a language
/// the built-in table doesn't have, then pass it to [`DateLocale::custom`].
///
/// Weekday lists start on Sunday. Patterns are text with `{y}` (year), `{m}` (month
/// number), `{d}` (day of the month), `{M}` (month name) and `{w}` (weekday name).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct LocaleData {
    /// The tag, such as `de-AT`.
    pub tag: Cow<'static, str>,
    /// Month names on their own, in a caption.
    pub months: [Cow<'static, str>; 12],
    /// Month names inside a date, when they differ (`ноября` against `ноябрь`).
    pub months_in_date: Option<[Cow<'static, str>; 12]>,
    /// Short month names, for a date with a day.
    pub months_short: [Cow<'static, str>; 12],
    /// Weekday names, Sunday first.
    pub weekdays: [Cow<'static, str>; 7],
    /// Short weekday names for the column heads, Sunday first.
    pub weekdays_short: [Cow<'static, str>; 7],
    /// The order of a typed date.
    pub order: DateOrder,
    /// Between the parts of a typed date.
    pub separator: Cow<'static, str>,
    /// After a typed date.
    pub suffix: Cow<'static, str>,
    /// The hint for an empty date field.
    pub placeholder: Cow<'static, str>,
    /// A whole date with its weekday, as read out.
    pub long_pattern: Cow<'static, str>,
    /// The heading of a month.
    pub caption_pattern: Cow<'static, str>,
    /// A date without its year, in short form.
    pub short_pattern: Cow<'static, str>,
    /// A date with its year, in short form.
    pub short_year_pattern: Cow<'static, str>,
    /// The first day of the week.
    pub first_day: Weekday,
    /// The usual clock.
    pub clock: Clock,
    /// The morning marker as written, with any space.
    pub am: Cow<'static, str>,
    /// The afternoon marker as written, with any space.
    pub pm: Cow<'static, str>,
    /// Whether the marker comes before the time.
    pub marker_first: bool,
    /// Whether the language reads right to left.
    pub rtl: bool,
    /// The words on the controls.
    pub labels: DateLabels,
}

#[derive(Clone, Debug)]
enum Repr {
    Static(&'static LocaleData),
    Shared(Arc<LocaleData>),
}

/// A locale for dates and times. Cheap to clone.
///
/// ```
/// use navree_ui::dates::DateLocale;
///
/// let de = DateLocale::of("de-AT");
/// assert_eq!(de.month_name(11), "November");
/// assert_eq!(de.first_day, chrono::Weekday::Mon);
/// // An unknown language falls back to English.
/// assert_eq!(DateLocale::of("xx").month_name(3), "March");
/// ```
#[derive(Clone, Debug)]
pub struct DateLocale {
    repr: Repr,
}

impl Deref for DateLocale {
    type Target = LocaleData;

    fn deref(&self) -> &LocaleData {
        match &self.repr {
            Repr::Static(data) => data,
            Repr::Shared(data) => data,
        }
    }
}

impl Default for DateLocale {
    fn default() -> Self {
        Self::english()
    }
}

impl DateLocale {
    /// American English.
    pub fn english() -> Self {
        Self {
            repr: Repr::Static(&data::EN),
        }
    }

    /// The locale for a tag such as `en`, `de-AT`, `zh-Hans-CN` or `pt_BR`. The
    /// language picks the names; the region sets the week's first day and, for a few
    /// English regions, the usual clock and the order of a date. A tag the table
    /// doesn't have falls back to English.
    pub fn of(tag: &str) -> Self {
        let (language, region) = split_tag(tag);
        let region = region.as_deref();
        let base = data::for_language(&language, region);
        let first_day = region
            .and_then(data::region_first_day)
            .filter(|day| *day != base.first_day);
        let (clock, order, separator) = data::english_region(&language, region);
        let clock = clock.filter(|clock| *clock != base.clock);
        let order = order.filter(|order| *order != base.order);
        if first_day.is_none() && clock.is_none() && order.is_none() {
            return Self {
                repr: Repr::Static(base),
            };
        }
        let mut changed = base.clone();
        changed.tag = Cow::Owned(normalize_tag(&language, region));
        if let Some(day) = first_day {
            changed.first_day = day;
        }
        if let Some(clock) = clock {
            changed.clock = clock;
        }
        if let Some(order) = order {
            changed.order = order;
            changed.placeholder = Cow::Borrowed(match order {
                DateOrder::Mdy => "MM/DD/YYYY",
                DateOrder::Dmy => "DD/MM/YYYY",
                DateOrder::Ymd => "YYYY-MM-DD",
            });
            if let Some(separator) = separator {
                changed.separator = Cow::Borrowed(separator);
            }
        }
        Self {
            repr: Repr::Shared(Arc::new(changed)),
        }
    }

    /// A locale from data a page built itself.
    pub fn custom(data: LocaleData) -> Self {
        Self {
            repr: Repr::Shared(Arc::new(data)),
        }
    }

    /// This locale with the first day of the week changed.
    #[must_use]
    pub fn with_first_day(&self, day: Weekday) -> Self {
        let mut data = (**self).clone();
        data.first_day = day;
        Self::custom(data)
    }

    /// This locale with its clock changed.
    #[must_use]
    pub fn with_clock(&self, clock: Clock) -> Self {
        let mut data = (**self).clone();
        data.clock = clock;
        Self::custom(data)
    }

    /// This locale with other words on the controls.
    #[must_use]
    pub fn with_labels(&self, labels: DateLabels) -> Self {
        let mut data = (**self).clone();
        data.labels = labels;
        Self::custom(data)
    }

    /// The name of month `month`, 1 to 12, on its own. Out-of-range numbers give
    /// an empty name.
    pub fn month_name(&self, month: u32) -> &str {
        month
            .checked_sub(1)
            .and_then(|at| self.months.get(at as usize))
            .map_or("", |name| name)
    }

    /// The name of month `month` as it reads inside a date.
    pub fn month_in_date(&self, month: u32) -> &str {
        let names = self.months_in_date.as_ref().unwrap_or(&self.months);
        month
            .checked_sub(1)
            .and_then(|at| names.get(at as usize))
            .map_or("", |name| name)
    }

    /// The short name of month `month`.
    pub fn month_short(&self, month: u32) -> &str {
        month
            .checked_sub(1)
            .and_then(|at| self.months_short.get(at as usize))
            .map_or("", |name| name)
    }

    /// The name of a weekday.
    pub fn weekday_name(&self, day: Weekday) -> &str {
        &self.weekdays[day.num_days_from_sunday() as usize]
    }

    /// The short name of a weekday.
    pub fn weekday_short(&self, day: Weekday) -> &str {
        &self.weekdays_short[day.num_days_from_sunday() as usize]
    }

    /// The locale as the browser module reads it: the same names, patterns and
    /// words the server drew from.
    pub(crate) fn client(&self) -> serde_json::Value {
        let names = |list: &[Cow<'static, str>]| -> Vec<String> {
            list.iter().map(ToString::to_string).collect()
        };
        let mut out = serde_json::json!({
            "t": &*self.tag,
            "m": names(&self.months),
            "ms": names(&self.months_short),
            "w": names(&self.weekdays),
            "ws": names(&self.weekdays_short),
            "o": self.order.code(),
            "s": &*self.separator,
            "x": &*self.suffix,
            "lp": &*self.long_pattern,
            "cp": &*self.caption_pattern,
            "sp": &*self.short_pattern,
            "yp": &*self.short_year_pattern,
            "c": if self.clock == Clock::H12 { 12 } else { 24 },
            "am": &*self.am,
            "pm": &*self.pm,
            "mf": u8::from(self.marker_first),
            "rtl": u8::from(self.rtl),
            "L": self.labels.as_list(),
        });
        if let Some(in_date) = &self.months_in_date {
            out["md"] = serde_json::json!(names(in_date));
        }
        out
    }
}

/// The language and the region of a tag, lower and upper case. Script and variant
/// subtags are skipped.
fn split_tag(tag: &str) -> (String, Option<String>) {
    let mut parts = tag.split(['-', '_']);
    let mut language = parts.next().unwrap_or_default().to_ascii_lowercase();
    language.retain(|c| c.is_ascii_alphabetic());
    language.truncate(8);
    if language == "iw" {
        language = "he".into();
    }
    let region = parts
        .find(|part| {
            (part.len() == 2 && part.bytes().all(|b| b.is_ascii_alphabetic()))
                || (part.len() == 3 && part.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(str::to_ascii_uppercase);
    (language, region)
}

fn normalize_tag(language: &str, region: Option<&str>) -> String {
    match region {
        Some(region) => format!("{language}-{region}"),
        None => language.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_read_loosely() {
        assert_eq!(split_tag("de_AT"), ("de".into(), Some("AT".into())));
        assert_eq!(split_tag("zh-Hans-CN"), ("zh".into(), Some("CN".into())));
        assert_eq!(split_tag("PT-br"), ("pt".into(), Some("BR".into())));
        assert_eq!(split_tag("iw"), ("he".into(), None));
        assert_eq!(split_tag("en"), ("en".into(), None));
        assert_eq!(split_tag(""), (String::new(), None));
        // Markup in a tag is dropped, never kept.
        assert_eq!(split_tag("<b>-x").0, "b");
    }

    #[test]
    fn the_region_sets_the_first_day() {
        assert_eq!(DateLocale::of("en").first_day, Weekday::Sun);
        assert_eq!(DateLocale::of("en-GB").first_day, Weekday::Mon);
        assert_eq!(DateLocale::of("de").first_day, Weekday::Mon);
        assert_eq!(DateLocale::of("de-AT").first_day, Weekday::Mon);
        assert_eq!(DateLocale::of("es-MX").first_day, Weekday::Sun);
        assert_eq!(DateLocale::of("ar").first_day, Weekday::Sat);
        assert_eq!(DateLocale::of("ar-SA").first_day, Weekday::Sun);
        assert_eq!(DateLocale::of("fr-CA").first_day, Weekday::Sun);
        assert_eq!(DateLocale::of("ja").first_day, Weekday::Sun);
        assert_eq!(DateLocale::of("zh").first_day, Weekday::Mon);
        assert_eq!(DateLocale::of("zh-TW").first_day, Weekday::Sun);
        assert_eq!(DateLocale::of("hi").first_day, Weekday::Sun);
        assert_eq!(DateLocale::of("pt").first_day, Weekday::Sun);
        assert_eq!(DateLocale::of("en-AU").first_day, Weekday::Mon);
        assert_eq!(DateLocale::of("en-AE").first_day, Weekday::Sat);
        assert_eq!(DateLocale::of("xx").first_day, Weekday::Sun);
    }

    #[test]
    fn english_regions_set_clock_and_order() {
        let gb = DateLocale::of("en-GB");
        assert_eq!((gb.order, gb.clock), (DateOrder::Dmy, Clock::H24));
        let au = DateLocale::of("en-AU");
        assert_eq!((au.order, au.clock), (DateOrder::Dmy, Clock::H12));
        let ca = DateLocale::of("en-CA");
        assert_eq!((ca.order, ca.clock), (DateOrder::Ymd, Clock::H12));
        assert_eq!(ca.separator, "-");
        let us = DateLocale::of("en-US");
        assert_eq!((us.order, us.clock), (DateOrder::Mdy, Clock::H12));
    }

    #[test]
    fn rtl_locales_say_so() {
        assert!(DateLocale::of("ar").rtl);
        assert!(DateLocale::of("he").rtl);
        assert!(DateLocale::of("iw").rtl);
        assert!(!DateLocale::of("de").rtl);
    }

    #[test]
    fn every_built_in_locale_is_complete() {
        for locale in data::all() {
            let name = &locale.tag;
            for month in 1..=12 {
                let locale = DateLocale::custom(locale.clone());
                assert!(!locale.month_name(month).is_empty(), "{name} month {month}");
                assert!(
                    !locale.month_short(month).is_empty(),
                    "{name} short {month}"
                );
                assert!(!locale.month_in_date(month).is_empty(), "{name} in {month}");
            }
            for text in locale.weekdays.iter().chain(&locale.weekdays_short) {
                assert!(!text.is_empty(), "{name} weekday");
            }
            for text in locale.labels.as_list() {
                assert!(!text.is_empty(), "{name} label");
            }
            assert!(locale.labels.invalid_date.contains("{example}"), "{name}");
            assert!(locale.labels.invalid_time.contains("{example}"), "{name}");
            assert!(locale.long_pattern.contains("{d}"), "{name}");
            assert!(locale.caption_pattern.contains("{y}"), "{name}");
            assert!(locale.short_pattern.contains("{d}"), "{name}");
            assert!(locale.short_year_pattern.contains("{y}"), "{name}");
            assert!(!locale.placeholder.is_empty(), "{name}");
            assert!(!locale.am.is_empty() && !locale.pm.is_empty(), "{name}");
        }
    }

    #[test]
    fn every_locale_names_each_preset_in_its_own_words() {
        let english = data::all()[0];
        assert_eq!(&*english.tag, "en");
        for preset in super::DatePreset::ALL {
            assert_eq!(english.labels.preset(preset), preset.label());
        }
        assert_eq!(&*english.labels.custom_range, "Custom range");
        for locale in data::all() {
            let name = &locale.tag;
            assert!(
                !locale.labels.custom_range.is_empty(),
                "{name} custom range"
            );
            assert!(!locale.labels.presets.is_empty(), "{name} presets");
            let mut seen = std::collections::HashSet::new();
            for preset in super::DatePreset::ALL {
                let text = locale.labels.preset(preset);
                assert!(!text.is_empty(), "{name} {preset:?}");
                assert!(
                    seen.insert(text.to_owned()),
                    "{name}: two presets read {text:?}"
                );
            }
            if !name.starts_with("en") {
                assert_ne!(
                    locale.labels.preset(super::DatePreset::Today),
                    "Today",
                    "{name} still says Today in English"
                );
                assert_ne!(&*locale.labels.custom_range, "Custom range", "{name}");
            }
        }
    }

    #[test]
    fn out_of_range_months_have_no_name() {
        let en = DateLocale::english();
        assert_eq!(en.month_name(0), "");
        assert_eq!(en.month_name(13), "");
        assert_eq!(en.month_short(0), "");
        assert_eq!(en.month_in_date(99), "");
    }
}
