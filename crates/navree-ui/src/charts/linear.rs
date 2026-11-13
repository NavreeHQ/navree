//! What the linear charts share: [`Meter`](super::Meter), [`MiniColumns`](super::MiniColumns),
//! [`ColorStrip`](super::ColorStrip) and [`RangeScale`](super::RangeScale) write their
//! words from [`LinearLabels`], split a whole into shares the same way, write a duration the
//! same way and list their parts in the same legend.

use std::borrow::Cow;

use super::format::{NumberFormat, NumberLocale};
use super::paint::Ink;
use super::{Color, fill};
use crate::components::icon;
use crate::node::{Node, el};

/// The most parts a meter or a column is split into: a ninth and later fold into Other.
pub const MAX_PARTS: usize = 12;

/// The words the linear charts write, in English by default. Templates name their blanks
/// in braces.
///
/// ```
/// use navree_ui::charts::{LinearLabels, Meter, Segment};
///
/// let mut words = LinearLabels::english();
/// words.hours = "घं".into();
/// words.minutes = "मि".into();
/// let meter = Meter::new("नींद के चरण")
///     .minutes()
///     .words(words)
///     .segment(Segment::new("गहरी", 83));
/// # let _ = navree_ui::render(&meter.into());
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct LinearLabels {
    /// After the hours of a duration: `h`.
    pub hours: Cow<'static, str>,
    /// After the minutes of a duration: `min`.
    pub minutes: Cow<'static, str>,
    /// A set of columns: `{count}`, `{parts}`, `{name}`, `{total}`.
    pub summary_columns: Cow<'static, str>,
    /// Under columns that are more than are drawn: `{shown}`, `{total}`.
    pub columns_cut: Cow<'static, str>,
    /// A colour strip: `{count}`, `{categories}`, `{name}`, `{share}`.
    pub summary_strip: Cow<'static, str>,
    /// A value on a scale: `{value}`, `{min}`, `{max}`.
    pub summary_scale: Cow<'static, str>,
    /// A value below the scale: `{value}`, `{min}`, `{max}`.
    pub summary_below: Cow<'static, str>,
    /// A value above the scale: `{value}`, `{min}`, `{max}`.
    pub summary_above: Cow<'static, str>,
    /// The value column of a meter or columns whose values are minutes.
    pub duration: Cow<'static, str>,
    /// The column of a meter's segment details, unless the meter names it.
    pub detail: Cow<'static, str>,
    /// A scale's table column for its low end.
    pub minimum: Cow<'static, str>,
    /// A scale's table column for its high end.
    pub maximum: Cow<'static, str>,
}

impl Default for LinearLabels {
    fn default() -> Self {
        Self::english()
    }
}

impl LinearLabels {
    /// The English words.
    pub const fn english() -> Self {
        Self {
            hours: Cow::Borrowed("h"),
            minutes: Cow::Borrowed("min"),
            summary_columns: Cow::Borrowed(
                "{count} columns, each split into {parts}. Largest total: {name}, {total}.",
            ),
            columns_cut: Cow::Borrowed("The first {shown} of {total} columns are drawn."),
            summary_strip: Cow::Borrowed(
                "{count} readings in {categories} categories. Most: {name}, {share}.",
            ),
            summary_scale: Cow::Borrowed("{value} on a scale from {min} to {max}."),
            summary_below: Cow::Borrowed("{value}, below the scale from {min} to {max}."),
            summary_above: Cow::Borrowed("{value}, above the scale from {min} to {max}."),
            duration: Cow::Borrowed("Duration"),
            detail: Cow::Borrowed("Detail"),
            minimum: Cow::Borrowed("Minimum"),
            maximum: Cow::Borrowed("Maximum"),
        }
    }
}

/// How a part's value is written: as a number, or as a duration given in minutes.
#[derive(Clone, Debug)]
pub(crate) enum Unit {
    Number(NumberFormat),
    Minutes,
}

impl Unit {
    pub fn text(&self, value: f64, numbers: &NumberLocale, words: &LinearLabels) -> String {
        match self {
            Self::Number(format) => format.format(value, numbers),
            Self::Minutes => duration(value, numbers, words),
        }
    }
}

/// `minutes` as `45 min`, `1 h 23 min` or `2 h`, to the whole minute.
pub(crate) fn duration(minutes: f64, numbers: &NumberLocale, words: &LinearLabels) -> String {
    if !minutes.is_finite() || minutes < 0.0 {
        return "–".to_owned();
    }
    // Past a million hours a duration is a number; the cap keeps the cast exact.
    let total = minutes.round().min(6.0e10);
    let whole = NumberFormat::integer();
    let (hours, rest) = ((total / 60.0).floor(), total % 60.0);
    match (hours > 0.0, rest > 0.0 || total == 0.0) {
        (false, _) => format!("{} {}", whole.format(rest, numbers), words.minutes),
        (true, false) => format!("{} {}", whole.format(hours, numbers), words.hours),
        (true, true) => format!(
            "{} {} {} {}",
            whole.format(hours, numbers),
            words.hours,
            whole.format(rest, numbers),
            words.minutes
        ),
    }
}

/// The parts of one whole as shares. A share is worked out against the largest part, so
/// parts near the largest number a float holds still add up; only the total shown may be
/// too large.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Shares {
    top: f64,
    scaled: f64,
}

impl Shares {
    /// The shares of the positive values; `None` when no value is positive.
    pub fn of(values: &[Option<f64>]) -> Option<Self> {
        let positive = |v: &Option<f64>| v.filter(|v| *v > 0.0 && v.is_finite());
        let top = values.iter().filter_map(positive).fold(0.0_f64, f64::max);
        if !(top > 0.0 && top.is_finite()) {
            return None;
        }
        let scaled = values.iter().filter_map(positive).map(|v| v / top).sum();
        Some(Self { top, scaled })
    }

    /// The part of the whole that `value` is, 0 to 1; zero for a value that draws nothing.
    pub fn fraction(&self, value: Option<f64>) -> f64 {
        match value.filter(|v| *v > 0.0 && v.is_finite()) {
            Some(v) => ((v / self.top) / self.scaled).clamp(0.0, 1.0),
            None => 0.0,
        }
    }

    /// The whole.
    pub fn total(&self) -> f64 {
        self.top * self.scaled
    }
}

/// One entry of a legend.
pub(crate) struct Entry {
    pub ink: Ink,
    pub name: String,
    pub value: Option<String>,
    /// The row it belongs to, so hovering it marks the mark and the reverse.
    pub index: Option<usize>,
    /// A second line under the name.
    pub detail: Option<String>,
    /// Whether the part holds nothing: its key is hollow.
    pub empty: bool,
}

/// A legend that wraps along a row: a key in the part's colour, its name and, when there
/// is one, its value. A status tone adds its icon, so a colour is never alone.
pub(crate) fn legend_list(entries: Vec<Entry>) -> Node {
    let items: Vec<Node> = entries
        .into_iter()
        .map(|e| {
            let key = e
                .ink
                .on(el("span").slot("chart-key").attr("data-key", "block"))
                .attr("aria-hidden", "true");
            let li = el("li")
                .slot("chart-legend-item")
                .attr_opt("data-i", e.index.map(|i| i as i64))
                .flag("data-empty", e.empty);
            let color: Color = e.ink.color();
            e.ink
                .on(li)
                .children((
                    key,
                    color.icon().map(icon),
                    el("span").slot("chart-legend-name").child(e.name),
                    e.value
                        .map(|v| el("span").slot("chart-legend-value").child(v)),
                    e.detail
                        .map(|d| el("span").slot("chart-legend-detail").child(d)),
                ))
                .into()
        })
        .collect();
    el("ul")
        .slot("chart-legend")
        .attr("data-layout", "inline")
        .child(items)
        .into()
}

/// `template` filled, for the summaries.
pub(crate) fn say(template: &str, values: &[(&str, &str)]) -> String {
    fill(template, values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duration_is_hours_and_minutes_to_the_whole_minute() {
        let en = NumberLocale::english();
        let words = LinearLabels::english();
        assert_eq!(duration(45.0, &en, &words), "45 min");
        assert_eq!(duration(0.0, &en, &words), "0 min");
        assert_eq!(duration(83.0, &en, &words), "1 h 23 min");
        assert_eq!(duration(120.0, &en, &words), "2 h");
        assert_eq!(duration(89.6, &en, &words), "1 h 30 min");
        assert_eq!(duration(-5.0, &en, &words), "–");
        assert_eq!(duration(f64::NAN, &en, &words), "–");
        assert_eq!(duration(f64::INFINITY, &en, &words), "–");
        // Hours group as the locale does.
        let india = NumberLocale::of("en-IN");
        assert_eq!(duration(60.0 * 12_345.0, &india, &words), "12,345 h");
    }

    #[test]
    fn shares_add_up_even_for_the_largest_numbers() {
        let s = Shares::of(&[Some(25.0), Some(75.0), None, Some(-4.0), Some(f64::NAN)]).unwrap();
        assert!((s.fraction(Some(25.0)) - 0.25).abs() < 1e-12);
        assert!((s.fraction(Some(75.0)) - 0.75).abs() < 1e-12);
        assert_eq!(s.fraction(None), 0.0);
        assert_eq!(s.fraction(Some(-4.0)), 0.0);
        assert!((s.total() - 100.0).abs() < 1e-9);
        let big = Shares::of(&[Some(f64::MAX), Some(f64::MAX)]).unwrap();
        assert!((big.fraction(Some(f64::MAX)) - 0.5).abs() < 1e-12);
        assert!(Shares::of(&[Some(0.0), None]).is_none());
        assert!(Shares::of(&[]).is_none());
    }
}
